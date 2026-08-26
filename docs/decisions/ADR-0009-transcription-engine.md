# ADR-0009 — Automatic guitar-chart transcription

**Status:** accepted · **Date:** 2026-08-26

## Context

BeatByte turns any imported song into a playable five-lane chart. The
first generator did this from onsets alone: every detected transient
became a note candidate, lanes came from spectral brightness, and
sustains from an energy heuristic. That produces a chart that reacts
to the song without following it — drums drive the notes, lanes do not
track the riff, and held tones are guessed.

The goal is a chart a player recognises as "the part I am hearing",
generated locally, deterministically, with no cloud service, no Python
or Node runtime, and no neural model.

## Decision

### 1. Measure first, then change

All transcription work is judged by `beatbyte-audio::eval`: eight
synthetic scenes (melody, riff, chords, drums+bass over guitar, voice
over guitar, sustains, syncopation, tempo ambiguity) whose ground
truth is placed by the scene itself, plus metrics for onset
precision/recall, timing error, pitch accuracy, sustain length error,
sustain fragmentation, distractor contamination and tempo octave
errors.

Synthetic rather than real audio, deliberately: ground truth for real
music is either unavailable or hand-annotated, and a metric you cannot
recompute after every change is not a gate. Real tracks remain the
final arbiter for feel — the harness never claims otherwise.

Every tuned constant is swept and the measurements recorded next to
it in the code.

### 2. Tempo: measurement chooses the grid, perception chooses the octave

Candidates come from the autocorrelation of the flux envelope **and**
from raw inter-onset intervals (sparse material defeats the
autocorrelation entirely). Each candidate is scored by how much onset
strength its **sixteenth-note** grid explains, weighted by the metric
hierarchy, with a tolerance that shrinks with the grid.

The octave is then decided separately, among candidates that are
octave-related to the best-supported grid, by perceptual preference ×
duple-metre plausibility × beat occupancy.

This split is not stylistic. A grid and its double explain the onsets
*equally well by construction* — the faster grid contains the slower
one — so no amount of onset evidence can separate them, while a
preference applied to unrelated tempi picks nonsense. Two earlier
attempts to decide the octave from onset evidence alone each fixed one
scene and broke another.

### 3. Melody: harmonic salience with the guitar's own resolution

Harmonic summation over a semitone grid, on the harmonic layer of an
HPSS separation, with:

- **interpolated magnitudes at the exact harmonic frequency.** Taking
  the maximum of neighbouring FFT bins destroys resolution exactly
  where a guitar lives: at 82 Hz with 10.8 Hz bins, three adjacent
  semitones share a bin.
- **sub-octave suppression.** Every even harmonic of F0/2 lands on a
  partial of F0, so the sub-octave collects half the evidence for
  free; the octave above separates them asymmetrically.
- **a flat register weighting across the neck.** An earlier Gaussian
  centred on MIDI 66 was fitted to one pop song's bassline and
  penalised the guitar's own low E fourfold.
- **a struck-voice preference.** A plucked string's energy appears
  within milliseconds of an attack; a voice swells into place. Pitches
  whose own salience jumps at a detected attack are favoured for the
  length of a note.

### 4. Notes end where the music says, and start where something is struck

A repeated note at the same pitch is invisible to a pitch tracker, so
segmentation splits at **attacks** — but only attacks belonging to
that voice, i.e. where the tracked pitch's own salience rises.
Without that qualifier a drum hit over a held guitar note split it.

### 5. Onsets: SuperFlux, per band, judged locally

Spectral flux against a maximum-filtered earlier frame (so frequency
drift is not an attack), computed in three bands that are normalized
separately (so a quiet pick attack is not buried by a loud kick), with
strength expressed **relative to the local peak** rather than to the
loudest onset in the song.

The local normalization is not cosmetic: the chart generator selects
notes by strength, and global normalization made it skip quiet
passages wholesale.

### 6. Charts: one master, difficulties as reductions

Melody notes drive placement — lanes follow the riff's pitch contour
(green low → orange high, by interval within a phrase), and a held
tone becomes a sustain of its **real** length, trimmed by the
tempo-scaled trailing gap the charting community standardised. While a
strong melody note is held, the lead owns the highway.

Easy/Medium/Hard/Expert are thinned **from the next harder chart**, to
a target note density in notes per beat, selecting by rank rather than
by a strength threshold. Deriving each difficulty independently from
the master does not guarantee nesting, and a threshold search has a
pathological case where no threshold hits the target.

### 7. Playability is a constraint, not a report

Density, burst, lane motion, jump size and direction changes are
measured, combined half as their mean and half as their worst term,
and the burst limit is **enforced during generation**: an overfull
one-second window loses its weakest notes, never its anchor.

## Consequences

**Good.** Tempo is correct on all eight scenes (three were wrong).
Melody F1 rose from 0.81 → 1.00 (plain melody), 0.53 → 0.98 (riff),
0.78 → 1.00 (drums and bass over guitar), 0.67 → 0.91 (syncopation);
pitch accuracy on the low riff 3 % → 100 %. Contamination by drums and
bass fell to zero. Everything stays deterministic, offline and
Rust-only.

**Bad.** Polyphony is unrepresented: a single-F0 tracker charts a
chord as whichever of its notes wins (scene C melody F1 0.24). The
pitch it reports is right and no chords are invented from coincidence,
but harmony is missing. A voice that is both louder and higher than
the guitar still wins some of the time (scene E recall 0.38).

**Unresolved.** On dense real mixes the tracked voice hops between
instruments, producing shorter melody segments than a listener would
say are there (median 0.15 s on a pop track). The harness cannot
settle whether the resulting chart feels better or worse, because
"accurate" and "fun" are different claims.

## Alternatives considered

- **Neural source separation** (Demucs/Spleeter class). Would resolve
  polyphony and the voice/guitar conflict outright. Rejected: no
  Rust-native option exists that keeps the offline, dependency-light,
  no-Python promise, and shipping a model would change what BeatByte
  is.
- **Deciding the tempo octave from onset evidence.** Tried twice,
  measured, discarded — see §2.
- **A strength threshold for difficulty thinning.** Note strengths are
  a step function; for many songs no threshold lands near the target
  density, and a bisection converges on the one that empties the
  chart.
- **Multi-resolution STFT for pitch.** Shorter windows do not help the
  guitar's low register — semitone spacing at 82 Hz is below the bin
  width of any usefully short window. Exact-frequency interpolation
  solved the same problem without a second transform.
