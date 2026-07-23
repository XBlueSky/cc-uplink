//! Human CLI: `doctor` / `send` / `invoke` / `log`.
//!
//! This is the direct, non-MCP entry point a person uses at a terminal to
//! exercise the same driver registry the MCP server (`mcp::serve`) exposes.
//! Every subcommand builds its own registry from config — there's no shared
//! long-lived process here, just one-shot invocations.

use crate::core::driver::{ReplyHint, SendRequest};

pub(crate) const SKILL_MD: &str =
    include_str!("../../plugins/cc-uplink/skills/uplink/SKILL.md");

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
    let skill = install_skill(claude_home)?;
    println!("installed skill: {}", skill.display());
    let exe = std::env::current_exe()?;
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

/// Format a single JSONL log record (as produced by `core::logsink::LogSink`)
/// into the fixed-width text line `cc-uplink log` prints. Pure and
/// side-effect free so it's unit-testable without a log file on disk.
///
/// Column layout: `ts  dir who             id body`, where `dir` is
/// left-padded to 3 chars and `who` (the `from` field for inbound records,
/// `channel` for outbound ones) is left-padded to 15 chars.
pub fn format_log_line(v: &serde_json::Value) -> String {
    let ts = v["ts"].as_str().unwrap_or("?");
    let dir = v["dir"].as_str().unwrap_or("?");
    let who = v["from"].as_str().or(v["channel"].as_str()).unwrap_or("-");
    let id = v["id"].as_str().unwrap_or("-");
    let body = v["raw"].as_str().or(v["excerpt"].as_str()).unwrap_or("");
    format!("{ts}  {dir:<3} {who:<15} {id} {body}")
}

/// Dispatch a human-CLI subcommand. `cmd` is the subcommand name (already
/// stripped of argv[0]); `rest` is everything after it.
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
                for l in &r.lines {
                    println!("  {l}");
                }
            }
            if reports.is_empty() {
                println!("(no drivers active)");
                ok = false;
            }
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        "send" => {
            let (channel, msg) = match rest {
                [c, m @ ..] if !m.is_empty() => (c.clone(), m.join(" ")),
                _ => {
                    eprintln!("usage: cc-uplink send <channel> <message...>");
                    std::process::exit(2);
                }
            };
            let reg = crate::mcp::build_registry().await;
            let (d, addr) = reg
                .driver_for(&channel)
                .map_err(|e| anyhow::anyhow!(e.render("core")))?;
            let r = d
                .send(
                    &addr,
                    SendRequest {
                        message: msg,
                        reply_hint: ReplyHint::Full,
                    },
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.render(channel.split(':').next().unwrap_or("?"))))?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        "invoke" => {
            let (channel, op, args) = match rest {
                [c, o] => (c.clone(), o.clone(), serde_json::json!({})),
                [c, o, j] => (c.clone(), o.clone(), serde_json::from_str(j)?),
                _ => {
                    eprintln!("usage: cc-uplink invoke <channel> <op> [json-args]");
                    std::process::exit(2);
                }
            };
            let reg = crate::mcp::build_registry().await;
            let (d, addr) = reg
                .driver_for(&channel)
                .map_err(|e| anyhow::anyhow!(e.render("core")))?;
            let out = d
                .invoke(&addr, &op, args)
                .await
                .map_err(|e| anyhow::anyhow!(e.render(channel.split(':').next().unwrap_or("?"))))?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        "log" => {
            let follow = rest.iter().any(|a| a == "--follow");
            let Some(path) = crate::core::logsink::log_path() else {
                eprintln!("no state dir available");
                std::process::exit(1);
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
                if !follow {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Ok(())
        }
        "setup" => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
                .join(".claude");
            run_setup("claude", &home).await
        }
        other => {
            eprintln!(
                "unknown command '{other}'\nusage: cc-uplink [serve|doctor|send|invoke|log|setup]"
            );
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_in_and_out_lines() {
        let inl = format_log_line(&serde_json::json!({
            "ts":"T","dir":"in","from":"codex","id":"cafe0001","raw":"[reply id:cafe0001] done"}));
        assert_eq!(
            inl,
            "T  in  codex           cafe0001 [reply id:cafe0001] done"
        );
        let outl = format_log_line(&serde_json::json!({
            "ts":"T","dir":"out","channel":"tmux:codex","id":"ab12cd34","excerpt":"ping"}));
        assert_eq!(outl, "T  out tmux:codex      ab12cd34 ping");
    }

    #[test]
    fn mcp_add_args_golden() {
        assert_eq!(
            mcp_add_args("/opt/bin/cc-uplink"),
            vec![
                "mcp",
                "add",
                "-s",
                "user",
                "cc-uplink",
                "--",
                "/opt/bin/cc-uplink",
                "serve"
            ]
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
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
                argv_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let home = tmp.path().join("claude-home");
        run_setup(fake.to_str().unwrap(), &home).await.unwrap();
        assert!(home.join("skills/uplink/SKILL.md").exists());
        let argv = std::fs::read_to_string(&argv_file).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(
            &lines[..6],
            &["mcp", "add", "-s", "user", "cc-uplink", "--"]
        );
        assert_eq!(lines[7], "serve");
        assert!(
            lines[6].ends_with(
                std::env::current_exe()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
        );
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
}
