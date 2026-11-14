#!/bin/bash
# SSHeesh measuring kit for macOS and Linux.
#
# Run it INSIDE SSHeesh's own terminal ("This computer", not a server, not tmux):
#     bash measure.sh            every hands-off measurement, about 4 minutes
#     bash measure.sh ctrlc      a long flood: press Ctrl+C once while it runs
#     bash measure.sh idle       only the 60 s idle measurement (for the 4-terminal run)
#     bash measure.sh notify     a test notification after 8 s
#     bash measure.sh clean      delete the test files this kit made
# Single parts: info, memory, cat, cyrillic.
#
# It measures the app whose terminal it runs in (0.10.x or the 1.0 prototype) and writes a
# short text file with numbers only: no computer name, user name, address or path.
#
# Options: --pid N (the app's process, if it is not found), --seconds N (idle length),
#          --mb N (flood size; 40 by default, because the terminal of 0.10.x throws output
#          away once more than 50 MB is waiting to be shown), --runs N.
# Works with the bash 3.2 that macOS ships. Needs nothing installed.

KIT_VERSION=1
set -u
export LC_NUMERIC=C

CMD=all
APP_PID=0
IDLE_SECONDS=60
FLOOD_MB=40
RUNS=3
while [ $# -gt 0 ]; do
  case "$1" in
    --pid) APP_PID=${2:-0}; shift ;;
    --seconds) IDLE_SECONDS=${2:-60}; shift ;;
    --mb) FLOOD_MB=${2:-40}; shift ;;
    --runs) RUNS=${2:-3}; shift ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "unknown option $1 (try --help)"; exit 2 ;;
    *) CMD=$1 ;;
  esac
  shift
done

HERE=$(cd "$(dirname "$0")" && pwd)
OS=$(uname -s)
case "$OS" in Darwin) OSNAME=macos ;; Linux) OSNAME=linux ;; *) echo "This script is for macOS and Linux. On Windows use measure.ps1."; exit 2 ;; esac
WORK="${TMPDIR:-/tmp}"; WORK="${WORK%/}/ssheesh-kit"
mkdir -p "$WORK" || { echo "cannot create $WORK"; exit 1; }

# ---------------------------------------------------------------- small tools

calc() { LC_ALL=C awk "BEGIN { printf \"%.3f\", $1 }"; }
calc1() { LC_ALL=C awk "BEGIN { printf \"%.1f\", $1 }"; }

# Seconds since 1970 with fractions.
if [ -n "${EPOCHREALTIME:-}" ]; then
  now() { local t=$EPOCHREALTIME; echo "${t/,/.}"; }
elif command -v perl >/dev/null 2>&1; then
  now() { perl -MTime::HiRes=time -e 'printf("%.3f\n", time)'; }
elif command -v python3 >/dev/null 2>&1; then
  now() { python3 -c 'import time; print("%.3f" % time.time())'; }
else
  now() { date +%s; }
fi

# "[[dd-]hh:]mm:ss[.cc]" (what ps prints for elapsed and CPU time) to seconds.
clock_seconds() {
  LC_ALL=C awk -v t="$1" 'BEGIN {
    d = 0; if (split(t, a, "-") == 2) { d = a[1]; t = a[2] }
    n = split(t, b, ":"); s = 0; for (i = 1; i <= n; i++) s = s * 60 + b[i];
    printf "%.2f", d * 86400 + s }'
}

# "85 MB", "512K", "1.2G+" to megabytes.
to_mb() {
  LC_ALL=C awk -v v="$1" -v u="$2" 'BEGIN {
    if (u == "") { u = v; sub(/^[0-9.]+/, "", u); sub(/[^0-9.].*$/, "", v) }
    u = toupper(substr(u, 1, 1)); m = v + 0;
    if (u == "K") m = m / 1024; else if (u == "G") m = m * 1024; else if (u == "B") m = m / 1048576;
    printf "%.1f", m }'
}

median() { sort -n | awk '{ a[NR] = $1 } END { if (NR == 0) print "none"; else if (NR % 2) print a[(NR + 1) / 2]; else printf "%.3f\n", (a[NR / 2] + a[NR / 2 + 1]) / 2 }'; }

RESULT=""
say() { echo "$*"; [ -n "$RESULT" ] && echo "$*" >> "$RESULT"; }
note() { echo "$*"; }

# ---------------------------------------------------------------- which app, which processes

pname() { local c; c=$(ps -o comm= -p "$1" 2>/dev/null); c=${c##*/}; c=${c# }; echo "${c// /_}"; }
ppid_of() { ps -o ppid= -p "$1" 2>/dev/null | tr -d ' '; }

find_app() {
  local pid=$$ name lower
  while [ -n "$pid" ] && [ "$pid" -gt 1 ]; do
    name=$(pname "$pid"); lower=$(echo "$name" | tr 'A-Z' 'a-z')
    case "$lower" in *ssheesh*) APP_PID=$pid; return 0 ;; esac
    pid=$(ppid_of "$pid")
  done
  return 1
}

if [ "$CMD" != clean ] && [ "$CMD" != notify ]; then
  if [ "$APP_PID" = 0 ] && ! find_app; then
    echo "I am not running inside SSHeesh."
    echo "Open a terminal for \"This computer\" in SSHeesh (not a server) and run me there."
    exit 1
  fi
  APP_NAME=$(pname "$APP_PID")
  [ -n "$APP_NAME" ] || { echo "no process $APP_PID"; exit 1; }
  case "$(echo "$APP_NAME" | tr 'A-Z' 'a-z')" in *proto*) APP_KIND=prototype ;; *) APP_KIND=current ;; esac
fi

# Prints "pid name" for every process of the app: the program itself, and for 0.10.x its web
# view's processes. Shells and ssh are never counted.
app_pids() {
  echo "$APP_PID $APP_NAME"
  [ "$APP_KIND" = prototype ] && return
  if [ "$OS" = Darwin ]; then
    # The web view's processes are started by the system, not by the app, so they are
    # recognised by name and by having started after the app did.
    local root_age pid age comm
    root_age=$(clock_seconds "$(ps -o etime= -p "$APP_PID" | tr -d ' ')")
    ps -U "$(id -u)" -o pid=,etime=,comm= | while read -r pid age comm; do
      case "$comm" in
        *com.apple.WebKit.*)
          if LC_ALL=C awk -v a="$(clock_seconds "$age")" -v r="$root_age" 'BEGIN { exit !(a <= r + 3) }'; then
            comm=${comm##*/}; echo "$pid ${comm// /_}"
          fi ;;
      esac
    done
  else
    ps -eo pid=,ppid=,comm= | LC_ALL=C awk -v root="$APP_PID" '
      { pp[$1] = $2; nm[$1] = $3 }
      END { for (p in pp) { q = p; while (q in pp && q != root && q > 1) q = pp[q];
                            if (q == root && p != root && nm[p] ~ /^WebKit/) print p, nm[p] } }'
  fi
}

cpu_of() {
  if [ "$OS" = Linux ]; then
    [ -r "/proc/$1/stat" ] || { echo 0; return; }
    sed 's/.*) //' "/proc/$1/stat" | LC_ALL=C awk -v hz="$(getconf CLK_TCK)" '{ printf "%.2f", ($12 + $13) / hz }'
  else
    clock_seconds "$(ps -o cputime= -p "$1" 2>/dev/null | tr -d ' ')"
  fi
}

MEM_METHOD=""
# Prints "main_mb rss_mb": the memory that is this process's own, and what it has resident.
mem_of() {
  local rss main="" line
  rss=$(ps -o rss= -p "$1" 2>/dev/null | tr -d ' '); rss=$(to_mb "${rss:-0}" K)
  if [ "$OS" = Linux ]; then
    if [ -r "/proc/$1/smaps_rollup" ]; then
      main=$(LC_ALL=C awk '/^Pss:/ { printf "%.1f", $2 / 1024 }' "/proc/$1/smaps_rollup" 2>/dev/null)
      MEM_METHOD=pss
    fi
  else
    if command -v footprint >/dev/null 2>&1; then
      line=$(footprint "$1" 2>/dev/null | grep -m1 'Footprint:')
      if [ -n "$line" ]; then
        # "... Footprint: 85 MB (16384 bytes per page)"
        line=$(echo "$line" | sed 's/.*Footprint: *//')
        main=$(to_mb "$(echo "$line" | awk '{ print $1 }')" "$(echo "$line" | awk '{ print $2 }')"); MEM_METHOD=footprint
      fi
    fi
    if [ -z "$main" ]; then
      line=$(top -l 1 -pid "$1" -stats mem 2>/dev/null | tail -1 | tr -d ' ')
      case "$line" in [0-9]*) main=$(to_mb "$line" ""); MEM_METHOD=top ;; esac
    fi
  fi
  [ -n "$main" ] || { main=$rss; MEM_METHOD=rss; }
  echo "$main $rss"
}

# One line per process: "pid name cpu_seconds main_mb rss_mb".
snapshot() {
  app_pids | while read -r pid name; do
    echo "$pid $name $(cpu_of "$pid") $(mem_of "$pid")"
  done
}
# mem_of runs in a subshell above, so find the method once here.
mem_method() { mem_of "$APP_PID" >/dev/null; echo "$MEM_METHOD"; }
sum_col() { LC_ALL=C awk -v c="$1" '{ s += $c } END { printf "%.1f", s }'; }

# ---------------------------------------------------------------- the terminal as a clock

# The terminal answers "where is the cursor" only after it has read everything printed
# before the question. So the answer's arrival is the moment the screen has caught up.
TTY_SAVED=""
tty_quiet() { TTY_SAVED=$(stty -g < /dev/tty 2>/dev/null); stty -echo -icanon min 1 time 0 < /dev/tty 2>/dev/null; }
tty_back() { [ -n "$TTY_SAVED" ] && stty "$TTY_SAVED" < /dev/tty 2>/dev/null; TTY_SAVED=""; }
trap 'tty_back' EXIT

caught_up() {
  local reply=""
  printf '\033[6n' > /dev/tty
  IFS= read -r -s -d R -t "${1:-120}" reply < /dev/tty
}

# ---------------------------------------------------------------- test files (made-up text only)

make_flood() {
  local file="$WORK/stream-$FLOOD_MB.txt" block="$WORK/block.txt" want size n i
  want=$((FLOOD_MB * 1048576))
  if [ -f "$file" ] && [ "$(wc -c < "$file" | tr -d ' ')" -ge "$want" ]; then FLOOD_FILE=$file; return; fi
  note "Making a $FLOOD_MB MB test file (once)..."
  i=0
  while [ $i -lt 7000 ]; do
    case $((i % 10)) in
      3) printf '\033[90m2026-11-14 10:%02d:%02d\033[0m \033[33mWARN\033[0m  web-7731 queue is %d items deep, worker %d is slow \033[35m[retry %d]\033[0m\n' $((i / 60 % 60)) $((i % 60)) $((i * 7 % 900)) $((i % 8)) $((i % 3)) ;;
      6) printf '\033[90m2026-11-14 10:%02d:%02d\033[0m \033[32mINFO\033[0m  db-7732 日本語のログ行 %05d 完了しました 🚀 ✅ 表示幅のテスト\n' $((i / 60 % 60)) $((i % 60)) $i ;;
      9) printf '\033[90m2026-11-14 10:%02d:%02d\033[0m \033[1;31mERROR\033[0m web-7731 upstream 203.0.113.%d timed out after %d ms \033[4mGET /api/items/%d\033[0m\n' $((i / 60 % 60)) $((i % 60)) $((i % 250 + 1)) $((i * 13 % 5000)) $i ;;
      *) printf '\033[90m2026-11-14 10:%02d:%02d\033[0m \033[32mINFO\033[0m  web-7731 request %06d GET /api/items/%d 200 in %d ms \033[36m%d bytes\033[0m\n' $((i / 60 % 60)) $((i % 60)) $i $((i * 31 % 9973)) $((i * 17 % 400)) $((i * 113 % 65536)) ;;
    esac
    i=$((i + 1))
  done > "$block"
  size=$(wc -c < "$block" | tr -d ' '); n=$(( (want + size - 1) / size ))
  : > "$file"; i=0
  while [ $i -lt $n ]; do cat "$block" >> "$file"; i=$((i + 1)); done
  rm -f "$block"
  FLOOD_FILE=$file
}

# Two files with the same number of characters: plain Latin letters, and Russian letters.
make_letters() {
  LATIN_FILE="$WORK/latin-20.txt"; CYR_FILE="$WORK/cyrillic-20.txt"
  [ -f "$LATIN_FILE" ] && [ -f "$CYR_FILE" ] && return
  note "Making two letter files (once)..."
  local l c i
  l="The quick brown fox jumps over the lazy dog and runs back to the quiet river before the night falls."
  c="Съешь же ещё этих мягких французских булок да выпей чаю, пока тихая река не скрылась в вечерней мгле"
  for pair in "$LATIN_FILE|$l" "$CYR_FILE|$c"; do
    i=0; while [ $i -lt 400 ]; do echo "${pair#*|}"; i=$((i + 1)); done > "$WORK/b.txt"
    : > "${pair%%|*}"; i=0
    while [ $i -lt 500 ]; do cat "$WORK/b.txt" >> "${pair%%|*}"; i=$((i + 1)); done
  done
  rm -f "$WORK/b.txt"
}

# Prints a file RUNS times. Sets SHELL_S and SCREEN_S to the medians.
timed_cat() {
  local file=$1 label=$2 i t0 t1 t2 shell screen shells="" screens=""
  i=1
  while [ $i -le "$RUNS" ]; do
    tty_quiet
    t0=$(now); cat "$file"; t1=$(now)
    if caught_up 60; then t2=$(now); screen=$(calc "$t2 - $t0"); else screen=none; fi
    tty_back
    shell=$(calc "$t1 - $t0")
    printf '\033[0m\n'
    say "$label run $i: cat returned after $shell s, screen caught up after $screen s"
    shells="$shells $shell"
    if [ "$screen" = none ]; then
      # No answer in a minute: the terminal threw part of the output away. Do not wait again.
      say "$label: the terminal did not answer; no more runs"
      break
    fi
    screens="$screens $screen"
    sleep 2
    i=$((i + 1))
  done
  SHELL_S=$(echo $shells | tr ' ' '\n' | median)
  SCREEN_S=$(echo $screens | tr ' ' '\n' | median)
}

# ---------------------------------------------------------------- the parts

open_result() {
  mkdir -p "$HERE/results" 2>/dev/null && RESULT="$HERE/results/ssheesh-kit-$OSNAME-$APP_KIND-$(date +%Y%m%d-%H%M%S).txt"
  [ -n "$RESULT" ] && : > "$RESULT" 2>/dev/null || RESULT="$WORK/ssheesh-kit-$OSNAME-$APP_KIND-$(date +%Y%m%d-%H%M%S).txt"
  say "SSHeesh measuring kit $KIT_VERSION, part: $CMD"
  say "numbers only: no computer name, user name, address or path is written here"
}

part_info() {
  local ver="" exe size
  say "--- info"
  say "date: $(date '+%Y-%m-%d %H:%M')"
  if [ "$OS" = Darwin ]; then
    say "system: macOS $(sw_vers -productVersion 2>/dev/null) ($(uname -m))"
    say "cpu: $(sysctl -n machdep.cpu.brand_string 2>/dev/null), $(sysctl -n hw.ncpu 2>/dev/null) cores"
    say "memory_gb: $(calc1 "$(sysctl -n hw.memsize 2>/dev/null) / 1073741824")"
    system_profiler SPDisplaysDataType 2>/dev/null | grep -E 'Chipset Model|Resolution:|UI Looks like|Refresh Rate|Display Type' | sed 's/^ */display: /' | while read -r line; do say "$line"; done
    say "power: $(pmset -g batt 2>/dev/null | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
    exe=$(ps -o comm= -p "$APP_PID" 2>/dev/null)
    case "$exe" in *.app/Contents/MacOS/*) ver=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "${exe%/MacOS/*}/Info.plist" 2>/dev/null) ;; esac
  else
    say "system: $(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-Linux}") ($(uname -m)), session ${XDG_SESSION_TYPE:-unknown}"
    say "cpu: $(LC_ALL=C awk -F': ' '/^model name/ { print $2; exit }' /proc/cpuinfo), $(getconf _NPROCESSORS_ONLN) cores"
    say "memory_gb: $(LC_ALL=C awk '/^MemTotal/ { printf "%.1f", $2 / 1048576 }' /proc/meminfo)"
  fi
  say "app: $APP_NAME ${ver:-version unknown} ($APP_KIND)"
  size=$(stty size < /dev/tty 2>/dev/null)
  say "terminal: ${size#* } columns x ${size% *} rows"
  say "app processes: $(app_pids | awk '{ print $2 }' | sort | uniq -c | awk '{ printf "%s%s x%s", (NR > 1 ? ", " : ""), $2, $1 }')"
}

part_memory() {
  local s0 s1 m0 m1 r0 r1 i
  say "--- memory (10,100 lines of 120 characters)"
  s0=$(snapshot); m0=$(echo "$s0" | sum_col 4); r0=$(echo "$s0" | sum_col 5)
  tty_quiet
  i=1; while [ $i -le 10100 ]; do printf '%0120d\n' $i; i=$((i + 1)); done
  caught_up 30; tty_back
  sleep 3
  s1=$(snapshot); m1=$(echo "$s1" | sum_col 4); r1=$(echo "$s1" | sum_col 5)
  say "memory_method: $(mem_method)"
  say "memory_before_mb: $m0 (resident $r0)"
  say "memory_after_mb: $m1 (resident $r1)"
  say "memory_growth_mb: $(calc1 "$m1 - $m0")"
  echo "$s1" | while read -r pid name cpu main rss; do say "  process $name: $main MB (resident $rss)"; done
}

part_cat() {
  local size s0 s1
  make_flood
  size=$(wc -c < "$FLOOD_FILE" | tr -d ' ')
  say "--- cat of a $(calc1 "$size / 1048576") MB file"
  s0=$(snapshot | sum_col 3)
  timed_cat "$FLOOD_FILE" cat
  s1=$(snapshot | sum_col 3)
  say "cat_shell_s: $SHELL_S"
  say "cat_screen_s: $SCREEN_S"
  if [ "$SCREEN_S" != none ]; then
    say "cat_mb_per_s: $(calc1 "$size / 1048576 / $SCREEN_S")"
    say "cat_50_mb_would_take_s: $(calc "$SCREEN_S * 52428800 / $size")"
  fi
  say "cat_app_cpu_s_per_run: $(calc "($s1 - $s0) / $RUNS")"
  say "memory_after_cat_mb: $(snapshot | sum_col 4)"
}

part_cyrillic() {
  local latin
  make_letters
  say "--- Latin against Russian letters (20,200,000 characters each)"
  timed_cat "$LATIN_FILE" latin; latin=$SCREEN_S
  say "latin_screen_s: $latin"
  timed_cat "$CYR_FILE" cyrillic
  say "cyrillic_screen_s: $SCREEN_S"
  if [ "$latin" != none ] && [ "$SCREEN_S" != none ]; then
    say "cyrillic_slower_by: $(LC_ALL=C awk -v a="$latin" -v b="$SCREEN_S" 'BEGIN { printf "%.2f", b / a }') times"
  fi
}

part_idle() {
  local a b t0 t1 secs
  say "--- idle ($IDLE_SECONDS s)"
  note ""
  note ">>> Do not touch the keyboard or the mouse for $((IDLE_SECONDS + 10)) seconds."
  note ">>> Leave this window in front, with the cursor blinking. I will beep when done."
  sleep 8
  a=$(snapshot); t0=$(now)
  sleep "$IDLE_SECONDS"
  b=$(snapshot); t1=$(now)
  secs=$(calc "$t1 - $t0")
  printf '\a'
  say "idle_seconds: $secs"
  say "idle_cpu_percent_of_one_core: $( (echo "$a"; echo ---; echo "$b") | LC_ALL=C awk -v s="$secs" '
      $1 == "---" { second = 1; next }
      !second { c[$1] = $3; next }
      ($1 in c) { d += $3 - c[$1] }
      END { printf "%.2f", d / s * 100 }')"
  (echo "$a"; echo ---; echo "$b") | LC_ALL=C awk -v s="$secs" '
      $1 == "---" { second = 1; next }
      !second { c[$1] = $3; next }
      ($1 in c) { printf "  process %s: %.2f%%\n", $2, ($3 - c[$1]) / s * 100 }' | while IFS= read -r line; do say "$line"; done
  say "app processes: $(echo "$b" | wc -l | tr -d ' ')"
}

part_ctrlc() {
  local got="" t2
  make_flood
  say "--- Ctrl+C in a flood"
  note ""
  note ">>> Text will fly for a while. Press Ctrl+C ONCE when it does."
  sleep 4
  tty_quiet
  trap 'got=$(now)' INT
  cat "$FLOOD_FILE" "$FLOOD_FILE" "$FLOOD_FILE" "$FLOOD_FILE"
  trap - INT
  if caught_up 30; then t2=$(now); else t2=""; fi
  tty_back
  printf '\033[0m\n'
  if [ -z "$got" ]; then
    say "ctrl_c: not pressed before the flood ended"
  elif [ -z "$t2" ]; then
    say "ctrl_c_screen_kept_running_s: no answer from the terminal in 30 s (it threw output away)"
  else
    say "ctrl_c_screen_kept_running_s: $(calc "$t2 - $got")"
  fi
}

part_notify() {
  note "In 8 seconds this terminal asks for a notification."
  note "Switch to another program NOW, then click the notification when it appears."
  sleep 8
  printf '\033]9;SSHeesh kit test\007'
  note "Sent."
}

finish() {
  [ -n "$RESULT" ] || return
  note ""
  if [ "$OS" = Darwin ] && command -v pbcopy >/dev/null 2>&1; then
    pbcopy < "$RESULT" && note "The result is in the clipboard: paste it into the chat."
  fi
  note "The result is also saved in: $RESULT"
}

case "$CMD" in
  all) open_result; part_info; part_memory; part_cyrillic; part_cat; sleep 5; part_idle; finish ;;
  info) open_result; part_info; finish ;;
  memory) open_result; part_info; part_memory; finish ;;
  cat) open_result; part_info; part_cat; finish ;;
  cyrillic) open_result; part_info; part_cyrillic; finish ;;
  idle) open_result; part_info; part_idle; finish ;;
  ctrlc) open_result; part_info; part_ctrlc; finish ;;
  notify) part_notify ;;
  clean) rm -rf "$WORK"; echo "Deleted $WORK" ;;
  *) echo "unknown part $CMD (try --help)"; exit 2 ;;
esac
