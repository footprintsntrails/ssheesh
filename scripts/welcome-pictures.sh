#!/bin/sh
# Makes desktop/sshee-ui/pictures/welcome/*.png: the welcome's pictures of the window, drawn by
# SSHeesh Preview itself with --demo (made-up servers and output, nothing from this computer),
# headless (--snap), then shrunk with ImageMagick.
#
#   scripts/welcome-pictures.sh [PREVIEW]   PREVIEW: the program to draw with (default: the
#                                           debug build, target/desktop/debug/ssheesh-preview)
#
# Needs a Vulkan driver (Mesa's lavapipe does) and ImageMagick 7 (`magick`). Run it after a
# change the pictures should show, then rebuild: they're built into the program.
#
# - look-*: the whole window in the light and the dark appearance, and half of each for
#   "Match my system", at 360 x 225 pixels (the cards are about 180 x 112 points).
# - <dark|light>-copy-picked: a terminal on backups with a file name selected.
# - <dark|light>-drop-*: the same terminal connected, then reconnecting, coming back, asking
#   ("Ask me") and saying Disconnected ("Off", picked in the welcome itself).
# The terminal ones are a 480 x 270 window drawn at twice its size, shrunk to 544 x 306 pixels
# (272 x 153 points).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bin=${1:-$root/target/desktop/debug/ssheesh-preview}
out=$root/desktop/sshee-ui/pictures/welcome
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
lvp=/usr/share/vulkan/icd.d/lvp_icd.json
if [ -z "${VK_ICD_FILENAMES:-}" ] && [ -f "$lvp" ]; then
  export VK_ICD_FILENAMES=$lvp
fi

# shot NAME THEME SCALE SIZE SCRIPT [ARGS...]: the window after SCRIPT (Rune script events, ending
# in the mark "end"), with a home of its own; ARGS go after --demo.
shot() {
  name=$1 theme=$2 scale=$3 size=$4 script=$5
  shift 5
  home=$work/home-$name
  mkdir -p "$home"
  printf '%s\n' "$script" >"$work/$name.json"
  HOME=$home XDG_DATA_HOME=$home/data XDG_CONFIG_HOME=$home/config SSHEE_PREVIEW_DIR=$home/.sshee-preview \
    "$bin" --demo --theme "$theme" --scale "$scale" --size "$size" "$@" \
    --snap "$work/$name.png" --script "$work/$name.json" --at mark:end >/dev/null
}

end='{ "t": 1.2, "type": "mark", "name": "end" }'

# ---- the window in each appearance
for theme in dark light; do
  shot "look-$theme" "$theme" 1 1280x800 "[$end]"
  magick "$work/look-$theme.png" -resize 360x225 -strip -colors 160 "PNG8:$out/look-$theme.png"
done
# "Match my system": dark at the left, light at the right, cut on a slant.
magick "$work/look-dark.png" "$work/look-light.png" \
  \( -size 1280x800 xc:black -fill white -draw "polygon 710,0 1280,0 1280,800 570,800" \) \
  -composite -resize 360x225 -strip -colors 160 "PNG8:$out/look-system.png"

# ---- the terminal: backups in a small window at twice its size
# term PICTURE THEME SCRIPT [ARGS...]; `size` is the window it starts in.
size=480x270
term() {
  pic=$1 look=$2 events=$3
  shift 3
  shot "$pic-$look" "$look" 2 "$size" "$events" "$@"
  magick "$work/$pic-$look.png" -crop 960x540+0+0 +repage -resize 544x306 -strip -colors 128 "PNG8:$out/$look-$pic.png"
}
open='{ "t": 0.3, "type": "tap", "key": "server-3" }'
late='{ "t": 1.6, "type": "mark", "name": "end" }'
drag='{ "t": 0.9, "type": "drag", "x": 65, "y": 87, "dx": 140, "dy": 0, "dur": 0.3 }'
# "Off" is picked in the welcome, in a window large enough for it: Continue twice, Off, Esc;
# then the window is made small and backups opened.
off='{ "t": 0.3, "type": "tap", "key": "welcome-next" }, { "t": 0.6, "type": "tap", "key": "welcome-next" },
     { "t": 0.9, "type": "tap", "key": "welcome-term-reconnect-off" }, { "t": 1.2, "type": "key", "name": "Escape" },
     { "t": 1.4, "type": "resize", "dw": -800, "dh": -530, "dur": 0.1 },
     { "t": 1.8, "type": "tap", "key": "server-3" }, { "t": 3.1, "type": "mark", "name": "end" }'
for theme in dark light; do
  # Copying starts from drop-on: the same terminal, nothing selected.
  term copy-picked "$theme" "[$open, $drag, $late]" --demo-card connected
  term drop-on "$theme" "[$open, $late]" --demo-card connected
  term drop-retrying "$theme" "[$open, $late]" --demo-card retrying
  term drop-rejoining "$theme" "[$open, $late]" --demo-card rejoining
  term drop-ask "$theme" "[$open, $late]" --demo-card disconnected
  size=1280x800
  term drop-off "$theme" "[$off]" --demo-card disconnected --demo-welcome
  size=480x270
done

du -cb "$out"/*.png | tail -1
