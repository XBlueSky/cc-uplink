# Remove `cc-uplink setup` — design

Date: 2026-07-23
Status: approved

## Goal

Delete the `cc-uplink setup` subcommand and everything that exists only to
serve it. The Claude Code plugin (`/plugin install cc-uplink@cc-uplink`) is
the supported install path; the owner has confirmed the `setup` path has no
users, so no deprecation cycle and no migration notes are needed.

## Changes

### `src/cli/mod.rs` (only code file touched)

Delete the whole setup chain — each item is used by nothing else:

- `SKILL_MD` const (the `include_str!` embed of
  `plugins/cc-uplink/skills/uplink/SKILL.md`). After this the binary no
  longer embeds the skill; the plugin ships it instead.
- `mcp_add_args()`
- `install_skill()`
- `run_setup()`
- the `"setup"` arm in `run()`; the unknown-command usage string becomes
  `cc-uplink [serve|doctor|send|invoke|log]`
- tests: `mcp_add_args_golden`, `install_skill_writes_uplink_skill`,
  `run_setup_calls_claude_and_installs_skill`,
  `run_setup_missing_claude_is_actionable`

`formats_in_and_out_lines` and everything else in the file stays.

### `README.md`

- Delete the "Migrating from `cc-uplink setup`?" block (both prose and the
  `claude mcp remove` code fence).
- Manual-install section: replace the `./cc-uplink setup` step with direct
  registration — `claude mcp add -s user cc-uplink -- /path/to/cc-uplink
  serve` (previously listed as the third alternative; now the only manual
  path).
- Add one sentence: the manual path registers the tools only; to get the
  `uplink` skill without the plugin, copy
  `plugins/cc-uplink/skills/uplink/SKILL.md` to `~/.claude/skills/uplink/`.
- CLI section: drop the `cc-uplink setup` example line.

### `.cc-marketspec/entries/plugin-cc-uplink.yaml`

- Drop the "Migrating from `cc-uplink setup`?" trap bullet. Re-run
  cc-marketspec check afterwards.

## Not changed

- `Cargo.toml` / `Cargo.lock` — `dirs` (config.rs, logsink.rs) and
  `tempfile` (image drivers, tests/common) remain used elsewhere.
- Historical specs/plans under `docs/superpowers/` — historical record.

## Verification

- `cargo test` + `cargo fmt --check` + `cargo clippy --release
  --all-targets` locally (Rust toolchain being installed on this machine as
  part of this session), plus CI on push.
- `grep -rn "setup\|SKILL_MD\|install_skill\|mcp_add_args\|run_setup"` over
  `src/ tests/` shows no live references.
- cc-marketspec check passes after the entry edit.

## Commit

One commit:
`refactor(cli)!: remove cc-uplink setup — plugin install supersedes it`
(`!` because a CLI subcommand disappears).
