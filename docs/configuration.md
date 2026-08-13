# Configuration

`~/.config/cc-uplink/config.toml` (all optional). `CC_UPLINK_CONFIG`
overrides the config path.

```toml
[drivers.tmux]
enabled = true

[drivers.image-openai]
enabled = true
api_key_env = "OPENAI_API_KEY"    # names the variable; the key itself is env-only
model = "gpt-image-1"

[drivers.image-codex]
enabled = true
codex_bin = "codex"
```

## tmux policy

Write access is deny-by-default and tiered: `observer` (read-only, the default
for every pane) ⊂ `operator` (send/ask/type/benign keys/label) ⊂ `godmode`
(dangerous chords like C-c). Grants come from two human-only carriers; the
pane option wins when present, config globs fill in otherwise:

```toml
[drivers.tmux]
write_allow = { "codex" = "operator", "lab-*" = "godmode" }  # label glob → tier
read_deny  = ["customer-*"]   # panes no tier may read (sticky deny)
```

Pane-level (dies with the pane): `tmux set -p @uplink_profile operator`,
`tmux set -p @uplink_read off` — or the `prefix+g` menu from
`cc-uplink tmux-snippet`. The driver never writes these options.

Policy fields hot-reload on file save (mtime-checked per decision); no MCP
restart needed to edit grants.

**Migration:** the old `allowlist` key is removed. `allowlist = ["codex"]`
becomes `write_allow = { "codex" = "operator" }`. Until you migrate, doctor
prints a warning and the old key does nothing.
