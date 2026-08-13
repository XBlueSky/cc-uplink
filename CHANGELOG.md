# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-08-13

### Bug Fixes

- close read_deny rename escape; harden send capture and doctor warning for read-blocked panes
- dangerous-keys test asserts benign-at-operator; hot-reload test cleans env panic-safe
- parse_pane_line fails closed on pipe-polluted pane lines
- parse_marks fails closed on pipe-polluted pane marks
- amber focus rings for the instrument pre regions
- honour the AA gates the reskin's seams broke, tokenize the instrument chrome
- pin the journey grid rows so flat mode survives the hidden trace
- anchor the trace to the pane gap, true-scale strokes, strip authoring comments
- wrap pane pre lines so the envelope tail is visible at rest
- self-healing enhancement gate, paint-stable copy buttons, final polish
- pre-paint enhancement gate, honest script budget, fail-flat demo
- assert the budget test really sees the inline enhance bundle
- render the you-pane fade, centre the pinned frame, honour the data-line contract
- scope the copy hooks, meet AA on the envelope tail, guard the hook inventory
- cap the act-4 artifact tighter at short viewports
- fit every pinned act inside the viewport, honour the amber ruling, harden edges
- ship a real 404 page so Pages stops SPA-rewriting misses to 200
- make the signal actually travel and harden the choreography contract
- single rAF chain, honoured context attrs, context-loss recovery, aligned beam
- keep reduced motion inert and the act stack correct before first scroll
- pass WCAG AA code contrast, keyboard access, and derive two more facts
- repin dependencies to the public npm registry, pin Node
- resolve manifest via static import instead of cwd arithmetic
- correct the landing's code samples against the real wire format
- widen no-JS guard to every docs page and derive titles from H1
- validate tool/tip/trap entries and improve missing-manifest error
- close guard drift and case/mutation gaps in content sync
- scan test files with an explicit quoted glob
- make the idle-wait timeout self-triaging
- make idle detection, ask, and recv work against TUI peers

### CI/CD

- automate releases with release-plz and guard the three version pins
- skip the gate on release completion, ignore wrangler's cache
- gate the build, leave deploy to the Pages Git integration
- build, gate, and deploy to Cloudflare Pages

### Documentation

- tiered permission model, type op, observe/act split, grant affordances
- extract configuration, CLI, and security reference into docs/
- remove-setup-command
- remove-setup-command design
- add community files + README badges
- plugin dir restructure + oss hygiene
- plugin dir restructure + oss hygiene design

### Features

- ship uplink.tmux grant menu + border badges via tmux-snippet
- doctor reports policy summary, grants, and contradiction warnings
- channel_list surfaces effective profile and readability per pane
- split channel_invoke into channel_observe/channel_act via OpSpec.mutating
- type op — literal no-hang console injection with sensitive redaction
- read gate on content ops; label operator-gated with escalation guard
- hot-reloaded policy cache; send/keys gated by tier, keys classified per chord
- pane marks, effective-tier resolution, read block, key classifier
- Tier enum, glob matcher, write_allow/read_deny config schema
- retire the page shader; light now lives in the instruments
- the landing becomes an engineering drawing
- panes become phosphor instruments; chrome joins the paper world
- ink-and-light tokens, Instrument Serif, paper ground
- ambience tracks scroll; enhancement polish
- demo scrub, reveals, rail scrollspy, hero heartbeat
- scroll-scrubbed signal journey as the page's one pinned scene
- replace the pinned deck with the flat six-section landing
- add scrub.mjs pure progress math for the redesign
- brand the site with the cc-uplink logo and act 4's real artifact
- choreograph the split, envelope typing, signal, and reply
- render the ambience as a scroll-driven WebGL shader
- pin and scrub the six acts with GSAP and Lenis
- add ambience layer with the CSS gradient fallback
- derive the ambience brightness ceiling from WCAG AA
- add pure act-state maths for the scroll choreography
- add the six-act landing in static form
- render docs pages with a guard against publishing internal docs
- add status-line navigation and pane components
- fetch release data at build time with a safe fallback
- add manifest loader that fails the build on bad data
- add content sync that refuses to publish internal docs
- scaffold Astro site with design tokens
- ship as Claude Code plugin + self-hosted marketplace

### Miscellaneous

- stop tracking superpowers process docs
- mark remove-setup-command plan executed
- mark plugin-dir/oss-hygiene plan executed

### Refactoring

- **BREAKING** remove cc-uplink setup — plugin install supersedes it
- move plugin into plugins/cc-uplink/

### Testing

- integration coverage for type, tier gates, hot reload, read block
- assert the landing page ships no JavaScript

### Style

- cargo fmt across the permission-model changes
