#!/bin/sh
# Installs or updates SSHeesh on macOS with one command:
#
#   curl -fsSL https://raw.githubusercontent.com/footprintsntrails/ssheesh/main/scripts/install-macos.sh | sh
#
# It downloads the latest release from GitHub, puts SSHeesh.app in
# /Applications and opens it. An app downloaded this way isn't flagged as
# "from the internet", so macOS opens it without the "can't verify the
# developer" prompt and there's nothing to allow in System Settings.
set -eu

REPO="footprintsntrails/ssheesh"
APP="/Applications/SSHeesh.app"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() {
  printf '\033[31m%s\033[0m\n' "$*" >&2
  exit 1
}

[ "$(uname -s)" = "Darwin" ] || fail "This installer is for macOS. Get SSHeesh for Windows and Linux at https://github.com/$REPO/releases/latest"

say "Finding the latest version..."
TAG=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" | sed 's#.*/tag/##')
VERSION=${TAG#v}
case "$VERSION" in
  [0-9]*) ;;
  *) fail "Couldn't find the latest release. Check your internet connection and try again." ;;
esac

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

say "Downloading SSHeesh $VERSION..."
curl -fL --progress-bar -o "$TMP/ssheesh.tar.gz" \
  "https://github.com/$REPO/releases/download/$TAG/SSHeesh_${VERSION}_universal.app.tar.gz" ||
  fail "The download didn't work. Try again in a minute."
tar -xzf "$TMP/ssheesh.tar.gz" -C "$TMP"
[ -d "$TMP/SSHeesh.app" ] || fail "The download looks broken. Try again in a minute."

if pgrep -xq SSHeesh; then
  say "Closing SSHeesh so it can be replaced..."
  osascript -e 'quit app "SSHeesh"' >/dev/null 2>&1 || true
  sleep 2
fi

say "Installing to /Applications..."
if [ -w /Applications ]; then
  rm -rf "$APP"
  mv "$TMP/SSHeesh.app" "$APP"
else
  echo "macOS needs your password to write to /Applications."
  sudo rm -rf "$APP"
  sudo mv "$TMP/SSHeesh.app" "$APP"
fi
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

say "SSHeesh $VERSION is installed. Opening it now."
open "$APP"
