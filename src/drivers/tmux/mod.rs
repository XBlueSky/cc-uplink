pub mod control;
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
use crate::drivers::tmux::transport::{OneShotCli, OwnCtx, TmuxTransport, own_context};
use crate::error::{DriverError, ErrorKind};

pub const PANE_FMT: &str = "#{pane_id}|#{session_name}:#{window_index}.#{pane_index}|#{pane_current_command}|#{@name}|#{pane_current_path}";

pub fn parse_pane_line(line: &str) -> Option<ChannelEntry> {
    let mut it = line.splitn(5, '|');
    let (pane, sw, proc_, label, cwd) =
        (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    Some(ChannelEntry {
        channel: format!("tmux:{pane}"),
        labels: if label.is_empty() {
            vec![]
        } else {
            vec![label.to_string()]
        },
        detail: serde_json::json!({ "sw": sw, "process": proc_, "cwd": cwd }),
    })
}

pub struct TmuxDriver {
    cfg: TmuxCfg,
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

impl TmuxDriver {
    pub async fn new(cfg: TmuxCfg) -> Result<Arc<Self>, DriverError> {
        let cli = OneShotCli::from_env();
        let own = own_context(&cli).await?;
        let cm = ControlMode::attach(cli.socket.clone(), &own.session)
            .await
            .ok();
        let d = Arc::new(Self {
            cfg,
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

    fn check_allowlist(&self, pane: &str, addr: &str) -> Result<(), DriverError> {
        if let Some(list) = &self.cfg.allowlist {
            if !list.iter().any(|x| x == pane || x == addr) {
                return Err(DriverError::new(
                    ErrorKind::Rejected,
                    format!("target '{addr}' not in allowlist"),
                ));
            }
        }
        Ok(())
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
                    return Err(DriverError::new(
                        ErrorKind::Timeout,
                        "pane did not become idle",
                    ));
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

    /// Polling-based idle detection: waits until `#{history_size}:#{cursor_x},#{cursor_y}`
    /// is stable for `quiet_ms`, or returns a timeout error at `deadline`. This is the
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
                return Err(DriverError::new(
                    ErrorKind::Timeout,
                    "pane did not become idle",
                ));
            }
            let probe = self
                .run(&[
                    "display-message".into(),
                    "-p".into(),
                    "-t".into(),
                    pane.into(),
                    "#{history_size}:#{cursor_x},#{cursor_y}".into(),
                ])
                .await?;
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

        let h0 = self.history_size(pane).await?;
        let receipt = Driver::send(
            self,
            addr,
            SendRequest {
                message: message.to_string(),
                reply_hint: ReplyHint::Full,
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
        Ok(out.lines().filter_map(parse_pane_line).collect())
    }

    fn ops(&self) -> Vec<OpSpec> {
        vec![
            OpSpec {
                op: "read".into(),
                summary: "capture last N lines of a pane".into(),
                params_schema: serde_json::json!({"type":"object","properties":{"lines":{"type":"integer","default":50}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            },
            OpSpec {
                op: "keys".into(),
                summary: "send special keys (Enter, Escape, C-c); requires read within 60s".into(),
                params_schema: serde_json::json!({"type":"object","required":["keys"],"properties":{"keys":{"type":"array","items":{"type":"string"}}}}),
                result_schema: serde_json::json!({"type":"object"}),
            },
            OpSpec {
                op: "label".into(),
                summary: "set pane @name label".into(),
                params_schema: serde_json::json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}),
                result_schema: serde_json::json!({"type":"object"}),
            },
            OpSpec {
                op: "await_idle".into(),
                summary: "wait until pane output is quiet".into(),
                params_schema: serde_json::json!({"type":"object","properties":{"quiet_ms":{"type":"integer","default":1500},"timeout_ms":{"type":"integer","default":60000}}}),
                result_schema: serde_json::json!({"type":"object","properties":{"idle":{"type":"boolean"}}}),
            },
            OpSpec {
                op: "ask".into(),
                summary: "send + await_idle + capture transcript delta".into(),
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
        self.check_allowlist(&pane, addr)?;
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
                self.check_allowlist(&pane, addr)?;
                let mark = self.read_marks.lock().await.get(&pane).copied();
                if !guard_ok(mark, Duration::from_secs(60)) {
                    return Err(DriverError::new(
                        ErrorKind::Rejected,
                        "read guard: pane not read recently",
                    )
                    .with_hint("invoke read on this pane first"));
                }
                let keys = args.get("keys").and_then(|v| v.as_array()).ok_or_else(|| {
                    DriverError::new(ErrorKind::Invalid, "keys requires 'keys' array")
                })?;
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
        let e = parse_pane_line("%3|main:0.1|node|codex|/home/t/proj").unwrap();
        assert_eq!(e.channel, "tmux:%3");
        assert_eq!(e.labels, vec!["codex".to_string()]);
        assert_eq!(e.detail["process"], "node");
    }

    #[test]
    fn empty_label_gives_no_labels() {
        let e = parse_pane_line("%0|main:0.0|zsh||/home/t").unwrap();
        assert!(e.labels.is_empty());
    }

    #[test]
    fn now_rfc3339_shape() {
        let s = now_rfc3339();
        assert!(s.ends_with('Z'));
        assert_eq!(s.len(), 20);
    }
}
