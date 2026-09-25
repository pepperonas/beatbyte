# ADR-0021 — Two devices, one career: device-owned snapshots and pure merges

**Status: Accepted** (2026-09-25, the user's commission: BeatByte on
this Mac and on a MacBook Pro 2015, with "everything the same" on both
— songs and charts, players, scores, history, achievements, telemetry
and settings. The four open choices were decided by the user the same
day; see "Decisions taken".)

## Context

Everything BeatByte remembers is per-machine state, and none of it was
written with a second machine in mind. Measured on this Mac today:

| Data | Where | Size | Identity it carries |
|---|---|---|---|
| Library | `songs/imported/` (repo-relative, 172 folders) **and** `<data>/beatbyte/songs/imported/` (4 folders, 2 of them also in the repo root) | 2.4 GB + 88 MB | folder name; `song.json` → `song_id` (ADR-0019) |
| `history.jsonl` | `<data>` | 389 lines | none — 201 lines predate the `player` field |
| `scores.json` | `<data>` | 85 records, v3 | song (`song_id`, else title+artist) × difficulty — **no player** |
| `achievements.json` | `<data>` | 1 player | player id → achievement → first-earned ms |
| `players.json` | `<data>` | 1 player | `id` from a per-device `next_id` counter |
| `telemetry.db` | `<data>` | 599 sessions, 295 381 events, schema v3 | `gameplay_session.uid` (start ms + a scramble, 32 hex) |
| `settings.json` | `<config>` = `<data>` on macOS | 45 keys | none; rewritten by the game on exit |
| models | `<data>/beatbyte/models` | 450 MB | registry id + SHA-256 |

Four facts decide the design.

1. **Both machines play offline.** The 2015 Mac sleeps most of the
   time; this one is carried around. Nothing may assume both are awake
   at once, and nothing may assume one of them is always right.
2. **The game is network-free, and says so** (the README claim a docs
   gate checks; ADR-0018: "local, and only local … no upload"). The
   precedent for the network is the offline tool: `library --catalogue`
   lives in `beatbyte-cli`, behind a feature.
3. **A live SQLite file with its WAL cannot be copied**, and a chart
   file swapped under a running browser breaks every Enter on that song
   (CLAUDE.md). So nothing touches a device's data while its game runs.
4. **Every data kind already has a natural identity, except players.**
   Telemetry sessions have a `uid` built from their start millisecond
   and a scramble; history lines have (start ms, title, artist,
   difficulty, player); scores have (song, difficulty); achievements
   have (player, achievement). Only `players.json` hands out ids from a
   counter, so two players made offline on two machines both get id 2.

## Decision (proposed)

**1. The merge is pure, and it is its own crate.** `beatbyte-sync`
holds one pure function per data kind — `(local, remote) → merged` —
plus the report of what each one did. No I/O, no transport, no clock.
Every rule below is a function with its conflict cases as tests and a
mutation probe per rule.

**2. Devices never write the same file.** A shared "hub" directory
(its location is the transport decision) holds one folder per device
that only that device writes:

```
hub/
  devices/<device-id>/
    snapshot.json      # history, scores, achievements, players, shared settings
    telemetry.db       # a consistent copy made with VACUUM INTO, never the live file
    library.json       # every library file: relative path → SHA-256, size; tombstones
  blobs/<sha256>       # library file contents, written once, never changed
```

A sync is: make my snapshot (game not running) → publish it → read
every other device's snapshot → merge into mine → publish again. Two
devices that sync in any order, any number of times, converge on the
same state; a merge applied twice changes nothing (idempotence is a
test). There is no "server state" to corrupt and no lock to hold.

**3. The merge rules** (the heart of this ADR — never "last writer
wins" as a blanket):

| Data | Rule |
|---|---|
| history | Union by natural key (start ms, title, artist, difficulty, player slot). A key present on both sides keeps the richer line (more fields set — the roster adoption added `player` to old lines on one device only). No new id field: the key is already unique, and a collision needs two devices to start the same song in the same millisecond. |
| scores | Per (song, difficulty): the better result by the game's own rule (higher score; equal scores → higher accuracy → longer streak → the local one). A name-keyed record and an id-keyed record for the same song collapse onto the id, as `ScoreBoard::record` already does. |
| achievements | Union per (player, achievement); the earliest date wins. They are re-derived from the merged history anyway (ADR-0017); the date is the only stored fact. |
| players | Identity is the id. **New ids become device-independent** (see 4). A remote player whose id matches a local one with a different `created_ms` is a collision from the old counter: the remote one is re-numbered and every reference in the remote snapshot is rewritten before merging. Same name, different id → the rule is an open decision. |
| telemetry | Rows, never files: attach the remote copy, insert every session whose `uid` is not here (new `session_id`, its events and notes carried along), `note_context` by its primary key. A `uid` on both sides keeps the more complete row (ended over open, more events). |
| shared settings | Per key, the most recent CHANGE wins — each shared key carries the time it last changed (the game stamps it when it saves). A preference is the one place where "the newer choice" is the honest rule; it is per key, not per file. |
| device settings | Never synced: `latency_offset_ms`, `video_offset_ms`, `mic_offset_ms`, `fullscreen`, `ui_scale`, `input_map`, `watch_folder`, `room_stage_url`, and **`anthropic_api_key`** (never over the sync path unless the user decides otherwise). |
| library files | By content. Same path, same hash → nothing. A path only one side has → copied, unless the other side has a tombstone for it newer than the file. Same path, different content → a chart version (`chart.vN.json`) is kept on both sides by giving the incoming one the next free version number (with its `.context.json`); the pointer `chart-active.json` keeps the local choice and the report names the other; `song.json` keeps the older document's `song_id` and rewrites the loser's references; a derived sidecar (`loudness`, `poly`, `words`, `context`) keeps the local one and reports. |

**4. Player ids stop being a counter.** A new id is
`created_ms << 10 | 10 random bits`: unique across devices, still
ordered by creation, still an SQLite `INTEGER`. Existing ids (`1`) stay
what they are: the new scheme never produces them, so they cannot
collide with anything made from now on; the collision rule in 3 covers
a device that ran an old build.

**5. One library root per device.** The library lives in
`<data>/beatbyte/songs/` on every device, the repo's `songs/imported/`
is migrated into it on this Mac (a rename on the same volume, not a
copy), and imports land there even when the game runs from a checkout.

**6. Sync runs only while the game does not.** `beatbyte-cli sync`
refuses while `beatbyte` runs, exactly as `classic` does; a launcher
syncs, starts the game, and syncs again when it exits.

## Alternatives

- **Copy the files and let the newest win.** Loses a whole offline
  session's history, scores and achievements on one side every time
  both were played. The commission rules it out, rightly.
- **A merged state on a server.** A server that holds THE state needs
  locking or a coordinator, and a half-written state file is corrupt.
  Device-owned snapshots make every write exclusive and every reader's
  job a merge it can repeat.
- **Sync inside the game.** Breaks the network-free promise the README
  makes and a gate checks, and would write library files while a
  browser holds them.
- **A per-line id added to history.** Unneeded for correctness (the
  natural key is unique), and 389 existing lines would need a migration
  that the key does not.

## Consequences

- The game changes in four places: player ids, the settings split and
  change stamps, import into the data root, and nothing else — the
  merge and the transport live outside it.
- ADR-0018's "local, and only local" is amended by exactly one path:
  the user's own devices, through a hub the user chooses. Nothing goes
  to a third party unless the chosen hub is one.
- A deleted song stays deleted only because deletion leaves a
  tombstone; without it, the other device's copy would bring it back
  on the next sync.

## Decisions taken (the user's, 2026-09-25)

1. **Transport: the raspi5 as hub** (`192.168.178.105`, always on, 213 GB
   free) — `rsync` over SSH, a `mkdir` lock per device folder. Syncs
   happen in the home network; offline play merges later.
2. **Library: one root, the data directory** on both machines; the
   repo's `songs/imported/` moves there.
3. **ML models: synced once** to the 2015 through the hub (450 MB),
   each file verified against its registry hash by the model store
   before it counts as installed.
4. **Same name on two devices is one person**: the players merge onto
   the one created first, and every reference to the other id is
   rewritten.
