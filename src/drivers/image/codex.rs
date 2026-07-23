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

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::ImageCodexCfg;
use crate::core::driver::OpSpec;
use crate::drivers::image::{ImageBackend, clip_tail};
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
            let ev = if stderr.trim().is_empty() {
                &stdout
            } else {
                &stderr
            };
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
                lines.push(format!(
                    "version: unparseable ('{}')",
                    clip(vout.trim(), 60)
                ));
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
        assert_eq!(
            parse_saved_lines(stdout),
            vec!["/tmp/one.png", "/tmp/two.png"]
        );
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
        assert_eq!(
            out["paths"],
            serde_json::json!(["/tmp/uplink-fake-out.png"])
        );

        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(
            &lines[..3],
            &["exec", "--full-auto", "--skip-git-repo-check"]
        );
        assert_eq!(lines[3], "--image");
        let canon = std::fs::canonicalize(&refpng)
            .unwrap()
            .display()
            .to_string();
        assert_eq!(lines[4], canon);
        let instruction = lines[5..].join("\n");
        assert!(instruction.contains("a cat"));
        assert!(
            instruction.contains(&canon),
            "abs ref path must be IN the instruction text"
        );
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
        assert_eq!(
            out["paths"],
            serde_json::json!(["/tmp/uplink-fake-edit.png"])
        );
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
        assert!(
            lines
                .iter()
                .any(|l| l.contains("--full-auto/--image supported"))
        );
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
}
