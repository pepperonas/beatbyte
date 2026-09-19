#!/bin/bash
# Build the [GS] twin of every song folder that has none.
#
#   tools/guitar-study.sh [<songs/imported>] [<scratch dir>]
#
# Per song: decode the audio to the timeline the analysis uses
# (`beatbyte-cli decode`), separate the "other" stem with demucs on that
# WAV (never on the original file — a different decoder is a different
# timeline), then `beatbyte-cli study`. Twins that exist are skipped, so
# the run is resumable; a song the study refuses (too little tonal
# evidence) is listed at the end and keeps its original as the only
# version. The originals are never written to.
set -u
ROOT="${1:-songs/imported}"
SCRATCH="${2:-${TMPDIR:-/tmp}/guitar-study}"
CLI="${BEATBYTE_CLI:-target/release/beatbyte-cli}"
DEVICE="${DEMUCS_DEVICE:-mps}"
mkdir -p "$SCRATCH"
[ -x "$CLI" ] || { echo "no CLI at $CLI (cargo build --release -p beatbyte-cli)"; exit 2; }
command -v demucs >/dev/null || { echo "demucs not on PATH"; exit 2; }
done_n=0; skip_n=0; refused=(); failed=()
for d in "$ROOT"/*/; do
  d="${d%/}"; name="$(basename "$d")"
  case "$name" in guitar-study-*) continue;; esac
  [ -d "$ROOT/guitar-study-$name" ] && { skip_n=$((skip_n+1)); continue; }
  audio="$(find "$d" -maxdepth 1 \( -iname '*.m4a' -o -iname '*.mp3' -o -iname '*.wav' -o -iname '*.ogg' -o -iname '*.flac' \) | head -1)"
  [ -n "$audio" ] || { failed+=("$name: no audio"); continue; }
  work="$SCRATCH/$name"; mkdir -p "$work"
  echo "=== $name  ($(date +%H:%M))"
  if [ ! -s "$work/stems/htdemucs/mix/other.wav" ]; then
    "$CLI" decode "$audio" --out "$work/mix.wav" >"$work/decode.log" 2>&1 || { failed+=("$name: decode"); continue; }
    # MPS first (minutes), CPU as the fallback (much longer).
    demucs --two-stems=other -n htdemucs -d "$DEVICE" -o "$work/stems" "$work/mix.wav" >"$work/demucs.log" 2>&1 \
      || demucs --two-stems=other -n htdemucs -d cpu -o "$work/stems" "$work/mix.wav" >>"$work/demucs.log" 2>&1 \
      || { failed+=("$name: demucs"); continue; }
  fi
  "$CLI" study "$d" --lead "$work/stems/htdemucs/mix/other.wav" >"$work/study.log" 2>&1; rc=$?
  tail -1 "$work/study.log"
  case $rc in 0) done_n=$((done_n+1));; 3) refused+=("$name");; *) failed+=("$name: study rc=$rc");; esac
done
echo
# `${arr[@]+"${arr[@]}"}`: macOS ships bash 3.2, where an EMPTY array
# is an unbound variable under `set -u` — the first run died on this
# line and swallowed its own list of failures.
echo "written $done_n, already there $skip_n, refused ${#refused[@]}, failed ${#failed[@]}"
for r in ${refused[@]+"${refused[@]}"}; do echo "  refused: $r"; done
for f in ${failed[@]+"${failed[@]}"}; do echo "  failed:  $f"; done
