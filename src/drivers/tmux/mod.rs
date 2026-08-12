pub mod control;
pub mod policy;
pub mod protocol;
pub mod transport;

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::config::TmuxCfg;
use crate::core::driver::*;
use crate::drivers::tmux::control::ControlMode;
use crate::drivers::tmux::policy::{KeyClass, PaneMarks, PolicyCache, Tier};
use crate::drivers::tmux::transport::{OneShotCli, OwnCtx, TmuxTransport, own_context};
use crate::error::{DriverError, ErrorKind};

pub const PANE_FMT: &str = "#{pane_id}|#{session_name}:#{window_index}.#{pane_index}|#{pane_current_command}|#{@name}|#{@uplink_profile}|#{@uplink_read}|#{pane_current_path}";

pub fn parse_pane_line(line: &str, cfg: &crate::config::TmuxCfg) -> Option<ChannelEntry> {
    let mut it = line.splitn(7, '|');
    let (pane, sw, proc_, label, profile, read, cwd) = (
        it.next()?, it.next()?, it.next()?, it.next()?, it.next()?, it.next()?, it.next()?,
    );
    let marks = PaneMarks {
        label: (!label.is_empty()).then(|| label.to_string()),
        profile: Tier::parse(profile),
        read_off: read == "off",
    };
    Some(ChannelEntry {
        channel: format!("tmux:{pane}"),
        labels: marks.label.clone().into_iter().collect(),
        detail: serde_json::json!({
            "sw": sw,
            "process": proc_,
            "cwd": cwd,
            "profile": policy::effective_tier(&marks, cfg).as_str(),
            "readable": policy::read_block(&marks, cfg).is_none(),
        }),
    })
}

pub struct TmuxDriver {
    policy: PolicyCache,
    cli: OneShotCli,
    cm: Mutex<Option<Arc<ControlMode>>>,
    pub own: OwnCtx,
    read_marks: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    inbox: Mutex<VecDeque<RecvItem>>,
    next_cursor: AtomicU64,
    sink: crate::core::logsink::LogSink,
}

/// Line-buffers ANSI-stripped inbound bytes, emitting only complete
/// (`\n`/`\r`-terminated) non-empty lines. Pure — no I/O, unit-tested below.
pub struct LineBuffer {
    buf: String,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineBuffer {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.buf.push_str(&protocol::strip_ansi(data));
        let mut out = vec![];
        while let Some(i) = self.buf.find(['\n', '\r']) {
            let line: String = self.buf.drain(..=i).collect();
            let line = line.trim_end_matches(['\n', '\r']).to_string();
            if !line.is_empty() {
                out.push(line);
            }
        }
        out
    }
}

/// Pure guard check: is `mark` present and younger than `ttl`?
pub fn guard_ok(mark: Option<std::time::Instant>, ttl: Duration) -> bool {
    mark.map(|t| t.elapsed() <= ttl).unwrap_or(false)
}

/// Single-line semantics, same rail as `send`: control characters (incl. \n,
/// \t) are rejected — multi-line input is multiple `type` calls, control keys
/// go through the `keys` op.
pub fn validate_type_text(text: &str) -> Result<(), DriverError> {
    if text.chars().any(|c| c.is_control()) {
        return Err(DriverError::new(
            ErrorKind::Invalid,
            "text contains control characters",
        )
        .with_hint("multi-line: multiple type calls; special keys: the keys op"));
    }
    Ok(())
}

/// The error both idle-wait paths return when a pane never goes quiet.
///
/// Carries its own triage, because this is a routine outcome rather than a
/// malfunction: an agent peer blocked on its own command-permission prompt
/// animates that prompt, and animation is indistinguishable from work.
fn idle_timeout() -> DriverError {
    DriverError::new(ErrorKind::Timeout, "pane did not become idle").with_hint(
        "peer is still working (raise timeout_ms) or is blocked waiting for its \
         own operator — invoke the read op on this pane to see which",
    )
}

impl TmuxDriver {
    pub async fn new(cfg: TmuxCfg) -> Result<Arc<Self>, DriverError> {
        let cli = OneShotCli::from_env();
        let own = own_context(&cli).await?;
        let cm = ControlMode::attach(cli.socket.clone(), &own.session)
            .await
            .ok();
        let d = Arc::new(Self {
            policy: PolicyCache::new(cfg),
            cli,
            cm: Mutex::new(cm),
            own,
            read_marks: Mutex::new(std::collections::HashMap::new()),
            inbox: Mutex::new(VecDeque::new()),
            next_cursor: AtomicU64::new(0),
            sink: crate::core::logsink::LogSink::open(),
        });
        // best-effort defaults
        let _ = d
            .run(&[
                "set-option".into(),
                "-g".into(),
                "history-limit".into(),
                "100000".into(),
            ])
            .await;
        let _ = d
            .run(&[
                "set-option".into(),
                "-g".into(),
                "mouse".into(),
                "on".into(),
            ])
            .await;

        // Inbound-envelope watcher: line-buffers own-pane %output and feeds
        // complete lines to `parse_inbound`, appending hits to the in-memory
        // ring buffer (drained by `recv`) and the JSONL conversation log.
        //
        // Spawned unconditionally (even when no control-mode receiver exists
        // yet at construction time) and structured as a resilient supervisor:
        // an outer loop re-subscribes via `driver.events().await` whenever the
        // inner drain loop ends — whether because the current `ControlMode`'s
        // broadcast sender closed (e.g. `run()` lazily re-attached and
        // replaced `self.cm`, dropping the old sender) or because no
        // control-mode connection is up yet at all. Without this, a CM
        // re-attach silently kills inbound `channel_recv` forever while
        // `channel_doctor` keeps reporting the new CM as healthy.
        //
        // Holds only a Weak ref across the outer loop (mirroring
        // `ControlMode::attach`'s reader task): a strong clone held for the
        // life of the task would create a reference cycle — this driver owns
        // the `ControlMode` whose broadcast sender keeps `rx` alive, so a
        // strong-held task would never observe channel closure and the
        // driver (and its child `tmux -C` process) could never be dropped
        // even once every external `Arc<TmuxDriver>` is released. Each outer
        // iteration upgrades only transiently — just long enough to check the
        // driver is still alive and to call `events()` — and drops the
        // strong ref before draining (which can block indefinitely on
        // `rx.recv()`) or sleeping, so the task never pins the driver.
        {
            let weak = Arc::downgrade(&d);
            let own_pane = d.own.pane.clone();
            tokio::spawn(async move {
                let mut lb = LineBuffer::new();
                loop {
                    let Some(driver) = weak.upgrade() else {
                        // Driver dropped: nothing left to watch for.
                        break;
                    };
                    let rx = driver.events().await;
                    drop(driver);
                    match rx {
                        Some(mut rx) => loop {
                            match rx.recv().await {
                                Ok(ev) if ev.pane == own_pane => {
                                    let Some(dd) = weak.upgrade() else {
                                        break;
                                    };
                                    for line in lb.push(&ev.data) {
                                        if let Some(inb) =
                                            crate::core::envelope::parse_inbound(&line)
                                        {
                                            let cursor =
                                                dd.next_cursor.fetch_add(1, Ordering::SeqCst);
                                            let item = RecvItem {
                                                cursor,
                                                at: now_rfc3339(),
                                                from: inb.from.clone(),
                                                id: inb.id.clone(),
                                                raw: line.clone(),
                                            };
                                            dd.sink.append(&serde_json::json!({
                                                "ts": item.at, "dir": "in", "from": item.from,
                                                "id": item.id, "raw": item.raw
                                            }));
                                            let mut q = dd.inbox.lock().await;
                                            if q.len() >= 1000 {
                                                q.pop_front();
                                            }
                                            q.push_back(item);
                                        }
                                    }
                                }
                                Ok(_) => {}
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    continue;
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            }
                        },
                        None => {
                            // CM currently down: short backoff, then re-check.
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            });
        }
        Ok(d)
    }

    /// Run through CM when connected; fall back to CLI; re-attach lazily.
    pub async fn run(&self, args: &[String]) -> Result<String, DriverError> {
        {
            let guard = self.cm.lock().await;
            if let Some(cm) = guard.as_ref() {
                if cm.is_connected() {
                    return cm.run(args).await;
                }
            }
        }
        // try one re-attach, else CLI fallback
        if let Ok(new_cm) = ControlMode::attach(self.cli.socket.clone(), &self.own.session).await {
            let mut guard = self.cm.lock().await;
            *guard = Some(new_cm.clone());
            return new_cm.run(args).await;
        }
        self.cli.run(args).await
    }

    pub async fn events(&self) -> Option<tokio::sync::broadcast::Receiver<transport::PaneEvent>> {
        self.cm
            .lock()
            .await
            .as_ref()
            .filter(|c| c.is_connected())
            .and_then(|c| c.events())
    }

    pub async fn resolve(&self, addr: &str) -> Result<String, DriverError> {
        if addr.starts_with('%') {
            return Ok(addr.to_string());
        }
        let out = self
            .run(&[
                "list-panes".into(),
                "-a".into(),
                "-F".into(),
                "#{pane_id} #{@name}".into(),
            ])
            .await?;
        for line in out.lines() {
            if let Some((pane, label)) = line.split_once(' ') {
                if label.trim() == addr {
                    return Ok(pane.to_string());
                }
            }
        }
        Err(
            DriverError::new(ErrorKind::NotFound, format!("no pane labeled '{addr}'"))
                .with_hint("run channel_list()"),
        )
    }

    async fn op_read(&self, pane: &str, lines: u32) -> Result<serde_json::Value, DriverError> {
        self.check_read(pane).await?;
        let out = self
            .run(&[
                "capture-pane".into(),
                "-t".into(),
                pane.into(),
                "-p".into(),
                "-J".into(),
                "-S".into(),
                format!("-{lines}"),
            ])
            .await?;
        self.read_marks
            .lock()
            .await
            .insert(pane.to_string(), std::time::Instant::now());
        Ok(serde_json::json!({ "text": out }))
    }

    /// Read (never write) the pane's grant marks.
    async fn pane_marks(&self, pane: &str) -> Result<PaneMarks, DriverError> {
        let raw = self
            .run(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{@name}|#{@uplink_profile}|#{@uplink_read}".into(),
            ])
            .await?;
        Ok(policy::parse_marks(&raw))
    }

    /// Layer-1 write gate: effective tier must cover `needed`. Returns the
    /// marks so callers can reuse them (label for escalation checks, etc.).
    async fn check_write(
        &self,
        pane: &str,
        needed: Tier,
        what: &str,
    ) -> Result<PaneMarks, DriverError> {
        let marks = self.pane_marks(pane).await?;
        let cfg = self.policy.current();
        policy::require(policy::effective_tier(&marks, &cfg), needed, what)?;
        Ok(marks)
    }

    /// Content gate: ops that return pane content (read, ask) are Forbidden on
    /// read-blocked panes — at every tier.
    async fn check_read(&self, pane: &str) -> Result<PaneMarks, DriverError> {
        let marks = self.pane_marks(pane).await?;
        let cfg = self.policy.current();
        match policy::read_block(&marks, &cfg) {
            None => Ok(marks),
            Some(policy::ReadBlock::PaneOption) => Err(DriverError::new(
                ErrorKind::Rejected,
                "pane is read-blocked (@uplink_read off)",
            )
            .with_hint("a human can lift it: `tmux set -pu @uplink_read` on that pane")),
            Some(policy::ReadBlock::ConfigGlob(g)) => Err(DriverError::new(
                ErrorKind::Rejected,
                format!("pane is read-blocked by config read_deny glob '{g}'"),
            )
            .with_hint("edit read_deny in config.toml (hot-reloaded)")),
        }
    }

    async fn target_session(&self, pane: &str) -> Result<String, DriverError> {
        Ok(self
            .run(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{session_name}".into(),
            ])
            .await?
            .trim()
            .to_string())
    }

    async fn verify_token(
        &self,
        pane: &str,
        token: &str,
        mut rx: Option<tokio::sync::broadcast::Receiver<transport::PaneEvent>>,
    ) -> Option<String> {
        if let Some(rx) = rx.as_mut() {
            let mut acc: Vec<u8> = vec![];
            let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
            while tokio::time::Instant::now() < deadline {
                match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                    Ok(Ok(ev)) if ev.pane == pane => {
                        acc.extend_from_slice(&ev.data);
                        let clean = protocol::strip_ansi(&acc);
                        if clean.contains(token) {
                            return Some(token.to_string());
                        }
                    }
                    Ok(Ok(_)) => {}
                    _ => {}
                }
            }
        }
        // capture fallback
        tokio::time::sleep(Duration::from_millis(300)).await;
        let cap = self
            .run(&[
                "capture-pane".into(),
                "-t".into(),
                pane.into(),
                "-p".into(),
                "-J".into(),
                "-S".into(),
                "-5".into(),
            ])
            .await
            .ok()?;
        cap.contains(token).then(|| token.to_string())
    }

    async fn history_size(&self, pane: &str) -> Result<u64, DriverError> {
        let out = self
            .run(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{history_size}".into(),
            ])
            .await?;
        out.trim()
            .parse()
            .map_err(|_| DriverError::new(ErrorKind::Upstream, "bad history_size"))
    }

    async fn pane_height(&self, pane: &str) -> Result<u64, DriverError> {
        let out = self
            .run(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{pane_height}".into(),
            ])
            .await?;
        out.trim()
            .parse()
            .map_err(|_| DriverError::new(ErrorKind::Upstream, "bad pane_height"))
    }

    async fn op_await_idle(
        &self,
        pane: &str,
        quiet_ms: u64,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, DriverError> {
        let start = tokio::time::Instant::now();
        let deadline = start + Duration::from_millis(timeout_ms);
        let same = self.target_session(pane).await? == self.own.session;
        let mut rx = if same { self.events().await } else { None };

        if let Some(rx) = rx.as_mut() {
            let mut last = tokio::time::Instant::now();
            loop {
                if tokio::time::Instant::now() > deadline {
                    return Err(idle_timeout());
                }
                match tokio::time::timeout(Duration::from_millis(quiet_ms), rx.recv()).await {
                    Ok(Ok(ev)) if ev.pane == pane => {
                        last = tokio::time::Instant::now();
                    }
                    Ok(Ok(_)) => {
                        if last.elapsed() >= Duration::from_millis(quiet_ms) {
                            break;
                        }
                    }
                    // Timeout elapsed with no event at all for quiet_ms: genuine idle signal.
                    Err(_) => {
                        if last.elapsed() >= Duration::from_millis(quiet_ms) {
                            break;
                        }
                    }
                    // Receiver fell behind and the broadcast channel dropped events; some of
                    // those dropped events may have belonged to the target pane. Treat this
                    // as activity (not idle) so a transcript-capture op waits longer rather
                    // than risking a truncated capture.
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        last = tokio::time::Instant::now();
                    }
                    // All senders are gone; recv() would busy-spin forever on this receiver.
                    // Stop using the event path and finish via the polling fallback, honoring
                    // the same quiet_ms/deadline.
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                        return self.poll_idle(pane, quiet_ms, deadline).await;
                    }
                }
            }
        } else {
            return self.poll_idle(pane, quiet_ms, deadline).await;
        }
        Ok(serde_json::json!({ "idle": true, "waited_ms": start.elapsed().as_millis() as u64 }))
    }

    /// What "this pane changed" means on the polling path: pane metadata plus the
    /// visible screen.
    ///
    /// Metadata alone (`#{history_size}` and the cursor position) is blind to a
    /// TUI that animates by rewriting the same screen region with the cursor
    /// parked in its input box — which is exactly how Claude Code renders its
    /// spinner, so a busy peer probed as byte-identical and read as idle.
    /// Including the screen makes any redraw count as activity; a genuinely idle
    /// pane renders a constant screen and still settles. `#{history_size}` stays
    /// in the probe to catch output that scrolled past between two polls.
    async fn activity_probe(&self, pane: &str) -> Result<String, DriverError> {
        let meta = self
            .run(&[
                "display-message".into(),
                "-p".into(),
                "-t".into(),
                pane.into(),
                "#{history_size}:#{cursor_x},#{cursor_y}".into(),
            ])
            .await?;
        let screen = self
            .run(&["capture-pane".into(), "-t".into(), pane.into(), "-p".into()])
            .await?;
        Ok(format!("{}\n{screen}", meta.trim()))
    }

    /// Polling-based idle detection: waits until [`Self::activity_probe`] is stable
    /// for `quiet_ms`, or returns a timeout error at `deadline`. This is the
    /// fallback used when no event stream is available (cross-session target) and when
    /// an event receiver's broadcast channel has closed mid-wait.
    async fn poll_idle(
        &self,
        pane: &str,
        quiet_ms: u64,
        deadline: tokio::time::Instant,
    ) -> Result<serde_json::Value, DriverError> {
        let start = tokio::time::Instant::now();
        let mut last_probe = String::new();
        let mut stable_since = tokio::time::Instant::now();
        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(idle_timeout());
            }
            let probe = self.activity_probe(pane).await?;
            if probe == last_probe {
                if stable_since.elapsed() >= Duration::from_millis(quiet_ms) {
                    break;
                }
            } else {
                last_probe = probe;
                stable_since = tokio::time::Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        Ok(serde_json::json!({ "idle": true, "waited_ms": start.elapsed().as_millis() as u64 }))
    }

    async fn op_ask(
        &self,
        addr: &str,
        pane: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "ask requires 'message'"))?;
        let quiet_ms = args
            .get("quiet_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(1500);
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        self.check_read(pane).await?;
        let h0 = self.history_size(pane).await?;
        // No reply hint: `ask` captures the peer's transcript itself, so asking
        // it to send the answer back is pure overhead — and against a TUI peer
        // it is harmful, because shelling out to `tmux send-keys` stalls the peer
        // at a command-permission prompt and the round-trip never completes.
        let receipt = Driver::send(
            self,
            addr,
            SendRequest {
                message: message.to_string(),
                reply_hint: ReplyHint::None,
            },
        )
        .await?;
        self.op_await_idle(pane, quiet_ms, timeout_ms).await?;

        let h1 = self.history_size(pane).await?;
        let height = self.pane_height(pane).await?;
        let back = (h1.saturating_sub(h0)) + height;
        let cap = self
            .run(&[
                "capture-pane".into(),
                "-t".into(),
                pane.into(),
                "-p".into(),
                "-J".into(),
                "-S".into(),
                format!("-{back}"),
            ])
            .await?;
        let token = format!("id:{}", receipt.correlation_id);
        let transcript = match cap.find(&token) {
            Some(pos) => {
                let line_start = cap[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
                cap[line_start..].to_string()
            }
            None => cap,
        };
        self.read_marks
            .lock()
            .await
            .insert(pane.to_string(), std::time::Instant::now());
        Ok(serde_json::json!({ "transcript": transcript, "receipt": receipt }))
    }

    /// Raw console injection: literal text, optional Enter, then return.
    /// Deliberately fire-and-forget — no echo verification, no idle wait —
    /// because raw consoles (nc, telnet, password prompts) break every
    /// assumption send's verify loop makes. Each tmux command still has the
    /// transport's hard 10 s timeout, so nothing here can hang. Confirmation
    /// is the caller's job: read the pane afterwards.
    async fn op_type(
        &self,
        pane: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "type requires 'text'"))?;
        let enter = args.get("enter").and_then(|v| v.as_bool()).unwrap_or(false);
        let sensitive = args
            .get("sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        validate_type_text(text)?;
        if pane == self.own.pane {
            return Err(DriverError::new(
                ErrorKind::Rejected,
                "cannot type into own pane",
            ));
        }
        self.check_write(pane, Tier::Operator, "type").await?;
        let mark = self.read_marks.lock().await.get(pane).copied();
        if !guard_ok(mark, Duration::from_secs(60)) {
            return Err(DriverError::new(
                ErrorKind::Rejected,
                "read guard: pane not read recently",
            )
            .with_hint("invoke read on this pane first — look, then type"));
        }
        self.run(&[
            "send-keys".into(),
            "-t".into(),
            pane.into(),
            "-l".into(),
            "--".into(),
            text.into(),
        ])
        .await?;
        if enter {
            self.run(&["send-keys".into(), "-t".into(), pane.into(), "Enter".into()])
                .await?;
        }
        self.sink.append(&serde_json::json!({
            "ts": now_rfc3339(),
            "dir": "out",
            "channel": format!("tmux:{pane}"),
            "op": "type",
            "excerpt": if sensitive { "[redacted]" } else { text },
            "len": text.chars().count(),
            "enter": enter,
        }));
        Ok(serde_json::json!({ "typed": text.chars().count(), "enter": enter }))
    }
}

pub use crate::core::now_rfc3339;

#[async_trait]
impl Driver for TmuxDriver {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: "tmux".into(),
            kind: DriverKind::Both,
            summary: "cross-pane messaging and pane ops via tmux".into(),
        }
    }

    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError> {
        let out = self
            .run(&[
                "list-panes".into(),
                "-a".into(),
                "-F".into(),
                PANE_FMT.into(),
            ])
            .await?;
        let cfg = self.policy.current();
        Ok(out.lines().filter_map(|l| parse_pane_line(l, &cfg)).collect())
    }

    fn ops(&self) -> Vec<OpSpec> {
        vec![
            OpSpec {
                op: "read".into(),
                summary: "capture last N lines of a pane".into(),
                mutating: false,
                params_schema: serde_json::json!({"type":"object","properties":{"lines":{"type":"integer","default":50}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            },
            OpSpec {
                op: "keys".into(),
                summary: "send special keys (Enter, Escape, C-c); requires read within 60s".into(),
                mutating: true,
                params_schema: serde_json::json!({"type":"object","required":["keys"],"properties":{"keys":{"type":"array","items":{"type":"string"}}}}),
                result_schema: serde_json::json!({"type":"object"}),
            },
            OpSpec {
                op: "type".into(),
                summary: "type literal text into a raw console (fire-and-forget; read afterwards to confirm)".into(),
                mutating: true,
                params_schema: serde_json::json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"},"enter":{"type":"boolean","default":false},"sensitive":{"type":"boolean","default":false}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"typed":{"type":"integer"},"enter":{"type":"boolean"}}}),
            },
            OpSpec {
                op: "label".into(),
                summary: "set pane @name label".into(),
                mutating: true,
                params_schema: serde_json::json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}),
                result_schema: serde_json::json!({"type":"object"}),
            },
            OpSpec {
                op: "await_idle".into(),
                summary: "wait until pane output is quiet".into(),
                mutating: false,
                params_schema: serde_json::json!({"type":"object","properties":{"quiet_ms":{"type":"integer","default":1500},"timeout_ms":{"type":"integer","default":60000}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"idle":{"type":"boolean"}}}),
            },
            OpSpec {
                op: "ask".into(),
                summary: "send + await_idle + capture transcript delta".into(),
                mutating: true,
                params_schema: serde_json::json!({"type":"object","required":["message"],"properties":{"message":{"type":"string"},"quiet_ms":{"type":"integer","default":1500},"timeout_ms":{"type":"integer","default":120000}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"transcript":{"type":"string"},"receipt":{"type":"object"}}}),
            },
        ]
    }

    async fn send(&self, addr: &str, msg: SendRequest) -> Result<SendReceipt, DriverError> {
        let pane = self.resolve(addr).await?;
        if pane == self.own.pane {
            return Err(DriverError::new(
                ErrorKind::Rejected,
                "cannot send to own pane (loop prevention)",
            ));
        }
        self.check_write(&pane, Tier::Operator, "send").await?;
        if msg.message.chars().any(|c| c.is_control()) {
            return Err(DriverError::new(
                ErrorKind::Invalid,
                "message contains control characters",
            )
            .with_hint("single-line messages only in v1"));
        }

        let id = crate::core::envelope::new_correlation_id();
        let from = self
            .own
            .label
            .clone()
            .unwrap_or_else(|| self.own.pane.clone());
        let text = crate::core::envelope::format_outbound(
            &from,
            &self.own.pane,
            &id,
            &msg.message,
            msg.reply_hint,
        );
        let token = format!("id:{id}");

        let same_session = self.target_session(&pane).await? == self.own.session;
        let rx = if same_session {
            self.events().await
        } else {
            None
        };

        self.run(&[
            "send-keys".into(),
            "-t".into(),
            pane.clone(),
            "-l".into(),
            "--".into(),
            text.clone(),
        ])
        .await?;

        let verified = self.verify_token(&pane, &token, rx).await;
        match verified {
            Some(excerpt) => {
                self.run(&[
                    "send-keys".into(),
                    "-t".into(),
                    pane.clone(),
                    "Enter".into(),
                ])
                .await?;
                let receipt = SendReceipt {
                    delivered: true,
                    correlation_id: id,
                    verify_excerpt: Some(excerpt),
                    injected_at: now_rfc3339(),
                };
                self.sink.append(&serde_json::json!({
                    "ts": receipt.injected_at,
                    "dir": "out",
                    "channel": format!("tmux:{addr}"),
                    "id": receipt.correlation_id,
                    "excerpt": &msg.message,
                }));
                Ok(receipt)
            }
            None => {
                let cap = self
                    .run(&[
                        "capture-pane".into(),
                        "-t".into(),
                        pane,
                        "-p".into(),
                        "-S".into(),
                        "-5".into(),
                    ])
                    .await
                    .unwrap_or_default();
                let tail: String = cap
                    .chars()
                    .rev()
                    .take(200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                Err(
                    DriverError::new(ErrorKind::Timeout, "could not verify injected text")
                        .with_evidence(tail)
                        .with_hint("target TUI may have consumed input; inspect with read op"),
                )
            }
        }
    }

    async fn invoke(
        &self,
        addr: &str,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        let pane = self.resolve(addr).await?;
        match op {
            "read" => {
                let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
                self.op_read(&pane, lines).await
            }
            "label" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "label requires 'name'"))?;
                // Own pane: self-naming is identity, not an attack surface —
                // ungated (write gates would make first-run labeling impossible).
                if pane != self.own.pane {
                    let marks = self.check_write(&pane, Tier::Operator, "label").await?;
                    let cfg = self.policy.current();
                    let eff = policy::effective_tier(&marks, &cfg);
                    if policy::label_escalates(name, eff, &cfg) {
                        return Err(DriverError::new(
                            ErrorKind::Rejected,
                            format!("renaming to '{name}' would raise this pane's config-granted tier"),
                        )
                        .with_hint("rename-as-escalation is blocked; ask the human to set the label"));
                    }
                }
                self.run(&[
                    "set-option".into(),
                    "-p".into(),
                    "-t".into(),
                    pane.clone(),
                    "@name".into(),
                    name.into(),
                ])
                .await?;
                Ok(serde_json::json!({ "labeled": pane, "name": name }))
            }
            "keys" => {
                if pane == self.own.pane {
                    return Err(DriverError::new(
                        ErrorKind::Rejected,
                        "cannot send keys to own pane",
                    ));
                }
                let keys = args.get("keys").and_then(|v| v.as_array()).ok_or_else(|| {
                    DriverError::new(ErrorKind::Invalid, "keys requires 'keys' array")
                })?;
                let mut needed = Tier::Operator;
                for k in keys {
                    let k = k.as_str().ok_or_else(|| {
                        DriverError::new(ErrorKind::Invalid, "keys must be strings")
                    })?;
                    if policy::classify_key(k)? == KeyClass::Dangerous {
                        needed = Tier::Godmode;
                    }
                }
                let what = if needed == Tier::Godmode {
                    "keys (dangerous chord)"
                } else {
                    "keys"
                };
                self.check_write(&pane, needed, what).await?;
                let mark = self.read_marks.lock().await.get(&pane).copied();
                if !guard_ok(mark, Duration::from_secs(60)) {
                    return Err(DriverError::new(
                        ErrorKind::Rejected,
                        "read guard: pane not read recently",
                    )
                    .with_hint("invoke read on this pane first"));
                }
                for k in keys {
                    let k = k.as_str().ok_or_else(|| {
                        DriverError::new(ErrorKind::Invalid, "keys must be strings")
                    })?;
                    self.run(&[
                        "send-keys".into(),
                        "-t".into(),
                        pane.clone(),
                        "--".into(),
                        k.into(),
                    ])
                    .await?;
                }
                Ok(serde_json::json!({ "sent": keys.len() }))
            }
            "type" => self.op_type(&pane, &args).await,
            "await_idle" => {
                let quiet_ms = args
                    .get("quiet_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1500);
                let timeout_ms = args
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60_000);
                self.op_await_idle(&pane, quiet_ms, timeout_ms).await
            }
            "ask" => self.op_ask(addr, &pane, &args).await,
            other => Err(
                DriverError::new(ErrorKind::NotFound, format!("no op '{other}'"))
                    .with_hint("run channel_describe(\"tmux:*\")"),
            ),
        }
    }

    async fn recv(&self, cursor: Option<u64>) -> Result<RecvBatch, DriverError> {
        let q = self.inbox.lock().await;
        let from = cursor.unwrap_or(0);
        let items: Vec<RecvItem> = q.iter().filter(|i| i.cursor >= from).cloned().collect();
        let next_cursor = items.last().map(|i| i.cursor + 1).unwrap_or(from);
        Ok(RecvBatch { items, next_cursor })
    }

    async fn doctor(&self) -> DoctorReport {
        let mut lines = vec![];
        let mut ok = true;
        match self
            .run(&["display-message".into(), "-p".into(), "#{version}".into()])
            .await
        {
            Ok(v) => lines.push(format!("tmux version:  {}", v.trim())),
            Err(e) => {
                ok = false;
                lines.push(format!("tmux:          UNREACHABLE ({})", e.message));
            }
        }
        let cm_up = self
            .cm
            .lock()
            .await
            .as_ref()
            .map(|c| c.is_connected())
            .unwrap_or(false);
        lines.push(format!(
            "transport:     {}",
            if cm_up {
                "control-mode"
            } else {
                "cli-fallback"
            }
        ));
        lines.push(format!(
            "own pane:      {} (session {})",
            self.own.pane, self.own.session
        ));
        DoctorReport {
            driver: "tmux".into(),
            ok,
            lines,
        }
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn guard_accepts_fresh_rejects_stale_and_missing() {
        assert!(guard_ok(Some(Instant::now()), Duration::from_secs(60)));
        assert!(!guard_ok(
            Some(Instant::now() - Duration::from_secs(61)),
            Duration::from_secs(60)
        ));
        assert!(!guard_ok(None, Duration::from_secs(60)));
    }
}

#[cfg(test)]
mod recv_tests {
    use super::*;

    #[test]
    fn line_buffer_emits_complete_lines_only() {
        let mut lb = LineBuffer::new();
        assert!(lb.push(b"[reply id:ab12cd34] par").is_empty());
        let lines = lb.push(b"tial answer\nnext");
        assert_eq!(
            lines,
            vec!["[reply id:ab12cd34] partial answer".to_string()]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_line() {
        let cfg = crate::config::TmuxCfg::default();
        let e = parse_pane_line("%3|main:0.1|node|codex|operator||/home/t/proj", &cfg).unwrap();
        assert_eq!(e.channel, "tmux:%3");
        assert_eq!(e.labels, vec!["codex".to_string()]);
        assert_eq!(e.detail["process"], "node");
        assert_eq!(e.detail["profile"], "operator");
        assert_eq!(e.detail["readable"], true);
    }

    #[test]
    fn empty_label_gives_no_labels_and_observer() {
        let cfg = crate::config::TmuxCfg::default();
        let e = parse_pane_line("%0|main:0.0|zsh|||off|/home/t", &cfg).unwrap();
        assert!(e.labels.is_empty());
        assert_eq!(e.detail["profile"], "observer");
        assert_eq!(e.detail["readable"], false); // @uplink_read off
    }

    #[test]
    fn now_rfc3339_shape() {
        let s = now_rfc3339();
        assert!(s.ends_with('Z'));
        assert_eq!(s.len(), 20);
    }

    #[test]
    fn idle_timeout_names_what_to_check() {
        let e = idle_timeout();
        assert!(matches!(e.kind, ErrorKind::Timeout));
        // A peer blocked at its own permission prompt animates that prompt, and
        // animation is activity — so this timeout is a routine outcome, not a
        // malfunction, and it has to say what to look at.
        let rendered = e.render("tmux");
        assert!(
            rendered.contains("read"),
            "an idle timeout must point at the read op: {rendered}"
        );
    }
}

#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn type_text_rejects_control_chars() {
        assert!(validate_type_text("ls -la /tmp").is_ok());
        assert!(validate_type_text("Enter").is_ok()); // literal word, not the key
        let e = validate_type_text("do\nthing").unwrap_err();
        assert!(matches!(e.kind, ErrorKind::Invalid));
        assert!(validate_type_text("tab\there").is_err());
    }
}
