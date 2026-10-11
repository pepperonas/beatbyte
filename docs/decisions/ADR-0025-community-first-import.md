# ADR-0025 — Adding a song by name: a community chart first, one chart

**Status: Accepted** (2026-10-11, the user's commission: "die playlist soll
per default den aktuellen umfang haben … Denoch soll manueller import via
youtube möglich sein — aber wichtig: mit der besten import strategie um
den song chart zu bauen". Of three offered strategies the user chose
"Encore zuerst, sonst generieren".)

## Context

The library is almost entirely what the bulk import (`tools/bridge-bulk`,
ADR-0022) fetched from Encore, the catalogue behind the Bridge app: 4 368
community charts and 4 203 game rips. Those are charts people made — or
the commercial games' own — and they play better than anything BeatByte
generates from a recording.

Adding a song by name (`Cmd/Ctrl+D`) went the other way: search YouTube
with `yt-dlp`, download a recording, generate a chart from the mix, and
queue a `[GS]` twin charted from the separated guitar stem. The game knew
nothing of Encore; a song that existed there as a finished chart was
generated anyway.

Measured before deciding (2026-10-11):
- `POST https://api.enchor.us/search/advanced` with `name` and `artist`
  filters on the server: *Kryptonite* by 3 Doors Down returns exactly its
  three charts (a community chart by Angevil, the Rock Band 2 DLC rip,
  the Guitar Hero 5 rip). The plain search matches the title only (six
  hits, three of them other songs).
- `https://files.enchor.us/<md5>.sng` serves the package to `ureq` (the
  bulk tools used curl because Python's client was refused).
- The whole way — search, download, decode, convert, lyrics — takes about
  eight seconds for one song.
- Not every song is there: *Africa* by Toto returns only covers.

## Decision

1. **A crate of its own, `beatbyte-encore`** (no Bevy, no chart types):
   - `hit` reads a search answer — untrusted JSON; an md5 that is not 32
     lowercase hex digits never becomes a URL;
   - `choose` decides purely which hits are the song and in which order
     to try them, with the rules the bulk import proved on eight thousand
     songs: same title without its furniture, a shared artist, not a
     cover/live/remix unless the player asked for one, an Expert part of
     at least 100 notes, no modchart, a song's length (and the
     recording's, when known); best first = a community chart before a
     rip, then no video, no open notes, fewest issues, most levels, most
     notes;
   - `sng` decodes the package — untrusted bytes: every length checked
     against what is left, a 512 MB cap, a file name accepted only as a
     plain name with a known extension;
   - `client` is the only part that touches the network.
2. **The search asks Encore first** (`community.rs`, a new COMMUNITY
   phase on the overlay) when the query names an artist. Up to three
   fitting charts are tried; each goes through the converter **B** uses
   (`beatbyte_library::bridge::import`), so it lands as a COMMUNITY or RIP
   version with the same duplicate check. A chart that loses more than 5 %
   of its notes as open notes is thrown away and the next is tried (the
   bulk import rejected those by hand). Karaoke lyrics are looked up for
   it as for a recording.
3. **Everything that fails is a reason to go on, not an error**: no
   network, no ffmpeg, no fitting chart, a package that does not convert —
   the line says why and the search continues exactly as before (YouTube,
   then a generated chart).
4. **A link skips Encore.** A pasted YouTube link names one recording; it
   is charted as it always was.
5. **No twin — the import's own chart is improved in place.** An import
   is one entry per version. The GUITAR STUDY TWINS setting is gone; the
   background job that made the twin now writes
   `beatbyte_chart::study::write_upgrade`: the separated stem charted on
   the accepted grid, played by the classic rules (`Recipe::all()`, chords
   only from a polyphony sidecar), saved as the folder's next revision and
   made the one that plays. The first chart stays one SWITCH REVISION away;
   a hand-edited chart is never replaced; a Bridge or rip folder is never
   touched.
6. **Always built in**, not behind a feature: the game already talks to
   lrclib and runs `yt-dlp` on the same key press, and a feature flag
   would only make the default build the worse one.

## Alternatives

- **Generate only, with the best pipeline** (stems + classic rules). No
  network beyond today's, but every song the community has charted better
  would still be generated — the opposite of what the library shows works.
- **Keep generating from the mix** and only drop the twin. Smallest
  change, worst charts.
- **Shell out to the Python bulk tools.** They exist and work, but tie the
  game to a Python install and a work folder on one machine.
- **Plain search plus client-side artist filtering.** Kept as the
  fallback when the advanced search finds nothing (an artist spelled
  another way), not as the main path: it returns other songs of the same
  name for `choose` to wade through.

## Consequences

**Good.** A song the community has charted arrives as that chart, in
seconds, with no recording downloaded and no generation run. The choice
is pinned against two recorded real answers, the decoder against lying
and cut packages, and the whole way by an ignored network test into a
throwaway folder.

**Costs.** A second network service on the add-a-song key (the README's
network section and badge say so). Encore can change its API; the
fallback keeps adding songs working when it does. A query without an
artist cannot be told apart from other songs of its name and skips Encore.

## Verification

`beatbyte-encore`: 20 unit tests (the real Kryptonite and Africa answers,
md5 and name safety, round trip, cut and lying packages, the rip rule,
the order, the open-note limit) plus one ignored network test.
`beatbyte-game::community`: the chip-style names, no lookup without an
artist, and an ignored test that brings *Kryptonite* in as COMMUNITY 1
and finds it "already in your library" the second time.
