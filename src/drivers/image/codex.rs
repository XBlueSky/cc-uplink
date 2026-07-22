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
}
