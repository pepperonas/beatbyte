# Audio pipeline — the state before any change (Phase 0)

Written 2026-09-01, reading only. No code was changed.

Placeholders resolved from the repository rather than guessed:

| Placeholder | Value | How it was established |
|---|---|---|
| `<REPO_PFAD>` | `/Users/martin/claude/beat-bytes` | working directory |
| `<ANALYSE_MODUL>` | `crates/beatbyte-audio/src/analysis/` | `mod.rs` there defines the `Analyzer` trait and the only implementation |
| `<TESTKORPUS_PFAD>` | **not resolvable** | no corpus directory exists anywhere in the tree, and none is referenced in code or docs. See "Open questions". |

---

## 1. The call chain, end to end

```
audio file
  └─ beatbyte_audio::decode_file                       decode.rs:184 (symphonia)
       → AudioData (MONO f32, source sample rate)      decode.rs:47
  └─ SpectralAnalyzer::analyze                         analysis/mod.rs:45
       ├─ downsample_half if rate ≥ 32 kHz             analysis/mod.rs:49  → ~22 kHz
       ├─ onset::analyze_onsets                        analysis/onset.rs:62
       ├─ tempo::estimate_tempo                        analysis/tempo.rs:49
       ├─ tempo::fit_beat_grid                         analysis/tempo.rs:140
       ├─ melody::extract_melody                       analysis/melody.rs:103
       └─ envelope::rms_envelope                       analysis/envelope.rs:8
       → SongAnalysis { bpm, bpm_confidence, alt_bpm,
                        beats, onsets, energy, melody } analysis/mod.rs:75
  └─ beatbyte_chart::generate_chart                    generate.rs:150
       ├─ generate_difficulty ×4                       generate.rs:189
       │    ├─ build_master                            generate.rs:239
       │    │    ├─ merge_candidates (onsets + melody) generate.rs:720
       │    │    ├─ select_candidates                  generate.rs:801
       │    │    └─ ContourMapper → lane               generate.rs:243
       │    └─ place_phrases                           generate.rs:1000
       └─ ChartFile
```

Entry points that run it: `beatbyte-cli` (`main.rs:536`), the game's
import (`beatbyte-game/src/import.rs`), and the built-in demo songs
(`beatbyte-audio/src/demo.rs:356`).

## 2. Stage by stage: method, parameters, thresholds

### 2.1 Decode — `decode.rs`
- `symphonia`, mixed down to **mono** (`AudioData` holds one channel,
  `decode.rs:47`).
- Sample rate is the file's own; **halved once** before analysis when
  ≥ 32 kHz (`mod.rs:49`), so a 44.1 kHz track is analysed at 22.05 kHz.

### 2.2 Onset detection — `onset.rs:62`
| Parameter | Value | Source |
|---|---|---|
| Window | 1024 samples ≈ **46 ms** at 22 kHz | `onset.rs:36` |
| Hop | 256 samples ≈ **11.6 ms** | `onset.rs:37` |
| Compression | `ln(1 + 50·|X|)` | `onset.rs:115` |
| ODF | half-wave-rectified **broadband** spectral flux, summed over ALL bins | `onset.rs:116` |
| Normalisation | divide by the **global** maximum of the whole track | `onset.rs:143` |
| Threshold | `1.3 × local median(±8 frames ≈ ±93 ms) + 0.02` | `onset.rs:38–39`, `onset.rs:176` |
| Peak picking | strict local maximum, then threshold, then `min_gap_s = 0.05` | `onset.rs:162–182` |
| Extra output | spectral **centroid** per frame ("brightness"), carried on each onset | `onset.rs:118–127` |

### 2.3 Tempo — `tempo.rs:49`
- **Autocorrelation of the broadband flux envelope**, mean-subtracted,
  normalised by lag 0 (`tempo.rs:70`).
- Search range **60–200 BPM** (`tempo.rs:27`), log-normal prior centred
  on **120 BPM, σ = 0.9 octaves** (`tempo.rs:29`).
- Parabolic interpolation around the winning lag (`tempo.rs:105`).
- `alt_bpm` reports the other octave when it scores > 50 % of the
  winner (`tempo.rs:126`) — **reported, never acted on.**

### 2.4 Beat grid — `tempo.rs:140`
- One global **period** (constant BPM) and one global **phase**.
- Phase: 64 candidates over one period, scored by Gaussian-weighted
  onset proximity (σ = **30 ms**), then one Gauss–Newton refinement
  step (`tempo.rs:150–187`).
- Beats laid across the whole duration from that phase.

### 2.5 Melody — `melody.rs:103`
- Window 2048 / hop 512, **HPSS by median filtering already exists
  here** (`melody.rs:188`): time-median = harmonic, frequency-median =
  percussive, soft Wiener mask `h²/(h²+p²)`, half-widths **4/4**
  (`melody.rs:83`).
- **Only the harmonic part is kept** (`harmonic_part`), for pitch
  salience over MIDI **40–88** (`melody.rs:87`).
- Output: pitched notes with start/end, used as chart candidates.

### 2.6 Energy — `envelope.rs:8`
RMS, 100 ms window / 50 ms hop (`mod.rs:69`).

### 2.7 Chart generation — `generate.rs`
- Candidates = onsets **merged with melody notes** (`generate.rs:720`).
- `select_candidates` (`generate.rs:801`) keeps a candidate when
  `strength ≥ 0.07` (`MASTER_STRENGTH_FLOOR`, `generate.rs:224`) and it
  is ≥ 0.10 s from the previous (`generate.rs:223`).
- **A held melody note suppresses percussive candidates underneath it**
  (`hold_until`, `generate.rs:809–823`), capped at four beats.
- Times are snapped to the musical grid (`quantize_musical`).
- Lanes come from the **melodic contour** where a pitch exists
  (`generate.rs:247`).
- "Phrases" = every 8 bars, a 2-bar window kept if it holds ≥ 4 notes
  (`generate.rs:1000–1019`).

## 3. Where the material profile a–g breaks this code

| # | Property | Where it breaks, concretely |
|---|---|---|
| **a** | Two timing rasters | `onset.rs:181` `min_gap_s = 0.05`: the two layers of one beat sit 10–40 ms apart, so the **second one is silently dropped** and its offset never reaches the grid. `tempo.rs:150` then fits ONE phase with σ = 30 ms over a bimodal distribution — the fit lands between the modes, off both. Nothing anywhere records a residual distribution. |
| **b** | Soft transients | `onset.rs:176`: `1.3 × median + 0.02` on a **globally normalised** flux (`onset.rs:143`). A track whose loudest transient is a hard hit rescales everything else down; a compressed 70s hit then sits under the threshold. `ln(1+50x)` (`onset.rs:115`) helps but is applied to a broadband sum where the loop's energy dominates. |
| **c** | Bit-identical loops | **No stage consumes self-similarity at all.** There is no SSM, no novelty curve, no changepoint detection. `place_phrases` (`generate.rs:1000`) is a fixed 8-bar stride with a note-count test — it cannot find a boundary, it only labels dense regions. |
| **d** | Filter sweeps | Two separate failures. (1) The sweep raises the **broadband** flux baseline for seconds; the ±93 ms median (`onset.rs:38`) is far too short to be a baseline estimator, so it rides up with the sweep and rejects real onsets. (2) The sweep itself is the biggest perceptual event and produces **no** entry anywhere: `brightness` is computed (`onset.rs:118`) and attached per onset, but no stage looks at its *trend*. |
| **e** | Harmonic stasis | `melody.rs` will happily track the vamp, and `generate.rs:809` then lets a held vamp note **suppress the drums under it** for up to four beats. On this material that is a direct cause of "too few events" — the loop is exactly what holds. |
| **f** | No accent hierarchy | **There is no downbeat detection at all.** `offset_s` is simply `beats.first()` (`generate.rs:152`), i.e. the first beat of an arbitrary phase — bar 1 is never determined. Octave errors: the prior (`tempo.rs:29`, σ = 0.9 oct) is weak, and `alt_bpm` is computed and then **ignored** (`tempo.rs:126`). |
| **g** | 6–7 min DJ format, 32-bar drum-only edges | `onset.rs:143` normalises by the **global** max, so a quiet intro is scaled against the loudest drop. `place_phrases` (`generate.rs:1008`) starts at a fixed 8-bar stride regardless of where music actually starts. Nothing distinguishes an intro from a drop. |

## 4. Where the assumed pipeline does **not** match reality

The brief assumes: *Spectral Flux → Peak Picking → Autocorrelation →
Beat-Tracking → Downbeat → SSM-Segmentation.* Differences:

1. **There is no beat TRACKING.** No dynamic programming, no
   beat-by-beat agent. One constant period plus one global phase
   (`tempo.rs:140`) — which is, in effect, already the "global fit" of
   Phase 3, only much coarser (64 phase candidates, no period search,
   no band weighting).
2. **There is no downbeat stage.** Not weak — absent.
3. **There is no SSM and no segmentation.** `place_phrases` is a
   fixed-stride density filter, not a boundary detector.
4. **HPSS already exists** (`melody.rs:188`) with a soft mask, exactly
   the Fitzgerald method Phase 2.1 asks for — but it is applied only in
   the melody stage and only its **harmonic** half is used. The
   percussive half is computed and thrown away.
5. **The ODF is broadband and single.** No bands, no SuperFlux, no
   complex-domain ODF.
6. **A pitch/melody stage sits between onsets and notes** and can
   *suppress* percussive events (`generate.rs:809`). The brief's model
   has no such stage; on this material it is a first-order effect.
7. **Spectral centroid is already computed per frame** (`onset.rs:118`)
   — the feature Phase 5.4 wants for filter sweeps is present and
   unused beyond a per-onset tag.
8. `alt_bpm` and `bpm_confidence` are produced but **no consumer acts
   on them** (`mod.rs:60`).

## 5. Open questions before Phase 1

1. **The test corpus does not exist.** No `rock/`, `house-modern/` or
   `house-sample/` directory is in the tree or referenced anywhere.
   Phase 1 needs ≥ 15 tracks WITH ground truth (Ableton `.asd`,
   Rekordbox XML, or the JSON fallback). I can build the harness, the
   importers and the metrics; I cannot supply the audio or the
   ground-truth grids. **Where should the corpus live, and which of
   the three sources will you export?**
2. **Ground truth for `rock/`**: the current quality is "good" by ear,
   not by measurement. Without reference grids for the rock tracks the
   2 % no-regression gate has nothing to compare against. Is
   *bit-identical chart output* an acceptable rock gate for the early
   phases (cheap, exact, catches any accidental change), with measured
   metrics once grids exist?
3. **`ort` / `ml-downbeat`**: the workspace is currently
   dependency-light and ships one binary per platform. Adding ONNX
   runtime behind a feature flag is fine for a local build, but the
   release workflow builds all platforms — should the flag stay off in
   CI and releases?
4. The 10 s budget for a 7-minute track: current end-to-end time is
   unmeasured. I will measure it as the first act of Phase 1 so the
   budget has a baseline.

**Phase 0 ends here. No code changed.**
