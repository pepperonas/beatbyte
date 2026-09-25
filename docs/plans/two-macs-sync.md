# Plan — BeatByte on two Macs, one career

Architecture: [ADR-0021](../decisions/ADR-0021-two-devices-one-career.md).
Status: **decided 2026-09-25** — raspi5 hub, data-directory library, models synced once, same name = one person (end of this file). In progress.

## Part 1 — the MacBook Pro 2015

Known from the network (2026-09-25): reachable at `192.168.178.24`
(ping 14–34 ms), Bonjour name "Martin's MacBook Pro 2015", Screen
Sharing on port 5900, **SSH closed**. The macOS version is not
advertised; it needs SSH (`sw_vers`) or a look at the screen.

1. **SSH.** On the 2015: System Preferences → Sharing → *Remote Login*
   on, for user `martin`. Everything below runs over it; VNC stays the
   way to look at the screen.
2. **Measure the machine** before building for it: `sw_vers`,
   `system_profiler SPDisplaysDataType` (which Iris, Metal support),
   `df -h`, free RAM.
3. **Build.** `rustup target add x86_64-apple-darwin`, then
   `MACOSX_DEPLOYMENT_TARGET=<its version, at most 12.0> cargo build
   --release --target x86_64-apple-darwin -p beatbyte --features ml`
   and the same for `beatbyte-cli` (the sync lives there).
   `packaging/macos.sh` already takes a triple; it gains
   `universal-apple-darwin` (both triples built, `lipo -create`), sets
   `LSMinimumSystemVersion` from the deployment target instead of the
   hard-coded `11.0`, and puts `beatbyte-cli` into the bundle.
   ⚠️ Disk: a second target triple roughly doubles `target/release`
   (~4 GB now). Checked before (33 GB free today) and the x86 tree
   removed after the bundle is made.
4. **Metal on Intel Iris.** wgpu runs on Metal on every Mac that
   Monterey supports; what is not known is the frame rate of the 3D
   stage and bloom on an Iris 6100 / Iris Pro. Measured, not assumed:
   `BEATBYTE_FPS=1` for median and 99th-percentile frame times on the
   2D and the 3D view.
5. **Proof it runs** (on the 2015, over SSH, screen watched via VNC):
   `BEATBYTE_SMOKE_TEST=1` boots to the menu; `BEATBYTE_AUTOPILOT=1` on
   a real synced song passes perfectly; the calibration screen opens
   and records a measurement (`BEATBYTE_SHOT_STATE=calibration` plus a
   real calibration done by the user at the machine — a tap test needs
   a person). Frame times from step 4 recorded in the roadmap.

## Part 2 — sync

### Phase 0: backup (before anything touches real data)

`~/Library/Application Support/beatbyte` without `models/` (and
without the 12 MB `telemetry.db.bak-before-songid` already a backup)
→ `local/backup-<stamp>/` as a tar with a SHA-256 beside it, and the
repo's `songs/imported/` listed with sizes and hashes. Restoring is
documented next to it.

### Phase 1: the pure merge crate (`beatbyte-sync`)

Engine-free, like `beatbyte-telemetry`. One module per rule, each with
the conflict cases from the ADR as tests and a mutation probe each:

- `history` — union by natural key; the richer line on a collision;
  lines that do not parse are carried through byte for byte.
- `scores` — best per (song, difficulty); name-keyed collapsing onto
  the id.
- `achievements` — union, earliest date.
- `players` — collision detection and re-numbering, with a remap that
  the other rules apply to the remote side before they merge.
- `settings` — per shared key, newest change; device keys untouched;
  the API key never leaves.
- `library` — the manifest diff: copy, keep, rename-to-next-version,
  tombstone. Output is a list of actions, not file operations.
- `telemetry` — the plan of rows to insert and rows to prefer (the SQL
  that executes it lives in `beatbyte-telemetry`, which alone may open
  the store for writing — the `telemetry_stays_evidence` gate).
- Idempotence and order-independence as properties: merging A into B
  then B into A gives equal states; merging twice changes nothing.

### Phase 2: the game side

- Player ids from `created_ms << 10 | random`; tests that no new id can
  be a small legacy one.
- `settings.json` gains a `changed_ms` map for the shared keys, stamped
  on save by comparing with what was loaded (the file the game rewrites
  on exit becomes the source of the stamps rather than their enemy).
- Imports land in `<data>/beatbyte/songs/imported` always.

### Phase 3: the library migration on this Mac

With the game closed: the repo's `songs/imported/*` renamed into
`<data>/beatbyte/songs/imported/` (same volume, no copy). The two
folders that exist in both places (`my-mine---hypnotic-tango-m4a` and
its study) are compared file by file; identical files once, differing
ones reported and both kept. Counts before and after; the autopilot
plays one migrated song.

### Phase 4: transport + `beatbyte-cli sync`

Snapshot, publish, fetch, merge, publish. Refuses while the game
runs. A launcher script (in the bundle and for the dev checkout) runs
sync → game → sync.

### Phase 5: end to end on both machines

1. Baseline counts on both (history lines, scores, achievements,
   players, telemetry sessions/events, library files and bytes).
2. First sync: the 2015 receives everything; counts must match.
3. Offline: play a different song on each machine (and one the same).
4. Sync both, in either order, twice. Counts before and after: every
   run exactly once, both best scores kept per their rule, telemetry
   sessions = sum without duplicates, library identical by hash.

## Transport — three variants

The architecture above is the same for all three; only where the hub
lives and how bytes move differ.

### A. Direct LAN between the Macs (Bonjour)

Each Mac runs a small sync listener during `beatbyte-cli sync`, finds
the other by Bonjour and exchanges snapshots and missing blobs.

- **2015 asleep:** no sync happens. This Mac keeps playing; everything
  merges the next time both are awake and one of them runs sync. A
  Mac that is always carried around and a Mac that is always asleep
  rarely meet — in practice sync happens only when you sit at the 2015.
- **Both played at once:** harmless (merge), but each only sees the
  other's runs after a later sync with both awake.
- **2.4 GB transfer aborted:** blobs are content-addressed and written
  to a temp name then renamed, so a restart skips what arrived; needs a
  resume-aware protocol written by us.
- Cost: the most code (listener, discovery, protocol, auth on the LAN),
  and a listening port on both Macs.

### B. A folder in Google Drive (`~/My Drive/BeatByte-sync`)

The hub is a Drive folder; Drive for desktop moves the bytes.

- **2015 asleep:** no problem — this Mac publishes to Drive, the 2015
  picks it up whenever it wakes and syncs.
- **Both played at once:** harmless; each device writes only its own
  folder, so Drive never has two writers on one file.
- **Half-synced files:** a snapshot names every file with its SHA-256
  and is written LAST; a reader that finds a hash mismatch or a missing
  blob treats that device's snapshot as not yet arrived and uses the
  previous one. Online-only placeholders ("stream" mode) must be forced
  to download (reading them does) — slow on a 2.4 GB first sync.
- **2.4 GB transfer aborted:** Drive resumes on its own; blobs already
  there are skipped.
- Cost: moderate code; 2.4 GB of Drive quota; **gameplay telemetry and
  history go to Google** — which contradicts ADR-0018's "no upload" and
  makes Drive a third party the store was designed to avoid.

### C. The raspi5 as hub (recommended)

The hub is a directory on the raspi5 (`192.168.178.105`, always on);
`beatbyte-cli sync` moves bytes with `rsync` over SSH (both Macs get a
key for `pi@raspi5`).

- **2015 asleep:** no problem — the Pi is always there; each Mac syncs
  when it runs, and they meet through the Pi.
- **Both played at once:** harmless (device-owned folders, merge).
  The only thing to arbitrate is two syncs in the same second: a lock
  file per device folder on the Pi, taken with `mkdir` (atomic), makes
  a second sync from the same device wait; two different devices never
  write the same folder anyway.
- **2.4 GB transfer aborted:** `rsync --partial` resumes a file; blobs
  land under a temp name and are renamed; the snapshot is uploaded
  last, so an aborted first sync leaves the previous snapshot valid.
- **Away from home:** no sync until back in the home network; offline
  play continues and merges later.
- Cost: least new code (rsync does the transport, a lock is `mkdir`);
  the data stays in the house; the Pi needs ~3 GB and has 213 GB
  free (measured 2026-09-25, `rsync` installed); it joins the backup
  question.

**Recommendation: C.** It is the only variant that works while the
2015 sleeps *without* sending the play history to a third party, and
it needs the least new transport code. B is the choice if syncing
away from home matters more than the ADR-0018 promise.

## Open decisions

1. **Transport:** A, B or C (recommended C).
2. **Library on this Mac:** migrate `songs/imported/` into
   `<data>/beatbyte/songs/` (recommended, one root on both machines),
   or keep the repo as this Mac's root and map both to one logical
   library in the sync.
3. **ML models on the 2015:** only needed to *import* or to run the
   aligner/meter/Basic Pitch there. Recommended: not synced; importing
   stays a job for this Mac (the 2015 plays what arrives). Alternative:
   sync them once (450 MB).
4. **Same name, different id** (two players "Anna" made offline on two
   machines): one person, merged (recommended — the roster already
   allows one name per person) or two people, the second renamed.
5. **SSH on the 2015** (needed for every option): Remote Login on,
   and the macOS version (`sw_vers` then answers it).
