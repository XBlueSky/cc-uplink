# Security Policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please report vulnerabilities **privately** via GitHub's Private
Vulnerability Reporting: open the repo's **Security** tab and click
**"Report a vulnerability"**, or go directly to
<https://github.com/XBlueSky/cc-uplink/security/advisories/new>.

Do **not** open a public issue for an exploitable bug.

You can expect an acknowledgement within a few days. Once a fix ships,
we'll credit you in the advisory unless you prefer otherwise.

## Threat model

The README's **Security posture** section documents the design decisions
relevant to security review: argv-only process invocation (no shell text),
loop prevention, deny-by-default tiered write permissions (human-only
grants), env-only secrets, and the deliberate plaintext visibility of
injected envelopes in tmux panes.
