# ADR-0005: Audio Architecture — Playback, Song Clock, Analysis

- **Status**: accepted
- **Date**: 2026-08-23

## Context

Three audio concerns with very different requirements:

1. **Music playback** must expose an accurate, monotonic playback
   position — it *is* the gameplay timeline.
2. **Analysis** (BPM, beats, onsets) must be pure and deterministic to
   be testable.
3. **UI sound effects** just need fire-and-forget playback.

Bevy's `bevy_audio` cannot serve (1): it exposes no reliable playback
position. It is fine for (3).

## Decision

### Playback: rodio, owned by `beatbyte-audio`

Music playback uses **rodio 0.22** directly (`DeviceSinkBuilder` /
`Player`): it provides `get_pos()`, `try_seek()`, pause/resume and
volume, decodes OGG/WAV/FLAC/MP3 through its Symphonia backends, and is
the same stack `bevy_audio` builds on — no extra native dependencies.

### Song clock: anchored monotonic time, reconciled against `get_pos`

`get_pos()` advances in audio-buffer-sized steps — too coarse to drive
judgment directly. The `SongClock` is a pure state machine:

```text
song_time(mono_now) = anchor_song + (mono_now − anchor_mono)   [playing]
```

- Anchors are set on play/seek/resume.
- Each frame the clock is *reconciled* against the position reported by
  the player: large drift (>30 ms) snaps the anchor, small drift is
  slewed gradually so the timeline never jumps audibly.
- The latency calibration offset is applied by the caller when
  timestamping inputs, never inside the clock.

The clock takes monotonic time as a *parameter* (no `Instant::now()`
inside), which makes drift, pause and seek behavior unit-testable.

### Analysis: pure `samples in → events out`

Decoding produces a mono `AudioData` buffer; analysis stages are pure
functions over it (spectral-flux onset detection, autocorrelation tempo
estimation — see `docs/audio/analysis.md`). An `Analyzer` trait keeps
the implementation replaceable. Analysis output types (`SongAnalysis`,
`Onset`) live in `beatbyte-core` so the chart generator can consume
them without depending on the audio stack.

### SFX: `bevy_audio`

Menu/feedback sounds don't need a timeline; the built-in audio plugin
is the simplest correct tool.

## Consequences

- `beatbyte-audio` owns threads (rodio's output stream) but exposes a
  synchronous, engine-free API; Bevy wraps it in a non-send resource.
- Two rodio instantiations exist (ours and `bevy_audio`'s) — they are
  independent crates sharing an output device through the OS mixer;
  this is the standard setup and costs nothing measurable.
- Full decoding for analysis is memory-bounded (duration cap) because
  analysis needs random access; playback streams and does not.
