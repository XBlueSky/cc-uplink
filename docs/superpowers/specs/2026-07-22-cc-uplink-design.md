# cc-uplink — Design Spec

- **Date**: 2026-07-22
- **Status**: Draft for review
- **Scope**: v1 (with named v1.5/v2/v3 roadmap items)

## 1. Problem & Positioning

Claude Code (CC) has no unified way to interact with the world outside its own
session: talking to a peer agent in another tmux pane, invoking an external
capability such as image generation, or (later) durable agent-to-agent mail.
The ecosystem answers exist but are fragmented — one MCP server per concern
(tmux-bridge-mcp), one CLI+skill per concern (agent-message-queue), one plugin
per concern (codex-image-in-cc) — each with its own tool surface, config,
error style, and diagnostics. Every added tool surface also erodes CC's
skill/tool listing budget.

**cc-uplink is CC's single outbound channel layer**: one Rust stdio MCP server
exposing a **fixed set of six tools**, with pluggable *drivers* underneath.
Adding a driver never adds a tool.

Prior-art lessons this design deliberately inherits:

| Source | Lesson carried over |
|---|---|
| tmux-bridge-mcp | mechanism over prompt discipline (read-guard); `@name` pane labels; loop prevention; envelope headers with correlation ids |
| agent-message-queue | delivery must produce *evidence* (receipts), not "OK"; doctor-style operational diagnostics |
| codex-image-in-cc | thin wrapper discipline; document load-bearing downstream contracts; never re-parse what a downstream LLM can interpret; subprocess hygiene (stdin ignore, no shell interpolation) |

### Topology: single-sided

cc-uplink runs **only on the CC side**. Peers (Codex CLI, other agents, plain
shells) install nothing. Peer replies rely on the fact that peers live inside
tmux and have bash: the message envelope carries reply instructions using raw
`tmux send-keys`. This is a hard constraint, not an oversight: it keeps the
project a client-side driver layer instead of a two-sided protocol with a
compatibility matrix.

## 2. Goals / Non-Goals

**Goals (v1)**
- One MCP server, six fixed tools, stable regardless of driver count.
- Drivers: `tmux` (messaging + pane ops), `image-openai` (direct API),
  `image-codex` (subprocess borrowing Codex's imagegen).
- tmux driver built **control-mode-first** (`tmux -C`), event-driven.
- Wire-shaped driver trait so a future out-of-process driver protocol is a
  mechanical extraction, not a rewrite.
- Human observability: conversations happen in visible panes; structured
  send/recv log via CLI.
- Open-source-ready from day 1 (Apache-2.0 or MIT, English docs, cargo-dist).

**Non-Goals (v1)**
- No two-sided protocol; no software installed on peers.
- No durable mailbox (v2 driver; `recv` semantics already accommodate it).
- No out-of-process driver plugin protocol (v3; see §4 wire discipline).
- No task orchestration, worktree management, or scheduling (stay one layer
  below orchestrators, as AMQ does).
- Not managed by cc-loadout internals; optional plugin packaging makes it
  *gateable by* cc-loadout (see §16).

## 3. Architecture Overview

```
Claude Code
    │  stdio MCP — six fixed tools
    ▼
┌────────────────────── cc-uplink serve ──────────────────────┐
│ MCP layer (rmcp): tool registration, schema serving          │
│ Core: Router (address parsing) · Registry · Policy           │
│ ───────────────── wire-shaped Driver trait ────────────────── │
│  tmux driver          │ image-openai      │ image-codex      │
│  (ControlModeHub,     │ (reqwest+rustls,  │ (codex exec      │
│   tmux ≥3.2, CLI      │  Images API)      │  subprocess)     │
│   fallback transport) │                   │                  │
└──────────────────────────────────────────────────────────────┘
```

**Channel addressing**: `<driver>:<address>` — `tmux:%3`, `tmux:codex`
(label, resolved via pane `@name`), `image:openai`, `image:codex`.

### Tool surface (six, fixed)

| Tool | Purpose |
|---|---|
| `channel_list()` | Enumerate channels + capability summaries across drivers |
| `channel_describe(channel, op?)` | On-demand JSON Schema for an op (schemas never occupy resident context) |
| `channel_send(channel, message, opts?)` | Async message. v1: tmux injection with mechanized verify cycle |
| `channel_invoke(channel, op, args)` | Capability call. v1: `image:*` generate/edit; `tmux:*` read/keys/label/await_idle/ask |
| `channel_recv(cursor?)` | Drain inbound envelope log since cursor (non-blocking) |
| `channel_doctor()` | Aggregated per-driver diagnostics |

Rationale: fixed tool count directly addresses the documented failure mode of
per-capability tool sprawl (skill/tool listing budget dilution). Schema
guidance lost by generic `invoke` args is recovered via `channel_describe` +
the companion skill; high-traffic ops may be promoted to dedicated typed tools
later if real usage shows elevated arg-error rates (escape hatch toward a
typed façade, without changing this architecture).

## 4. Core & the Wire-Shaped Driver Trait

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> DriverInfo;                     // id, kind, summary
    async fn channels(&self) -> Result<Vec<ChannelEntry>, DriverError>;
    fn ops(&self) -> Vec<OpSpec>;                     // op + params/result JSON Schema
    async fn send(&self, addr: &str, msg: SendRequest) -> Result<SendReceipt, DriverError>;
    async fn invoke(&self, addr: &str, op: &str, args: serde_json::Value)
        -> Result<serde_json::Value, DriverError>;
    async fn recv(&self, cursor: Option<Cursor>) -> Result<RecvBatch, DriverError>;
    async fn doctor(&self) -> DoctorReport;
}
```

**Wire disciplines (binding):**
1. Every trait input/output is a serde-serializable type. No lifetimes, no
   callbacks, no trait objects in payloads. Events flow through explicit
   channels owned by core.
2. Zero shared mutable state between core and drivers; interaction is
   request/response plus an event stream.
3. `docs/wire-contract.md` (one page) mirrors the trait. Any trait change is
   treated as a protocol change and logged in its changelog section.

These three rules make the future v3 out-of-process driver protocol a
serialization of the existing trait plus a `SubprocessDriver` adapter — in-tree
drivers unchanged, external drivers additive.

`SendReceipt` carries evidence, not sentiment: `{ delivered: bool,
correlation_id, verify_excerpt?, injected_at }`.

## 5. tmux Driver

### 5.1 ControlModeHub

Primary transport is **tmux control mode** (`tmux -C attach`), one long-lived
client subprocess per attached session:

- **Command path**: commands written as lines; replies framed by
  `%begin`/`%end`/`%error` matched FIFO.
- **Event path**: `%output %pane-id data` (octal-escaped; hub unescapes),
  `%session-changed`, `%exit`, `%pause`/`%continue` (hub answers `continue`
  and reads eagerly).
- **Reconnect**: tmux server restart / `%exit` → backoff reconnect; state
  visible in doctor.
- **Multi-session**: hub manages N control clients (one per session that
  hosts a subscribed peer). v1 opens one (own session); additional
  connections are additive.

**Transport seam**: `trait TmuxTransport { run(cmd) -> Output; events() ->
Stream }`. `ControlMode` is the primary implementation; `OneShotCli` is the
fallback (ancient tmux, or control attach failure) and the unit-test seam.

**Version gate**: full feature set requires tmux ≥ 3.2 (subscriptions,
pause/continue). Doctor reports version and active transport. Reference
environment: tmux 3.5a.

### 5.2 Send cycle (mechanized, single tool call)

```
resolve(label → pane id)
→ policy: refuse own pane ($TMUX_PANE); optional allowlist
→ inject: send-keys -l -- "<envelope> <message>"   (never through a shell)
→ verify: same-session → await own %output echo (race-free)
          cross-session → capture-pane compare (same semantics, snapshot-based)
→ send-keys Enter
→ SendReceipt { delivered, correlation_id, verify_excerpt }
```

Verify failure does **not** auto-retry (no destructive line clearing, no
double-send risk); the error carries capture evidence and CC decides.

### 5.3 Envelope v2

```
[uplink from:<label|pane> pane:%A id:<8hex>] <message>
(reply: run `tmux send-keys -t %A -l '[reply id:<8hex>] <your answer>' \; send-keys -t %A Enter`)
```

The reply block teaches an uninstrumented peer how to answer using only bash +
tmux. Verbosity is configurable per send (`opts.reply_hint = full | short |
none`) — peers already carrying a companion skill don't need the boilerplate.

### 5.4 Ops

| Op | Params | Behavior |
|---|---|---|
| `read` | `{lines}` | capture-pane snapshot (`-J`, ANSI stripped) |
| `keys` | `{keys[]}` | raw special keys (Enter/Escape/C-c). Guarded: requires a `read` of that pane within the last 60 s (in-process, per-session guard — an upgrade over tmpdir global guard files) |
| `label` | `{name}` | set pane `@name` |
| `await_idle` | `{quiet_ms, timeout_ms}` | event-driven wait until pane output is quiet (same-session: `%output` silence; cross-session: `refresh-client -B` format-change approximation) |
| `ask` | `{message, quiet_ms, timeout_ms}` | composed round-trip: record target pane history watermark → send (full cycle) → `await_idle` → capture delta from watermark → return rendered transcript slice ("everything the peer printed since the question") |

`ask` is the pull-side guarantee of response completeness; the push side
(envelope reply convention) is the primary conversational path. Both exist by
design: push depends on peer cooperation, pull is mechanical but includes TUI
chrome noise.

### 5.5 recv

Hub watches own pane `%output` for inbound `[uplink …]` / `[reply …]`
envelopes and appends them to a cursor-indexed in-memory log. `channel_recv`
drains non-blockingly since the given cursor. This is an audit/recovery
surface — inbound messages also land directly in CC's input box (that remains
the primary delivery path; **never poll for replies** stays the rule taught by
the companion skill).

Startup defaults applied best-effort (as tmux-bridge does): `history-limit
100000`, mouse on — non-fatal if they fail.

## 6. image-openai Driver

- Channel: `image:openai`. Ops: `generate {prompt, n?, size?, quality?,
  refs?[], out_dir?}`, `edit {input, prompt, mask?}`.
- Direct HTTPS to OpenAI Images API (`gpt-image-1`), reqwest + **rustls**
  (no OpenSSL anywhere in the dependency tree).
- API key read from env only (`api_key_env` names the variable; the key is
  never stored in config).
- Output: PNG files written under `out_dir` (default
  `./uplink-images/<UTC-ts>-<n>.png`); result returns absolute paths.
- Doctor: key presence, endpoint reachability (HEAD, short timeout).

## 7. image-codex Driver

- Channel: `image:codex`. Ops: `generate {prompt, refs?[]}`, `edit {input,
  prompt}`.
- Spawns `codex exec --full-auto --skip-git-repo-check` with:
  - stdin ignored (documented hang otherwise),
  - `--image <abs>` per reference **and** absolute paths listed in the
    instruction text (required by the 0.144+ `referenced_image_paths` tool
    path; keeps 0.142–0.143 working),
  - instruction requiring one `SAVED: <absolute path>` line per image;
    stdout parsed for those lines only.
- Doctor: codex present, semver ≥ 0.142, login status, `exec --full-auto`
  accepted, `--image` supported.
- These downstream contracts are load-bearing and version-drifting; they are
  mirrored in `docs/downstream-contracts.md` and must be updated in the same
  PR as any behavior change (discipline inherited from codex-image-in-cc).
- This driver is also the living rehearsal of the subprocess boundary that
  the v3 external-driver protocol will formalize.

## 8. Error Model

```
DriverError { kind: NotFound | Unavailable | Rejected | Timeout | Upstream | Invalid,
              message, hint?, evidence? }
```

Rendered to MCP as: `uplink error [tmux:NotFound]: no pane labeled 'codex' —
hint: run channel_list()`. Policy denials are `Rejected` with explicit hints.
All drivers normalize into this envelope; no driver-specific error shapes leak
to the tool surface.

## 9. CLI Surface (same binary, same core)

| Command | Purpose |
|---|---|
| `cc-uplink serve` | stdio MCP server |
| `cc-uplink doctor` | human diagnostics (CI-friendly exit codes) |
| `cc-uplink send / invoke` | human-driven parity with MCP tools — driver debugging without an LLM |
| `cc-uplink log [--follow]` | structured conversation timeline (send receipts + recv envelopes, correlation-id threaded) |
| `cc-uplink setup` | `claude mcp add -s user cc-uplink -- cc-uplink serve` + install companion skill |

## 10. Companion Skill

`skills/uplink/SKILL.md`, installed by `setup`. Teaches CC: address forms,
describe-before-first-invoke, push-vs-`ask` choice for peer conversations, and
the no-polling rule. The skill is documentation, not enforcement — enforcement
(guards, policy, verify) lives in the binary.

## 11. Config

`~/.config/cc-uplink/config.toml`:

```toml
[drivers.tmux]
enabled = true
# allowlist = ["%1", "codex"]        # optional send-target allowlist

[drivers.image-openai]
enabled = true
api_key_env = "OPENAI_API_KEY"
model = "gpt-image-1"

[drivers.image-codex]
enabled = true
codex_bin = "codex"
```

## 12. Security / Policy

- Never route argv through a shell (`Command` arg vectors only; prompts and
  messages are data, not shell text).
- Refuse interacting with own pane (loop prevention).
- Optional per-driver target allowlists.
- Secrets: env-only; config names variables, never values.
- Injected envelopes are visible plaintext in panes by design (human
  observability is a feature); no secret material belongs in messages.

## 13. Testing Strategy

| Layer | Approach |
|---|---|
| Pure units | envelope format, address parsing, config, control-mode line parser (golden `%begin/%end/%output` streams, octal unescape) |
| Core | Router/Policy against a mock `Driver`; recv cursor semantics |
| tmux integration (Linux CI) | real tmux on a private `-S` socket: control attach, inject→verify→Enter→capture round-trip, `await_idle`, `ask` watermark slicing |
| image-openai | wiremock HTTP server, golden requests; CI never calls the real API |
| image-codex | fake `codex` script on PATH emitting `SAVED:` lines; asserts stdin-ignore + argv contract |
| doctor | degraded matrices (no tmux / old tmux / no key / old codex) |
| Quality gates | `cargo fmt --check`, `cargo clippy --release --all-targets` in CI |

## 14. Distribution

- Rust, single binary. rmcp (official SDK), tokio, reqwest(rustls), serde,
  notify (future mailbox driver).
- Targets: `x86_64/aarch64-unknown-linux-musl` (static),
  `x86_64/aarch64-apple-darwin`. Windows: WSL-only, tier-2, documented.
- Release automation: release-plz + cargo-dist. License: MIT (consistent with
  the author's existing tooling, e.g. cc-loadout).
- Internal first (git.synology.inc), GitHub when opened.
- **Plugin packaging (post-v1)**: wrap binary + skill as a Claude Code plugin
  so cc-loadout profiles can gate where uplink loads — integration by
  packaging, zero cc-loadout changes.

## 15. Milestones

| M | Deliverable |
|---|---|
| M1 | Repo scaffolding; ControlModeHub (attach, framing, `%output`, reconnect); `channel_list/describe/doctor`; tmux `read` |
| M2 | Send cycle + envelope v2; `keys/label/await_idle/ask`; `recv`; `cc-uplink log` |
| M3 | image-openai driver (+ wiremock suite) |
| M4 | image-codex driver (+ fake-codex suite); `docs/downstream-contracts.md` |
| M5 | Companion skill; `setup`; cargo-dist/release-plz; README |

## 16. Roadmap (post-v1)

- **v1.5**: `codex:exec` driver (`invoke("codex:exec","ask",{prompt})` — clean
  full-text one-shot Q&A, no shared pane context; reuses image-codex subprocess
  machinery). Multi-session ControlModeHub connections on demand.
- **v2**: durable mailbox driver (Maildir semantics: tmp→fsync→rename→new;
  receipts; `recv` gains a second, durable source). Other model-API drivers
  (Gemini, local LLM) as `invoke` capabilities.
- **v3**: extract the wire contract into an out-of-process driver protocol
  (JSON over stdio) + `SubprocessDriver` adapter, opening drivers to any
  language. Gated on real external-driver demand; the three v1 drivers are its
  usage evidence.

## 17. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Keyboard injection into TUIs is inherently best-effort (peer TUI state can mangle input) | mechanized verify + evidence-bearing receipts; no silent failure; `ask` as pull-side fallback |
| Peer doesn't follow reply convention | `ask` op guarantees transcript capture; companion skill documents both paths |
| `%output` not available cross-session | capture-based verify + `refresh-client -B` approximation; multi-client hub is additive |
| rmcp SDK maturity lags TS SDK | v1 needs stdio+tools only; surface is minimal by design |
| Codex CLI contract drift (image-codex) | doctor gates + `downstream-contracts.md` same-PR discipline |
| Generic `invoke` args mis-called by model | `channel_describe` schemas + skill; promotion path to typed tools if error rates prove it |

## 18. Alternatives Considered

- **Extend cc-loadout**: rejected — loadout manager must stay deletable;
  capabilities are not loadout concerns.
- **Modular monolith with per-driver typed tools**: rejected for tool-budget
  erosion; kept as a promotion escape hatch per-op.
- **Protocol-first microkernel (external drivers day 1)**: deferred to v3 —
  no usage evidence yet to design the wire against; wire-shaped trait keeps
  the extraction mechanical.
- **TypeScript**: rejected on maintenance-language fit, single-binary
  distribution, and pure-Rust dependency tree; TS's SDK maturity advantage is
  immaterial for a stdio+tools server.
- **One-shot CLI tmux transport as primary**: superseded by control-mode-first
  decision (event-driven verify/await/recv); retained as fallback transport
  and test seam.

## 19. Known v1 limitations & tracked follow-ups

Surfaced during implementation review and deliberately deferred. None blocks
v1 merge; each is on a secondary/audit/cosmetic path.

- **`channel_recv` uses one shared cursor space across drivers.** The MCP layer
  merges every driver's `recv` into a single `next_cursor = max(...)`. This is
  correct with one driver (tmux) but **must become per-driver cursors before the
  v2 mailbox driver ships** — otherwise a second driver with an independent
  cursor would skip items. Prerequisite for §16 v2.
- **Transport reconnect is on-demand, not a supervised backoff loop.** Task 8's
  plan described a background reconnect supervisor (500ms→8s backoff); the
  implementation instead re-attaches lazily inside `TmuxDriver::run()` (no
  backoff task). Reasonable simplification; recorded here (and belongs in the
  wire-contract changelog) because it is the mechanism the recv watcher
  supervisor now compensates for by re-subscribing across a CM swap.
- **`%end`/`%error` control-mode terminators are matched by prefix, not by the
  block's sequence number.** A reply-body line reading exactly `%end <ts> <n>
  <flags>` could close a block early. The seq is already parsed but unused;
  matching it against the open `%begin` closes the gap. Low probability.
- **`op_await_idle` deadline can overshoot** the requested timeout by up to one
  `quiet_ms` (event path) / 300ms (poll path); bounded, never a hang.
- **`TmuxDriver::run()` holds the `cm` mutex across the tmux round-trip**,
  serializing driver commands instance-wide and leaving ControlMode's FIFO
  multi-waiter machinery single-depth in v1. Latency-only; clone the
  `Arc<ControlMode>` out and drop the guard before the await to restore
  concurrency.
- **CLI renders `driver_for` errors under a fixed `"core"` id** while the MCP
  layer uses the channel prefix (`driver_of`). Both are safe (neither puts
  malformed input in the id slot); the inconsistency is cosmetic. Unify by
  having the CLI reuse `driver_of`.
- **`recv` watcher keeps one `LineBuffer` across re-subscribes**, so a partial
  line straddling a CM re-attach may garble/drop exactly one inbound envelope at
  the seam (then recovers). Reset the buffer on each fresh subscription to tidy.
- **Minor operability/edges**: `channel_recv` swallows per-driver `recv` errors
  silently; `log --follow` re-reads the whole file each poll (O(file-size));
  `read_marks` grows one entry per distinct pane (inbox is bounded at 1000);
  `LineBuffer` strips ANSI per `%output` chunk so an escape split across two
  chunks can leak a fragment into a logged envelope; concurrent `LogSink`
  writers can interleave in the JSONL log.
- **`channel_describe` over-returns sibling-backend ops for composite drivers.**
  For an `image:<backend>` channel, `channel_describe` returns ALL image
  backends' ops (both `openai` and `codex` `generate`/`edit` schemas), because
  the frozen `Driver::ops()` takes no address. The `[openai]`/`[codex]` summary
  prefixes disambiguate, and the `invoke` path routes correctly by address, so
  this is non-fatal. A proper fix (address-aware `ops(addr)`) is a
  `Driver`-trait change deferred with the §16 v3 out-of-process protocol; until
  then it is a documented limitation.
- **`image:openai` output filenames are second-precision.** `image_filename`
  names files `<UTC-second-ts>-<n>.png`; two `generate` calls completing in
  the same wall-clock second overwrite each other's `-<n>.png`. Rare given API
  latency; a sub-second/random suffix would remove the footgun (weigh against
  spec §6's `<UTC-ts>-<n>.png` shape).
