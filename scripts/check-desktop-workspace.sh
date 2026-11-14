#!/bin/sh
# Keeps the 1.0 workspace (desktop/) in step with the root one (src-tauri and the shared crates).
# Run in CI and before every Rune bump:
#
#   scripts/check-desktop-workspace.sh
#
# 1. desktop/Cargo.toml patches exactly what Rune's root Cargo.toml patches at the pinned rev,
#    each from that same rev, plus our saturating-time. A dependency's [patch] doesn't reach the
#    app that builds it, so a patch Rune adds and we miss builds the stock crate without a word.
# 2. saturating-time is patched in both workspaces (Arti's copy hangs Tor on Windows).
# 3. Crates both lock files have resolve to the same version, so sshee-core runs on the same
#    crates under 1.0 as in the app 0.10 ships.
# 4. The bundled fonts are the files desktop/sshee-ui/fonts/SOURCES.md lists.
# 5. Rune's own crate is ours patched: desktop/Cargo.toml takes `rune` from desktop/vendor/rune,
#    and that copy is exactly what scripts/vendor-rune.sh makes from the pinned rev and
#    desktop/vendor/rune.patch (no edit made there by hand, no patch left unapplied).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
desk="$root/desktop"
failed=0
fail() {
  printf 'check-desktop-workspace: %s\n' "$*" >&2
  failed=1
}

# The [patch.crates-io] table of a Cargo.toml: one "name<TAB>rest of the line" per entry.
patches() {
  awk '
    /^\[/ { inside = ($0 == "[patch.crates-io]"); next }
    inside && /^[A-Za-z0-9_-]+[ \t]*=/ {
      name = $0; sub(/[ \t]*=.*/, "", name)
      rest = $0; sub(/^[^=]*=[ \t]*/, "", rest)
      print name "\t" rest
    }' "$1"
}

# The rev `rune` is pinned to.
rev=$(sed -n 's/^rune = {.*rev = "\([0-9a-f]*\)".*/\1/p' "$desk/Cargo.toml")
[ -n "$rev" ] || { fail "desktop/Cargo.toml: no rev for rune"; exit 1; }
short=$(printf '%s' "$rev" | cut -c1-7)

# Rune's own Cargo.toml at that rev, as Cargo checked it out.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
find_rune() { ls -d "$cargo_home"/git/checkouts/rune-*/"$short" 2>/dev/null | head -n 1; }
checkout=$(find_rune)
if [ -z "$checkout" ]; then
  (cd "$desk" && cargo fetch --locked >/dev/null)
  checkout=$(find_rune)
fi
[ -n "$checkout" ] && [ -f "$checkout/Cargo.toml" ] || { fail "Rune at $short isn't checked out (cargo fetch in desktop/)"; exit 1; }

# 1. The same patch list as Rune's, from the same rev.
theirs=$(patches "$checkout/Cargo.toml" | cut -f1 | sort)
ours=$(patches "$desk/Cargo.toml" | cut -f1 | grep -vx 'saturating-time' | sort)
if [ "$theirs" != "$ours" ]; then
  fail "desktop/Cargo.toml patches [$(echo $ours)], Rune $short patches [$(echo $theirs)]"
fi
stray=$(patches "$desk/Cargo.toml" | awk -F '\t' -v want="{ git = \"https://github.com/anomalyco/rune\", rev = \"$rev\" }" \
  '$1 != "saturating-time" && $2 != want { print $1 }')
[ -z "$stray" ] || fail "desktop/Cargo.toml: not from Rune's git at $rev: $(echo $stray)"

# 2. saturating-time in both workspaces, from our copy.
patches "$root/Cargo.toml" | grep -q '^saturating-time	{ path = "crates/saturating-time" }$' ||
  fail "Cargo.toml doesn't patch saturating-time with crates/saturating-time"
patches "$desk/Cargo.toml" | grep -q '^saturating-time	{ path = "../crates/saturating-time" }$' ||
  fail "desktop/Cargo.toml doesn't patch saturating-time with ../crates/saturating-time"

# 3. Lock files: "name compat version" for every crates.io package. Versions Cargo would unify
#    (the same major, or the same 0.minor) must be equal; 0.8 beside 0.9 are different crates.
locked() {
  awk '
    function compat(v,  p) {
      split(v, p, ".")
      if (p[1] != "0") return p[1]
      if (p[2] != "0") return "0." p[2]
      return v
    }
    /^\[\[package\]\]/ { name = ""; version = ""; registry = 0 }
    /^name = /     { name = $3; gsub(/"/, "", name) }
    /^version = /  { version = $3; gsub(/"/, "", version) }
    /^source = "registry\+/ { registry = 1 }
    /^$/ { if (name != "" && registry) print name, compat(version), version; name = "" }
    END  { if (name != "" && registry) print name, compat(version), version }' "$1" | sort -u
}
locked "$root/Cargo.lock" >"${TMPDIR:-/tmp}/sshee-lock-root.$$"
locked "$desk/Cargo.lock" >"${TMPDIR:-/tmp}/sshee-lock-desk.$$"
differ=$(awk '
  NR == FNR { root[$1 " " $2] = $3; next }
  ($1 " " $2) in root && root[$1 " " $2] != $3 { print "  " $1 ": " root[$1 " " $2] " (root) vs " $3 " (desktop)" }
' "${TMPDIR:-/tmp}/sshee-lock-root.$$" "${TMPDIR:-/tmp}/sshee-lock-desk.$$")
rm -f "${TMPDIR:-/tmp}/sshee-lock-root.$$" "${TMPDIR:-/tmp}/sshee-lock-desk.$$"
if [ -n "$differ" ]; then
  fail "the two lock files resolve these crates differently (cargo update -p NAME --precise V in one of them):
$differ"
fi

# 4. Fonts.
fonts="$desk/sshee-ui/fonts"
sed -n '/^```sha256$/,/^```$/p' "$fonts/SOURCES.md" | grep -v '^```' >"${TMPDIR:-/tmp}/sshee-fonts.$$"
if ! (cd "$fonts" && sha256sum --quiet -c "${TMPDIR:-/tmp}/sshee-fonts.$$"); then
  fail "a font in desktop/sshee-ui/fonts isn't the file SOURCES.md lists"
fi
listed=$(awk '{ print $2 }' "${TMPDIR:-/tmp}/sshee-fonts.$$" | sort)
rm -f "${TMPDIR:-/tmp}/sshee-fonts.$$"
present=$(cd "$fonts" && ls | grep -vx 'SOURCES.md' | sort)
[ "$listed" = "$present" ] || fail "desktop/sshee-ui/fonts holds files SOURCES.md doesn't list, or misses some"

# 5. The vendored rune crate.
rune_patch=$(awk '
  /^\[/ { inside = ($0 == "[patch.\"https://github.com/anomalyco/rune\"]"); next }
  inside && /^[A-Za-z0-9_-]+[ \t]*=/ { print }' "$desk/Cargo.toml")
[ "$rune_patch" = 'rune = { path = "vendor/rune" }' ] ||
  fail "desktop/Cargo.toml must patch Rune's git with exactly rune = { path = \"vendor/rune\" }, not: $(echo $rune_patch)"
fresh="${TMPDIR:-/tmp}/sshee-vendor-rune.$$"
if sh "$root/scripts/vendor-rune.sh" --into "$fresh" >/dev/null; then
  if ! diff -r "$fresh/rune" "$desk/vendor/rune" >"$fresh.diff" 2>&1; then
    fail "desktop/vendor/rune isn't what scripts/vendor-rune.sh makes (run it again, and put changes in vendor/rune.patch):
$(head -n 20 "$fresh.diff")"
  fi
else
  fail "scripts/vendor-rune.sh failed"
fi
rm -rf "$fresh" "$fresh.diff"

[ "$failed" = 0 ] && echo "check-desktop-workspace: ok (Rune $short)"
exit "$failed"
