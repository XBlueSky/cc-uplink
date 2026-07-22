use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Clone)]
pub struct PaneEvent {
    pub pane: String,
    pub data: Vec<u8>,
}

#[async_trait]
pub trait TmuxTransport: Send + Sync {
    async fn run(&self, args: &[String]) -> Result<String, DriverError>;
    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>>;
}

pub struct OneShotCli {
    pub socket: Option<String>,
}

impl OneShotCli {
    pub fn from_env() -> Self {
        let socket = std::env::var("TMUX")
            .ok()
            .and_then(|v| v.split(',').next().map(str::to_string))
            .filter(|p| std::path::Path::new(p).exists());
        Self { socket }
    }
    fn base_args(&self) -> Vec<String> {
        match &self.socket {
            Some(s) => vec!["-S".into(), s.clone()],
            None => vec![],
        }
    }
}

#[async_trait]
impl TmuxTransport for OneShotCli {
    async fn run(&self, args: &[String]) -> Result<String, DriverError> {
        let mut cmd = tokio::process::Command::new("tmux");
        cmd.args(self.base_args()).args(args).kill_on_drop(true);
        let out = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.output())
            .await
            .map_err(|_| DriverError::new(ErrorKind::Timeout, "tmux command timed out"))?
            .map_err(|e| {
                DriverError::new(ErrorKind::Unavailable, format!("tmux not runnable: {e}"))
            })?;
        if !out.status.success() {
            return Err(DriverError::new(
                ErrorKind::Upstream,
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>> {
        None
    }
}

pub struct OwnCtx {
    pub pane: String,
    pub session: String,
    pub label: Option<String>,
}

pub async fn own_context(t: &dyn TmuxTransport) -> Result<OwnCtx, DriverError> {
    let pane = std::env::var("TMUX_PANE").map_err(|_| {
        DriverError::new(
            ErrorKind::Unavailable,
            "$TMUX_PANE is unset (not inside tmux)",
        )
    })?;
    let out = t
        .run(&[
            "display-message".into(),
            "-p".into(),
            "-t".into(),
            pane.clone(),
            "#{session_name}|#{@name}".into(),
        ])
        .await?;
    let line = out.trim();
    let (session, label) = line.split_once('|').unwrap_or((line, ""));
    Ok(OwnCtx {
        pane,
        session: session.to_string(),
        label: if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        },
    })
}
