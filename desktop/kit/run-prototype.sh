#!/bin/bash
# Starts the SSHeesh 1.0 terminal prototype with its own meter switched on (macOS and Linux).
#
#     bash run-prototype.sh [--hud] [path to ssheesh-proto]
#
# Without a path it looks for ssheesh-proto beside this folder. The meter writes numbers only
# (times, frame counts, memory) into results/proto-meter-<time>.jsonl; never text you typed,
# server names or paths. --hud also shows the numbers in the title row; leave it off for the
# idle measurement, because the row redraws once a second.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
HUD=""
PROG=""
for a in "$@"; do
  case "$a" in --hud) HUD=--hud ;; *) PROG=$a ;; esac
done
if [ -z "$PROG" ]; then
  for p in "$HERE/ssheesh-proto" "$HERE/../ssheesh-proto" "$HERE/../MacOS/ssheesh-proto"; do
    [ -x "$p" ] && { PROG=$p; break; }
  done
fi
if [ -z "$PROG" ] || [ ! -x "$PROG" ]; then
  echo "I cannot find ssheesh-proto. Give me its path:"
  echo "    bash run-prototype.sh /path/to/ssheesh-proto"
  exit 1
fi
mkdir -p "$HERE/results" || exit 1
OUT="$HERE/results/proto-meter-$(date +%Y%m%d-%H%M%S).jsonl"
echo "Meter file: $OUT"
echo "Close the prototype's window when you are done; then send that file."
exec "$PROG" --local $HUD --metrics "$OUT"
