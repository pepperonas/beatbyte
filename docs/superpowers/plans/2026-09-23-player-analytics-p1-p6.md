# Player Analytics P1–P6 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill Stats shells through Insights and harden for a playable tomorrow-morning build.

**Architecture:** One async RO `TelemetrySnapshot` job (histogram, technique flags, context misses, problem notes + timeline for busiest chart). History tabs stay on `core::stats`. New `plot` primitives: vertical histogram row + heat strip. Mute badge stays out of scope (spec).

**Tech Stack:** Rust, Bevy 0.19, `beatbyte-telemetry::analytics`, `stats_ui`, `plot`

**Spec:** `docs/superpowers/specs/2026-09-23-player-analytics-design.md`

## Global Constraints

- Stats never uses gameplay writer; RO open only.
- Autopilot/practice excluded (honest runs).
- ui_kit / plot only; version bump once for the ship (0.18.18).

## Files

| File | Role |
|------|------|
| `beatbyte-telemetry/src/analytics.rs` | `technique_by_kind`, `timing_bias_ms`, snapshot helpers |
| `beatbyte-game/src/plot.rs` | `spawn_histogram`, `spawn_heat_strip` |
| `beatbyte-game/src/stats_ui.rs` | Wire P1–P5; expand probe → snapshot |
| `beatbyte-cli` | optional `telemetry timing` for fixture parity |
| docs / Cargo / CHANGELOG | P6 |

## Tasks

### Task 1: plot primitives
- [ ] `spawn_histogram` from `[(i32,u32)]` bins → bar row
- [ ] `spawn_heat_strip` from `[f64]` 0..1 → Miss→Perfect colours
- [ ] Unit tests on colour mapping / empty

### Task 2: telemetry aggregators
- [ ] `timing_bias_ms(store) -> Option<f64>` mean signed hit offset
- [ ] `technique_by_kind(store) -> Vec<TechniqueRow>` chord/hopo/sustain/single hit rates
- [ ] `busiest_chart(store) -> Option<(hash, diff, title)>`
- [ ] Tests with fixture store

### Task 3: TelemetrySnapshot job
- [ ] Replace count-only probe with snapshot load (sessions + hist + bias + technique + context + problems + timeline)
- [ ] Apply difficulty/window filters in SQL where possible
- [ ] Supersede on filter change; cancel on exit

### Task 4: Wire tabs P1–P5
- [ ] Timing: hist + bias line + keep history mix
- [ ] Technique: flag bars + context miss bars
- [ ] Progress: windowed accuracy line + consistency sentence (history)
- [ ] Songs: history bests list + problems + heat for busiest chart
- [ ] Insights: ≤5 sample-gated sentences from snapshot + history
- [ ] Versus: add overstrums/drift to duel note when present

### Task 5: P6 ship
- [ ] Version 0.18.18, CHANGELOG, ROADMAP G45, harness note
- [ ] Gate + push + `cargo build --release -p beatbyte --features ml`
