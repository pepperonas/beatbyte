# Optimizing a library: how every song gets lyrics that sing word by word

The procedure that takes an imported song from "a folder with an audio
file" to a chart on the current generator and lyrics that light up word
by word, in sync — and what to do with the songs that do not get there
on the first pass. It is written from the run that first produced it
(2026-09-06, the Böhse Onkelz *Mexico* case, judged "perfekt" by ear)
and every rule in it is one the pipeline now applies by itself; the
manual steps are what remains for a human when the pipeline says it
cannot.

**The measure of done is the ear.** The pipeline's numbers (verdict,
consensus, legibility) are guards — *Mexico* came back "shifted master,
85 % agreement" and ran three seconds off on screen. A song is finished
when its lyrics are in sync while playing, and the numbers say why.

## The pipeline in one picture

```
audio ──decode (priming skipped)──► the game's timeline
   │
   ├─► beatbyte-cli redesign        hard + expert on the current generator
   │                                (easy + medium carried, ear-approved)
   │
   ├─► lyrics lookup (in the game)  lrclib, three steps, the song's length
   │                                as part of the question → <song>.lrc
   │
   ├─► demucs --two-stems vocals    the vocal stem, on the same timeline
   │                                (measured: lag 0 against `decode`)
   │
   └─► beatbyte-cli align --vocals  plain pass → anchors → gate → words.json
           │
           ├─ same / shifted master  ✔ word level
           ├─ stretched              ✔ word level, stamps mapped, unsung lines dropped
           ├─ different edit         aligned times kept; the TEXT is the problem
           └─ failed                 line level from the stamps; the AUDIO or TEXT is
```

Everything up to the separator is the game's own code and runs on any
clone. The separator is an external tool on this machine (demucs via
pipx, Apple-silicon GPU) — the game may not ship its weights (their
licence, `docs/ROADMAP.md` L6), a person may use it on their own
library.

## The algorithm, step by step

### 0. Decode, and know where the sound ends

`beatbyte-cli decode <song>` writes the song exactly as the game hears
it: mono, 16-bit, the container's encoder priming skipped so sample 0
is the master's. Two facts come from the decode and both matter later:

- **The timeline.** Anything that produces audio *for* the pipeline (a
  separator, a DAW) must be on this timeline. The check is
  `cargo run --release -p beatbyte-lyrics --example timeline -- <song>
  <stem> [<stem>…]`: it sums the stems back into a mix and
  cross-correlates it against the song at the aligner's 16 kHz.
  **Measured 2026-09-06: demucs (through ffmpeg) is at lag 0 on the
  m4a class the library consists of and on the corpus's mp3s.** The
  aligner also refuses a stem whose length differs from the song's by
  more than 15 ms — one AAC priming is 23 ms, so a separator fed the
  container instead of the decoded song is caught by construction.
- **The sound's end**, not the container's. `Mexico`'s file is 283 s
  long; the music ends at 168.7 s and the rest is digital silence. Every
  length rule in the pipeline judges against the sound's end (the gate,
  the unsung-line rule); the catalogue lookup still asks with the
  container's length, which is why it matched the wrong entries there
  (see *what is still manual*).

### 1. The chart: `beatbyte-cli redesign <dir> --all`

Regenerates hard and expert on the current generator as a new sibling
version and moves the pointer; easy and medium are carried from the
active version, note for note, and moved onto the fresh grid. In an
`ml` build with the meter models installed the grid — beats and bar
lines — comes from the model (ADR-0015); the `meter:` line on stderr
says so, and a chart whose `grid.downbeats` is non-empty came that way. It says "already current" for a folder
whose hard and expert already match the generator and writes nothing.

⚠️ That check was dead until 2026-09-06 — `serde_json` moves a float by
one ULP on load, so a freshly generated chart never hashed like the one
read back from disk, and every `redesign --all` rolled every song over
(46 of 70 byte-identical to their parent but for the provenance). The
comparison now runs on what a reader would GET. A second run on a
current library must write nothing; that is the proof it works.

### 2. The text: the lookup, and what it cannot know

The game fetches lyrics from lrclib in three steps (names as imported,
then without a download's furniture, then a search judged by the
length rule) and writes `<song>.lrc` beside the audio. The catalogue
carries ONE synced text per song, copied across every recording of it:
for *Mexico*, 18 entries from 170 s to 368 s all carry the stamps of the
254 s studio version. **The stamps a song gets are a hypothesis about
which edit the recording is**, and the aligner is what tests it.

### 3. The stem: `demucs --two-stems vocals -d mps -o <out> <song>`

About 40 s per song on the M1 Pro (8× real time), the first run
downloads the model (80 MB). ⚠️ It writes TWO files per song,
`vocals.wav` and `no_vocals.wav`, 50–100 MB each: delete the
accompaniment as you go, or a library of sixty songs plus the
79-song corpus fills a disk (it did — 129 MB left, a build failed).

The stem is what makes the difference on a loud mix: *Mexico* reads
0.18 letters a second on the mix (the model hears "II" in twenty
seconds of singing) and 2.34 on the stem — above the legibility floor
of 1.0, so the alignment counts as evidence at all. Whether stems help
on average, and by how much, is measured on the corpus in
[`evaluation.md`](evaluation.md).

### 4. The alignment: `beatbyte-cli align <song> <lrc> --vocals <stem> --separator demucs:htdemucs --keep-better`

What happens inside, in order — every step is a pure function with a
test, and every one exists because a real song needed it:

1. **Plain pass.** One Viterbi over the whole song, no constraints.
2. **Agreement.** Do the source's stamps sit a constant off the plain
   pass (same master, or shifted)? If yes → the anchored pass with a
   tight window (±1 s) around the shifted stamps. Stamps the shift
   puts past the sound's end are **unsung** — the album's last verses
   on a recording that stops before them — and lose their stamp
   first (four library songs; their verdict stays what the other
   lines say).
3. **Warp.** If they agree on no constant, do they agree on a LINE?
   A Theil–Sen fit over the better-heard half of the lines (robust to
   the lines the plain pass lost), accepted when the tempo ratio is
   within ±10 %, the residual under 2 s and most in-sound lines sit
   within 3 s of where the line puts them. If yes → the stamps are
   mapped (`scale · t + offset`), lines the map puts beyond the
   sound's end are marked **unsung**, and the anchoring runs against
   the mapped stamps — with windows at least twice the residual wide
   when the map is coarse. Tried before the raw stamps are judged
   usable at all, because a sheet that runs past the file is the
   case it exists for. (*Mexico*: 1.0388 · t − 10.88 s, 24 of 39
   lines sung, the rest dropped. *Fettes Brot*: 1.0987 · t − 1.33 s.)
4. **Anchored pass** with a wide window (±4 s) when the offset is not
   known.
5. **Pushed?** An anchored pass that sits at its windows' edge, or
   whose confidence collapsed against the plain pass's, was pushed
   against the evidence, not placed by it: the window opens ×3 once;
   if still pushed, the plain pass stands for the gate.
6. **Tighter pass** when the anchored pass then agrees on an offset the
   plain one could not see.
7. **The gate.** Legibility floor (below it the alignment is not
   evidence against a human's stamps), the verdict, word rules
   (sprinted, endless, and **parked** — a word more than 2 s after its
   predecessor inside a line was put where the next letters were,
   which is the next line's onset; it is retimed to follow its
   predecessor), line fallback, unsung lines dropped.
8. **Keep-better.** With `--keep-better` the file is replaced only when
   the new alignment outranks the one beside the audio: verdict
   standing first (failed < unjudged < confirmed), then how much of the
   song the model heard. A tie keeps the old file.

### 5. Read the verdict, then decide

| Verdict | What it means | What to do |
| --- | --- | --- |
| `same_master`, `shifted_master` | The text is this recording's. | Nothing. Play it. |
| `stretched` | The text is another edit's; the map fitted, unsung lines dropped. | Play it; check the last sung line ends where the singing does. |
| `different_edit` | The stamps run past the sound, or the text ends a minute before it and the lines disagree, or the deltas drift more than 2.5 s along a line the fit did not accept; aligned times stand on their own. | Read the consensus first: 94 % of lines within 1.5 s (France Gall) is a good alignment wearing an honest label. Otherwise get the RIGHT text: another catalogue entry, or a plain text (the aligner needs words, not stamps). A remix that sings the text twice (Annie) or an extended mix that spreads it over seven minutes (Cyndi Lauper) has no right text in the catalogue; the plain pass stands where the model heard the lines. |
| `failed` | Fewer than half the lines agree, or the model could not read the audio (legibility under 1.0). Lines from the stamps. | Below the floor with a stem: the vocal is buried or not in the model's language — nothing further to tune. Above it: the text is wrong; treat as `different_edit`. |
| no `.lrc` | The catalogue has no synced text of a matching length. | Look for an unsynced text; ask with the sound's length, not the container's. |

`beatbyte-cli lyrics-check <song>` writes a click on every aligned
word, for judging by ear without watching the screen.

### 6. Verify the way the user does

The last check is not a number. Play the song. Where that is not
possible, the two probes below answer the question the numbers cannot:

- **What the model hears where** — the `voice` example
  (`--example voice -- <stem> <from> <to>`) prints the model's greedy
  letters in a time range. On *Mexico* it read "TAMAXICHO … NON FLOWS"
  at 16–22 s where the stamps said the line was at 27–30 s; that is
  how the wrong edit was found.
- **Where the singing is** — the vocal stem's RMS per second. *Mexico*
  sings at 11–34, 38–60, 73–96, 99–121 and 145–167 s and nowhere
  after; the fifteen lines the text had after that are not in this
  recording.

## What is still manual

- **A wrong text.** The catalogue's one-text-per-song means a live cut,
  a radio edit or a remix gets a studio version's words. The warp
  rescues the "same performance, other edit" case; a different
  performance (other verses, other order) needs a text of its own,
  fetched by hand. The lookup could be made to ask with the sound's
  length instead of the container's — open.
- **A language the model does not speak.** `wav2vec2-base-960h` is an
  English character model. German rock reads at 0.03–0.5 letters a
  second on the mix; on the stem *Mexico* reaches 2.3 and aligns, but
  it is the stem that made the difference, not the model. The
  multilingual model the plan names (MMS-FA) has a licence the project
  cannot ship (L6 in the roadmap).
- **The separator itself.** It runs on this machine, by hand, outside
  the game. Shipping one would need weights under a licence the project
  may distribute; the runtime side is ready (ADR-0013, `rten`
  implements every operator a separator needs).

## Using the AI further, honestly

Of the three models the plan names, the beat tracker is in (*Beat
This!*, MIT — `beatbyte-meter`, ADR-0015): with `beatbyte-cli models
install beat-this-mel` and `beat-this` on the machine, step 1 below
takes the song's beats and bar lines from the model instead of the
built-in tracker, and says so on stderr (`meter: … heard N beats; M
downbeats`). The separator and the multilingual aligner are each
blocked on the same thing: weights this project may ship. The
pipeline is built so that when one becomes available it is a registry
entry and a download on the user's action, verified against a pinned
hash, nothing else. Until then the honest
path for a library is the one above: the game's own aligner on a stem
a person made locally, with the pipeline saying exactly which songs it
could not vouch for and why.

Numbers for the library this was run on are in
[`library-pass.md`](library-pass.md); the corpus measurements the rules
were set by are in [`evaluation.md`](evaluation.md).
