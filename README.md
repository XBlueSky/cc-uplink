# cc-uplink

[![CI](https://github.com/XBlueSky/cc-uplink/actions/workflows/ci.yml/badge.svg)](https://github.com/XBlueSky/cc-uplink/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/XBlueSky/cc-uplink)](https://github.com/XBlueSky/cc-uplink/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Claude Code's unified outbound channel layer: one Rust binary, one stdio MCP
server, **six fixed tools** — with pluggable drivers underneath. Adding a
driver never adds a tool, so your tool/skill listing budget stays flat no
matter how many ways Claude can reach the outside world.

| Tool | Purpose |
|---|---|
| `channel_list()` | Enumerate channels across drivers |
| `channel_describe(channel, op?)` | On-demand JSON Schema for an op |
| `channel_send(channel, message, opts?)` | Async message (tmux: inject → verify → Enter, evidence-bearing receipt); `image:*` channels are invoke-only, so `channel_send` is rejected there — use `channel_invoke` with `generate`/`edit` |
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
  `codex exec --sandbox workspace-write` (ChatGPT login, no API key needed).
  Ops: `generate`, `edit`.

## Install

### Claude Code plugin (recommended)

```
/plugin marketplace add XBlueSky/cc-uplink
/plugin install cc-uplink@cc-uplink
```

The plugin ships the `uplink` skill and registers the MCP server. On first
connection `scripts/launcher.sh` downloads the release binary pinned in
`.claude-plugin/server-version` for your platform (static musl Linux or
macOS, x86_64/aarch64), verifies its sha256, and caches it in the plugin
data dir — the skill and the binary it describes can never version-skew.
Set `CC_UPLINK_BIN=/path/to/local/build` to bypass pinning during
development.

### Manual (any other MCP client, or no plugins)

Prebuilt binaries are on the
[Releases page](https://github.com/XBlueSky/cc-uplink/releases) — Linux
builds are fully static musl (x86_64/aarch64, any distro with kernel ≥ 3.2),
plus macOS (x86_64/aarch64):

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

Driver wire contract: `docs/wire-contract.md`.
Downstream contracts (OpenAI API, Codex CLI): `docs/downstream-contracts.md`.

## Releasing

Three version fields move together in one release commit:

1. `Cargo.toml` `version`
2. `plugins/cc-uplink/.claude-plugin/plugin.json` `version` (plugin cache/display axis)
3. `plugins/cc-uplink/.claude-plugin/server-version` (the binary the plugin launcher pins)

```bash
git commit -am "chore: release vX.Y.Z"
git tag vX.Y.Z && git push && git push --tags
```

Push a `v*` tag → `.github/workflows/release.yml` builds static musl Linux
(x86_64/aarch64) + macOS (x86_64/aarch64) tarballs with sha256 checksums and
attaches them to the GitHub Release. Windows is WSL-only, tier-2.
Versioning/changelog automation via [release-plz](https://release-plz.dev)
is configured (`release-plz.toml`) but not yet wired into CI.

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for dev
setup, test commands, and PR expectations. Report security issues
privately per [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
