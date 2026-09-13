#!/bin/sh
set -eu

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "This download does not support your Mac. See the requirements in the README." >&2
  exit 1
}

repo=smokey18/Ledge
tmp=$(mktemp -d)
staging=
destination=/Applications/Ledge.app
cleanup() {
  hdiutil detach "$tmp/mnt" >/dev/null 2>&1 || true
  rm -rf "$tmp"
  if [ -n "$staging" ]; then
    if [ -d "$staging/previous.app" ] && [ ! -e "$destination" ]; then
      mv "$staging/previous.app" "$destination" || {
        echo "Restore your previous app from $staging/previous.app" >&2
        return
      }
    fi
    rm -rf "$staging"
  fi
}
trap cleanup EXIT

url=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
  sed -n 's/.*"browser_download_url": *"\(.*\.dmg\)".*/\1/p' | head -1)
[ -n "$url" ] || { echo "no .dmg found in the latest release" >&2; exit 1; }

echo "Downloading ${url##*/}"
curl -fsSL -o "$tmp/ledge.dmg" "$url"
hdiutil attach -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/ledge.dmg" >/dev/null

[ -d "$tmp/mnt/Ledge.app" ] || { echo "The download contains no Ledge.app" >&2; exit 1; }
staging=$(mktemp -d /Applications/.ledge.XXXXXX)
cp -R "$tmp/mnt/Ledge.app" "$staging/Ledge.app"
if [ -e "$destination" ]; then
  mv "$destination" "$staging/previous.app"
fi
mv "$staging/Ledge.app" "$destination"

echo "Installed /Applications/Ledge.app"
