---
name: uplink
description: Use when messaging another agent or tmux pane (ask codex, talk to the peer in the other pane, cross-pane communication, read or press keys in another pane) or when generating or editing images (draw, illustrate, make a logo, edit a photo) — cc-uplink's channel_list/channel_describe/channel_send/channel_invoke/channel_recv/channel_doctor tools
---

# uplink — outbound channels

cc-uplink exposes exactly six tools for reaching peers and image backends
outside the session.
Channels are addressed `<driver>:<address>`: `tmux:%3`, `tmux:codex` (pane
label), `image:openai`, `image:codex`.

## Rules

1. **Discover first.** `channel_list()` shows every live channel. Pane labels
   (`tmux:codex`) beat raw pane ids — they survive pane reshuffles.
2. **Describe before first invoke.** `channel_describe(channel)` (op
   omitted) lists every op a channel supports. Before the FIRST
   `channel_invoke` of any op in a session, call
   `channel_describe(channel, op)` and follow the schema exactly.
   Do not guess args.
3. **Never poll for replies.** After `channel_send`, the peer's reply arrives
   in YOUR pane as a `[reply id:…]` line — you will see it as input.
   `channel_recv` is an audit/recovery log, not a mailbox. Do not loop on it.
4. **Push vs pull for peer agents (e.g. Codex in another pane):**
   - Default: `channel_send(tmux:codex, …)` — async; the reply comes to you.
   - Need the peer's complete output now: `channel_invoke(tmux:codex, "ask",
     {message, quiet_ms?, timeout_ms?})` — mechanized round-trip that returns
     everything the peer printed since your question (may include TUI chrome).
5. **keys guard:** `channel_invoke(tmux:X, "keys", …)` requires a
   `channel_invoke(tmux:X, "read", …)` of that pane within the last 60 s.
   Read, look, then press.
6. **Send failures carry evidence.** A failed send receipt/error includes a
   capture excerpt. Read it and decide; cc-uplink never auto-retries.
7. **Images are invoke-only.** `channel_send` to `image:*` is rejected.
   (Arg lists below are orientation only — the schema from
   `channel_describe` is authoritative.)
   - `image:openai` — direct API (needs OPENAI_API_KEY): `generate
     {prompt, n?, size?, quality?, refs?, out_dir?}`, `edit {input, prompt,
     mask?}`. Returns absolute file paths.
   - `image:codex` — borrows Codex CLI's imagegen (needs `codex login`, no
     API key): `generate {prompt, refs?}`, `edit {input, prompt}`. Express
     size/count/output location in the prompt text.
8. **Something broken? `channel_doctor()` first.** It names the missing
   piece (tmux version, transport, API key, codex login) before you debug.

## Examples

- Message a peer pane: `channel_send {channel: "tmux:codex", message: "review my diff in /tmp/x.diff please"}`
- Guaranteed full answer: `channel_invoke {channel: "tmux:codex", op: "ask", args: {message: "summarize your findings"}}`
- Generate an image: `channel_invoke {channel: "image:openai", op: "generate", args: {prompt: "watercolor lighthouse, 1024x1024", size: "1024x1024"}}`
- Edit with codex: `channel_invoke {channel: "image:codex", op: "edit", args: {input: "./logo.png", prompt: "make the background transparent"}}`
