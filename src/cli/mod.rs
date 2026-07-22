//! Human CLI: `doctor` / `send` / `invoke` / `log`.
//!
//! This is the direct, non-MCP entry point a person uses at a terminal to
//! exercise the same driver registry the MCP server (`mcp::serve`) exposes.
//! Every subcommand builds its own registry from config — there's no shared
//! long-lived process here, just one-shot invocations.

use crate::core::driver::{ReplyHint, SendRequest};

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
        other => {
            eprintln!("unknown command '{other}'\nusage: cc-uplink [serve|doctor|send|invoke|log]");
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
}
