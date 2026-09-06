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

## With the source's line stamps: no song is lost any more

The numbers above deliberately withhold the one thing the game almost
always has. JamendoLyrics' lyrics carry no stamps, but the corpus does
ship `annotations/lines` — the same shape lrclib hands the game — so
the real case can be measured instead of guessed at. The stamps are
handed to the aligner **made imperfect on purpose** (a deterministic
wobble of up to ±0.5 s per line), because a corpus annotation is exact
and an lrclib stamp is not; measuring against ground truth smuggled in
through the back door would prove nothing.

Each line's words are then confined to the room between its own stamp
and the next, plus four seconds at each end, with a constant shift
taken from the unanchored pass when that pass agreed with itself.

| | Songs | AAE mean | AAE median | PCO@0.1 | PCO@0.3 | lost |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| no stamps | 79 | 5.30 s | 1.88 s | 45.5 % | 55.9 % | **20** |
| **with line stamps** | 79 | **0.64 s** | **0.45 s** | **49.5 %** | **65.9 %** | **0** |

Per language, with stamps: English 0.65 s / 46.8 %, German 0.69 s /
49.8 %, Spanish 0.54 s / 51.7 %, French 0.68 s / 49.7 % — the same
everywhere, as before.

**Not one song of the 79 is lost.** The mean error falls by a factor
of eight, PCO@0.3 gains ten points, and the songs that meet the 0.30 s
gate on their own go from 20 to 28. This is the measured answer to the
question the first table raised, and it needed no model, no download
and no new dependency: the slide cannot travel past the next stamp.

Two honest qualifications. First, **PCO@0.1 gains only four points**
(45.5 → 49.5 %): anchoring puts the LINE where it belongs, it does not
teach the model to hear the individual words, and fine placement is
still where the aligner is weak. Second, **the share of words the gate
marks estimated rises from 12.5 % to 32.4 %** — inside a bounded window
the path more often has to sprint through words, and the gate says so
rather than pretending. A line in the right place with an honest
line-level fill beats a line thirty seconds out with a confident wrong
one, but it is not the same as knowing every word.

### How wide the window is: measured, and the obvious answer was wrong

The window each line's words are confined to started at ±4 s. Sweeping
it over 26 corpus songs said the opposite of what shipped:

| window | source on time | source 3 s off |
| --- | --- | --- |
| ±1 s | AAE 0.573 s, PCO@0.1 **51.7 %**, 0 lost | AAE 1.818 s, PCO@0.1 38.4 %, **1 lost** |
| ±2 s | AAE 0.585 s, PCO@0.1 49.5 %, 0 lost | — |
| ±4 s | AAE 0.627 s, PCO@0.1 46.7 %, 0 lost | AAE 0.998 s, PCO@0.1 45.4 %, 0 lost |
| **by agreement** | AAE 0.601 s, PCO@0.1 **48.8 %**, 0 lost | AAE 0.972 s, PCO@0.1 **47.4 %**, 0 lost |

On stamps that are on time, tighter is better on every number, and the
share of words the gate cannot vouch for falls from 34.4 % to 17.3 %.
Put the same stamps three seconds off and it reverses — a window that
cannot hold the truth forces words somewhere they are not, and a song
is lost outright.

So the width follows what is known. Once the unanchored pass has
**agreed** on the source's offset, that offset is removed and the
window closes to ±1 s; while it is unknown the window stays ±4 s and
holds an offset nobody has seen. That beats the old fixed ±4 s in both
conditions without ever losing a song.

⚠️ The first sweep alone would have made this change wrong. Twelve
songs in the test library sit on another master, and a fixed ±1 s
would have degraded exactly those. The second condition was run
because they exist.

That still did not reach ±1 s on the on-time case (48.8 % against
51.7 %), and the mechanism said why: a song whose unanchored pass
derailed never agrees, so it never gets the tight window — even though
the anchored pass would then place it well. So the anchored pass is
now asked in turn:

| | source on time | source 3 s off |
| --- | --- | --- |
| two passes | PCO@0.1 48.8 %, PCO@0.3 66.1 %, uncertain 30.2 % | 47.4 %, 62.0 %, 29.5 % |
| **three passes** | **50.3 %, 67.9 %, 25.8 %** | **48.0 %, 62.8 %, 27.0 %** |

Better on every number in both conditions, no song lost, and the
share of words the gate cannot vouch for falls by four points. It
closes most of the gap to a fixed tight window without taking on that
window's failure when the source is off. The third pass runs only for
the songs whose first pass could not see the offset.

Anchoring engages only when the stamps are structurally plausible for
the file — enough of them, rising, and spanning it. That check is
deliberately **not** the gate's verdict: the gate judges by how well
the unanchored pass agreed with the stamps, and the songs that need
anchors most are exactly the ones where that pass derailed.

A whole library taken through this pipeline, with the numbers it
produced, is written up in [`library-pass.md`](library-pass.md).

## An alignment is only evidence if the model heard the song

The anchored pass has a failure mode that looks exactly like success,
and a real song walked into it.

Böhse Onkelz' *Mexico* is a loud German rock mix under an English
character model. Over six seconds of singing the model's greedy
reading is the single letter `I`; over the whole song it produces
**0.18 letters a second**, against 2.0 to 4.4 for the songs the
aligner places well. There is nothing in those emissions to prefer one
position over another.

The alignment came back anyway — they always do — and the gate read
it as a **shifted master: 39 lines compared, 85 % of them agreeing on
−3.34 s, a scatter of 0.36 s.** Every one of those numbers is an
artefact of the pipeline talking to itself:

1. The unanchored pass, with nothing to hold on to, derails and its
   median delta is noise. Here it was about −3.3 s.
2. The anchored pass centres each line's window on *that* number.
3. With no acoustic preference, the words land inside their windows —
   so the deltas reproduce the guess, and "consensus" measures the
   window rather than the audio.

The lyrics then ran three and a bit seconds early on screen, with a
confident verdict beside them.

So the gate now asks a question the alignment cannot answer about
itself: **did the model hear anything at all?** Both measures are read
off the emissions the aligner has already computed, so they cost
nothing:

| Measure | What it is |
| --- | --- |
| voiced share | frames where the model is surer it heard a letter than nothing (`p(blank) < 0.5`) |
| **letters per second** | letters in the model's own greedy reading, after the CTC collapse |

The second is the sharper one — it counts what the model would
*write*, not merely where it hesitated — and it is the one the gate
uses. Below `min_letters_per_s` the alignment may not outvote the
source's stamps: the verdict is `Failed`, every line falls back to the
stamps a human made, and the song sings by the line. The file records
the measurement, so a song can say why it fell back.

### Where the floor sits, and why that number

Not at the elbow of the library's distribution — against **measured
word error**. The corpus knows every word's true onset, so the raw
aligner was run over all 79 songs with the legibility of each
recorded beside its score:

| Floor | songs below | median AAE below | songs above | median AAE above |
| ---: | ---: | ---: | ---: | ---: |
| 0.8 | 14 | 13.57 s | 65 | 0.94 s |
| **1.0** | **17** | **15.46 s** | **62** | **0.78 s** |
| 1.2 | 18 | 13.57 s | 61 | 0.76 s |
| 1.6 | 22 | 10.62 s | 57 | 0.71 s |
| 2.0 | 24 | 10.62 s | 55 | 0.62 s |

**1.0** is where the contrast is widest: a factor of twenty. Raising
it further buys a little accuracy above and demotes many more songs
below — and each demoted song loses word-level karaoke it may have
deserved.

⚠️ **It is a floor, not a predictor.** *Songwriterz — Back In Time*
reads 0.65 letters a second and aligns to within a second;
*Color Out — Falling Star* reads 1.74 and is 18 s out. The claim is
only the negative one: below the floor an alignment carries no
information worth setting against a human's stamps. Seventeen of the
79 corpus songs are below it, and their median error is fifteen
seconds — that is not timing, it is noise with a verdict attached.

⚠️ **Word confidence does not work for this.** It was the obvious
candidate and the measurement rejected it: across this library the
median word confidence of the songs that aligned well overlaps the
ones that did not (Mexico 0.013 sits between two `same master` songs
at 0.004 and 0.013). Confidence is relative to a mix; the greedy
letter rate is not.

⚠️ **Nor does the size of the claimed shift.** A shift near the
anchor window's width looks like the window's edge — but real shifts
of 7 s, 10 s, 32 s and 81 s exist in this library and are correct.

### The cheap fallback does not work, and that is measured too

The plan carries a no-download fallback for exactly the songs below
the floor: mid/side plus a band-pass to favour the voice. Both halves
are now answered.

**The mid/side half is already in effect.** `decode_file` averages
the channels, so every analysis here has always run on the mid
channel — the one that favours a centred vocal. There is nothing
left to take.

**The band-pass half makes things worse.** A 200–3500 Hz pass over
five songs from the bottom of the distribution and one legible
control: 0.03 → 0.02, 0.18 → 0.13, 0.16 → 0.01, 0.49 → 0.34,
0.75 → 0.77, and the control 2.03 → 1.63. The model is trained on
full-band speech, and a rock mix keeps its guitar and snare inside
the voice's own band — the filter removes context rather than
instruments.

So the twenty-two songs below the floor are not a tuning problem.
They need the voice actually separated from the mix, which is
milestone L6 and blocked on weights this project may ship.

## With a vocal stem

The library's third round (`library-pass.md`) aligned every song on a
vocal stem a local separator made (ADR-0014), and the rules that
round produced — the pushed-window check, the warp, the parked word,
the sound's end — are in the pipeline the corpus measures. Here is
the stem condition on the same 79 songs, same code, same truth:
`lyrics-eval --vocals-dir` with demucs' `htdemucs` vocals (on the
game's timeline, lag 0 measured), against the mix. "On time" is the
anchored pass with the corpus's line stamps wobbled by ±0.5 s as
above; "3 s / 6 s off" moves every stamp by that much, the condition
twelve songs in the real library are actually in; "raw" is the plain
pass with no stamps at all.

| Condition | AAE mean | AAE median | PCO@0.1 | PCO@0.3 | derailed | estimated | legible (≥ 1.0) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| mix, on time | 0.650 s | 0.444 s | 49.4 % | 65.8 % | 0 | 35 % | 62 / 79 |
| mix, 6 s off | 2.787 s | 0.506 s | 42.8 % | 52.6 % | **24** | 40 % | 62 / 79 |
| **stem, on time** | **0.174 s** | **0.111 s** | **77.8 %** | **90.6 %** | 0 | 14 % | **79 / 79** |
| stem, 3 s off | 0.174 s | 0.111 s | 77.8 % | 90.6 % | 0 | 14 % | 79 / 79 |
| stem, 6 s off | 0.180 s | 0.121 s | 77.9 % | 90.6 % | 0 | 14 % | 79 / 79 |
| stem, raw (no stamps) | 0.195 s | **0.082 s** | **83.3 %** | **94.3 %** | 0 | 0 % | 79 / 79 |

Per language on the stem, on time: English 0.226 s / 88.2 %, German
0.118 s / 95.0 %, Spanish 0.151 s / 90.2 %, French 0.202 s / 88.8 %
(AAE / PCO@0.3) — the German rock mix that read at 0.18 letters a
second is the language the stem serves best.

What the table says, in order of weight:

1. **The plan's gates pass with a stem, and only with a stem.** AAE
   0.174 s against the 0.30 s gate, PCO@0.3 90.6 % against 0.80,
   PCO@0.1 77.8 % against 0.55 — all three, where the mix fails all
   three (0.650 s, 65.8 %, 49.4 %). Separation was the plan's own
   prediction for this; the measurement confirms it by a factor of
   nearly four on the mean error.
2. **The stem makes the stamps almost irrelevant.** Six seconds of
   stamp error costs the stem 6 ms of mean error and no song; the
   same error on the mix derails 24 songs. The warp and the
   agreement rules were built on the library for exactly this case
   and they hold on the corpus.
3. **The model hears every song.** Legibility rises from a median of
   3.1 to 4.7 letters a second; 68 of 79 songs read more, 17 cross
   the floor upward and none downward; 74 of 79 have a smaller error.
   The 17 songs the mix could not vouch for are the corpus's version
   of the library's 27.
4. **The plain pass on a stem is the best fine placement of all**
   (median 0.082 s, PCO@0.3 94.3 %, nothing estimated) and a slightly
   worse mean (0.195 s: a few songs a line or two out, with no stamp
   to hold them). On a stem the stamps buy robustness, not accuracy —
   which is the right thing for stamps of unknown quality to buy.

The plan's own gate test agrees:
`BEATBYTE_LYRICS_EVAL_REPORT=<stems-ontime.json> cargo test -p
beatbyte-lyrics --test eval_gates` passes on the stem report and fails
on the mix report with "AAE 0.650 s is not under 0.3".

What it does not say: the separator is not in the tree (its weights'
licence, ADR-0014), so this is the condition a user creates by hand
with a local tool, not the one the game ships. The six runs shared
one binary and one pipeline — the queue was restarted after the last
code change and the lyrics crates did not change while it ran.

## What this measurement does not say

- **The first table is the hard case**, and the section above is the
  ordinary one: the corpus's lyrics carry no line stamps, the game's
  almost always do.
- **No separation in the game.** The first tables are the aligner on
  the full mix; the stem section is a condition a local tool creates,
  and the game does not ship the tool.
- **Nothing was tuned.** No threshold was touched, no window changed.

## Where this points

In order of expected effect on the twenty lost songs:

1. **Line stamps as anchors — done, measured above.** Not one song
   lost, mean error down by a factor of eight, at no cost in
   downloads or dependencies. It is on in the game whenever the
   source's stamps are plausible.
2. **Vocal separation** (L6, planned): a stem where the instrumental
   passages really are quiet removes the mechanism above at its root
   — and it is the only lever left for PCO@0.1, which anchoring
   barely moved.
   ⚠️ The runtime can carry it — `rten` implements `STFT`, `LSTM`,
   `ConvTranspose` and the rest — but as of 2026-09-05 no separator
   has been found whose *weights* may ship in an MIT project:
   open-unmix `umxl` is CC BY-NC-SA 4.0, `umxhq` states no weight
   licence, and Demucs licenses its code MIT while saying nothing
   about its models.
3. **A blank prior in low-energy frames**: nudging the blank
   probability where nothing voice-like is present, the classic
   remedy for this failure. The model already knows where the singing
   is — `cargo run -p beatbyte-lyrics --example voice -- <audio>`
   prints it per second — the aligner simply does not use it yet.
   This is the remaining lever for songs whose source has no stamps
   at all.
