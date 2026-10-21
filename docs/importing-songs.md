# Importing Your Own Songs

BeatByte turns your audio into a playable chart; analysis time depends on
the recording: analyze → generate → play, with the built-in editor as the
correction pass. Since 0.19.3 new imports require the installed song-chart
pipeline; the legacy command examples later in this guide are archived.

**Where the library lives.** Every import lands in the data
directory: `~/Library/Application Support/beatbyte/songs/imported` on
macOS, `~/.local/share/beatbyte/songs/imported` on Linux,
`%APPDATA%\beatbyte\songs\imported` on Windows — one library per
device, the one the sync keeps identical (ADR-0021) — unless you
moved it (SETTINGS > LIBRARY, see the README); then `$LIB` is the
folder named in `<data>/beatbyte/library-location.json`. The commands
below write it as `$LIB`:

```bash
LIB="$HOME/Library/Application Support/beatbyte/songs/imported"
```

**How a song folder is named** (since 0.18.x, user's scheme of
2026-10-12): `artist--title--version`, e.g.
`009-sound-system--dreamscape--bg01` for the first community chart and
`alice-cooper--schools-out--gr01` for the first game rip; an own import
has no version (`rick-astley--never-gonna-give-you-up`). Lower-case,
words joined by `-`, the parts by `--`, no spaces or brackets, umlauts
written out (`die-aerzte`), accents and apostrophes dropped, other
scripts kept as they are, at most 100 characters cut at a word. One
function writes it (`beatbyte_chart::twin::song_folder_base`). Folders
an older build named (`bridge-01-…`, `gamerip-01-…`) are still read, and
`beatbyte-cli rename-folders "$LIB"` renames them; scores, telemetry and
the active chart version are keyed by the song id and file names inside
the folder, never by the folder's name.

## Drag and drop with local AI charting

Since 0.19.2, an explicitly installed local charting runtime is used by the
shared audio import path, including dropped files and fetched recordings.
Drop MP3, M4A, WAV, OGG or FLAC into the menu or browser. The panel shows
beats, instrument separation, note detection and creation of all four
difficulties. The song appears after chart validation; long recordings
can take several minutes. See [setup and behavior](audio/song-chart-prototype.md).

Since 0.19.3, this is the only enabled import path. Missing or broken runtime
registration reports an error; no standard chart is produced. The legacy
algorithms remain in the source but their game/CLI entry points are disabled.
Failed imports can be retried; existing song folders are never overwritten.
Audio and all playable song data are copied together into the chosen library
(SETTINGS > LIBRARY). A disconnected chosen drive stops imports rather than
redirecting them to the internal disk. The source audio is kept.

## By name, from inside the game

Press **Cmd/Ctrl+D** in the browser (or ADD A SONG in the action menu),
type the song and press **Enter** (**Esc** cancels).

The search fetches a recording, then sends its audio through the same installed
song-chart pipeline as a dropped file. Direct YouTube links select one recording.
Bridge/Encore community charts and game-rip chart downloads are disabled, as are
the old automatic guitar upgrades and study twins. Optional vocal processing
remains available. Existing community, study and classic charts still play.

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

## The `[GS]` twin (no longer made by the game)

Up to 0.18.84 every import also got a second browser entry, **`[GS]
<title>`**, charted from the separated instrument stem. The game no
longer makes one, and the SETTINGS row is gone (2026-10-11: one entry
per version, and the tags GS and CL leave the interface; what the stem
pipeline knows goes into the import's own chart instead — see the
roadmap). Existing twins still load and group under their song, shown as
EXTRA CHART. The old `beatbyte-cli study` command and `tools/guitar-study.sh` are disabled
since 0.19.3. The following legacy sections describe the preserved historical
formats; their authoring commands no longer execute.

## The `[CL]` twin (archived authoring)

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
beatbyte-cli classic "$LIB" --all --twin  # every folder
beatbyte-cli classic "$LIB" --all --twin --dry-run
```

Without `--with` a run uses only the ingredients that have passed a
blind test. **To blind-test one more**, apply it ALONE to a `[CL]` twin
as a new version, then press `T` on the twin in the browser: the test
plays the new version against its parent, which differ by that
ingredient and nothing else.

```bash
beatbyte-cli classic "$LIB/classic-<song>" --with strum
```

### Chords need a polyphony sidecar

`chords` is the one ingredient that needs to know what the recording
does: which notes are **struck together**. Nothing in the analysis can
say that — it hears one line at a time — so an `ml` build of the tool
transcribes the song once with Spotify's *Basic Pitch* (Apache-2.0,
0.2 MB) and writes `<audio stem>.poly.json` beside the audio:

```bash
beatbyte-cli models install basic-pitch              # once
beatbyte-cli poly "$LIB/<song>"              # one song, ~1 min
beatbyte-cli poly "$LIB" --all               # every folder
beatbyte-cli classic "$LIB/classic-<song>" --with chords
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
removing them again is `rm -rf "$LIB"/classic-*`.

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

Create a folder per song in the library (`$LIB`, above) — the game
imports there itself, and a folder you make by hand is read the same
way:

```bash
mkdir -p "$LIB/my-song"
cp ~/Music/my-song.mp3 "$LIB/my-song/"
```

Installed builds also scan the user songs directory
(`~/Library/Application Support/beatbyte/songs` on macOS,
`~/.local/share/beatbyte/songs` on Linux, `%APPDATA%\beatbyte\songs`
on Windows). Both locations are scanned up to two folder levels deep.

## 2. Analyze (optional, but tells you what to expect)

```bash
beatbyte-cli analyze "$LIB/my-song/my-song.mp3"
```

```text
Analysis of `$LIB/my-song/my-song.mp3`
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
beatbyte-cli generate "$LIB/my-song/my-song.mp3" \
  --title "Test Drive" --artist "You"
```

```text
Generated `$LIB/my-song/my-song.chart.json` — 92.1 BPM, 70 s
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

## Community charts from Bridge (archived)

New Bridge/community chart imports, `beatbyte-cli bridge`, the browser shortcut
and the bulk downloader are disabled since 0.19.3. Existing converted song
folders still play. The preserved format and earlier bulk workflow are described
in [bridge-bulk-import.md](bridge-bulk-import.md).
