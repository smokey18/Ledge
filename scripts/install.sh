#!/bin/sh
set -eu

repo=smokey18/Ledge
tmp=$(mktemp -d)
trap 'hdiutil detach "$tmp/mnt" >/dev/null 2>&1 || true; rm -rf "$tmp"' EXIT

url=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
  sed -n 's/.*"browser_download_url": *"\(.*\.dmg\)".*/\1/p' | head -1)
[ -n "$url" ] || { echo "no .dmg found in the latest release" >&2; exit 1; }

echo "Downloading ${url##*/}"
curl -fsSL -o "$tmp/ledge.dmg" "$url"
hdiutil attach -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/ledge.dmg" >/dev/null

rm -rf /Applications/Ledge.app
cp -R "$tmp/mnt/Ledge.app" /Applications/Ledge.app

echo "Installed /Applications/Ledge.app"
