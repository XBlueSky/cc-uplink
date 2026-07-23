# Plugin directory restructure + open-source hygiene — design

Date: 2026-07-23
Status: approved

## Goal

1. Move the Claude Code plugin out of the repo root into `plugins/cc-uplink/`,
   following the conventional marketplace layout (marketplace manifest at
   root, each plugin in its own subdirectory).
2. Make the repo more welcoming to outside contributors: community docs,
   GitHub issue/PR templates, README badges.

Explicitly out of scope: publishing to crates.io.

## Part 1 — plugin moves to `plugins/cc-uplink/`

### Target layout

```
.claude-plugin/
  marketplace.json          # stays at root (marketplace spec requires it);
                            # plugins[0].source: "./" → "./plugins/cc-uplink"
plugins/cc-uplink/
  .claude-plugin/
    plugin.json             # moved from root .claude-plugin/
    server-version          # moved from root .claude-plugin/
  .mcp.json                 # moved from repo root
  scripts/launcher.sh       # moved from root scripts/
  skills/uplink/SKILL.md    # moved from root skills/
```

All moves use `git mv` to preserve history.

### Ripple effects

- `src/cli/mod.rs` — `include_str!("../../skills/uplink/SKILL.md")` becomes
  `include_str!("../../plugins/cc-uplink/skills/uplink/SKILL.md")`. Existing
  unit tests cover the embedded skill install; `cargo test` catches a wrong
  path at compile time.
- `README.md` — the Releasing section names repo paths
  `.claude-plugin/plugin.json` and `.claude-plugin/server-version`; both gain
  the `plugins/cc-uplink/` prefix. Install-section mentions of
  `scripts/launcher.sh` and `.claude-plugin/server-version` are
  plugin-root-relative and stay as-is.
- `scripts/launcher.sh` — no change. It resolves the plugin root as
  `dirname $0/..`; the `scripts/` ↔ `.claude-plugin/` relative relationship
  is unchanged inside the new directory.
- `.cc-marketspec/` — presentation overlay is path-independent (keyed by
  plugin name), but the generated `marketplace.json` must be re-validated
  with cc-check after the `source` change; regenerate with cc-generate if
  validation asks for it.
- `.github/workflows/` — no references to plugin paths (verified by grep);
  no CI changes needed.

### Verification

- `cargo test` (compile-time include_str check + skill-install tests)
- `cargo fmt --check && cargo clippy --release --all-targets`
- cc-marketspec cc-check passes
- plugin-validator agent review of `plugins/cc-uplink/`

## Part 2 — community files

All documents in English, matching the repo's existing convention.

- `CONTRIBUTING.md` — dev setup (rust-toolchain pinned), test/lint commands
  (`cargo test`, `cargo fmt --check`, `cargo clippy --release
  --all-targets`), PR expectations, pointer to the release process in
  README, note on the three version fields that move together.
- `CODE_OF_CONDUCT.md` — Contributor Covenant v2.1, enforcement contact via
  GitHub (no personal email published).
- `SECURITY.md` — report vulnerabilities through GitHub Private
  Vulnerability Reporting (Security tab → Report a vulnerability); no public
  issue for exploitable bugs.
- `.github/ISSUE_TEMPLATE/bug_report.yml` — YAML form: what happened,
  expected, repro, platform (OS/arch), install method (plugin vs manual),
  `cc-uplink doctor` output.
- `.github/ISSUE_TEMPLATE/feature_request.yml` — YAML form: problem,
  proposed solution, alternatives.
- `.github/ISSUE_TEMPLATE/config.yml` — `blank_issues_enabled: false`, one
  contact link pointing to the repo's Security advisories page for
  vulnerability reports.
- `.github/PULL_REQUEST_TEMPLATE.md` — summary, how verified (test/lint
  commands), checklist (fmt/clippy/tests, docs updated if user-facing).
- `README.md` — badges at top (CI workflow status, latest GitHub release,
  MIT license); a short Contributing section linking CONTRIBUTING.md and
  SECURITY.md.

## Commit strategy

Two commits, in this order:

1. `refactor(plugin): move plugin into plugins/cc-uplink/`
2. `docs: add community files + README badges`

Separation keeps the mechanical move reviewable and independently
revertable from the new prose.
