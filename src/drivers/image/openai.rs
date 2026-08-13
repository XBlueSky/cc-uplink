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
            DriverError::new(
                ErrorKind::Upstream,
                format!("invalid base64 image data: {e}"),
            )
        })
}

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::config::ImageOpenAiCfg;
use crate::core::driver::OpSpec;
use crate::drivers::image::{ImageBackend, clip};

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
        DriverError::new(
            ErrorKind::Invalid,
            format!("cannot read image file '{path}': {e}"),
        )
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

    async fn parse_images_response(resp: reqwest::Response) -> Result<Vec<Vec<u8>>, DriverError> {
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
            DriverError::new(
                ErrorKind::Upstream,
                format!("unparseable openai response: {e}"),
            )
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
        let dir = PathBuf::from(
            a.out_dir
                .clone()
                .unwrap_or_else(|| "./uplink-images".into()),
        );
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
                mutating: true,
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
                mutating: true,
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
        // Cleanup before asserts — but remove ONLY the file this test wrote:
        // ./uplink-images in the crate root is a real output directory, and a
        // blanket remove_dir_all here deletes the user's generated images on
        // every `cargo test` (observed live). remove_dir keeps the directory
        // unless this test's file was the only content.
        std::fs::remove_file(&written).ok();
        std::fs::remove_dir("./uplink-images").ok();
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
        assert!(
            ok,
            "reachable wiremock + key present ⇒ ok; lines: {lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("key: present")));
        assert!(
            !lines.iter().any(|l| l.contains("sk-test")),
            "key value must never leak"
        );

        let b2 = OpenAiBackend::new(cfg("http://127.0.0.1:9", "CC_UPLINK_T_OPENAI_DOC"));
        let (ok2, lines2) = b2.doctor_lines().await;
        assert!(!ok2);
        assert!(lines2.iter().any(|l| l.contains("UNREACHABLE")));
    }
}
