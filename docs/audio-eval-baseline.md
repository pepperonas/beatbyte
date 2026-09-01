# Analysis baseline (Phase 1)

Measured 2026-09-01 against the pipeline described in
`docs/audio-pipeline-ist.md`.
**No analysis code was changed to produce these numbers.**

## The corpus

The commission asks for three classes of ≥ 5 real tracks with
annotated grids. Two of the three now exist, and one of them is
**real music with grids a human DJ has accepted**:

| Class | What is in it | Ground truth |
|---|---|---|
| `house-sample/` (real) | **7 tracks from your own library**, 118–130 BPM, 5:12–8:16 | **Rekordbox's own beat grids**, read from its `ANLZ0000.DAT` analysis files |
| `house-sample/` (synthetic) | 4 constructed cases, one per breaking property | exact by construction — the beat times ARE where the hits were placed |
| `rock/` | the two built-in songs | exact by construction — rendered from a known BPM and bar count |
| `house-modern/` | *empty* | — |

The real grids come from `~/Library/Pioneer/rekordbox/`: 168 analysis
files, 56 carrying a real grid, 41 with the audio still present, 7 in
the target profile. Read via a new parser (`eval::anlz`) for the two
sections a grid needs — `PPTH` (the file path) and `PQTZ` (one 8-byte
entry per beat: position in the bar, tempo, time in ms).

⚠️ **`master.db` is deliberately untouched.** Rekordbox 6 encrypts its
library database; the path in `PPTH` makes it unnecessary, so no
protection mechanism is worked around. **No audio, no grid and no
library file enters the repository** — the corpus is a local path,
the parser's tests build ANLZ buffers in memory.

⚠️ **Rekordbox's grid is a strong reference, not ground truth.** It is
software output a DJ has beatmatched to and not corrected — good
enough to measure a two-orders-of-magnitude failure, not good enough
to argue about 10 ms.

## Baseline — real tracks

| Fall | BPM-Ref | BPM-ist | Beat-F | CMLt | AMLt | DB | N/s med | N/s p95 |
|---|---|---|---|---|---|---|---|---|
| Groovemasta et al. | 120.00 | 120.02 | **0.000** | **0.000** | 1.000 | 0 | 7.0 | 7.7 |
| Lime – Angel Eyes | 121.78 | 121.97 | 0.332 | 0.396 | 0.745 | 1 | 7.6 | 8.1 |
| Ross – Coming Up | 121.93 | 122.21 | 0.271 | 0.321 | 0.678 | 0 | 4.9 | 6.0 |
| Zsak – I Want Your Soul | 123.00 | 123.03 | **0.000** | **0.000** | 1.000 | 0 | 6.7 | 7.4 |
| Ross – Buscame | 124.42 | 124.37 | 0.676 | 0.767 | 1.000 | 0 | 5.1 | 6.0 |
| BICEP / OPAL (Four Tet Rmx) | 127.00 | 126.68 | 0.339 | 0.398 | 0.739 | 0 | 6.7 | 7.8 |
| Vera – Love Comes Easy | 128.77 | 128.88 | 0.328 | 0.365 | 0.632 | 1 | 5.0 | 5.5 |

**Median Beat-F: 0.332.** No octave errors. Tempo accurate to 0.25 %.

## Baseline — synthetic and rock

| Klasse | Fall | Eig. | Beat-F | CMLt | AMLt | DB | N/s med | N/s p95 | BPM |
|---|---|---|---|---|---|---|---|---|---|
| rock | circuit-breaker | – | **0.000** | **0.000** | 0.957 | 0 | 6.2 | 7.5 | 128.3 |
| rock | solder-groove | – | 0.995 | 1.000 | 1.000 | 1 | 4.0 | 6.6 | 92.1 |
| house-sample | flat-4x4 | f | 0.977 | 1.000 | 1.000 | **0** | 3.9 | 4.2 | 125.2 |
| house-sample | two-rasters | a | 0.863 | 1.000 | 1.000 | **0** | **2.1** | 2.1 | 125.3 |
| house-sample | soft-transients | b | 0.977 | 1.000 | 1.000 | **0** | 3.4 | 3.6 | 125.2 |
| house-sample | filter-sweep | d | 0.977 | 1.000 | 1.000 | **0** | 2.9 | 3.1 | 125.2 |

⚠️ **The synthetic cases score 0.86–0.98 where the real ones score
0.33.** That gap is the most useful thing they say: 32 bars of
constructed audio does not reproduce the defect. They stay as a
regression floor per property; they are not evidence about the real
material, and the real table above is the number that counts.

## What the measurement actually says

### 1. The tempo estimate is not the problem

Seven real tracks, error 0.02 %–0.25 %, **no octave error anywhere**.
The brief expects octave errors on flat four-to-the-floor; on this
material they do not occur. The 60–200 BPM window and the log-normal
prior are doing their job.

### 2. The problem is phase — and it has two distinct causes

**(a) A constant global tempo cannot hold a 6–8 minute track.**
The estimate is an excellent *average* and a poor *grid*: the residual
error accumulates linearly, and the ±70 ms tolerance is 1/7 of a beat.

| Track | Länge | BPM-Fehler | aufgelaufener Drift | in Fenstern |
|---|---|---|---|---|
| BICEP / OPAL | 496 s | −0.250 % | **1238 ms** | 17.7× |
| Ross – Coming Up | 410 s | +0.231 % | **947 ms** | 13.5× |
| Lime – Angel Eyes | 469 s | +0.157 % | **738 ms** | 10.5× |
| Vera – Love Comes Easy | 332 s | +0.088 % | 293 ms | 4.2× |
| Ross – Buscame | 409 s | −0.039 % | 160 ms | 2.3× |
| Zsak – I Want Your Soul | 312 s | +0.026 % | 82 ms | 1.2× |
| Groovemasta et al. | 380 s | +0.020 % | 77 ms | 1.1× |

**Every single track drifts past the tolerance**, the worst by a
factor of 18. This is what Beat-F ≈ 0.33 at AMLt ≈ 0.74 describes:
the grid slides through alignment and is correct about a third of the
time. A DJ track needs 0.014 % tempo accuracy to hold 70 ms over
8 minutes — that is not an estimator you tune, it is an architecture
that does not fit the material.

**(b) Four of seven lock onto the wrong half of the beat.**
Measured first-beat residual in beats: −0.473 (Groovemasta), −0.459
(Zsak), −0.419 (BICEP), −0.252 (Buscame) against +0.06…+0.08 for the
other three. The two at −0.46/−0.47 score Beat-F **0.000** at AMLt
**1.000**: a metrically valid interpretation on the wrong phase — the
textbook offbeat lock. Expected from property (f): the phase fit
(`tempo.rs:150`, 64 candidates, σ = 30 ms, onset proximity only) has
nothing that prefers a downbeat over an offbeat when every beat is
identical.

⚠️ The residual is a nearest-beat distance and **wraps at half a
period**, so start/end residuals cannot be subtracted to get a drift
(−232 ms to +191 ms is 65 ms apart, not 423). The drift column above
is derived from the tempo error, which has no wrap ambiguity.

### 3. The rock finding is the same defect, not a separate one

`circuit-breaker` scores 0.000 at +0.22 % tempo error over 64 s —
141 ms of drift, plus a start phase of 0.322 s (0.687 beats, i.e.
−0.313 wrapped). Both causes, on cleaner material. So the earlier
"146 ms phase error" is not a rock peculiarity; the built-in song is
simply short enough that only one of the two causes dominates.

This also qualifies the "rock is good" premise properly: rock is good
*as a game chart*, because the chart is generated from the same grid
and agrees with itself. It has never been verified as a
*transcription*.

### 4. The second timing layer is discarded, as predicted

`two-rasters` places two hits per beat, 22 ms apart. The pipeline
reports **exactly 128 onsets for 128 beats** — density drops to 2.1/s
against 3.9/s for the same material without the second layer. Property
(a) reproducing on demand: `min_gap_s = 0.05` (`onset.rs:38`).

### 5. Downbeat accuracy is 0 almost everywhere

Expected — there is no downbeat stage (`docs/audio-pipeline-ist.md`
§4.2). The two 1s in the real table are luck of phase.

### 6. Note density is already in range

5.0–7.6 notes/s median on the real tracks. Whatever "too few events"
means on this material, it is not a global density shortfall — worth
knowing before Phase 5 tunes anything.

## What this means for the plan

The brief's Phase 3 proposes "a global fit over (period, phase)".
**That is what the code already does** (`tempo.rs:140`), and the
measurement above is the evidence that a global fit is the wrong
shape for 6–8 minute material, however well it is tuned. The finding
argues for a *time-varying* grid — a per-beat tracker or a piecewise
tempo — and for an accent/downbeat cue that can break the offbeat
tie. I am not proposing the design here; Phase 1 measures.

## Caveats on the metrics themselves

- **AMLt is lenient toward drift.** A grid that slides through
  alignment scores well because `continuity_total` matches per
  annotation rather than tracking a continuous run. Read AMLt as
  "some metrical interpretation matches somewhere".
- **Downbeat accuracy** currently asks only whether the grid's first
  beat lands on a real downbeat. It becomes a sequence metric when a
  downbeat stage exists.
- **Boundary metrics** are implemented but score 0: only
  `filter-sweep` carries reference boundaries and the pipeline emits
  none.

## The regression gate

`rock/` must not degrade by more than 2 %. Two parts:

1. **Metric floor** on the two built-in songs, from the table above.
2. **Bit-identical chart output** for both built-ins — cheap, exact,
   and it catches any accidental behaviour change immediately, which
   a metric with 2 % slack does not.

Part 2 is the one to rely on early; part 1 becomes meaningful once
the phase defect is addressed, since a fix will legitimately move
those numbers — including rock's.

## Reproducing

```bash
cargo run -p beatbyte-audio --example baseline        # synthetic + rock
cargo run -p beatbyte-audio --example baseline_real \
  ~/Library/Pioneer/rekordbox/share/PIONEER/USBANLZ ~/Music/DJ
cargo run -p beatbyte-audio --example drift_real  <same args>   # §2a
cargo run -p beatbyte-audio --example phase_real  <same args>   # §2b
cargo test -p beatbyte-audio --lib eval               # the metrics
```

The two real-corpus examples take paths because the corpus is local
and stays local.

**Phase 1 ends here. Nothing in the analysis pipeline was changed.**
