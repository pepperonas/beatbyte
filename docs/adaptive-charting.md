# Adaptive charting

How AI chart design and telemetry-driven improvement fit into
BeatByte. This is the living spec for [ADR-0011](decisions/ADR-0011-adaptive-charting.md);
the raw source material lives in [`docs/planning/`](planning/).

The one-line architecture:

```text
game records the truth → CLI turns it into evidence → Claude designs
offline → a new chart VERSION → validation → the ear decides
```

The runtime never calls a model. The model never touches a chart the
ear has approved. Nothing overwrites anything.

## What already exists (do not rebuild)

| Need (from the plans) | Already in the tree |
|---|---|
| Millisecond timing per note | `SessionEvent::NoteHit { judgment, offset_s }` — produced every hit, currently discarded after the session |
| Musical analysis | `SongAnalysis`: bpm + confidence, beat grid, onsets, energy envelope, melody notes (beatbyte-audio) |
| Deterministic generation | melody-first master → derived difficulties (beatbyte-chart), seeded, bit-reproducible |
| Playability constraints | `DifficultyProfile` (chord size, gaps, sustain rules) + `validate()` caps |
| Correction pass | the in-game editor, ops invertible |
| Quality reference | tag `chart-feel-good-20260826` — the by-ear arbiter (ADR-0009) |

## Layer 1 — Telemetry (foundation; everything depends on it)

A session log, written by beatbyte-game beside `scores.json`
(`<data_dir>/beatbyte/telemetry/`), append-only JSONL, one file per
session. Never uploaded, never read by the runtime.

Session header (first line):

```json
{"schema": 1, "song": "maria|blondie", "difficulty": "medium",
 "chart_hash": "…", "generator": "0.11.8", "started": "…",
 "completed": true, "score": 139968, "notes": 463}
```

One line per note event thereafter:

```json
{"i": 41, "j": "perfect", "off_ms": -12.3}
{"i": 42, "j": "miss"}
{"o": 1}
```

(`i` = event index into the chart's track, `j` = judgment, `off_ms` =
signed offset; `o` = overstrum count at that moment.)

Rules that are load-bearing:

- **`chart_hash` is the content hash of the chart file.** Evidence
  binds to the exact notes that were played. An edited or regenerated
  chart starts with zero evidence — inherited statistics would judge
  notes that no longer exist.
- **Schema versioned from day one** (`schema: 1`); readers skip
  records they do not understand rather than failing the run.
- Writing must never affect gameplay: buffered, flushed at session
  end; a write failure logs and drops, never panics.

## Layer 2 — Analytics (`beatbyte-cli review`)

Reads all sessions for one (song, difficulty, chart_hash), joins with
the chart, and answers *where*, not just *how well*:

- accuracy, timing mean and stddev **per section** (sections derived
  from the bar grid and energy envelope until charts carry them);
- miss and overstrum clusters (note-index runs, not song averages);
- systematic early/late tendency;
- the boredom signal: sections at ~100 % with tiny timing spread
  across every session.

Output: a human-readable report, and — when evidence thresholds are
met — a **generation directive**:

```json
{"song": "maria|blondie", "difficulty": "medium", "chart_hash": "…",
 "section": {"bars": [33, 40]},
 "problem": "miss_cluster",
 "evidence": {"sessions": 5, "accuracy": 0.62, "timing_stddev_ms": 44},
 "recommend": ["reduce_density", "simplify_lane_movement"],
 "constraints": ["preserve_musical_identity", "stay_playable"]}
```

Thresholds are sized for a household, not a population (default:
**3 sessions** of the same chart_hash before any directive; values
configurable). A single bad run changes nothing.

## Layer 3 — Design (`beatbyte-cli dossier` + Claude at design time)

`dossier` exports one file per song: the musical representation
(analysis summary, melody notes, beat grid, structure guesses, energy
curve), the current chart, the playability constraints, and any open
directives. A design session — Claude Code following the philosophy
distilled from `docs/planning/` (musical feel over density, salience
over completeness, pauses are gameplay, four independent difficulty
designs) — produces a **new chart version** from it.

This is a workflow, not a runtime feature: the game's only interface
to all of this is "load a chart file".

## Layer 4 — Versions and the ear

- A regenerated chart is written as a sibling (`chart.v2.json`) with
  provenance: parent hash, directive, generator/designer, date.
- It must pass `validate()` plus the playability lints before it can
  be selected at all.
- It becomes the **active** version only after a by-ear A/B against
  the current one — the ADR-0009 rule. The library loads the active
  version; the browser can expose the choice.
- Import never overwrites a version that has telemetry.

## Phases (mirrored in the roadmap)

1. **A1 — Telemetry recorder** in beatbyte-game (schema above, tests
   for round-trip, hash binding, failure isolation).
2. **A2 — `beatbyte-cli review`**: per-section report + directives
   with thresholds.
3. **A3 — Chart versioning**: sibling files, provenance, active
   pointer, validation of the new fields (charts stay untrusted
   input).
4. **A4 — `beatbyte-cli dossier`** + the design-session workflow,
   with the by-ear gate written into `docs/workflow`.
5. **A5 — In-game feedback** (optional, later): a one-key fun rating
   on the results screen; a pairwise "which felt better?" when two
   versions of a chart exist. The smallest honest human signal, and
   the only one the plans ask for that telemetry cannot derive.

**Parked** with reopen criteria (a real player population, or an
explicit request): population percentiles, automated rollout/A-B
infrastructure, ML preference and skill models, personalization.

## The rule that outranks the loop

From ADR-0009, unchanged and now structural: **metrics are a guard,
the ear is the arbiter.** No layer of this system may replace the
by-ear comparison against the current reference — the loop exists to
make that comparison better-informed, cheaper and rarer, not to
automate it away.
