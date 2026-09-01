# Analysis baseline (Phase 1)

Measured 2026-09-01 with `cargo run -p beatbyte-audio --example baseline`,
against the pipeline described in `docs/audio-pipeline-ist.md`.
**No analysis code was changed to produce these numbers.**

## What the corpus is — and what it is not

The commission asks for three classes of ≥ 5 real tracks with
annotated grids. That corpus does not exist in this repository and I
cannot supply it: the audio and the Ableton/Rekordbox exports are
yours. Rather than block, Phase 1 measures what CAN be measured
today, and the harness is built so the real corpus drops straight in:

| Class | What is in it now | Ground truth |
|---|---|---|
| `rock/` | the two built-in songs | **exact by construction** — rendered from a known BPM and bar count (`demo.rs:19`, `demo.rs:217`) |
| `house-sample/` | four synthetic cases, one per breaking property | **exact by construction** — the beat times ARE where the hits were placed |
| `house-modern/` | *empty* | — |

⚠️ **The synthetic cases are cleaner than the real material.** A
60 Hz burst on every beat is not a Disco loop with a live drummer
under it. They isolate one property each so a fix can be attributed;
they do not prove the pipeline handles Phonk D. Treat the
`house-sample` numbers below as a floor that must not fall, not as a
verdict on the real problem.

Importers ready for the real thing: the JSON sidecar
(`eval::GroundTruth`, the format from the brief verbatim) and
Rekordbox XML (`eval::rekordbox` — `Inizio`/`Bpm`/`Battito`, tempo
changes and all). **Ableton `.asd` is deliberately not parsed**: it
is an undocumented binary format, and guessing its layout would put
invented facts into the very measurement everything else is judged
by.

## Baseline

| Klasse | Fall | Eig. | Beat-F | CMLt | AMLt | DB | N/s med | N/s p95 | BPM |
|---|---|---|---|---|---|---|---|---|---|
| rock | circuit-breaker | – | **0.000** | **0.000** | 0.957 | 0 | 6.2 | 7.5 | 128.3 |
| rock | solder-groove | – | 0.995 | 1.000 | 1.000 | 1 | 4.0 | 6.6 | 92.1 |
| house-sample | flat-4x4 | f | 0.977 | 1.000 | 1.000 | **0** | 3.9 | 4.2 | 125.2 |
| house-sample | two-rasters | a | 0.863 | 1.000 | 1.000 | **0** | **2.1** | 2.1 | 125.3 |
| house-sample | soft-transients | b | 0.977 | 1.000 | 1.000 | **0** | 3.4 | 3.6 | 125.2 |
| house-sample | filter-sweep | d | 0.977 | 1.000 | 1.000 | **0** | 2.9 | 3.1 | 125.2 |

No octave errors on this material. Tempo is accurate to ~0.3 %
everywhere.

## Three findings the harness produced immediately

### 1. The rock reference's beat grid is 146 ms off the music

`circuit-breaker` scores 0.000 on both beat metrics at the correct
tempo. Measured directly (`example phase_probe`): the true grid is a
beat every 0.4688 s from 0.000, the fitted grid starts at **0.322 s**
— a **−146 ms** phase error. That is not a half-beat (234 ms), so it
is not the classic offbeat lock; it is a genuinely wrong phase.

This has been invisible until now because the CHART is generated from
the same wrong grid, so notes and grid agree internally and the
autopilot still scores 100 %. The error is against the *music*, not
against the chart. **It is the strongest argument in the whole
exercise for building the harness before touching the algorithm** —
and it means the "rock is good" premise needs qualifying: rock is
good *as a game chart*, not verified as a *transcription*.

I have not established the cause and will not guess. Candidates to
check in Phase 2/3: the look-ahead compensation
(`onset.rs:68` `frame_offset_s`), and the fact that the first two
bars of this song carry bass but no drums (`demo.rs:72`), so the
phase fit is dominated by non-percussive onsets.

### 2. The second timing layer is being discarded, as predicted

`two-rasters` places two hits per beat, 22 ms apart. The pipeline
reports **exactly 128 onsets for 128 beats** — the second layer is
gone, and note density drops to **2.1/s** against 3.9/s for the same
material without the second layer. This is property (a) from the
inventory reproducing on demand: `min_gap_s = 0.05` (`onset.rs:38`)
discards anything within 50 ms of the previous onset.

That is the "too few events" symptom, isolated and measurable.

### 3. Downbeat accuracy is 0 on every four-to-the-floor case

Expected: there is no downbeat stage at all (`docs/audio-pipeline-ist.md`
§4.2). `solder-groove` scores 1 by luck of phase, not by detection.

## Caveats on the metrics themselves

- **AMLt is lenient toward drift.** `circuit-breaker` scores 0.957
  AMLt with a 146 ms phase error, because the estimated tempo is
  0.22 % fast and the grid slides through alignment over 64 s. My
  `continuity_total` uses nearest-neighbour matching per annotation;
  a stricter MIREX implementation tracks a continuous run. Read AMLt
  here as "some metrical interpretation matches somewhere", not as a
  quality score.
- **Downbeat accuracy** currently asks only whether the grid's FIRST
  beat lands on a real downbeat, because there is no downbeat
  sequence to compare. It becomes a real sequence metric when Phase 4
  produces one.
- **Boundary metrics** are implemented but score 0 everywhere: only
  `filter-sweep` carries reference boundaries, and the pipeline emits
  no boundaries at all to compare against.

## The regression gate

`rock/` must not degrade by more than 2 %. Until real annotated rock
exists, the gate has two parts:

1. **Metric floor** on the two built-in songs, from the table above.
2. **Bit-identical chart output** for both built-ins — cheap, exact,
   and it catches any accidental behaviour change immediately, which
   a metric with 2 % slack does not.

Part 2 is what I would rely on for the early phases; part 1 becomes
meaningful once the rock phase error (finding 1) is understood, since
a fix there will legitimately move those numbers.

## Reproducing

```bash
cargo run -p beatbyte-audio --example baseline      # the table
cargo run -p beatbyte-audio --example phase_probe   # finding 1
cargo test -p beatbyte-audio --lib eval             # the metrics themselves
```

**Phase 1 ends here.** Nothing in the analysis pipeline was changed.
