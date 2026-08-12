# Security posture

- argv vectors only — prompts/messages/paths are never shell text
- refuses to send to its own pane (loop prevention)
- secrets are env-only; config names variables, never values
- injected envelopes are visible plaintext in panes **by design** — human
  observability is a feature; don't put secrets in messages
- send verification never auto-retries; failures return capture evidence
- write ops are deny-by-default behind human-set tiers (`@uplink_profile`
  pane option / `write_allow` globs); the driver only ever reads the grant
  markers — an agent cannot grant itself
- renaming a pane can never raise its config-granted tier (rename-as-escalation
  is rejected)
- `type` with `sensitive: true` redacts the text from the JSONL log (metadata
  only); the agent itself necessarily saw the text — this protects the disk,
  not the model
- **scope, honestly:** these gates stop accidents and injection-shaped
  mistakes, not a malicious agent — unrestricted Bash can drive `tmux`
  directly. To make cc-uplink the only tmux path, deny raw tmux in the
  harness, e.g. in Claude Code `settings.json`:
  `"permissions": { "deny": ["Bash(tmux:*)"] }` — then every pane operation
  flows through the policy gate and audit log. The `uplink.tmux` border badge
  makes an out-of-band grant visually conspicuous.

Report security issues privately per [SECURITY.md](https://github.com/XBlueSky/cc-uplink/blob/main/SECURITY.md).
