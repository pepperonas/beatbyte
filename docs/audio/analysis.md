# Music Analysis Pipeline

Implemented in `beatbyte-audio` (pure stages) with output types in
`beatbyte-core::music`. Consumed by the chart generator
(`beatbyte-chart::generate`).

```text
Audio File
    │  decode (rodio/symphonia) → mono f32, native rate
    ▼
Preprocessing
    │  channel downmix, optional 2× decimation (half-band FIR),
    │  duration cap (untrusted input)
    ▼
Onset Detection (spectral flux)
    │  STFT (Hann 1024, hop 512) → log-magnitude spectra
    │  flux[t] = Σ max(0, |X_t| − |X_{t−1}|)
    │  adaptive threshold (moving median + δ), local-maximum picking,
    │  minimum inter-onset gap
    ▼
Tempo Estimation
    │  autocorrelation of the flux envelope over 60–200 BPM lags,
    │  log-normal prior around 120 BPM (octave disambiguation),
    │  parabolic peak interpolation (sub-BPM resolution)
    ▼
Beat Grid
    │  tracked (dynamic programming over a kick-weighted flux, Ellis
    │  2007; Phase 2) and extended across the full duration; the
    │  constant-tempo grid remains available (`GridMode`)
    ▼
Meter (optional — `beatbyte-meter`, `ml` builds with the model installed)
    │  Beat This! (ISMIR 2024) through the pinned runtime: 22 050 Hz
    │  mono → log-mel (128 bands, 50 fps) → beat + downbeat logits
    │  in 1500-frame chunks stitched keep-first → peak picking,
    │  downbeats snapped onto beats. Where it reads the same grid as
    │  the tracker (tempo within 5 %) its grid REPLACES the
    │  tracked one (beats, downbeats, tempo from its median interval)
    │  — the corpus's choice, ADR-0015; a level apart, the tracked
    │  grid stays and nothing of the model is taken.
    ▼
Melody Extraction (lead transcription)
    │  STFT (Hann 2048, hop 512 — pitch needs longer windows)
    │  HPSS: median filter across time (harmonic) vs frequency
    │    (percussive), Wiener-style soft mask keeps the tonal layer
    │  pitch salience per frame: harmonic summation over a semitone
    │    grid (E2..E6, 6 harmonics, ±1-bin mistuning tolerance),
    │    register-weighted toward the lead (the bass must not win)
    │  contour tracking: DP over semitone states + unvoiced state
    │    (jump penalties; flat frames fall to unvoiced)
    │  segmentation: stable voiced runs → notes with TRUE start/end;
    │    transient smears rejected by their decaying salience and by
    │    the loneliness rule (riffs are runs, blips are drums)
    ▼
SongAnalysis { bpm, beats[], downbeats[], onsets[], melody[{time, end, midi}], duration }
```

## Design rules

- Every stage is a pure function — same samples, same result. Tests
  synthesize known signals (click tracks at known BPMs, noise beds) and
  assert tolerances instead of exact floats.
- Analysis quality targets: BPM within ±2% on steady material; onsets
  within ±15 ms of synthetic truth.

## Known limitations (deliberately documented)

- **Bars without a model**: the built-in analyzer knows no downbeat;
  a chart from it counts four beats from the first one, and the
  corpus says that is a bar line about three times in four on loop
  house and less elsewhere (`docs/audio-eval-baseline.md`). With the
  meter model installed the bars are measured.
- **Tempo**: format v1 charts carry one `bpm` for display and for
  readers without `grid`; since v0.14.30 the chart's `grid` is the
  tracked one and every consumer follows it. Notes still quantise to
  the LOCAL beat's subdivisions and fall back to onset times outside
  it, so a hit stays on the music even where the grid is imperfect.
- **Octave errors**: 87 vs 174 BPM is genuinely ambiguous; the prior
  prefers the danceable octave, and the CLI reports the alternative.
- **No pitch/instrument separation**: lane assignment is driven by
  onset strength and spectral brightness, not melody transcription.
  Automatic charts are *playable, not transcriptions*.
- The intended workflow remains: `automatic chart → human correction →
  final chart`.
