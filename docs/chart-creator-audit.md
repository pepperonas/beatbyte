# Chart Creator — Phase 0 Audit

Read-only analysis of the existing charting logic in BeatByte, written
before any change. Every claim below is traceable to a file and line.

## 0. Project context (block 1, determined from the repo)

The brief's context block arrived empty; these are the values read out
of the workspace rather than assumed.

| Field | Value | Source |
|---|---|---|
| Repo path | `/Users/martin/claude/beat-bytes` | — |
| Game framework | **Bevy 0.19** | `Cargo.toml:35` |
| Audio backend | **rodio 0.22** (Symphonia decoders), own thread + atomics | `Cargo.toml:38`, `beatbyte-audio/src/playback.rs:149-180` |
| Existing chart logic | `crates/beatbyte-chart/` (format, validate, generate), `crates/beatbyte-core/` (timing, notes, session), `crates/beatbyte-editor/` (ops, session) | — |
| Existing chart format | **Own JSON**, `format_version: 1` | `beatbyte-chart/src/schema.rs` |
| Editor runs as | **In-game mode** (`AppState::Editor`), UI in `beatbyte-game/src/editor_ui.rs`; domain logic in `beatbyte-editor` | `states.rs:29`, `editor_ui.rs` |
| Target platforms | macOS, Windows, Linux (all three shipped) | `.github/workflows/release.yml` |
| Edition / MSRV | **2024** / Rust 1.95+ | `Cargo.toml:15`, README |

`CLAUDE.md` exists and its conventions take precedence — notably: the
crate layering rule (core knows nothing of Bevy or audio; chart knows
nothing of Bevy), determinism as a feature, invertible editor ops, and
the quality gate before every commit. `.claude/rules/` and
`.claude/skills/` do not exist in this repo.

## 1. As-is architecture

```text
beatbyte-core          domain, engine-free
  ├─ timing.rs         TempoMap (seconds ↔ beats), TimingWindows, Judgment
  ├─ note.rs           NoteEvent { time_s, lanes: LaneSet, kind, sustain_s }, Track
  ├─ session.rs        TrackSession — judgment engine (input-stamp driven)
  ├─ score.rs          PlayerPerformance
  └─ music.rs          SongAnalysis, Onset, MelodyNote

beatbyte-chart         format + generation, no Bevy
  ├─ schema.rs         ChartFile { format_version, song: SongMeta, charts: Vec<ChartDef> }
  ├─ convert.rs        ChartDef → core::Track (groups same-time notes into chords)
  ├─ validate.rs       Issue/Severity linter
  ├─ generate.rs       SongAnalysis → ChartFile (the automatic charter)
  ├─ playability.rs    density/burst/motion metrics + burst limiter
  └─ io.rs             load/save + audio path resolution (traversal-hardened)

beatbyte-editor        editing domain, no Bevy
  ├─ ops.rs            EditOp (Add/Remove/ToggleHopo/Move/SetLen) + apply → inverse
  └─ session.rs        EditorSession: chart + undo/redo stacks + dirty flag

beatbyte-audio         decode, playback, clock — engine-free
  ├─ playback.rs       MusicHandle (mpsc commands, AtomicU64 position) + music thread
  ├─ clock.rs          SongClock: monotonic anchor + reconcile against device position
  └─ analysis/         onsets, tempo, melody, envelope

beatbyte-game          Bevy layer
  └─ editor_ui.rs      EditorState, keyboard input, note drawing, HUD
```

**The layering the brief asks for is already largely in place.** `bb-chart`
≈ `beatbyte-chart` + `beatbyte-core`, `bb-editor` domain ≈
`beatbyte-editor`, and neither depends on Bevy or on the audio crate.
This is the most valuable structural property the brief names, and it
does not need to be built — only kept.

## 2. Time model today

**Seconds are the stored truth. There are no ticks anywhere in the
codebase.**

- `ChartNote { time: f64, lane: u8, len: f64, hopo: bool }` —
  `schema.rs:84-96`. Position and sustain are seconds, `f64`.
- `ChartPhrase { start: f64, end: f64 }` — `schema.rs:100-106`.
- `NoteEvent { time_s: f64, sustain_s: f64 }` — `note.rs`.
- `EditOp` addresses notes **by `f64` time** and compares them with an
  epsilon of 3 ms (`ops.rs:13`, `EDIT_EPSILON_S`).
- `TempoMap` (`timing.rs:28`) stores `TempoChange { time_s, bpm }` and
  converts seconds ↔ beats. `beats_at` (`timing.rs:94`) is a **linear
  scan over all tempo changes on every call** — no cumulative table, no
  binary search.
- The tempo map is **not part of the chart file**. `SongMeta` carries a
  single `bpm: f64` plus `offset_s`; a variable-tempo song cannot be
  represented at all.

Conversion frequency: `beats_at`/`time_at_beats` are not called per
frame in gameplay (the hot path works in seconds directly). The editor
calls `snap`/`tempo` per input event, not per frame.

## 3. Sync path

```text
rodio Sink ──get_pos()──► music thread (2 ms sleep loop)
                              │  playback.rs:327-333
                              ▼
                      AtomicU64 position_us
                              │  MusicHandle::position_s(), playback.rs:232
                              ▼
   Bevy Update ──► SongClock::reconcile(mono, reported)   audio_sys.rs:55
                              │  snap ≥30 ms, else slew 10%   clock.rs:118
                              ▼
                   GameClock::song_time(Time)   audio_sys.rs:25
                              │  anchored to Time::elapsed_secs_f64()
                              ▼
                        gameplay / editor
```

**What is right:** the playhead is *not* frame-delta accumulated — it is
anchored to a monotonic clock and reconciled against the device
position, with snap/slew so the timeline never visibly jumps. The audio
thread does no locking on the read path; the UI reads a relaxed atomic.
Judgment itself never uses this clock — it is input-stamp driven
(`core/session.rs`), which is why presentation changes cannot alter
scores.

**What falls short of the brief:**

- The position is **polled**, not counted in a callback. rodio's `Sink`
  owns the callback, so there is no place to increment a sample
  counter. `get_pos()` is itself sample-derived inside rodio, but it is
  sampled by a **2 ms sleep loop** (`playback.rs:333`), adding up to
  2 ms of quantisation before the UI ever sees it.
- **No sub-frame interpolation of the atomic.** The UI reads whatever
  value the poller last stored.
- **Latency compensation is a single number.** `Settings.latency_offset_ms`
  (`config.rs:26`) conflates output latency, input latency and
  song-specific offset. `SongMeta` has no `chart_offset_ms`.
- **No calibration median.** `calibration.rs` exists and collects taps;
  it is a single combined offset, not the three-way split the brief
  specifies.
- Seek uses `try_seek` with **no fade ramp** (`playback.rs:127`) —
  audible clicks on scrub.
- **No playback-rate control** at all; charting a fast passage means
  playing it at full speed.

## 4. Concrete weaknesses, prioritised

### (a) Correctness / sync

| # | Where | Problem | Impact |
|---|---|---|---|
| A1 | `schema.rs:84`, `ops.rs` | Note positions are `f64` seconds, and edits address notes by float equality within a 3 ms epsilon | Two legitimately distinct notes 2 ms apart are indistinguishable to the editor; a note's position depends on the BPM used when it was written, so changing the tempo silently desynchronises the whole chart. This is the brief's forbidden pattern #1 and #2 at once. |
| A2 | `schema.rs:38-58` | One `bpm` + `offset_s` per song; no tempo map, no time signatures | Variable-tempo songs cannot be charted correctly; there is no bar/beat display that survives a tempo change. |
| A3 | `schema.rs:95` | `hopo: bool` is **persisted per note** | The brief requires HOPO to be a derived function of spacing and lane. Persisting it means an edit that moves a note leaves a stale flag. `convert.rs:81-95` already has a special rule (a chord is HOPO only if all its notes are) that duplicates the derivation in a second place. |
| A4 | `playback.rs:333` | 2 ms polling loop between rodio and the atomic | Up to 2 ms of quantisation on the playhead before any smoothing. |
| A5 | `playback.rs:127` | Seek without fade | Clicks when scrubbing — the brief calls this out explicitly. |
| A6 | `config.rs:26` | One latency value for three physical effects | Cannot compensate a Bluetooth output path and a keyboard independently; per-song offset has nowhere to live. |

### (b) Data-loss risk

| # | Where | Problem | Impact |
|---|---|---|---|
| B1 | `io.rs` `save_chart_file` | Writes **directly to the destination file** — no temp-file + rename | A crash or a full disk during save destroys the chart. The brief's §12 requires the opposite, and this is the single highest-value fix in the audit. |
| B2 | `editor_ui.rs` | **No autosave, no recovery file** | A crash loses the whole session. |
| B3 | `io.rs` | Loading rejects unknown fields? — no: serde defaults tolerate them, but unknown **sections are not preserved** on write | Round-tripping a chart written by a future version silently drops what this version does not understand. |
| B4 | `session.rs:16-17` | Undo/redo stacks are **unbounded** | A long session grows memory without limit; the brief asks for a bounded stack. |

### (c) Performance

| # | Where | Problem | Impact |
|---|---|---|---|
| C1 | `editor_ui.rs:467-481` | `redraw_notes` runs **every frame** and despawns *every* note entity before respawning the visible ones | Per-frame entity churn proportional to the visible window; at 20 000 notes the scan in `editor_ui.rs:486` is over the whole chart every frame. Fails the brief's §9 budget by construction. |
| C2 | `timing.rs:94` | `beats_at` scans all tempo changes linearly | Harmless today (one change), O(n) per call once tempo maps exist. |
| C3 | `stage3d.rs` `apply_note_events` | Iterates all live note entities per feedback message | O(n·m); small today, wrong shape. |
| C4 | `editor_ui.rs:271,345,639` | Note lookup is a **linear `find`** over the difficulty's notes | O(n) per keypress; needs to be a binary search on a sorted vector. |

### (d) Ergonomics

| # | Problem |
|---|---|
| D1 | No waveform display — charting is blind to the audio. |
| D2 | No onset overlay, although `beatbyte-audio::analysis::onset` already computes exactly this for the automatic charter. The data exists and is not shown. |
| D3 | Snap is limited to 1/1, 1/2, 1/4 (`division: u32`); no triplets, no free mode. |
| D4 | No tap-recording mode; notes are placed one keypress at a time. |
| D5 | No metronome and no note-clap. Timing errors are inaudible. |
| D6 | No A/B loop, no playback-rate control. |
| D7 | Selection is a single time range across all lanes (`editor_ui.rs:48`); no box select, no per-lane selection, no copy/paste. |
| D8 | Only 15 keys bound; no minimap, no sections, no status line beyond a note count. |
| D9 | The linter exists (`validate.rs`, 491 lines, good coverage) but is not surfaced as a clickable panel in the editor. |

## 5. Keep / refactor / remove

**Keep as-is**

- The crate layering. `beatbyte-chart` and `beatbyte-core` are already
  free of Bevy and audio; this is what the brief's §4 is asking for.
- `EditOp`'s apply-returns-inverse design (`ops.rs`) and
  `EditorSession::edit_batch`'s atomic rollback (`session.rs:63-86`).
  This is the command pattern the brief specifies, already correct and
  already tested for round-tripping.
- `validate.rs` — the linter's rule set is close to the brief's §8 list.
- `io.rs`'s path hardening (traversal, Windows-drive rejection).
- `SongClock`'s snap/slew reconciliation.

**Refactor**

- **Time model → ticks.** The largest single change, and the root of A1
  and A2. `ChartNote.tick: i64`, `resolution: u32`, tempo map in the
  chart file, seconds derived through a cumulative segment table with
  binary search.
- **HOPO → derived.** Remove `hopo` from the persisted note; compute it
  with a cache invalidated per tick window. `FORCED`/`TAP` flags replace
  the boolean.
- **`EditOp` addressing → tick + lane**, which removes the epsilon
  entirely and makes note lookup an exact binary search.
- **Save → atomic** (temp + fsync + rename).
- **Editor rendering → windowed**, binary search into the sorted note
  list, entity reuse instead of despawn/respawn.
- **Undo stack → bounded** with drag-coalescing (`merge`).

**Remove**

- `EDIT_EPSILON_S` (`ops.rs:13`) — an artefact of float positions.
- The single `latency_offset_ms`, replaced by the three-way split.

## 6. Test coverage today

231 unit tests across the workspace: core 70, chart 52, audio 41, game
49, editor 19. What is covered:

- Timing windows, judgment tiers, scoring, HOPO/tap rules in the
  session engine — thorough.
- Chart validation (every rule), serialisation round-trip, format
  version rejection, path traversal.
- Every `EditOp` and its inverse, batch atomicity, redo invalidation.
- Tempo map beats↔time round-trip across changes.
- Generation determinism and difficulty ordering.

What is **not** covered:

- No property tests (`proptest` is not a dependency).
- No benchmarks (`criterion` is not a dependency) — none of the brief's
  §9 budgets is currently measurable.
- No sync-drift test over a long playback.
- No crash-during-save test.
- No `.chart` import/export at all, so no golden files.
- The editor UI layer (`editor_ui.rs`, 647 lines) has **no tests**; all
  19 editor tests are in the domain crate.

## 7. Forbidden patterns found (brief §11)

| Pattern | Present? | Where |
|---|---|---|
| Floats as note position | **Yes** | `schema.rs:84`, `note.rs`, `ops.rs` |
| Seconds as stored truth | **Yes** | same |
| Playhead from frame-delta accumulation | No | anchored + reconciled, `clock.rs` |
| Mutex/alloc/logging/IO in audio callback | No | rodio owns the callback; our thread only stores atomics |
| Chart mutation bypassing commands | No | `EditorSession` is the only mutator |
| `unwrap`/`expect` outside tests | No | only in doc examples (`lib.rs:28,34`) |
| Blocking IO in the UI thread | **Partly** | chart save/load run inline in `editor_ui.rs`; import already uses `AsyncComputeTaskPool` |
| Re-parsing/re-sorting per frame | **Partly** | `redraw_notes` despawns and rebuilds every frame (C1) |
| Two chart implementations | No | one `ChartFile`, shared |
| String compares in hot paths | No | lanes and difficulties are enums |

## 8. Summary

The foundations are better than the brief assumes: the crate split it
asks for exists, the command pattern exists and is tested, judgment is
already decoupled from presentation, and the linter is substantial.

Three things are genuinely wrong and everything else follows from them:

1. **Notes are stored in seconds**, so the chart is bound to the tempo
   it was written at and the editor has to compare positions with an
   epsilon.
2. **Saving is not atomic**, so a crash at the wrong moment destroys
   work — the only defect here that can lose a user's data.
3. **The editor is blind and mute**: no waveform, no onsets, no
   metronome, no note-clap, no rate control. That, not the data model,
   is why charting a four-minute song currently takes far longer than
   the hour the brief targets.
