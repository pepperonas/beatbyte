# Transcription Engine: Analysis, Baseline and Plan

Written before changing anything, so that every later claim can be
checked against a number that existed first. The measurements come
from `beatbyte-audio::eval` (eight synthetic scenes whose ground truth
is placed by the scene itself) and are reproducible with:

```bash
cargo test -p beatbyte-audio print_the_report -- --nocapture
```

## 1. What the pipeline is today

```text
decode → downsample(≥32 kHz) → spectral flux onsets → tempo (ACF) →
beat grid → melody (HPSS → semitone salience → DP contour) →
RMS energy → SongAnalysis → master chart → difficulty reductions
```

**What already works and must not regress**

- Determinism end to end: no wall-clock, no unseeded randomness, no
  hash-order decisions. Verified by an equality test on repeated runs.
- Onset *timing* is genuinely good: 9–25 ms mean error on matched
  events, well inside the 30 ms Perfect window.
- The chart layer is sound: one master, difficulties as a reduction
  chain, structurally nested, pinned by tests.
- Decoding covers wav/ogg/flac/mp3/m4a with real fixture files.

**What is heuristic and shows it**

- One STFT resolution for onsets (1024/256 at ~22 kHz), one for melody
  (2048/512). Nothing is analyzed at more than one time scale.
- Onset strength is *broadband* flux summed over all bins with equal
  weight, then normalized by the loudest onset in the whole song.
- Melody is a single-F0 tracker: polyphony is outside its model.
- Lane assignment for unpitched notes uses spectral centroid, which
  is a timbre proxy, not a pitch.

## 2. Baseline (before any change)

```text
scene                      bpm  on-F1 mel-F1  pitch   time    frag contam
a_simple_melody           99.8   0.70   0.81    82%    25ms   0.00     0%
b_guitar_riff            139.9   0.99   0.53     3%     9ms   0.00     0%
c_chords                 120.0   0.50   0.00   n/a      0ms   0.00     0%
d_drums_and_guitar       120.1   0.67   0.78    89%    21ms   0.00     0%
e_vocals_and_guitar      110.0   0.79   0.20     0%     1ms   0.00     0%
f_sustains                73.6   0.29   1.00   100%    18ms   0.00     0%
g_syncopation            191.9   0.89   0.67   100%    16ms   0.00     0%
h_tempo_ambiguity         75.0   1.00   1.00    50%    21ms   0.00     0%
```

True tempi: a 100, b 140, c 90, d 120, e 110, f 80, g 96, h 150.

## 3. The five defects, in order of damage

### D1 — Tempo octave errors (3 of 8 scenes wrong)

`c` reports 120 against 90, `g` reports 192 against 96, `h` reports 75
against 150. The cause is in `tempo::estimate_tempo`: the log-normal
prior is centred on 120 BPM with a width of 0.9 *octaves*, which is
wide enough to pull almost any real tempo toward 120. A prior that
strong is not disambiguating octaves, it is choosing them.

A wrong tempo is the most expensive error in the system: the beat grid,
every quantization decision, sustain caps and the difficulty density
targets are all expressed in beats.

**Fix direction:** decide the octave from *musical evidence* rather
than a fixed preference — how well each candidate period explains the
onsets (a beat should have onsets on it), and whether the faster
candidate's off-beats are also populated (if they are, the faster
pulse is the real one). Keep a prior, but narrow it to breaking ties.

### D2 — Pitch collapses exactly in the guitar register

Scene `b` (a riff at MIDI 40–47, i.e. the guitar's low E upward)
scores **3 %** pitch accuracy. Two causes, both mine:

1. The register weighting added for the Rick Astley track is a
   Gaussian centred on MIDI 66 with σ = 16. At MIDI 40 it multiplies
   salience by 0.27 — a four-fold penalty on the exact range this is
   a guitar game for. It was tuned to beat a bassline on one pop song
   and became a rule.
2. The melody STFT window is 2048 samples ≈ 93 ms. At 140 BPM a
   sixteenth is 107 ms, so one window spans most of a note and the
   pitch of neighbouring notes smears together.

### D3 — Polyphony produces nothing at all

Scene `c` (real triads) yields **0** melody events. A single-F0
tracker has no representation for a chord, so the chart falls back
entirely to onsets — which is exactly the "chords must not be invented
from coincidence" failure the chart layer then has to guess around.

### D4 — Onset precision on melodic material

`f_sustains` scores precision **0.17**: about thirty onsets for five
held tones. Broadband flux fires on the internal beating of a harmonic
stack and on release ramps, not only on attacks. `a_simple_melody`
shows the same at 0.53.

Every spurious onset is a candidate note the chart layer must reject
later with heuristics — cheaper to not create it.

### D5 — A voice hijacks the guitar line

Scene `e` puts a loud vibrato voice above a quieter riff: melody
recall against the guitar is **0.12**, precision 0.50. The tracker
follows loudness and register, and both favour the voice. The
information that separates them is not being used: a plucked string
has a *percussive attack coinciding with the note start* and *stable
pitch*, a voice has a slow attack, a scoop and vibrato.

## 4. Plan

Ordered by damage, each step measured against the table above.

1. **P1 Tempo octave decision** — DONE. Evidence-based selection in
   three separable steps, because no single one of them works alone:
   *(a)* candidates come from the autocorrelation **and** from raw
   inter-onset intervals (the ACF produced no candidate at all for a
   sparse chord progression, and the pipeline silently fell back to
   120 BPM); *(b)* the grid that explains the onsets wins, scored on
   the **sixteenth-note** grid with the metric hierarchy weighted, not
   on beat hits alone (beat-only scoring preferred a dotted-eighth
   grid at 186 BPM over the real 140); *(c)* the **octave** is then
   chosen by perceptual preference plus a duple-metre prior, because
   a grid and its double explain the onsets equally well *by
   construction* and no onset evidence can separate them.
   **Result: 7 of 8 scenes exact** (a 99.9/100, b 139.9/140,
   c 90.0/90, d 120.0/120, e 110.0/110, g 96.2/96, h 149.9/150).
   f_sustains stays wrong at 120 against 80 — its onset precision is
   0.33, so its tempo is derived from mostly-spurious detections; it
   is expected to come good with P3 and is pinned as such.
### Result after P1–P3

```text
scene                      bpm  on-F1 mel-F1  pitch   time    frag contam
a_simple_melody          100.0   0.67   1.00   100%    20ms   0.00     0%
b_guitar_riff            140.0   0.99   0.96   100%     9ms   0.00     0%
c_chords                  90.0   0.50   0.41   100%    19ms   0.29     0%
d_drums_and_guitar       120.0   0.67   0.86    77%    17ms   0.22    19%
e_vocals_and_guitar      110.0   0.74   0.27    78%    26ms   0.00     0%
f_sustains                80.3   0.67   1.00   100%    19ms   0.00     0%
g_syncopation             95.9   0.65   0.98   100%    18ms   0.00     0%
h_tempo_ambiguity        150.0   1.00   1.00   100%    20ms   0.00     0%
```

**Every tempo is now correct** (was 3 of 8 wrong). Melody F1 went
0.81 → 1.00 on the plain melody, 0.53 → 0.96 on the riff, 0.67 → 0.98
on the syncopated figure; pitch accuracy on the riff went 3 % → 100 %.

⚠️ **Open question for the ear, not the harness:** on a real pop
track the number of long melody notes fell (147 → 36 held ≥ 0.45 s),
and with it the sustain count in the generated chart (medium 110 → 37).
The harness says this is a correction — scene `a` used to merge four
notes into one 1.8 s event, and its fragmentation metric dropped from
0.36 to 0.09 — but "more accurate" and "more fun" are not the same
claim, and only playing it settles which one this is.

2. **P2 Guitar-register pitch** — DONE, and the diagnosis in the
   original write-up was only half right. The register Gaussian was
   indeed wrong (replaced by a flat weighting across the neck that
   rolls off outside it), but the dominant cause was resolution:
   harmonic salience took the MAXIMUM of three neighbouring FFT bins,
   and at 82 Hz with 10.8 Hz bins three adjacent semitones share a
   bin — the low guitar register was literally unresolvable. Reading
   the interpolated magnitude at the exact harmonic frequency fixed
   it. That exposed the next layer, classic sub-octave shadows (every
   even harmonic of F0/2 lands on a partial of F0), removed by
   comparing each candidate with the pitch an octave above it.
3. **P3 Onset quality** — DONE. SuperFlux (difference against a
   maximum-filtered earlier frame) plus per-band normalization across
   three bands, so a quiet pick attack is not buried by a loud kick.
   `f_sustains` precision 0.17 → 0.67, and with it its tempo became
   correct. The temporal lag had to stay at one frame: two frames held
   the flux elevated after the attack and pushed detections 17 ms late
   on a click track with exactly known positions.
   The onsets then unlocked the real fix for note segmentation: a
   repeated note at the same pitch is invisible to a pitch tracker, so
   notes are now split at ATTACKS — but only attacks belonging to that
   voice, i.e. where the tracked pitch's own salience rises. Without
   that qualifier a drum hit over a held guitar note split it (caught
   by an existing test).
4. **P4 Lead/voice discrimination** — an event score combining onset
   agreement, pitch stability and harmonic salience instead of raw
   loudness. Target: scene `e` recall against the guitar ≫ 0.12.
5. **P5 Chords** — chroma-based detection of simultaneous harmony, so
   scene `c` produces real chord events and the chart stops inventing
   chords from coincident transients.

Non-goals for this pass: a neural source separator (no Rust-native
option that keeps the offline, dependency-light promise), and beat
tracking with tempo drift (the scenes are fixed-tempo; real drift is
a separate measurement).
