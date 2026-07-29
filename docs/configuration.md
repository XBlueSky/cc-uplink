# Configuration

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
