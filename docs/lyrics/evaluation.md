# The aligner, measured

First measurement of the word-level aligner against published ground
truth (plan milestone L5). The rule for this round was: **measure
first, change nothing.** Nothing in the aligner was touched to produce
these numbers, and nothing has been tuned to the corpus.

## What was measured, and against what

- **Corpus**: JamendoLyrics MultiLang, 79 songs — 20 English, 20
  German, 20 Spanish, 19 French — with a hand-made onset and offset
  for every sung word. CC-licensed, part of it NC; it lives outside
  this repository (812 MB) and is pointed at through
  `BEATBYTE_LYRICS_CORPUS`.
- **Pipeline**: exactly what the game runs — `wav2vec2-base-960h` over
  the **mix** (no vocal separation; that is L6), one Viterbi over the
  whole song, then the confidence gate.
- **Metrics** (plan §2): AAE, the average absolute error between
  predicted and true word onset; PCO@τ, the share of words inside a
  tolerance. Coverage says how many truth words found a partner in our
  transcript at all — it was **100 % on all 79 songs**, so nothing
  below is an artefact of words going missing.

```
cargo build --release -p beatbyte-cli --features ml
beatbyte-cli models install wav2vec2-base-960h
beatbyte-cli lyrics-eval --corpus <corpus> --out report.json
BEATBYTE_LYRICS_EVAL_REPORT=report.json cargo test -p beatbyte-lyrics --test eval_gates
```

## The numbers

| Set | Songs | AAE mean | AAE median | PCO@0.1 | PCO@0.3 | lost |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| English | 20 | 7.40 s | 3.41 s | 42.1 % | 52.7 % | 7 |
| German | 20 | 4.43 s | 1.61 s | 47.6 % | 57.2 % | 5 |
| Spanish | 20 | 5.51 s | 1.84 s | 45.8 % | 56.8 % | 5 |
| French | 19 | 3.78 s | 1.84 s | 46.3 % | 57.0 % | 3 |
| **All** | **79** | **5.30 s** | **1.88 s** | **45.5 %** | **55.9 %** | **20** |

Against the plan's gates — AAE < 0.30 s, PCO@0.3 > 0.80, PCO@0.1 >
0.55 — this is a **FAIL on all three**, and the regression test says
so. Published systems reach AAE below 0.2 s on this corpus.

"Lost" counts songs whose average error passes 5 s: not mis-timed,
*lost*. There are 20 of them, and they are the whole story.

## The distribution has two humps, so the mean describes neither

Per-song AAE ranges from **0.05 s to 38 s**. Twenty songs meet the
0.30 s gate on their own; twenty are minutes out. Set the lost ones
aside and the remaining 59 read: AAE mean 1.42 s, median 0.76 s,
PCO@0.1 57.3 %, PCO@0.3 70.1 %.

That is why the aggregate now reports a median and a lost count beside
the mean. **The gates stay on the mean, as the plan set them** —
swapping the statistic under a gate would be moving it.

## Why songs are lost: the instrumental passages

Sorting the corpus by the longest stretch of music without a sung word
separates it almost cleanly:

| Longest instrumental gap | Songs | Lost | AAE median | PCO@0.1 |
| --- | ---: | ---: | ---: | ---: |
| under 10 s | 28 | 2 (7 %) | **0.28 s** | **59.1 %** |
| 10–25 s | 31 | 9 (29 %) | 2.09 s | 41.5 % |
| over 25 s | 20 | 9 (45 %) | 4.77 s | 32.5 % |

A song whose singing is continuous aligns **at the gate**: AAE median
0.28 s, PCO@0.1 59 % — both inside the plan's targets. Give the same
aligner a minute of guitars and it slides.

The mechanism is plain in the method. Forced alignment must lay every
word of the transcript along the audio in order; a long instrumental
has to be absorbed by CTC's blank state, and the model — hearing
guitars, not silence — keeps emitting letter probabilities there. Past
some length it becomes cheaper for the path to spend words in the
instrumental than to stay blank, words leak in, and everything after
slides. The three worst songs in the corpus have gaps of 66 s, 40 s
and 32 s; the three best have 0.9 s, 2.7 s and 5.1 s.

**Language barely matters** — German scores slightly *better* than
English (PCO@0.1 47.6 % vs 42.1 %) even though the model is English
only. That is not a fluke: with the transcript given, the path is
constrained by word order and by where the audio has voice-like
energy, so a wrong phone set degrades the alignment gracefully rather
than breaking it. It also means the multilingual work in L6 should be
judged on whether it fixes the *lost* songs, not on the language
averages.

## The confidence gate costs a little accuracy and buys honesty

The same 79 songs, measured with the gate off (`--raw`):

| | Songs | AAE mean | AAE median | PCO@0.1 | PCO@0.3 | lost |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| gated (what ships) | 79 | 5.30 s | 1.88 s | 45.5 % | 55.9 % | 20 |
| raw aligner | 79 | 5.31 s | 1.86 s | **47.2 %** | 56.8 % | 20 |

Per song the gate helps 27, hurts 18 and changes nothing for 34.

So the gate is **not** a timing improvement — it costs 1.7 points of
PCO@0.1. That is the expected direction and worth stating plainly: the
gate replaces word times it cannot vouch for with an even spread, and
an even spread scores worse against ground truth than the aligner's
own guess. What it buys is on screen, not in the metric: a line that
falls back stops pretending to know where each word is, instead of
filling confidently in the wrong place. The metric cannot see that
difference; a player can.

Nothing here is an argument for turning the gate off in the game.

## What this measurement does not say

- **The corpus is the hard case.** Its lyrics carry no line stamps at
  all. In the game they usually do (lrclib), and the gate uses them —
  a source stamp per line is exactly the anchor a sliding alignment
  lacks here. These numbers are therefore a floor, not the experience.
- **No separation.** Everything above is the aligner on the full mix.
  Removing the instruments is the plan's own next milestone.
- **Nothing was tuned.** No threshold was touched, no window changed.

## Where this points

In order of expected effect on the twenty lost songs:

1. **Vocal separation** (L6, planned): a stem where the instrumental
   passages really are quiet removes the mechanism above at its root.
   ⚠️ The runtime can carry it — `rten` implements `STFT`, `LSTM`,
   `ConvTranspose` and the rest — but as of 2026-09-05 no separator
   has been found whose *weights* may ship in an MIT project:
   open-unmix `umxl` is CC BY-NC-SA 4.0, `umxhq` states no weight
   licence, and Demucs licenses its code MIT while saying nothing
   about its models.
2. **Line stamps as anchors** (not in the plan): when the source has
   line stamps — the common case in the game — constraining the
   Viterbi to a window around each line would stop a slide from
   propagating past it. Cheap, local, and it uses data already in
   hand. It is also *measurable* here: JamendoLyrics carries
   `annotations/lines` beside the word annotations, which is the same
   shape lrclib hands the game, so the game's real case can be
   measured rather than guessed at.
3. **A blank prior in low-energy frames**: nudging the blank
   probability where nothing voice-like is present, which is the
   classic remedy for exactly this failure.

None of these were implemented in this round, on purpose.
