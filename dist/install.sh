#!/usr/bin/env bash
# rak-setup bootstrap — downloads and runs the rak-setup wizard for Linux.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.sh | bash
#
# Passes any extra args through to rak-setup (e.g. `| bash -s -- --yes --install rakc,rakpkg`).
set -euo pipefail

ARCH="$(uname -m)"
OS="$(uname -s)"
case "$OS" in
  Linux) os=linux ;;
  *) echo "rak-setup: this bootstrap is for Linux (got $OS). On Windows, use install.ps1." >&2; exit 1 ;;
esac
case "$ARCH" in
  x86_64|amd64) arch=x86_64 ;;
  *) echo "rak-setup: unsupported arch $ARCH (only x86_64 for now)" >&2; exit 1 ;;
esac

ASSET="rak-setup-${os}-${arch}"
URL="https://github.com/Louiml/Rak/releases/latest/download/${ASSET}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
echo "[rak-setup] downloading $URL"
curl -fsSL "$URL" -o "$TMP/$ASSET"
chmod +x "$TMP/$ASSET"
exec "$TMP/$ASSET" "$@"
