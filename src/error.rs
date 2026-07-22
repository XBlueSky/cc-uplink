use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    NotFound,
    Unavailable,
    Rejected,
    Timeout,
    Upstream,
    Invalid,
}

#[derive(Debug, Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct DriverError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: Option<String>,
    pub evidence: Option<String>,
}

impl DriverError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: None,
            evidence: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn with_evidence(mut self, ev: impl Into<String>) -> Self {
        self.evidence = Some(ev.into());
        self
    }
    pub fn render(&self, driver_id: &str) -> String {
        let base = format!(
            "uplink error [{}:{:?}]: {}",
            driver_id, self.kind, self.message
        );
        match &self.hint {
            Some(h) => format!("{base} — hint: {h}"),
            None => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_with_hint() {
        let e = DriverError::new(ErrorKind::NotFound, "no pane labeled 'codex'")
            .with_hint("run channel_list()");
        assert_eq!(
            e.render("tmux"),
            "uplink error [tmux:NotFound]: no pane labeled 'codex' — hint: run channel_list()"
        );
    }

    #[test]
    fn render_without_hint() {
        let e = DriverError::new(ErrorKind::Timeout, "verify timed out");
        assert_eq!(
            e.render("tmux"),
            "uplink error [tmux:Timeout]: verify timed out"
        );
    }
}
