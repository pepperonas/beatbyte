# ADR-0018 — Gameplay telemetry is a versioned event store, not a log file

**Status: Accepted** (2026-09-20, the user's commission: BeatByte
should become a data-driven rhythm game — a structured, versioned,
long-term-analysable record of what the chart expected, what the song
was doing, what the player input, how the input system read it, how
the engine reacted and how it was scored; explicitly *not* a debug
log)

## Context

[ADR-0011](ADR-0011-adaptive-charting.md) already decided the shape of
the feedback loop — telemetry, analytics, design, versioned adoption —
and shipped its first layer: one JSONL file per played session, a
header line plus one line per judged note, read by
`beatbyte-cli review`. Four hundred and sixty-seven of those files
existed on the author's machine when this decision was made: 139 210
lines, 5.7 MB, 298 lines per session.

That layer answers "which notes get missed on this chart". It cannot
answer any of these:

- **Did the input reach the engine at all?** The physical device and
  button are consumed by `InputSources::just_pressed` and discarded
  one line later (`gameplay/input.rs`); a fret that was pressed and
  did nothing leaves no trace anywhere. So a player problem, a
  controller problem, a mapping problem and a chart problem all look
  identical from the evidence.
- **Under what conditions was this played?** The header carries one
  version field. Not the scoring rules, not the analysis, not the
  device, and not the calibration offsets that every timing figure is
  measured relative to.
- **What was the song doing there?** `SongAnalysis` — onsets, energy,
  the beat grid — is computed at import and never persisted, so the
  correlation between "this note is missed" and "this note came from
  a weak onset in a dense passage" cannot be drawn at all.
- **What happened around the notes?** Pauses, seeks, hype, the run's
  ending. `record_feedback` drops every session event that is not a
  hit, a miss, an overstrum or a sustain end.
- **Anything that needs a join.** "Miss rate by genre", "generator
  v17 against v18", "this device against that one" are full scans
  over hundreds of files, and the files grow without bound.

And the write path is one `write_session` on the way out: a crash
mid-song loses the entire run.

## Decision

**A local SQLite database, one file, written off the frame thread.**
A new crate `beatbyte-telemetry` owns it: the vocabulary and its
stable integer codes, the schema and its migration runner, the store,
the queue and worker, the analytics and the import of the old files.
It depends on `beatbyte-core` and on nothing else of ours — not Bevy,
not `beatbyte-chart` — so chart identity enters as a hash string and
the whole thing is testable with plain values and a database in
memory.

Six rules hold it together.

**1. Events, never frames.** Nothing is sampled per frame and no game
state is snapshotted. An event is a thing that happened: an action
reached the session, a note was judged, a hold ended, the song was
paused.

**2. Nothing derivable is stored.** No combo, no score, no accuracy,
no per-judgment counts, no "early / late" (that is the sign of
`delta_us`), no sustain *starts* (a hit on a note with a tail), no
phrase completions (the hits inside the phrase's span). The play
history (`history.jsonl`) is the session summary and stays exactly
that: a derived cache, recomputable from the raw events.

One documented exception: the **note-shape flags** — chord, HOPO,
sustain — are stored although the chart also says so. The redesign
rollover deletes superseded chart versions, and "do chords fail more
often" must still be answerable a year later. They ride in a `flags`
column that exists anyway and cost nothing.

**3. The song is referenced, never copied.** An event carries a note
index; the session carries the chart hash. Tempo, genre, section,
onset strength and everything else about the music is a join away.
Copying them per event would have multiplied the storage by an order
of magnitude to store what was already on disk.

**4. Three layers stay distinguishable.** Physical input → logical
action → gameplay result. The middle one is new: `Action` events are
the only evidence that the player did something the engine then did
nothing with. The physical layer is opt-in (see below).

**5. Integers, and time in microseconds.** Every repeated field is a
`u8` with a code pinned by a test — a million rows of `"NOTE_HIT"` is
a million copies of one word. Song time is `i64` microseconds and
timing offsets `i32` microseconds, converted once at the boundary;
the engine keeps its `f64` seconds (ADR-0004). A microsecond is two
orders of magnitude under the tightest hit window, and an integer
cannot drift by an ULP between writing and reading — which a float
demonstrably does in this repository.

**6. Provenance is mandatory.** Game version, chart format, chart
version (the hash *is* the version), generator, scoring version,
analysis version, vocal pipeline, telemetry schema, plus the device,
the calibration offsets, the detail level and the mode flags. Each of
them is a reason two sessions may not be compared blindly, and a
comparison that does not know about them is worse than none.

### Stable note identity

Chart notes have no id field and do not get one. The stable reference
already exists:

```
(chart_hash, difficulty, note_index)
```

`chart_hash` is a content hash, so it identifies the chart *and* its
version in one value; `ChartFile::to_track` is deterministic, so the
index is stable for a given pair; and a regenerated chart gets a new
hash, which makes confusing its notes with the old ones impossible.
That is exactly what the commission asked for, without inventing an
id that would then have to be kept stable by hand.

⚠️ `note_index` indexes **merged chord events**, not chart notes:
`to_track` groups simultaneous notes. An analysis that needs lanes
goes back through `to_track`. This was not written down anywhere
before.

### The detail level

Three settings, `RESULTS` / `ACTIONS` / `DIAGNOSTIC`, default
`ACTIONS`.

The blackbox is complete at `ACTIONS`: every decision the engine made
and every action it was asked to make one about. `DIAGNOSTIC` adds
the physical layer — which device, which button, how long held —
which is the only way to answer the hardware questions (a stuck
button, a double edge, a device that drops inputs), and which roughly
doubles the row count.

Measured from the real corpus (467 sessions, 170 charts, median 328
notes):

| Level | Events / session | Per session | 10 000 sessions |
|---|---:|---:|---:|
| `RESULTS` | ~360 | 18 KB | 180 MB |
| `ACTIONS` (default) | ~1 360 | 68 KB | **680 MB** |
| `DIAGNOSTIC` | ~2 600 | 130 KB | 1.3 GB |

Ten thousand sessions is roughly five hundred hours of playing. No
retention policy ships: deleting evidence by default is the one thing
a store like this must not do. If it is ever needed it will be
explicit and configurable.

### Writing

```
gameplay frame
  └─ build a small Copy struct, try_send on a bounded queue (8192)
       └─ worker thread
            └─ 500 events or 2 s → BEGIN / INSERT × n / COMMIT
```

`try_send` never blocks. Lifecycle messages — a session opening,
closing, a note the player left — travel on a **second, unbounded**
channel, because they must not be dropped and must not block either.

A full queue drops the event, counts it, and leaves a **gap in the
sequence numbers** at the exact place the loss happened; the count is
stored on the session and clears `telemetry_complete`. A hole is
never readable as a quiet passage.

WAL with `synchronous = NORMAL`: a crash of the game costs at most the
events since the last commit, and the session is visible as one that
never ended (`ended_ms` is null). A power cut can still cost the tail
of the write-ahead log. **No stronger promise is made, because none
can be kept.**

### Privacy

Local, and only local. The crate opens no socket. There is no upload,
no identifier beyond the local roster's own number, and no microphone
audio — a sung note is stored as a cent error and a rating, never as a
sample. `DIAGNOSTIC` records only actions BeatByte itself is bound to;
a key the game is not listening for never reaches it. Opt-in sharing
is not designed here and would need its own decision.

### What analytics may do

Produce evidence. Nothing in the crate changes a chart, a score, a
difficulty or a setting, and the game does not read the store back.
The note-quality signal (hit rate, median offset, spread, confidence)
is **computed on demand and never written into a chart** — a chart
that carried its own quality score would be a chart that argues with
the next measurement. A single miss means nothing; every ranking
takes a minimum sample count and reports the one it had.

**No self-learning.** No training, no automatic regeneration, no
adaptive difficulty. Analytics produce evidence; changing an
algorithm stays a deliberate, versioned act, exactly as ADR-0011 set
out.

## Measured

`beatbyte-cli telemetry bench` fills a throwaway store with generated
play in the proportions three real autopilot runs showed, then times
the questions above. On this machine (M-series laptop, release
build):

| | 1 000 sessions | 10 000 sessions |
|---|---:|---:|
| Events | 1 128 816 | 11 287 708 |
| Fill | 2.7 s (**424 000 events/s**) | 29.7 s (380 000 events/s) |
| Batch commit | 0.86 ms | 0.95 ms |
| On disk | 51 MB | **513 MB** |
| Per event | 47.7 bytes | 47.7 bytes |
| Every event of one session | 0.22 ms | 0.24 ms |
| Note quality, one chart version | 1.2 ms | 14 ms |
| Generator comparison, one genre | 106 ms | 1.8 s |
| Calibration bias, all players | 412 ms | 4.8 s |
| Timing histogram, everything | 130 ms | 1.7 s |

Ten thousand sessions is roughly five hundred hours of playing. The
per-session reads stay flat because the primary key is
`(session_id, sequence)`; the whole-corpus aggregates scale linearly
and are seconds, which is what an offline command line may cost.

The **real** figures agree with the generated ones where it counts:
three autopilot runs wrote 51.4 bytes per event, and 475 real
sessions (471 of them imported from the JSONL era) hold 146 290
events in 7.1 MB.

**One index was measured and rejected.** A partial index on
`(event_type, delta_us)` takes the calibration median from 221 ms to
28 ms — an eight-fold win — and costs **3.84 MB on a 45 MB store,
8.5 %**. For a query that runs offline, in under half a second, at a
thousand sessions, that is not a trade worth making; the number is
recorded here so the decision does not have to be re-derived.

## Alternatives considered

**Keep JSONL and read it better.** It is greppable, it is already
there, and the repository has a strong culture of plain JSON sidecars.
But the questions the commission asks are joins over session × event ×
chart, and the corpus is already 467 files; at ten thousand sessions
every question is a full scan of hundreds of megabytes. Indices,
transactions and a batch commit are exactly what a database gives for
free. The readability loss is real and is paid back with
`beatbyte-cli telemetry show`, which renders a session as lines, and
with a CSV/JSON export.

**A second, parallel system beside the JSONL one.** Rejected as a
matter of policy: the commission says not to build a parallel
architecture where an existing one can grow. The old files are
imported once and the store becomes the one sink; the readers move
with it.

**An embedded key-value store (`sled`, `redb`).** No query language, so
every analysis becomes hand-written iteration — which is the part
SQLite is actually good at.

**A columnar file (Parquet) for analytics.** Better for the analysis
and worse for everything else: no incremental writes, no updates, and
a dependency an order of magnitude heavier. It remains a good
*export* target, and the export module is shaped so it can be added.

**Bundling SQLite (`rusqlite` with `bundled`) vs. linking the system
one.** Bundled: the same behaviour on every platform and no system
package to install before the game builds. It compiles a large C file
once. The precedent for a vendored C dependency in this workspace is
`rusb`.

**Always recording the physical input layer.** It answers the
hardware questions and it is the only layer that can. It also doubles
the volume and records which buttons a person pressed, which is not
something a default should do. Hence the opt-in level.

## Consequences

- One new workspace crate and one new dependency (`rusqlite`,
  bundled), which lengthens a cold build by the time it takes to
  compile SQLite once.
- The telemetry store is now the thing to keep working across
  versions: a shipped migration is never edited, and `user_version`
  counts what has been applied. An older store opened by a newer
  build gains the new shape and keeps its rows; that property has its
  own test.
- `beatbyte-cli review` and the design dossier move onto the store,
  and the JSONL writer retires once they have. Until then the old
  files remain readable and importable.
- The per-note musical context (`onset strength`, energy, section,
  beat position) has to be **written at chart-generation time** into
  a sidecar beside the chart: it exists only inside the analysis
  today and is thrown away. It must not change `chart_hash`, or every
  stored session would lose the chart it names.
- Storage grows with playing. At the default level it is under a
  gigabyte for a lifetime of play, and the debug view shows the
  number rather than leaving it to be discovered.
