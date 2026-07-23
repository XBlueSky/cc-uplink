# Plugin Directory Restructure + OSS Hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the Claude Code plugin from repo root into `plugins/cc-uplink/`, and add community docs (CONTRIBUTING, CODE_OF_CONDUCT, SECURITY), GitHub issue/PR templates, and README badges.

**Architecture:** Pure restructure + docs. The marketplace manifest stays at root `.claude-plugin/marketplace.json` (spec requirement) with its `source` repointed; everything plugin-scoped moves under `plugins/cc-uplink/`. One Rust source line (`include_str!`) follows the move. No behavior changes.

**Tech Stack:** git mv, Rust (`include_str!` path only), GitHub issue forms (YAML), shields.io badges.

## Global Constraints

- All documents in English (repo convention).
- All file moves via `git mv` (preserve history).
- No personal email published anywhere; security/CoC contact goes through GitHub.
- Exactly two feature commits: the move commit, then the docs commit.
- Verification gate for the move: `cargo test`, `cargo fmt --check`, `cargo clippy --release --all-targets`, cc-marketspec check, plugin-validator agent.

---

### Task 1: Move plugin into `plugins/cc-uplink/`

**Files:**
- Move: `.claude-plugin/plugin.json` → `plugins/cc-uplink/.claude-plugin/plugin.json`
- Move: `.claude-plugin/server-version` → `plugins/cc-uplink/.claude-plugin/server-version`
- Move: `.mcp.json` → `plugins/cc-uplink/.mcp.json`
- Move: `scripts/launcher.sh` → `plugins/cc-uplink/scripts/launcher.sh`
- Move: `skills/` → `plugins/cc-uplink/skills/` (contains `uplink/SKILL.md`)
- Modify: `.claude-plugin/marketplace.json` (source path)
- Modify: `src/cli/mod.rs:10` (include_str path)
- Modify: `README.md:142-143` (Releasing section paths)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: plugin root at `plugins/cc-uplink/` — Task 2 does not depend on it, but future release commits bump `plugins/cc-uplink/.claude-plugin/plugin.json` and `plugins/cc-uplink/.claude-plugin/server-version`.

- [x] **Step 1: git mv everything plugin-scoped**

```bash
mkdir -p plugins/cc-uplink/.claude-plugin plugins/cc-uplink/scripts
git mv .claude-plugin/plugin.json      plugins/cc-uplink/.claude-plugin/plugin.json
git mv .claude-plugin/server-version   plugins/cc-uplink/.claude-plugin/server-version
git mv .mcp.json                       plugins/cc-uplink/.mcp.json
git mv scripts/launcher.sh             plugins/cc-uplink/scripts/launcher.sh
git mv skills                          plugins/cc-uplink/skills
```

Expected: `git status` shows renames only; root `scripts/` disappears (was launcher-only); root `.claude-plugin/` keeps only `marketplace.json`.

- [x] **Step 2: Repoint marketplace source**

In `.claude-plugin/marketplace.json` change:

```json
    { "name": "cc-uplink", "source": "./" }
```

to:

```json
    { "name": "cc-uplink", "source": "./plugins/cc-uplink" }
```

- [x] **Step 3: Follow the move in `src/cli/mod.rs`**

```rust
pub(crate) const SKILL_MD: &str =
    include_str!("../../plugins/cc-uplink/skills/uplink/SKILL.md");
```

(The skill-install tests in the same file write to `<claude_home>/skills/uplink/` — an install destination, not a repo path — and stay untouched.)

- [x] **Step 4: Update README Releasing paths**

Lines 142–143, add the `plugins/cc-uplink/` prefix:

```markdown
2. `plugins/cc-uplink/.claude-plugin/plugin.json` `version` (plugin cache/display axis)
3. `plugins/cc-uplink/.claude-plugin/server-version` (the binary the plugin launcher pins)
```

(README line 43's `.claude-plugin/server-version` and line 42's `scripts/launcher.sh` are plugin-root-relative — leave them.)

- [x] **Step 5: Verify build + tests**

Run: `cargo test`
Expected: all pass (a wrong include_str path fails at compile time).

Run: `cargo fmt --check && cargo clippy --release --all-targets`
Expected: clean.

- [x] **Step 6: Validate marketplace + plugin structure**

Invoke the `cc-marketspec:cc-check` skill; expected: validation passes with the new `source`. If it asks for regeneration, run `cc-marketspec:cc-generate`.

Dispatch the `plugin-dev:plugin-validator` agent on `plugins/cc-uplink/`; expected: no structural errors (plugin.json valid, .mcp.json uses `${CLAUDE_PLUGIN_ROOT}`, skill frontmatter valid).

- [x] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(plugin): move plugin into plugins/cc-uplink/"
```

---

### Task 2: Community docs, GitHub templates, README badges

**Files:**
- Create: `CONTRIBUTING.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `SECURITY.md`
- Create: `.github/ISSUE_TEMPLATE/bug_report.yml`
- Create: `.github/ISSUE_TEMPLATE/feature_request.yml`
- Create: `.github/ISSUE_TEMPLATE/config.yml`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`
- Modify: `README.md` (badges at top; Contributing section before License)

**Interfaces:**
- Consumes: `plugins/cc-uplink/` layout from Task 1 (CONTRIBUTING references it).
- Produces: nothing downstream.

- [x] **Step 1: Write `CONTRIBUTING.md`**

Content: intro; dev setup (toolchain pinned by `rust-toolchain.toml`, tmux ≥ 3.2 needed for integration tests); commands `cargo test`, `cargo fmt --check`, `cargo clippy --release --all-targets`; plugin layout note (`plugins/cc-uplink/`, `CC_UPLINK_BIN` dev override); conventional-commit style matching repo history (`feat:`, `fix:`, `docs:`, `ci:`, `chore:`); PR expectations (tests for behavior changes, docs for user-facing changes); pointer to README Releasing section and the three version fields that move together.

- [x] **Step 2: Write `CODE_OF_CONDUCT.md`**

Contributor Covenant v2.1 standard text; enforcement contact = report privately to the maintainer via GitHub (no email).

- [x] **Step 3: Write `SECURITY.md`**

Supported version = latest release. Report via GitHub Private Vulnerability Reporting (Security tab → "Report a vulnerability", i.e. `https://github.com/XBlueSky/cc-uplink/security/advisories/new`); never open public issues for exploitable bugs; link README's Security-posture section for the threat model.

- [x] **Step 4: Write issue forms**

`bug_report.yml` (YAML form, label `bug`): what happened / expected / repro steps (textareas), platform input (OS + arch), install-method dropdown (plugin / prebuilt binary / built from source), `cc-uplink doctor` output textarea (optional).

`feature_request.yml` (label `enhancement`): problem, proposed solution, alternatives (textareas).

`config.yml`:

```yaml
blank_issues_enabled: false
contact_links:
  - name: Report a security vulnerability
    url: https://github.com/XBlueSky/cc-uplink/security/advisories/new
    about: Please report exploitable bugs privately — see SECURITY.md.
```

- [x] **Step 5: Write `.github/PULL_REQUEST_TEMPLATE.md`**

Sections: Summary; How verified (checkboxes for `cargo test`, `cargo fmt --check && cargo clippy --release --all-targets`); Checklist (docs updated if user-facing, no secrets/keys in code or fixtures).

- [x] **Step 6: README badges + Contributing section**

Immediately under the `# cc-uplink` H1:

```markdown
[![CI](https://github.com/XBlueSky/cc-uplink/actions/workflows/ci.yml/badge.svg)](https://github.com/XBlueSky/cc-uplink/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/XBlueSky/cc-uplink)](https://github.com/XBlueSky/cc-uplink/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
```

New section between `## Releasing` and `## License`:

```markdown
## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for dev
setup, test commands, and PR expectations. Report security issues
privately per [SECURITY.md](SECURITY.md).
```

- [x] **Step 7: Sanity-check YAML + commit**

Run: `python3 -c "import yaml,glob; [yaml.safe_load(open(f)) for f in glob.glob('.github/ISSUE_TEMPLATE/*.yml')]"`
Expected: no output (all forms parse).

```bash
git add CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md .github README.md
git commit -m "docs: add community files + README badges"
```
