# Wire Contract (Driver trait)

One page mirroring `src/core/driver.rs`. Any change to the trait or its DTOs
is a protocol change: update this page and its changelog in the same commit.
These shapes are the future v3 out-of-process driver protocol — keep every
type serde-serializable, no lifetimes/callbacks/trait objects in payloads,
zero shared mutable state between core and drivers.

## Trait

| Method | In | Out |
|---|---|---|
| `info()` | — | `DriverInfo {id, kind: messaging\|capability\|both, summary}` |
| `channels()` | — | `Vec<ChannelEntry {channel, labels[], detail}>` |
| `ops()` | — | `Vec<OpSpec {op, summary, params_schema, result_schema}>` |
| `send(addr, SendRequest {message, reply_hint: full\|short\|none})` | | `SendReceipt {delivered, correlation_id, verify_excerpt?, injected_at}` |
| `invoke(addr, op, args: Value)` | | `Value` |
| `recv(cursor: Option<u64>)` | | `RecvBatch {items: [{cursor, at, from?, id?, raw}], next_cursor}` |
| `doctor()` | — | `DoctorReport {driver, ok, lines[]}` |

`ops()` now returns `OpSpec {op, summary, mutating: bool, params_schema, result_schema}`
— `mutating` is self-declared per driver (`false` = returns state without
changing the world; `true` = injects, spends, or renames).

Errors: `DriverError {kind: NotFound|Unavailable|Rejected|Timeout|Upstream|Invalid, message, hint?, evidence?}`, rendered as
`uplink error [<driver>:<Kind>]: <message> — hint: <hint>`. The ` — hint: <hint>`
suffix is omitted entirely when `hint` is absent.

## MCP tool surface vs. the trait

The trait keeps a single `invoke()` — the split lives one layer up, in
`src/mcp.rs`. The MCP server no longer exposes a `channel_invoke` tool;
instead it exposes `channel_observe` (routes to `invoke()` for ops with
`mutating: false`) and `channel_act` (routes to `invoke()` for ops with
`mutating: true`), so Claude Code's tool-name-granular permission layer can
auto-allow "look" while gating "act". Calling an op through the wrong class
— e.g. `channel_observe` with a mutating op, or `channel_act` with a
read-only one — returns `DriverError {kind: Invalid, ...}` naming the correct
tool to use instead. The human CLI's `invoke` subcommand (`docs/cli.md`)
bypasses this split entirely — it calls the trait's `invoke()` directly and
is class-agnostic.

## Semantics notes

- `SendReceipt` carries evidence, not sentiment: `delivered` is only true
  after mechanized verification; failures carry capture evidence instead.
- `recv` cursors are per-registry-merge in v1 (single shared cursor space);
  MUST become per-driver before a second recv-bearing driver ships
  (spec §19).
- Composite drivers are legal: the `image` driver multiplexes backends on
  the address part; the Registry only routes on the prefix.

## Changelog

- 2026-07-22: initial contract as implemented by M1/M2 (tmux) — note:
  transport reconnect is on-demand inside the tmux driver, not a supervised
  background loop (spec §19).
- 2026-07-22: `image` composite driver added (M3/M4). No trait changes.
- 2026-08-12: `OpSpec` gains `mutating: bool`. Trait `invoke()` unchanged; the
  MCP surface above it drops `channel_invoke` for `channel_observe`/
  `channel_act`, routed by `mutating` (terminal-ops permission model).
