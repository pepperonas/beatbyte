# Player Analytics & Performance Insights — Design

**Date:** 2026-09-23  
**Status:** Approved in chat (sections 1–5); awaiting final spec review before implementation plan  
**Related:** ADR-0018 (telemetry store), ADR-0016 (diagrams / `plot.rs`), G40 Stats UI (`stats_ui.rs`, `core::stats`)

## Goal

Extend BeatByte’s existing statistics suite so it answers concrete player questions (am I improving? where do I fail? what should I practise?) without replacing the current Overview / Timing / Difficulty / Versus architecture, and without inventing a second note-performance log.

## Locked decisions

| Decision | Choice |
|----------|--------|
| Store read policy | **A** — in-game Stats UI may open `telemetry.db` **read-only**. Telemetry never writes back into gameplay. Analytics produce evidence only (ADR-0018). |
| Persistence architecture | **Approach 1 — Dual-source, on-demand.** Run/career views stay on `history.jsonl` + `beatbyte_core::stats`. Note/passage/weakness views call `beatbyte_telemetry::analytics::*` (and new pure aggregators) via async jobs. No rollup DB until measured cost forces Approach 2. |
| New event log | **None.** Existing `gameplay_event` / session / context are enough. |
| Mute badge / other UI | **Out of scope** for this spec (separate task). |

## Out of scope (until need is proven)

- Invented genre taxonomy when `song.json` lacks genre
- Named song sections (Verse/Chorus) — charts do not carry them
- Approach 2 (rebuildable analytics projection) unless P0–P5 measurements force it
- Approach 3 (telemetry-first unified reader that retires history as Stats primary)
- Automatic training or chart self-rewrite from analytics

---

## 1. Navigation & views

Existing Stats tabs are **extended**, not replaced. New tabs only where today’s four cannot answer the brief’s questions.

| Tab | Primary source | Role |
|-----|----------------|------|
| **Overview** | history | Career totals + denser trends |
| **Timing** | telemetry | Histogram, early/late bias, consistency |
| **Technique** *(new)* | telemetry + chart join | Frets, patterns, chords, sustains, transitions |
| **Difficulty** | history (+ telemetry where useful) | Existing view + trend/NPS when samples allow |
| **Progress** *(new)* | history (+ events for recovery) | Windows 7/30/90/All, consistency, miss recovery |
| **Songs** *(new)* | history + telemetry deep dive | Lists, deep dive, timeline heatmap |
| **Versus** | history | Standing vs other players; extended metrics |
| **Insights** *(new)* | derived | Few, sample-gated sentences — no fake precision |

**Global filters:** Player · Difficulty · Window (7/30/90/All). Song/Genre only on Songs.  
**Sections:** Timeline by time / note index / NPS — never invented section names.  
**Unlinked plays:** Show honesty when `song_id` is missing; do not hide rows.  
**Input:** Controller-first via `ui_kit` (keyboard / gamepad / guitar as today); mouse via existing row/chip patterns.

---

## 2. Data contract

### Principle

Raw events remain the source of truth. The Stats UI does not persist a second note log and does not write into the play path. Aggregations are **derivable** and recomputed when needed.

### Already persisted (consume)

| Layer | Grain | Relevant fields |
|-------|-------|-----------------|
| `gameplay_event` | note / action | `note_index`, `song_time_us`, `EventType` (Hit/Miss/Overstrum/Sustain…), `delta_us`, `rating`, `Flags` (`CHORD` / `HOPO` / `SUSTAIN` / `ASSISTED` / …) |
| `gameplay_session` | run | `chart_hash`, difficulty, version stamps, `autopilot` / `practice`, outcome, optional `song_id` |
| `note_context` | chart note | onset/energy/… — **reference**, not performance |
| `history.jsonl` + `core::stats` | run / career | score, accuracy, streak, judgment mix, drift, versus |

### Derived only (never store)

Accuracy, perfect share, histogram bins, fret/pattern buckets, miss chains, recovery, trends, comfort zone, insight text, confidence / `min_samples`.

### Joins

- Event → `chart_hash` + `note_index` → loaded chart (lane, NPS, chord shape).
- Song lists → `song_id` when attached; otherwise label as unlinked.
- No fabricated section labels.

### Honesty filters

Every analytical query: `autopilot = 0 AND practice = 0` (same as CLI `analytics`).  
Where Technique/Timing measure the player’s hand, treat `ASSISTED` hits as excluded or separately labelled.  
Thin samples: show the value with confidence; do not emit strong insights.

### Schema

No new analytics schema for P0–P5. Existing telemetry schema version + history suffice. New `EventType`s only if a required question cannot be reconstructed — then ADR amendment first.

---

## 3. Engine & read path

### Existing `analytics::*` → UI

| API | Tab | Use |
|-----|-----|-----|
| `timing_histogram`, `calibration_bias` | Timing | Distribution, early/late bias |
| `note_quality`, `problem_notes` | Songs / Technique | Problem spots + confidence |
| `misses_by_context` | Technique / Songs | Miss buckets by context |
| `summary` | optional / parity | Session cross-check, not primary UI |
| `generator_comparison`, `orphan_strums`, `incomplete_sessions` | **not** player Stats | Engine / tooling evidence |

### New pure aggregators

Add only what tabs need and events+chart can reconstruct:

- **Technique:** fret / pattern / chord / sustain / transition aggregates
- **Progress:** windowed series, consistency, miss-recovery
- **Insights:** small rule set over aggregates + `min_samples` / confidence (shape of `NoteQuality::confidence`)

No frame loop; no write paths from analytics.

### Async in-game path

Pattern matches Librarian / Import (`AsyncComputeTaskPool`):

1. Stats enter or tab/filter change → spawn job.
2. Job opens `telemetry.db` **read-only** on its own connection (never the gameplay writer handle).
3. Result lands in a Bevy resource; UI shows a loading placeholder until ready.
4. OnExit or superseding job → drop stale task; never paint superseded results.
5. History / `core::stats` stay synchronous on the main thread (already small).

### Sample defaults

- Default `min_samples` aligned with CLI (e.g. ~10 notes / enough runs).
- Below threshold: dimmed numbers, no insight sentence.
- Lazy per tab; no precompute DB. Revisit Approach 2 only if a tab measures > ~200 ms block or writer lock contention.

---

## 4. UI (`ui_kit` + `plot`, ADR-0016)

No second charting library. House palette and type scale only.

### Tab questions (subtitles)

| Tab | Question |
|-----|----------|
| Overview | Am I getting better? |
| Timing | Early, late, or just noisy? |
| Technique | Which frets and patterns break me? |
| Difficulty | Where do I play, and how far? |
| Progress | Steady, or streaky? |
| Songs | What should I practise next? |
| Versus | How do I stand against the others? |
| Insights | What matters right now? |

### Diagram mapping

| Need | Means |
|------|--------|
| Trends / Progress | `spawn_line_plot` |
| Difficulty / frets / patterns | `spawn_bar_plot` |
| Judgment mix | `spawn_stack` |
| Versus | `spawn_duel_plot` |
| Timing histogram | **New:** pure bins → bar row |
| Song timeline | **New:** single-row heat strip (Miss→Perfect palette) |

### Filter chrome

One row under tabs: Player · Difficulty · Window. Song/Genre on Songs only.  
Navigate like settings rows / chips; mouse activates chips. Filter change retriggers async job.

### Empty / loading / thin

- Zero runs → `empty_note` with an honest invite to play.
- Telemetry loading → “…” — never fake zeros.
- Below `min_samples` → dimmed value, suppressed insight.
- Missing `song_id` → explicit “(no library id)”.

### Layout

Header + tabs + filter + one scroll panel + footer/Back. No bespoke font sizes or panels. Insights: at most ~5 sentences, ranked by confidence × impact.

### Harness

Extend `BEATBYTE_SHOT_STATE=stats` (or equivalent) to cover all eight tabs; empty and “many runs” fixtures where useful; luma-check shots.

---

## 5. Implementation phases

| Phase | Ship | Definition of Done (core) |
|-------|------|---------------------------|
| **P0 — Foundation** | RO store open + async job pattern in Stats; filter chrome; expanded tab enum (empty shells OK) | No frame block; cancel on exit; tests pin RO open |
| **P1 — Timing deep** | Histogram + bias/consistency wired from existing analytics | CLI and UI agree on a fixture |
| **P2 — Technique** | Frets → patterns → chords → sustains; pure aggregators + bars | Aggregator mutation tests; empty/thin states |
| **P3 — Progress + Overview trends** | 7/30/90/All, consistency, miss recovery; Overview trend lines | History path still correct |
| **P4 — Songs** | Lists + deep dive + timeline heat; `note_quality` / `problem_notes` | Unlinked honesty; confidence visible |
| **P5 — Insights + Versus extend** | ≤5 insight rules with sample gates; richer Versus metrics | No insight under `min_samples` |
| **P6 — Hardening** | Measured perf, shot harness all tabs, docs/ROADMAP; projection only if forced | Quality gate green; shots verified |

Each phase ends in a releasable, gate-green state on `main`.

---

## Success criteria

- Player questions from the brief are answerable from the eight tabs without a second note log.
- Stats never blocks the frame on SQLite; gameplay writer path untouched.
- Autopilot/practice never pollute player-facing aggregates.
- Numbers that look precise are either well-sampled or visually demoted.
- Diagrams stay in `plot.rs` / `ui_kit` (ADR-0016).

## Next step

After human review of this file: write an implementation plan (`writing-plans`) starting at **P0**, then implement phase by phase.
