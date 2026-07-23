# cc-uplink

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
  `codex exec --full-auto` (ChatGPT login, no API key needed). Ops:
  `generate`, `edit`.

## Install

```bash
cargo build --release
./target/release/cc-uplink setup   # installs companion skill + `claude mcp add`
```

Or register manually:

```bash
claude mcp add -s user cc-uplink -- /path/to/cc-uplink serve
```

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
cc-uplink setup                        # register MCP server + install skill
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

Design spec: `docs/superpowers/specs/2026-07-22-cc-uplink-design.md`.
Driver wire contract: `docs/wire-contract.md`.
Downstream contracts (OpenAI API, Codex CLI): `docs/downstream-contracts.md`.

## Releasing

Versioning/changelog via [release-plz](https://release-plz.dev), artifacts via
[cargo-dist](https://opensource.axo.dev/cargo-dist/) (`dist-workspace.toml`):
static musl Linux (x86_64/aarch64) + macOS (x86_64/aarch64). Windows is
WSL-only, tier-2. Configs are inert until the repo has a public remote.

## License

[MIT](LICENSE)
