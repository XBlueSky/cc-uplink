//! Long-lived tmux control-mode transport (`tmux -C attach`).
//!
//! A single [`ControlMode`] owns one `tmux -C attach` child. A background reader
//! task consumes the child's stdout line-by-line, driving a pure [`Framing`] core
//! that turns control-mode lines into [`FramingOut`] actions: pane events, command
//! replies (matched FIFO to in-flight [`ControlMode::run`] callers), pause resumes,
//! and disconnect. Reconnect/backoff supervision is intentionally NOT here — this
//! type is single-connection; on `%exit`/EOF it marks itself disconnected and fails
//! all pending replies. The supervising driver (Task 9) owns reconnection.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, broadcast, oneshot};

use crate::drivers::tmux::protocol::{CmLine, parse_line, quote_cm_arg};
use crate::drivers::tmux::transport::{PaneEvent, TmuxTransport};
use crate::error::{DriverError, ErrorKind};

/// One decoded action produced by feeding a single control-mode line into
/// [`Framing`]. Kept free of any I/O so the framing core is unit-testable
/// without a tmux process.
#[derive(Debug)]
pub enum FramingOut {
    /// `%output` — pane produced bytes.
    Event { pane: String, data: Vec<u8> },
    /// A completed command-reply block matched to an in-flight waiter.
    Reply { ok: bool, body: String },
    /// `%pause` — pane output was paused and must be resumed.
    Pause { pane: String },
    /// `%exit` — the control client is detaching.
    Exited,
}

/// Pure control-mode framing state machine.
///
/// Tracks whether we are inside a `%begin`…`%end`/`%error` reply block and
/// buffers its body lines. A completed block is emitted as [`FramingOut::Reply`]
/// only if a waiter was queued (via [`Framing::push_waiter`]); otherwise it is an
/// unsolicited block (e.g. the attach greeting) and is discarded. This type
/// performs no I/O and is exhaustively unit-tested below.
#[derive(Default)]
pub struct Framing {
    waiters: VecDeque<u64>,
    in_block: bool,
    body: Vec<String>,
}

impl Framing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that a command was sent and its reply block is expected next
    /// (FIFO). The token value is opaque; only ordering matters.
    pub fn push_waiter(&mut self, token: u64) {
        self.waiters.push_back(token);
    }

    /// Advances the state machine by one parsed line, returning zero or more
    /// actions for the transport layer to execute.
    pub fn feed(&mut self, line: CmLine) -> Vec<FramingOut> {
        match line {
            CmLine::Output { pane, data } => vec![FramingOut::Event { pane, data }],
            CmLine::Pause { pane } => vec![FramingOut::Pause { pane }],
            CmLine::Exit => vec![FramingOut::Exited],
            CmLine::Begin { .. } => {
                self.in_block = true;
                self.body.clear();
                vec![]
            }
            CmLine::Body(s) => {
                if self.in_block {
                    self.body.push(s);
                }
                vec![]
            }
            CmLine::End { .. } | CmLine::CmdError { .. } if self.in_block => {
                self.in_block = false;
                let ok = matches!(&line, CmLine::End { .. });
                let body = std::mem::take(&mut self.body).join("\n");
                match self.waiters.pop_front() {
                    // Matched to an in-flight command.
                    Some(_token) => vec![FramingOut::Reply { ok, body }],
                    // Unsolicited block (attach greeting / stray reply): discard.
                    None => vec![],
                }
            }
            _ => vec![],
        }
    }
}

struct CmState {
    framing: Framing,
    pending: VecDeque<oneshot::Sender<(bool, String)>>,
    next_token: u64,
}

/// A single long-lived control-mode connection implementing [`TmuxTransport`].
pub struct ControlMode {
    /// Child stdin. Lock order is always `state` → `stdin`; never the reverse.
    stdin: Mutex<tokio::process::ChildStdin>,
    state: Mutex<CmState>,
    events_tx: broadcast::Sender<PaneEvent>,
    connected: AtomicBool,
    /// Kept alive for the connection's lifetime so `kill_on_drop` reaps tmux only
    /// when this `ControlMode` is dropped (not when `attach` returns).
    _child: Mutex<tokio::process::Child>,
}

type StdoutLines = tokio::io::Lines<BufReader<tokio::process::ChildStdout>>;

impl ControlMode {
    /// Spawns `tmux [-S sock] -C attach -t <session>`, consumes the initial
    /// greeting block, and starts the background reader task. Returns an
    /// `Arc<Self>` implementing [`TmuxTransport`].
    pub async fn attach(socket: Option<String>, session: &str) -> Result<Arc<Self>, DriverError> {
        let mut cmd = tokio::process::Command::new("tmux");
        if let Some(s) = &socket {
            cmd.arg("-S").arg(s);
        }
        cmd.args(["-C", "attach", "-t", session])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| DriverError::new(ErrorKind::Unavailable, format!("spawn tmux -C: {e}")))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut lines = BufReader::new(stdout).lines();

        // tmux -C emits an unsolicited greeting block (`%begin`…`%end`) the instant
        // it attaches. Consume it *before* any waiter can be enqueued, otherwise a
        // near-simultaneous `run()` would FIFO-match its waiter to the greeting's
        // `%end` and receive the empty greeting body instead of its real reply.
        Self::drain_greeting(&mut lines).await?;

        let (events_tx, _) = broadcast::channel(4096);
        let cm = Arc::new(Self {
            stdin: Mutex::new(stdin),
            state: Mutex::new(CmState {
                framing: Framing::new(),
                pending: VecDeque::new(),
                next_token: 1,
            }),
            events_tx,
            connected: AtomicBool::new(true),
            _child: Mutex::new(child),
        });

        // The reader holds only a Weak ref: when the last external Arc is dropped,
        // `_child` drops with it, `kill_on_drop` reaps tmux, stdout hits EOF, and the
        // reader exits — no strong reference cycle keeps the connection alive.
        let weak = Arc::downgrade(&cm);
        tokio::spawn(async move {
            let mut lines = lines;
            while let Ok(Some(line)) = lines.next_line().await {
                match weak.upgrade() {
                    Some(cm) => cm.on_line(&line).await,
                    None => return,
                }
            }
            if let Some(cm) = weak.upgrade() {
                cm.on_disconnect().await;
            }
        });
        Ok(cm)
    }

    /// Reads and discards lines up to and including the first complete reply block
    /// (the attach greeting). A short timeout guards the pathological case of a
    /// server that never sends one; EOF before any block means attach failed.
    async fn drain_greeting(lines: &mut StdoutLines) -> Result<(), DriverError> {
        let mut in_block = false;
        let scan = async {
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_line(&line) {
                    CmLine::Begin { .. } => in_block = true,
                    CmLine::End { .. } | CmLine::CmdError { .. } if in_block => return true,
                    _ => {}
                }
            }
            false
        };
        match tokio::time::timeout(Duration::from_secs(5), scan).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(DriverError::new(
                ErrorKind::Unavailable,
                "tmux -C exited before control-mode greeting",
            )),
            // No greeting within the window: proceed defensively rather than hang.
            Err(_) => Ok(()),
        }
    }

    /// Processes one control-mode line from the reader task. Reply matching (pop
    /// waiter + pop pending sender) happens under a single `state` critical
    /// section so it stays atomic and FIFO; async stdin writes (pause resume) run
    /// after the lock is released, preserving the `state` → `stdin` lock order.
    async fn on_line(&self, raw: &str) {
        let mut pause_lines: Vec<String> = Vec::new();
        {
            let mut st = self.state.lock().await;
            for out in st.framing.feed(parse_line(raw)) {
                match out {
                    FramingOut::Event { pane, data } => {
                        let _ = self.events_tx.send(PaneEvent { pane, data });
                    }
                    FramingOut::Reply { ok, body } => {
                        if let Some(tx) = st.pending.pop_front() {
                            let _ = tx.send((ok, body));
                        }
                    }
                    FramingOut::Pause { pane } => {
                        pause_lines.push(format!(
                            "refresh-client -A {}\n",
                            quote_cm_arg(&format!("{pane}:continue"))
                        ));
                    }
                    FramingOut::Exited => self.connected.store(false, Ordering::SeqCst),
                }
            }
        }
        for line in pause_lines {
            let _ = self.stdin.lock().await.write_all(line.as_bytes()).await;
        }
    }

    /// Marks the connection dead and fails every in-flight `run()` by dropping its
    /// reply sender (the caller then sees `Unavailable`).
    async fn on_disconnect(&self) {
        self.connected.store(false, Ordering::SeqCst);
        self.state.lock().await.pending.clear();
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TmuxTransport for ControlMode {
    async fn run(&self, args: &[String]) -> Result<String, DriverError> {
        if !self.is_connected() {
            return Err(DriverError::new(
                ErrorKind::Unavailable,
                "control-mode disconnected",
            ));
        }
        let (tx, rx) = oneshot::channel();
        let line = args
            .iter()
            .map(|a| quote_cm_arg(a))
            .collect::<Vec<_>>()
            .join(" ")
            + "\n";
        {
            // Enqueue the waiter+sender and write the command while holding `state`,
            // then acquire `stdin` nested inside it. This guarantees waiter-push
            // order equals stdin-write order, so replies stay FIFO-aligned even under
            // concurrent `run()` calls. Lock order is always state → stdin (no cycle).
            let mut st = self.state.lock().await;
            let token = st.next_token;
            st.next_token += 1;
            st.framing.push_waiter(token);
            st.pending.push_back(tx);
            self.stdin
                .lock()
                .await
                .write_all(line.as_bytes())
                .await
                .map_err(|e| DriverError::new(ErrorKind::Unavailable, format!("cm write: {e}")))?;
        }
        let (ok, body) = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| DriverError::new(ErrorKind::Timeout, "cm reply timed out"))?
            .map_err(|_| DriverError::new(ErrorKind::Unavailable, "cm reply dropped"))?;
        if ok {
            Ok(body)
        } else {
            Err(DriverError::new(ErrorKind::Upstream, body))
        }
    }

    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>> {
        Some(self.events_tx.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::tmux::protocol::parse_line;

    fn feed_all(f: &mut Framing, lines: &[&str]) -> Vec<FramingOut> {
        lines.iter().flat_map(|l| f.feed(parse_line(l))).collect()
    }

    #[test]
    fn greeting_block_without_waiter_is_discarded() {
        let mut f = Framing::new();
        let out = feed_all(&mut f, &["%begin 1 0 1", "%end 1 0 1"]);
        assert!(out.iter().all(|o| !matches!(o, FramingOut::Reply { .. })));
    }

    #[test]
    fn command_reply_is_matched_fifo_with_body() {
        let mut f = Framing::new();
        f.push_waiter(7);
        let out = feed_all(&mut f, &["%begin 1 7 1", "line-a", "line-b", "%end 1 7 1"]);
        match &out[..] {
            [FramingOut::Reply { ok: true, body, .. }] => assert_eq!(body, "line-a\nline-b"),
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn error_block_reports_not_ok_and_output_passes_through() {
        let mut f = Framing::new();
        f.push_waiter(9);
        let out = feed_all(
            &mut f,
            &["%output %3 abc", "%begin 1 9 1", "bad", "%error 1 9 1"],
        );
        assert!(matches!(out[0], FramingOut::Event { .. }));
        assert!(matches!(out[1], FramingOut::Reply { ok: false, .. }));
    }
}
