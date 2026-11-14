#!/bin/sh
# Makes desktop/vendor/rune: Rune's `rune` crate at the rev desktop/Cargo.toml pins, as a crate
# of its own, with our fixes in desktop/vendor/rune.patch applied. desktop/Cargo.toml builds it
# in place of Rune's (`[patch."https://github.com/anomalyco/rune"]`).
#
#   scripts/vendor-rune.sh              remake desktop/vendor/rune (after a Rune bump, or an
#                                       edit of rune.patch)
#   scripts/vendor-rune.sh --into DIR   make it in DIR instead (check-desktop-workspace.sh
#                                       compares the two)
#   scripts/vendor-rune.sh --plain DIR  the same without rune.patch: to make a new patch,
#                                       `diff -ruN DIR/rune desktop/vendor/rune`
#
# Why a copy: Rune is a git dependency, and its fixes we need aren't in it yet. Cargo patches a
# git dependency only with another source, so the crate is copied here and patched. What
# changes from Rune's own crate besides the patch:
# - only what building the library reads: Cargo.toml, build.rs, src/, assets/, the precompiled
#   Windows shaders (*.dxbc) and Rune's LICENSE (MIT); not its tests, examples or README;
# - Cargo.toml: the values it took from Rune's workspace written out, its path dependencies on
#   Rune's other crates made git dependencies at the same rev, its tests, examples and
#   dev-dependencies left out (their files aren't here).
# The other crates of Rune (rune-motion, rune-taffy, the patched winit...) still come from git.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
desk="$root/desktop"
url="https://github.com/anomalyco/rune"
out="$desk/vendor"
patch="$desk/vendor/rune.patch"
case "${1:-}" in
  --into) out=$2 ;;
  --plain) out=$2; patch= ;;
  "") ;;
  *) echo "usage: $0 [--into DIR | --plain DIR]" >&2; exit 2 ;;
esac

rev=$(sed -n 's/^rune = {.*rev = "\([0-9a-f]*\)".*/\1/p' "$desk/Cargo.toml")
[ -n "$rev" ] || { echo "vendor-rune: desktop/Cargo.toml pins no rev for rune" >&2; exit 1; }
short=$(printf '%s' "$rev" | cut -c1-7)

# Rune's repository at that rev: Cargo's checkout when it has one, else a fresh fetch.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
src=$(ls -d "$cargo_home"/git/checkouts/rune-*/"$short" 2>/dev/null | head -n 1 || true)
tmp=
if [ -z "$src" ] || [ ! -f "$src/crates/rune/Cargo.toml" ]; then
  tmp=$(mktemp -d)
  git -C "$tmp" init -q
  git -C "$tmp" fetch -q --depth 1 "$url" "$rev"
  git -C "$tmp" checkout -q FETCH_HEAD
  src=$tmp
fi
crate="$src/crates/rune"

dest="$out/rune"
rm -rf "$dest"
mkdir -p "$dest"
cp -R "$crate/src" "$crate/assets" "$crate/build.rs" "$dest/"
cp "$crate"/*.dxbc "$dest/"
cp "$src/LICENSE" "$dest/LICENSE"

# Cargo.toml: workspace values written out, path dependencies to git, no tests or examples.
edition=$(sed -n '/^\[workspace.package\]/,/^\[/s/^edition = "\(.*\)"/\1/p' "$src/Cargo.toml")
license=$(sed -n '/^\[workspace.package\]/,/^\[/s/^license = "\(.*\)"/\1/p' "$src/Cargo.toml")
{
  awk -v edition="$edition" -v license="$license" -v git="git = \"$url\", rev = \"$rev\"" '
    /^\[/ {
      skip = ($0 ~ /^\[\[(example|test|bench)\]\]$/ || $0 ~ /dev-dependencies\]$/ || $0 == "[lints]")
    }
    skip { next }
    /^edition\.workspace = true$/ { print "edition = \"" edition "\""; next }
    /^license\.workspace = true$/ { print "license = \"" license "\""; next }
    /^[A-Za-z0-9_-]+ = \{ path = "[^"]*"/ { sub(/path = "[^"]*"/, git) }
    { print }
  ' "$crate/Cargo.toml"
  # Rune's workspace lints, as the crate had them; and its warnings off, as for any git or
  # registry dependency (Cargo caps their lints; a path dependency's would show in our builds).
  echo
  awk '
    /^\[/ { inside = ($0 ~ /^\[workspace\.lints\./) }
    inside { line = $0; sub(/^\[workspace\.lints\./, "[lints.", line); print line }
    $0 == "[workspace.lints.rust]" { print "warnings = { level = \"allow\", priority = -1 }" }
  ' "$src/Cargo.toml"
} >"$dest/Cargo.toml"
cat >"$dest/VENDORED.md" <<EOF
# Rune's \`rune\` crate, vendored

From $url at $rev (crates/rune), made by \`scripts/vendor-rune.sh\`$( [ -n "$patch" ] && printf ' with `desktop/vendor/rune.patch` applied' ). Don't edit it
here: edit the patch (or the script) and run the script again. Licence: MIT, \`LICENSE\`.
EOF

if [ -n "$patch" ]; then
  (cd "$dest" && patch -s -p1 <"$patch")
fi
[ -z "$tmp" ] || rm -rf "$tmp"
echo "vendor-rune: $dest (Rune $short$( [ -n "$patch" ] && echo ', patched' ))"
