#!/usr/bin/env bash
# Render the README mockups from raw game screenshots.
#
#   tools/mockups/render.sh            # every mockup into docs/media/
#
# The raw frames come from the game's own screenshot harness
# (BEATBYTE_SHOT_DIR / BEATBYTE_SHOT_STATE / BEATBYTE_SHOT_TIMES); the
# list below names which frame becomes which picture. The layout is
# tools/mockups/mockup.html, drawn in CSS and rendered by headless
# Chrome — no device photo, no third-party artwork.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
raw="$root/tools/mockups/raw"
out="${OUT:-$root/docs/media}"
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
page="$root/tools/mockups/mockup.html"

render() { # <output.png> <hash>
    local hash
    hash=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1], safe="|/._-"))' "$2")
    "$chrome" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
        --allow-file-access-from-files --window-size=1600,900 \
        --virtual-time-budget=2000 --screenshot="$1" "file://$page#$hash" >/dev/null 2>&1
    echo "wrote ${1#"$root"/}"
}

# Mockups name raw frames relative to mockup.html.
r=raw
[[ -d $raw ]] || { echo "no raw frames in $raw - shoot them first (see README)" >&2; exit 1; }
need() { [[ -f "$raw/$1" ]] || { echo "missing raw frame $1" >&2; exit 1; }; }

while IFS='|' read -r name layout a b c d e; do
    [[ -z $name || $name == \#* ]] && continue
    case $layout in
        hero) need "$a"; need "$b"; need "$c"
              render "$out/$name" "hero|$r/$a|$r/$b|$r/$c" ;;
        device) need "$a"
              render "$out/$name" "device|$r/$a|$b|$c" ;;
        duo) need "$a"; need "$b"
              render "$out/$name" "duo|$r/$a|$r/$b|$c|$d|$e" ;;
    esac
done < "$root/tools/mockups/shots.txt"
