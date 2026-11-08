#!/usr/bin/env bash
# Capture the README screenshots on a Linux desktop.
#
# Needs Xvfb, xdotool, ImageMagick (import/convert) and a window manager-free X
# display. The app is driven at 1440x900; the images land in screenshot/.
#
#   ./scripts/screenshots.sh [output-dir]
#
# Set FM1_GRID_BIN to an already-built binary; otherwise this builds release.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-"$ROOT/screenshot"}
cd "$ROOT"
BIN=${FM1_GRID_BIN:-"$ROOT/target/release/fm1-grid"}
W=1440
H=900
DISPLAY_NUM=${SCREENSHOT_DISPLAY:-:99}

for tool in Xvfb xdotool import convert; do
    command -v "$tool" >/dev/null || { echo "missing $tool" >&2; exit 1; }
done
if [[ ! -x $BIN ]]; then
    echo "==> building fm1-grid"
    cargo build --release -p fm1-grid
fi
mkdir -p "$OUT"

TMP=$(mktemp -d)
DATA=$TMP/data
LOG=$TMP/app.log
ALSA=$TMP/asound.conf
mkdir -p "$DATA/objects"
printf '%s\n' '{"sidebar_width":512,"tab":0}' >"$DATA/objects/session.json"
# A null output keeps the machine's real audio out of the way. The sequencer
# default lets MIDI setup succeed when /dev/snd/seq exists.
cat >"$ALSA" <<'CONF'
pcm.!default { type null }
seq.!default { type hw }
CONF
[[ -e /dev/snd/seq ]] || echo "warning: no /dev/snd/seq; MIDI screenshots may show an error" >&2

Xvfb "$DISPLAY_NUM" -screen 0 1700x1200x24 -nolisten tcp >"$TMP/xvfb.log" 2>&1 &
XVFB=$!
APP=
cleanup() {
    [[ -n ${APP:-} ]] && kill "$APP" 2>/dev/null || true
    kill "$XVFB" 2>/dev/null || true
    rm -rf "$TMP"
}
trap cleanup EXIT

sleep 2
export DISPLAY=$DISPLAY_NUM
export ALSA_CONFIG_PATH=$ALSA
export FM1_GRID_DATA_DIR=$DATA
"$BIN" >"$LOG" 2>&1 &
APP=$!
WID=
for _ in $(seq 1 60); do
    WID=$(xdotool search --name "Synth Firmware Zoo" 2>/dev/null | head -1 || true)
    [[ -n $WID ]] && break
    sleep 0.5
done
if [[ -z $WID ]]; then
    cat "$LOG" >&2
    echo "fm1-grid did not open" >&2
    exit 1
fi
xdotool windowmove "$WID" 0 0
xdotool windowsize "$WID" "$W" "$H"
sleep 3

click() { xdotool mousemove "$1" "$2"; sleep 0.25; xdotool click 1; sleep "${3:-0.5}"; }
shot() { import -window "$WID" "$OUT/$1.png"; }

# Emulator: power all four panes, then select the first.
for xy in "300 180" "560 180" "300 450" "560 450"; do
    set -- $xy
    click "$1" "$2" 0.4
    click 860 60 3
done
click 300 180 0.5
sleep 8
shot emulator

# Hold a white key for the pressed-state detail.
xdotool mousemove 265 800
sleep 0.3
xdotool mousedown 1
sleep 1
import -window "$WID" "$TMP/pressed.png"
xdotool mouseup 1
convert "$TMP/pressed.png" -crop 1050x130+220+730 +repage -scale 200% "$OUT/pressed-key.png"

# Library.
click 22 148 8
shot library

# Devices and its MIDI Graph section.
click 22 104 1
shot devices
click 785 58 2
click 1382 132 0.5
click 1350 166 2
shot midi-graph

# Settings (gear at the bottom of the rail).
click 22 835 2
shot settings

echo "==> wrote screenshots to $OUT"
