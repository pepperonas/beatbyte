#!/usr/bin/env bash
# Did a change leave the screens alone? Photograph them before, again
# after, and compare.
#
#   tools/shot-check.sh record  <dir>   # reference pictures, BEFORE the change
#   tools/shot-check.sh compare <dir>   # new pictures, compared with <dir>
#
# Every picture is taken by the game's own screenshot harness
# (BEATBYTE_SHOT_STATE / BEATBYTE_SHOT_ROW / BEATBYTE_SHOT_DIR) in a
# throwaway HOME, so no setting or song of this machine shows up in
# it, and in a pinned 1280x800 window. `compare` exits non-zero when a
# picture changed; LOOK at the pair before deciding whether the change
# was meant.
#
# SHOTS overrides the list: space-separated `screen[:row]` items, the
# screen names BEATBYTE_SHOT_STATE takes.
#
# Before believing a result (docs/development/harness.md):
#   - the screen must be unlocked — a locked screen photographs black;
#   - the window must open on a display that renders; a picture of a
#     different SIZE than the reference means it opened on the other
#     display (Retina gives 2560x1600, a plain monitor 1280x800).
set -euo pipefail

mode=${1:-}
dir=${2:-}
if [[ -z $mode || -z $dir || ( $mode != record && $mode != compare ) ]]; then
    echo "usage: $0 record|compare <dir>" >&2
    exit 2
fi

root=$(cd "$(dirname "$0")/.." && pwd)
shots=${SHOTS:-"settings:0 settings:13 settings:39 menu"}

cargo build --quiet --manifest-path "$root/Cargo.toml" -p beatbyte -p beatbyte-cli

if [[ $mode == record ]]; then
    out=$dir
else
    out=$(mktemp -d "${TMPDIR:-/tmp}/beatbyte-shots.XXXXXX")
fi
mkdir -p "$out"

for item in $shots; do
    screen=${item%%:*}
    row=""
    [[ $item == *:* ]] && row=${item#*:}
    name="$screen${row:+-row$row}.png"
    home=$(mktemp -d "${TMPDIR:-/tmp}/beatbyte-home.XXXXXX")
    shot=$(mktemp -d "${TMPDIR:-/tmp}/beatbyte-shot.XXXXXX")
    (
        cd "$root"
        # `env`, because an assignment produced by an expansion is
        # not an assignment: `${row:+X=1} cmd` would run `X=1`.
        env HOME="$home" BEATBYTE_WINDOW=1280x800 BEATBYTE_SHOT_STATE="$screen" \
            ${row:+BEATBYTE_SHOT_ROW="$row"} BEATBYTE_SHOT_DIR="$shot" \
            target/debug/beatbyte >"$shot/log" 2>&1
    ) || true
    picture=$(find "$shot" -name '*.png' | head -1)
    if [[ -z $picture ]]; then
        echo "no picture of $item (log: $shot/log)" >&2
        exit 1
    fi
    cp "$picture" "$out/$name"
    rm -rf "$home" "$shot"
    echo "took $name"
done

if [[ $mode == compare ]]; then
    status=0
    "$root/target/debug/beatbyte-cli" shots compare "$dir" "$out" || status=$?
    echo "new pictures: $out"
    exit $status
fi
echo "reference pictures: $out"
