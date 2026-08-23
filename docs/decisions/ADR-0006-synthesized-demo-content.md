# ADR-0006: Synthesized Demo Content, No Binary Audio in Git

- **Status**: accepted
- **Date**: 2026-08-23

## Context

A rhythm game without a song is a black screen. BeatByte needs bundled
music that is (a) legally distributable under the project's licensing,
(b) available on first launch without downloads, and (c) not megabytes
of binary blobs bloating a source repository forever.

## Decision

The bundled demo track **"Circuit Breaker" by The Null Pointers** is an
original composition *rendered by code* (`beatbyte-audio::demo`):
deterministic synthesis of kick/snare/hats/bass/lead over an original
progression, chiptune-flavored to match the 8-bit identity.

- The **game** renders it in a background task at boot and plays it
  from memory — no files involved, first launch always works.
- The **CLI** (`beatbyte-cli demo`) writes it as a WAV (hand-rolled
  16-bit PCM writer — 30 lines beat a dependency) and runs the real
  analysis → generation pipeline on it, producing an on-disk example
  song + chart.
- The repository contains **no audio binaries**; `songs/` holds only
  user/generated content.

## Consequences

- The demo doubles as an end-to-end test: the analyzer must find the
  demo's own BPM (pinned by a unit test), and the generator charts it
  like any user song.
- Rendering costs ~1–2 s of CPU at boot, hidden behind the boot screen.
- Anyone can regenerate the demo assets from source alone — the repo
  stays honest about "all assets original or generated".
