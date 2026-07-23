# Downstream Contracts

The contracts below are load-bearing and version-drifting. Any change to the
code that speaks them MUST update this file in the same commit
(discipline inherited from codex-image-in-cc).

## OpenAI Images API (`src/drivers/image/openai.rs`)

- Base URL: `drivers.image-openai.base_url` (default `https://api.openai.com/v1`).
- Auth: `Authorization: Bearer <key>`; key read from the env var named by
  `api_key_env` at call time; never stored, never logged.
- `generate` without refs → `POST {base}/images/generations`, JSON body
  `{model, prompt, n?, size?, quality?}` — absent options are omitted, never null.
- `generate` with refs → `POST {base}/images/edits`, multipart form:
  text fields `model`, `prompt`, optional `n`/`size`/`quality`; each ref is a
  file part named `image[]` (gpt-image-1 multi-image form).
- `edit` → `POST {base}/images/edits`, multipart form: `model`, `prompt`,
  input file part `image`, optional file part `mask`.
- Response contract: `200` JSON `{"data": [{"b64_json": "<base64 png>"}]}`.
  `gpt-image-1` always returns `b64_json` (no `url` variant is handled).
- Files are written to `out_dir` (default `./uplink-images/`) as
  `<compact-UTC-ts>-<n>.png`; results carry absolute paths.

## Codex CLI (`src/drivers/image/codex.rs`)

- Spawn: `<codex_bin> exec --sandbox workspace-write --skip-git-repo-check
  [--image <abs>]... "<instruction>"` — argv vector, never a shell.
  `--sandbox workspace-write` replaces the deprecated `--full-auto`
  (hidden from `exec --help` since ~0.144.6 but semantically equivalent for
  non-interactive `exec`; workspace-write is required so imagegen can save
  the output file into the CWD).
- stdin MUST be `Stdio::null()`: codex exec blocks reading stdin otherwise
  (documented hang, inherited from codex-image-in-cc).
- Reference/input images are passed BOTH as `--image <abs>` flags and as
  absolute paths inside the instruction text: the 0.144+ extension-backed
  image tool reads `referenced_image_paths` from the text, while
  0.142–0.143 use the `--image` attachments. Dropping either side breaks one
  version range.
- Output contract: the instruction requires one `SAVED: <absolute path>`
  line per saved image; stdout is parsed ONLY for those lines. Everything
  else codex prints is ignored — never re-parse LLM prose.
- Doctor gates: `codex --version` parseable and ≥ 0.142.0;
  `codex login status` exits 0 when logged in; `codex exec --help`
  advertises `--sandbox` and `--image`.
- Timeout: one codex run is capped at 600 s (`Timeout` error beyond that).
- Every spawn of `codex_bin` (exec, doctor probes) retries up to 5× on
  ETXTBSY with short backoff — the classic fork/exec race when the binary
  was just written (cargo applies the same mitigation); surfaced as flaky
  spawn failures in parallel test runs.

## Changelog

- 2026-07-23: all codex spawns gained ETXTBSY retry (5×, short backoff) —
  first GitHub Actions run exposed the fork/exec race as a flaky
  `cargo test --lib` failure; reproduced locally at iteration 1 of a
  200-run stress loop.
- 2026-07-23: codex spawn/doctor switched from `--full-auto` to
  `--sandbox workspace-write` — codex-cli 0.144.6 hides `--full-auto` from
  `exec --help` (still parses as a hidden alias), which false-DEGRADED the
  doctor gate; found in live smoke testing.
- 2026-07-22: initial version (M3 openai + M4 codex as shipped).
