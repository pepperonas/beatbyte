# Bulk import from the Bridge chart library (Encore API)

BeatByte can play the community guitar charts that the
[Bridge](https://github.com/Geomitron/Bridge) downloader fetches: a
download is converted into a numbered **BG twin** (`[BG-01] <title>`)
with its own audio (ADR-0022). This page is about doing that **in
bulk**: finding the best-known songs in the whole community library,
choosing one good chart for each, skipping everything the library
already has, and downloading and converting hundreds of them unattended.

The game itself does none of this. It has no search over the API, and
it never contacts Encore. The bulk import is a set of three scripts in
[`tools/bridge-bulk/`](../tools/bridge-bulk/) that you run yourself. What
comes out of them goes through the same converter as a folder Bridge
downloaded: `beatbyte-cli bridge`, or **B** in the song browser.

```text
catalog.py crawl ─► catalog.jsonl     every guitar chart the API lists
catalog.py rank  ─► ranks.json        a popularity number per song (Deezer)
pick.py          ─► selection-*.json  the next N songs the library lacks
fetch.py         ─► songs/bridge-NN-… download · unpack · beatbyte-cli bridge
```

## What you need

| | why |
|---|---|
| `python3` (3.9+, no packages) | the scripts |
| `curl` | every request; `files.enchor.us` refused Python's own HTTP client |
| `ffmpeg` | the converter mixes Bridge's stems into one `song.m4a` |
| `beatbyte-cli` | the converter itself, built from this repository |
| disk | about **10 MB per song** in the library; the download is deleted after conversion |

Paths come from the environment. The defaults are this machine's paths:

| variable | default | meaning |
|---|---|---|
| `BRIDGE_WORK` | `/Volumes/Samsung SSD/beatbyte-bridge-work` | catalogue, ranks, picks, logs, `done.txt` |
| `BEATBYTE_LIB` | `/Volumes/Samsung SSD/beatbyte/songs` | the library (the folder SETTINGS > LIBRARY points at) |
| `BEATBYTE_CLI` | `$BRIDGE_WORK/beatbyte-cli` | the converter (a COPY of the binary, see below) |
| `CAP_GB` | `200` | `fetch.py` stops before the total download would pass this |
| `TOP_N`, `OUT` | `20`, `selection-next.json` | `pick.py`: how many, and where to write them |
| `SELECTION` | `selection-next.json` | `fetch.py`: which pick to download |

Copy the CLI into the work folder before a long run (`cargo build
--release -p beatbyte-cli && cp target/release/beatbyte-cli
"$BRIDGE_WORK/"`). If you rebuild the repository during a run, the binary
`fetch.py` is calling changes under it, and the batches of one run end
up converted by different converters.

## Quick start: the next 20 songs

```bash
export BRIDGE_WORK="/Volumes/Samsung SSD/beatbyte-bridge-work"
python3 tools/bridge-bulk/catalog.py            # once; resumable (~2 h the first time)
TOP_N=20 OUT=selection-next.json python3 tools/bridge-bulk/pick.py
# read the review list it prints (see "Duplicates"), then:
SELECTION=selection-next.json python3 tools/bridge-bulk/fetch.py | tee "$BRIDGE_WORK/fetch-next.log"
```

To pick more later, run `pick.py` again. It reads the library afresh,
so whatever the last run added counts as "already there".

## The API

Encore is the backend of the Bridge app, and an unofficial one: it has
no published contract, no key and no stated rate limit. The scripts keep
to a polite pace and retry.

**Search:** `POST https://api.enchor.us/search` with a JSON body:

```json
{"search": "", "page": 1, "per_page": 250, "instrument": "guitar",
 "difficulty": null, "drumType": null, "source": "bridge"}
```

The answer is `{"found": N, "data": [hit, …]}`. Each hit carries:

| field | used for |
|---|---|
| `md5` | the chart's identity and its download name |
| `name`, `artist`, `charter` | the song; charters carry rich-text tags (`<color=…>`) |
| `packName`, `drivePath` | reveal game rips (see "Filters") |
| `song_length` (ms) | the length filter |
| `hasVideoBackground` | a video makes the download big for nothing (it is skipped on unpack) |
| `modchart` | a modchart is not a playable guitar chart |
| `notesData.noteCounts[]` | notes per instrument and difficulty, so a guitar chart's levels are known before downloading |
| `notesData.hasOpenNotes` | whether the chart uses open notes (BeatByte has no open lane) |
| `notesData.chartIssues[]` | problems the API's own checker found |

The API returns HTTP 500 now and then; every call retries with backoff.
⚠️ **Paging is not stable**: charts are re-indexed while you page, so a
single pass misses some and repeats others. `crawl` makes two full
passes and keys the result by `md5`. The first full crawl found
**87 354 distinct guitar charts**.

**Download:** `GET https://files.enchor.us/<md5>.sng`, with curl. A
`HEAD` request gives the size first, which is how `CAP_GB` is enforced
before a byte is fetched.

**The `.sng` container:** `SNGPKG`, a version, a 16-byte XOR mask, then
the metadata pairs (exactly what a folder's `song.ini` would hold) and a
file index of name, size and offset. Byte *i* of every file is XORed with
`mask[i % 16] ^ (i & 0xFF)`, so the key repeats every 4 096 bytes.
`fetch.py` unpacks it into the folder layout Bridge itself writes
(`notes.chart`/`notes.mid`, `song.ini`, the audio stems), leaves video
files out and keeps only each entry's base name, so a file name in the
container cannot point out of the folder. An archive without a notes file
fails here and is not converted.

## Ranking: which songs are "top"

The chart library has no notion of how well known a song is, so
`catalog.py rank` asks Deezer's public search
(`https://api.deezer.com/search?q=<artist> <title>`) and takes the `rank`
of an **exact** artist-and-title match, normalised the same way on both
sides. Covers, karaoke tracks and same-named songs in Deezer's results
therefore count for nothing. If there is no exact match the rank is 0,
and a song with rank 0 is never picked. The requests are throttled to
8 per second; Deezer allows 50 per 5 s. Ranking all **54 768** songs
that pass the filters took about two hours. `ranks.json` is checkpointed
every 1 000 songs, and a restart only asks for the ones still missing.

The rank is a rough measure. It reflects streaming popularity today, it
skews toward what Deezer's audience listens to, and songs that are
missing from Deezer or spelled differently there sink to the bottom.
Treat it as "well known first", not as a chart position.

## Filters: one chart per song, community charts only

`catalog.usable()` drops a chart before it is ranked if it is:

- a **game rip**: the commercial games' own charts or recordings, found
  by charter, pack or drive path (Harmonix, Neversoft, Rock Band, Guitar
  Hero, `WaveGroup` = the cover recordings of GH1/2, Rocksmith, DLC, …).
  The library takes community charts only (ADR-0022, and the project's
  rule of no trademarks or game assets);
- **not the original recording, or not a song**: live, remix, cover,
  karaoke, instrumental, acoustic, demo, medley, mashup, full album,
  parody, "but …", "sung by …", 8-bit, nightcore, sped up / slowed, joke,
  version, edit, reprise, intro/outro, `(feat`, square brackets, `vs`.
  A naive search matches all of these ("… sung by Goofy", "… but the
  band is on crack");
- a modchart, a chart **without Expert guitar**, Expert with **fewer
  than 100 notes**, or a song shorter than **1 minute** or longer than
  **10 minutes**.

Songs often have several community charts. `catalog.best_chart()` keeps
one per song, preferring in this order: **no video**, **no open notes**,
**fewest API-reported issues**, **most levels charted by hand**, **most
Expert notes**. A chart that is Expert only is fine, because the
converter derives Hard, Medium and Easy down the classic ladder.

## Duplicates

Nothing should enter the library twice, and three checks see to that.

1. **Against the library (automatic).** `pick.py` reads every chart
   version (`chart.json`, `chart.vN.json`) and every song document
   (`song.json`, ADR-0019) in the library, including imports, GS/CL
   twins and earlier BG twins. A candidate is a duplicate when its
   **title** matches and **at least one artist** matches:
   - the title is compared without its twin tag (`[GS]`, `[CL]`,
     `[BG-NN]`), accents, case, parentheses and every character that is
     not a letter or a digit, so `Bang!` = `bang` and `50//50` = `5050`;
   - the artist is split into its credits (`feat`, `ft`, `featuring`,
     `and`, `x`, `with`, `&`, `,`, `/`, `+`) and any one shared credit
     counts, so `Dua Lipa feat. DaBaby` meets `Dua Lipa`.
2. **Inside the pick (automatic).** Every song picked is added to the
   same index immediately, so a second spelling of the same song further
   down the ranking is skipped. `done.txt` (every md5 ever fetched) is
   excluded too, so a chart that failed to convert is not fetched a
   second time.
3. **Covers (a person decides).** A cover has the same title under
   another artist, so check 1 cannot tell it from an unrelated song with
   the same name. Some are covers (HIM's *Wicked Game* = Chris Isaak's),
   most are not (*Stay* by Lisa Loeb, Blackpink and Post Malone are three
   songs). `pick.py` prints every such case **with the library's
   artist** beside it, plus every title that occurs twice in the pick.
   Read the list, add the real covers to `covers` in
   [`exclusions.json`](../tools/bridge-bulk/exclusions.json) and pick
   again. The pick fills up from further down the ranking. In the
   1 000-song run, 62 titles were listed and 6 were covers:
   *Somebody That I Used To Know*, *Hurt* (the Nine Inch Nails original
   of the Johnny Cash recording in the library), *Wicked Game*, *Maniac*,
   *Thunderstruck* and *Enjoy the Silence*.
   In the next 1 000, 73 were listed and 12 excluded. Two kinds are easy
   to miss: the **original of a cover the library has** is the same song
   (Thin Lizzy's *Whiskey in the Jar* beside Metallica's, The Cars' *You
   Might Think* beside Weezer's). And **when unsure, exclude**: a song
   wrongly excluded only costs a replacement from further down, while a
   duplicate stays in the library (Sum 41 *Paint It Black*, Avenged
   Sevenfold *Wish You Were Here* and Pennywise *Revolution* went out on
   that rule).

The converter adds a fourth check of its own. A download is
fingerprinted (FNV-1a over the chart file and `song.ini`, stored as
`bridge-source.json`), so the same download imported again writes
nothing and is reported as "already there".

## Downloading and converting: `fetch.py`

`fetch.py` works in batches of **25**. For each chart it gets the size
with `HEAD`, checks it against `CAP_GB`, downloads, unpacks and deletes
the `.sng`. Then the whole batch goes through
`beatbyte-cli bridge <batch> --library $BEATBYTE_LIB` in one call, and
the md5s are appended to `done.txt`.

- **Resumable.** Stop it at any point and start it again with the same
  `SELECTION`: everything in `done.txt` is skipped.
- **It waits for the game.** The import refuses to write to the library
  while BeatByte runs, because someone may be playing. `fetch.py` waits
  until the game exits instead of losing the batch.
- **Progress.** Each batch logs
  `batch N: done/total, X GB downloaded`. If the `taskline` Python module
  is importable, it also shows a live bar above the Claude Code prompt;
  without it the script runs the same, just without the bar.
- **The report.** For every song the converter prints what it wrote and
  what it left out, for example:
  `BG-01 a song of its own → …/bridge-01-goldfinger---superman (easy 500, medium 675, hard 1087, expert 1196); 5 open notes left out; 315 taps as HOPOs`.
  "A song of its own" means the library had no song by that name, so the
  twin stands alone. Otherwise the twin is filed under the existing song
  with the library's spelling of it.

The conversion itself is described in ADR-0022: the tempo map becomes
the exact beat grid, natural HOPOs follow the format's threshold, taps
become HOPOs, star power becomes Hype phrases, opens are left out and
counted, and the stems are mixed into a 256 kbit/s AAC that is decoded
once before the chart is written.

## Known limitation: open notes

BeatByte has no open lane, so the converter leaves open notes out. That
is the right call, because guessing a lane would teach a shape the song
does not have, but on a chart that relies on opens it leaves holes. The
catalogue only says *whether* a chart has opens, not *how many*, so the
pick can prefer charts without them but cannot reject a chart for having
too many. The count appears only after conversion, in the report.

Measured on the two 1 000-song runs of 2026-10-06: **507** and **488**
of the 1 000 charts lost at least one open note, and **219** and
**222** lost more than 5 % of their notes. The worst were all metal and hard rock, where the open
string carries the riff: The Warning *Ritual* 56 %, Bring Me The
Horizon *Antivist* 55 % and *Empire* 52 %, Metallica *Fight Fire with
Fire* and *Whiplash* 51 %, Black Sabbath *Children of the Grave* 46 %.
Treat more than about 5 % as a sign the song will feel gappy, and more
than 30 % as a different song.

To list them from a run's log (share = opens ÷ (opens + Expert notes),
the Expert count in the report being what is left):

```bash
python3 - "$BRIDGE_WORK"/fetch9.log <<'PY'
import re, sys
for l in open(sys.argv[1]):
    m, o = re.search(r"expert (\d+)\)", l), re.search(r"(\d+) open notes left out", l)
    if m and o:
        share = int(o[1]) / (int(m[1]) + int(o[1]))
        if share > 0.05:
            print(f"{share:4.0%}  {l.split(' (')[0].strip()}")
PY
```

Two ways out, neither taken yet: delete those twins and pick the next
songs instead, or give BeatByte a way to play opens (ADR-0022 rejected
mapping them to a fret lane, because that teaches a wrong shape).

## Bridge versions of your own songs: `own.py`

`pick.py` ranks the whole community library. For the songs you imported
yourself (and their GS and CL twins) there is `own.py`, which asks the
**live** search API once per song instead of reading the crawled
catalogue, because the catalogue was missing charts the API has:

```bash
python3 tools/bridge-bulk/own.py                       # writes selection-own.json
SELECTION=selection-own.json python3 tools/bridge-bulk/fetch.py
```

Two properties of the API it relies on, both found the hard way
(2026-10-07):

- **The search looks at the song name only.** "Toto Africa" returns
  nothing; "Africa" returns sixteen charts, Toto's among them. The query
  is therefore the title (and the artist as well, for imports whose
  artist and title are swapped, like "Numb – Linkin Park"), and the
  artist is matched on the results. Common titles need paging: "Stan"
  has 301 hits, so up to six pages of 100 are read.
- **A community chart may have no Expert.** Toto's *Africa* is charted
  on Hard only; any guitar level with 100 notes or more is accepted, and
  the converter derives the levels below the highest one.

Titles from video imports carry junk ("- Remastered - 1080p",
"- OFFICIAL VIDEO"). A title counts as a match when one contains the
other and the shorter is at least 60 % of the longer, so "Nothing Else
Matters" finds "Metallica- Nothing Else Matters" but "Danke" does not
pass for "Danke für nichts". Game rips stay out and are listed.

The first run (2026-10-07): 86 own songs, 20 already with a BG version;
of the 66 others, 7 had a community chart (5 new, 2 fetched earlier
under another spelling), 7 exist only as game rips (*Livin' On A
Prayer*, *Born to Run*, *Don't Stop Believin'*, *Crazy Train*, *In the
Shadows*, *Seven Nation Army*, *Ain't Talkin' 'Bout Love*), and 52 are
not on Bridge at all.

## The work folder

| file | what it is |
|---|---|
| `catalog.jsonl` (+ `.done`) | one line per chart, the fields above; `.done` = both crawl sweeps finished |
| `ranks.json` | `"artist\|title" → Deezer rank` (normalised keys) |
| `selection-*.json` | a pick: rank + chart, in download order |
| `done.txt` | every md5 handled, converted or not |
| `downloaded-bytes.txt` | running total against `CAP_GB` |
| `fetch*.log` | one log per run, the converter's report included |
| `batch/` | the batch in flight; removed after it is converted |

To refresh the catalogue (new charts appear every day), delete
`catalog.jsonl.done` and run `catalog.py` again. The crawl merges by md5,
and `rank` asks Deezer only about songs it has not ranked before.

## What the runs produced

| run | log | songs converted | notes |
|---|---|---|---|
| 2026-10-04 | — | 20 | picked by hand-tuned rules before the ranking existed (ADR-0022 roadmap entry); 3 replaced for losing too many opens |
| 2026-10-05 | `fetch.log` | 997 of 1 000 | the first top 1 000 by rank |
| 2026-10-05/06 | `fetch2.log` | 1 000 | the next 1 000 |
| 2026-10-06 | `fetch3.log`, `fetch4.log` | 200 + 20 | |
| 2026-10-06 | `fetch5.log` … `fetch8.log` | 20 + 3 + 3 + 3 | each picked against the library as it stood after the previous one |
| 2026-10-06 | `fetch9.log` | **1 000 of 1 000, 0 failed** | ranks 552 410 … 492 326; 62 same-title cases reviewed, 6 covers excluded; 10.2 GB downloaded in 1 h 42 min; 268 charts with all four levels by hand, the rest Expert with the lower levels derived; every one a song of its own (the library had none of them) |
| 2026-10-06 | `fetch10.log` | **1 000 of 1 000, 0 failed** | ranks 492 293 … 448 266, the first run with the scripts from this repository; 73 same-title cases reviewed, 12 excluded (9 covers or the same song as a cover in the library, 3 uncertain and excluded to be safe); 11.0 GB in 1 h 46 min; 263 with all four levels by hand; every one a song of its own |

After the last run the library holds **4 713** song folders, **4 368**
of them BG twins, in 37 GB. A duplicate audit over the whole library
afterwards (same title and a shared artist between any two folders)
found no pair involving either of the two 1 000-song runs. The pairs it did find are either
the intended twin families (a BG, GS or CL version beside the import of
the same song) or two older imports of the user's own that no bulk run
touched: Nena *99 Luftballons* twice, and Cyndi Lauper *Girls Just Want
to Have Fun* in its studio and live versions.

## Why it is a tool and not a game feature

ADR-0022 decided that *downloading stays in Bridge* and that BeatByte
gets no search over the unofficial API. That still holds for the game:
it never talks to Encore or Deezer, and the "What leaves your machine"
section of the README does not change. The bulk scripts are a
maintainer's tool. They run when you start them, they write only to the
work folder, and they hand the library nothing that a folder downloaded
by Bridge would not have. See the 2026-10-06 amendment to ADR-0022.
