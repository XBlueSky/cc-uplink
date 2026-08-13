# CLI

Same binary, same drivers, no LLM required:

```bash
cc-uplink doctor                       # diagnostics, CI-friendly exit code
cc-uplink send tmux:codex "hello"      # full mechanized send cycle
cc-uplink invoke image:openai generate '{"prompt":"a lighthouse"}'
cc-uplink log --follow                 # correlation-id-threaded conversation log
cc-uplink tmux-snippet                 # print the uplink.tmux grant-menu snippet
```

`invoke` is class-agnostic: unlike the MCP surface's `channel_observe`/
`channel_act` split, this is a human at a terminal, so there's no permission
layer to grant per-tool — `invoke` calls any op, read-only or mutating,
directly.

`tmux-snippet` prints the `uplink.tmux` snippet (grant menu on `prefix+g`,
border badge showing each pane's profile) to stdout for you to source from
`~/.tmux.conf` yourself — cc-uplink never edits your tmux config.
