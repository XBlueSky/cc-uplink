use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::DriverError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    Messaging,
    Capability,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    pub id: String,
    pub kind: DriverKind,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    pub channel: String,
    pub labels: Vec<String>,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpSpec {
    pub op: String,
    pub summary: String,
    /// false = returns state without changing the world (routed via
    /// channel_observe); true = injects, spends, or renames (channel_act).
    #[serde(default)]
    pub mutating: bool,
    pub params_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReplyHint {
    #[default]
    Full,
    Short,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest {
    pub message: String,
    #[serde(default)]
    pub reply_hint: ReplyHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendReceipt {
    pub delivered: bool,
    pub correlation_id: String,
    pub verify_excerpt: Option<String>,
    pub injected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecvItem {
    pub cursor: u64,
    pub at: String,
    pub from: Option<String>,
    pub id: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecvBatch {
    pub items: Vec<RecvItem>,
    pub next_cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub driver: String,
    pub ok: bool,
    pub lines: Vec<String>,
}

#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> DriverInfo;
    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError>;
    fn ops(&self) -> Vec<OpSpec>;
    async fn send(&self, addr: &str, msg: SendRequest) -> Result<SendReceipt, DriverError>;
    async fn invoke(
        &self,
        addr: &str,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError>;
    async fn recv(&self, cursor: Option<u64>) -> Result<RecvBatch, DriverError>;
    async fn doctor(&self) -> DoctorReport;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shape_receipt_roundtrip() {
        let r = SendReceipt {
            delivered: true,
            correlation_id: "ab12cd34".into(),
            verify_excerpt: Some("id:ab12cd34".into()),
            injected_at: "2026-07-22T00:00:00Z".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: SendReceipt = serde_json::from_str(&s).unwrap();
        assert_eq!(back.correlation_id, "ab12cd34");
    }

    #[test]
    fn reply_hint_default_is_full() {
        let req: SendRequest = serde_json::from_str(r#"{"message":"hi"}"#).unwrap();
        assert!(matches!(req.reply_hint, ReplyHint::Full));
    }
}
