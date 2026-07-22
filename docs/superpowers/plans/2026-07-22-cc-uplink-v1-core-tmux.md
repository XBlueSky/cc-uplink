# cc-uplink v1 — Core + tmux Driver (M1+M2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working `cc-uplink` binary: stdio MCP server with six fixed tools backed by a control-mode-first tmux driver (send/read/keys/label/await_idle/ask/recv), plus human CLI (`doctor/send/invoke/log`).

**Architecture:** One Rust binary. Core = wire-shaped `Driver` trait + Registry routing `<driver>:<address>`. tmux driver talks to tmux through a `TmuxTransport` seam: `ControlMode` (long-lived `tmux -C attach`, event stream) primary, `OneShotCli` fallback/bootstrap. MCP layer (rmcp) and CLI subcommands share the same core.

**Tech Stack:** Rust edition 2024 (MSRV 1.85), tokio, rmcp, serde/serde_json, toml, thiserror, async-trait, uuid, dirs. Dev: tempfile. No OpenSSL anywhere (reqwest arrives in the M3 plan with rustls only).

**Spec:** `docs/superpowers/specs/2026-07-22-cc-uplink-design.md` (approved).

## Global Constraints

- Six MCP tools, exact names: `channel_list`, `channel_describe`, `channel_send`, `channel_invoke`, `channel_recv`, `channel_doctor`. Adding a driver must never add a tool.
- Driver trait I/O: serde-serializable types only. No lifetimes/callbacks in payloads.
- Never route argv through a shell. `tokio::process::Command` with arg vectors only.
- tmux ≥ 3.2 for full control-mode features; reference environment tmux 3.5a; `OneShotCli` fallback must keep basic ops working.
- Secrets env-only (not needed in this plan; rule stands).
- License MIT. Single binary `cc-uplink`.
- Quality gates on every commit: `cargo fmt --check` and `cargo clippy --release --all-targets` must pass (per user's Rust pre-commit rules).
- Commit trailer: `Signed-off-by: tonyhu <tonyhu@synology.com>`. Never add a Claude-Session trailer.
- Error rendering format, verbatim: `uplink error [<driver>:<Kind>]: <message>` + optional ` — hint: <hint>`.
- Envelope header, verbatim: `[uplink from:<from> pane:<pane> id:<8hex>] <message>`; reply header: `[reply id:<8hex>] <message>`.

## File Structure

```
Cargo.toml, rust-toolchain.toml, .gitignore, .gitlab-ci.yml
src/
  main.rs            # arg dispatch: serve|doctor|send|invoke|log
  lib.rs             # pub mod tree
  error.rs           # ErrorKind, DriverError, render()
  config.rs          # ~/.config/cc-uplink/config.toml
  core/mod.rs
  core/driver.rs     # Driver trait + all DTOs
  core/registry.rs   # address parse + routing
  core/envelope.rs   # envelope v2 format/parse
  core/logsink.rs    # JSONL conversation log (state dir)
  drivers/mod.rs
  drivers/tmux/mod.rs        # TmuxDriver (Driver impl, ops, guard, recv buffer)
  drivers/tmux/protocol.rs   # pure: CM line parser, octal unescape, ANSI strip, CM quoting
  drivers/tmux/transport.rs  # TmuxTransport trait + OneShotCli + bootstrap
  drivers/tmux/control.rs    # ControlMode transport: framing, events, pause, reconnect
  mcp.rs             # rmcp server: six tools → Registry
  cli/mod.rs         # doctor/send/invoke/log implementations
tests/
  common/mod.rs      # private-socket tmux test server harness
  tmux_integration.rs
```

---

### Task 1: Scaffold + error model

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `.gitlab-ci.yml`, `src/main.rs`, `src/lib.rs`, `src/error.rs`

**Interfaces:**
- Produces: `error::{ErrorKind, DriverError}`; `DriverError::new(kind, msg)`, `.with_hint(s)`, `.render(driver_id) -> String`. All later tasks return `Result<_, DriverError>`.

- [ ] **Step 1: Scaffold files**

`Cargo.toml`:
```toml
[package]
name = "cc-uplink"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"
description = "Claude Code's unified outbound channel layer (MCP server + CLI)"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "io-util", "sync", "time", "fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "2"
anyhow = "1"
async-trait = "0.1"
uuid = { version = "1", features = ["v4"] }
dirs = "6"

[dev-dependencies]
tempfile = "3"
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
```

`.gitignore`:
```
/target
```

`.gitlab-ci.yml`:
```yaml
image: rust:latest
before_script:
  - apt-get update -qq && apt-get install -y -qq tmux
  - rustup component add rustfmt clippy
stages: [check]
check:
  stage: check
  script:
    - cargo fmt --check
    - cargo clippy --release --all-targets -- -D warnings
    - cargo test
```

`src/main.rs`:
```rust
fn main() {
    println!("cc-uplink");
}
```

`src/lib.rs`:
```rust
pub mod error;
```

- [ ] **Step 2: Write the failing test** (in `src/error.rs`, module test)

```rust
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
        Self { kind, message: message.into(), hint: None, evidence: None }
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
        todo!()
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
        assert_eq!(e.render("tmux"), "uplink error [tmux:Timeout]: verify timed out");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p cc-uplink error::` — Expected: FAIL (panic at `todo!()`).

- [ ] **Step 4: Implement `render`**

```rust
    pub fn render(&self, driver_id: &str) -> String {
        let base = format!("uplink error [{}:{:?}]: {}", driver_id, self.kind, self.message);
        match &self.hint {
            Some(h) => format!("{base} — hint: {h}"),
            None => base,
        }
    }
```

- [ ] **Step 5: Run tests, fmt, clippy — all green**

Run: `cargo test && cargo fmt --check && cargo clippy --release --all-targets -- -D warnings` — Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: scaffold crate with error model

Signed-off-by: tonyhu <tonyhu@synology.com>"
```

---

### Task 2: Core DTOs + wire-shaped Driver trait

**Files:**
- Create: `src/core/mod.rs`, `src/core/driver.rs`
- Modify: `src/lib.rs` (add `pub mod core;`)

**Interfaces:**
- Produces (all `Serialize + Deserialize + Debug + Clone`):
  - `DriverKind { Messaging, Capability, Both }`
  - `DriverInfo { id: String, kind: DriverKind, summary: String }`
  - `ChannelEntry { channel: String, labels: Vec<String>, detail: serde_json::Value }`
  - `OpSpec { op: String, summary: String, params_schema: serde_json::Value, result_schema: serde_json::Value }`
  - `SendRequest { message: String, reply_hint: ReplyHint }`, `ReplyHint { Full, Short, None }` (serde rename_all = "lowercase", default Full)
  - `SendReceipt { delivered: bool, correlation_id: String, verify_excerpt: Option<String>, injected_at: String }`
  - `RecvItem { cursor: u64, at: String, from: Option<String>, id: Option<String>, raw: String }`, `RecvBatch { items: Vec<RecvItem>, next_cursor: u64 }`
  - `DoctorReport { driver: String, ok: bool, lines: Vec<String> }`
  - `trait Driver: Send + Sync` with methods exactly as in spec §4 (`info/channels/ops/send/invoke/recv/doctor`), `async_trait`.

- [ ] **Step 1: Write the failing test** (`src/core/driver.rs` bottom)

```rust
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
```

- [ ] **Step 2: Run to verify it fails** — `cargo test core::` — FAIL (types missing).

- [ ] **Step 3: Implement**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::DriverError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind { Messaging, Capability, Both }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo { pub id: String, pub kind: DriverKind, pub summary: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry { pub channel: String, pub labels: Vec<String>, pub detail: serde_json::Value }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpSpec {
    pub op: String,
    pub summary: String,
    pub params_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReplyHint { #[default] Full, Short, None }

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
pub struct RecvBatch { pub items: Vec<RecvItem>, pub next_cursor: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport { pub driver: String, pub ok: bool, pub lines: Vec<String> }

#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> DriverInfo;
    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError>;
    fn ops(&self) -> Vec<OpSpec>;
    async fn send(&self, addr: &str, msg: SendRequest) -> Result<SendReceipt, DriverError>;
    async fn invoke(&self, addr: &str, op: &str, args: serde_json::Value)
        -> Result<serde_json::Value, DriverError>;
    async fn recv(&self, cursor: Option<u64>) -> Result<RecvBatch, DriverError>;
    async fn doctor(&self) -> DoctorReport;
}
```

`src/core/mod.rs`: `pub mod driver; pub mod registry; pub mod envelope; pub mod logsink;` — add modules as they appear in later tasks (start with `pub mod driver;`).

- [ ] **Step 4: Run tests/fmt/clippy** — PASS.

- [ ] **Step 5: Commit** — `git commit -m "feat: core DTOs and wire-shaped Driver trait"` (+ Signed-off-by trailer as in Task 1).

---

### Task 3: Envelope v2

**Files:**
- Create: `src/core/envelope.rs` (add `pub mod envelope;` to `core/mod.rs`)

**Interfaces:**
- Produces:
  - `new_correlation_id() -> String` (8 lowercase hex)
  - `format_outbound(from: &str, own_pane: &str, id: &str, message: &str, hint: ReplyHint) -> String`
  - `parse_inbound(line: &str) -> Option<Inbound>` where `Inbound { kind: InboundKind, from: Option<String>, id: Option<String>, body: String }`, `InboundKind { Uplink, Reply }`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::ReplyHint;

    #[test]
    fn outbound_full() {
        let s = format_outbound("claude", "%5", "ab12cd34", "hello", ReplyHint::Full);
        assert!(s.starts_with("[uplink from:claude pane:%5 id:ab12cd34] hello"));
        assert!(s.contains("tmux send-keys -t %5 -l '[reply id:ab12cd34]"));
    }

    #[test]
    fn outbound_none_has_no_reply_block() {
        let s = format_outbound("claude", "%5", "ab12cd34", "hello", ReplyHint::None);
        assert_eq!(s, "[uplink from:claude pane:%5 id:ab12cd34] hello");
    }

    #[test]
    fn parse_uplink_and_reply() {
        let u = parse_inbound("[uplink from:codex pane:%2 id:ffffffff] hi there").unwrap();
        assert!(matches!(u.kind, InboundKind::Uplink));
        assert_eq!(u.from.as_deref(), Some("codex"));
        assert_eq!(u.id.as_deref(), Some("ffffffff"));
        assert_eq!(u.body, "hi there");

        let r = parse_inbound("[reply id:ab12cd34] the answer").unwrap();
        assert!(matches!(r.kind, InboundKind::Reply));
        assert_eq!(r.id.as_deref(), Some("ab12cd34"));
        assert_eq!(r.body, "the answer");

        assert!(parse_inbound("plain output line").is_none());
    }

    #[test]
    fn correlation_id_is_8_hex() {
        let id = new_correlation_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
```

- [ ] **Step 2: Run to fail** — `cargo test envelope` — FAIL.

- [ ] **Step 3: Implement**

```rust
use crate::core::driver::ReplyHint;

pub struct Inbound {
    pub kind: InboundKind,
    pub from: Option<String>,
    pub id: Option<String>,
    pub body: String,
}

pub enum InboundKind { Uplink, Reply }

pub fn new_correlation_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub fn format_outbound(from: &str, own_pane: &str, id: &str, message: &str, hint: ReplyHint) -> String {
    let head = format!("[uplink from:{from} pane:{own_pane} id:{id}] {message}");
    match hint {
        ReplyHint::None => head,
        ReplyHint::Short => format!("{head} (reply-to:{own_pane} id:{id})"),
        ReplyHint::Full => format!(
            "{head} (reply: run `tmux send-keys -t {own_pane} -l '[reply id:{id}] <your answer>' \\; send-keys -t {own_pane} Enter`)"
        ),
    }
}

pub fn parse_inbound(line: &str) -> Option<Inbound> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("[reply id:") {
        let (id, body) = rest.split_once(']')?;
        return Some(Inbound {
            kind: InboundKind::Reply,
            from: None,
            id: Some(id.trim().to_string()),
            body: body.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix("[uplink ") {
        let (fields, body) = rest.split_once(']')?;
        let mut from = None;
        let mut id = None;
        for tok in fields.split_whitespace() {
            if let Some(v) = tok.strip_prefix("from:") { from = Some(v.to_string()); }
            if let Some(v) = tok.strip_prefix("id:") { id = Some(v.to_string()); }
        }
        return Some(Inbound { kind: InboundKind::Uplink, from, id, body: body.trim().to_string() });
    }
    None
}
```

- [ ] **Step 4: Run tests/fmt/clippy** — PASS.
- [ ] **Step 5: Commit** — `feat: envelope v2 format and inbound parser`.

---

### Task 4: Registry + address routing

**Files:**
- Create: `src/core/registry.rs` (add `pub mod registry;`)

**Interfaces:**
- Consumes: `Driver` trait (Task 2).
- Produces: `Registry::new()`, `.register(Arc<dyn Driver>)`, `.parse_addr("tmux:codex") -> Result<(String, String)>`, `.driver_for(addr) -> Result<(Arc<dyn Driver>, String)>`, `.list_all() -> Vec<(DriverInfo, Vec<ChannelEntry>)>` (async), `.doctor_all() -> Vec<DoctorReport>` (async).

- [ ] **Step 1: Write the failing tests** — a `MockDriver` implementing `Driver` with canned data lives in this test module (id `"mock"`, one channel `mock:a`, one op `echo` that returns its args, send returns receipt with `correlation_id:"fixed"`).

```rust
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
            DriverInfo { id: "mock".into(), kind: DriverKind::Both, summary: "mock".into() }
        }
        async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError> {
            Ok(vec![ChannelEntry { channel: "mock:a".into(), labels: vec![], detail: serde_json::json!({}) }])
        }
        fn ops(&self) -> Vec<OpSpec> {
            vec![OpSpec { op: "echo".into(), summary: "echo".into(),
                params_schema: serde_json::json!({}), result_schema: serde_json::json!({}) }]
        }
        async fn send(&self, _addr: &str, _msg: SendRequest) -> Result<SendReceipt, DriverError> {
            Ok(SendReceipt { delivered: true, correlation_id: "fixed".into(),
                verify_excerpt: None, injected_at: "t".into() })
        }
        async fn invoke(&self, _addr: &str, op: &str, args: serde_json::Value)
            -> Result<serde_json::Value, DriverError> {
            if op == "echo" { Ok(args) } else {
                Err(DriverError::new(ErrorKind::NotFound, format!("no op {op}")))
            }
        }
        async fn recv(&self, _c: Option<u64>) -> Result<RecvBatch, DriverError> {
            Ok(RecvBatch { items: vec![], next_cursor: 0 })
        }
        async fn doctor(&self) -> DoctorReport {
            DoctorReport { driver: "mock".into(), ok: true, lines: vec![] }
        }
    }

    #[tokio::test]
    async fn routes_by_prefix() {
        let mut reg = Registry::new();
        reg.register(Arc::new(MockDriver));
        let (d, addr) = reg.driver_for("mock:a").unwrap();
        assert_eq!(addr, "a");
        let out = d.invoke(&addr, "echo", serde_json::json!({"x":1})).await.unwrap();
        assert_eq!(out["x"], 1);
    }

    #[test]
    fn unknown_prefix_is_not_found() {
        let reg = Registry::new();
        let e = reg.driver_for("nope:a").unwrap_err();
        assert!(matches!(e.kind, ErrorKind::NotFound));
    }

    #[test]
    fn missing_colon_is_invalid() {
        let reg = Registry::new();
        let e = reg.driver_for("nocolon").unwrap_err();
        assert!(matches!(e.kind, ErrorKind::Invalid));
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::driver::{ChannelEntry, DoctorReport, Driver, DriverInfo};
use crate::error::{DriverError, ErrorKind};

#[derive(Default)]
pub struct Registry {
    drivers: BTreeMap<String, Arc<dyn Driver>>,
}

impl Registry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, d: Arc<dyn Driver>) {
        self.drivers.insert(d.info().id, d);
    }

    pub fn driver_for(&self, full_addr: &str) -> Result<(Arc<dyn Driver>, String), DriverError> {
        let (prefix, rest) = full_addr.split_once(':').ok_or_else(|| {
            DriverError::new(ErrorKind::Invalid, format!("address '{full_addr}' must be <driver>:<address>"))
                .with_hint("run channel_list()")
        })?;
        let d = self.drivers.get(prefix).ok_or_else(|| {
            DriverError::new(ErrorKind::NotFound, format!("no driver '{prefix}'"))
                .with_hint("run channel_list()")
        })?;
        Ok((d.clone(), rest.to_string()))
    }

    pub fn drivers(&self) -> impl Iterator<Item = &Arc<dyn Driver>> { self.drivers.values() }

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
        for d in self.drivers.values() { out.push(d.doctor().await); }
        out
    }
}
```

- [ ] **Step 4: Run tests/fmt/clippy** — PASS.
- [ ] **Step 5: Commit** — `feat: registry with driver-prefix address routing`.

---

### Task 5: Config

**Files:**
- Create: `src/config.rs` (add `pub mod config;` to lib.rs)

**Interfaces:**
- Produces: `Config { drivers: DriversCfg }`, `DriversCfg { tmux: TmuxCfg }`, `TmuxCfg { enabled: bool (default true), allowlist: Option<Vec<String>> }`; `Config::load() -> Config` (reads `~/.config/cc-uplink/config.toml` via `dirs::config_dir()`, missing file ⇒ defaults); `Config::from_str(&str) -> Result<Config, DriverError>` (Invalid on bad TOML). Image driver sections arrive in the M3 plan; unknown TOML keys must be ignored (`serde(default)` everywhere, no deny_unknown_fields).

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.tmux.enabled);
        assert!(c.drivers.tmux.allowlist.is_none());
    }

    #[test]
    fn parses_allowlist() {
        let c = Config::from_str("[drivers.tmux]\nenabled = true\nallowlist = [\"codex\", \"%1\"]\n").unwrap();
        assert_eq!(c.drivers.tmux.allowlist.as_deref(), Some(&["codex".to_string(), "%1".to_string()][..]));
    }

    #[test]
    fn bad_toml_is_invalid() {
        assert!(matches!(Config::from_str("not [ toml").unwrap_err().kind,
            crate::error::ErrorKind::Invalid));
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub drivers: DriversCfg,
}

#[derive(Debug, Default, Deserialize)]
pub struct DriversCfg {
    #[serde(default)]
    pub tmux: TmuxCfg,
}

#[derive(Debug, Deserialize)]
pub struct TmuxCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
}

impl Default for TmuxCfg {
    fn default() -> Self { Self { enabled: true, allowlist: None } }
}

fn default_true() -> bool { true }

impl Config {
    pub fn from_str(s: &str) -> Result<Self, DriverError> {
        if s.trim().is_empty() { return Ok(Self::default()); }
        toml::from_str(s).map_err(|e| DriverError::new(ErrorKind::Invalid, format!("config: {e}")))
    }

    pub fn load() -> Self {
        let path = dirs::config_dir().map(|d| d.join("cc-uplink/config.toml"));
        match path.and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(s) => Self::from_str(&s).unwrap_or_default(),
            None => Self::default(),
        }
    }
}
```

- [ ] **Step 4: Tests/fmt/clippy PASS.**
- [ ] **Step 5: Commit** — `feat: config loading with defaults`.

---

### Task 6: Control-mode protocol parser (pure)

**Files:**
- Create: `src/drivers/mod.rs` (`pub mod tmux;`), `src/drivers/tmux/mod.rs` (starts as `pub mod protocol;`), `src/drivers/tmux/protocol.rs`
- Modify: `src/lib.rs` (add `pub mod drivers;`)

**Interfaces:**
- Produces:
  - `CmLine { Begin{seq:u64}, End{seq:u64}, CmdError{seq:u64}, Output{pane:String, data:Vec<u8>}, Pause{pane:String}, Exit, Notification(String), Body(String) }`
  - `parse_line(&str) -> CmLine`
  - `unescape_octal(&str) -> Vec<u8>`
  - `strip_ansi(&[u8]) -> String` (drops CSI `ESC[...cmd` and OSC `ESC]...BEL/ESC\` sequences, keeps printable text; lossy UTF-8)
  - `quote_cm_arg(&str) -> String` (single-quote wrap; `'` → `'\''`)

- [ ] **Step 1: Failing golden tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_begin_end_error() {
        assert!(matches!(parse_line("%begin 1721600000 42 1"), CmLine::Begin { seq: 42 }));
        assert!(matches!(parse_line("%end 1721600000 42 1"), CmLine::End { seq: 42 }));
        assert!(matches!(parse_line("%error 1721600000 42 1"), CmLine::CmdError { seq: 42 }));
    }

    #[test]
    fn parses_output_with_octal() {
        match parse_line(r"%output %3 hello\040world\134x") {
            CmLine::Output { pane, data } => {
                assert_eq!(pane, "%3");
                assert_eq!(data, b"hello world\\x");
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parses_pause_exit_notification_body() {
        assert!(matches!(parse_line("%pause %3"), CmLine::Pause { .. }));
        assert!(matches!(parse_line("%exit"), CmLine::Exit));
        assert!(matches!(parse_line("%session-changed $1 main"), CmLine::Notification(_)));
        assert!(matches!(parse_line("plain body line"), CmLine::Body(_)));
    }

    #[test]
    fn strips_ansi() {
        let s = strip_ansi(b"\x1b[1;32mgreen\x1b[0m id:ab12cd34 \x1b]0;title\x07tail");
        assert_eq!(s, "green id:ab12cd34 tail");
    }

    #[test]
    fn quotes_cm_arg() {
        assert_eq!(quote_cm_arg("plain"), "'plain'");
        assert_eq!(quote_cm_arg("it's"), r"'it'\''s'");
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
#[derive(Debug)]
pub enum CmLine {
    Begin { seq: u64 },
    End { seq: u64 },
    CmdError { seq: u64 },
    Output { pane: String, data: Vec<u8> },
    Pause { pane: String },
    Exit,
    Notification(String),
    Body(String),
}

fn seq_of(rest: &str) -> u64 {
    rest.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

pub fn parse_line(line: &str) -> CmLine {
    if let Some(r) = line.strip_prefix("%begin ") { return CmLine::Begin { seq: seq_of(r) }; }
    if let Some(r) = line.strip_prefix("%end ") { return CmLine::End { seq: seq_of(r) }; }
    if let Some(r) = line.strip_prefix("%error ") { return CmLine::CmdError { seq: seq_of(r) }; }
    if let Some(r) = line.strip_prefix("%output ") {
        if let Some((pane, data)) = r.split_once(' ') {
            return CmLine::Output { pane: pane.to_string(), data: unescape_octal(data) };
        }
    }
    if let Some(r) = line.strip_prefix("%pause ") {
        return CmLine::Pause { pane: r.trim().to_string() };
    }
    if line == "%exit" || line.starts_with("%exit ") { return CmLine::Exit; }
    if line.starts_with('%') { return CmLine::Notification(line.to_string()); }
    CmLine::Body(line.to_string())
}

pub fn unescape_octal(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            if i + 3 < b.len() && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit() && b[i + 3].is_ascii_digit() {
                let oct = std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("0");
                if let Ok(v) = u8::from_str_radix(oct, 8) {
                    out.push(v);
                    i += 4;
                    continue;
                }
            }
            if i + 1 < b.len() && b[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

pub fn strip_ansi(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b {
            if i + 1 < data.len() && data[i + 1] == b'[' {
                i += 2;
                while i < data.len() && !(0x40..=0x7e).contains(&data[i]) { i += 1; }
                i += 1; // final byte
                continue;
            }
            if i + 1 < data.len() && data[i + 1] == b']' {
                i += 2;
                while i < data.len() && data[i] != 0x07 {
                    if data[i] == 0x1b && i + 1 < data.len() && data[i + 1] == b'\\' { i += 1; break; }
                    i += 1;
                }
                i += 1;
                continue;
            }
            i += 2; // other short escape
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn quote_cm_arg(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
```

- [ ] **Step 4: Tests/fmt/clippy PASS.**
- [ ] **Step 5: Commit** — `feat: control-mode protocol parser (framing lines, octal, ansi, quoting)`.

---

### Task 7: TmuxTransport trait, OneShotCli, bootstrap, test harness

**Files:**
- Create: `src/drivers/tmux/transport.rs` (add `pub mod transport;` to tmux/mod.rs), `tests/common/mod.rs`, `tests/tmux_integration.rs`

**Interfaces:**
- Produces:
  - `PaneEvent { pane: String, data: Vec<u8> }`
  - `#[async_trait] trait TmuxTransport: Send + Sync { async fn run(&self, args: &[String]) -> Result<String, DriverError>; fn events(&self) -> Option<tokio::sync::broadcast::Receiver<PaneEvent>>; }`
  - `OneShotCli { socket: Option<String> }` implementing it (`events()` returns `None`); `OneShotCli::from_env()` reads `$TMUX` (first comma-field = socket path) — same detection rule as tmux-bridge.
  - `Bootstrap::own_context(t: &dyn TmuxTransport) -> Result<OwnCtx>` where `OwnCtx { pane: String, session: String, label: Option<String> }` via `display-message -p -t $TMUX_PANE '#{session_name}|#{@name}'` (Unavailable if `$TMUX_PANE` unset).
  - Test harness `tests/common/mod.rs`: `TmuxTestServer::start() -> Option<Self>` (None + eprintln skip when tmux missing): private socket in TempDir, `tmux -S <sock> new-session -d -x 180 -y 45 -s it`, `.sock() -> String`, `.run(args)` sync helper, kill-server on Drop.

- [ ] **Step 1: Write failing integration test** (`tests/tmux_integration.rs`)

```rust
mod common;
use cc_uplink::drivers::tmux::transport::{OneShotCli, TmuxTransport};

#[tokio::test]
async fn one_shot_cli_runs_against_private_server() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let t = OneShotCli { socket: Some(srv.sock()) };
    let out = t.run(&["display-message".into(), "-p".into(), "ok-#{session_name}".into()]).await.unwrap();
    assert_eq!(out.trim(), "ok-it");
    assert!(t.events().is_none());
}
```

`tests/common/mod.rs`:

```rust
use std::process::Command;
use tempfile::TempDir;

pub struct TmuxTestServer { dir: TempDir }

impl TmuxTestServer {
    pub fn start() -> Option<Self> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("SKIP: tmux not installed");
            return None;
        }
        let dir = TempDir::new().unwrap();
        let s = Self { dir };
        s.run(&["new-session", "-d", "-x", "180", "-y", "45", "-s", "it"]);
        Some(s)
    }
    pub fn sock(&self) -> String {
        self.dir.path().join("sock").to_string_lossy().into_owned()
    }
    pub fn run(&self, args: &[&str]) -> String {
        let out = Command::new("tmux").arg("-S").arg(self.sock()).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for TmuxTestServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux").arg("-S").arg(self.sock()).args(["kill-server"]).output();
    }
}
```

- [ ] **Step 2: Run to fail** — `cargo test --test tmux_integration` — FAIL (transport module missing).

- [ ] **Step 3: Implement `transport.rs`**

```rust
use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Clone)]
pub struct PaneEvent { pub pane: String, pub data: Vec<u8> }

#[async_trait]
pub trait TmuxTransport: Send + Sync {
    async fn run(&self, args: &[String]) -> Result<String, DriverError>;
    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>>;
}

pub struct OneShotCli { pub socket: Option<String> }

impl OneShotCli {
    pub fn from_env() -> Self {
        let socket = std::env::var("TMUX").ok()
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
            .map_err(|e| DriverError::new(ErrorKind::Unavailable, format!("tmux not runnable: {e}")))?;
        if !out.status.success() {
            return Err(DriverError::new(ErrorKind::Upstream,
                String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>> { None }
}

pub struct OwnCtx { pub pane: String, pub session: String, pub label: Option<String> }

pub async fn own_context(t: &dyn TmuxTransport) -> Result<OwnCtx, DriverError> {
    let pane = std::env::var("TMUX_PANE").map_err(|_| {
        DriverError::new(ErrorKind::Unavailable, "$TMUX_PANE is unset (not inside tmux)")
    })?;
    let out = t.run(&["display-message".into(), "-p".into(), "-t".into(), pane.clone(),
        "#{session_name}|#{@name}".into()]).await?;
    let line = out.trim();
    let (session, label) = line.split_once('|').unwrap_or((line, ""));
    Ok(OwnCtx {
        pane,
        session: session.to_string(),
        label: if label.is_empty() { None } else { Some(label.to_string()) },
    })
}
```

Add `pub mod transport;` to `src/drivers/tmux/mod.rs`.

- [ ] **Step 4: Run integration + unit tests, fmt, clippy** — PASS (integration skips gracefully without tmux).
- [ ] **Step 5: Commit** — `feat: tmux transport seam with one-shot CLI and test harness`.

---

### Task 8: ControlMode transport

**Files:**
- Create: `src/drivers/tmux/control.rs` (add `pub mod control;`)
- Test: framing unit tests in-module; integration in `tests/tmux_integration.rs`

**Interfaces:**
- Consumes: `protocol::{parse_line, CmLine, quote_cm_arg}`, `PaneEvent`, `TmuxTransport`.
- Produces: `ControlMode::attach(socket: Option<String>, session: &str) -> Result<Arc<ControlMode>, DriverError>` implementing `TmuxTransport`; `run()` sends the command line over CM stdin (each arg passed through `quote_cm_arg`, joined by spaces, `\n`-terminated) and awaits its `%begin/%end|%error` block FIFO; `events()` returns `broadcast::Receiver<PaneEvent>` (capacity 4096). `%pause %N` ⇒ immediately writes `refresh-client -A '%N:continue'\n`. `%exit`/EOF ⇒ pending replies fail `Unavailable`; a supervisor task retries attach with 500 ms → 8 s backoff and swaps the connection; `is_connected() -> bool` for doctor. Pure framing core `Framing::feed(CmLine) -> Vec<FramingOut>` (unit-testable without a process): tracks in-block state, discards a reply block when no waiter is queued (covers the attach greeting block).

- [ ] **Step 1: Failing unit test for framing core**

```rust
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
        let out = feed_all(&mut f, &["%output %3 abc", "%begin 1 9 1", "bad", "%error 1 9 1"]);
        assert!(matches!(out[0], FramingOut::Event { .. }));
        assert!(matches!(out[1], FramingOut::Reply { ok: false, .. }));
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, broadcast, oneshot};

use crate::drivers::tmux::protocol::{CmLine, parse_line, quote_cm_arg};
use crate::drivers::tmux::transport::{PaneEvent, TmuxTransport};
use crate::error::{DriverError, ErrorKind};

pub enum FramingOut {
    Event { pane: String, data: Vec<u8> },
    Reply { token: u64, ok: bool, body: String },
    Pause { pane: String },
    Exited,
}

pub struct Framing {
    waiters: VecDeque<u64>,
    in_block: bool,
    body: Vec<String>,
}

impl Framing {
    pub fn new() -> Self { Self { waiters: VecDeque::new(), in_block: false, body: vec![] } }
    pub fn push_waiter(&mut self, token: u64) { self.waiters.push_back(token); }

    pub fn feed(&mut self, line: CmLine) -> Vec<FramingOut> {
        match line {
            CmLine::Output { pane, data } => vec![FramingOut::Event { pane, data }],
            CmLine::Pause { pane } => vec![FramingOut::Pause { pane }],
            CmLine::Exit => vec![FramingOut::Exited],
            CmLine::Begin { .. } => { self.in_block = true; self.body.clear(); vec![] }
            CmLine::Body(s) => { if self.in_block { self.body.push(s); } vec![] }
            CmLine::End { .. } | CmLine::CmdError { .. } if self.in_block => {
                self.in_block = false;
                let ok = matches!(line_kind(&line), BlockEnd::Ok);
                let body = self.body.join("\n");
                match self.waiters.pop_front() {
                    Some(token) => vec![FramingOut::Reply { token, ok, body }],
                    None => vec![], // greeting / unsolicited block
                }
            }
            _ => vec![],
        }
    }
}

enum BlockEnd { Ok, Err }
fn line_kind(l: &CmLine) -> BlockEnd {
    match l { CmLine::End { .. } => BlockEnd::Ok, _ => BlockEnd::Err }
}

pub struct ControlMode {
    stdin: Mutex<tokio::process::ChildStdin>,
    state: Mutex<CmState>,
    events_tx: broadcast::Sender<PaneEvent>,
    connected: AtomicBool,
}

struct CmState {
    framing: Framing,
    pending: VecDeque<oneshot::Sender<(bool, String)>>,
    next_token: u64,
}

impl ControlMode {
    pub async fn attach(socket: Option<String>, session: &str) -> Result<Arc<Self>, DriverError> {
        let mut cmd = tokio::process::Command::new("tmux");
        if let Some(s) = &socket { cmd.arg("-S").arg(s); }
        cmd.args(["-C", "attach", "-t", session])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd.spawn()
            .map_err(|e| DriverError::new(ErrorKind::Unavailable, format!("spawn tmux -C: {e}")))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (events_tx, _) = broadcast::channel(4096);

        let cm = Arc::new(Self {
            stdin: Mutex::new(stdin),
            state: Mutex::new(CmState { framing: Framing::new(), pending: VecDeque::new(), next_token: 1 }),
            events_tx,
            connected: AtomicBool::new(true),
        });

        let reader_cm = cm.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                reader_cm.on_line(&line).await;
            }
            reader_cm.on_disconnect().await;
        });
        Ok(cm)
    }

    async fn on_line(&self, raw: &str) {
        let outs = { self.state.lock().await.framing.feed(parse_line(raw)) };
        for out in outs {
            match out {
                FramingOut::Event { pane, data } => { let _ = self.events_tx.send(PaneEvent { pane, data }); }
                FramingOut::Reply { ok, body, .. } => {
                    if let Some(tx) = self.state.lock().await.pending.pop_front() {
                        let _ = tx.send((ok, body));
                    }
                }
                FramingOut::Pause { pane } => {
                    let line = format!("refresh-client -A {}\n", quote_cm_arg(&format!("{pane}:continue")));
                    let _ = self.stdin.lock().await.write_all(line.as_bytes()).await;
                }
                FramingOut::Exited => self.on_disconnect_flag(),
            }
        }
    }

    fn on_disconnect_flag(&self) { self.connected.store(false, Ordering::SeqCst); }

    async fn on_disconnect(&self) {
        self.on_disconnect_flag();
        let mut st = self.state.lock().await;
        while let Some(tx) = st.pending.pop_front() {
            let _ = tx.send((false, "control-mode connection lost".into()));
        }
    }

    pub fn is_connected(&self) -> bool { self.connected.load(Ordering::SeqCst) }
}

#[async_trait]
impl TmuxTransport for ControlMode {
    async fn run(&self, args: &[String]) -> Result<String, DriverError> {
        if !self.is_connected() {
            return Err(DriverError::new(ErrorKind::Unavailable, "control-mode disconnected"));
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut st = self.state.lock().await;
            let token = st.next_token;
            st.next_token += 1;
            st.framing.push_waiter(token);
            st.pending.push_back(tx);
        }
        let line = args.iter().map(|a| quote_cm_arg(a)).collect::<Vec<_>>().join(" ") + "\n";
        self.stdin.lock().await.write_all(line.as_bytes()).await
            .map_err(|e| DriverError::new(ErrorKind::Unavailable, format!("cm write: {e}")))?;
        let (ok, body) = tokio::time::timeout(std::time::Duration::from_secs(10), rx).await
            .map_err(|_| DriverError::new(ErrorKind::Timeout, "cm reply timed out"))?
            .map_err(|_| DriverError::new(ErrorKind::Unavailable, "cm reply dropped"))?;
        if ok { Ok(body) } else { Err(DriverError::new(ErrorKind::Upstream, body)) }
    }

    fn events(&self) -> Option<broadcast::Receiver<PaneEvent>> {
        Some(self.events_tx.subscribe())
    }
}
```

Reconnect supervision lives in the driver (Task 9): the driver holds `ArcSwap`-like `Mutex<Arc<dyn TmuxTransport>>` and re-attaches on `Unavailable` with backoff — implemented there to keep `ControlMode` single-connection simple. (No new crate: a `Mutex<Arc<...>>` suffices.)

- [ ] **Step 4: Add integration test** (`tests/tmux_integration.rs`)

```rust
use cc_uplink::drivers::tmux::control::ControlMode;

#[tokio::test]
async fn control_mode_attach_run_and_events() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let cm = ControlMode::attach(Some(srv.sock()), "it").await.unwrap();
    let out = cm.run(&["display-message".into(), "-p".into(), "cm-#{session_name}".into()]).await.unwrap();
    assert_eq!(out.trim(), "cm-it");

    // events: make the pane print something and observe %output
    let mut rx = cm.events().unwrap();
    srv.run(&["send-keys", "-t", "it", "-l", "echo uplink-evt-marker"]);
    srv.run(&["send-keys", "-t", "it", "Enter"]);
    let mut seen = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Ok(ev)) => {
                if String::from_utf8_lossy(&ev.data).contains("uplink-evt-marker") { seen = true; break; }
            }
            _ => {}
        }
    }
    assert!(seen, "expected %output containing marker");
}
```

- [ ] **Step 5: Run all tests/fmt/clippy** — PASS.
- [ ] **Step 6: Commit** — `feat: control-mode transport with framing, events, pause handling`.

---

### Task 9: TmuxDriver base — channels / label / read + transport supervision

**Files:**
- Create: rewrite `src/drivers/tmux/mod.rs` (keep `pub mod protocol; pub mod transport; pub mod control;`, add `TmuxDriver`)

**Interfaces:**
- Consumes: everything above.
- Produces: `TmuxDriver::new(cfg: TmuxCfg) -> Arc<TmuxDriver>` — bootstraps via `OneShotCli::from_env()`, resolves `own_context`, then tries `ControlMode::attach`; on failure stays on CLI (records `transport: "cli-fallback"` for doctor). Implements `Driver`:
  - `info()` → id `"tmux"`, kind Messaging… (`Both` — it also has invoke ops).
  - `channels()` → `list-panes -a -F '#{pane_id}|#{session_name}:#{window_index}.#{pane_index}|#{pane_current_command}|#{@name}|#{pane_current_path}'`, parsed by pure `parse_pane_line(&str) -> Option<ChannelEntry>` (channel = `tmux:%N`, labels = [@name if set], detail = `{sw, process, cwd}`).
  - `resolve(addr) -> Result<String>`: `%N` passes through; otherwise match against `@name` labels (NotFound with hint otherwise).
  - ops `read {lines:u32=50}` (capture-pane `-p -J -S -<lines>`), `label {name}` (`set-option -p -t <pane> @name <name>`); `ops()` returns full `OpSpec` list for read/keys/label/await_idle/ask with JSON Schemas inline (write them as `serde_json::json!` literals; keys/await_idle/ask specs are declared now, implementations land in Tasks 10–12 returning `Invalid("not yet implemented")` until then).
  - `send/recv` stubs return `Invalid("not yet implemented")` (replaced in Tasks 10/13). `doctor()` reports: tmux version, transport kind, connected flag, pane/session identity.
  - Startup best-effort defaults: `set-option -g history-limit 100000`, `set-option -g mouse on` (ignore failures).

- [ ] **Step 1: Failing unit test for pane-line parsing**

```rust
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
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement driver skeleton**

```rust
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

pub const PANE_FMT: &str =
    "#{pane_id}|#{session_name}:#{window_index}.#{pane_index}|#{pane_current_command}|#{@name}|#{pane_current_path}";

pub fn parse_pane_line(line: &str) -> Option<ChannelEntry> {
    let mut it = line.splitn(5, '|');
    let (pane, sw, proc_, label, cwd) =
        (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    Some(ChannelEntry {
        channel: format!("tmux:{pane}"),
        labels: if label.is_empty() { vec![] } else { vec![label.to_string()] },
        detail: serde_json::json!({ "sw": sw, "process": proc_, "cwd": cwd }),
    })
}

pub struct TmuxDriver {
    cfg: TmuxCfg,
    cli: OneShotCli,
    cm: Mutex<Option<Arc<ControlMode>>>,
    pub own: OwnCtx,
}

impl TmuxDriver {
    pub async fn new(cfg: TmuxCfg) -> Result<Arc<Self>, DriverError> {
        let cli = OneShotCli::from_env();
        let own = own_context(&cli).await?;
        let cm = ControlMode::attach(cli.socket.clone(), &own.session).await.ok();
        let d = Arc::new(Self { cfg, cli, cm: Mutex::new(cm), own });
        // best-effort defaults
        let _ = d.run(&["set-option".into(), "-g".into(), "history-limit".into(), "100000".into()]).await;
        let _ = d.run(&["set-option".into(), "-g".into(), "mouse".into(), "on".into()]).await;
        Ok(d)
    }

    /// Run through CM when connected; fall back to CLI; re-attach lazily.
    pub async fn run(&self, args: &[String]) -> Result<String, DriverError> {
        {
            let guard = self.cm.lock().await;
            if let Some(cm) = guard.as_ref() {
                if cm.is_connected() { return cm.run(args).await; }
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
        self.cm.lock().await.as_ref().filter(|c| c.is_connected()).and_then(|c| c.events())
    }

    pub async fn resolve(&self, addr: &str) -> Result<String, DriverError> {
        if addr.starts_with('%') { return Ok(addr.to_string()); }
        let out = self.run(&["list-panes".into(), "-a".into(), "-F".into(),
            "#{pane_id} #{@name}".into()]).await?;
        for line in out.lines() {
            if let Some((pane, label)) = line.split_once(' ') {
                if label.trim() == addr { return Ok(pane.to_string()); }
            }
        }
        Err(DriverError::new(ErrorKind::NotFound, format!("no pane labeled '{addr}'"))
            .with_hint("run channel_list()"))
    }

    async fn op_read(&self, pane: &str, lines: u32) -> Result<serde_json::Value, DriverError> {
        let out = self.run(&["capture-pane".into(), "-t".into(), pane.into(),
            "-p".into(), "-J".into(), "-S".into(), format!("-{lines}")]).await?;
        Ok(serde_json::json!({ "text": out }))
    }
}

#[async_trait]
impl Driver for TmuxDriver {
    fn info(&self) -> DriverInfo {
        DriverInfo { id: "tmux".into(), kind: DriverKind::Both,
            summary: "cross-pane messaging and pane ops via tmux".into() }
    }

    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError> {
        let out = self.run(&["list-panes".into(), "-a".into(), "-F".into(), PANE_FMT.into()]).await?;
        Ok(out.lines().filter_map(parse_pane_line).collect())
    }

    fn ops(&self) -> Vec<OpSpec> {
        let s = |v: serde_json::Value| v;
        vec![
            OpSpec { op: "read".into(), summary: "capture last N lines of a pane".into(),
                params_schema: s(serde_json::json!({"type":"object","properties":{"lines":{"type":"integer","default":50}}})),
                result_schema: s(serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}})) },
            OpSpec { op: "keys".into(), summary: "send special keys (Enter, Escape, C-c); requires read within 60s".into(),
                params_schema: s(serde_json::json!({"type":"object","required":["keys"],"properties":{"keys":{"type":"array","items":{"type":"string"}}}})),
                result_schema: s(serde_json::json!({"type":"object"})) },
            OpSpec { op: "label".into(), summary: "set pane @name label".into(),
                params_schema: s(serde_json::json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}}})),
                result_schema: s(serde_json::json!({"type":"object"})) },
            OpSpec { op: "await_idle".into(), summary: "wait until pane output is quiet".into(),
                params_schema: s(serde_json::json!({"type":"object","properties":{"quiet_ms":{"type":"integer","default":1500},"timeout_ms":{"type":"integer","default":60000}}})),
                result_schema: s(serde_json::json!({"type":"object","properties":{"idle":{"type":"boolean"}}})) },
            OpSpec { op: "ask".into(), summary: "send + await_idle + capture transcript delta".into(),
                params_schema: s(serde_json::json!({"type":"object","required":["message"],"properties":{"message":{"type":"string"},"quiet_ms":{"type":"integer","default":1500},"timeout_ms":{"type":"integer","default":120000}}})),
                result_schema: s(serde_json::json!({"type":"object","properties":{"transcript":{"type":"string"},"receipt":{"type":"object"}}})) },
        ]
    }

    async fn send(&self, _addr: &str, _msg: SendRequest) -> Result<SendReceipt, DriverError> {
        Err(DriverError::new(ErrorKind::Invalid, "not yet implemented"))
    }

    async fn invoke(&self, addr: &str, op: &str, args: serde_json::Value)
        -> Result<serde_json::Value, DriverError> {
        let pane = self.resolve(addr).await?;
        match op {
            "read" => {
                let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
                self.op_read(&pane, lines).await
            }
            "label" => {
                let name = args.get("name").and_then(|v| v.as_str())
                    .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "label requires 'name'"))?;
                self.run(&["set-option".into(), "-p".into(), "-t".into(), pane.clone(),
                    "@name".into(), name.into()]).await?;
                Ok(serde_json::json!({ "labeled": pane, "name": name }))
            }
            "keys" | "await_idle" | "ask" =>
                Err(DriverError::new(ErrorKind::Invalid, "not yet implemented")),
            other => Err(DriverError::new(ErrorKind::NotFound, format!("no op '{other}'"))
                .with_hint("run channel_describe(\"tmux:*\")")),
        }
    }

    async fn recv(&self, _cursor: Option<u64>) -> Result<RecvBatch, DriverError> {
        Ok(RecvBatch { items: vec![], next_cursor: 0 })
    }

    async fn doctor(&self) -> DoctorReport {
        let mut lines = vec![];
        let mut ok = true;
        match self.run(&["display-message".into(), "-p".into(), "#{version}".into()]).await {
            Ok(v) => lines.push(format!("tmux version:  {}", v.trim())),
            Err(e) => { ok = false; lines.push(format!("tmux:          UNREACHABLE ({})", e.message)); }
        }
        let cm_up = self.cm.lock().await.as_ref().map(|c| c.is_connected()).unwrap_or(false);
        lines.push(format!("transport:     {}", if cm_up { "control-mode" } else { "cli-fallback" }));
        lines.push(format!("own pane:      {} (session {})", self.own.pane, self.own.session));
        DoctorReport { driver: "tmux".into(), ok, lines }
    }
}
```

- [ ] **Step 4: Integration test — channels + label + read round-trip**

```rust
use cc_uplink::core::driver::Driver;

#[tokio::test]
async fn driver_channels_label_read() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    // Run the driver against the private server: point $TMUX/$TMUX_PANE at it.
    // own_context needs TMUX_PANE; use the first pane of session "it".
    let pane = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]).trim().to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &pane);
    }
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default()).await.unwrap();
    let chans = d.channels().await.unwrap();
    assert!(chans.iter().any(|c| c.channel == format!("tmux:{pane}")));

    d.invoke(&pane, "label", serde_json::json!({"name":"selfpane"})).await.unwrap();
    let resolved = d.resolve("selfpane").await.unwrap();
    assert_eq!(resolved, pane);

    srv.run(&["send-keys", "-t", "it", "-l", "echo read-marker"]);
    srv.run(&["send-keys", "-t", "it", "Enter"]);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let out = d.invoke(&pane, "read", serde_json::json!({"lines": 20})).await.unwrap();
    assert!(out["text"].as_str().unwrap().contains("read-marker"));
}
```

Note: env-var tests mutate process env — mark this test `#[serial]`? No extra crate: keep all env-mutating integration tests in ONE test function or run with `--test-threads=1` in CI (`cargo test --test tmux_integration -- --test-threads=1`). Set that flag in `.gitlab-ci.yml` now.

- [ ] **Step 5: Run tests (`cargo test --test tmux_integration -- --test-threads=1`), fmt, clippy** — PASS.
- [ ] **Step 6: Commit** — `feat: tmux driver base (channels, resolve, label, read, doctor)`.

---

### Task 10: Send cycle (policy + inject + verify + Enter + receipt)

**Files:**
- Modify: `src/drivers/tmux/mod.rs` (replace `send` stub; add helpers)

**Interfaces:**
- Consumes: `envelope::{new_correlation_id, format_outbound}`, `protocol::strip_ansi`, events.
- Produces: real `Driver::send`. Behavior (spec §5.2, exact):
  1. `resolve(addr)`; reject if pane == own pane (`Rejected`, "cannot send to own pane").
  2. If `cfg.allowlist` is Some, target pane or its label must be in it (`Rejected` otherwise).
  3. Reject messages containing control chars incl. `\n` (`Invalid`, hint "single-line messages only in v1").
  4. Build envelope; inject `send-keys -t <pane> -l -- <envelope>`.
  5. Verify: subscribe events BEFORE injecting when transport events available and target session == own session; wait ≤1500 ms for the correlation token `id:<8hex>` in `strip_ansi(accumulated)`. Else (or on miss): capture-pane `-p -J -S -5` once after 300 ms and search token.
  6. Verified ⇒ `send-keys -t <pane> Enter`; receipt `{delivered:true, verify_excerpt}`. Not verified ⇒ `Err(Timeout)` with evidence = last 200 chars of capture; DO NOT press Enter, DO NOT retry.

- [ ] **Step 1: Failing integration test**

```rust
#[tokio::test]
async fn send_delivers_and_verifies() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let own = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]).trim().to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    // second pane runs `cat` — echoes what it receives
    srv.run(&["split-window", "-t", "it", "-d", "cat"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes.lines().find(|p| p.trim() != own).unwrap().trim().to_string();

    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default()).await.unwrap();
    let r = d.send(&target, cc_uplink::core::driver::SendRequest {
        message: "ping from claude".into(),
        reply_hint: cc_uplink::core::driver::ReplyHint::None,
    }).await.unwrap();
    assert!(r.delivered);
    assert_eq!(r.correlation_id.len(), 8);

    // sending to own pane is rejected
    let e = d.send(&own, cc_uplink::core::driver::SendRequest {
        message: "loop".into(), reply_hint: cc_uplink::core::driver::ReplyHint::None,
    }).await.unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Rejected));

    // multiline is invalid
    let e = d.send(&target, cc_uplink::core::driver::SendRequest {
        message: "a\nb".into(), reply_hint: cc_uplink::core::driver::ReplyHint::None,
    }).await.unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Invalid));
}
```

- [ ] **Step 2: Run to fail** (send stub returns Invalid).

- [ ] **Step 3: Implement**

```rust
impl TmuxDriver {
    fn check_allowlist(&self, pane: &str, addr: &str) -> Result<(), DriverError> {
        if let Some(list) = &self.cfg.allowlist {
            if !list.iter().any(|x| x == pane || x == addr) {
                return Err(DriverError::new(ErrorKind::Rejected,
                    format!("target '{addr}' not in allowlist")));
            }
        }
        Ok(())
    }

    async fn target_session(&self, pane: &str) -> Result<String, DriverError> {
        Ok(self.run(&["display-message".into(), "-p".into(), "-t".into(), pane.into(),
            "#{session_name}".into()]).await?.trim().to_string())
    }

    async fn verify_token(&self, pane: &str, token: &str,
        mut rx: Option<tokio::sync::broadcast::Receiver<transport::PaneEvent>>) -> Option<String> {
        if let Some(rx) = rx.as_mut() {
            let mut acc: Vec<u8> = vec![];
            let deadline = tokio::time::Instant::now() + Duration::from_millis(1500);
            while tokio::time::Instant::now() < deadline {
                match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                    Ok(Ok(ev)) if ev.pane == pane => {
                        acc.extend_from_slice(&ev.data);
                        let clean = protocol::strip_ansi(&acc);
                        if clean.contains(token) { return Some(token.to_string()); }
                    }
                    Ok(Ok(_)) => {}
                    _ => {}
                }
            }
        }
        // capture fallback
        tokio::time::sleep(Duration::from_millis(300)).await;
        let cap = self.run(&["capture-pane".into(), "-t".into(), pane.into(),
            "-p".into(), "-J".into(), "-S".into(), "-5".into()]).await.ok()?;
        cap.contains(token).then(|| token.to_string())
    }
}
```

Replace the `send` stub:

```rust
    async fn send(&self, addr: &str, msg: SendRequest) -> Result<SendReceipt, DriverError> {
        if msg.message.chars().any(|c| c.is_control()) {
            return Err(DriverError::new(ErrorKind::Invalid, "message contains control characters")
                .with_hint("single-line messages only in v1"));
        }
        let pane = self.resolve(addr).await?;
        if pane == self.own.pane {
            return Err(DriverError::new(ErrorKind::Rejected, "cannot send to own pane (loop prevention)"));
        }
        self.check_allowlist(&pane, addr)?;

        let id = crate::core::envelope::new_correlation_id();
        let from = self.own.label.clone().unwrap_or_else(|| self.own.pane.clone());
        let text = crate::core::envelope::format_outbound(&from, &self.own.pane, &id,
            &msg.message, msg.reply_hint);
        let token = format!("id:{id}");

        let same_session = self.target_session(&pane).await? == self.own.session;
        let rx = if same_session { self.events().await } else { None };

        self.run(&["send-keys".into(), "-t".into(), pane.clone(), "-l".into(),
            "--".into(), text.clone()]).await?;

        let verified = self.verify_token(&pane, &token, rx).await;
        match verified {
            Some(excerpt) => {
                self.run(&["send-keys".into(), "-t".into(), pane.clone(), "Enter".into()]).await?;
                Ok(SendReceipt {
                    delivered: true,
                    correlation_id: id,
                    verify_excerpt: Some(excerpt),
                    injected_at: now_rfc3339(),
                })
            }
            None => {
                let cap = self.run(&["capture-pane".into(), "-t".into(), pane, "-p".into(),
                    "-S".into(), "-5".into()]).await.unwrap_or_default();
                let tail: String = cap.chars().rev().take(200).collect::<String>()
                    .chars().rev().collect();
                Err(DriverError::new(ErrorKind::Timeout, "could not verify injected text")
                    .with_evidence(tail)
                    .with_hint("target TUI may have consumed input; inspect with read op"))
            }
        }
    }
```

Add helper (module level):

```rust
pub fn now_rfc3339() -> String {
    // avoid a chrono dependency: seconds-precision UTC from SystemTime
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    // days-since-epoch → y/m/d (civil algorithm), hh:mm:ss from remainder
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
```

(Unit-test `now_rfc3339` shape: `assert!(now_rfc3339().ends_with('Z') && now_rfc3339().len() == 20);`)

- [ ] **Step 4: Run all tests/fmt/clippy** — PASS.
- [ ] **Step 5: Commit** — `feat: mechanized send cycle with token verify and receipts`.

---

### Task 11: keys op + in-process read-guard

**Files:**
- Modify: `src/drivers/tmux/mod.rs`

**Interfaces:**
- Produces: `keys` op live. Guard: `Mutex<HashMap<String, std::time::Instant>>` field `read_marks`; `op_read` and `ask` record `Instant::now()` for the pane; `keys` requires a mark younger than 60 s (`Rejected` with hint `"invoke read on this pane first"`). `keys` also refuses own pane and applies allowlist. Each key sent via separate `send-keys -t <pane> <key>` (no `-l`).

- [ ] **Step 1: Failing unit test for the guard logic** (extract pure helper)

```rust
#[cfg(test)]
mod guard_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn guard_accepts_fresh_rejects_stale_and_missing() {
        assert!(guard_ok(Some(Instant::now()), Duration::from_secs(60)));
        assert!(!guard_ok(Some(Instant::now() - Duration::from_secs(61)), Duration::from_secs(60)));
        assert!(!guard_ok(None, Duration::from_secs(60)));
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
pub fn guard_ok(mark: Option<std::time::Instant>, ttl: std::time::Duration) -> bool {
    mark.map(|t| t.elapsed() <= ttl).unwrap_or(false)
}
```

Add field `read_marks: Mutex<std::collections::HashMap<String, std::time::Instant>>` to `TmuxDriver` (init empty in `new`). In `op_read` after success: `self.read_marks.lock().await.insert(pane.to_string(), std::time::Instant::now());`. Implement `keys` in `invoke`:

```rust
            "keys" => {
                if pane == self.own.pane {
                    return Err(DriverError::new(ErrorKind::Rejected, "cannot send keys to own pane"));
                }
                self.check_allowlist(&pane, addr)?;
                let mark = self.read_marks.lock().await.get(&pane).copied();
                if !guard_ok(mark, Duration::from_secs(60)) {
                    return Err(DriverError::new(ErrorKind::Rejected, "read guard: pane not read recently")
                        .with_hint("invoke read on this pane first"));
                }
                let keys = args.get("keys").and_then(|v| v.as_array())
                    .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "keys requires 'keys' array"))?;
                for k in keys {
                    let k = k.as_str().ok_or_else(|| DriverError::new(ErrorKind::Invalid, "keys must be strings"))?;
                    self.run(&["send-keys".into(), "-t".into(), pane.clone(), k.into()]).await?;
                }
                Ok(serde_json::json!({ "sent": keys.len() }))
            }
```

- [ ] **Step 4: Integration test** — read then keys succeeds; keys without read is Rejected:

```rust
#[tokio::test]
async fn keys_requires_recent_read() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let own = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]).trim().to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    srv.run(&["split-window", "-t", "it", "-d", "cat"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes.lines().find(|p| p.trim() != own).unwrap().trim().to_string();
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default()).await.unwrap();

    let e = d.invoke(&target, "keys", serde_json::json!({"keys":["Enter"]})).await.unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Rejected));

    d.invoke(&target, "read", serde_json::json!({"lines":5})).await.unwrap();
    d.invoke(&target, "keys", serde_json::json!({"keys":["Enter"]})).await.unwrap();
}
```

- [ ] **Step 5: Run/fmt/clippy PASS; Commit** — `feat: keys op with in-process read guard`.

---

### Task 12: await_idle + ask

**Files:**
- Modify: `src/drivers/tmux/mod.rs`

**Interfaces:**
- Produces:
  - `await_idle {quiet_ms=1500, timeout_ms=60000}` → `{idle:true, waited_ms}` or `Err(Timeout)`. Same-session with events: idle = no `%output` for the pane during `quiet_ms`. Fallback (no events / cross-session): poll `display-message -p -t <pane> '#{history_size}:#{cursor_x},#{cursor_y}'` every 300 ms; idle when unchanged for `quiet_ms`.
  - `ask {message, quiet_ms=1500, timeout_ms=120000}` → `{transcript, receipt}`:
    1. watermark `H0` = history_size (display-message).
    2. full send cycle (reuse `Driver::send` semantics with ReplyHint::Full — call internal send fn).
    3. `await_idle`.
    4. `H1` = history_size; capture `-S -(H1-H0+pane_height)` … `-p -J`; slice transcript from the first line containing the correlation token; return receipt from step 2.
  - `ask` records a read-mark (it observed the pane).

- [ ] **Step 1: Failing integration test**

```rust
#[tokio::test]
async fn ask_returns_transcript_delta() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let own = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]).trim().to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    // target pane: a shell that will execute what we send after Enter
    srv.run(&["split-window", "-t", "it", "-d", "sh"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes.lines().find(|p| p.trim() != own).unwrap().trim().to_string();
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default()).await.unwrap();

    // 'ask' a shell: the injected envelope is not a valid command (sh prints error),
    // but the transcript delta must contain both the envelope and the shell's reaction.
    let out = d.invoke(&target, "ask", serde_json::json!({
        "message": "echo uplink-ask-answer", "quiet_ms": 800, "timeout_ms": 15000
    })).await.unwrap();
    let t = out["transcript"].as_str().unwrap();
    assert!(t.contains("uplink-ask-answer") || t.contains("[uplink"),
        "transcript should contain the exchange, got: {t}");
    assert!(out["receipt"]["delivered"].as_bool().unwrap());
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
impl TmuxDriver {
    async fn history_size(&self, pane: &str) -> Result<u64, DriverError> {
        let out = self.run(&["display-message".into(), "-p".into(), "-t".into(), pane.into(),
            "#{history_size}".into()]).await?;
        out.trim().parse().map_err(|_| DriverError::new(ErrorKind::Upstream, "bad history_size"))
    }

    async fn pane_height(&self, pane: &str) -> Result<u64, DriverError> {
        let out = self.run(&["display-message".into(), "-p".into(), "-t".into(), pane.into(),
            "#{pane_height}".into()]).await?;
        out.trim().parse().map_err(|_| DriverError::new(ErrorKind::Upstream, "bad pane_height"))
    }

    async fn op_await_idle(&self, pane: &str, quiet_ms: u64, timeout_ms: u64)
        -> Result<serde_json::Value, DriverError> {
        let start = tokio::time::Instant::now();
        let deadline = start + Duration::from_millis(timeout_ms);
        let same = self.target_session(pane).await? == self.own.session;
        let mut rx = if same { self.events().await } else { None };

        if let Some(rx) = rx.as_mut() {
            let mut last = tokio::time::Instant::now();
            loop {
                if tokio::time::Instant::now() > deadline {
                    return Err(DriverError::new(ErrorKind::Timeout, "pane did not become idle"));
                }
                match tokio::time::timeout(Duration::from_millis(quiet_ms), rx.recv()).await {
                    Ok(Ok(ev)) if ev.pane == pane => { last = tokio::time::Instant::now(); }
                    Ok(Ok(_)) => {
                        if last.elapsed() >= Duration::from_millis(quiet_ms) { break; }
                    }
                    Ok(Err(_)) | Err(_) => {
                        if last.elapsed() >= Duration::from_millis(quiet_ms) { break; }
                    }
                }
            }
        } else {
            let mut last_probe = String::new();
            let mut stable_since = tokio::time::Instant::now();
            loop {
                if tokio::time::Instant::now() > deadline {
                    return Err(DriverError::new(ErrorKind::Timeout, "pane did not become idle"));
                }
                let probe = self.run(&["display-message".into(), "-p".into(), "-t".into(), pane.into(),
                    "#{history_size}:#{cursor_x},#{cursor_y}".into()]).await?;
                if probe == last_probe {
                    if stable_since.elapsed() >= Duration::from_millis(quiet_ms) { break; }
                } else {
                    last_probe = probe;
                    stable_since = tokio::time::Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
        Ok(serde_json::json!({ "idle": true, "waited_ms": start.elapsed().as_millis() as u64 }))
    }

    async fn op_ask(&self, addr: &str, pane: &str, args: &serde_json::Value)
        -> Result<serde_json::Value, DriverError> {
        let message = args.get("message").and_then(|v| v.as_str())
            .ok_or_else(|| DriverError::new(ErrorKind::Invalid, "ask requires 'message'"))?;
        let quiet_ms = args.get("quiet_ms").and_then(|v| v.as_u64()).unwrap_or(1500);
        let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(120_000);

        let h0 = self.history_size(pane).await?;
        let receipt = Driver::send(self, addr, SendRequest {
            message: message.to_string(),
            reply_hint: ReplyHint::Full,
        }).await?;
        self.op_await_idle(pane, quiet_ms, timeout_ms).await?;

        let h1 = self.history_size(pane).await?;
        let height = self.pane_height(pane).await?;
        let back = (h1.saturating_sub(h0)) + height;
        let cap = self.run(&["capture-pane".into(), "-t".into(), pane.into(),
            "-p".into(), "-J".into(), "-S".into(), format!("-{back}")]).await?;
        let token = format!("id:{}", receipt.correlation_id);
        let transcript = match cap.find(&token) {
            Some(pos) => {
                let line_start = cap[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
                cap[line_start..].to_string()
            }
            None => cap,
        };
        self.read_marks.lock().await.insert(pane.to_string(), std::time::Instant::now());
        Ok(serde_json::json!({ "transcript": transcript, "receipt": receipt }))
    }
}
```

Wire into `invoke` (replace the two stub arms):

```rust
            "await_idle" => {
                let quiet = args.get("quiet_ms").and_then(|v| v.as_u64()).unwrap_or(1500);
                let to = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(60_000);
                self.op_await_idle(&pane, quiet, to).await
            }
            "ask" => self.op_ask(addr, &pane, &args).await,
```

- [ ] **Step 4: Run all tests/fmt/clippy** — PASS.
- [ ] **Step 5: Commit** — `feat: await_idle and ask ops with history watermark transcript`.

---

### Task 13: recv (inbound envelope watcher) + JSONL log sink

**Files:**
- Create: `src/core/logsink.rs` (add `pub mod logsink;`)
- Modify: `src/drivers/tmux/mod.rs` (watcher task + ring buffer + real `recv`)

**Interfaces:**
- Produces:
  - `logsink::LogSink::open() -> LogSink` (append JSONL at `dirs::state_dir()/cc-uplink/log.jsonl`, fallback `dirs::data_local_dir()`, best-effort — never errors); `.append(entry: &serde_json::Value)`; `logsink::log_path() -> Option<PathBuf>` (shared with `cc-uplink log`).
  - TmuxDriver: background task (spawned in `new` when events available) watches own-pane `%output`, line-buffers `strip_ansi`'d text, feeds complete lines to `envelope::parse_inbound`; hits append `RecvItem` to `Mutex<VecDeque<RecvItem>>` (cap 1000) with monotonically increasing cursor, and append `{dir:"in", ...}` to LogSink. `Driver::recv(cursor)` drains items with `item.cursor >= cursor.unwrap_or(0)`, `next_cursor` = last+1. `Driver::send` success additionally appends `{dir:"out", channel, id, excerpt}` to LogSink.

- [ ] **Step 1: Failing unit test for the inbound line-buffer** (pure)

```rust
#[cfg(test)]
mod recv_tests {
    use super::*;

    #[test]
    fn line_buffer_emits_complete_lines_only() {
        let mut lb = LineBuffer::new();
        assert!(lb.push(b"[reply id:ab12cd34] par").is_empty());
        let lines = lb.push(b"tial answer\nnext");
        assert_eq!(lines, vec!["[reply id:ab12cd34] partial answer".to_string()]);
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

`LineBuffer` in tmux mod:

```rust
pub struct LineBuffer { buf: String }

impl LineBuffer {
    pub fn new() -> Self { Self { buf: String::new() } }
    pub fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.buf.push_str(&protocol::strip_ansi(data));
        let mut out = vec![];
        while let Some(i) = self.buf.find(['\n', '\r']) {
            let line: String = self.buf.drain(..=i).collect();
            let line = line.trim_end_matches(['\n', '\r']).to_string();
            if !line.is_empty() { out.push(line); }
        }
        out
    }
}
```

`logsink.rs`:

```rust
use std::io::Write;
use std::path::PathBuf;

pub fn log_path() -> Option<PathBuf> {
    dirs::state_dir().or_else(dirs::data_local_dir).map(|d| d.join("cc-uplink/log.jsonl"))
}

pub struct LogSink { path: Option<PathBuf> }

impl LogSink {
    pub fn open() -> Self {
        let path = log_path();
        if let Some(p) = &path {
            if let Some(dir) = p.parent() { let _ = std::fs::create_dir_all(dir); }
        }
        Self { path }
    }
    pub fn append(&self, entry: &serde_json::Value) {
        let Some(p) = &self.path else { return };
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{entry}");
        }
    }
}
```

Watcher task in `TmuxDriver::new` (after CM attach), fields `inbox: Mutex<VecDeque<RecvItem>>`, `next_cursor: std::sync::atomic::AtomicU64`, `sink: logsink::LogSink`:

```rust
        if let Some(mut rx) = d.events().await {
            let dd = d.clone();
            tokio::spawn(async move {
                let mut lb = LineBuffer::new();
                loop {
                    match rx.recv().await {
                        Ok(ev) if ev.pane == dd.own.pane => {
                            for line in lb.push(&ev.data) {
                                if let Some(inb) = crate::core::envelope::parse_inbound(&line) {
                                    let cursor = dd.next_cursor.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                    let item = RecvItem {
                                        cursor,
                                        at: now_rfc3339(),
                                        from: inb.from.clone(),
                                        id: inb.id.clone(),
                                        raw: line.clone(),
                                    };
                                    dd.sink.append(&serde_json::json!({
                                        "ts": item.at, "dir": "in", "from": item.from,
                                        "id": item.id, "raw": item.raw }));
                                    let mut q = dd.inbox.lock().await;
                                    if q.len() >= 1000 { q.pop_front(); }
                                    q.push_back(item);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });
        }
```

Real `recv`:

```rust
    async fn recv(&self, cursor: Option<u64>) -> Result<RecvBatch, DriverError> {
        let q = self.inbox.lock().await;
        let from = cursor.unwrap_or(0);
        let items: Vec<RecvItem> = q.iter().filter(|i| i.cursor >= from).cloned().collect();
        let next_cursor = items.last().map(|i| i.cursor + 1).unwrap_or(from);
        Ok(RecvBatch { items, next_cursor })
    }
```

In `send` success path, before returning: `self.sink.append(&serde_json::json!({"ts": receipt-time, "dir":"out", "channel": format!("tmux:{addr}"), "id": id, "excerpt": &msg.message}));`

- [ ] **Step 4: Integration test** — inject a reply-shaped line into OWN pane from outside, then `recv`:

```rust
#[tokio::test]
async fn recv_sees_inbound_reply_envelope() {
    let Some(srv) = common::TmuxTestServer::start() else { return };
    let own = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]).trim().to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default()).await.unwrap();
    // peer-style reply: typed into our pane, then Enter → shell echoes the line into pane output
    srv.run(&["send-keys", "-t", &own, "-l", "echo '[reply id:cafe0001] done'"]);
    srv.run(&["send-keys", "-t", &own, "Enter"]);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let batch = cc_uplink::core::driver::Driver::recv(&*d, None).await.unwrap();
    assert!(batch.items.iter().any(|i| i.id.as_deref() == Some("cafe0001")));
}
```

- [ ] **Step 5: Run/fmt/clippy PASS; Commit** — `feat: inbound envelope recv buffer and jsonl conversation log`.

---

### Task 14: MCP layer (six tools) + serve

**Files:**
- Create: `src/mcp.rs` (add `pub mod mcp;` to lib.rs)
- Modify: `src/main.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `Registry`, `Config`, `TmuxDriver`.
- Produces: `mcp::serve() -> anyhow::Result<()>`: builds Config → Registry (register TmuxDriver when enabled and construction succeeds; a failed driver becomes a doctor-visible absence, not a crash), then serves rmcp over stdio with EXACTLY the six tools. Each tool returns text content; success = pretty JSON of the DTO; driver errors render via `DriverError::render(driver_id)` as MCP tool errors.

- [ ] **Step 1: Add dependency**

Run: `cargo add rmcp --features server,transport-io && cargo add schemars`
(rmcp's macro API moves between versions; the mapping below targets the current `tool_router`/`tool_handler` macro set. If attribute names differ on the resolved version, adapt inside this file only — the boundary is designed so drift stays contained in `mcp.rs`.)

- [ ] **Step 2: Failing test** — tool logic is thin; test the shared result-mapping helper:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_driver_error_to_rendered_text() {
        let e = crate::error::DriverError::new(crate::error::ErrorKind::NotFound, "x");
        let s = render_result::<serde_json::Value>(Err(e), "tmux").unwrap_err();
        assert!(s.contains("uplink error [tmux:NotFound]: x"));
    }

    #[test]
    fn maps_ok_to_pretty_json() {
        let v = serde_json::json!({"a": 1});
        let s = render_result(Ok(v), "tmux").unwrap();
        assert!(s.contains("\"a\": 1"));
    }
}
```

- [ ] **Step 3: Implement**

```rust
use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::core::driver::SendRequest;
use crate::core::registry::Registry;
use crate::error::DriverError;

pub fn render_result<T: serde::Serialize>(r: Result<T, DriverError>, driver: &str)
    -> Result<String, String> {
    match r {
        Ok(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string())),
        Err(e) => Err(e.render(driver)),
    }
}

fn driver_of(addr: &str) -> &str { addr.split(':').next().unwrap_or("core") }

#[derive(Deserialize, JsonSchema)]
pub struct DescribeParams { pub channel: String, pub op: Option<String> }

#[derive(Deserialize, JsonSchema)]
pub struct SendParams {
    pub channel: String,
    pub message: String,
    /// full | short | none — how much reply instruction to embed
    pub reply_hint: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct InvokeParams { pub channel: String, pub op: String, pub args: Option<serde_json::Value> }

#[derive(Deserialize, JsonSchema)]
pub struct RecvParams { pub cursor: Option<u64> }

#[derive(Clone)]
pub struct Uplink {
    reg: Arc<Registry>,
    tool_router: ToolRouter<Self>,
}

fn text_ok(s: String) -> CallToolResult { CallToolResult::success(vec![Content::text(s)]) }
fn text_err(s: String) -> CallToolResult { CallToolResult::error(vec![Content::text(s)]) }

#[tool_router]
impl Uplink {
    pub fn new(reg: Arc<Registry>) -> Self {
        Self { reg, tool_router: Self::tool_router() }
    }

    #[tool(description = "List all channels across drivers with capability summaries")]
    async fn channel_list(&self) -> CallToolResult {
        let all = self.reg.list_all().await;
        let mut lines = vec![];
        for (info, chans) in all {
            lines.push(format!("# driver {} — {}", info.id, info.summary));
            for c in chans {
                lines.push(format!("{} | labels:{:?} | {}", c.channel, c.labels, c.detail));
            }
        }
        text_ok(if lines.is_empty() { "no channels".into() } else { lines.join("\n") })
    }

    #[tool(description = "Get the JSON Schema for a channel op (call before first invoke of an op)")]
    async fn channel_describe(&self, Parameters(p): Parameters<DescribeParams>) -> CallToolResult {
        match self.reg.driver_for(&p.channel) {
            Ok((d, _)) => {
                let ops = d.ops();
                let sel: Vec<_> = ops.into_iter()
                    .filter(|o| p.op.as_deref().map(|x| x == o.op).unwrap_or(true))
                    .collect();
                text_ok(serde_json::to_string_pretty(&sel).unwrap_or_default())
            }
            Err(e) => text_err(e.render(driver_of(&p.channel))),
        }
    }

    #[tool(description = "Send an async message to a channel (tmux: full inject+verify+Enter cycle)")]
    async fn channel_send(&self, Parameters(p): Parameters<SendParams>) -> CallToolResult {
        let hint = match p.reply_hint.as_deref() {
            Some("short") => crate::core::driver::ReplyHint::Short,
            Some("none") => crate::core::driver::ReplyHint::None,
            _ => crate::core::driver::ReplyHint::Full,
        };
        match self.reg.driver_for(&p.channel) {
            Ok((d, addr)) => match render_result(
                d.send(&addr, SendRequest { message: p.message, reply_hint: hint }).await,
                driver_of(&p.channel)) {
                Ok(s) => text_ok(s),
                Err(s) => text_err(s),
            },
            Err(e) => text_err(e.render(driver_of(&p.channel))),
        }
    }

    #[tool(description = "Invoke a capability op on a channel (see channel_describe for schemas)")]
    async fn channel_invoke(&self, Parameters(p): Parameters<InvokeParams>) -> CallToolResult {
        match self.reg.driver_for(&p.channel) {
            Ok((d, addr)) => match render_result(
                d.invoke(&addr, &p.op, p.args.unwrap_or(serde_json::json!({}))).await,
                driver_of(&p.channel)) {
                Ok(s) => text_ok(s),
                Err(s) => text_err(s),
            },
            Err(e) => text_err(e.render(driver_of(&p.channel))),
        }
    }

    #[tool(description = "Drain inbound message envelopes received since cursor (non-blocking)")]
    async fn channel_recv(&self, Parameters(p): Parameters<RecvParams>) -> CallToolResult {
        let mut items = vec![];
        let mut next = p.cursor.unwrap_or(0);
        for d in self.reg.drivers() {
            if let Ok(batch) = d.recv(p.cursor).await {
                next = next.max(batch.next_cursor);
                items.extend(batch.items);
            }
        }
        text_ok(serde_json::to_string_pretty(&serde_json::json!({
            "items": items, "next_cursor": next })).unwrap_or_default())
    }

    #[tool(description = "Aggregated diagnostics across all drivers")]
    async fn channel_doctor(&self) -> CallToolResult {
        let reports = self.reg.doctor_all().await;
        let mut out = vec!["cc-uplink doctor".to_string(), "---".to_string()];
        for r in &reports {
            out.push(format!("[{}] {}", r.driver, if r.ok { "OK" } else { "DEGRADED" }));
            out.extend(r.lines.iter().map(|l| format!("  {l}")));
        }
        text_ok(out.join("\n"))
    }
}

#[tool_handler]
impl ServerHandler for Uplink {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some("cc-uplink: unified outbound channels. Start with channel_list; \
                call channel_describe before the first invoke of an op; never poll for replies — \
                they arrive in your own pane (channel_recv is an audit log).".into()),
            ..Default::default()
        }
    }
}

pub async fn build_registry() -> Arc<Registry> {
    let cfg = crate::config::Config::load();
    let mut reg = Registry::new();
    if cfg.drivers.tmux.enabled {
        match crate::drivers::tmux::TmuxDriver::new(cfg.drivers.tmux).await {
            Ok(d) => reg.register(d),
            Err(e) => eprintln!("cc-uplink: tmux driver unavailable: {}", e.message),
        }
    }
    Arc::new(reg)
}

pub async fn serve() -> anyhow::Result<()> {
    let reg = build_registry().await;
    let service = Uplink::new(reg).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

`src/main.rs`:

```rust
use cc_uplink::mcp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") | None => mcp::serve().await,
        Some(other) => {
            cc_uplink::cli::run(other, &args[1..]).await
        }
    }
}
```

(`cli::run` arrives in Task 15; for this task's commit, temporarily match only `serve`/`None` and print usage for others: `_ => { eprintln!("usage: cc-uplink [serve|doctor|send|invoke|log]"); std::process::exit(2); }`.)

- [ ] **Step 4: Run all tests/fmt/clippy; manual smoke** — `printf '' | cargo run -- serve` starts and exits on EOF without panic.
- [ ] **Step 5: Commit** — `feat: rmcp server exposing the six fixed channel tools`.

---

### Task 15: CLI — doctor / send / invoke / log

**Files:**
- Create: `src/cli/mod.rs` (add `pub mod cli;` to lib.rs)
- Modify: `src/main.rs` (route to `cli::run`)

**Interfaces:**
- Consumes: `mcp::build_registry`, `logsink::log_path`.
- Produces: `cli::run(cmd: &str, rest: &[String]) -> anyhow::Result<()>`:
  - `doctor` — print `channel_doctor`-equivalent text; exit code 1 if any report not ok.
  - `send <channel> <message...>` — send with default Full hint; print receipt JSON.
  - `invoke <channel> <op> [json-args]` — parse optional JSON args; print result JSON.
  - `log [--follow]` — print each JSONL line as `ts dir from/channel id raw-or-excerpt`; `--follow` polls the file every 500 ms for appended lines.

- [ ] **Step 1: Failing unit test** for the log-line formatter (pure):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_in_and_out_lines() {
        let inl = format_log_line(&serde_json::json!({
            "ts":"T","dir":"in","from":"codex","id":"cafe0001","raw":"[reply id:cafe0001] done"}));
        assert_eq!(inl, "T  in  codex           cafe0001 [reply id:cafe0001] done");
        let outl = format_log_line(&serde_json::json!({
            "ts":"T","dir":"out","channel":"tmux:codex","id":"ab12cd34","excerpt":"ping"}));
        assert_eq!(outl, "T  out tmux:codex      ab12cd34 ping");
    }
}
```

- [ ] **Step 2: Run to fail.**

- [ ] **Step 3: Implement**

```rust
use crate::core::driver::{ReplyHint, SendRequest};

pub fn format_log_line(v: &serde_json::Value) -> String {
    let ts = v["ts"].as_str().unwrap_or("?");
    let dir = v["dir"].as_str().unwrap_or("?");
    let who = v["from"].as_str().or(v["channel"].as_str()).unwrap_or("-");
    let id = v["id"].as_str().unwrap_or("-");
    let body = v["raw"].as_str().or(v["excerpt"].as_str()).unwrap_or("");
    format!("{ts}  {dir:<3} {who:<15} {id} {body}")
}

pub async fn run(cmd: &str, rest: &[String]) -> anyhow::Result<()> {
    match cmd {
        "doctor" => {
            let reg = crate::mcp::build_registry().await;
            let reports = reg.doctor_all().await;
            let mut ok = true;
            println!("cc-uplink doctor\n---");
            for r in &reports {
                ok &= r.ok;
                println!("[{}] {}", r.driver, if r.ok { "OK" } else { "DEGRADED" });
                for l in &r.lines { println!("  {l}"); }
            }
            if reports.is_empty() { println!("(no drivers active)"); ok = false; }
            if !ok { std::process::exit(1); }
            Ok(())
        }
        "send" => {
            let (channel, msg) = match rest {
                [c, m @ ..] if !m.is_empty() => (c.clone(), m.join(" ")),
                _ => { eprintln!("usage: cc-uplink send <channel> <message...>"); std::process::exit(2); }
            };
            let reg = crate::mcp::build_registry().await;
            let (d, addr) = reg.driver_for(&channel).map_err(|e| anyhow::anyhow!(e.render("core")))?;
            let r = d.send(&addr, SendRequest { message: msg, reply_hint: ReplyHint::Full }).await
                .map_err(|e| anyhow::anyhow!(e.render(channel.split(':').next().unwrap_or("?"))))?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        "invoke" => {
            let (channel, op, args) = match rest {
                [c, o] => (c.clone(), o.clone(), serde_json::json!({})),
                [c, o, j] => (c.clone(), o.clone(), serde_json::from_str(j)?),
                _ => { eprintln!("usage: cc-uplink invoke <channel> <op> [json-args]"); std::process::exit(2); }
            };
            let reg = crate::mcp::build_registry().await;
            let (d, addr) = reg.driver_for(&channel).map_err(|e| anyhow::anyhow!(e.render("core")))?;
            let out = d.invoke(&addr, &op, args).await
                .map_err(|e| anyhow::anyhow!(e.render(channel.split(':').next().unwrap_or("?"))))?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        "log" => {
            let follow = rest.iter().any(|a| a == "--follow");
            let Some(path) = crate::core::logsink::log_path() else {
                eprintln!("no state dir available"); std::process::exit(1);
            };
            let mut offset = 0u64;
            loop {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let bytes = content.len() as u64;
                    if bytes > offset {
                        for line in content[offset as usize..].lines() {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                                println!("{}", format_log_line(&v));
                            }
                        }
                        offset = bytes;
                    }
                }
                if !follow { break; }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Ok(())
        }
        other => {
            eprintln!("unknown command '{other}'\nusage: cc-uplink [serve|doctor|send|invoke|log]");
            std::process::exit(2);
        }
    }
}
```

Update `main.rs` to route non-serve commands to `cli::run` (replace the temporary usage arm from Task 14).

- [ ] **Step 4: Run all tests/fmt/clippy; manual smoke inside tmux** — in a real tmux session: `cargo run -- doctor` shows tmux OK + transport; `cargo run -- invoke tmux:%<other> read '{"lines":10}'` prints text.
- [ ] **Step 5: Commit** — `feat: human CLI (doctor, send, invoke, log)`.

---

## Self-Review (run after Task 15)

1. **Spec coverage (M1+M2):** six tools ✓ (Task 14); wire-shaped trait + serde-only DTOs ✓ (Task 2); envelope v2 with reply_hint tiers ✓ (Task 3); CM hub: framing/`%output`/pause/greeting-discard ✓ (Task 8), reconnect = lazy re-attach + CLI fallback ✓ (Task 9 `run`); send cycle exact order + no-retry + evidence ✓ (Task 10); keys guard in-process 60 s ✓ (Task 11); await_idle event/poll dual path + ask watermark ✓ (Task 12); recv cursor buffer + JSONL log ✓ (Task 13); CLI four subcommands ✓ (Task 15); startup defaults history-limit/mouse ✓ (Task 9). Deferred per plan scope: image drivers (M3/M4 plan), skill/setup/dist (M5 plan), `refresh-client -B` cross-session await approximation is implemented as display-message polling (documented deviation — simpler, same semantics; revisit if polling proves too coarse).
2. **Placeholder scan:** stubs in Tasks 9 explicitly replaced in 10–13; no TBDs remain.
3. **Type consistency:** `Driver` signatures identical across Tasks 2/4/9–13; `RecvItem.cursor: u64` consistent; `render()` format identical in Tasks 1/14/15.

## Execution

Work happens in `/synosrc/misc/cc-uplink`. Every task ends with fmt + clippy + test green and a commit with the `Signed-off-by: tonyhu <tonyhu@synology.com>` trailer (never a Claude-Session trailer).
