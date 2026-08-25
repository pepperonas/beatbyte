# Importing Your Own Songs

BeatByte turns any song you own into a playable chart in about a
minute: analyze → generate → play, with the built-in editor as the
correction pass. Every command below was run exactly as written
(against an MP3) while writing this guide.

Supported audio: **WAV, Ogg Vorbis, FLAC, MP3, M4A/AAC** — the
verified list lives in
[the chart-format spec](chart-format/chart-format-v1.md#supported-audio-formats).

## The fast path: drag and drop

Drop the audio file onto the BeatByte window (main menu or song
browser). The song is analyzed and charted in the background and
appears in the browser when done — file names like
`Artist - Title (Official Video) [id].m4a` become a clean
title/artist automatically. Everything below is the manual route with
full control over naming and output. To remove a song, highlight it
in the browser and press `Backspace`/`Del` twice — an imported song's
folder is deleted entirely; for hand-managed charts only the chart
file goes and your audio stays.

## 1. Put the audio where BeatByte looks

Create a folder per song under `songs/` (in development / portable
layouts) — `songs/imported/` is the conventional place for your own
music and is never committed:

```bash
mkdir -p songs/imported/my-song
cp ~/Music/my-song.mp3 songs/imported/my-song/
```

Installed builds also scan the user songs directory
(`~/Library/Application Support/beatbyte/songs` on macOS,
`~/.local/share/beatbyte/songs` on Linux, `%APPDATA%\beatbyte\songs`
on Windows). Both locations are scanned up to two folder levels deep.

## 2. Analyze (optional, but tells you what to expect)

```bash
beatbyte-cli analyze songs/imported/my-song/my-song.mp3
```

```text
Analysis of `songs/imported/my-song/my-song.mp3`
  duration          69.8 s
  bpm               92.1   (confidence 51%)
  beats              108
  onsets             215
  first beat       0.023 s
```

Low BPM confidence or a listed "alt bpm" means the generator may pick
the wrong tempo octave — the editor pass will show it immediately
(notes consistently between grid lines).

## 3. Generate the chart

```bash
beatbyte-cli generate songs/imported/my-song/my-song.mp3 \
  --title "Test Drive" --artist "You"
```

```text
Generated `songs/imported/my-song/my-song.chart.json` — 92.1 BPM, 70 s
  easy        67 notes,  2 phrases
  medium      93 notes,  3 phrases
  hard       170 notes,  3 phrases
  expert     211 notes,  3 phrases
```

The chart lands next to the audio and references it by relative path —
keep the two files together. `beatbyte-cli validate <chart>` and
`beatbyte-cli inspect <chart>` check and summarize it.

## 4. Play it

Start BeatByte — the song is in the browser (built-in songs first,
then yours, sorted by title). Pick a difficulty and play.

## 5. Correct it (this is expected!)

Generated charts are **playable, not perfect** — the intended workflow
is `generated chart → human correction → final chart`. Highlight the
song in the browser and press `E`:

- move through the beat grid, add/remove notes, move a note with `M`
  (grab, navigate, place), toggle HOPOs, adjust sustains
- select a range with `V`, then `X` deletes it or `H` toggles HOPO on
  it — bulk edits undo as a single step
- `P` auditions from the cursor with a metronome tick on every beat
- undo/redo covers everything
- saving re-validates the chart; an invalid state cannot be saved

Typical corrections: deleting ghost notes from noisy sections, adding
missed downbeats, and simplifying machine-gun runs the analyzer heard
in a busy mix.

## Troubleshooting

- **Song not in the browser** — the chart must be valid and its audio
  reference resolvable: run `beatbyte-cli validate`, and check the
  game log for a `skipping …` line naming the reason. Files more than
  two folder levels below `songs/` are not scanned.
- **Everything feels early/late** — run the in-game latency
  calibration (Settings → Calibration) before blaming the chart.
- **Wrong tempo feel (half/double)** — regenerate after checking
  `analyze`; if the alt bpm is the right one, correct the chart in
  the editor or re-encode the intro (long ambient intros weaken the
  tempo fit).
