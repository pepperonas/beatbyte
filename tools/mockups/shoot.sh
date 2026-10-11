#!/usr/bin/env bash
# Shoot the raw frames the README mockups are made from.
#
#   tools/mockups/shoot.sh "<song needle>"     # e.g. '"Heroes"'
#
# Runs the release binary (build it with --features ml first) through
# the screenshot harness and copies the chosen frames into raw/, which
# render.sh then lays out. raw/ is not committed: the frames are
# re-shot from the current build whenever the screens change.
#
# Before running: the screen must be UNLOCKED (a locked screen makes
# every capture black and the run still passes), lyrics OFF in the
# settings (the README never shows a song's words), and no game open
# (it rewrites settings.json when it exits).
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
song=${1:?"name a song from your library, e.g. '\"Heroes\"'"}
bin="$root/target/release/beatbyte"
raw="$root/tools/mockups/raw"
work=$(mktemp -d)
trap 'kill "${raiser:-}" 2>/dev/null || true; rm -rf "$work"' EXIT
[[ -x $bin ]] || { echo "build first: cargo build --release -p beatbyte --features ml" >&2; exit 1; }
if python3 -c 'import Quartz,sys; sys.exit(0 if dict(Quartz.CGSessionCopyCurrentDictionary()).get("CGSSessionScreenIsLocked") else 1)' 2>/dev/null; then
    echo "the screen is locked - every frame would be black" >&2; exit 1
fi
mkdir -p "$raw"

shoot() { # <dir> <env...>
    local dir="$work/$1"; shift
    mkdir -p "$dir"
    (cd "$root" && env BEATBYTE_WINDOW=1600x900 BEATBYTE_AUTOPILOT_MUTE=1 BEATBYTE_SHOT_DIR="$dir" "$@" \
        caffeinate -disu -t 900 "$bin" >"$dir.log" 2>&1)
}

shoot browser BEATBYTE_SHOT_STATE=songselect BEATBYTE_SHOT_ROW=3
shoot actions BEATBYTE_SHOT_STATE=songselect BEATBYTE_SHOT_ACTIONS=1 BEATBYTE_SHOT_ROW=3
shoot help    BEATBYTE_SHOT_STATE=songselect BEATBYTE_SHOT_HELP=1
shoot nomatch BEATBYTE_SHOT_STATE=songselect "BEATBYTE_SHOT_SEARCH=toto africa live"
# An occluded window hands back a stale frame: keep it in front.
( while sleep 2.5; do osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "beatbyte") to true' >/dev/null 2>&1; done ) &
raiser=$!
shoot play BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_DIFFICULTY=expert "BEATBYTE_AUTOPILOT_SONG=$song"

cp "$work/browser/beatbyte-songselect.png" "$raw/browser.png"
cp "$work/actions/beatbyte-songselect.png" "$raw/actions.png"
cp "$work/help/beatbyte-songselect.png" "$raw/help.png"
cp "$work/nomatch/beatbyte-songselect.png" "$raw/nomatch.png"
cp "$work/play/beatbyte-gameplay-hype.png" "$raw/gameplay.png"
cp "$work/play/beatbyte-gameplay-phrase.png" "$raw/starpower.png"
cp "$work/play/beatbyte-gameplay-yourock.png" "$raw/finale.png"
cp "$work/play/beatbyte-results.png" "$raw/results.png"

# A black or near-black frame is a failed capture, not a dark screen.
for f in "$raw"/*.png; do
    python3 - "$f" <<'PY'
import sys
from PIL import Image, ImageStat
luma = ImageStat.Stat(Image.open(sys.argv[1]).convert("L")).mean[0]
print(f"{sys.argv[1].rsplit('/', 1)[-1]:14} luma {luma:5.1f}")
sys.exit(1 if luma < 5 else 0)
PY
done
echo "now: tools/mockups/render.sh"
