#!/bin/sh
# cc-uplink plugin launcher — the `.mcp.json` command.
#
# Runs the release binary pinned in `.claude-plugin/server-version`,
# downloading it into the plugin data dir on first use so the plugin's
# skill and the binary it talks to can never version-skew. Every message
# goes to stderr: stdout is the MCP stdio channel.
set -eu

# Dev override: run a local build, skip pinning entirely.
if [ -n "${CC_UPLINK_BIN:-}" ]; then
  exec "$CC_UPLINK_BIN" "$@"
fi

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
VER=$(cat "$ROOT/.claude-plugin/server-version")
case "$VER" in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "cc-uplink launcher: bad version pin '$VER' in .claude-plugin/server-version" >&2
    exit 1
    ;;
esac
# CLAUDE_PLUGIN_DATA survives plugin updates; CLAUDE_PLUGIN_ROOT does not.
DATA="${CLAUDE_PLUGIN_DATA:-${XDG_DATA_HOME:-$HOME/.local/share}/cc-uplink}"
BIN="$DATA/bin/$VER/cc-uplink"

if [ ! -x "$BIN" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)   target=x86_64-unknown-linux-musl ;;
    Linux-aarch64)  target=aarch64-unknown-linux-musl ;;
    Darwin-arm64)   target=aarch64-apple-darwin ;;
    Darwin-x86_64)  target=x86_64-apple-darwin ;;
    *)
      echo "cc-uplink launcher: unsupported platform $(uname -s)/$(uname -m);" \
        "build from source and set CC_UPLINK_BIN" >&2
      exit 1
      ;;
  esac
  command -v curl >/dev/null 2>&1 || {
    echo "cc-uplink launcher: curl is required to download the pinned binary" >&2
    exit 1
  }

  asset="cc-uplink-$target.tar.gz"
  # taiki-e/upload-rust-binary-action names the checksum `<bin>-<target>.sha256`
  # (no .tar.gz); its content references the .tar.gz filename.
  # The checksum guards download integrity, not release authenticity — it
  # ships next to the tarball. The trust root is the version pin + HTTPS.
  sum="cc-uplink-$target.sha256"
  base="https://github.com/XBlueSky/cc-uplink/releases/download/v$VER"

  mkdir -p "$DATA/bin/$VER"
  # tmp dir inside the version dir: same filesystem, so the final mv is
  # atomic even when two sessions race the first download.
  tmp=$(mktemp -d "$DATA/bin/$VER/.download.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT

  echo "cc-uplink launcher: downloading $asset (v$VER)" >&2
  curl -fsSL --proto '=https' --proto-redir '=https' -o "$tmp/$asset" "$base/$asset" >&2
  curl -fsSL --proto '=https' --proto-redir '=https' -o "$tmp/$sum" "$base/$sum" >&2

  # Verify BEFORE anything from the archive is trusted or executed.
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp" && sha256sum -c "$sum") >&2
  else
    (cd "$tmp" && shasum -a 256 -c "$sum") >&2
  fi

  tar -xzf "$tmp/$asset" -C "$tmp" cc-uplink
  chmod +x "$tmp/cc-uplink"
  mv -f "$tmp/cc-uplink" "$BIN"
  rm -rf "$tmp"
  trap - EXIT
fi

exec "$BIN" "$@"
