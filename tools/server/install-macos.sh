#!/usr/bin/env bash
# Hermes Assistant Server on a Mac (Apple Silicon): download the latest release
# package and register the launchd daemon (docs/SERVER.md, «macOS»).
#
#   curl -fsSL https://raw.githubusercontent.com/koavart-dotcom/hermes-assistant/main/tools/server/install-macos.sh | bash
#
# Asks for the sudo password once. The daemon runs as the current user (not root);
# `--user NAME` as the first argument picks another account. Later versions arrive
# through the service's own self-update (signed package, the panel → «Обзор»).
set -euo pipefail

user="${1:-}"
[ "${1:-}" = "--user" ] && user="${2:-}"
user="${user:-$(id -un)}"

[ "$(uname -s)" = Darwin ] || { echo "this script is for macOS" >&2; exit 1; }
[ "$(uname -m)" = arm64 ] || { echo "only Apple Silicon (arm64) packages are published" >&2; exit 1; }
command -v git >/dev/null || { echo "git is missing: xcode-select --install" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
url="https://github.com/koavart-dotcom/hermes-assistant/releases/latest/download/hermesd-macos-arm64.zip"
echo "downloading $url"
curl -fL --progress-bar -o "$tmp/hermesd.zip" "$url"
mkdir "$tmp/pkg"
unzip -q "$tmp/hermesd.zip" -d "$tmp/pkg"
chmod 755 "$tmp/pkg/hermesd"
# Downloaded with curl, so no quarantine flag; clear it anyway in case the zip came from a browser.
xattr -dr com.apple.quarantine "$tmp/pkg" 2>/dev/null || true
sudo "$tmp/pkg/hermesd" install --user "$user"
