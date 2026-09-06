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
| same master | 20 | Alignment and source agree. Word timing from the alignment. |
| shifted master | 15 | Same edit, different master — the source's stamps are seconds off. The alignment's times are used and the shift is reported. `Big Enough`'s lyrics were **7.2 s early**; every line had been showing seven seconds before it was sung. |
| different edit | 9 | The source's stamps belong to another recording. The alignment stands; the stamps are not used as a fallback. |
| failed | 8 | No consensus between alignment and stamps. Every line falls back to the source's own stamps — line-level karaoke rather than a confident wrong fill. |

Words the gate cannot vouch for, averaged over the 44 songs that did
not fail: **24.2 %**, down from 32.9 % before the third alignment pass
— the corpus predicted the same direction (25.8 % against 30.2 %).
The verdict tally did not move: the third pass sharpens the songs the
aligner already understood rather than rescuing the ones it did not.

### A second pass over the library

Those numbers are after a second round. The first said 18 / 12 / **14**
/ 8, and the fourteen "different edit" songs were worth another look:
their lyrics had been fetched **before** the length check existed. Asking
the catalogue again, this time for an entry of *this* song's length,
found a better one for nine of them; re-aligning moved **five** into
usable verdicts (two same master, three shifted master). Four stayed on
another edit — for those the catalogue simply has no entry matching the
recording, and a duration-matched text is still the better basis than
the one that was there.

The same round re-aligned every song with the anchor window that now
follows what is known about the source (see
[`evaluation.md`](evaluation.md)); the eight failures stayed eight.

The eight failures are dense rock mixes (Nirvana, Bon Jovi, Van
Halen, Green Day among them), which is exactly the failure the corpus
measurement predicts: the model cannot find the voice under the
guitars. They are **not** derailed — Nirvana's lines sit a median
1.6 s from their stamps with 2.4 s of scatter — they simply are not
precise enough to be trusted word by word, and the gate says so.

## The two columns

The browser carries the two states as separate columns, because they
are separate jobs with separate fixes — and each draws a **mark**,
not a word, so a glance down the list answers "what has the AI been
through" without reading:

| Column | Mark | Reads |
| --- | --- | --- |
| `LYRICS` | nothing | no lyrics beside the audio — nothing is owed |
| | dim microphone | lyrics with the source's line stamps only |
| | lit microphone with two waves | every word placed by the aligner |
| `CHART` | one dim bar | the import's own first draft |
| | two to four lit rising bars | a redesigned generation, one bar per generation |

**Lit means the pass has been through; dim means it has not.** The
waves and the bar count carry the same message as the colour, on
purpose: a mark that is only a colour says nothing to a player who
cannot tell those two apart. The marks are node art — the 8-bit face
has no symbol glyphs, so the microphone is drawn from boxes the way
every other shape in this list is.

The two are **independent**: a song can sing word by word off a
first-draft chart, and a redesigned chart can have no words at all.
Both columns sort: by `LYRICS` the untimed songs float to the top, by
`CHART` the first drafts do.

## Asking the catalogue better

The first pass asked once, with the names exactly as the import left
them and the song's length as a hard filter, and 19 songs came back
empty. Eight of those nineteen were our question's fault:

| Why it missed | Songs |
| --- | --- |
| a download's furniture in the names (`- OFFICIAL VIDEO`, a channel as the artist, a fullwidth comma) | MANOWAR, and two others |
| our rip is seconds off every catalogue entry, and `get` matches within two | Bloodhound Gang (15 s), Metallica (20 s), and three more |

The lookup now asks in three steps — the names as they are, then
without the furniture, then a **search** judged by our own length
rule, which is the larger of 12 s and 8 % of the song. One absolute
number cannot hold both ends: 15 s on a 245-second rip is the same
recording, 278 s on a 517-second remix is a different sheet.

All eight were then aligned. Three sing word by word; Bloodhound
Gang's sheet arrived **8.70 s off** and the aligner put it right —
which is exactly the case the anchoring exists for; three the model
cannot read fell back to their stamps.

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

## What the model could not hear

The pass above trusted every alignment it produced. It should not
have: on a mix the model cannot read, a forced alignment still
returns a path, and the anchored pass then reproduces whatever shift
it was centred on. *Mexico* came back "shifted master, 85 %
agreement" and ran 3.3 s early on screen. The mechanism, the measure
that catches it and the two deliberate exceptions are in
[`evaluation.md`](evaluation.md#an-alignment-is-only-evidence-if-the-model-heard-the-song).

Measured over all 52 songs with lyrics, as letters per second in the
model's own greedy reading:

| | letters/s |
| --- | ---: |
| lowest (Dragonforce, Onkelz, MGMT) | 0.00 – 0.03 |
| median | 1.23 |
| highest | 8.70 |

The bottom of that range is not a language problem alone — Cyndi
Lauper (0.39), Nirvana (0.49) and Bon Jovi (0.16) sing English. It is
loudness: a dense mix buries the voice, and the eight songs that
already failed to align sit there.

**22 of the 52 read below the floor of 1.0.** Their alignments may no
longer claim a shift against the source's stamps, and their word
times are marked estimated — those songs sing by the line, which is
what the pipeline actually knows about them.

Re-aligning the whole library with the rule in place:

| Verdict | before | after |
| --- | ---: | ---: |
| same master | 20 | 10 |
| shifted master | 15 | 13 |
| different edit | 9 | 9 |
| **failed → the source's stamps** | **8** | **20** |
| | | |
| songs singing word by word | 44 | 28 |

Only **two** songs' lines actually moved on screen, and both moved
back onto the human's stamps: *Mexico* (18.02 → 21.69 s, the 3.3 s
the bug report described) and Moby's *Lift Me Up* (9.6 → 12.7 s).
The rest of the change is honesty rather than motion — a song at
0.03 letters a second was already showing its lines at the right
times, it was just claiming to know where each word sat inside them.

⚠️ A song judged a **different edit** keeps its own span, and with
every word now estimated that span can start earlier than before:
Ozzy's *Crazy Train* moved from 6.38 s to 0.46 s, because the old
figure was the gate's own repair of a 5.3-second-long "All" rather
than a measurement. 0.46 s is where the raw aligner puts the opening
"All aboard!", which is also where it belongs.

⚠️ **The measure has a known confound**: a song with long
instrumental passages produces fewer letters a second even when its
vocal is perfectly legible. That is why the floor was placed against
measured word error on the corpus rather than at the elbow of this
distribution — see
[`evaluation.md`](evaluation.md#where-the-floor-sits-and-why-that-number).

## The third round: every song with lyrics sings word by word (2026-09-06)

Run with the vocal stems and the rules they made necessary
([`optimizing-a-library.md`](optimizing-a-library.md)): every song
with lyrics separated by demucs on this machine, aligned on the stem
with `--keep-better`, then the songs the first pass left unconfirmed
taken through one more pass each as the rules were found. Counted on
disk afterwards:

| | after round two | after round three |
| --- | ---: | ---: |
| songs singing word by word | 33 | **60** |
| songs singing by the line | 27 | **0** |
| songs without lyrics | 11 | 11 |
| alignments computed on a stem | 0 | 59 |

The gate's verdicts, as stored:

| Verdict | Songs | Note |
| --- | ---: | --- |
| same master | 29 | |
| shifted master | 21 | four of them with the album's last lines dropped as unsung |
| stretched | 5 | *Mexico* (1.0388 · t − 10.88 s, 24 of 39 lines sung), *Hotel California* live 1977 (1.0095 · t + 1.69 s), *Terpentin*, *Easy Lover* (0.9599 · t), *An Tagen wie diesen* (1.0987 · t) |
| different edit | 5 | the aligned times stand; see below |
| failed | 0 | |

What moved the twenty-seven songs, in the order the rules were found:

1. **The stem.** Twenty-three songs had failed on the mix because the
   model could not read it (0.03–0.9 letters a second); on the stem
   nineteen of them read 1.9–11.8 and aligned outright.
2. **Stamps from another edit** (*Mexico*, the case that opened the
   round): the sheet of the 254 s studio version on a 168 s recording
   with 115 s of digital silence appended; mapped, the unsung tail
   dropped, judged in sync by ear.
3. **A held final word parked at the next line's onset** ("sein",
   14 s late): the parked-word rule.
4. **Texts with more verses than the recording** on a source that
   otherwise agreed to within a tenth of a second (*Life Is a
   Flower*, *Maria*, *Through the Fire and Flames*, *For You*): the
   lines past the sound's end dropped instead of the whole source
   being called another edit.
5. **A coarse drift** (*Fettes Brot*, *Easy Lover*): the map accepted
   with a wider window; and the map tried before the raw stamps are
   judged usable at all.
6. **A second of tempo breath** over four minutes with 98 % of the
   lines agreeing (*For You*) is not another edit: the drift rule's
   threshold raised to 2.5 s.

### The five that stand on their own

| Song | Why the text does not fit | What plays |
| --- | --- | --- |
| Cyndi Lauper — Girls Just Want to Have Fun | a 7:08 extended mix; the catalogue has only the single's 54 lines (227–232 s), which it sings with growing instrumental gaps (deltas 0 → 168 s) | the plain pass on the stem (2.7 letters/s): each line where the model heard it, 16 at line level |
| My Mine — Hypnotic Tango | a 7:03 extended mix, catalogue texts 145–370 s | the mix alignment (the stem read no better) |
| Annie — Two of Hearts (Skatebård remix) | 8:36; the text is the original's and the remix sings it twice (+81 s, then +222 s) | the plain pass, first half in the first run, second half in the second |
| Ozzy Osbourne — Crazy Train | a 3:46 cut; every catalogue text is the 4:50 album version's | the plain pass, 12 lines at line level |
| Apollo 440 — Ain't Talkin' 'Bout Dub | a 3:57 cut; the catalogue's 3:55 entries carry the 4:30 version's stamps | the plain pass |

None of these has a right text in the catalogue. A text made for the
recording — or, for the extended mixes, the single's text duplicated
where the mix repeats it — is the remaining lever, and it is manual.

### The eleven without lyrics

Unchanged: five instrumentals and phonk tracks with no catalogue
entry, three with plain words only, and three the lookup could not
match. A plain text can be aligned (the aligner needs words, not
stamps); nobody has fetched one by hand yet.

## What is still open

- ~~**Eight songs have no word timing** and sing at line level.~~
  Closed in the third round: with a vocal stem made locally every
  song with lyrics sings word by word (the separator is a tool on
  this machine, not a model the game ships — ADR-0014).
- **Five songs have words in the catalogue but no timing**, and five
  have no entry at all. The untimed five are alignable in principle —
  a forced alignment needs a text, not stamps — but without an anchor
  they are the pipeline's hard case, and four of the five read below
  the legibility floor anyway.
- **Nine songs' lyrics belong to another edit.** Their alignment
  stands on its own, which is the honest outcome, but a remix that
  repeats a verse the original's sheet contains once will leave that
  repeat unsung: no aligner can place a line twice, and the fix is a
  text that matches the edit.
- **The anchor window has been tuned** — and the measurement said the
  opposite of the obvious, so the width now follows whether the
  source's offset is known. It did not rescue the eight failures, and
  neither did the third pass that reads the offset off the
  wide-anchored result: the whole library was re-aligned with it and
  the tally stayed 20 / 15 / 9 / 8. What it bought was precision on
  the songs that already worked (uncertain words 32.9 % → 24.2 %),
  which is worth having but is not the missing lever.
