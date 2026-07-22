# cc-uplink v1 — Image Drivers + Release (M3+M4+M5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `image:openai` (direct Images API) and `image:codex` (Codex CLI subprocess) drivers behind the existing six fixed tools, then ship v1: companion skill, `setup` subcommand, LICENSE/README/contract docs, and release automation configs.

**Architecture:** One new registry driver with id `image` (composite over internal `ImageBackend`s, so `image:openai` / `image:codex` both route through the single `<driver>:<address>` scheme without touching the Registry). OpenAI backend = reqwest(rustls) against the Images API with wiremock-tested golden requests. Codex backend = `codex exec` subprocess with argv-contract + `SAVED:` stdout contract, tested against a fake `codex` script. M5 is packaging: embedded companion skill installed by `cc-uplink setup`, docs, release-plz + cargo-dist configs.

**Tech Stack:** Existing M1/M2 crate (Rust edition 2024, MSRV 1.85, tokio, rmcp 2.2.0). New: reqwest 0.12 (default-features off, `rustls-tls,json,multipart`), base64 0.22. Dev: wiremock 0.6, tempfile (already present).

**Spec:** `docs/superpowers/specs/2026-07-22-cc-uplink-design.md` (approved) — §6 image-openai, §7 image-codex, §9 CLI (`setup`), §10 companion skill, §11 config, §13 testing, §14 distribution.

**Branch:** create `feat/v1-image-release` from `main` (`340be00`) before Task 1.

## Global Constraints

- Six MCP tools, exact names: `channel_list`, `channel_describe`, `channel_send`, `channel_invoke`, `channel_recv`, `channel_doctor`. **Adding these two drivers must add zero tools.**
- Channel addressing stays `<driver>:<address>`: `image:openai`, `image:codex`. Registry routing (`Registry::driver_for`, split at first `:`) is NOT modified; the composite `ImageDriver` (id `image`) resolves the backend from the address part.
- Driver trait I/O: serde-serializable types only. The `Driver` trait itself is NOT changed by this plan.
- reqwest with **rustls only** — "no OpenSSL anywhere in the dependency tree" (spec §6). Gate: `cargo tree -e normal | grep -i openssl` must output nothing.
- Secrets env-only: `api_key_env` names the variable; **the key is never stored in config** and never printed (not in doctor lines, not in errors, not in logs).
- OpenAI output naming, verbatim from spec: default out dir `./uplink-images/`, file name `<UTC-ts>-<n>.png`; results return **absolute** paths.
- Codex subprocess contract, verbatim from spec §7: `codex exec --full-auto --skip-git-repo-check`; **stdin ignored** (`Stdio::null` — documented hang otherwise); `--image <abs>` per reference **and** absolute paths listed in the instruction text; instruction requires one `SAVED: <absolute path>` line per image; stdout parsed for those lines only; doctor gates codex present, semver ≥ 0.142, login status, `exec --full-auto`/`--image` accepted.
- `docs/downstream-contracts.md` mirrors the load-bearing downstream contracts and **must be updated in the same commit as any behavior change** to those contracts.
- Never route argv through a shell: `tokio::process::Command` / `reqwest` arg/body construction only; prompts, paths and messages are data.
- CI never calls the real OpenAI API or real codex: wiremock + fake scripts only. Network-touching doctor tests use local wiremock or `http://127.0.0.1:9`.
- Error rendering format, verbatim: `uplink error [<driver>:<Kind>]: <message>` + optional ` — hint: <hint>`. All new errors are `DriverError` with a `kind` and, where actionable, a `hint`.
- Quality gates on every commit: `cargo fmt --check` and `cargo clippy --release --all-targets` must pass. Commit with `git commit -s` (adds `Signed-off-by: tonyhu <tonyhu@synology.com>`). Never add a Claude-Session trailer.
- License MIT; docs in English.
- Rust edition 2024: `std::env::set_var` is `unsafe`; tests that set env vars use a **unique variable name per test** inside `unsafe { }` with a `// SAFETY:` comment.

## File Structure

```
Cargo.toml                      # + reqwest, base64; dev: wiremock
src/
  config.rs                     # + ImageOpenAiCfg, ImageCodexCfg (renames "image-openai"/"image-codex")
  core/mod.rs                   # + now_rfc3339 (moved from drivers/tmux)
  drivers/mod.rs                # + pub mod image;
  drivers/image/mod.rs          # ImageDriver (Driver impl) + ImageBackend trait + clip helpers
  drivers/image/openai.rs       # OpenAiBackend: request building, HTTP, file writing, doctor
  drivers/image/codex.rs        # CodexBackend: instruction/argv/SAVED contract, doctor
  drivers/tmux/mod.rs           # now_rfc3339 body replaced with re-export from core
  mcp.rs                        # build_registry registers ImageDriver
  cli/mod.rs                    # + setup subcommand (embeds SKILL.md)
  main.rs                       # usage string mentions setup
skills/uplink/SKILL.md          # companion skill (embedded via include_str!)
docs/downstream-contracts.md    # OpenAI Images API + Codex CLI contracts
docs/wire-contract.md           # one-page Driver trait mirror + changelog
LICENSE                         # MIT
README.md
release-plz.toml
dist-workspace.toml
```

---

### Task 1: Dependencies + image-openai config

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`

**Interfaces:**
- Consumes: `config::default_true` (existing), `DriversCfg`.
- Produces: `config::ImageOpenAiCfg { enabled: bool, api_key_env: String, model: String, base_url: String }` with `Default` (true, `"OPENAI_API_KEY"`, `"gpt-image-1"`, `"https://api.openai.com/v1"`); `Config.drivers.image_openai` reachable from TOML section `[drivers.image-openai]`. All fields `pub` (tests and the backend construct it directly).

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` `[dependencies]`, append:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "multipart"] }
base64 = "0.22"
```

In `[dev-dependencies]`, append:

```toml
wiremock = "0.6"
```

Run: `cargo build`
Expected: compiles. Then run: `cargo tree -e normal | grep -i openssl`
Expected: no output (rustls-only constraint holds).

- [ ] **Step 2: Write the failing config tests**

Append to the `tests` module in `src/config.rs`:

```rust
    #[test]
    fn image_openai_defaults() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.image_openai.enabled);
        assert_eq!(c.drivers.image_openai.api_key_env, "OPENAI_API_KEY");
        assert_eq!(c.drivers.image_openai.model, "gpt-image-1");
        assert_eq!(c.drivers.image_openai.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn image_openai_section_parses_with_dash_name() {
        let c = Config::from_str(
            "[drivers.image-openai]\nenabled = false\napi_key_env = \"MY_KEY\"\nmodel = \"gpt-image-1-mini\"\nbase_url = \"http://127.0.0.1:8080/v1\"\n",
        )
        .unwrap();
        assert!(!c.drivers.image_openai.enabled);
        assert_eq!(c.drivers.image_openai.api_key_env, "MY_KEY");
        assert_eq!(c.drivers.image_openai.model, "gpt-image-1-mini");
        assert_eq!(c.drivers.image_openai.base_url, "http://127.0.0.1:8080/v1");
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib config`
Expected: FAIL — `image_openai` field does not exist.

- [ ] **Step 4: Implement the config struct**

In `src/config.rs`, add the field to `DriversCfg`:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct DriversCfg {
    #[serde(default)]
    pub tmux: TmuxCfg,
    #[serde(default, rename = "image-openai")]
    pub image_openai: ImageOpenAiCfg,
}
```

And add below `TmuxCfg`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ImageOpenAiCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_openai_model")]
    pub model: String,
    #[serde(default = "default_openai_base")]
    pub base_url: String,
}

impl Default for ImageOpenAiCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key_env: default_key_env(),
            model: default_openai_model(),
            base_url: default_openai_base(),
        }
    }
}

fn default_key_env() -> String {
    "OPENAI_API_KEY".into()
}
fn default_openai_model() -> String {
    "gpt-image-1".into()
}
fn default_openai_base() -> String {
    "https://api.openai.com/v1".into()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config`
Expected: PASS (all config tests, old and new).

- [ ] **Step 6: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add Cargo.toml Cargo.lock src/config.rs
git commit -s -m "feat(config): image-openai driver section + reqwest(rustls)/base64 deps"
```

---

### Task 2: ImageDriver scaffold (composite driver, backend trait, shared helpers)

**Files:**
- Create: `src/drivers/image/mod.rs`
- Modify: `src/drivers/mod.rs`
- Modify: `src/core/mod.rs`
- Modify: `src/drivers/tmux/mod.rs` (move `now_rfc3339` to core, keep re-export)

**Interfaces:**
- Consumes: `core::driver::{Driver, DriverInfo, DriverKind, ChannelEntry, OpSpec, SendRequest, SendReceipt, RecvBatch, DoctorReport}`, `error::{DriverError, ErrorKind}`.
- Produces:
  - `pub(crate) trait ImageBackend: Send + Sync { fn name(&self) -> &'static str; fn detail(&self) -> serde_json::Value; fn ops(&self) -> Vec<OpSpec>; async fn invoke(&self, op: &str, args: serde_json::Value) -> Result<serde_json::Value, DriverError>; async fn doctor_lines(&self) -> (bool, Vec<String>); }`
  - `pub struct ImageDriver` with `pub(crate) fn from_backends(Vec<Box<dyn ImageBackend>>) -> Self`, implementing `Driver` with id `"image"`.
  - `pub(crate) fn clip(s: &str, max_chars: usize) -> String` (head) and `pub(crate) fn clip_tail(s: &str, max_chars: usize) -> String` — char-boundary-safe evidence truncation used by both backends.
  - `core::now_rfc3339()` (moved; `drivers::tmux::now_rfc3339` remains valid as a re-export so existing callers/tests are untouched).

- [ ] **Step 1: Move `now_rfc3339` to core**

Append to `src/core/mod.rs` (file currently only has the four `pub mod` lines) the exact function body currently at `src/drivers/tmux/mod.rs:536-554`:

```rust
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
```

In `src/drivers/tmux/mod.rs`, delete that function definition and replace it with:

```rust
pub use crate::core::now_rfc3339;
```

Run: `cargo test`
Expected: PASS — all existing lib tests still green (call sites inside the tmux module resolve through the re-export).

- [ ] **Step 2: Write the failing driver-scaffold tests**

Create `src/drivers/image/mod.rs` with module docs, the trait, the driver, helpers, and a tests module (tests first — they reference the items, so write the whole file now; the point of this step is the test list):

```rust
//! Composite `image` driver: routes `image:<backend>` addresses to internal
//! [`ImageBackend`]s (openai, codex). One registry driver — adding a backend
//! never adds an MCP tool, and never touches `Registry` routing.

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
                .with_hint("use channel_invoke with op 'generate' or 'edit'"),
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
        assert!(e.hint.unwrap().contains("channel_invoke"));
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
```

Note: `pub mod openai;` will not resolve yet — create a placeholder `src/drivers/image/openai.rs` so the module tree compiles (Task 3 replaces it entirely):

```rust
//! OpenAI Images API backend (`image:openai`). Filled in by Tasks 3-4.
```

In `src/drivers/mod.rs`, add:

```rust
pub mod image;
pub mod tmux;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib drivers::image`
Expected: PASS (all 7 new tests). Then `cargo test` — full lib suite still green.

- [ ] **Step 4: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/core/mod.rs src/drivers/mod.rs src/drivers/image/ src/drivers/tmux/mod.rs
git commit -s -m "feat(image): composite image driver scaffold with backend trait"
```

---

### Task 3: OpenAI backend — pure request/response helpers

**Files:**
- Modify: `src/drivers/image/openai.rs` (replace placeholder)

**Interfaces:**
- Consumes: `config::ImageOpenAiCfg`, `error::{DriverError, ErrorKind}`.
- Produces (all consumed by Task 4 in the same file):
  - `pub(crate) struct GenerateArgs { prompt: String, n: Option<u32>, size: Option<String>, quality: Option<String>, refs: Option<Vec<String>>, out_dir: Option<String> }` (serde, `deny_unknown_fields`)
  - `pub(crate) struct EditArgs { input: String, prompt: String, mask: Option<String> }` (serde, `deny_unknown_fields`)
  - `pub(crate) fn generation_body(model: &str, a: &GenerateArgs) -> serde_json::Value` — omits absent optional fields
  - `pub(crate) fn image_filename(rfc3339: &str, n: usize) -> String` — `20260722T101530Z-1.png` shape
  - `pub(crate) fn decode_b64_png(s: &str) -> Result<Vec<u8>, DriverError>` — `Upstream` on bad base64

- [ ] **Step 1: Write the file with types, helpers, and failing tests**

Replace `src/drivers/image/openai.rs` with:

```rust
//! OpenAI Images API backend (`image:openai`).
//!
//! Request/endpoint contracts are mirrored in `docs/downstream-contracts.md`
//! (created in Task 8); any change here must update that file in the same
//! commit.

use serde::Deserialize;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenerateArgs {
    pub prompt: String,
    pub n: Option<u32>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub refs: Option<Vec<String>>,
    pub out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditArgs {
    pub input: String,
    pub prompt: String,
    pub mask: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImagesResponse {
    data: Vec<ImageDatum>,
}

#[derive(Debug, Deserialize)]
struct ImageDatum {
    b64_json: Option<String>,
}

/// JSON body for POST /images/generations. Optional fields are omitted when
/// absent (never sent as null) so golden-request tests pin the exact wire
/// shape.
pub(crate) fn generation_body(model: &str, a: &GenerateArgs) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("model".into(), model.into());
    m.insert("prompt".into(), a.prompt.clone().into());
    if let Some(n) = a.n {
        m.insert("n".into(), n.into());
    }
    if let Some(s) = &a.size {
        m.insert("size".into(), s.clone().into());
    }
    if let Some(q) = &a.quality {
        m.insert("quality".into(), q.clone().into());
    }
    serde_json::Value::Object(m)
}

/// `2026-07-22T10:15:30Z` → `20260722T101530Z-<n>.png` (spec §6: files are
/// named `<UTC-ts>-<n>.png`).
pub(crate) fn image_filename(rfc3339: &str, n: usize) -> String {
    format!("{}-{}.png", rfc3339.replace(['-', ':'], ""), n)
}

pub(crate) fn decode_b64_png(s: &str) -> Result<Vec<u8>, DriverError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| {
            DriverError::new(ErrorKind::Upstream, format!("invalid base64 image data: {e}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_body_includes_all_present_fields() {
        let a = GenerateArgs {
            prompt: "a red square".into(),
            n: Some(2),
            size: Some("1024x1024".into()),
            quality: Some("high".into()),
            refs: None,
            out_dir: None,
        };
        assert_eq!(
            generation_body("gpt-image-1", &a),
            serde_json::json!({
                "model": "gpt-image-1",
                "prompt": "a red square",
                "n": 2,
                "size": "1024x1024",
                "quality": "high"
            })
        );
    }

    #[test]
    fn generation_body_omits_absent_fields() {
        let a = GenerateArgs {
            prompt: "p".into(),
            n: None,
            size: None,
            quality: None,
            refs: None,
            out_dir: None,
        };
        assert_eq!(
            generation_body("gpt-image-1", &a),
            serde_json::json!({"model": "gpt-image-1", "prompt": "p"})
        );
    }

    #[test]
    fn filename_compacts_timestamp() {
        assert_eq!(
            image_filename("2026-07-22T10:15:30Z", 1),
            "20260722T101530Z-1.png"
        );
    }

    #[test]
    fn args_reject_unknown_fields() {
        let e = serde_json::from_value::<GenerateArgs>(
            serde_json::json!({"prompt": "p", "promt_typo": 1}),
        )
        .err()
        .unwrap();
        assert!(e.to_string().contains("promt_typo"));
    }

    #[test]
    fn b64_decode_maps_to_upstream() {
        let e = decode_b64_png("!!!not-base64!!!").err().unwrap();
        assert!(matches!(e.kind, ErrorKind::Upstream));
        use base64::Engine as _;
        let ok = decode_b64_png(&base64::engine::general_purpose::STANDARD.encode(b"png-bytes"))
            .unwrap();
        assert_eq!(ok, b"png-bytes");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib drivers::image::openai`
Expected: PASS (5 tests). Note: `ImagesResponse`/`ImageDatum` (and possibly `EditArgs`) trigger `dead_code` *warnings* at this intermediate commit — expected and non-gating (the local gate is exit code, not `-D warnings`); Task 4 consumes them all, and CI's `-D warnings` only ever runs against the branch tip.

- [ ] **Step 3: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/drivers/image/openai.rs
git commit -s -m "feat(image-openai): request building, filenames, b64 decode (pure)"
```

---

### Task 4: OpenAI backend — HTTP, file writing, doctor (wiremock suite)

**Files:**
- Modify: `src/drivers/image/openai.rs`
- Modify: `.gitignore` (add `/uplink-images`)

**Interfaces:**
- Consumes: Task 3 helpers; `ImageBackend` trait; `clip` from `drivers::image`; `core::now_rfc3339`.
- Produces: `pub struct OpenAiBackend` with `pub(crate) fn new(cfg: ImageOpenAiCfg) -> Self`, implementing `ImageBackend` (name `"openai"`). Op results: `{"paths": ["<abs>.png", ...]}`.

**Behavior contract (from spec §6):**
- `generate` without `refs` → POST `{base_url}/images/generations`, JSON `generation_body`, `Authorization: Bearer <key>`.
- `generate` with `refs` → POST `{base_url}/images/edits` multipart: `model`, `prompt`, optional `n`/`size`/`quality` as text fields, each ref as a file part named `image[]` (gpt-image-1 multi-image form).
- `edit` → POST `{base_url}/images/edits` multipart: `model`, `prompt`, input file part named `image`, optional `mask` file part named `mask`.
- Response: JSON `{data: [{b64_json}]}` → decode → write `out_dir` (default `./uplink-images`) with `image_filename(now, i+1)` → return absolute paths.
- Errors: key missing → `Unavailable` + hint naming the env var; transport failure → `Unavailable`; non-2xx → `Upstream` with first-500-chars body evidence; zero/undecodable images → `Upstream`; unreadable input/mask/ref or unwritable out_dir → `Invalid`.
- Doctor: key presence line (never the value), HEAD `base_url` reachability (3 s timeout), model line; ok = key present AND reachable.

- [ ] **Step 1: Implement the backend**

Append to `src/drivers/image/openai.rs` (above the tests module):

```rust
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::config::ImageOpenAiCfg;
use crate::core::driver::OpSpec;
use crate::drivers::image::{clip, ImageBackend};

pub struct OpenAiBackend {
    cfg: ImageOpenAiCfg,
    client: reqwest::Client,
}

fn bad_args(e: serde_json::Error) -> DriverError {
    DriverError::new(ErrorKind::Invalid, format!("bad args: {e}"))
        .with_hint("run channel_describe(image:openai) for the exact schema")
}

fn file_part(path: &str) -> Result<reqwest::multipart::Part, DriverError> {
    let bytes = std::fs::read(path).map_err(|e| {
        DriverError::new(ErrorKind::Invalid, format!("cannot read image file '{path}': {e}"))
    })?;
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image.png")
        .to_string();
    reqwest::multipart::Part::bytes(bytes)
        .file_name(name)
        .mime_str("image/png")
        .map_err(|e| DriverError::new(ErrorKind::Invalid, format!("bad mime: {e}")))
}

impl OpenAiBackend {
    pub(crate) fn new(cfg: ImageOpenAiCfg) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("reqwest client construction cannot fail with static options");
        Self { cfg, client }
    }

    fn key(&self) -> Result<String, DriverError> {
        std::env::var(&self.cfg.api_key_env)
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                DriverError::new(
                    ErrorKind::Unavailable,
                    format!("API key env '{}' is not set", self.cfg.api_key_env),
                )
                .with_hint(format!(
                    "export {}=<key> in the environment running cc-uplink",
                    self.cfg.api_key_env
                ))
            })
    }

    async fn parse_images_response(
        resp: reqwest::Response,
    ) -> Result<Vec<Vec<u8>>, DriverError> {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(DriverError::new(
                ErrorKind::Upstream,
                format!("openai returned HTTP {status}"),
            )
            .with_evidence(clip(&body, 500)));
        }
        let parsed: ImagesResponse = serde_json::from_str(&body).map_err(|e| {
            DriverError::new(ErrorKind::Upstream, format!("unparseable openai response: {e}"))
                .with_evidence(clip(&body, 500))
        })?;
        let mut out = vec![];
        for d in parsed.data {
            let b64 = d.b64_json.ok_or_else(|| {
                DriverError::new(ErrorKind::Upstream, "no b64_json image data in response")
            })?;
            out.push(decode_b64_png(&b64)?);
        }
        if out.is_empty() {
            return Err(DriverError::new(
                ErrorKind::Upstream,
                "openai returned zero images",
            ));
        }
        Ok(out)
    }

    fn write_images(dir: &Path, images: &[Vec<u8>]) -> Result<Vec<String>, DriverError> {
        std::fs::create_dir_all(dir).map_err(|e| {
            DriverError::new(
                ErrorKind::Invalid,
                format!("cannot create out_dir '{}': {e}", dir.display()),
            )
        })?;
        let ts = crate::core::now_rfc3339();
        let mut paths = vec![];
        for (i, bytes) in images.iter().enumerate() {
            let p = dir.join(image_filename(&ts, i + 1));
            std::fs::write(&p, bytes).map_err(|e| {
                DriverError::new(
                    ErrorKind::Invalid,
                    format!("cannot write '{}': {e}", p.display()),
                )
            })?;
            let abs = std::fs::canonicalize(&p).unwrap_or(p);
            paths.push(abs.display().to_string());
        }
        Ok(paths)
    }

    fn transport_err(&self, e: reqwest::Error) -> DriverError {
        DriverError::new(
            ErrorKind::Unavailable,
            format!("cannot reach {}: {e}", self.cfg.base_url),
        )
    }

    async fn generate(&self, a: GenerateArgs) -> Result<serde_json::Value, DriverError> {
        let key = self.key()?;
        let dir = PathBuf::from(a.out_dir.clone().unwrap_or_else(|| "./uplink-images".into()));
        let refs = a.refs.clone().unwrap_or_default();
        let resp = if refs.is_empty() {
            self.client
                .post(format!("{}/images/generations", self.cfg.base_url))
                .bearer_auth(&key)
                .json(&generation_body(&self.cfg.model, &a))
                .send()
                .await
        } else {
            let mut form = reqwest::multipart::Form::new()
                .text("model", self.cfg.model.clone())
                .text("prompt", a.prompt.clone());
            if let Some(n) = a.n {
                form = form.text("n", n.to_string());
            }
            if let Some(s) = &a.size {
                form = form.text("size", s.clone());
            }
            if let Some(q) = &a.quality {
                form = form.text("quality", q.clone());
            }
            for r in &refs {
                form = form.part("image[]", file_part(r)?);
            }
            self.client
                .post(format!("{}/images/edits", self.cfg.base_url))
                .bearer_auth(&key)
                .multipart(form)
                .send()
                .await
        };
        let resp = resp.map_err(|e| self.transport_err(e))?;
        let images = Self::parse_images_response(resp).await?;
        let paths = Self::write_images(&dir, &images)?;
        Ok(serde_json::json!({ "paths": paths }))
    }

    async fn edit(&self, a: EditArgs) -> Result<serde_json::Value, DriverError> {
        let key = self.key()?;
        let mut form = reqwest::multipart::Form::new()
            .text("model", self.cfg.model.clone())
            .text("prompt", a.prompt.clone())
            .part("image", file_part(&a.input)?);
        if let Some(m) = &a.mask {
            form = form.part("mask", file_part(m)?);
        }
        let resp = self
            .client
            .post(format!("{}/images/edits", self.cfg.base_url))
            .bearer_auth(&key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| self.transport_err(e))?;
        let images = Self::parse_images_response(resp).await?;
        let paths = Self::write_images(Path::new("./uplink-images"), &images)?;
        Ok(serde_json::json!({ "paths": paths }))
    }
}

#[async_trait]
impl ImageBackend for OpenAiBackend {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn detail(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.cfg.model,
            "api_key_env": self.cfg.api_key_env,
            "key_present": std::env::var(&self.cfg.api_key_env)
                .map(|v| !v.is_empty())
                .unwrap_or(false),
        })
    }

    fn ops(&self) -> Vec<OpSpec> {
        vec![
            OpSpec {
                op: "generate".into(),
                summary: "[openai] generate image(s) via the OpenAI Images API; refs[] switches to the multi-image edits endpoint".into(),
                params_schema: serde_json::json!({
                    "type": "object",
                    "required": ["prompt"],
                    "additionalProperties": false,
                    "properties": {
                        "prompt": {"type": "string"},
                        "n": {"type": "integer", "minimum": 1, "maximum": 10},
                        "size": {"type": "string", "description": "e.g. 1024x1024, 1536x1024, 1024x1536, auto"},
                        "quality": {"type": "string", "description": "low | medium | high | auto"},
                        "refs": {"type": "array", "items": {"type": "string"}, "description": "reference image file paths"},
                        "out_dir": {"type": "string", "description": "output directory (default ./uplink-images)"}
                    }
                }),
                result_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"paths": {"type": "array", "items": {"type": "string"}}}
                }),
            },
            OpSpec {
                op: "edit".into(),
                summary: "[openai] edit an existing image (optional mask) via the OpenAI Images API".into(),
                params_schema: serde_json::json!({
                    "type": "object",
                    "required": ["input", "prompt"],
                    "additionalProperties": false,
                    "properties": {
                        "input": {"type": "string", "description": "input image file path"},
                        "prompt": {"type": "string"},
                        "mask": {"type": "string", "description": "mask image file path"}
                    }
                }),
                result_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"paths": {"type": "array", "items": {"type": "string"}}}
                }),
            },
        ]
    }

    async fn invoke(
        &self,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        match op {
            "generate" => {
                let a: GenerateArgs = serde_json::from_value(args).map_err(bad_args)?;
                self.generate(a).await
            }
            "edit" => {
                let a: EditArgs = serde_json::from_value(args).map_err(bad_args)?;
                self.edit(a).await
            }
            other => Err(DriverError::new(
                ErrorKind::NotFound,
                format!("no op '{other}' on image:openai"),
            )
            .with_hint("run channel_describe(image:openai)")),
        }
    }

    async fn doctor_lines(&self) -> (bool, Vec<String>) {
        let key_ok = std::env::var(&self.cfg.api_key_env)
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let mut lines = vec![format!(
            "key: {} ({})",
            if key_ok { "present" } else { "MISSING" },
            self.cfg.api_key_env
        )];
        let reach = self
            .client
            .head(&self.cfg.base_url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok();
        lines.push(format!(
            "endpoint: {} ({})",
            if reach { "reachable" } else { "UNREACHABLE" },
            self.cfg.base_url
        ));
        lines.push(format!("model: {}", self.cfg.model));
        (key_ok && reach, lines)
    }
}
```

Remove any `#[cfg_attr(not(test), allow(dead_code))]` added in Task 3.

- [ ] **Step 2: Write the wiremock tests**

Append to the `tests` module in `src/drivers/image/openai.rs`:

```rust
    use base64::Engine as _;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg(base: &str, key_env: &str) -> crate::config::ImageOpenAiCfg {
        crate::config::ImageOpenAiCfg {
            enabled: true,
            api_key_env: key_env.into(),
            model: "gpt-image-1".into(),
            base_url: base.into(),
        }
    }

    fn b64_response(bytes: &[u8]) -> serde_json::Value {
        serde_json::json!({
            "data": [{"b64_json": base64::engine::general_purpose::STANDARD.encode(bytes)}]
        })
    }

    #[tokio::test]
    async fn generate_sends_golden_body_and_writes_file() {
        // SAFETY: unique env var name, set before any reader in this test.
        unsafe { std::env::set_var("CC_UPLINK_T_OPENAI_GEN", "sk-test") };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .and(body_json(serde_json::json!({
                "model": "gpt-image-1",
                "prompt": "a red square",
                "n": 1,
                "size": "1024x1024",
                "quality": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(b64_response(b"PNGBYTES")))
            .expect(1)
            .mount(&server)
            .await;
        let out_dir = tempfile::tempdir().unwrap();
        let b = OpenAiBackend::new(cfg(&server.uri(), "CC_UPLINK_T_OPENAI_GEN"));
        let out = b
            .invoke(
                "generate",
                serde_json::json!({
                    "prompt": "a red square", "n": 1, "size": "1024x1024",
                    "quality": "high", "out_dir": out_dir.path().to_str().unwrap()
                }),
            )
            .await
            .unwrap();
        let p = out["paths"][0].as_str().unwrap();
        assert!(std::path::Path::new(p).is_absolute());
        assert!(p.ends_with(".png"));
        assert_eq!(std::fs::read(p).unwrap(), b"PNGBYTES");
        // auth header carried the key
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(
            reqs[0].headers.get("authorization").unwrap(),
            "Bearer sk-test"
        );
    }

    #[tokio::test]
    async fn generate_with_refs_routes_to_edits_multipart() {
        // SAFETY: unique env var name.
        unsafe { std::env::set_var("CC_UPLINK_T_OPENAI_REFS", "sk-test") };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/images/edits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(b64_response(b"OUT")))
            .expect(1)
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let r1 = tmp.path().join("ref1.png");
        std::fs::write(&r1, b"ref-one").unwrap();
        let b = OpenAiBackend::new(cfg(&server.uri(), "CC_UPLINK_T_OPENAI_REFS"));
        let out = b
            .invoke(
                "generate",
                serde_json::json!({
                    "prompt": "styled scene",
                    "refs": [r1.to_str().unwrap()],
                    "out_dir": tmp.path().join("out").to_str().unwrap()
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["paths"].as_array().unwrap().len(), 1);
        let reqs = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&reqs[0].body);
        assert!(body.contains("name=\"prompt\""));
        assert!(body.contains("styled scene"));
        assert!(body.contains("name=\"image[]\""));
        assert!(body.contains("name=\"model\""));
    }

    #[tokio::test]
    async fn edit_sends_multipart_with_image_and_mask() {
        // SAFETY: unique env var name.
        unsafe { std::env::set_var("CC_UPLINK_T_OPENAI_EDIT", "sk-test") };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/edits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(b64_response(b"EDITED")))
            .expect(1)
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("in.png");
        let mask = tmp.path().join("mask.png");
        std::fs::write(&input, b"in").unwrap();
        std::fs::write(&mask, b"mask").unwrap();
        let b = OpenAiBackend::new(cfg(&server.uri(), "CC_UPLINK_T_OPENAI_EDIT"));
        // `edit` has no out_dir (spec §6): it writes to ./uplink-images
        // relative to the test CWD (crate root). Never change the process
        // CWD in a test — other tests run in parallel threads. Instead,
        // capture the result, clean the directory up, then assert.
        let out = b
            .invoke(
                "edit",
                serde_json::json!({
                    "input": input.to_str().unwrap(),
                    "prompt": "tint blue",
                    "mask": mask.to_str().unwrap()
                }),
            )
            .await
            .unwrap();
        let written = out["paths"][0].as_str().unwrap().to_string();
        let existed = std::path::Path::new(&written).is_file();
        std::fs::remove_dir_all("./uplink-images").ok(); // cleanup before asserts
        assert!(existed, "edit output file must exist: {written}");
        assert!(std::path::Path::new(&written).is_absolute());
        let reqs = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&reqs[0].body);
        assert!(body.contains("name=\"image\""));
        assert!(body.contains("name=\"mask\""));
        assert!(body.contains("tint blue"));
    }

    #[tokio::test]
    async fn missing_key_is_unavailable_with_env_hint() {
        let b = OpenAiBackend::new(cfg("http://127.0.0.1:9", "CC_UPLINK_T_OPENAI_NOKEY"));
        let e = b
            .invoke("generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Unavailable));
        assert!(e.hint.unwrap().contains("CC_UPLINK_T_OPENAI_NOKEY"));
    }

    #[tokio::test]
    async fn upstream_http_error_carries_body_evidence() {
        // SAFETY: unique env var name.
        unsafe { std::env::set_var("CC_UPLINK_T_OPENAI_401", "sk-test") };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(r#"{"error":{"message":"Incorrect API key"}}"#),
            )
            .mount(&server)
            .await;
        let b = OpenAiBackend::new(cfg(&server.uri(), "CC_UPLINK_T_OPENAI_401"));
        let e = b
            .invoke("generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Upstream));
        assert!(e.message.contains("401"));
        assert!(e.evidence.unwrap().contains("Incorrect API key"));
    }

    #[tokio::test]
    async fn unknown_op_is_not_found() {
        let b = OpenAiBackend::new(cfg("http://127.0.0.1:9", "CC_UPLINK_T_OPENAI_OP"));
        let e = b
            .invoke("transmogrify", serde_json::json!({}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::NotFound));
    }

    #[tokio::test]
    async fn doctor_reports_key_and_reachability() {
        // SAFETY: unique env var name.
        unsafe { std::env::set_var("CC_UPLINK_T_OPENAI_DOC", "sk-test") };
        let server = MockServer::start().await;
        let b = OpenAiBackend::new(cfg(&server.uri(), "CC_UPLINK_T_OPENAI_DOC"));
        let (ok, lines) = b.doctor_lines().await;
        assert!(ok, "reachable wiremock + key present ⇒ ok; lines: {lines:?}");
        assert!(lines.iter().any(|l| l.contains("key: present")));
        assert!(!lines.iter().any(|l| l.contains("sk-test")), "key value must never leak");

        let b2 = OpenAiBackend::new(cfg("http://127.0.0.1:9", "CC_UPLINK_T_OPENAI_DOC"));
        let (ok2, lines2) = b2.doctor_lines().await;
        assert!(!ok2);
        assert!(lines2.iter().any(|l| l.contains("UNREACHABLE")));
    }
```

Also append `/uplink-images` to `.gitignore` (defense in depth: the edit test cleans up after itself, but a crash mid-test or a real `cargo run` from the repo root would otherwise leave untracked output in the working tree).

- [ ] **Step 3: Run tests**

Run: `cargo test --lib drivers::image::openai`
Expected: PASS (12 tests: 5 pure + 7 wiremock).

- [ ] **Step 4: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/drivers/image/openai.rs .gitignore
git commit -s -m "feat(image-openai): HTTP backend with wiremock-tested golden requests"
```

---

### Task 5: Register the image driver (MCP + CLI wiring)

**Files:**
- Modify: `src/mcp.rs`

**Interfaces:**
- Consumes: `drivers::image::{ImageDriver, ImageBackend}`, `drivers::image::openai::OpenAiBackend`, `config::Config`.
- Produces: `build_registry()` registers `ImageDriver` when at least one image backend is enabled. (CLI `doctor/send/invoke` automatically gain image support — they share `build_registry`.)

- [ ] **Step 1: Write the failing registry-level test**

Append to the `tests` module in `src/mcp.rs`:

```rust
    use std::sync::Arc;

    use crate::config::ImageOpenAiCfg;
    use crate::core::registry::Registry;
    use crate::drivers::image::{openai::OpenAiBackend, ImageBackend, ImageDriver};
    use crate::error::ErrorKind;

    #[tokio::test]
    async fn image_driver_routes_via_registry() {
        let mut reg = Registry::new();
        let cfg = ImageOpenAiCfg {
            enabled: true,
            api_key_env: "CC_UPLINK_T_MCP_NOKEY".into(),
            model: "gpt-image-1".into(),
            base_url: "http://127.0.0.1:9".into(),
        };
        let backends: Vec<Box<dyn ImageBackend>> = vec![Box::new(OpenAiBackend::new(cfg))];
        reg.register(Arc::new(ImageDriver::from_backends(backends)));
        let (d, addr) = reg.driver_for("image:openai").unwrap();
        assert_eq!(addr, "openai");
        // no key set → invoke fails Unavailable through the full routing path
        let e = d
            .invoke(&addr, "generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Unavailable));
        let list = reg.list_all().await;
        assert!(list
            .iter()
            .any(|(i, cs)| i.id == "image" && cs.iter().any(|c| c.channel == "image:openai")));
    }
```

Run: `cargo test --lib mcp`
Expected: this test PASSES already (it exercises existing pieces) — it is the integration pin for this task. If it fails, the wiring below is wrong.

- [ ] **Step 2: Wire build_registry**

In `src/mcp.rs`, replace the body of `build_registry` with:

```rust
pub async fn build_registry() -> Arc<Registry> {
    let cfg = crate::config::Config::load();
    let mut reg = Registry::new();
    if cfg.drivers.tmux.enabled {
        match crate::drivers::tmux::TmuxDriver::new(cfg.drivers.tmux).await {
            Ok(d) => reg.register(d),
            Err(e) => eprintln!("cc-uplink: tmux driver unavailable: {}", e.message),
        }
    }
    let mut backends: Vec<Box<dyn crate::drivers::image::ImageBackend>> = Vec::new();
    if cfg.drivers.image_openai.enabled {
        backends.push(Box::new(crate::drivers::image::openai::OpenAiBackend::new(
            cfg.drivers.image_openai,
        )));
    }
    if !backends.is_empty() {
        reg.register(Arc::new(crate::drivers::image::ImageDriver::from_backends(
            backends,
        )));
    }
    Arc::new(reg)
}
```

(The doc comment above `build_registry` should now say it registers the tmux driver and the image driver per config.)

- [ ] **Step 3: Run the full suite + manual smoke**

Run: `cargo test`
Expected: PASS (all lib tests).

Manual (informational, not gating): `cargo run -- doctor`
Expected: `[image]` section appears with `openai: key: …` / `endpoint: …` lines (DEGRADED is fine on a machine without a key — that's the doctor doing its job).

- [ ] **Step 4: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/mcp.rs
git commit -s -m "feat(mcp): register image driver in the shared registry"
```

---

### Task 6: image-codex config + pure contract functions

**Files:**
- Modify: `src/config.rs`
- Create: `src/drivers/image/codex.rs`
- Modify: `src/drivers/image/mod.rs` (add `pub mod codex;`)

**Interfaces:**
- Consumes: `config::default_true`.
- Produces:
  - `config::ImageCodexCfg { enabled: bool, codex_bin: String }`, defaults `(true, "codex")`, TOML section `[drivers.image-codex]`, field `Config.drivers.image_codex`.
  - In `codex.rs` (all consumed by Task 7): `pub(crate) fn build_instruction(prompt: &str, refs: &[PathBuf]) -> String`; `pub(crate) fn build_edit_instruction(input: &Path, prompt: &str) -> String`; `pub(crate) fn exec_args(instruction: &str, images: &[PathBuf]) -> Vec<String>`; `pub(crate) fn parse_saved_lines(stdout: &str) -> Vec<String>`; `pub(crate) fn parse_codex_version(s: &str) -> Option<(u64, u64, u64)>`; `pub(crate) fn version_ok(v: (u64, u64, u64)) -> bool`.

- [ ] **Step 1: Config — failing tests then implementation**

Append tests to `src/config.rs`:

```rust
    #[test]
    fn image_codex_defaults() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.image_codex.enabled);
        assert_eq!(c.drivers.image_codex.codex_bin, "codex");
    }

    #[test]
    fn image_codex_section_parses() {
        let c = Config::from_str(
            "[drivers.image-codex]\nenabled = false\ncodex_bin = \"/opt/codex/bin/codex\"\n",
        )
        .unwrap();
        assert!(!c.drivers.image_codex.enabled);
        assert_eq!(c.drivers.image_codex.codex_bin, "/opt/codex/bin/codex");
    }
```

Run `cargo test --lib config` → FAIL. Then add to `DriversCfg`:

```rust
    #[serde(default, rename = "image-codex")]
    pub image_codex: ImageCodexCfg,
```

and below `ImageOpenAiCfg`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ImageCodexCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_codex_bin")]
    pub codex_bin: String,
}

impl Default for ImageCodexCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            codex_bin: default_codex_bin(),
        }
    }
}

fn default_codex_bin() -> String {
    "codex".into()
}
```

Run `cargo test --lib config` → PASS.

- [ ] **Step 2: Pure contract functions with tests**

Create `src/drivers/image/codex.rs`:

```rust
//! Codex CLI backend (`image:codex`) — borrows Codex's built-in imagegen.
//!
//! Every downstream contract in this file (argv shape, stdin discipline,
//! `SAVED:` stdout lines, version/login gates) is mirrored in
//! `docs/downstream-contracts.md`; update that file in the same commit as any
//! change here.

use std::path::{Path, PathBuf};

/// Instruction text for `generate`. Load-bearing pieces: absolute ref paths
/// listed in the text (required by the 0.144+ `referenced_image_paths` tool
/// path, keeps 0.142–0.143 working alongside `--image`), and the exact
/// `SAVED: <absolute path>` line contract our stdout parser depends on.
pub(crate) fn build_instruction(prompt: &str, refs: &[PathBuf]) -> String {
    let mut s = format!("Generate image(s) with your imagegen skill.\n\nTask: {prompt}\n");
    if !refs.is_empty() {
        s.push_str(
            "\nReference images (attached via --image; also readable at these absolute paths):\n",
        );
        for r in refs {
            s.push_str(&format!("- {}\n", r.display()));
        }
    }
    s.push_str(
        "\nRequirements:\n\
         - Save every final image to disk.\n\
         - After saving, print exactly one line per saved image, of the form:\n\
         \x20 SAVED: <absolute path>\n",
    );
    s
}

pub(crate) fn build_edit_instruction(input: &Path, prompt: &str) -> String {
    format!(
        "Edit the image at {input} with your imagegen skill.\n\n\
         Edit request: {prompt}\n\n\
         The input image is attached via --image and also readable at the absolute path above.\n\n\
         Requirements:\n\
         - Save every final image to disk.\n\
         - After saving, print exactly one line per saved image, of the form:\n\
         \x20 SAVED: <absolute path>\n",
        input = input.display()
    )
}

/// Full argv (after the binary) for a codex image run. Contract (spec §7):
/// `exec --full-auto --skip-git-repo-check [--image <abs>]... <instruction>`.
pub(crate) fn exec_args(instruction: &str, images: &[PathBuf]) -> Vec<String> {
    let mut v = vec![
        "exec".to_string(),
        "--full-auto".to_string(),
        "--skip-git-repo-check".to_string(),
    ];
    for p in images {
        v.push("--image".to_string());
        v.push(p.display().to_string());
    }
    v.push(instruction.to_string());
    v
}

/// stdout → saved paths. Only lines of the form `SAVED: <path>` count;
/// everything else Codex prints is ignored (never re-parse LLM output).
pub(crate) fn parse_saved_lines(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("SAVED:"))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// First `<major>.<minor>.<patch>` token in `codex --version` output
/// (e.g. `codex-cli 0.144.0`).
pub(crate) fn parse_codex_version(s: &str) -> Option<(u64, u64, u64)> {
    // No let-chains here: MSRV is 1.85 and `if … && let …` needs 1.88.
    for tok in s.split_whitespace() {
        let parts: Vec<&str> = tok.trim_start_matches('v').split('.').collect();
        if parts.len() != 3 {
            continue;
        }
        match (
            parts[0].parse::<u64>(),
            parts[1].parse::<u64>(),
            parts[2].parse::<u64>(),
        ) {
            (Ok(a), Ok(b), Ok(c)) => return Some((a, b, c)),
            _ => continue,
        }
    }
    None
}

/// Doctor gate: spec §7 requires codex ≥ 0.142.
pub(crate) fn version_ok(v: (u64, u64, u64)) -> bool {
    v >= (0, 142, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_lists_refs_and_saved_contract() {
        let refs = vec![PathBuf::from("/abs/a.png"), PathBuf::from("/abs/b.png")];
        let s = build_instruction("a cat", &refs);
        assert!(s.contains("Task: a cat"));
        assert!(s.contains("- /abs/a.png"));
        assert!(s.contains("- /abs/b.png"));
        assert!(s.contains("SAVED: <absolute path>"));
        let no_refs = build_instruction("a cat", &[]);
        assert!(!no_refs.contains("Reference images"));
    }

    #[test]
    fn edit_instruction_names_input_and_saved_contract() {
        let s = build_edit_instruction(Path::new("/abs/in.png"), "tint blue");
        assert!(s.contains("/abs/in.png"));
        assert!(s.contains("tint blue"));
        assert!(s.contains("SAVED: <absolute path>"));
    }

    #[test]
    fn exec_args_order_and_shape() {
        let args = exec_args("INSTR", &[PathBuf::from("/a.png"), PathBuf::from("/b.png")]);
        assert_eq!(
            args,
            vec![
                "exec",
                "--full-auto",
                "--skip-git-repo-check",
                "--image",
                "/a.png",
                "--image",
                "/b.png",
                "INSTR"
            ]
        );
    }

    #[test]
    fn parse_saved_ignores_noise_and_trims() {
        let stdout = "thinking...\nSAVED: /tmp/one.png\r\nnoise SAVED-ish\n  SAVED:   /tmp/two.png  \nSAVED:\n";
        assert_eq!(parse_saved_lines(stdout), vec!["/tmp/one.png", "/tmp/two.png"]);
    }

    #[test]
    fn version_parsing_and_gate() {
        assert_eq!(parse_codex_version("codex-cli 0.144.0"), Some((0, 144, 0)));
        assert_eq!(parse_codex_version("0.142.0"), Some((0, 142, 0)));
        assert_eq!(parse_codex_version("v1.2.3 extra"), Some((1, 2, 3)));
        assert_eq!(parse_codex_version("no version here"), None);
        assert!(version_ok((0, 142, 0)));
        assert!(version_ok((1, 0, 0)));
        assert!(!version_ok((0, 141, 9)));
    }
}
```

In `src/drivers/image/mod.rs`, change the module list to:

```rust
pub mod codex;
pub mod openai;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib drivers::image::codex && cargo test --lib config`
Expected: PASS (5 codex + 2 new config tests).

- [ ] **Step 4: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/config.rs src/drivers/image/codex.rs src/drivers/image/mod.rs
git commit -s -m "feat(image-codex): config section + pure argv/instruction/SAVED contracts"
```

---

### Task 7: Codex backend — subprocess execution (fake-codex suite)

**Files:**
- Modify: `src/drivers/image/codex.rs`

**Interfaces:**
- Consumes: Task 6 pure functions; `ImageBackend`; `clip_tail` from `drivers::image`; `config::ImageCodexCfg`.
- Produces: `pub struct CodexBackend` with `pub(crate) fn new(cfg: ImageCodexCfg) -> Self`, implementing `ImageBackend` (name `"codex"`) for ops `generate {prompt, refs?}` / `edit {input, prompt}`. Result: `{"paths": [...]}` (paths are whatever codex reported in `SAVED:` lines — codex chose them). Doctor is Task 8; this task's `doctor_lines` returns `(false, vec!["doctor: not implemented yet".into()])` as an explicit placeholder that Task 8 replaces.

**Behavior contract (spec §7):** subprocess `codex_bin` + `exec_args(...)`; `stdin(Stdio::null())` (documented hang otherwise); stdout/stderr piped; `kill_on_drop(true)`; 600 s timeout → `Timeout`; spawn failure → `Unavailable` + install hint; nonzero exit → `Upstream` with stderr (or stdout) tail evidence; zero `SAVED:` lines → `Upstream` with stdout tail evidence; nonexistent ref/input file → `Invalid` (checked via `canonicalize`, which also produces the absolute paths the contract requires).

- [ ] **Step 1: Implement the backend**

Append to `src/drivers/image/codex.rs` (above tests):

```rust
use async_trait::async_trait;
use serde::Deserialize;

use crate::config::ImageCodexCfg;
use crate::core::driver::OpSpec;
use crate::drivers::image::{clip_tail, ImageBackend};
use crate::error::{DriverError, ErrorKind};

const CODEX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexGenerateArgs {
    pub prompt: String,
    pub refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexEditArgs {
    pub input: String,
    pub prompt: String,
}

pub struct CodexBackend {
    cfg: ImageCodexCfg,
}

fn bad_args(e: serde_json::Error) -> DriverError {
    DriverError::new(ErrorKind::Invalid, format!("bad args: {e}"))
        .with_hint("run channel_describe(image:codex) for the exact schema")
}

/// Existing file → absolute path (spec §7 requires absolute paths both in
/// `--image` argv and in the instruction text).
fn abs_existing(path: &str) -> Result<PathBuf, DriverError> {
    std::fs::canonicalize(path).map_err(|e| {
        DriverError::new(ErrorKind::Invalid, format!("image file '{path}': {e}"))
            .with_hint("pass paths to existing image files")
    })
}

impl CodexBackend {
    pub(crate) fn new(cfg: ImageCodexCfg) -> Self {
        Self { cfg }
    }

    async fn run_exec(
        &self,
        instruction: &str,
        images: &[PathBuf],
    ) -> Result<Vec<String>, DriverError> {
        let mut cmd = tokio::process::Command::new(&self.cfg.codex_bin);
        cmd.args(exec_args(instruction, images))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let out = tokio::time::timeout(CODEX_TIMEOUT, cmd.output())
            .await
            .map_err(|_| {
                DriverError::new(
                    ErrorKind::Timeout,
                    format!("codex exec timed out after {}s", CODEX_TIMEOUT.as_secs()),
                )
            })?
            .map_err(|e| {
                DriverError::new(
                    ErrorKind::Unavailable,
                    format!("cannot run '{}': {e}", self.cfg.codex_bin),
                )
                .with_hint("install @openai/codex >= 0.142 or set drivers.image-codex.codex_bin")
            })?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let ev = if stderr.trim().is_empty() { &stdout } else { &stderr };
            return Err(DriverError::new(
                ErrorKind::Upstream,
                format!("codex exec failed ({})", out.status),
            )
            .with_evidence(clip_tail(ev, 500)));
        }
        let saved = parse_saved_lines(&stdout);
        if saved.is_empty() {
            return Err(DriverError::new(
                ErrorKind::Upstream,
                "codex finished without printing any 'SAVED: <path>' line",
            )
            .with_evidence(clip_tail(&stdout, 500)));
        }
        Ok(saved)
    }
}

#[async_trait]
impl ImageBackend for CodexBackend {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn detail(&self) -> serde_json::Value {
        serde_json::json!({ "codex_bin": self.cfg.codex_bin })
    }

    fn ops(&self) -> Vec<OpSpec> {
        vec![
            OpSpec {
                op: "generate".into(),
                summary: "[codex] generate image(s) via Codex CLI's built-in imagegen (uses your codex login; no API key)".into(),
                params_schema: serde_json::json!({
                    "type": "object",
                    "required": ["prompt"],
                    "additionalProperties": false,
                    "properties": {
                        "prompt": {"type": "string", "description": "natural-language request; express size/count/output path inside the prompt"},
                        "refs": {"type": "array", "items": {"type": "string"}, "description": "reference image file paths (max 5)"}
                    }
                }),
                result_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"paths": {"type": "array", "items": {"type": "string"}}}
                }),
            },
            OpSpec {
                op: "edit".into(),
                summary: "[codex] edit an existing image via Codex CLI's built-in imagegen".into(),
                params_schema: serde_json::json!({
                    "type": "object",
                    "required": ["input", "prompt"],
                    "additionalProperties": false,
                    "properties": {
                        "input": {"type": "string", "description": "input image file path"},
                        "prompt": {"type": "string"}
                    }
                }),
                result_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"paths": {"type": "array", "items": {"type": "string"}}}
                }),
            },
        ]
    }

    async fn invoke(
        &self,
        op: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, DriverError> {
        match op {
            "generate" => {
                let a: CodexGenerateArgs = serde_json::from_value(args).map_err(bad_args)?;
                let refs = a
                    .refs
                    .unwrap_or_default()
                    .iter()
                    .map(|r| abs_existing(r))
                    .collect::<Result<Vec<_>, _>>()?;
                let instruction = build_instruction(&a.prompt, &refs);
                let saved = self.run_exec(&instruction, &refs).await?;
                Ok(serde_json::json!({ "paths": saved }))
            }
            "edit" => {
                let a: CodexEditArgs = serde_json::from_value(args).map_err(bad_args)?;
                let input = abs_existing(&a.input)?;
                let instruction = build_edit_instruction(&input, &a.prompt);
                let saved = self.run_exec(&instruction, &[input]).await?;
                Ok(serde_json::json!({ "paths": saved }))
            }
            other => Err(DriverError::new(
                ErrorKind::NotFound,
                format!("no op '{other}' on image:codex"),
            )
            .with_hint("run channel_describe(image:codex)")),
        }
    }

    async fn doctor_lines(&self) -> (bool, Vec<String>) {
        (false, vec!["doctor: not implemented yet".into()])
    }
}
```

- [ ] **Step 2: Write the fake-codex tests**

Append to the `tests` module in `src/drivers/image/codex.rs`:

```rust
    use crate::config::ImageCodexCfg;
    use crate::error::ErrorKind;

    /// Write an executable `codex` fake into `dir`. `body` is sh after the
    /// shebang. Tests bake absolute output paths directly into the script —
    /// no env-var plumbing.
    fn fake_codex(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("codex");
        std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn backend(bin: &Path) -> CodexBackend {
        CodexBackend::new(ImageCodexCfg {
            enabled: true,
            codex_bin: bin.display().to_string(),
        })
    }

    #[tokio::test]
    async fn generate_argv_contract_stdin_null_and_saved_parsing() {
        let tmp = tempfile::tempdir().unwrap();
        let argv_file = tmp.path().join("argv.txt");
        let refpng = tmp.path().join("ref.png");
        std::fs::write(&refpng, b"png").unwrap();
        // `cat >/dev/null` hangs forever unless stdin is null/EOF — the 10s
        // timeout wrapper converts a stdin-discipline regression into a fail.
        let script = format!(
            "printf '%s\\n' \"$@\" > {argv}\ncat >/dev/null\necho 'model thinking noise'\necho 'SAVED: /tmp/uplink-fake-out.png'\n",
            argv = argv_file.display()
        );
        let b = backend(&fake_codex(tmp.path(), &script));
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            b.invoke(
                "generate",
                serde_json::json!({"prompt": "a cat", "refs": [refpng.to_str().unwrap()]}),
            ),
        )
        .await
        .expect("must not hang: stdin must be Stdio::null")
        .unwrap();
        assert_eq!(out["paths"], serde_json::json!(["/tmp/uplink-fake-out.png"]));

        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(&lines[..3], &["exec", "--full-auto", "--skip-git-repo-check"]);
        assert_eq!(lines[3], "--image");
        let canon = std::fs::canonicalize(&refpng).unwrap().display().to_string();
        assert_eq!(lines[4], canon);
        let instruction = lines[5..].join("\n");
        assert!(instruction.contains("a cat"));
        assert!(instruction.contains(&canon), "abs ref path must be IN the instruction text");
        assert!(instruction.contains("SAVED:"));
    }

    #[tokio::test]
    async fn edit_attaches_input_as_image() {
        let tmp = tempfile::tempdir().unwrap();
        let argv_file = tmp.path().join("argv.txt");
        let input = tmp.path().join("in.png");
        std::fs::write(&input, b"png").unwrap();
        let script = format!(
            "printf '%s\\n' \"$@\" > {argv}\necho 'SAVED: /tmp/uplink-fake-edit.png'\n",
            argv = argv_file.display()
        );
        let b = backend(&fake_codex(tmp.path(), &script));
        let out = b
            .invoke(
                "edit",
                serde_json::json!({"input": input.to_str().unwrap(), "prompt": "tint blue"}),
            )
            .await
            .unwrap();
        assert_eq!(out["paths"], serde_json::json!(["/tmp/uplink-fake-edit.png"]));
        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let canon = std::fs::canonicalize(&input).unwrap().display().to_string();
        assert!(argv.lines().any(|l| l == canon));
        assert!(argv.contains("tint blue"));
    }

    #[tokio::test]
    async fn nonzero_exit_is_upstream_with_stderr_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&fake_codex(
            tmp.path(),
            "echo 'boom: sandbox denied' >&2\nexit 3\n",
        ));
        let e = b
            .invoke("generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Upstream));
        assert!(e.evidence.unwrap().contains("sandbox denied"));
    }

    #[tokio::test]
    async fn no_saved_line_is_upstream_with_stdout_evidence() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&fake_codex(tmp.path(), "echo 'I generated it, trust me'\n"));
        let e = b
            .invoke("generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Upstream));
        assert!(e.message.contains("SAVED"));
        assert!(e.evidence.unwrap().contains("trust me"));
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable_with_install_hint() {
        let b = CodexBackend::new(ImageCodexCfg {
            enabled: true,
            codex_bin: "/nonexistent/cc-uplink-no-such-codex".into(),
        });
        let e = b
            .invoke("generate", serde_json::json!({"prompt": "x"}))
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Unavailable));
        assert!(e.hint.unwrap().contains("codex"));
    }

    #[tokio::test]
    async fn missing_ref_file_is_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&fake_codex(tmp.path(), "echo 'SAVED: /x.png'\n"));
        let e = b
            .invoke(
                "generate",
                serde_json::json!({"prompt": "x", "refs": ["/no/such/ref.png"]}),
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(e.kind, ErrorKind::Invalid));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib drivers::image::codex`
Expected: PASS (11 tests: 5 pure + 6 subprocess).

- [ ] **Step 4: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/drivers/image/codex.rs
git commit -s -m "feat(image-codex): subprocess backend with fake-codex contract tests"
```

---

### Task 8: Codex doctor + registry wiring + docs/downstream-contracts.md

**Files:**
- Modify: `src/drivers/image/codex.rs` (replace placeholder `doctor_lines`)
- Modify: `src/mcp.rs` (register codex backend)
- Create: `docs/downstream-contracts.md`

**Interfaces:**
- Consumes: `parse_codex_version`, `version_ok`, `clip` from `drivers::image`.
- Produces: real `CodexBackend::doctor_lines` (gates: present, version ≥ 0.142, login, `exec --full-auto`/`--image` advertised); `build_registry` registers the codex backend when `drivers.image-codex.enabled`.

- [ ] **Step 1: Write the failing doctor tests**

Append to the `tests` module in `src/drivers/image/codex.rs`:

```rust
    const DOCTOR_OK_SCRIPT: &str = r#"if [ "$1" = "--version" ]; then echo "codex-cli 0.144.0"; exit 0; fi
if [ "$1" = "login" ]; then echo "Logged in using ChatGPT"; exit 0; fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then echo "usage: codex exec [--full-auto] [--image <path>]"; exit 0; fi
exit 1
"#;

    #[tokio::test]
    async fn doctor_all_green() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&fake_codex(tmp.path(), DOCTOR_OK_SCRIPT));
        let (ok, lines) = b.doctor_lines().await;
        assert!(ok, "lines: {lines:?}");
        assert!(lines.iter().any(|l| l.contains("0.144.0")));
        assert!(lines.iter().any(|l| l.contains("login: ok")));
        assert!(lines.iter().any(|l| l.contains("--full-auto/--image supported")));
    }

    #[tokio::test]
    async fn doctor_old_version_degrades() {
        let tmp = tempfile::tempdir().unwrap();
        let script = DOCTOR_OK_SCRIPT.replace("0.144.0", "0.141.0");
        let b = backend(&fake_codex(tmp.path(), &script));
        let (ok, lines) = b.doctor_lines().await;
        assert!(!ok);
        assert!(lines.iter().any(|l| l.contains("TOO OLD")));
    }

    #[tokio::test]
    async fn doctor_not_logged_in_degrades() {
        let tmp = tempfile::tempdir().unwrap();
        let script = DOCTOR_OK_SCRIPT.replace(
            r#"if [ "$1" = "login" ]; then echo "Logged in using ChatGPT"; exit 0; fi"#,
            r#"if [ "$1" = "login" ]; then echo "Not logged in"; exit 1; fi"#,
        );
        let b = backend(&fake_codex(tmp.path(), &script));
        let (ok, lines) = b.doctor_lines().await;
        assert!(!ok);
        assert!(lines.iter().any(|l| l.contains("not logged in")));
    }

    #[tokio::test]
    async fn doctor_missing_binary() {
        let b = CodexBackend::new(ImageCodexCfg {
            enabled: true,
            codex_bin: "/nonexistent/cc-uplink-no-such-codex".into(),
        });
        let (ok, lines) = b.doctor_lines().await;
        assert!(!ok);
        assert!(lines[0].contains("not found"));
    }
```

Run: `cargo test --lib drivers::image::codex`
Expected: FAIL — placeholder `doctor_lines` returns the "not implemented" line.

- [ ] **Step 2: Implement doctor**

Replace the placeholder `doctor_lines` in the `ImageBackend` impl with:

```rust
    async fn doctor_lines(&self) -> (bool, Vec<String>) {
        use crate::drivers::image::clip;
        let mut ok = true;
        let mut lines = vec![];
        let Some(vout) = run_capture(&self.cfg.codex_bin, &["--version"]).await else {
            return (
                false,
                vec![format!("not found (bin '{}')", self.cfg.codex_bin)],
            );
        };
        match parse_codex_version(&vout) {
            Some(v) if version_ok(v) => {
                lines.push(format!("version: {}.{}.{} (>= 0.142)", v.0, v.1, v.2));
            }
            Some(v) => {
                ok = false;
                lines.push(format!(
                    "version: {}.{}.{} — TOO OLD, need >= 0.142",
                    v.0, v.1, v.2
                ));
            }
            None => {
                ok = false;
                lines.push(format!("version: unparseable ('{}')", clip(vout.trim(), 60)));
            }
        }
        match run_status(&self.cfg.codex_bin, &["login", "status"]).await {
            Some(true) => lines.push("login: ok".into()),
            _ => {
                ok = false;
                lines.push("login: not logged in (run `codex login`)".into());
            }
        }
        match run_capture(&self.cfg.codex_bin, &["exec", "--help"]).await {
            Some(h) if h.contains("--full-auto") && h.contains("--image") => {
                lines.push("exec: --full-auto/--image supported".into());
            }
            _ => {
                ok = false;
                lines.push("exec: --full-auto/--image not confirmed".into());
            }
        }
        (ok, lines)
    }
```

And add the two helpers at module level (near `abs_existing`):

```rust
/// Run `bin args…` (stdin null, 5 s cap) and return stdout, or None if the
/// binary is missing/unrunnable/timed out.
async fn run_capture(bin: &str, args: &[&str]) -> Option<String> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new(bin)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Like `run_capture` but only reports whether the command exited 0.
async fn run_status(bin: &str, args: &[&str]) -> Option<bool> {
    let st = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new(bin)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await
    .ok()?
    .ok()?;
    Some(st.success())
}
```

Note: `codex login status` — with the fake script above, `[ "$1" = "login" ]` matches regardless of `$2`; the real CLI accepts `codex login status` (0.142+) and exits 0 when logged in. That contract goes in `docs/downstream-contracts.md` below.

Run: `cargo test --lib drivers::image::codex`
Expected: PASS (15 tests).

- [ ] **Step 3: Register the backend**

In `src/mcp.rs` `build_registry`, after the `image_openai` push, add:

```rust
    if cfg.drivers.image_codex.enabled {
        backends.push(Box::new(crate::drivers::image::codex::CodexBackend::new(
            cfg.drivers.image_codex,
        )));
    }
```

Run: `cargo test`
Expected: full suite PASS.

- [ ] **Step 4: Write docs/downstream-contracts.md**

Create `docs/downstream-contracts.md`:

```markdown
# Downstream Contracts

The contracts below are load-bearing and version-drifting. Any change to the
code that speaks them MUST update this file in the same commit
(discipline inherited from codex-image-in-cc).

## OpenAI Images API (`src/drivers/image/openai.rs`)

- Base URL: `drivers.image-openai.base_url` (default `https://api.openai.com/v1`).
- Auth: `Authorization: Bearer <key>`; key read from the env var named by
  `api_key_env` at call time; never stored, never logged.
- `generate` without refs → `POST {base}/images/generations`, JSON body
  `{model, prompt, n?, size?, quality?}` — absent options are omitted, never null.
- `generate` with refs → `POST {base}/images/edits`, multipart form:
  text fields `model`, `prompt`, optional `n`/`size`/`quality`; each ref is a
  file part named `image[]` (gpt-image-1 multi-image form).
- `edit` → `POST {base}/images/edits`, multipart form: `model`, `prompt`,
  input file part `image`, optional file part `mask`.
- Response contract: `200` JSON `{"data": [{"b64_json": "<base64 png>"}]}`.
  `gpt-image-1` always returns `b64_json` (no `url` variant is handled).
- Files are written to `out_dir` (default `./uplink-images/`) as
  `<compact-UTC-ts>-<n>.png`; results carry absolute paths.

## Codex CLI (`src/drivers/image/codex.rs`)

- Spawn: `<codex_bin> exec --full-auto --skip-git-repo-check
  [--image <abs>]... "<instruction>"` — argv vector, never a shell.
- stdin MUST be `Stdio::null()`: codex exec blocks reading stdin otherwise
  (documented hang, inherited from codex-image-in-cc).
- Reference/input images are passed BOTH as `--image <abs>` flags and as
  absolute paths inside the instruction text: the 0.144+ extension-backed
  image tool reads `referenced_image_paths` from the text, while
  0.142–0.143 use the `--image` attachments. Dropping either side breaks one
  version range.
- Output contract: the instruction requires one `SAVED: <absolute path>`
  line per saved image; stdout is parsed ONLY for those lines. Everything
  else codex prints is ignored — never re-parse LLM prose.
- Doctor gates: `codex --version` parseable and ≥ 0.142.0;
  `codex login status` exits 0 when logged in; `codex exec --help`
  advertises `--full-auto` and `--image`.
- Timeout: one codex run is capped at 600 s (`Timeout` error beyond that).

## Changelog

- 2026-07-22: initial version (M3 openai + M4 codex as shipped).
```

- [ ] **Step 5: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add src/drivers/image/codex.rs src/mcp.rs docs/downstream-contracts.md
git commit -s -m "feat(image-codex): doctor gates, registry wiring, downstream contracts doc"
```

---

### Task 9: Companion skill + `setup` subcommand

**Files:**
- Create: `skills/uplink/SKILL.md`
- Modify: `src/cli/mod.rs` (`src/main.rs` needs no change — it pass-through-dispatches any subcommand to `cli::run`, and the usage string lives in `cli::run`'s unknown-command arm)

**Interfaces:**
- Consumes: `dirs::home_dir`, `tokio::process::Command`.
- Produces: `cli::mcp_add_args(exe: &str) -> Vec<String>`; `cli::install_skill(claude_home: &Path) -> std::io::Result<PathBuf>`; `cli::run_setup(claude_bin: &str, claude_home: &Path) -> anyhow::Result<()>`; dispatch arm `"setup"`.

- [ ] **Step 1: Write the skill**

Create `skills/uplink/SKILL.md`:

```markdown
---
name: uplink
description: Use when messaging another agent or tmux pane (ask codex, talk to a peer, cross-pane communication) or when generating/editing images — cc-uplink's channel_list/channel_describe/channel_send/channel_invoke/channel_recv/channel_doctor tools
---

# uplink — outbound channels

cc-uplink gives you exactly six tools for everything outside your session.
Channels are addressed `<driver>:<address>`: `tmux:%3`, `tmux:codex` (pane
label), `image:openai`, `image:codex`.

## Rules

1. **Discover first.** `channel_list()` shows every live channel. Pane labels
   (`tmux:codex`) beat raw pane ids — they survive pane reshuffles.
2. **Describe before first invoke.** Before the FIRST `channel_invoke` of any
   op in a session, call `channel_describe(channel, op)` and follow the
   schema exactly. Do not guess args.
3. **Never poll for replies.** After `channel_send`, the peer's reply arrives
   in YOUR pane as a `[reply id:…]` line — you will see it as input.
   `channel_recv` is an audit/recovery log, not a mailbox. Do not loop on it.
4. **Push vs pull for peer agents (e.g. Codex in another pane):**
   - Default: `channel_send(tmux:codex, …)` — async; the reply comes to you.
   - Need the peer's complete output now: `channel_invoke(tmux:codex, "ask",
     {message, quiet_ms?, timeout_ms?})` — mechanized round-trip that returns
     everything the peer printed since your question (may include TUI chrome).
5. **keys guard:** `channel_invoke(tmux:X, "keys", …)` requires a `read` of
   that pane within the last 60 s. Read, look, then press.
6. **Send failures carry evidence.** A failed send receipt/error includes a
   capture excerpt. Read it and decide; cc-uplink never auto-retries.
7. **Images are invoke-only.** `channel_send` to `image:*` is rejected.
   - `image:openai` — direct API (needs OPENAI_API_KEY): `generate
     {prompt, n?, size?, quality?, refs?, out_dir?}`, `edit {input, prompt,
     mask?}`. Returns absolute file paths.
   - `image:codex` — borrows Codex CLI's imagegen (needs `codex login`, no
     API key): `generate {prompt, refs?}`, `edit {input, prompt}`. Express
     size/count/output location in the prompt text.
8. **Something broken? `channel_doctor()` first.** It names the missing
   piece (tmux version, transport, API key, codex login) before you debug.

## Examples

- Message a peer pane: `channel_send {channel: "tmux:codex", message: "review my diff in /tmp/x.diff please"}`
- Guaranteed full answer: `channel_invoke {channel: "tmux:codex", op: "ask", args: {message: "summarize your findings"}}`
- Generate an image: `channel_invoke {channel: "image:openai", op: "generate", args: {prompt: "watercolor lighthouse, 1024x1024", size: "1024x1024"}}`
- Edit with codex: `channel_invoke {channel: "image:codex", op: "edit", args: {input: "./logo.png", prompt: "make the background transparent"}}`
```

- [ ] **Step 2: Write the failing setup tests**

Append to the `tests` module in `src/cli/mod.rs`:

```rust
    #[test]
    fn mcp_add_args_golden() {
        assert_eq!(
            mcp_add_args("/opt/bin/cc-uplink"),
            vec!["mcp", "add", "-s", "user", "cc-uplink", "--", "/opt/bin/cc-uplink", "serve"]
        );
    }

    #[test]
    fn install_skill_writes_uplink_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let p = install_skill(tmp.path()).unwrap();
        assert_eq!(p, tmp.path().join("skills/uplink/SKILL.md"));
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.starts_with("---"));
        assert!(body.contains("name: uplink"));
        assert!(body.contains("channel_describe"));
    }

    #[tokio::test]
    async fn run_setup_calls_claude_and_installs_skill() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let argv_file = tmp.path().join("argv.txt");
        let fake = tmp.path().join("claude");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n", argv_file.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let home = tmp.path().join("claude-home");
        run_setup(fake.to_str().unwrap(), &home).await.unwrap();
        assert!(home.join("skills/uplink/SKILL.md").exists());
        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(&lines[..6], &["mcp", "add", "-s", "user", "cc-uplink", "--"]);
        assert_eq!(lines[7], "serve");
        assert!(lines[6].ends_with(std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap()));
    }

    #[tokio::test]
    async fn run_setup_missing_claude_is_actionable() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        let e = run_setup("/nonexistent/cc-uplink-no-claude", &home)
            .await
            .err()
            .unwrap();
        assert!(e.to_string().contains("run manually"));
        // skill install happens BEFORE the claude call, so it must exist
        assert!(home.join("skills/uplink/SKILL.md").exists());
    }
```

Run: `cargo test --lib cli`
Expected: FAIL — `mcp_add_args`/`install_skill`/`run_setup` don't exist.

- [ ] **Step 3: Implement setup**

Add to `src/cli/mod.rs` (above `run`):

```rust
pub(crate) const SKILL_MD: &str = include_str!("../../skills/uplink/SKILL.md");

/// argv (after the binary) for `claude mcp add …` — kept as data so tests
/// pin the exact registration command.
pub(crate) fn mcp_add_args(exe: &str) -> Vec<String> {
    ["mcp", "add", "-s", "user", "cc-uplink", "--", exe, "serve"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Install the embedded companion skill under `<claude_home>/skills/uplink/`.
/// Overwrites an existing copy (reinstall = upgrade).
pub(crate) fn install_skill(claude_home: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let dir = claude_home.join("skills").join("uplink");
    std::fs::create_dir_all(&dir)?;
    let p = dir.join("SKILL.md");
    std::fs::write(&p, SKILL_MD)?;
    Ok(p)
}

/// `cc-uplink setup`: install the skill, then register the MCP server via
/// the `claude` CLI. Skill first — it must survive a missing `claude`.
pub(crate) async fn run_setup(
    claude_bin: &str,
    claude_home: &std::path::Path,
) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let skill = install_skill(claude_home)?;
    println!("installed skill: {}", skill.display());
    let args = mcp_add_args(&exe.display().to_string());
    let st = tokio::process::Command::new(claude_bin)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .status()
        .await;
    match st {
        Ok(s) if s.success() => {
            println!("registered MCP server: {claude_bin} {}", args.join(" "));
            println!("restart Claude Code to load the cc-uplink tools");
            Ok(())
        }
        Ok(s) => anyhow::bail!(
            "'{claude_bin} {}' exited with {s} — run manually to see why",
            args.join(" ")
        ),
        Err(e) => anyhow::bail!(
            "cannot run '{claude_bin}': {e}\nrun manually: claude {}",
            args.join(" ")
        ),
    }
}
```

Add the dispatch arm in `run` (before the `other =>` arm):

```rust
        "setup" => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
                .join(".claude");
            run_setup("claude", &home).await
        }
```

Update the usage string in `cli::run`'s `other` arm to:
`usage: cc-uplink [serve|doctor|send|invoke|log|setup]`

- [ ] **Step 4: Run tests**

Run: `cargo test --lib cli`
Expected: PASS (5 tests: 1 existing + 4 new).

- [ ] **Step 5: Gate + commit**

```bash
cargo fmt --check && cargo clippy --release --all-targets
git add skills/uplink/SKILL.md src/cli/mod.rs
git commit -s -m "feat(setup): companion skill install + claude mcp registration"
```

---

### Task 10: LICENSE, README, wire-contract doc

**Files:**
- Create: `LICENSE`
- Create: `README.md`
- Create: `docs/wire-contract.md`

**Interfaces:** none (documentation).

- [ ] **Step 1: LICENSE**

Create `LICENSE` with the standard MIT text:

```
MIT License

Copyright (c) 2026 tonyhu

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: docs/wire-contract.md**

Create `docs/wire-contract.md`:

```markdown
# Wire Contract (Driver trait)

One page mirroring `src/core/driver.rs`. Any change to the trait or its DTOs
is a protocol change: update this page and its changelog in the same commit.
These shapes are the future v3 out-of-process driver protocol — keep every
type serde-serializable, no lifetimes/callbacks/trait objects in payloads,
zero shared mutable state between core and drivers.

## Trait

| Method | In | Out |
|---|---|---|
| `info()` | — | `DriverInfo {id, kind: messaging\|capability\|both, summary}` |
| `channels()` | — | `Vec<ChannelEntry {channel, labels[], detail}>` |
| `ops()` | — | `Vec<OpSpec {op, summary, params_schema, result_schema}>` |
| `send(addr, SendRequest {message, reply_hint: full\|short\|none})` | | `SendReceipt {delivered, correlation_id, verify_excerpt?, injected_at}` |
| `invoke(addr, op, args: Value)` | | `Value` |
| `recv(cursor: Option<u64>)` | | `RecvBatch {items: [{cursor, at, from?, id?, raw}], next_cursor}` |
| `doctor()` | — | `DoctorReport {driver, ok, lines[]}` |

Errors: `DriverError {kind: NotFound|Unavailable|Rejected|Timeout|Upstream|Invalid, message, hint?, evidence?}`, rendered as
`uplink error [<driver>:<Kind>]: <message> — hint: <hint>`.

## Semantics notes

- `SendReceipt` carries evidence, not sentiment: `delivered` is only true
  after mechanized verification; failures carry capture evidence instead.
- `recv` cursors are per-registry-merge in v1 (single shared cursor space);
  MUST become per-driver before a second recv-bearing driver ships
  (spec §19).
- Composite drivers are legal: the `image` driver multiplexes backends on
  the address part; the Registry only routes on the prefix.

## Changelog

- 2026-07-22: initial contract as implemented by M1/M2 (tmux) — note:
  transport reconnect is on-demand inside the tmux driver, not a supervised
  background loop (spec §19).
- 2026-07-22: `image` composite driver added (M3/M4). No trait changes.
```

- [ ] **Step 3: README.md**

Create `README.md`:

```markdown
# cc-uplink

Claude Code's unified outbound channel layer: one Rust binary, one stdio MCP
server, **six fixed tools** — with pluggable drivers underneath. Adding a
driver never adds a tool, so your tool/skill listing budget stays flat no
matter how many ways Claude can reach the outside world.

| Tool | Purpose |
|---|---|
| `channel_list()` | Enumerate channels across drivers |
| `channel_describe(channel, op?)` | On-demand JSON Schema for an op |
| `channel_send(channel, message, opts?)` | Async message (tmux: inject → verify → Enter, evidence-bearing receipt) |
| `channel_invoke(channel, op, args)` | Capability call (tmux ops, image generate/edit) |
| `channel_recv(cursor?)` | Drain inbound envelope audit log |
| `channel_doctor()` | Aggregated per-driver diagnostics |

## Channels

- **`tmux:%3` / `tmux:<label>`** — talk to whatever runs in another tmux pane
  (Codex CLI, another Claude, a shell). Control-mode-first (`tmux -C`),
  event-driven verify; peers install nothing — the message envelope teaches
  them how to reply with plain `tmux send-keys`. Ops: `read`, `keys`
  (read-guarded), `label`, `await_idle`, `ask` (mechanized round-trip that
  captures everything the peer printed since your question).
- **`image:openai`** — direct OpenAI Images API (`gpt-image-1`), rustls only,
  key from env. Ops: `generate`, `edit`. Files land in `./uplink-images/`,
  absolute paths returned.
- **`image:codex`** — borrows Codex CLI's built-in imagegen via
  `codex exec --full-auto` (ChatGPT login, no API key needed). Ops:
  `generate`, `edit`.

## Install

```bash
cargo build --release
./target/release/cc-uplink setup   # installs companion skill + `claude mcp add`
```

Or register manually:

```bash
claude mcp add -s user cc-uplink -- /path/to/cc-uplink serve
```

Requirements: tmux ≥ 3.2 for the full tmux feature set (3.5a is the reference
environment; a one-shot CLI fallback covers older tmux), `OPENAI_API_KEY` for
`image:openai`, `@openai/codex` ≥ 0.142 + `codex login` for `image:codex`.

## Configuration

`~/.config/cc-uplink/config.toml` (all optional):

```toml
[drivers.tmux]
enabled = true
# allowlist = ["%1", "codex"]     # optional send-target allowlist

[drivers.image-openai]
enabled = true
api_key_env = "OPENAI_API_KEY"    # names the variable; the key itself is env-only
model = "gpt-image-1"

[drivers.image-codex]
enabled = true
codex_bin = "codex"
```

## CLI

Same binary, same drivers, no LLM required:

```bash
cc-uplink doctor                       # diagnostics, CI-friendly exit code
cc-uplink send tmux:codex "hello"      # full mechanized send cycle
cc-uplink invoke image:openai generate '{"prompt":"a lighthouse"}'
cc-uplink log --follow                 # correlation-id-threaded conversation log
cc-uplink setup                        # register MCP server + install skill
```

## Security posture

- argv vectors only — prompts/messages/paths are never shell text
- refuses to send to its own pane (loop prevention); optional allowlists
- secrets are env-only; config names variables, never values
- injected envelopes are visible plaintext in panes **by design** — human
  observability is a feature; don't put secrets in messages
- send verification never auto-retries; failures return capture evidence

## Development

```bash
cargo test          # unit + integration (integration spins private-socket tmux servers)
cargo fmt --check && cargo clippy --release --all-targets
```

Design spec: `docs/superpowers/specs/2026-07-22-cc-uplink-design.md`.
Driver wire contract: `docs/wire-contract.md`.
Downstream contracts (OpenAI API, Codex CLI): `docs/downstream-contracts.md`.

## Releasing

Versioning/changelog via [release-plz](https://release-plz.dev), artifacts via
[cargo-dist](https://opensource.axo.dev/cargo-dist/) (`dist-workspace.toml`):
static musl Linux (x86_64/aarch64) + macOS (x86_64/aarch64). Windows is
WSL-only, tier-2. Configs are inert until the repo has a public remote.

## License

[MIT](LICENSE)
```

- [ ] **Step 4: Verify + commit**

Run: `cargo test` (docs must not break anything — sanity only).

```bash
git add LICENSE README.md docs/wire-contract.md
git commit -s -m "docs: LICENSE (MIT), README, wire-contract page"
```

---

### Task 11: Release automation configs + final gate

**Files:**
- Create: `release-plz.toml`
- Create: `dist-workspace.toml`

**Interfaces:** none (release tooling).

- [ ] **Step 1: release-plz.toml**

```toml
[workspace]
changelog_update = true
git_tag_enable = true
# Internal-first: no GitHub releases until the repo is public.
git_release_enable = false
```

- [ ] **Step 2: dist-workspace.toml**

```toml
[workspace]
members = ["cargo:."]

# See https://opensource.axo.dev/cargo-dist/
[dist]
cargo-dist-version = "0.28.0"
ci = "github"
installers = ["shell"]
targets = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
]
install-updater = false
```

(Do NOT run `dist init`/`dist generate` here — CI workflow generation needs a
GitHub remote and the `dist` tool; the config is checked in so the first
`dist init` run after the repo goes public is a no-op re-generation. This is
documented in README §Releasing.)

- [ ] **Step 3: Verify the TOML parses**

Run:
```bash
python3 -c "import tomllib; tomllib.load(open('release-plz.toml','rb')); tomllib.load(open('dist-workspace.toml','rb')); print('toml ok')"
```
Expected: `toml ok`

- [ ] **Step 4: Full final gate**

Run, in order:
```bash
cargo fmt --check
cargo clippy --release --all-targets
cargo test            # lib + tmux integration (requires tmux ≥3.2 on the host)
cargo tree -e normal | grep -i openssl || echo "no openssl ✔"
```
Expected: fmt clean; clippy clean; `test result: ok` for lib suite AND `tmux_integration`; `no openssl ✔`.

- [ ] **Step 5: Commit**

```bash
git add release-plz.toml dist-workspace.toml
git commit -s -m "chore(release): release-plz + cargo-dist configs"
```

---

## Self-Review Notes

- **Spec coverage:** §6 → Tasks 1–5; §7 → Tasks 6–8; §9 `setup` → Task 9; §10 skill → Task 9; §11 config → Tasks 1, 6; §13 test rows (wiremock, fake-codex, doctor matrices) → Tasks 4, 7, 8; §14 distribution → Tasks 10–11. Not in scope (per spec): §16 roadmap items, §19 deferred follow-ups (unchanged by this plan: image driver's `recv` returns empty batches, so the shared-cursor limitation is not worsened).
- **Type consistency:** `ImageBackend` signature identical in Tasks 2/4/7; `ImageOpenAiCfg`/`ImageCodexCfg` fields match between config tasks and backend constructors; `clip`/`clip_tail` defined once (Task 2), consumed Tasks 4/7/8.
- **Known simplifications (accepted):** `channel_describe(image:openai)` returns both backends' op entries (trait has no per-address ops); summaries carry `[openai]`/`[codex]` tags to disambiguate — documented in wire-contract.md. `edit` on `image:openai` writes to the CWD-relative default dir (no `out_dir` param, per spec).
```
