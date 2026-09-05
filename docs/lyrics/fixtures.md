# The own fixture set — what to record, and how it gets corrected

The evaluation harness (`beatbyte-cli lyrics-eval`) measures against
the JamendoLyrics corpus, which is 812 MB of CC-licensed music and
therefore lives **outside** this repository, behind
`BEATBYTE_LYRICS_CORPUS`. That means a clone with no corpus measures
nothing. The plan's answer (§L5) is a small set of **our own** songs,
committed, so something is always measured.

Own means own: three short clips of Martin's voice singing his own
words. No third-party recording, no third-party lyrics — the same rule
that governs every other asset here.

## What to record

Three clips, **10–20 seconds each**, and deliberately of different
difficulty, because a fixture set that is all easy proves nothing:

| # | What | Why |
| --- | --- | --- |
| 1 | Sung, dry — voice only, no music | The ceiling: what the aligner can do when nothing is in the way |
| 2 | Sung over an instrumental (any backing, hummed, a guitar, a drum loop) | The real case: the aligner runs on the mix, and the mix is what hides consonants |
| 3 | Sung fast or slurred — a run of short words, or a line sung right through | The floor: where a word gets one frame and the gate should say so |

Any format the game reads (`wav`, `ogg`, `flac`, `mp3`, `m4a`), any
sample rate, mono or stereo. **English words**, because the model that
ships is `wav2vec2-base-960h` — an English acoustic model. German
would measure the model's absence of German, which the corpus run
already reports.

For each clip, also write the words down, one line per sung line, in a
plain `.txt` next to it — exactly the words as sung, in order.

Drop the six files (three audio, three text) anywhere and say where.

## How the ground truth gets made

Word times are the thing being measured, so they cannot come from the
aligner alone — that would be the aligner grading its own homework.
The loop is:

1. **I align** each clip and write the word times out.
2. **I render a check track** — `beatbyte-cli lyrics-check <audio>`
   writes `<audio>.check.wav`: your clip with a short tick exactly at
   every predicted word onset (the clip ducks under each tick so it
   stays audible), and prints the word sheet, one line per lyric line,
   with `*` on every word the pipeline could not vouch for.
3. **You listen once per clip.** A click that sits on the word is
   right; a click that lands early or late marks a word to fix. Say
   which words are off and roughly which way ("the clicks in line 2
   are all a beat late", "‘through’ fires before I sing it").
4. **I nudge those words** and re-render, until the clicks sit on the
   words.
5. The corrected times are committed as the fixture's ground truth, in
   the same layout the corpus uses (`lyrics/<name>.words.txt`,
   `annotations/words/<name>.csv` with `word_start,word_end,line_end`),
   so the same reader and the same metrics run over them.

Step 3 is the part only a person can do: the fixture is *hand*
corrected by definition, and this session cannot hear.

## What the fixture set is for, and what it is not

It is a **floor**: a clone with no corpus still measures the pipeline
end to end, and a change that breaks alignment shows up immediately.

It is **not** a published benchmark. Three clips of one voice cannot
carry the gates in the plan's §2 — those stay tied to the corpus run.
The fixture's own thresholds are set from its first measured values,
generously, so the test catches breakage rather than fluctuation.
