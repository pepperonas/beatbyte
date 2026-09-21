# ADR-0019 — A song's metadata lives in its folder; the index is a projection

**Status: Accepted** (2026-09-21, the user's commission: every song
should carry a structured, migratable metadata record that can later
feed library search, filters, sorting, statistics, history,
recommendations and playlists — explicitly *not* "one big struct of
`Option<T>`")

## Context

Song data already exists in BeatByte; it is simply scattered, and
three of the places it lives disagree about what a song even is.

| Where | What | Versioned |
|---|---|---|
| `chart[.vN].json` | title, artist, genre, bpm, offset, duration, preview, notes, grid, provenance | `format_version` |
| `*.context.json` | per-note onset / energy / brightness / bar phase / repeat span | with its chart |
| `*.loudness.json` | sample rate, channels, bitrate, lossy, bandwidth, clipping, DC, true peak | no |
| `*.lrc`, `*.words.json` | lyrics, word alignment | no |
| vocal chart | pipeline version, `audio_sha256`, separator, analyser, aligner, parts | `schema` + `pipeline_version` |
| `telemetry.db` | every played session and its events | `PRAGMA user_version` + migrations |
| `history.jsonl` | the same sessions again, in less detail | serde defaults |
| `scores.json` | best result per **title + artist + difficulty** | v2 |

Measured on the author's machine when this was decided: **172 song
folders, 2.4 GB of audio, a 12 MB telemetry store, 359 history
lines.**

Three facts in that table decided this ADR.

1. **There is no song identity.** `chart_hash` identifies a *chart*
   and changes on every redesign — which is exactly what it is for.
   `audio_sha256` identifies *bytes* and changes when a rip is
   replaced. `scores.json` keys on title and artist, so **fixing a
   typo in a song's title silently orphans its records**. Nothing
   names the song.
2. **SQLite with migrations already exists here** and its migration
   runner was written to be driven by an arbitrary list
   (`schema::apply(conn, migrations)`, "so a test can drive it with a
   different list"). A second database costs almost no new mechanism.
3. **`history.jsonl` and `gameplay_session` are two records of one
   event.** That duplication predates this decision. Extending it
   would be the wrong direction.

## Decision

**Three domains, three stores, and the middle one is disposable.**

```text
SONG    song.json in the song folder     what the song IS       authoritative, portable
INDEX   library.db (SQLite)              how to FIND it         a projection, rebuildable
EVENTS  telemetry.db (exists)            what HAPPENED          append-only evidence
```

1. **A song's metadata lives in the song's folder**, as `song.json`
   beside the audio and the chart, modelled by the `beatbyte-library`
   crate. Copying a folder moves a whole song — audio, chart,
   analysis artefacts, identity, provenance and the player's own
   edits — and that property is worth more than the convenience of
   one central file.
2. **`library.db` is a projection and never the only copy.** It
   exists so that "Deep House between 115 and 125 BPM that I have
   never played" is an index scan instead of 172 file parses. It can
   be deleted at any time and rebuilt from the folders, and a test
   pins that the rebuild equals the original.
3. **It is a separate database from `telemetry.db`.**
   [ADR-0018](ADR-0018-gameplay-telemetry-store.md) makes the
   telemetry store evidence that nothing reads back into the game and
   that may be discarded wholesale. The library is the opposite:
   authoritative user data the game reads constantly. Same
   technology, different lifetimes, different backup semantics.
4. **A `SongId` is given, not derived.** A random, time-prefixed id
   assigned once at import. The identities that already exist stay
   what they are and become *attributes*: the file hash and the audio
   fingerprint are what duplicate detection is built on, and external
   ids (MusicBrainz, ISRC, a video id) are recorded but are never the
   primary key.
5. **Playing is an event, not a counter.** No `play_count` on a song.
   `first_played_at`, `last_played_at`, play counts and every
   statistic are derived from `gameplay_session`, which already holds
   them per run.
6. **Provenance is per field, but only where fields are contested.**
   `Sourced<T>` (value + source + optional confidence) wraps genre,
   year, title, key, bpm — not file size, which has one answer.
   A field the player edited is recorded in an `overrides` set and is
   closed to every later refresh, whatever authority it claims.

## Alternatives considered

**One `library.db` as the single source of truth, no per-song file.**
Fewer files, one place to query, no synchronisation question at all.
Rejected because it ends the folder's portability: a song copied to
another machine would arrive without its identity, its edits and its
provenance, and re-importing it would mint a new `SongId` and orphan
its records — the exact failure this ADR exists to end. It also makes
the database the only copy of data the player typed by hand.

**Everything in the existing `telemetry.db`.** One file, one
migration list, joins for free. Rejected because it collapses the
distinction ADR-0018 rests on: that store is evidence the game never
reads back and that a player may delete without losing anything they
made. Library metadata is neither.

**Keep JSON only and query by scanning.** Simplest, and honest about
the size of the library today — 172 songs parse fast. Rejected on
where it goes: the commission's queries are joins against play
history ("never played", "accuracy between 120 and 130 BPM"), and
those are not a file scan at any library size. Keeping it would also
leave the `history.jsonl` duplication in place.

**`chart_hash` or `audio_sha256` as the song id.** Free, already
computed, no new concept. Rejected because both are *content*
hashes, and the commission's requirement is stability under exactly
the events that change content: a redesign, a re-encode, a re-rip.
They remain excellent identities of a chart and of bytes.

**A per-field provenance wrapper on every property.** Uniform, no
judgement calls about which fields are contested. Rejected because it
turns a file size into a three-line object, makes the document
unreadable in a folder and in a diff, and buys nothing: nobody
disputes a byte count.

**Derive `first_played_at` / `play_count` onto the song for speed.**
Rejected: two copies of one fact disagree, and the session log is the
one that can answer a question nobody has asked yet.

## Consequences

**Good**

- A song folder is self-contained and portable, and stays so.
- A rename, a move, a re-tag, a re-encode and a redesign all leave
  the song's records attached to it.
- The index may be corrupted or deleted without data loss.
- Statistics are derived from raw events, so a question invented next
  year can still be answered about last year.
- An analyser upgrade is detectable per song without re-analysing
  anything: the document records which version measured what.

**Costs**

- One more file per song folder, and one more database.
- The index must be kept in step with the documents; a test pins that
  a rebuild reproduces it, but drift is a real class of bug.
- Migrating `scores.json` onto `SongId` touches real user data.
- A per-field source makes some code paths wordier than a plain
  assignment.

## Verification

- `beatbyte-library` is pure and tested: round-trip, forward
  compatibility, id stability under rename/move/re-tag/re-encode/
  redesign, the override rule, the confidence rule, the placeholder
  rule, the timestamp semantics. Each pin mutated once and seen to
  fail.
- The index carries a test that a rebuild from the folders equals the
  stored projection — the whole content of "it is not authoritative".
- The `scores.json` migration keeps the old file as a rollback and is
  tested against a real one.
