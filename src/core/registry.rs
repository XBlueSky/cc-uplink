use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::driver::{ChannelEntry, DoctorReport, Driver, DriverInfo};
use crate::error::{DriverError, ErrorKind};

#[derive(Default)]
pub struct Registry {
    drivers: BTreeMap<String, Arc<dyn Driver>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, d: Arc<dyn Driver>) {
        self.drivers.insert(d.info().id, d);
    }

    pub fn driver_for(&self, full_addr: &str) -> Result<(Arc<dyn Driver>, String), DriverError> {
        let (prefix, rest) = full_addr.split_once(':').ok_or_else(|| {
            DriverError::new(
                ErrorKind::Invalid,
                format!("address '{full_addr}' must be <driver>:<address>"),
            )
            .with_hint("run channel_list()")
        })?;
        let d = self.drivers.get(prefix).ok_or_else(|| {
            DriverError::new(ErrorKind::NotFound, format!("no driver '{prefix}'"))
                .with_hint("run channel_list()")
        })?;
        Ok((d.clone(), rest.to_string()))
    }

    pub fn drivers(&self) -> impl Iterator<Item = &Arc<dyn Driver>> {
        self.drivers.values()
    }

    pub async fn list_all(&self) -> Vec<(DriverInfo, Vec<ChannelEntry>)> {
        let mut out = Vec::new();
        for d in self.drivers.values() {
            let chans = d.channels().await.unwrap_or_default();
            out.push((d.info(), chans));
        }
        out
    }

    pub async fn doctor_all(&self) -> Vec<DoctorReport> {
        let mut out = Vec::new();
        for d in self.drivers.values() {
            out.push(d.doctor().await);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::*;
    use crate::error::{DriverError, ErrorKind};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockDriver;

    #[async_trait]
    impl Driver for MockDriver {
        fn info(&self) -> DriverInfo {
            DriverInfo {
                id: "mock".into(),
                kind: DriverKind::Both,
                summary: "mock".into(),
            }
        }
        async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError> {
            Ok(vec![ChannelEntry {
                channel: "mock:a".into(),
                labels: vec![],
                detail: serde_json::json!({}),
            }])
        }
        fn ops(&self) -> Vec<OpSpec> {
            vec![OpSpec {
                op: "echo".into(),
                summary: "echo".into(),
                params_schema: serde_json::json!({}),
                result_schema: serde_json::json!({}),
            }]
        }
        async fn send(&self, _addr: &str, _msg: SendRequest) -> Result<SendReceipt, DriverError> {
            Ok(SendReceipt {
                delivered: true,
                correlation_id: "fixed".into(),
                verify_excerpt: None,
                injected_at: "t".into(),
            })
        }
        async fn invoke(
            &self,
            _addr: &str,
            op: &str,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, DriverError> {
            if op == "echo" {
                Ok(args)
            } else {
                Err(DriverError::new(ErrorKind::NotFound, format!("no op {op}")))
            }
        }
        async fn recv(&self, _c: Option<u64>) -> Result<RecvBatch, DriverError> {
            Ok(RecvBatch {
                items: vec![],
                next_cursor: 0,
            })
        }
        async fn doctor(&self) -> DoctorReport {
            DoctorReport {
                driver: "mock".into(),
                ok: true,
                lines: vec![],
            }
        }
    }

    #[tokio::test]
    async fn routes_by_prefix() {
        let mut reg = Registry::new();
        reg.register(Arc::new(MockDriver));
        let (d, addr) = reg.driver_for("mock:a").unwrap();
        assert_eq!(addr, "a");
        let out = d
            .invoke(&addr, "echo", serde_json::json!({"x":1}))
            .await
            .unwrap();
        assert_eq!(out["x"], 1);
    }

    #[test]
    fn unknown_prefix_is_not_found() {
        let reg = Registry::new();
        let e = reg.driver_for("nope:a").err().unwrap();
        assert!(matches!(e.kind, ErrorKind::NotFound));
        assert_eq!(e.hint.as_deref(), Some("run channel_list()"));
    }

    #[test]
    fn missing_colon_is_invalid() {
        let reg = Registry::new();
        let e = reg.driver_for("nocolon").err().unwrap();
        assert!(matches!(e.kind, ErrorKind::Invalid));
        assert_eq!(e.hint.as_deref(), Some("run channel_list()"));
    }
}
