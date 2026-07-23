# Remove `cc-uplink setup` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the `cc-uplink setup` subcommand and everything that exists only to serve it; the Claude Code plugin is the supported install path.

**Architecture:** Pure deletion in one file (`src/cli/mod.rs`) plus doc/marketspec cleanup. No new code, no new tests — the deliverable's test cycle is the existing suite passing after removal and grep proving no live references remain.

**Tech Stack:** Rust (deletion only), cc-marketspec check.

## Global Constraints

- No `Cargo.toml` / `Cargo.lock` changes — `dirs` and `tempfile` stay (used by config.rs/logsink.rs and image drivers/tests respectively).
- Historical specs/plans under `docs/superpowers/` are untouched.
- Single commit: `refactor(cli)!: remove cc-uplink setup — plugin install supersedes it`.
- Verification gate: `cargo test`, `cargo fmt --check`, `cargo clippy --release --all-targets`, cc-marketspec check, reference grep.

---

### Task 1: Delete the setup chain

**Files:**
- Modify: `src/cli/mod.rs` (delete lines 10–62, the `"setup"` arm at 176–181, usage string at 184, tests at 208–284)
- Modify: `README.md` (migration block 53–59, manual-install 68–84, CLI line 118)
- Modify: `.cc-marketspec/entries/plugin-cc-uplink.yaml` (middle trap bullet)

**Interfaces:**
- Consumes: nothing.
- Produces: `src/cli/mod.rs` keeps `format_log_line(&serde_json::Value) -> String` and `pub async fn run(cmd: &str, rest: &[String]) -> anyhow::Result<()>` with arms `doctor|send|invoke|log` only.

- [x] **Step 1: Delete the setup chain in `src/cli/mod.rs`**

Delete the contiguous block lines 10–62 — `SKILL_MD` const, `mcp_add_args()`, `install_skill()`, `run_setup()` — so the file goes from the `use` line straight to the `format_log_line` doc comment:

```rust
use crate::core::driver::{ReplyHint, SendRequest};

/// Format a single JSONL log record (as produced by `core::logsink::LogSink`)
```

- [x] **Step 2: Delete the `"setup"` dispatch arm and fix the usage string**

Remove:

```rust
        "setup" => {
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
                .join(".claude");
            run_setup("claude", &home).await
        }
```

Change the unknown-command message to:

```rust
        other => {
            eprintln!("unknown command '{other}'\nusage: cc-uplink [serve|doctor|send|invoke|log]");
            std::process::exit(2);
        }
```

- [x] **Step 3: Delete the four setup tests**

Remove the contiguous test block (lines 208–284): `mcp_add_args_golden`, `install_skill_writes_uplink_skill`, `run_setup_calls_claude_and_installs_skill`, `run_setup_missing_claude_is_actionable`. The `tests` module keeps only `formats_in_and_out_lines`.

- [x] **Step 4: README — delete the migration block**

Delete lines 53–59 entirely:

```markdown
Migrating from `cc-uplink setup`? Remove the old user-scope copies so tools
and skills aren't listed twice:

```bash
claude mcp remove -s user cc-uplink
rm -rf ~/.claude/skills/uplink   # the plugin ships its own copy
```
```

- [x] **Step 5: README — rewrite the manual-install steps**

Replace lines 68–84 (the three code fences `./cc-uplink setup`, `cargo build … setup`, "Or register manually") with:

```markdown
```bash
curl -fsSL https://github.com/XBlueSky/cc-uplink/releases/latest/download/cc-uplink-x86_64-unknown-linux-musl.tar.gz | tar xz
claude mcp add -s user cc-uplink -- "$PWD/cc-uplink" serve
```

Or build from source:

```bash
cargo build --release
claude mcp add -s user cc-uplink -- "$PWD/target/release/cc-uplink" serve
```

(For a non-Claude MCP client, point it at `cc-uplink serve` however it
registers stdio servers.) The manual path registers the tools only; to get
the `uplink` skill without the plugin, copy
`plugins/cc-uplink/skills/uplink/SKILL.md` to `~/.claude/skills/uplink/`.
```

- [x] **Step 6: README — drop the CLI example line**

Delete line 118:

```markdown
cc-uplink setup                        # register MCP server + install skill
```

- [x] **Step 7: Marketspec entry — drop the migration trap**

In `.cc-marketspec/entries/plugin-cc-uplink.yaml` delete the middle bullet:

```yaml
  - Migrating from `cc-uplink setup`? Run `claude mcp remove -s user
    cc-uplink` and delete ~/.claude/skills/uplink, or tools and skill both
    list twice.
```

- [x] **Step 8: Verify no live references**

Run: `grep -rn "SKILL_MD\|install_skill\|mcp_add_args\|run_setup" src/ tests/`
Expected: no output.

Run: `grep -rn "cc-uplink setup" README.md .cc-marketspec/`
Expected: no output.

- [x] **Step 9: Full verification gate**

Run: `cargo test`
Expected: all pass (remaining cli test: `formats_in_and_out_lines`).

Run: `cargo fmt --check && cargo clippy --release --all-targets`
Expected: clean.

Run: `npx @xbluesky/cc-marketspec@latest --check`
Expected: `OK — 1 plugins, 0 warning(s)`.

- [x] **Step 10: Commit**

```bash
git add src/cli/mod.rs README.md .cc-marketspec/entries/plugin-cc-uplink.yaml
git commit -m "refactor(cli)!: remove cc-uplink setup — plugin install supersedes it"
```
