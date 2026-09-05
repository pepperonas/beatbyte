# Taking a whole library through the pipeline

What the imported library looked like before this pass, what was done
to it, what came out, and what the two columns in the song browser
mean. Written from the run itself — every number below was counted on
disk, not estimated.

## Before

71 song folders. Of those:

| | |
| --- | ---: |
| with lyrics beside them (`.lrc`) | 32 |
| with a word-level alignment (`words.json`) | 4 |
| with a redesigned chart (`chart.v*.json`) | 26 |

The four alignments were the songs used while building the aligner.
The 26 redesigned charts were the songs present when the difficulty
rollout ran; everything imported since had kept its first draft.

## What was done

**1. The charts.** `beatbyte-cli redesign` regenerates hard and expert
from the current pipeline and writes them as a new sibling version,
leaving easy and medium alone and moving the `chart-active.json`
pointer. 44 folders were rolled over; one was skipped and stays
skipped — see *the legacy folder* below.

**2. The missing lyrics.** For the 39 songs without any, lrclib was
asked — **with the song's own length as part of the question**. 20
came back with synced lyrics of a matching length. Five were refused
because the only synced entry under that name had a different length,
and three had unsynced words only; ten had nothing.

Refusing those five is the point, not a shortfall. See *the length
check* below.

**3. The alignment.** Every song with lyrics was run through
`beatbyte-cli align`: the acoustic model over the mix, one Viterbi
anchored to the source's line stamps, then the confidence gate. 52 of
52 songs with lyrics now carry a `words.json`.

## After

| | before | after |
| --- | ---: | ---: |
| songs with lyrics | 32 | **52** |
| word-level alignments | 4 | **52** |
| redesigned charts | 26 | **70 of 71** |

## What the aligner found in the library

The gate's verdict for each song, as stored in its `words.json`:

| Verdict | Songs | What it means, and what the game does |
| --- | ---: | --- |
| same master | 18 | Alignment and source agree. Word timing from the alignment. |
| shifted master | 12 | Same edit, different master — the source's stamps are seconds off. The alignment's times are used and the shift is reported. `Big Enough`'s lyrics were **7.2 s early**; every line had been showing seven seconds before it was sung. |
| different edit | 14 | The source's stamps belong to another recording. The alignment stands; the stamps are not used as a fallback. |
| failed | 8 | No consensus between alignment and stamps. Every line falls back to the source's own stamps — line-level karaoke rather than a confident wrong fill. |

The eight failures are dense rock mixes (Nirvana, Bon Jovi, Van
Halen, Green Day among them), which is exactly the failure the corpus
measurement predicts: the model cannot find the voice under the
guitars. They are **not** derailed — Nirvana's lines sit a median
1.6 s from their stamps with 2.4 s of scatter — they simply are not
precise enough to be trusted word by word, and the gate says so.

## The two columns

The browser carries the two states as separate columns, because they
are separate jobs with separate fixes:

| Column | Values | Reads |
| --- | --- | --- |
| `LYRICS` | `-` / `LINE` / `WORD` | no lyrics · lyrics with line timing · word- and character-timed |
| `CHART` | `BASE` / `v2`, `v3`… | the import's own first draft · which redesigned generation is active |

Amber marks work still to do, dim marks done. A song with **no
lyrics owes nothing** — its dash stays quiet rather than glowing
forever for something that cannot exist. Both columns sort: by
`LYRICS` the untimed songs float to the top, by `CHART` the first
drafts do.

## There is no database, and there should not be

A song is a **folder**:

```
<song>/
  Artist - Title.m4a        the audio
  chart.json                the import's own chart
  chart.v2.json             a redesigned generation
  chart-active.json         which one the game loads
  Artist - Title.lrc        the lyrics as the catalogue gave them
  Artist - Title.words.json the alignment (schema beatbyte.lyrics/1)
  Artist - Title.lyrics-offset.json   this song's own lyric offset
```

Copying a song copies everything it knows about itself. No migration
can corrupt it, no schema can drift from it, and every file opens in
a text editor. The scan reads the two column facts — is the active
chart a version, does a `words.json` sit beside the audio — exactly
the way it reads the title.

A server-backed store (Postgres or otherwise) would add a process the
player has to install and keep running, a migration story, and a
second source of truth that can disagree with the folder — for
seventy rows that change only when a file changes. The one thing a
database would buy, fast queries over a large library, is not a
problem this has: the scan of 71 folders is not measurable against
the time the game spends loading a single song's audio.

If the library ever grows to where scanning hurts, the answer is a
**cache** beside the settings — derived data, thrown away and rebuilt
whenever it disagrees with the folders — not a second home for the
truth.

## The length check

lrclib's `/api/get` takes a `duration` alongside artist and title, and
matches within two seconds. BeatByte now sends the song's own length,
so the catalogue answers about **this** recording.

⚠️ This is not defensive programming, it is a bug that happened. An
8:37 remix in this library — Annie's *Two of Hearts*, Skatebård's
remix — was handed the 4-minute original's lyrics, because the
catalogue does have the words under that name and nothing checked the
length. Every stamp was then wrong, and the confidence gate made it
worse: reading the disagreement as a failed alignment, it "fell back"
onto those stamps and crammed all 83 lines into the first 45 % of the
track, leaving 4:41 unsung. Two fixes came out of it:

1. The lookup sends the duration (and the batch fetch rejected five
   entries on exactly this basis).
2. A stamp grid that **stops far short of the audio** is now read as a
   different edit — the mirror of the existing rule for stamps
   running past the end. A long instrumental outro is not caught: the
   rule needs both a span under half the file and more than a minute
   of unstamped tail.

## The legacy folder

One folder, `girls-just-want-to-have-fun/`, holds `girls.chart.json`
and `girls.m4a` — a naming from before the version scheme. `redesign`
refuses it by design: versioning writes `chart.vN.json` beside a
`chart.json`, and inventing that name for a legacy layout would leave
the folder in a state neither convention describes. The song itself is
in the library twice; its proper folder
(`cyndi-lauper---girls-just-want-to-have-fun-m4a`) is redesigned and
aligned, and the browser's title+artist dedupe hides the duplicate.

## What is still open

- **Eight songs have no word timing** and sing at line level. The
  measured lever for them is vocal separation (plan milestone L6),
  which is blocked on model-weight licences, not on the runtime —
  see [`evaluation.md`](evaluation.md).
- **Fourteen songs' lyrics belong to another edit.** Their alignment
  stands on its own, which is the honest outcome, but a remix that
  repeats a verse the original's sheet contains once will leave that
  repeat unsung: no aligner can place a line twice, and the fix is a
  text that matches the edit.
- **The anchor window (±4 s) has not been tuned.** Nirvana's case
  suggests a tighter window might keep word timing where the current
  one gives up; that is a corpus measurement, not a guess, and it
  would mean re-aligning the library afterwards.
