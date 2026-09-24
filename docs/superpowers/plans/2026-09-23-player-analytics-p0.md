# Player Analytics P0 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land Stats foundation: read-only `telemetry.db` open, async job pattern, eight tabs (empty shells for new ones), and filter chrome — without blocking the frame.

**Architecture:** Dual-source on-demand (spec). History stays sync; telemetry opens RO on `AsyncComputeTaskPool`. No new event schema.

**Tech Stack:** Rust, Bevy 0.19, `beatbyte-telemetry` / `rusqlite`, `beatbyte-game` `stats_ui` + `ui_kit` + `plot`

**Spec:** `docs/superpowers/specs/2026-09-23-player-analytics-design.md` (P0 row)

## Global Constraints

- Stats never uses the gameplay `Telemetry` writer handle.
- RO open must not run migrations (writes).
- Autopilot never counted in player-facing aggregates (existing `Filter` / CLI honesty).
- `ui_kit` type scale / panels only; ADR-0016 for diagrams.
- User-visible → version + CHANGELOG in the same commit series.

## Files

| File | Role |
|------|------|
| `crates/beatbyte-telemetry/src/store.rs` | `Store::open_readonly` |
| `crates/beatbyte-telemetry/src/lib.rs` | re-export if needed |
| `crates/beatbyte-game/src/stats_ui.rs` | tabs, filters, async probe, shells |
| `CHANGELOG.md` / `Cargo.toml` / `docs/ROADMAP.md` | ship notes |

---

### Task 1: `Store::open_readonly`

- [x] Failing test: after `open` + one session, `open_readonly` can `session_count`; `begin` fails (or insert fails).
- [x] Implement `open_readonly` with `OpenFlags::SQLITE_OPEN_READ_ONLY` (no `prepare`/migrate).
- [x] Missing file → `Err` (caller shows empty).
- [x] `cargo test -p beatbyte-telemetry open_readonly`

### Task 2: Expand Stats tabs + empty shells

- [x] `View::ALL` → 8 tabs; update `label` / `question` / `step` / `view_named` / tests.
- [x] Match arms: Technique / Progress / Songs / Insights → `plot::empty_note` placeholders.
- [x] Existing four views unchanged in content.

### Task 3: Filter chrome

- [x] `StatsFilters { difficulty: Option<Difficulty>, window: TimeWindow }` resource.
- [x] Spawn chip row under tabs; mouse + keyboard cycle (document keys in footer).
- [x] Apply difficulty + window when building `runs` for Overview/Timing/Difficulty/Versus.
- [x] Changing filter respawns panel (same as tab change).

### Task 4: Async RO probe

- [x] Resource holding `Option<Task<Result<u64, String>>>` + generation + last `session_count`.
- [x] OnEnter / filter-or-tab change: spawn job that `open_readonly(store_path)` → `session_count`.
- [x] Poll system: never `block_on` unless task finished; cancel/supersede on exit.
- [x] New-tab shells show "…" then "TELEMETRY · N SESSIONS" or "NO TELEMETRY STORE".
- [x] Unit/wired test: RO open path is used (or pure helper `probe_session_count(path)` pinned).

### Task 5: Docs + gate

- [x] Bump to next patch, CHANGELOG, ROADMAP note under analytics / Stats.
- [x] `cargo fmt` / `clippy -D warnings` / `test -p beatbyte-telemetry` / `test -p beatbyte-game stats`
