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

## An unplanned finding: charts are reproducible per platform, not across them

The rock gate found something nobody had looked for, because nothing
had ever compared two platforms' charts. Hashing the serialised chart
passed on macOS and failed on Linux. Rounding note times to whole
milliseconds fixed `circuit-breaker` and **not** `solder-groove` —
so the divergence there is larger than a millisecond: a threshold
comparison somewhere in generation resolves the other way, and a note
is kept, dropped, or snapped to a different frame.

The cause is not that the arithmetic is wrong. It is that a pipeline
built on thresholds amplifies a last-bit difference into a discrete
decision, and `ln`/`exp`/trigonometry are not required to agree to
the last bit between platform libm implementations. Fixing it at the
source means finding the comparison sitting on the knife edge —
worth doing one day, out of scope here, and recorded so it is not
rediscovered from scratch.

What was done instead: the gate records a fingerprint per platform,
and the README's claim was corrected from "bit-identical charts out"
to what is actually true — reproducible per platform, every time; not
across platforms.

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

---

# The grid reaches the chart (Phase 3a, 2026-09-06)

Phase 2 gave the analysis a tracked grid. The chart never got it:
format v1 stores one `bpm` and one `offset_s`, and the generator
quantised every note to that constant grid, the highway drew its
lines from it, the phrases counted from it, the editor snapped to it
and the sustain ticks were scored on it. Measured on the library
(`beatbyte-cli analyze --json`, the tracked beats against the constant
grid laid from the first beat at the median tempo):

| Song | local tempo | tracked − constant, max | at the end |
|---|---|---:|---:|
| Eagles — Hotel California (live 1977) | 140.6–152.0 BPM | **1.61 s** | +1.49 s |
| Böhse Onkelz — Mexico | 106–120 BPM | 0.36 s | +0.24 s |
| Manowar — Warriors of the World (live) | 84–91 BPM | 0.24 s | −0.18 s |
| Toto — Africa | 89–95 BPM | 0.19 s | +0.17 s |

More than three beats on the live recording; a third to two thirds of
a beat on studio songs. Every note was snapped to a subdivision of the
wrong grid (within the 55 ms tolerance of *some* eighth, which is
always available), and the bar lines ran ahead of the drummer.

## What was built

`ChartFile.grid` (`docs/chart-format/chart-format-v1.md`): the tracked
beats to a tenth of a millisecond, and the downbeats once a stage
knows them. Notes are quantised to the subdivisions of the LOCAL beat
(`BeatGrid::quantize`), tails and hold windows use the local beat, the
phrases count bars on the grid, the track's tempo map is one change
per beat (a beat is exactly one beat in), the highway's lines and bars
follow the grid, the lyric countdown counts the local beat, the editor
snaps to it. A chart without a grid behaves exactly as before.
`redesign` carries the fresh grid and moves the carried difficulties
onto it within the snap tolerance.

## The rollout, counted

70 charts rolled over; the legacy folder skipped as always. The
carried easy and medium — the ear-approved readings — kept every
note, lane and tail:

| | notes | moved > 1 ms | moved > 20 ms | median | mean | max |
|---|---:|---:|---:|---:|---:|---:|
| easy | 15 872 | 97 % | 53 % | 21.7 ms | 23.4 ms | 55.0 ms |
| medium | 30 423 | 97 % | 54 % | 21.8 ms | 23.4 ms | 55.0 ms |

Hard and expert were regenerated on the grid (note counts change by a
few percent per song, as a regeneration does). The two synthesized
reference tracks' fingerprints moved once with it — their notes sit
on the grid's subdivisions and their phrases on its bars now — and
are re-recorded (`apps/beatbyte/tests/rock_is_unchanged.rs`), the
projection rounding to the microsecond before the millisecond so an
eighth at an exact half-millisecond cannot flip under float noise.
Autopilot on two rolled-over songs at expert: Hotel
California 1580/1580 perfect, Africa 908/908 perfect, no miss, no
overstrum. A second `redesign --all` writes nothing.

⚠️ It did not, the first time. `snap_notes` recomputed `a + k · step`
for a note already on the grid and got the stored float back a few
ULPs off, called that a move, and rewrote every chart on every run —
the same mechanism as the currency check earlier that day, one layer
down. A snap under a microsecond is not a move (`SNAP_NOISE_S`), and
the proof is the run that writes nothing.

## What it does not claim

The ear has not judged it. A hit moved by 22 ms toward the drummer is
the intended direction and below what a hit window notices; whether
the medium readings still *feel* as approved is the user's call, and
every folder keeps its previous version one pointer away.

# Downbeats from a model (Phase 3b, 2026-09-06)

The chart's bar lines had never been measured. Phase 2 gave the
analysis a tracked grid; nothing gave it bars, so every consumer —
the phrases, the highway, the editor — counted four beats from the
first one. The corpus has the DJ's downbeats, so the question was
answerable: how often is "every fourth beat from the first" a bar
line, and what does a model do instead?

## What was built

`beatbyte-meter` (ADR-0015) runs the *Beat This!* pair (ISMIR 2024,
MIT) through the game's own runtime — the mel front end and the
beat model, chunked as the reference does, its minimal peak picker,
downbeats snapped onto beats — and folds the answer into the
analysis under a **policy**: only the model's downbeats placed on the
tracker's beats (`Downbeats`), or the model's whole grid (`Grid`).
The corpus example (`cargo run --release -p beatbyte-meter --example
corpus -- <anlz-root> <audio-root> [--all]`) scores every condition on
the same decode, against the same truth, with the same MIREX
metrics as Phase 2 plus a downbeat F-measure at ±70 ms
(`Scores::downbeat_f`).

## Loop house, the Phase 2 corpus

Seven tracks, the DJ's grids as truth. *F/C/A* = beat F-measure /
CMLt / AMLt; *D* = downbeat F. "fours" is what shipped: the tracker's
grid with a bar every fourth beat. "hybrid" is the tracker's beats
with the full model's downbeats snapped onto them.

| Track | BPM | ours F | ours C | fours D | small F | small D | full F | full C | full A | full D | hybrid D |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Groovemasta et al. | 120.0 | 1.000 | 1.000 | 1.000 | 1.000 | 0.995 | 0.997 | 0.993 | 0.995 | 0.995 | 0.995 |
| Lime – Angel Eyes | 121.8 | 1.000 | 1.000 | 1.000 | 0.996 | 0.998 | 0.996 | 0.993 | 0.994 | 0.998 | 0.998 |
| Ross – Coming Up | 121.9 | 0.748 | 0.798 | 0.000 | 0.751 | 0.690 | 0.758 | 0.803 | 0.995 | 0.746 | 0.741 |
| Zsak – I Want Your Soul | 123.0 | 0.245 | 0.246 | 0.244 | 1.000 | 1.000 | **1.000** | 1.000 | 1.000 | **1.000** | 0.244 |
| Ross – Buscame | 124.4 | 1.000 | 1.000 | 1.000 | 0.993 | 0.915 | 0.991 | 0.988 | 0.988 | 0.984 | 0.984 |
| BICEP / OPAL (Four Tet Rmx) | 127.0 | 0.889 | 0.994 | 0.905 | 0.826 | 0.883 | 0.850 | 0.855 | 0.987 | 0.880 | 0.833 |
| Vera – Love Comes Easy | 128.8 | 1.000 | 1.000 | 1.000 | 0.992 | 0.997 | 0.953 | 0.975 | 0.997 | 0.930 | 0.997 |
| **mean** | | 0.840 | 0.863 | 0.736 | 0.937 | 0.925 | **0.935** | 0.944 | 0.994 | **0.933** | 0.827 |

Read it in three parts. Where the tracker was right it stays within a
frame of the model (four tracks at 1.000 against 0.95–1.00: the
model's 20 ms frames against a grid fitted to the audio). Where it
had locked onto the wrong level (Zsak, F 0.245 with AMLt 1.000 — the
Phase 2 finding that survived the kick channel) the model is at
1.000, and the hybrid inherits the tracker's error whole: 0.244,
because no downbeat placed on the wrong beats is a bar line. And
where the music is genuinely hard (Ross – Coming Up, BICEP) both are
partial and the model's downbeats are still worth 0.75–0.88 against
0.00–0.90 by counting.

The full model over the small one: a hair better on beats and
downbeats on average, worse on one track (Vera); it costs 19–32 s a
song against 12–21 s under load. Both are registered; the driver
prefers the full one when both are installed.

## Every track with a grid (`--all`)

Eleven tracks pair (of 29 files in the folder; the rest have no
Rekordbox analysis or are not music). The seven above plus a pop
record and three more house tracks:

| Track | BPM | ours F | ours C | ours A | fours D | small F | small D | full F | full C | full A | full D | hybrid D |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Magou – Pas Jolie | 118.0 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 | 0.972 | 0.972 |
| Huey Lewis – The Power of Love | 118.5 | 0.999 | 1.000 | 1.000 | 0.000 | 0.993 | 0.000 | 0.971 | 0.989 | 0.991 | 0.000 | 0.000 |
| Krystal Klear – Essentia | 128.0 | 0.880 | 0.881 | 0.888 | 0.880 | 0.894 | 0.905 | 0.965 | 0.953 | 0.953 | 0.961 | 0.887 |
| BICEP – Glue | 130.0 | 0.606 | 0.605 | 0.633 | 0.606 | 0.931 | 0.926 | **0.956** | 0.932 | 0.932 | **0.950** | 0.623 |
| **mean of 11** | | 0.851 | 0.866 | 0.956 | 0.694 | 0.943 | 0.846 | **0.949** | 0.953 | 0.985 | **0.856** | 0.752 |

Same picture, wider: two more tracks the tracker had lost (Glue
0.606, Essentia 0.880) are at 0.956 and 0.965 on the model, and the
hybrid again keeps the tracker's errors (0.623, 0.887). One honest
zero: on *The Power of Love* every condition scores 0.000 on
downbeats while every one of them is at 0.97–1.00 on beats — the
model and the DJ agree on the beats and disagree on which of them
is the "one". Whether the DJ set the phrase on a half-bar or the
model hears a rock "one" on the backbeat is a listening question;
it is not evidence for the tracker, whose fours land on the same
wrong beat.

**Decision: the model's grid** (`Policy::Grid`, the shipped default
in `ml` builds with the models installed). Both means say so — beat
F 0.851 → 0.949, downbeat F 0.694 → 0.856 over the hybrid's 0.752 —
and the mechanism says why: the tracker's failures are phase and
level errors, and downbeats cannot repair either. On every one of
the eleven the model's tempo is within 2 % of the tracker's (the
model reads 120.00 or 125.00 — its beats sit on 20 ms frames, so a
median interval is quantised; the shipped tempo is the mean of the
steady intervals, which cancels that).

## What the library said: the level

The corpus has no rock with truth; the user's library is mostly
rock. The rollover with the model adopted its grid on 41 of 64
folders and kept the tracker's on 23; a look at five of the kept
ones shows a disagreement the corpus never exercised — not the
phase, the **metrical level**:

| Song | tracker BPM | model BPM | ratio | tracker bar | model bar | bar ratio | model beats/bar |
|---|---:|---:|---:|---:|---:|---:|---:|
| Böhse Onkelz – Keine Amnestie für MTV | 112.51 | 230.77 | 2.05 | 2.133 s | 1.060 s | 0.50 | 4.08 |
| Böhse Onkelz – So sind wir | 95.34 | 187.50 | 1.97 | 2.517 s | 1.260 s | 0.50 | 3.94 |
| Böhse Onkelz – Terpentin | 104.17 | 214.29 | 2.06 | 2.304 s | 1.140 s | 0.49 | 4.07 |
| Böhse Onkelz – Mexico | 112.51 | 166.67 | 1.48 | 2.133 s | 1.420 s | 0.67 | 3.94 |
| Christina Aguilera – Genie in a Bottle | 117.19 | 88.24 | 0.75 | 2.048 s | 2.720 s | 1.33 | 4.00 |

On fast rock the model hears double time in 4/4 (bars of 1.06 s
where the tracker's fours make 2.13 s); on the shuffle it hears a
3:2 and on the pop song a 3:4. Both readings are internally
consistent — the model puts four of its beats in each of its bars —
and the charts on the tracker's level are the ones the user's ear
approved. Nothing here says which level is *right*; what is certain
is that a downbeat at the other level is a half-bar or a bar and a
half, so neither policy is safe across a level.

The 23 ratios, model over tracker: 2.00–2.03 on nine songs, 1.99
and 1.97, 1.41–1.51 on three, 0.75–0.76 on three, 0.50–0.52 on
four, 0.82 on one, and 1.11 and 1.06 on the two nearest (Sniff 'n'
the Tears, Tom Petty). Nothing between 1.06 and 1.41; the 41 adopted
songs are all within 5 %, the corpus's eleven within 2 %.

**Rule (`merge::apply`, shipped):** the model's answer is adopted
when its tempo is within 5 % of the tracker's — the same number
`redesign` merges charts within, so an import and a rollover decide
alike — and otherwise the chart keeps the tracker's grid, takes
nothing from the model and says so (`meter: … not a reading of the
same grid`). The decision is a pure function with these ratios
pinned. The library after the rollover: the model's grid on the 41
songs where the two agree, the tracker's on the 23, the previous
version of every folder one pointer away, a second `redesign --all`
writing nothing, and the ear decides there as it did for the tracked
grid. A rock corpus with downbeat truth is what would settle the
level question.

## Reproducing

```text
beatbyte-cli models install beat-this-mel
beatbyte-cli models install beat-this          # or beat-this-small
cargo run --release -p beatbyte-meter --example corpus -- <anlz-root> <audio-root> [--all]
```

# Repeated sections charted identically (Phase 3c, 2026-09-06)

A human charter charts a chorus once and pastes it. The generator
read every chorus afresh, and because its lane choice carries a
per-note hash and its thinning is a global budget, the same music at
minute one and minute three came out as two different charts — the
single most audible "generated" tell (plan, C4).

## What was built

`beatbyte-audio::analysis::structure` finds the pairs of beat spans
that are the same music: per beat, a chroma vector and a 20-band log
spectral envelope, both centred and scaled over the song's sounding
beats (a silent tail otherwise drags the average away from the music
— and matched itself at 1.00); the diagonals of the self-similarity
matrix scanned for runs above a floor of 0.6 (smoothed over four
beats, then grown back to the raw floor), at least eight bars long,
snapped to bar lines, the longest and most similar non-overlapping
pairs kept. `SongAnalysis.repeats` carries them; the meter recomputes
them when it replaces the grid (they are beat indices). The generator
then copies the master notes of every repeat's first occurrence onto
its second, beat by beat, before the difficulties derive — so every
difficulty inherits one reading of the chorus.

## On real songs

What the stage claims, on the analyzer's grid (beat spans as clock
times; similarity is the mean cosine along the pair):

| Song | repeats | coverage | the pairs |
|---|---:|---:|---|
| Nirvana – Smells Like Teen Spirit | 2 | 65 % | 0:18–1:30 = 1:32–2:44 (verse + chorus, both times); 3:44–4:02 = 4:04–4:22 |
| Toto – Africa | 1 | 50 % | 0:36–1:43 = 1:43–2:51 |
| Metallica – The Unforgiven | 3 | 42 % | 1:00–1:52 = 2:16–3:07; 1:53–2:09 = 3:09–3:24; the outro loop 5:13–5:26 = 5:26–5:40 |
| Journey – Don't Stop Believin' | 2 | 40 % | 0:20–0:36 = 0:36–0:52; 1:20–1:54 = 2:31–3:05 |
| Böhse Onkelz – Mexico | 1 | 15 % | 0:38–0:59 = 2:25–2:47 (the first chorus and the last) |

Under 0.2 s a song. The synthetic pin: an A-B-A-C song where A is an
eight-bar riff finds exactly A = A at the right beats (and nothing
with the silence appended to it).

## What it changes, measured on the reference tracks

The two synthesized reference tracks are loop-based, and the stage
finds their repeats (the demo song: 7.5–33.8 s = 37.5–63.7 s, 56
beats, 0.97; the groove song: 5.2–36.5 s = 36.5–67.8 s, 48 beats —
groove + pad bridge, twice, 0.98). `repeat_consistency` — the share
of the second occurrence's notes that the first occurrence has at
the same beat position and lane — before and after the copy:

| | easy | medium | hard | expert | notes (expert) |
|---|---:|---:|---:|---:|---:|
| demo, before | 0.54 | 0.56 | 0.86 | 0.86 | 352 |
| demo, after | **1.00** | **1.00** | **1.00** | **1.00** | 352 |
| groove, before | 0.25 | 0.39 | 0.31 | 0.40 | 185 |
| groove, after | **0.89** | **0.97** | **0.97** | **1.00** | 177 |

That is the tell, in numbers: identical music charted with a quarter
to half of its notes in common. After the copy expert is identical by
construction; the lower difficulties are thinned from the master
under a song-wide budget, so a note at the edge of a span can still
fall one way in one occurrence and the other way in the other (three
in forty on the groove song's easy). The rock gate's fingerprints
moved once with this and are re-recorded.

## On the library

`redesign --all` with the stage in place, 70 folders: 62 songs have
at least one repeat (1.85 a song on average, at most 5), covering
36 % of their beats on average (9–79 %). The expert consistency of
those repeats BEFORE the copy — how much the second occurrence
already agreed with the first — was **0.25 on average (median 0.24,
range 0.00–0.46); not one song above a half.** That is what "the
same chorus playing differently at minute one and minute three"
measures as. After the rollover the same measure reads **0.99 on
average (median 1.00; 46 of 62 songs at 1.00, 58 at 0.95 or above,
the lowest 0.85)** — the copy is at the master, and expert is thinned
from it under a song-wide budget and spacing, so a note at a span's
edge can still fall either way. The second pass writes nothing.

## Reproducing

```text
cargo run --release -p beatbyte-audio --example repeats -- <audio>…
beatbyte-cli analyze <audio>          # lists the repeats
beatbyte-cli redesign <library> --all # prints each folder's consistency before
```
