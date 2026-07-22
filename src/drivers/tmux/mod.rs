pub mod control;
pub mod protocol;
pub mod transport;

use std::sync::Arc;

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
    // Reserved for allowlist/enabled enforcement, landing in a later task; not
    // read yet, so silence the field-never-read lint rather than drop it.
    #[allow(dead_code)]
    cfg: TmuxCfg,
    cli: OneShotCli,
    cm: Mutex<Option<Arc<ControlMode>>>,
    pub own: OwnCtx,
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
        Ok(serde_json::json!({ "text": out }))
    }
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

    async fn send(&self, _addr: &str, _msg: SendRequest) -> Result<SendReceipt, DriverError> {
        Err(DriverError::new(ErrorKind::Invalid, "not yet implemented"))
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
            "keys" | "await_idle" | "ask" => {
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
}
