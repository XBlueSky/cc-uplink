pub mod control;
pub mod protocol;
pub mod transport;

use std::sync::Arc;
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
}

/// Hand-rolled RFC3339 UTC timestamp (seconds precision) from `SystemTime`,
/// avoiding a `chrono` dependency for a single call site. Uses the standard
/// civil-calendar algorithm (Howard Hinnant's `civil_from_days`) to turn
/// days-since-epoch into y/m/d.
pub fn now_rfc3339() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let (days, rem) = (secs / 86400, secs % 86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d_ = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d_:02}T{h:02}:{m:02}:{s:02}Z")
}

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
                Ok(SendReceipt {
                    delivered: true,
                    correlation_id: id,
                    verify_excerpt: Some(excerpt),
                    injected_at: now_rfc3339(),
                })
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
                    self.run(&["send-keys".into(), "-t".into(), pane.clone(), k.into()])
                        .await?;
                }
                Ok(serde_json::json!({ "sent": keys.len() }))
            }
            "await_idle" | "ask" => {
                Err(DriverError::new(ErrorKind::Invalid, "not yet implemented"))
            }
            other => Err(
                DriverError::new(ErrorKind::NotFound, format!("no op '{other}'"))
                    .with_hint("run channel_describe(\"tmux:*\")"),
            ),
        }
    }

    async fn recv(&self, _cursor: Option<u64>) -> Result<RecvBatch, DriverError> {
        Ok(RecvBatch {
            items: vec![],
            next_cursor: 0,
        })
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
