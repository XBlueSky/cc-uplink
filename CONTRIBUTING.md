# Contributing to cc-uplink

Thanks for your interest in improving cc-uplink! This document covers the
practical bits: getting a dev environment running, verifying your changes,
and what we look for in pull requests.

## Dev setup

The Rust toolchain is pinned by `rust-toolchain.toml` — with
[rustup](https://rustup.rs) installed, the right version is picked up
automatically the first time you build.

```bash
cargo build
```

Integration tests spin up private-socket tmux servers, so you'll want
**tmux ≥ 3.2** installed (3.5a is the reference environment). No API keys
are needed for the test suite.

## Verifying changes

Run the same gate CI runs before opening a PR:

```bash
cargo test
cargo fmt --check && cargo clippy --release --all-targets
```

## Repo layout

- `src/` — the Rust binary: MCP server (`serve`), human CLI, drivers.
- `plugins/cc-uplink/` — the Claude Code plugin: skill, `.mcp.json`,
  launcher, version pin. The repo root doubles as a plugin marketplace
  (`.claude-plugin/marketplace.json`).
- `docs/` — wire contract, downstream contracts, design specs.

To develop against the plugin without touching the version pin, point the
launcher at your local build:

```bash
export CC_UPLINK_BIN="$PWD/target/release/cc-uplink"
```

## Commit messages

We follow conventional-commit style, matching the existing history:
`feat:`, `fix:`, `docs:`, `ci:`, `chore:`, with optional scopes like
`feat(plugin):`.

## Pull requests

- Behavior changes come with tests; user-facing changes come with doc
  updates (README or `docs/`).
- Keep PRs focused — mechanical refactors and behavior changes are easier
  to review as separate PRs.
- Fill in the PR template's verification checklist honestly; "not run"
  with a reason beats a silently skipped gate.

## Releases

Releases are cut by the maintainer. Three version fields move together in
one release commit (see the README's **Releasing** section):
`Cargo.toml` `version`, `plugins/cc-uplink/.claude-plugin/plugin.json`
`version`, and `plugins/cc-uplink/.claude-plugin/server-version`.

## Security issues

Please **do not** open public issues for exploitable bugs — see
[SECURITY.md](SECURITY.md) for the private reporting channel.
