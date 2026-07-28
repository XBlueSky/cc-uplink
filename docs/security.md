# Security posture

- argv vectors only — prompts/messages/paths are never shell text
- refuses to send to its own pane (loop prevention); optional allowlists
- secrets are env-only; config names variables, never values
- injected envelopes are visible plaintext in panes **by design** — human
  observability is a feature; don't put secrets in messages
- send verification never auto-retries; failures return capture evidence

Report security issues privately per [SECURITY.md](https://github.com/XBlueSky/cc-uplink/blob/main/SECURITY.md).
