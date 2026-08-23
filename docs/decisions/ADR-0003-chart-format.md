# ADR-0003: Versioned JSON Chart Format

- **Status**: accepted
- **Date**: 2026-08-23

## Context

BeatByte needs an on-disk chart format that is human-inspectable,
diff-friendly, editor-friendly, safely parseable as untrusted input, and
able to evolve (tempo maps, new note types) without breaking old files.

## Decision

- **JSON** with an explicit `format_version` integer, implemented with
  serde in `beatbyte-chart`. Specification: `docs/chart-format/`.
- **Seconds, not ticks**: note times are `f64` seconds on the song
  timeline. Beat/tick views are derived through the domain `TempoMap`
  when needed (sustain scoring, editor grid).
- **Per-lane notes in the file, chord events in the domain**: the file
  stores flat `{time, lane, len, hopo}` entries; the loader groups notes
  within 5 ms into chord events. This keeps the file trivial to generate
  and edit while gameplay gets the unit it judges.
- **Tolerant reader, strict validator**: unknown fields are accepted
  (forward compatibility); semantic validation collects *all* issues
  with locations and severities instead of failing fast.

## Alternatives considered

- **Tick-based times** (à la MIDI/.chart): more precise for editors
  under tempo changes, but forces every consumer through tempo math and
  couples the file to a resolution constant. Our generator and gameplay
  are time-based; seconds keep the pipeline simple. A future version can
  add an optional tick view if the editor demands it.
- **Binary format**: faster to parse, but opaque to users and hostile to
  community tooling; parsing speed is irrelevant at our sizes.
- **Reusing an existing community format**: legally and technically
  murky (formats tied to specific games); an original format avoids both
  problems and can still gain importers later.

## Consequences

- Charts are treated as untrusted input everywhere: version gate,
  numeric ranges, note-count and file-size caps, path-traversal checks
  on the audio reference (including Windows drive letters on Unix).
- The 5 ms chord-grouping epsilon is part of the format contract and
  documented; generators should emit exactly equal times for chords.
