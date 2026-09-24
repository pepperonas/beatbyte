# ADR-0020 — Classic rules travel with the chart; chords only from heard polyphony

**Status: Accepted** (2026-09-25, the classic programme K2–K5 of
`docs/ROADMAP.md`: a third chart twin, `[CL]`, built from the measured
rules of the early guitar games, one ingredient at a time and each
decided by the user's blind test)

## Context

The classic programme changes a chart one variable at a time so that a
person comparing two versions (`T` in the browser) answers a question
about exactly one thing. K1 only re-flagged hammer-ons; the four
ingredients after it are of three different kinds, and each kind forced
a decision about where it lives.

1. **K2, the strum grace**, is not about notes at all. It is a rule of
   the judge: a strum under the wrong fret waits about 60 ms for the
   fret (the engine's own number in the early games' successors).
2. **K3 and K4**, the classic Hard and Medium, change *which notes* a
   level holds — the one thing the earlier ingredients promised never
   to touch.
3. **K5, chords**, needs to know which notes the recording strikes
   together. Nothing in BeatByte could say that: the analyzer and the
   pitch tracker are monophonic, and every chord ever charted was placed
   from loudness.

## Decision

**1. A judgment rule is part of the chart.** `rules.strum_grace_ms` is
an optional field of the chart file (0–100, validated), converted into
`Track::with_strum_grace`, and part of `chart_hash`. An empty rule set
is serialized as nothing, so a chart that gains one keeps its identity.

**2. The ladder derives, never regenerates.** A classic Hard is the
chart's own Expert with notes taken away; a classic Medium is its Hard
with notes taken away and five frets folded to four. Nothing is moved
in time and nothing is added. What goes first is a pure score
(crowding, metrical position, fret movement, chords and sustains
protected), recomputed after every deletion.

**3. Chords come from a transcription, or not at all.** A new crate,
`beatbyte-poly`, runs Spotify's Basic Pitch (Apache-2.0) through the
existing pinned runtime (ADR-0013) over the separated `other` stem
(ADR-0014's local Demucs) and writes the notes it heard beside the
audio as `<audio>.poly.json`. The `chords` ingredient reads that file
and writes a chord only where two or more pitch classes were struck
within 60 ms, overtones of a lower note excluded. Without the file it
does nothing and says so.

## Alternatives

- **The strum grace as a setting** (like tap mode). Simpler, and a
  player could choose it. Rejected: a setting cannot be blind-tested by
  `T`, which compares chart versions; and a run recorded under one
  judge would be indistinguishable in the telemetry from a run under
  the other, because the chart hash would not change. Carried by the
  chart, the rule is evidence the store already keys on.
- **The ladder as new generator profiles** (`DifficultyProfile` for
  Hard/Medium tuned to the early games' numbers). Rejected: the
  generator places notes from the analysis, so a new profile changes
  timing, lanes and density at once — three variables in one blind
  test — and would need the audio. Deriving from the chart's own
  Expert changes only which notes are there, and needs nothing.
- **Chords from loudness or onset strength** (what the generator does).
  Rejected by the research itself: a chord invented from a monophonic
  reading is a guess, and the roadmap parked K5 for exactly that
  reason. **Chords from the mix** through the same model: kept as the
  CLI's `--mix` escape hatch only — Basic Pitch hears one instrument
  best, and on a mix the bass, keys and voice read as chord tones.
- **Re-hosting Basic Pitch as a release asset** like the other four
  models. Deferred: that is a publish, the maintainer's call. The
  registry pins the upstream file by commit, size and SHA-256, which
  fixes the bytes exactly as tightly.

## Consequences

- A chart can now play differently from another with the same notes.
  Every reader of charts that builds a `Track` goes through
  `to_track`, so the rule cannot be lost on the way; a test pins that.
- The ladder ingredients break the earlier "flags and nothing else"
  promise *on purpose* — which notes a level holds is their variable —
  but each still changes exactly one level and leaves the others byte
  for byte (pinned). The per-note analysis sidecar is carried by
  MOMENT rather than by position, because a derived level strikes only
  at moments its parent struck.
- The ladder passes on what the Expert is. Measured on the library, a
  classic Hard keeps 26 % hammer-ons where the early games' Hard has
  11–15 %: our Expert is sixteenth-heavy (40 % against their 20–24 %),
  and deletion alone cannot remove what the level above holds. That is
  a finding about Expert, recorded rather than tuned away.
- `beatbyte-poly` is the thirteenth crate and the second model the
  classic programme depends on only at authoring time: the game never
  runs it, and a song without a polyphony sidecar plays exactly as it
  did.
