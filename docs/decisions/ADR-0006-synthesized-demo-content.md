# ADR-0006: Synthesized Demo Content, No Binary Audio in Git

- **Status**: amended 2026-09-05 — the synthesized songs no longer
  ship *in the game*; they remain the test fixtures (see
  [Amendment](#amendment-2026-09-05-the-songs-leave-the-library))
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

## Amendment (2026-09-05): the songs leave the library

The premise in *Context* — "a rhythm game without a song is a black
screen" — turned out to be the weaker half of the argument once the
game had a working import path, and the songs themselves gave the
decisive counter-argument: **they are instrumentals, and one of them
carried hand-written karaoke lyrics.** A synthesized chiptune track
with no voice on it was singing along. That is not a small blemish on
a demo; it is the game demonstrating a feature by lying about the
audio.

So the two synthesized tracks no longer appear in the game
(`beatbyte-game::boot` builds none, and the bundled
`circuit-breaker.lrc` is deleted). What remains unchanged:

- `beatbyte-audio::demo` still renders both tracks. They are the
  deterministic fixture the analysis and charting regression tests
  are built on (`apps/beatbyte/tests/rock_is_unchanged.rs`,
  `docs/audio-eval-baseline.md`) — a known-BPM signal is worth more
  as a test input than as a playlist entry.
- `beatbyte-cli demo` still writes them to disk for anyone who wants
  to hear or chart them.
- No audio binaries in git, as before.

The cost is accepted knowingly: **a fresh clone has nothing to play.**
The browser therefore states its own emptiness ("no songs yet — drag
an audio file onto the window") instead of showing a blank panel, and
the README says so before the download link.
