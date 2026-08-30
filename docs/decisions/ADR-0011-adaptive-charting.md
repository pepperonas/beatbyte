# ADR-0011 — Adaptive charting: AI designs offline, telemetry decides what to redesign

**Status: Accepted** (2026-08-30) · Living spec: [`docs/adaptive-charting.md`](../adaptive-charting.md)

## Context

Four planning documents in `docs/planning/` (written 2026-08-29, each an
iteration of the previous; the last subsumes the rest) envision two
things: **AI-designed song charts** — a designer that interprets a song
rather than transcribing it — and a **closed improvement loop** in which
every gameplay interaction is recorded, analyzed, and fed back into
chart regeneration.

Four facts about this repository constrain how that vision can land:

1. **The per-note signal already exists and is thrown away.**
   `SessionEvent::NoteHit` carries a judgment and a signed timing
   offset for every note, judged from stamped input times (ADR-0004).
   Only one aggregate per (song, difficulty) survives into
   `scores.json`. The plans' "milliseconds are a first-class signal"
   requirement is not new instrumentation — it is *stopping the
   deletion* of data the engine already produces.
2. **BeatByte is a local, offline, single-player game.** There is no
   population of players, no server, no rollout infrastructure. The
   plans' population percentiles, A/B cohorts, preference-learning
   models and cross-player personalization have no data to feed them —
   a plan that pretends otherwise would be built on fiction.
3. **The game is deterministic and self-contained by rule** (no
   network dependency, no secrets, autopilot-reproducible). A model
   call inside the runtime would break all three. The plans themselves
   demand this split: "Claude ist kein Runtime-Controller" (§54).
4. **ADR-0009 is the governing precedent.** A transcription rework
   that measured better on every synthetic metric played *worse* and
   was reverted. The tag `chart-feel-good-20260826` is the by-ear
   quality reference, and the standing rule is: harness metrics are a
   guard, the ear is the arbiter. Any loop that optimizes charts
   against metrics alone will repeat that failure automatically.

## Decision

One architecture, four layers, strictly ordered by dependency:

1. **Telemetry (in-game, always on).** Every session appends one
   compact record per note event — judgment, signed offset, lane(s) —
   plus a session header (song, difficulty, **chart content hash**,
   generator version, completion) to an append-only local log next to
   `scores.json`. Schema versioned; the hash binds every observation
   to the exact chart it was played on, so an edited chart never
   inherits another version's evidence.
2. **Analytics (CLI, offline).** `beatbyte-cli review` joins telemetry
   with the chart and reports *where* a chart fails or bores:
   accuracy and timing spread per section, miss clusters, overstrum
   hotspots — the plans' "local difficulty analysis", sized for a
   household rather than a population (evidence thresholds in
   sessions, configurable, small). When thresholds are met it emits a
   machine-readable **generation directive**: section, problem,
   evidence, recommended change, constraints.
3. **Design (Claude at design time, never at runtime).**
   `beatbyte-cli dossier` exports what the plans call the musical
   representation — analysis, candidate events, structure, constraints,
   plus the telemetry diagnosis — and a design session (Claude Code, or
   a human in the editor) produces a **new chart version** from it.
   The game only ever loads chart files; it never calls a model.
4. **Versioned adoption, gated by the ear.** A regenerated chart is a
   sibling version with provenance (parent, directive, generator
   version), never an overwrite. It must pass the existing
   untrusted-input validation plus playability lints, and it becomes
   the active version only after the by-ear A/B against the current
   one — the ADR-0009 rule, now written into the loop instead of
   around it.

**Deliberately not built** (recorded so it is a decision, not an
omission): population percentile analytics, automated A/B rollout,
ML preference/skill models, cross-player personalization. Reopen
criterion: a real player population, or an explicit request. The
telemetry schema already stores what those layers would need, so
deferring them costs nothing but time.

## Alternatives considered

- **Model call in the runtime** (generate/adapt charts in-app).
  Rejected: breaks offline play, determinism, and the no-secrets rule;
  adds latency where judgment lives; and the plans themselves forbid
  it. Design time is where interpretation belongs.
- **Implement the full ML loop now** (phases 5–12 of the master plan).
  Rejected: no population to learn from, and ADR-0009 demonstrated
  that optimizing against measurable proxies without the ear regresses
  the thing being optimized. The loop can grow into ML once its data
  exists — the reverse order produces models trained on nothing.
- **Auto-apply difficulty adjustments from telemetry.** Rejected: the
  chart-feel reference is a human judgment, and an automatic writer
  would eventually overwrite the version the ear approved. Bounded
  directives + versioned regeneration + by-ear adoption keep the human
  as the arbiter with the machine doing the evidence work.
- **Treat the four planning documents as the spec.** Rejected: they
  are overlapping iterations with population-scale assumptions; kept
  as source material under `docs/planning/`, superseded by this ADR
  and the living spec.

## Consequences

Good: every later ambition — better generation defaults, difficulty
calibration, even eventual learning — feeds off recorded truth about
how charts actually play instead of assumptions; the evidence survives
chart edits because observations are hash-bound; the ear stays in
charge; the runtime stays deterministic and offline.

Costs: a new schema to maintain (versioned from day one); disk for
logs (a full song ≈ tens of kilobytes — negligible); chart versioning
adds a layer to the library; and the loop is only as fast as the human
gate — which is the point, not a flaw.

## Verification

Each layer lands with its own tests (schema round-trip, hash binding,
directive thresholds, version provenance) and the documentation drift
tests bind this ADR into the index and badge. The roadmap carries the
phases; nothing here is done until it is checked off there.
