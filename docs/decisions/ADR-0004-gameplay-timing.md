# ADR-0004: Deterministic Gameplay Timing & Judgment

- **Status**: accepted
- **Date**: 2026-08-23

## Context

Timing is the product in a rhythm game. Frame-based timing drifts with
refresh rate, hitches corrupt judgments, and untestable timing logic
rots. We need gameplay that is exact, testable and independent of the
renderer.

## Decision

1. **One timeline**: all gameplay times are `f64` seconds on the *song
   timeline* (0.0 = start of audio). The audio subsystem owns the
   authoritative song clock (ADR to follow with Milestone 3); the
   renderer interpolates but never invents time.
2. **The judgment engine is a pure state machine**
   (`beatbyte_core::session::TrackSession`): it consumes timestamped
   inputs plus clock advances and emits judgments/score/feedback events.
   No engine types, no wall clock, no randomness — identical inputs
   yield identical outcomes, and the whole ruleset is unit-tested.
3. **Symmetric, configurable hit windows** (`TimingWindows`), defaults
   ±30 ms Perfect / ±60 ms Great / ±100 ms Good; outside = no hit.
   Windows are data, not code, so calibration and difficulty tuning
   never touch rules.
4. **Frame independence**: the presentation layer calls
   `advance(song_time)` once per frame and forwards inputs with
   song-clock timestamps. A dropped frame delays *feedback*, never
   *judgment* (inputs carry their own timestamps).

## Gameplay rules encoded (and tested) in the engine

- Strum matching with **anchoring** (single notes: highest held fret
  counts; chords: exact match), earliest-matching-note selection,
  note skipping, overstrum semantics.
- HOPO hammer-ons (fret press) and pull-offs (fret release exposing a
  lower held fret), gated on a live chain.
- Sustains scored per musical beat via the `TempoMap`, early-release
  grace, overstrum kills the tail.
- Special phrases → Hype meter (gain per completed phrase, activation
  threshold, drain in beats).

## Consequences

- Latency calibration becomes a pure input/audio offset applied when
  timestamping inputs — the engine never knows about it.
- Replays and automated tests are the same thing: a recorded input
  sequence deterministically reproduces a run.
- Multiplayer is N independent sessions over the same track — no shared
  mutable judgment state.
