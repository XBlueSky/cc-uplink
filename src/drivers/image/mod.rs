//! Composite `image` driver: routes `image:<backend>` addresses to internal
//! [`ImageBackend`]s (openai, codex). One registry driver — adding a backend
//! never adds an MCP tool, and never touches `Registry` routing.

pub mod codex;
pub mod openai;

use async_trait::async_trait;

use crate::core::driver::{
    ChannelEntry, DoctorReport, Driver, DriverInfo, DriverKind, OpSpec, RecvBatch, SendReceipt,
    SendRequest,
};
use crate::error::{DriverError, ErrorKind};

/// First `max_chars` characters (char-boundary safe; for evidence heads).
pub(crate) fn clip(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Last `max_chars` characters (char-boundary safe; for evidence tails).
pub(crate) fn clip_tail(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(max_chars)).collect()
}

#[async_trait]
pub(crate) trait ImageBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn detail(&self) -> serde_json::Value;
    fn ops(&self) -> Vec<OpSpec>;
    async fn invoke(
        &self,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError>;
    /// (ok, human diagnostic lines) — lines are prefixed with the backend
    /// name by the driver, so return them bare.
    async fn doctor_lines(&self) -> (bool, Vec<String>);
}

pub struct ImageDriver {
    backends: Vec<Box<dyn ImageBackend>>,
}

impl ImageDriver {
    pub(crate) fn from_backends(backends: Vec<Box<dyn ImageBackend>>) -> Self {
        Self { backends }
    }

    fn backend(&self, addr: &str) -> Result<&dyn ImageBackend, DriverError> {
        self.backends
            .iter()
            .find(|b| b.name() == addr)
            .map(|b| b.as_ref())
            .ok_or_else(|| {
                let avail: Vec<&str> = self.backends.iter().map(|b| b.name()).collect();
                DriverError::new(ErrorKind::NotFound, format!("no image backend '{addr}'"))
                    .with_hint(format!(
                        "available: {} — run channel_list()",
                        avail.join(", ")
                    ))
            })
    }
}

#[async_trait]
impl Driver for ImageDriver {
    fn info(&self) -> DriverInfo {
        DriverInfo {
            id: "image".into(),
            kind: DriverKind::Capability,
            summary: "image generation/editing (OpenAI Images API, Codex CLI)".into(),
        }
    }

    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError> {
        Ok(self
            .backends
            .iter()
            .map(|b| ChannelEntry {
                channel: format!("image:{}", b.name()),
                labels: vec![],
                detail: b.detail(),
            })
            .collect())
    }

    fn ops(&self) -> Vec<OpSpec> {
        self.backends.iter().flat_map(|b| b.ops()).collect()
    }

    async fn send(&self, _addr: &str, _msg: SendRequest) -> Result<SendReceipt, DriverError> {
        Err(
            DriverError::new(ErrorKind::Rejected, "image channels do not accept messages")
                .with_hint("use channel_act with op 'generate' or 'edit'"),
        )
    }

    async fn invoke(
        &self,
        addr: &str,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        self.backend(addr)?.invoke(op, args).await
    }

    async fn recv(&self, cursor: Option<u64>) -> Result<RecvBatch, DriverError> {
        Ok(RecvBatch {
            items: vec![],
            next_cursor: cursor.unwrap_or(0),
        })
    }

    async fn doctor(&self) -> DoctorReport {
        let mut ok = true;
        let mut lines = vec![];
        if self.backends.is_empty() {
            ok = false;
            lines.push("no image backends enabled".to_string());
        }
        for b in &self.backends {
            let (bok, blines) = b.doctor_lines().await;
            ok &= bok;
            lines.extend(blines.into_iter().map(|l| format!("{}: {}", b.name(), l)));
        }
        DoctorReport {
            driver: "image".into(),
            ok,
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend {
        ok: bool,
    }

    #[async_trait]
    impl ImageBackend for MockBackend {
        fn name(&self) -> &'static str {
            "mockai"
        }
        fn detail(&self) -> serde_json::Value {
            serde_json::json!({"model": "m"})
        }
        fn ops(&self) -> Vec<OpSpec> {
            vec![OpSpec {
                op: "generate".into(),
                summary: "[mockai] gen".into(),
                mutating: true,
                params_schema: serde_json::json!({}),
                result_schema: serde_json::json!({}),
            }]
        }
        async fn invoke(
            &self,
            op: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, DriverError> {
            Ok(serde_json::json!({"op": op, "args": args}))
        }
        async fn doctor_lines(&self) -> (bool, Vec<String>) {
            (self.ok, vec!["line-a".into(), "line-b".into()])
        }
    }

    fn driver(oks: &[bool]) -> ImageDriver {
        ImageDriver::from_backends(
            oks.iter()
                .map(|&ok| Box::new(MockBackend { ok }) as Box<dyn ImageBackend>)
                .collect(),
        )
    }

    #[tokio::test]
    async fn routes_invoke_by_address() {
        let d = driver(&[true]);
        let out = d
            .invoke("mockai", "generate", serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert_eq!(out["op"], "generate");
        assert_eq!(out["args"]["x"], 1);
    }

    #[tokio::test]
    async fn unknown_backend_is_not_found_with_available_hint() {
        let d = driver(&[true]);
        let e = d
            .invoke("nope", "generate", serde_json::json!({}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::NotFound));
        assert!(e.hint.unwrap().contains("mockai"));
    }

    #[tokio::test]
    async fn send_is_rejected_with_invoke_hint() {
        let d = driver(&[true]);
        let e = d
            .send(
                "mockai",
                SendRequest {
                    message: "hi".into(),
                    reply_hint: Default::default(),
                },
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Rejected));
        assert!(e.hint.unwrap().contains("channel_act"));
    }

    #[tokio::test]
    async fn recv_is_empty_and_preserves_cursor() {
        let d = driver(&[true]);
        let b = d.recv(Some(42)).await.unwrap();
        assert!(b.items.is_empty());
        assert_eq!(b.next_cursor, 42);
    }

    #[tokio::test]
    async fn channels_and_doctor_aggregate_backends() {
        let d = driver(&[true, false]);
        let chans = d.channels().await.unwrap();
        assert_eq!(chans.len(), 2);
        assert_eq!(chans[0].channel, "image:mockai");
        let rep = d.doctor().await;
        assert!(!rep.ok); // one degraded backend degrades the driver
        assert!(rep.lines.iter().all(|l| l.starts_with("mockai: ")));
    }

    #[tokio::test]
    async fn empty_driver_doctor_is_degraded() {
        let d = ImageDriver::from_backends(vec![]);
        let rep = d.doctor().await;
        assert!(!rep.ok);
        assert_eq!(rep.lines, vec!["no image backends enabled".to_string()]);
    }

    #[test]
    fn clip_helpers_are_char_safe() {
        assert_eq!(clip("héllo", 2), "hé");
        assert_eq!(clip_tail("héllo", 3), "llo");
        assert_eq!(clip("ab", 10), "ab");
        assert_eq!(clip_tail("ab", 10), "ab");
    }
}
