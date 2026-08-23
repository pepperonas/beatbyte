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
    │  phase chosen to maximize onset support, beats laid across the
    │  full duration
    ▼
SongAnalysis { bpm, beats[], onsets[{time, strength}], duration }
```

## Design rules

- Every stage is a pure function — same samples, same result. Tests
  synthesize known signals (click tracks at known BPMs, noise beds) and
  assert tolerances instead of exact floats.
- Analysis quality targets: BPM within ±2% on steady material; onsets
  within ±15 ms of synthetic truth.

## Known limitations (deliberately documented)

- **Constant tempo assumption**: format v1 charts carry one BPM. Rubato
  and live drummers will drift against the grid; the generator falls
  back to onset times (not grid positions) so notes stay on the music
  even when the grid is imperfect.
- **Octave errors**: 87 vs 174 BPM is genuinely ambiguous; the prior
  prefers the danceable octave, and the CLI reports the alternative.
- **No pitch/instrument separation**: lane assignment is driven by
  onset strength and spectral brightness, not melody transcription.
  Automatic charts are *playable, not transcriptions*.
- The intended workflow remains: `automatic chart → human correction →
  final chart`.
