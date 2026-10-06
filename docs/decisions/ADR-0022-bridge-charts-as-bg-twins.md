# ADR-0022 — Charts from the Bridge downloader as numbered BG twins

**Status: Accepted** (2026-10-04, the user's commission: "für dragonforce
fire and flames … über die bridge api song chart herunterladen und in
beat-byte den chart hinzufügen. im menü soll dieser anstelle von GS / CL
dann mit BG-X gekennzeichnet sein … mehrere Bridge versionen eines lieds
… BG-01, 02, 03". The four open choices were decided by the user the same
day; see "Decisions taken".)

## Context

Bridge (Geomitron/Bridge) is a desktop downloader for the community chart
library of five-fret guitar games. A download is a folder:

| File | What it is |
|---|---|
| `notes.chart` **or** `notes.mid` | the chart, in ticks against a tempo map, four difficulties |
| `song.ini` | title, artist, charter, `delay`, HOPO threshold, … |
| `song.opus` / `guitar.opus` / `rhythm.opus` / … | the audio, often split into stems |
| `album.jpg`, `preview.*`, video | not part of the song |

Measured on the five downloads on this Mac: two versions of *Through the
Fire and Flames* (charters MrOriginalityCH and Stargazer, `notes.chart`,
resolution 192 and 480) and three Sygenysis + Nunchuck songs
(`notes.mid`, `hopo_frequency = 170`). Charter names carry rich-text tags
(`<color=#0000FF>MrO</color>…`).

None of it plays in BeatByte as it is: the chart is not BeatByte JSON,
and Symphonia 0.5 has no Opus decoder.

## Decision

1. **A converter, pure, in `beatbyte-chart::bridge`.** Both chart formats
   parse into one intermediate form in ticks (`TickSong`), and one
   assembly turns it into a `ChartFile`:
   - frets 0–4 are lanes 0–4; notes on one tick are a chord;
   - HOPOs are decided as those charts expect: a single note within the
     threshold (`hopo_frequency`, else an eighth with `eighthnote_hopo`,
     else 65/192 of a beat) of a previous chord it shares no fret with
     is one naturally; `.chart`'s forced flag **flips** that, `.mid`'s
     force ranges **set** it; a tap is a HOPO;
   - **open notes are left out and counted**: BeatByte has no open lane,
     and a guess (all frets? green?) would teach a wrong shape;
   - star power becomes Hype phrases, merged where they touch;
   - the tempo map becomes the chart's **exact** beat grid (beats every
     quarter, downbeats from the time signatures) — no analysis guess;
   - `song.ini` wins over the chart file's own metadata.
   Untrusted input like every chart: bounded counts, a small MIDI reader
   of its own that ends a track at the first unreadable byte, and the
   result goes through `ChartFile::validate`.
2. **A third twin kind, `Bridge`, the only one that numbers.** Folder
   `bridge-NN-<song folder>`, title `[BG-NN] <title>`, two digits, 01–99.
   Being a twin, every rule that already protects twins holds (the
   redesign refuses it, the catalogue skips it, the browser files it).
   Unlike the other kinds it brings its own audio.
3. **The library song it belongs to is found by letters and digits** —
   `DRAGONFORCE` is `Dragonforce` — and the twin takes the library's
   spelling, so the browser's exact pairing finds it. Without such a song
   the download stands alone; its BG siblings gather under the lowest
   number in the browser.
4. **One download, one number.** `bridge-source.json` beside the chart
   holds a fingerprint (FNV-1a over the chart file and `song.ini`); the
   same download again writes nothing, another one gets the lowest free
   number.
5. **The audio is Bridge's, mixed with ffmpeg into one AAC `song.m4a`.**
   Stems are summed (`amix … normalize=0` — a split mix added back
   together is the mix), 44.1 kHz, 256 kbit/s. AAC rather than Vorbis:
   the ffmpeg here has no libvorbis, and the m4a path is the one the game
   already handles priming for. The file is decoded once before the chart
   is written: audio that does not play fails the import, not the song.
6. **Two entrances, one implementation**: `beatbyte-cli bridge` and **B**
   in the song browser (a chore). Both read Bridge's download folder from
   Bridge's own `settings.json` and write where imports land.

## Decisions taken (the user, 2026-10-04)

- **Audio: Bridge's**, not the library's own recording — the chart is
  timed against it; another master is seconds apart.
- **Placement: under the existing song** when there is one, else a song
  of its own.
- **Entrances: CLI and a button in the game.**
- **Downloading stays in Bridge.** BeatByte gets no search of its own over
  the unofficial API.

## Consequences

- The library's own lyrics are **not** copied into a BG twin: they are
  timed to the library's recording, not to Bridge's.
- ffmpeg becomes a runtime requirement **of the Bridge import only**; the
  game and every other import run without it, and its absence is said in
  words.
- A download's chart may use what BeatByte cannot show (opens, drums,
  vocals); the report says what was left out.
- No downloaded chart or audio ever enters the repository: the tests
  hand-write their `.chart` and MIDI files.

## Alternatives not taken

- **Reusing the library's recording** with an estimated offset — rejected
  by the user; an offset estimate fails exactly on live and remastered
  versions.
- **Opens as green, or as all five frets** — both teach a shape the song
  does not have.
- **An Opus decoder in the game** — a codec dependency for one import
  path, where a one-off transcode costs nothing at play time.

## Amendment 2026-10-06 — bulk import through the API, as a tool

On 2026-10-05 and 2026-10-06 the user asked for songs in bulk: first
the top 1 000, then batches of 20, 3 and another 1 000, each with "make
sure there are no duplicates". That cannot be done through Bridge's
interface, one search at a time, so the downloads went to the Encore
API directly. The scripts that did it are now versioned as
[`tools/bridge-bulk/`](../../tools/bridge-bulk/) and documented in
[`docs/bridge-bulk-import.md`](../bridge-bulk-import.md).

**Unchanged:** the game has no search over the API and never contacts
it, and "downloading stays in Bridge" still holds for everything the
game does. The converter is the only way into the library: a bulk
download is unpacked into exactly the folder layout Bridge writes and
goes through `beatbyte-cli bridge`, so every rule above (one download
= one number, the fingerprint, opens left out, audio decoded before
the chart is written) applies unchanged.

**New:**
- The tools rank songs by an outside popularity number (Deezer's
  public search, exact artist-and-title matches only).
- They pick one community chart per song: game rips are filtered out
  by charter, pack and path, as are live versions, remixes, covers and
  parodies.
- Before downloading, they refuse anything the library already has,
  matching title and any credited artist across every chart version
  and song document.
- Covers, which have the same title under another artist and which no
  string rule can tell from unrelated songs that share a name, are
  listed for a person to decide on and excluded in
  `tools/bridge-bulk/exclusions.json`.

**Cost:** the API is unofficial and unstable. Its order shifts while
paging, so the crawl makes two passes, and it answers 500 at random,
so every call retries. A change on Encore's side breaks the tools, not
the game. Open notes can only be counted after conversion, so a pick
cannot keep out charts that lose many of them. See "Known limitation"
in the bulk-import document.

**Not taken:** a search screen in the game over the same API. The
user's decision of 2026-10-04 stands, and a bulk tool that runs while
nobody plays needs none of the game's machinery.
