# AI Song Graph Upgrade: word level lyrics and better charts

Status: plan — **L0 done 2026-09-05 including the decoder fix and the
chart migration (v0.14.10); L1 done 2026-09-05 (v0.14.11, ADR-0013:
`rten`, `beatbyte-ml`, `beatbyte-cli models`); L2 done 2026-09-05
(v0.14.12, `beatbyte-lyrics`, `beatbyte-cli align`, measured on the
raw mix — see CHANGELOG); L3 done 2026-09-05 (v0.14.13, `gate.rs`:
verdict on the source + per-word/per-line fallback; the "vocal energy
check" waits for the stem of L6, and "delta inconsistent" became
"no majority within tolerance" — a MAD threshold discarded the good
two thirds of a song whose choruses drift); L4 done 2026-09-05
(v0.14.14: `words.json` read by the game, character fill, real line
ends, lead-in setting, gap countdown, per-song offset in the pause
menu); L4b done 2026-09-05 (v0.14.15: §6 in the game — LYRICS MODEL
row with download/stop, `K` aligns in the browser, releases carry
`ml`; the separation switch and language row wait for L6)**, see
[`docs/audio/decode-offset.md`](../audio/decode-offset.md); §11 decided
2026-09-05: English first · separation its own switch, no cloud ·
character-level fill · lyric offset per song in the song folder
Author: Martin Pfeffer
Date: 2026-09-05
Location in the repo: `docs/plans/ai-song-graph-upgrade.md`
Scope: `beatbyte-audio`, `beatbyte-chart`, `beatbyte-game` (lyric rendering), `beatbyte-cli`, two new crates

---

## 0. The short version

**Claude Code CLI cannot do the conversion at runtime.** It is a coding agent, it cannot listen
to audio, and shelling out to it per song would break determinism, offline operation and the
"no accounts, no keys" promise in the README. Claude Code is the *builder* of this pipeline,
not a stage inside it.

The runtime intelligence comes from three ONNX/Rust models that run locally, offline and
deterministically, which is exactly the position BeatByte already claims:

| Job | Model | Rust access |
| --- | --- | --- |
| Vocals isolated from the mix | HTDemucs FT (ONNX) or an MDX model | `stem-splitter-core`, or `ort`/`rten` directly |
| Word and character timings for known lyrics | wav2vec2 CTC (EN) / MMS-FA (multilingual) | `wav2vec2-rs`, or own Viterbi over `ort` emissions |
| Beats and downbeats | Beat This! (ISMIR 2024) | `beat-this` crate, pure Rust `rten` backend |
| Note events (pitch, onset, offset) | Basic Pitch (Spotify, Apache 2.0) | `ort`/`rten` |

Cloud APIs stay as an **opt in fallback only**, for people whose machine is too slow, with the
key in the OS keychain and a visible marker in the UI. See section 8, including what that
forces you to change in the README and in `docs_stay_true.rs`.

Two work streams, independent of each other:

* **Track L (lyrics)**: fix the karaoke sync. This is the visible bug in the screenshot and it
  is the smaller of the two jobs. L1 to L3 alone already give KaraFun quality.
* **Track C (charts)**: better beat grid, better melody, better structure. Bigger, riskier,
  and gated by your own rule that chart feel is judged by ear against the
  `chart-feel-good-20260826` tag.

Do Track L first. It is self contained, measurable against a public ground truth corpus, and
it cannot regress a single note.

---

## 1. Why the lyrics are out of sync

Three separate causes, and they need three separate fixes. Do not fix them with one offset
slider.

### 1.1 The source has no word timings at all

lrclib serves plain lyrics and **line level** synced LRC. There is a newer word by word format
being introduced through LRCGET, but the catalogue is line level in practice. So every word
position you currently display is an interpolation across the line, which assumes a singer
distributes syllables evenly across a line. Singers never do. On a line like
"Ooh, wanna make her all your own?" the "Ooh" is usually held and the rest is fast, so the
fill runs ahead of the voice in the first half and lags in the second. That is exactly what a
KaraFun file does not do, because KaraFun files carry per syllable stamps.

**Fix:** forced alignment of the known lyric text against your own audio file. This is the
core of Track L.

### 1.2 The LRC was written against a different master

lrclib timings belong to whatever release the contributor had. A remaster, a radio edit, a
YouTube rip with a half second of silence in front, or a different intro all shift the whole
file. A constant offset can rescue this, but only if you measure it rather than let the user
hunt for it with a slider.

**Fix:** after alignment, compute the median delta between your aligned line starts and the
lrclib line starts. A large consistent delta means "different master", and you keep your own
timings. A large *inconsistent* delta means the alignment failed and you should fall back.

### 1.3 The decoder may add its own offset on `.m4a`

AAC in an MP4 container carries encoder delay (priming samples, classically 2112 for iTunes
encoders) plus gapless metadata (`iTunSMPB` or the `edts`/`elst` edit list). If Symphonia's
ISO-MP4 demuxer does not apply the edit list, everything you decode is shifted by roughly
20 to 50 ms against the timeline the LRC was written on, and worse, playback and analysis can
disagree if they take different paths.

**Fix, and do this one first because it is cheap and it is a test, not a guess:** synthesize a
click track with clicks at known times, encode it to `.m4a` with `afconvert` and with `ffmpeg`,
decode it through the exact same path the game uses, and measure the first click's sample
index. Record the result in `docs/audio/`. If there is a constant offset, correct it in the
decoder, not in the lyric layer. Add it as a test fixture next to the existing per format
decode tests.

---

## 2. Target metrics

Borrow the metrics the research field uses for audio to lyrics alignment, because then you can
compare yourself against published numbers instead of arguing about feel:

* **AAE**, average absolute error between predicted and true word onset, in seconds.
  Published systems reach below 0.2 s on the Jamendo corpus.
* **PCO@0.3 s**, percentage of correct onsets inside a 300 ms tolerance window. This is the
  standard, and it is far too loose for a karaoke fill.
* **PCO@0.1 s**, same thing at 100 ms. This is the one that decides whether the fill looks
  glued to the voice. Make it your primary number.

Targets to write into the eval harness:

| Metric | Gate (fail the test below this) | Goal |
| --- | --- | --- |
| AAE | < 0.30 s | < 0.15 s |
| PCO@0.3 s | > 0.80 | > 0.92 |
| PCO@0.1 s | > 0.55 | > 0.80 |
| Words marked "uncertain" | reported, not gated | < 10 % |

Ground truth: **JamendoLyrics MultiLang**, 79 songs in English, French, Spanish and German,
with released word level alignments, plus the Jam-ALT revision of the same songs for text.
German is in there, which matters for you. Do not commit the corpus, it is CC licensed with
NC terms on part of it and it is large. Point the eval at a local path via an env var and skip
the test when the path is unset, the same way a corpus based test should behave.

---

## 3. Architecture

### 3.1 Crate layout

Two new crates, so that `beatbyte-audio` keeps its fast test suite and zero heavy dependencies:

```
crates/
  beatbyte-ml/       model registry, download + checksum verify, ONNX/rten session cache,
                     tensor helpers, deterministic execution settings. No domain logic.
  beatbyte-lyrics/   lyric text acquisition and cleanup, forced alignment, word/char timing
                     model, Enhanced LRC read+write, confidence gating, fallbacks.
```

`beatbyte-audio` gains nothing but a stem cache API. `beatbyte-game` gains a renderer that
consumes word and character timings. `beatbyte-cli` gains `align`, `lyrics-eval` and
`stems` subcommands.

### 3.2 Feature gates, so the promise in the README survives

```toml
[features]
default = []              # cargo run --release -p beatbyte still needs nothing
ml = ["beatbyte-ml"]      # local models, downloaded on first use into the app data dir
cloud = ["ml"]            # optional remote providers, off unless a key is present
```

Models never go into the repository. They go next to settings, in
`~/Library/Application Support/beatbyte/models/` and the platform equivalents, downloaded on
explicit user action ("Enable smart lyrics" in settings), verified by SHA-256, and usable
offline forever after. This keeps repo size, the MIT licence story and the one command build
intact.

### 3.3 Determinism

Same rule as your chart generation: identical input, identical output, per platform.

* Pin model files by SHA-256 and record the hash in the output artifact.
* Fix the ONNX execution provider, thread count and graph optimization level in
  `beatbyte-ml`, do not let it pick per machine.
* Cache alignment output next to the audio, keyed by `audio_sha256 + model_sha256 +
  pipeline_version`. A chart or lyric track never silently changes under the player.
* Expect and document the same last bit divergence across platforms you already documented
  for `ln`/`exp`. Fingerprint per platform, do not claim cross platform identity.

---

## 4. Track L, the lyrics pipeline

### L0. Text acquisition (mostly exists)

Input priority: user supplied `.lrc` or `.txt` next to the audio, then the lrclib lookup you
already have. You need the **words**, not the timings. Plain lyrics are enough.

Cleanup before alignment, this is the one place where an LLM is genuinely the right tool
(text only, cheap, cacheable, no audio):

* strip section markers (`[Chorus]`, `[Verse 2]`), repeat markers (`(x2)`), and credits
* decide what to do with parenthesised backing vocals (default: keep, they are sung)
* normalise unicode quotes, dashes, umlauts
* for the multilingual aligner: produce the romanized form MMS-FA expects

All of that is doable with rules for 90 % of songs. Write the rules first, and only offer the
LLM path as the cleanup fallback behind the same `cloud` feature.

### L1. Vocal isolation

Run a separator, keep the `vocals` stem at 16 kHz mono for the aligner, cache it. Published
work is unambiguous that source separation improves both lyric transcription and alignment on
polyphonic music, and the difference is largest exactly where you need it, dense mixes with
loud guitars.

Rust options, in order of preference:

1. `stem-splitter-core` (Rust, ONNX Runtime, four stem Demucs class models). Note that it
   fetches models through a manifest URL, which is a network call you must put behind the
   explicit opt in.
2. Drive an HTDemucs FT ONNX export yourself through `ort`. More work, full control over where
   the file comes from and what gets verified.

Cheap fallback when the user declines the download: a mid/side plus band pass vocal emphasis.
Worse, but better than the raw mix, and it costs nothing.

### L2. Forced alignment, the core

CTC forced alignment against the known transcript. Not ASR. You already know the words, you
only need where they are, and constraining the decoder to the known text removes the entire
class of hallucination errors that plague Whisper on singing.

* English: wav2vec2-base-960h, 29 symbol character vocabulary, 20 ms frames, stride 320 at
  16 kHz. Emissions `[1, T, 29]`, Viterbi over the token sequence of the transcript.
* Other languages including German: MMS-FA (`mms-300m-1130-forced-aligner`), romanized input,
  1130+ languages. **Check the licence before you ship it**, the MMS lineage carries CC-BY-NC
  in places and BeatByte is MIT. If NC is a problem, the fallback is a per language wav2vec2
  CTC model, or shipping English only and letting other languages use the line level path.
* Long audio: 60 s windows with 50 s hop, centre crop, and one single Viterbi over the
  concatenated emission matrix so no word drifts at a chunk boundary.

Implementation choice:

* `wav2vec2-rs` does exactly this, with a Candle (pure Rust) and an ONNX Runtime backend, and
  it claims MFA quality word boundaries. It is young. Evaluate it for a day.
* If it does not hold up: the algorithm is not big. Emissions come from `ort` or `rten`, and
  CTC Viterbi with a reachability band is a couple of hundred lines with a clean test.
  Own it, it is the heart of the feature.

**The part most people miss:** CTC alignment is character level by construction. Every
character of the transcript gets a frame. Do not throw that away when you aggregate to words.
Keep the per character spans, because that is what drives a true per glyph fill instead of a
linear sweep across a word. Held notes and melisma are where a linear sweep looks wrong, and
character spans fix it for free.

### L3. Confidence gating and fallback

Never let a bad alignment ship a worse experience than the current interpolation. Per word:

* posterior confidence from the Viterbi path
* vocal energy check: does the word's span overlap voiced frames in the vocals stem
* monotonicity and a minimum duration per word
* line sanity: aligned line start compared against the lrclib line start

Rules:

* word below threshold: keep it, mark `estimated: true`, interpolate inside its line
* more than 30 % of a line uncertain: fall back to line level for that line only
* median line delta against lrclib above ~1.5 s but consistent: treat as a different master,
  keep your timings, log it
* delta inconsistent: alignment failed, fall back to the lrclib timings entirely

The fallback must be per line, not per song. A chorus that aligns is not worth throwing away
because a mumbled bridge did not.

### L4. Output format

Two artifacts, both cached next to the user's audio, never in the repository, because lyrics
are copyrighted and your README already commits to that.

**Canonical, internal:** `<song>.words.json`

```json
{
  "schema": "beatbyte.lyrics/1",
  "audio_sha256": "…",
  "pipeline_version": 1,
  "language": "en",
  "source": {
    "text": "lrclib:1234567",
    "separator": "htdemucs-ft@sha256:…",
    "aligner": "wav2vec2-base-960h@sha256:…"
  },
  "offset_ms": 0,
  "lines": [
    {
      "start": 44.120, "end": 47.980,
      "text": "Ooh, wanna make her all your own?",
      "words": [
        { "text": "Ooh,", "start": 44.120, "end": 44.910, "conf": 0.93,
          "chars": [[44.120,44.260],[44.260,44.480],[44.480,44.910]] },
        { "text": "wanna", "start": 44.950, "end": 45.240, "conf": 0.88, "estimated": false }
      ]
    }
  ]
}
```

**Interoperable, exported:** Enhanced LRC (A2), which your `beatbyte-chart` crate already
parses and tests:

```
[00:44.12] <00:44.12>Ooh, <00:44.95>wanna <00:45.24>make <00:45.51>her …
```

Export it so the user can take their work to any other player, and so you can import a hand
corrected file back. Character spans do not survive that round trip, that is fine, they are a
rendering nicety.

### L5. Rendering, the KaraFun feel

Alignment alone will not look like KaraFun if the renderer keeps its current rules. What
KaraFun actually does:

* the active line fills left to right per syllable, with a hard fill edge, not a fade
* the line has a real **end**, so it stops filling and dims rather than staying "in progress"
  until the next line begins
* the next line is already on screen and legible before it becomes active
* long instrumental gaps get a countdown, not an empty stage

Concrete changes:

1. Drive the fill from `chars` when present, from `words` otherwise, from the line only as a
   last resort. Never a linear sweep across a line that has word data.
2. Add `line.end`. You have it now, alignment gives it to you.
3. Lead in: make the active line appear `lead_in` seconds before its first word, default
   around 1.5 s, and make it a setting.
4. Gap > 4 s: show a countdown of three or four pulses on the beat grid, which you already
   have, tied to the same reconciled song clock the notes use.
5. A **lyric offset** in settings, separate from the audio latency calibration and the video
   offset, persisted per song, because sources vary. Never reuse the note offsets for this.
6. Highlight the sung word with a colour step, not only the fill, so it reads at speed on a
   1280 wide window.

### L6. Evaluation harness

Mirror what `beatbyte-audio` already does with MIREX beat scores.

```
beatbyte-cli lyrics-eval --corpus $JAMENDOLYRICS --out report.json
```

Reports AAE, PCO@0.1, PCO@0.3, per song and aggregated, plus the uncertain word rate. A
regression test reads the report and fails below the gates in section 2, and is skipped when
the env var is unset. Add three of your own songs as a small hand corrected fixture set so
something is always measured, even without the corpus.

---

## 5. Track C, better charts

Only start this after L is shipped. Every step here is A/B'd by ear against
`chart-feel-good-20260826` on real music before it touches a chart on disk, per your own rule.

### C1. Beat grid: Beat This!

Replace, or first only *compare against*, the autocorrelation tempo estimator with the
`beat-this` crate. ISMIR 2024, MIT, pure Rust `rten` backend, a 10 MB small model that can live
in the repo and an 83 MB full model that can be downloaded. It gives beats **and downbeats**,
which the current pipeline does not have at all.

Downbeats are worth more than tempo accuracy for a rhythm game:

* bar aware quantisation instead of beat aware
* Hype/energy phrases that start on a downbeat and last whole bars
* tempo drift handled, which autocorrelation with a single global BPM cannot do, and which is
  the reason live recordings and any human drummer currently drift out of grid late in a song

You already have a beat evaluation harness with MIREX scores. Run both trackers through it and
let the numbers decide, then confirm by ear.

### C2. Stems for note sources

With separation available from L1 you get four sources instead of one mix:

* `drums` for the percussive filler, far cleaner than spectral flux on a full mix
* `bass` for a possible bass difficulty or for lane low end anchoring
* `other`/`guitar` for the lead layer
* `vocals` deliberately excluded from note generation, but useful as a melody hint on tracks
  where the vocal *is* the hook

### C3. Melody: Basic Pitch instead of HPSS salience

Basic Pitch (Spotify, Apache 2.0, small ONNX graph) gives polyphonic note events with onset,
offset, pitch and confidence, plus pitch bends. Run it on the lead stem, not the mix.

This replaces the "per frame pitch salience tracked by dynamic programming" stage with real
note events. Lane mapping from the pitch contour stays exactly as it is, green low to orange
high, and sustains get their real held length from the note offset instead of from a decay
heuristic. Keep the existing path behind a flag so the A/B is a switch, not a rewrite.

### C4. Structure: repeated sections chart identically

A self similarity matrix over chroma or over the separator's latent features finds the
repeated chorus. Human charters chart a chorus once and paste it. Doing the same removes the
single most noticeable "this was generated" tell: the same chorus playing differently at
minute one and minute three. Pure Rust, deterministic, no model needed.

---

## 6. Settings and UI

New section in settings, off by default:

```
SMART LYRICS
  Local models …………… [ Not downloaded ] → [ Download 450 MB ]
  Vocal separation ……… [ On / Off ]        (slower, much better)
  Language …………………… [ Auto / English / German / … ]
  Lyric offset ………………  0 ms              (per song, separate from calibration)
  Realign this song ……  [ Run ]            (progress panel, cancelable)

  Cloud providers ………  [ None ]           (opt in, needs an API key)
```

Progress must be visible and cancellable. Alignment on CPU for a four minute song is seconds
for the aligner and tens of seconds to minutes for separation, depending on the machine. Run it
off the Bevy main thread, feed a progress channel, reuse the import batch panel you already
built.

---

## 7. What this costs in wall clock

Rough, per four minute song, on Apple Silicon CPU. Measure before you quote these anywhere.

| Stage | Order of magnitude |
| --- | --- |
| Decode + resample to 16 kHz mono | under a second |
| Separation (Demucs class, CPU) | 30 s to 3 min |
| wav2vec2 emissions, 60 s windows | a few seconds |
| CTC Viterbi | milliseconds |
| Beat This! (small model) | a few seconds |
| Basic Pitch on one stem | a few seconds |

Separation dominates everything. That is the setting that must be optional, and it is the one
that most justifies a cloud fallback for weak machines.

---

## 8. The cloud path, and what it costs you politically

Your README makes a strong, tested claim: exactly one request leaves the machine, the lrclib
lookup, plus the opt in LAN light service. `apps/beatbyte/tests/docs_stay_true.rs` checks the
network claim against the code. **The moment you add a cloud provider, that test and that
paragraph have to change in the same commit.** Do not let this arrive as a surprise in CI.

Design rules if you do it:

* Off unless the user enters a key. No key, no code path, no DNS lookup.
* Key in the OS keychain (macOS Keychain, Secret Service, Windows Credential Manager), not in
  `settings.json`. If you must use `settings.json` for a first version, say so in the UI and
  never log the value.
* The UI states, on the screen where the key is entered, exactly what is uploaded: the audio
  file or the isolated vocal stem, and to whom.
* Uploading a full commercial song to a third party is the user's decision to make, not a
  default. Uploading only the vocal stem is smaller and slightly less bad.
* Cache the result like any other artifact, so a song is uploaded at most once.

Providers worth supporting, in this order:

1. **ElevenLabs Scribe v2**: word level timestamps, 90+ languages, batch pricing around
   $0.22 per hour of audio at the time of writing. Currently the best accuracy in independent
   benchmarks.
2. **OpenAI `whisper-1`** with `timestamp_granularities: ["word"]`, $0.006 per minute. Note
   that the newer `gpt-4o-transcribe` models do **not** expose word level timestamps, so pick
   `whisper-1` deliberately.
3. A hosted separation API if you want to skip Demucs on weak machines.

And note what cloud ASR gives you: a transcript with word timings, which is **not** the same
as an alignment of the *official* lyrics. You still want to map that ASR output onto your known
lyric text by edit distance, and take the timings from the matched words. Otherwise the karaoke
text changes to whatever the model thought it heard, which for singing is regularly wrong.

---

## 9. Milestones and acceptance criteria

Each milestone ends with tests green, clippy clean, autopilot flawless, changelog entry and a
patch version bump, per your release rule.

| ID | Milestone | Done when |
| --- | --- | --- |
| L0 | m4a offset audit | Click track fixture decoded, offset measured and documented in `docs/audio/`, corrected in the decoder if non zero |
| L1 | `beatbyte-ml` skeleton | Model registry, download with SHA-256 verify, session cache, deterministic execution settings, no domain logic, unit tested with a dummy model |
| L2 | Aligner, English | `beatbyte-cli align song.m4a lyrics.txt` writes `words.json`; character spans present; runs offline after download |
| L3 | Gating and fallback | Per word confidence, per line fallback, master offset detection, all with unit tests on synthetic emissions |
| L4 | Renderer | Fill driven by characters, real line ends, lead in, gap countdown, lyric offset setting; screenshot harness updated |
| L5 | Eval harness | `lyrics-eval` command, gates from section 2 enforced as a test, three own fixture songs committed |
| L6 | Separation + multilingual | Vocals stem in the pipeline, German model path, licence checked and recorded in `docs/development/asset-licenses.md` |
| C1 | Beat This! A/B | Both trackers measured through the existing MIREX harness, decision recorded as an ADR |
| C2 | Stems into charting | Drums and lead stems feed onsets and melody, behind a flag |
| C3 | Basic Pitch melody | Note events replace salience tracking behind a flag, A/B'd by ear against `chart-feel-good-20260826` |
| C4 | Structure repetition | Repeated sections produce identical note patterns, verified on a song with an exact repeat |

An ADR belongs to at least: the decision to add local ML at all (L1), the aligner choice (L2),
the beat tracker swap (C1), and the cloud policy change (section 8) if you take it.

---

## 10. Risks

| Risk | Mitigation |
| --- | --- |
| `wav2vec2-rs` is young and may not hold up | Budget one day to evaluate, own the Viterbi if it fails; the model file is the asset, not the crate |
| MMS-FA licence may be non commercial | Check before shipping; English only path stays valid; per language wav2vec2 as alternative |
| Model downloads break the "no network" story | Explicit opt in, one time, verified hash, README and `docs_stay_true.rs` updated in the same commit |
| Separation is slow on old CPUs | Optional, cached, with a band pass fallback and a visible time estimate |
| Alignment fails on rap, screamed vocals, heavy effects | Per line fallback, uncertain rate reported, never worse than today |
| Determinism regressions from ONNX | Pin EP, threads, opt level; hash the model; fingerprint per platform like the chart tests already do |
| Repo size and MIT hygiene | No weights in git; licences recorded in `docs/development/asset-licenses.md` |

---

## 11. Open decisions, decide before starting

1. English only for the first release, or German from day one? German pulls in MMS-FA and its
   licence question.
2. Is separation part of the default smart lyrics path, or a second switch? It is the entire
   runtime cost.
3. Cloud providers at all in a project whose README sells "nothing leaves your machine"? A
   clean "no" is a legitimate answer and it saves a lot of surface.
4. Character level fill, or word level only? Character spans are free from CTC, but they
   change the renderer more.
5. Does the lyric offset live per song in the song folder, or in `settings.json` keyed by song
   hash?

---

## 12. Master prompt for Claude Code CLI

Paste this at the start of a session in the repository root. It is deliberately structured as
research, then design, then implementation, with the repo's own rules stated as constraints,
because the rules are what CI enforces.

````text
You are working in the BeatByte repository (Rust + Bevy, Cargo workspace, MIT).
Read CLAUDE.md, docs/plans/ai-song-graph-upgrade.md, docs/audio/, docs/chart-format/ and
docs/decisions/README.md before you write any code.

GOAL
Implement milestone <L0 | L1 | L2 | …> from docs/plans/ai-song-graph-upgrade.md.
Nothing beyond that milestone. If you find work that belongs to a later milestone, write it
down at the end of your report instead of doing it.

NON NEGOTIABLE CONSTRAINTS
1. Rust only. No Python, no Node, no build step beyond cargo. A user must still be able to run
   `cargo run --release -p beatbyte` on a clean clone with nothing else installed.
2. Default build stays offline and dependency free. All model code sits behind the `ml`
   feature; all remote providers behind `cloud`. Neither is in `default`.
3. No model weights in the repository. Downloads go to the platform app data dir, are verified
   by SHA-256, and only happen after an explicit user action.
4. Determinism: same input, same output, per platform. Pin execution provider, thread count
   and graph optimization level. Record model hashes in every artifact you write.
5. Untrusted input discipline, exactly as the existing chart validation does it: size caps,
   time range clamps, path traversal rejection. Lyric text and model files are untrusted.
6. Lyrics are copyrighted. Nothing lyric shaped is ever committed to this repository, and
   caches live next to the user's own audio file.
7. Tests: every new module gets unit tests. Update the counts in the README test table only
   through the mechanism `apps/beatbyte/tests/docs_stay_true.rs` expects, and run it.
8. Release rules: a user visible change bumps the patch version in the same commit and gets a
   CHANGELOG entry. Conventional commits.
9. If this milestone changes what leaves the machine, update the README "What leaves your
   machine" section and the network claim test in the same commit, and say so loudly in your
   report.

WORKFLOW, in this order, and stop at each checkpoint
A. RESEARCH. Read the existing code you are about to touch and summarize, in your own words,
   how it works today: the decode path, the current lyric path from lrclib to the renderer,
   and the analysis pipeline. Quote file paths and function names. Do not write code yet.
B. DESIGN. Propose the module layout, the public API, the data structures and the error type.
   State every assumption. Where the plan document and the existing code disagree, say so and
   propose which one wins. Write an ADR draft in docs/decisions/ if this milestone carries a
   real decision. STOP and wait for my confirmation.
C. IMPLEMENT. Smallest working slice first, with tests, then extend. Run
   `cargo test --workspace`, `cargo fmt --all`, and
   `cargo clippy --workspace --all-targets --all-features -- -D warnings` after every slice.
D. VERIFY. Run BEATBYTE_SMOKE_TEST=1 and BEATBYTE_AUTOPILOT=1. Report the numbers, not a
   summary of the numbers.
E. REPORT. What you built, what you measured, what you did not do, what you are unsure about,
   and the exact next milestone.

STYLE
Match the existing code. No new dependencies without naming the crate, its licence, its size
and why an existing dependency cannot do the job. Ask before adding one. Prefer pure functions
over sample buffers, which is the style the analysis code already uses.

WHAT I DO NOT WANT
Do not "improve" chart generation while you are in here. Do not touch the timing or judgment
code. Do not reformat files you did not otherwise change. Do not invent benchmark numbers:
if you did not measure it, say you did not measure it.
````

### Per milestone add on prompts

**L0, the m4a offset audit:**

```text
Milestone L0. Build a click track fixture: a WAV with impulses at exactly 1.000, 2.000 …
10.000 seconds. Encode it to .m4a (AAC) and to .mp3 with whatever encoder is available on this
machine, note which one you used. Decode all three through the exact path
crates/beatbyte-audio uses for analysis, and separately through the path used for playback.
Report, in samples and in milliseconds, where the first impulse lands in each case. Then tell
me whether Symphonia's MP4 demuxer applies the edit list / iTunSMPB priming skip in this
version. If there is a constant offset, propose the fix in the decoder and a regression test
that keeps it fixed. Do not change the lyric code in this milestone.
```

**L2, the aligner:**

```text
Milestone L2. Implement CTC forced alignment of a known transcript against a vocal signal.
Evaluate the crate `wav2vec2-rs` first: build a spike, align one of my songs, and report word
boundary quality and API fit honestly. If it does not fit, implement the Viterbi ourselves over
emissions from `ort` or `rten`; the algorithm is small and I would rather own it.
Requirements: 16 kHz mono input, 60 s windows with 50 s hop, one single Viterbi over the
concatenated emissions so no word drifts at a window boundary, per word confidence, and
per character spans preserved rather than aggregated away. Output the words.json schema in
section L4 of the plan. Add `beatbyte-cli align <audio> <lyrics>` behind the `ml` feature.
```

**L5, the eval harness:**

```text
Milestone L5. Add `beatbyte-cli lyrics-eval` computing AAE, PCO@0.1 and PCO@0.3 against a
word level ground truth corpus (JamendoLyrics MultiLang layout), reading the corpus path from
BEATBYTE_LYRICS_CORPUS and skipping cleanly when unset. Add a regression test with the gates
from section 2 of the plan. Report the per language breakdown, German separately. Do not tune
the aligner to the corpus in this milestone; measure first, and tell me what the numbers are
before you change anything.
```

---

## 13. Appendix

### A. Why forced alignment rather than transcription

Transcription asks "what is being sung", which on polyphonic music with singing voice is a
hard, error prone task, and Whisper in particular hallucinates through instrumental sections.
Forced alignment asks "where is this known text", which is a much smaller search problem with a
constrained output space. You already have the text from lrclib. Use it.

### B. Why line level LRC cannot be fixed by interpolation

Syllables per second is not constant inside a line, and the variance is the whole musical
point. A held first syllable and a fast run at the end is the single most common phrasing in
pop, and it is exactly the case where equal spacing looks the most wrong.

### C. References

* Beat This! Accurate Beat Tracking Without DBN Postprocessing, ISMIR 2024, CPJKU. MIT.
* Basic Pitch, Bittner et al., ICASSP 2022, Spotify. Apache 2.0.
* Exploiting Music Source Separation for Automatic Lyrics Transcription with Whisper,
  ICME workshop 2025.
* Jam-ALT: a readability aware lyrics transcription benchmark, ISMIR 2024.
* JamendoLyrics MultiLang, the standard corpus for word level lyrics alignment.
* Contrastive Learning Based Audio to Lyrics Alignment for Multiple Languages, ICASSP 2023,
  source of the AAE and PCO metric convention.
* torchaudio forced alignment API and CTC segmentation, for the algorithm reference.

---

© 2026 Martin Pfeffer | celox.io
