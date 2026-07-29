# CLI

Same binary, same drivers, no LLM required:

```bash
cc-uplink doctor                       # diagnostics, CI-friendly exit code
cc-uplink send tmux:codex "hello"      # full mechanized send cycle
cc-uplink invoke image:openai generate '{"prompt":"a lighthouse"}'
cc-uplink log --follow                 # correlation-id-threaded conversation log
```
