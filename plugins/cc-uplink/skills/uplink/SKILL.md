---
name: uplink
description: Use when messaging another agent or tmux pane (ask codex, talk to the peer in the other pane, cross-pane communication, read or press keys in another pane), typing into a raw console (nc, telnet, serial, a login prompt), or when generating or editing images (draw, illustrate, make a logo, edit a photo) — cc-uplink's channel_list/channel_describe/channel_observe/channel_act/channel_send/channel_recv/channel_doctor tools
---

# uplink — outbound channels

cc-uplink exposes exactly seven tools for reaching peers, raw consoles, and
image backends outside the session.
Channels are addressed `<driver>:<address>`: `tmux:%3`, `tmux:codex` (pane
label), `image:openai`, `image:codex`.

## Rules

1. **Discover first.** `channel_list()` shows every live channel. Pane labels
   (`tmux:codex`) beat raw pane ids — they survive pane reshuffles. Each
   entry's `profile` (effective tier) and `readable` tell you upfront what
   you're allowed to do there.
2. **Describe before first use.** `channel_describe(channel)` (op omitted)
   lists every op a channel supports, including which class it belongs to.
   Before the FIRST use of an op in a session, call
   `channel_describe(channel, op)` and follow the schema exactly. Do not
   guess args.
3. **Observe vs act.** Read-only ops (`read`, `await_idle`) go through
   `channel_observe`; everything that mutates (`type`, `keys`, `ask`,
   `label`, image `generate`/`edit`) goes through `channel_act`. Calling an
   op via the wrong tool is rejected with the correct tool named in the
   error — retry there, don't guess further.
4. **`type` vs `send`, know which channel you're on.** `type` is for raw
   consoles: literal text, fire-and-forget, no envelope, no verification —
   confirm what happened with a `read` afterwards. `send` is for agent peers:
   it wraps the message in a correlation envelope and mechanically verifies
   delivery. Using `send` on a raw console injects envelope garbage it can't
   parse; using `type` on an agent peer bypasses correlation. Pick the one
   that matches what's actually running in the pane.
5. **Never poll for replies.** After `channel_send`, the peer's reply arrives
   in YOUR pane as a `[reply id:…]` line — you will see it as input.
   `channel_recv` is an audit/recovery log, not a mailbox. Do not loop on it.
6. **Push vs pull for peer agents (e.g. Codex in another pane):**
   - Default: `channel_send(tmux:codex, …)` — async; the reply comes to you.
   - Need the peer's complete output now: `channel_act(tmux:codex, "ask",
     {message, quiet_ms?, timeout_ms?})` — mechanized round-trip that returns
     everything the peer printed since your question (may include TUI chrome).
   - Answering a `channel_send` costs the peer a shell command (`tmux
     send-keys`), so an agent peer that gates shell access — Claude Code, for
     one — stalls at a permission prompt until its operator allows it. `ask`
     has no such dependency: it reads the peer's pane directly.
7. **Writes need a human grant.** Every pane starts `observer` (read-only).
   `type`, `keys`, `ask`, and `label` all need at least `operator`; the
   dangerous key chords (`C-c`, `C-d`, …) need `godmode`. A human grants this
   per-pane with the `prefix+g` menu (from `cc-uplink tmux-snippet`) or a
   `write_allow` glob in config — never the agent itself. A `Rejected` error
   from a write op carries the exact remedy in its hint (which command or
   glob to ask the human for); read the hint and ask, don't retry blind.
8. **read/keys/type guard:** `channel_act(tmux:X, "keys" | "type", …)`
   requires a `channel_observe(tmux:X, "read", …)` of that pane within the
   last 60 s. Read, look, then act. A read-blocked pane (`readable: false`)
   is also untypeable — no blind writes into panes you may not see.
9. **Failures carry evidence; nothing auto-retries.** A failed send
   receipt/error includes a capture excerpt — read it and decide. An `ask`
   that returns `Timeout` means the pane never went quiet: either the peer is
   still working (raise `timeout_ms`) or it is blocked waiting for its own
   operator — a pending permission dialog animates, and animation reads as
   activity. Use `read` on the pane to see which before retrying.
10. **Images are act-only.** `channel_send` to `image:*` is rejected; so is
    `channel_observe` (image ops are all mutating). Use `channel_act`.
    (Arg lists below are orientation only — the schema from
    `channel_describe` is authoritative.)
    - `image:openai` — direct API (needs OPENAI_API_KEY): `generate
      {prompt, n?, size?, quality?, refs?, out_dir?}`, `edit {input, prompt,
      mask?}`. Returns absolute file paths.
    - `image:codex` — borrows Codex CLI's imagegen (needs `codex login`, no
      API key): `generate {prompt, refs?}`, `edit {input, prompt}`. Express
      size/count/output location in the prompt text.
11. **Something broken? `channel_doctor()` first.** It names the missing
    piece (tmux version, transport, API key, codex login, policy
    misconfiguration) before you debug.

## Examples

- Message a peer pane: `channel_send {channel: "tmux:codex", message: "review my diff in /tmp/x.diff please"}`
- Guaranteed full answer: `channel_act {channel: "tmux:codex", op: "ask", args: {message: "summarize your findings"}}`
- Type into a raw console: `channel_act {channel: "tmux:%7", op: "type", args: {text: "AT+CSQ", enter: true}}`
- Generate an image: `channel_act {channel: "image:openai", op: "generate", args: {prompt: "watercolor lighthouse, 1024x1024", size: "1024x1024"}}`
- Edit with codex: `channel_act {channel: "image:codex", op: "edit", args: {input: "./logo.png", prompt: "make the background transparent"}}`
