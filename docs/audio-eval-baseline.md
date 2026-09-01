# Analysis baseline (Phase 1) and the grid fix (Phase 2)

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

**Phase 1 ended here. Nothing in the analysis pipeline was changed 
to produce anything above this line.**

---

# The grid fix (Phase 2)

Measured 2026-09-01, same corpus, same metrics. **The pipeline
changed here**; everything above this line is the untouched baseline.

## What was built

Two things, both small, both aimed at the two causes Phase 1 named.

**A kick channel** (`onset.rs`). The flux is still summed broadband
as before, and now *also* over a narrow low band — 30–130 Hz, a
kick's fundamental and first harmonic, computed in the same FFT loop
for one extra add per bin. The point is what it cannot hear: an
offbeat open hat at 6 kHz. On four-to-the-floor the broadband curve
is dominated by the hat layer, half of which sits deliberately off
the beat, which is exactly the tie the phase fit was losing.

**A tracked grid** (`analysis/beats.rs`). Dynamic programming over
the onset envelope, after Ellis (2007): each frame chooses its best
predecessor and pays a squared-log penalty for landing anywhere but
one period back, then the best chain is read out backwards. A tracked
sequence cannot accumulate error, because each beat only has to sit
one period after the previous one rather than *k* periods after the
first.

Both are configurable through the existing central `AnalyzerConfig`,
which is now serialisable end to end (round-tripped by a test, not
merely derived).

## The result on real tracks

| Fall | BPM-Ref | Beat-F starr | Beat-F verfolgt | CMLt starr | CMLt verfolgt |
|---|---|---|---|---|---|
| Groovemasta et al. | 120.00 | 0.000 | **1.000** | 0.000 | 1.000 |
| Lime – Angel Eyes | 121.78 | 0.332 | **1.000** | 0.396 | 1.000 |
| Ross – Coming Up | 121.93 | 0.271 | **0.748** | 0.321 | 0.798 |
| Zsak – I Want Your Soul | 123.00 | 0.000 | **0.245** | 0.000 | 0.246 |
| Ross – Buscame | 124.42 | 0.676 | **1.000** | 0.767 | 1.000 |
| BICEP / OPAL (Four Tet Rmx) | 127.00 | 0.339 | **0.889** | 0.398 | 0.994 |
| Vera – Love Comes Easy | 128.77 | 0.328 | **1.000** | 0.365 | 1.000 |
| **Mittel** | | 0.278 | **0.840** | | |

Note density is unchanged to one decimal on every track: the grid
moved, the note count did not.

## And on rock, which was the constraint

| Fall | Beat-F starr | Beat-F verfolgt | CMLt starr | CMLt verfolgt |
|---|---|---|---|---|
| rock / circuit-breaker | 0.000 | **0.982** | 0.000 | 0.993 |
| rock / solder-groove | 0.995 | **0.995** | 1.000 | 1.000 |
| house-sample / flat-4x4 | 0.977 | 0.977 | 1.000 | 1.000 |
| house-sample / two-rasters | 0.863 | **0.977** | 1.000 | 1.000 |
| house-sample / soft-transients | 0.977 | 0.977 | 1.000 | 1.000 |
| house-sample / filter-sweep | 0.977 | 0.977 | 1.000 | 1.000 |

**Nothing regressed anywhere.** The commission asked for no rock
regression and got a rock *fix*: the 146 ms phase error Phase 1 found
on `circuit-breaker` is gone, which also means the built-in song is
now a defensible transcription and not merely a self-consistent
chart.

That is why the tracked grid is the shipped default, and why the
chart fingerprints in `apps/beatbyte/tests/rock_is_unchanged.rs`
moved — deliberately, once, with this table as the reason.

## The two mechanisms, verified separately

**Drift is gone.** The residual no longer grows across a track:

| Track | Rest 1. Min | Rest letzte Min | Drift den das starre Raster hatte |
|---|---|---|---|
| Lime – Angel Eyes | −12 ms | −16 ms | 738 ms |
| BICEP / OPAL | −15 ms | −67 ms | 1238 ms |
| Vera – Love Comes Easy | +4 ms | −7 ms | 293 ms |
| Ross – Buscame | +2 ms | −4 ms | 160 ms |

**The offbeat lock is gone.** First-beat phase, in beats, was −0.473 /
−0.459 / −0.419 / −0.252 on four tracks; it is now between −0.085 and
+0.094 on **all seven**.

The kick channel is what did the second one, and the sweep says so
monotonically rather than by argument — mean beat F over the corpus
at low-band weights 0.0 / 0.5 / 0.75 / 1.0:

| Kick-Gewicht | Beat-F Mittel | Median | schlechtester |
|---|---|---|---|
| 0.00 | 0.530 | 0.713 | 0.000 |
| 0.50 | 0.588 | 0.721 | 0.082 |
| 0.75 | 0.733 | 0.735 | 0.241 |
| **1.00** | **0.840** | **1.000** | 0.245 |

⚠️ My first guess was 0.75, reasoning that a breakdown without a kick
would leave a kick-only tracker with nothing to hold. The reasoning
was wrong: dynamic programming does not need onsets to cross a gap —
with nothing to reward it simply continues at the target period and
picks the music up on the far side.

## What is left, precisely

**Zsak – I Want Your Soul, 0.245.** Not a tracking failure. Its
residual is −75 ms at the start and −71 ms at the end: a *constant*
offset of about 73 ms, a grid parallel to Rekordbox's but shifted,
sitting just outside the ±70 ms tolerance and therefore scoring near
zero. `Ross – Coming Up` degrades similarly (−12 → −100 ms) for its
0.748.

So the remaining error is **sub-beat systematic alignment**, on 2 of
7 tracks, right at the tolerance edge. Candidates, none established
and none guessed at further here: the analysis look-ahead
compensation (`onset.rs`, `frame_offset_s` = 34.8 ms), a slow kick
attack placing the flux peak late, and Rekordbox's own grid placement
on those tracks. Establishing which needs a listening test or a
second reference, not another sweep.

## Cost

469 s of music analysed in 3.6 s — about 130× real time, against a
commission budget of 10 s for a 7-minute track. The tracker is
O(*n* · 1.5*p*) and adds roughly a tenth of a second.

## The rock gate, and its honest limit

`apps/beatbyte/tests/rock_is_unchanged.rs` hashes both built-in
songs' generated charts. It caught the grid-mode flip immediately
(both fingerprints moved). It does **not** catch tracker *tuning*:
changing the tightness from 100 to 12, or the kick weight from 1.0 to
0.5, leaves both charts byte-identical, because the demo songs'
beats are unambiguous enough that any reasonable envelope finds the
same chain. The gate guards the architecture, not the parameters —
worth knowing before trusting it for something it does not do.

## Reproducing

```bash
cargo run -p beatbyte-audio --example baseline           # rock + synthetic, A/B
cargo run -p beatbyte-audio --example baseline_real \
  ~/Library/Pioneer/rekordbox/share/PIONEER/USBANLZ ~/Music/DJ
cargo run -p beatbyte-audio --example sweep_real  <same args>   # the weight sweep
cargo run -p beatbyte-audio --example drift_real  <same args>   # drift, before/after
cargo run -p beatbyte-audio --example phase_real  <same args>   # phase, before/after
```

**Phase 2 ends here.** Phase 3 has a precise target for the first
time: sub-beat alignment, worth 0.16 of the remaining 0.16.
