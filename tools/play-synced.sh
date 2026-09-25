#!/bin/sh
# Play with the career synced before and after (ADR-0021):
#   tools/play-synced.sh [game arguments]
# A sync that fails (hub asleep, no network) is said and skipped — the
# game is always playable offline, and the next sync catches up.
# The hub is the one `beatbyte-cli sync --hub …` remembered.
# Inside BeatByte.app this script is in Contents/Resources and the two
# binaries in Contents/MacOS; in a checkout they are in target/release.
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ -x "$HERE/../MacOS/beatbyte-cli" ]; then BIN="$HERE/../MacOS"; else BIN="$HERE/../target/release"; fi
CLI="${BEATBYTE_CLI:-$BIN/beatbyte-cli}"
GAME="${BEATBYTE_GAME:-$BIN/beatbyte}"
"$CLI" sync || echo "play-synced: sync before failed — playing offline"
"$GAME" "$@"
status=$?
"$CLI" sync || echo "play-synced: sync after failed — the next one catches up"
exit $status
