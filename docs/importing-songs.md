# Importing Your Own Songs

BeatByte turns any song you own into a playable chart in about a
minute: analyze → generate → play, with the built-in editor as the
correction pass. Every command below was run exactly as written
(against an MP3) while writing this guide.

## By name, from inside the game

Press **D** in the browser, type the song and press **Enter** (**Esc**
cancels). The search looks it up, judges the recordings it finds, fetches the best one,
measures it, pulls the lyrics and charts it — the result lands in
`songs/imported/` exactly as a dropped file does, and appears in the
browser the moment it is there.

**It runs in the background.** Start a search, then browse, change a
setting or play a song: it carries on and lands anyway. A panel at the
bottom of the screen — the one a dropped file uses — says which step
it is on and how far along it is, on whatever screen you are looking
at.

Write it as `Artist - Title` where you can: only then can the
catalogue be asked how long the song is, and that length is what
throws out live takes, remixes and hour-long loops *before* anything
is downloaded. Without the dash it still searches, but the choice
rests on the titles alone.

## Direct YouTube links

Paste one into the same **D** field (`CMD+V` on macOS, `CTRL+V`
elsewhere) and press ENTER. The panel says which of the two things
ENTER will do before you press it: *"ENTER fetches that video"* for
a link, *"ENTER searches"* for a name.

Accepted, with or without a scheme, `www.`, or whatever the share
button appended:

    https://www.youtube.com/watch?v=<id>
    https://youtu.be/<id>?si=…&t=42
    https://www.youtube.com/shorts/<id>
    https://www.youtube.com/embed/<id>
    https://music.youtube.com/watch?v=<id>
    <id>                     the bare eleven characters

Anything else stays a **search**, including a link to another site
and a YouTube URL that names no video (a channel, a bare playlist).
A wrong guess here would download some other video, so the parser
answers "no" whenever it is not certain.

**A link skips the choosing.** No catalogue lookup, no search, no
ranking, no model, and no verdict that could refuse the fetch: the
ladder that picks a recording is built to *avoid* live takes,
remixes and covers, and it would fight a deliberate one. The
loudness and tempo measurement still runs and is still reported — it
just no longer decides. If you name it, you get it.

**The video ID is written into the song's document**
(`song.json`, `identity.external.source_id`, with `source.kind` set
to `youtube`), for links and for the name search alike. That is what
makes duplicates answerable:

| Input | Result |
|---|---|
| The same video ID again | Rejected, naming the folder it is already in. |
| A different video ID for the same title and artist | Allowed as a separate version — a live take, a remix, a radio edit. |
| An invalid or unreachable URL | A readable line in the panel, before anything is downloaded. |

⚠️ Songs imported **before** this shipped carry no ID, so they cannot
take part in the check. The audio fingerprint still catches a
re-download of the same file; a re-encode of the same video is what
the ID is for.

It needs **`yt-dlp`** on your machine (`brew install yt-dlp`); the
game ships no downloader of its own and says so if the tool is
missing. ⚠️ Downloading from YouTube is against its terms of service —
what you fetch and from where is your call.

Which recording it takes is decided in two stages, and both are
measurements rather than guesses:

| Stage | What decides |
|---|---|
| On the metadata | The catalogue's length for the song, through the same rule the lyrics lookup uses, throws out every different edit. What is left is ordered by what its own title admits — live, cover, karaoke, nightcore, hour-long loop. |
| On the audio, leader only | The loudness report (a video rip gives itself away by where its spectrum ends) and the analyzer's tempo confidence. Fail either and the next candidate gets its turn. |

**SETTINGS → AI SEARCH** (on by default) lets a model help with the
first stage: it reads the candidate titles and answers with a number.
On means "use it when there is one to use": it runs through the Claude
Code CLI if that is installed — already signed in, so no key is stored
anywhere — otherwise through an API key you store. With neither it
costs nothing and does nothing, the row says as much, and the ranking
above stands on its own. It never touches the audio.

Supported audio: **WAV, Ogg Vorbis, FLAC, MP3, M4A/AAC** — the
verified list lives in
[the chart-format spec](chart-format/chart-format-v1.md#supported-audio-formats).

## The `[GS]` twin

Every import also gets a second browser entry, **`[GS]
<title>`**: the same audio in a folder of its own, charted from the
separated instrument stem instead of the whole mix — fewer notes,
melodically grounded, the version the ear preferred by a distance.
It is made in the background after the import (decode → `demucs`
`other` stem → chart), between songs, and reported on the import
overlay. The original is never touched, so both play against each
other. **SETTINGS → GUITAR STUDY TWINS** switches it (on by default);
it needs `demucs` on the machine (`pipx install demucs`) — the row says
when it is missing, and the import is then what it always was.
`beatbyte-cli study <folder> --lead <other.wav>` writes one by hand,
`tools/guitar-study.sh` every folder without a twin.

## The `[CL]` twin

A second kind, and one nothing makes for you: **`[CL] <title>`** is a
folder's ACTIVE chart with the classic ingredients applied — the
rules the early guitar games played by, measured and written down in
`beatbyte-chart::classic`. Only what an ingredient changes is
different, which is what makes a blind test between the two answer a
question about one variable. There is no audio analysis and nothing
is generated — except for `chords`, which reads evidence made from the
audio beforehand (below).

| Ingredient | What it changes |
|---|---|
| `hopo` | Hammer-ons in beats, not seconds: under 170/480 of a beat, exclusive. Straight eighths are strummed, triplets hammered, at any tempo. |
| `strum` | A judgment rule the chart carries: a strum under the wrong fret waits 60 ms for the fret. No note changes. |
| `hard` | Hard is rebuilt as the chart's own Expert with a ninth of its notes taken away — crowded, off-beat, fret-changing notes first; pairs only, never green with orange. |
| `medium` | Medium is rebuilt from Hard: thinned to ~2.2 notes a second (inside 61–76 % of Expert), beats kept longest, folded to four frets passage by passage. |
| `chords` | Chords where the recording strikes several notes at once — read from the song's polyphony sidecar, never guessed. The interval decides the shape (a fifth one fret apart), at most 36 % of a level's events, triples at most 16 % of the chords and only on Expert. |

```bash
beatbyte-cli classic <folder> --twin              # one song, blind-tested recipe
beatbyte-cli classic <folder> --twin --with all   # every ingredient
beatbyte-cli classic songs/imported --all --twin  # every folder
beatbyte-cli classic songs/imported --all --twin --dry-run
```

Without `--with` a run uses only the ingredients that have passed a
blind test. **To blind-test one more**, apply it ALONE to a `[CL]` twin
as a new version, then press `T` on the twin in the browser: the test
plays the new version against its parent, which differ by that
ingredient and nothing else.

```bash
beatbyte-cli classic songs/imported/classic-<song> --with strum
```

### Chords need a polyphony sidecar

`chords` is the one ingredient that needs to know what the recording
does: which notes are **struck together**. Nothing in the analysis can
say that — it hears one line at a time — so an `ml` build of the tool
transcribes the song once with Spotify's *Basic Pitch* (Apache-2.0,
0.2 MB) and writes `<audio stem>.poly.json` beside the audio:

```bash
beatbyte-cli models install basic-pitch              # once
beatbyte-cli poly songs/imported/<song>              # one song, ~1 min
beatbyte-cli poly songs/imported --all               # every folder
beatbyte-cli classic songs/imported/classic-<song> --with chords
```

The song is separated first (a local `demucs`, as for the `[GS]`
twin) and the `other` stem is transcribed: Basic Pitch hears one
instrument best, and on the mix the bass, keys and voice would read as
chord tones. `--mix` transcribes the mix anyway. The run says how much
of the song's Expert the evidence reaches. A twin made after the
sidecar carries a copy of it; for an existing twin, run `poly` on the
twin's own folder. Without a sidecar `chords` writes nothing and says
which command makes one.

A twin of a `[GS]` study is `[CL] [GS] <title>` and sits under the
study in the browser — the study chart played by the classic rules,
which is the combination this library is mostly played on. The dry
run says what each folder would get, per difficulty and inside the
thirty seconds the blind test plays, and a chart the rules would not
change gets **no twin at all**: a second entry playing the same notes
is worse than none.

⚠️ A twin copies the audio, so a whole library of them costs what
the library costs. Nothing in the song's own folder is touched, and
removing them again is `rm -rf songs/imported/classic-*`.

Without `--twin` the same command writes the ingredient as a new
chart VERSION inside the folder and moves the pointer, which is the
form the blind test (`T` in the browser) plays: it puts the active
version against its parent.

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

Start BeatByte — the song is in the browser, sorted by title. Pick a
difficulty and play. (BeatByte ships no songs of its own, so until you
import one the browser says so.)

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

## Watching a folder

Drop a **folder** onto the window (instead of files) and BeatByte
remembers it as the watched song folder: every few seconds, while you
are in the menu or the browser, it looks for new audio files (up to
five directory levels deep) and imports them through the same
pipeline as a drop. A file is only picked up once it has stopped
changing between two polls, so a track still being copied is never
half-imported.

Duplicates are skipped by **content**, not by name: each imported
file's fingerprint (FNV-1a 64 over its bytes, plus its size) is
persisted in `imported-hashes.json` next to the imports. A renamed
copy of an imported song is recognized and skipped; a different song
that happens to share a file name is not. A song you delete in the
browser stays deleted — its fingerprint is remembered, so the watcher
does not resurrect it from the folder. The SONG FOLDER row in the
settings shows the watched folder; confirming the row clears it.
