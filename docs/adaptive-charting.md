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

A local, versioned **event store**
([ADR-0018](decisions/ADR-0018-gameplay-telemetry-store.md)):
`<data_dir>/beatbyte/telemetry.db`, written off the frame thread and
never uploaded. The game does not read it back to decide anything;
the results screen only ever ADDS what the player says.

Every song start opens one session per player, carrying what the run
was played under:

```text
uid · started/ended · title · artist · genre
chart_hash (chart identity AND version in one value) · difficulty
game version · chart format · generator · scoring version
  · analysis version · vocal pipeline · telemetry schema
device · input offset · video offset · mic offset
tap mode · no fail · practice · autopilot
completion · notes_total · dropped_events · telemetry_complete
```

and then one small row per thing that happened — a logical action, a
judgment with its microsecond offset, a hold that ended, an
overstrum, a hype window, a pause, a seek, a sung note or phrase.
What the player says afterwards (the fun rating, a sentence, the
pairwise verdict) is kept beside the session rather than in the event
stream.

Rules that are load-bearing:

- **`chart_hash` is the content hash of the chart.** Evidence binds
  to the exact notes that were played; an edited or regenerated chart
  starts with zero evidence.
- **Title and artist are separate fields.** The score board's
  `title|artist` key is a known collision (roadmap C5); a new schema
  does not copy a known defect.
- **Sustain endings are recorded**, because dropped holds are the
  evidence that separates "too hard" from "too easy".
- **`autopilot` and `practice` are marked**, and every analysis
  excludes both by default: a perfect robot makes every chart look
  easy and a run at half speed is not a run.
- **Nothing derivable is stored** — no combo, no score, no accuracy,
  no per-judgment counts, no "early / late". `analytics::summary`
  recomputes them from the raw events, which is what makes the play
  history's copy a cache rather than a second truth.
- **Schema-versioned and migrated**, and a shipped migration is never
  edited. An older store opened by a newer build keeps its rows.
- **A lost event is never silent**: a full queue drops it, counts it,
  leaves a gap in the sequence numbers where it happened, and clears
  `telemetry_complete`.
- Writing never affects gameplay: a non-blocking hand-off to a
  bounded queue, batched commits on a worker thread, and a failure
  that warns and drops rather than panicking. Measured: at the most
  talkative level the median frame time is 16.65 ms against 16.66 ms
  with telemetry off.

### The older JSONL files

Layer 1 shipped as one JSONL file per session
(`<data_dir>/beatbyte/telemetry/*.jsonl`): a header line, then one
line per observation. **Nothing writes them any more.** They are
still on disk and still readable, and

```bash
beatbyte-cli telemetry import
```

takes them into the store, once and idempotently. What they cannot
carry survives as a lower detail level rather than as a hole: they
recorded WHICH note but never WHEN, and they had no action stream, no
device and no offsets — so an imported session says
`detail = results` and stays `telemetry_complete`, because nothing
was dropped.

Their format, for reading an old file by hand:

```json
{"schema": 1, "title": "Maria", "artist": "Blondie",
 "difficulty": "medium", "chart_hash": "…", "generator": "0.11.10",
 "started_ms": 1756500000000, "player": 0, "autopilot": false,
 "notes_total": 463}
{"i": 41, "j": "perfect", "off_ms": -12.3}
{"i": 42, "j": "miss"}
{"s": 41, "done": false}
{"o": 1, "near": 42}
{"fun": 4}
{"versus": "better", "parent": "d29f1c07e1a54b32"}
{"comment": "the verse drags, chorus is great"}
```

(`i` = event index into the played track, `j` = judgment, `off_ms` =
signed offset in ms; `s`/`done` = a sustain ended; `o` = an
overstrum, `near` = the most recently judged event when it happened.
`fun` = the one-key rating, `versus` = the pairwise verdict against
`parent`, `comment` = a sentence. `beatbyte-cli telemetry show`
prints a stored session in the same spirit, and
`telemetry export --what session` gives it back as JSON.)

## Layer 2 — Analytics (`beatbyte-cli review`)

Reads all sessions for one (song, difficulty, chart_hash), joins with
the chart, and answers *where*, not just *how well*.

**Where it reads from** is decided in one place
(`beatbyte-cli`'s `telemetry::sessions_for`): the store
([ADR-0018](decisions/ADR-0018-gameplay-telemetry-store.md)) when
there is one, the older JSONL directory when there is not, and
exactly the named directory when `--telemetry-dir` names one. The
review prints which. The two sources were checked against each other
on the same song and produced byte-identical output — the port
changed no answer.

`beatbyte-cli telemetry` sits beside it and asks the questions a
folder of files could not: `problems` (notes of one chart version
missed far more than the rest), `generators` (two generator versions
on comparable material), `calibration` (whether a player lands early
or late), `input` (strums that reached the engine and did nothing),
plus `status`, `list`, `show`, `export` and the one-time `import`.

It answers:

- accuracy, timing mean and stddev **per section** (sections derived
  from the bar grid and energy envelope until charts carry them);
- miss and overstrum clusters (note-index runs, not song averages);
- systematic early/late tendency;
- the boredom signal: sections at ~100 % with tiny timing spread
  across every session.

Output: a human-readable report, and — when evidence thresholds are
met — a **generation directive**:

```json
{"title": "Maria", "artist": "Blondie", "difficulty": "medium",
 "chart_hash": "…",
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
to all of this is "load a chart file". The session itself is
documented step by step in
[`docs/workflow/design-session.md`](workflow/design-session.md).

## Layer 4 — Versions and the ear

- A regenerated chart is written as a sibling (`chart.v2.json`) with
  provenance: parent hash, directive, generator/designer, date.
- It must pass `validate()` plus the playability lints before it can
  be selected at all.
- It becomes the **active** version only after a by-ear A/B against
  the current one — the ADR-0009 rule. The library loads the active
  version; the browser can expose the choice.
- Import never overwrites **any** existing chart — stronger than the
  original wording (only versions with telemetry), because it is
  simpler and strictly safer: a re-import writes the next version and
  moves the pointer.

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
5. **A5 — In-game feedback** *(v0.12.15)*: the results screen takes
   a one-key fun rating (1–5) and, when the played chart carries
   provenance, a LEFT/RIGHT "worse/better than the previous
   version?" verdict — appended to the session log just written,
   surfaced by `review` (mean fun, better/worse tally), zero
   friction when skipped (no log = no hint, ENTER always exits
   untouched). The smallest honest human signal, and the only one
   the plans asked for that telemetry cannot derive.

6. **A6 — The blind taste test** *(v0.15.11)*: `T` in the browser
   plays one thirty-second window of a song **twice**, on two chart
   versions, in a seeded order and unlabelled; the results screen
   asks which was better and reveals which was which only after the
   answer. It records the existing pairwise `versus` line, so blind
   tests and ordinary better/worse verdicts tally in one place.

   The reason it exists: a rating of a whole run measures the song,
   the day and how awake the player is all at once, and two ratings
   from two evenings are barely comparable — which makes the loop's
   slowest step the human one. Hearing the same passage twice in a
   row compares the charting and nothing else, and blind because
   knowing which one is "the new one" decides the answer before the
   music starts.

   What it picks: the folder's active version and the one it came
   from (a redesign writes the next number and moves the pointer, so
   the neighbour is the parent); the window is the chart's own
   preview anchor, or the busiest thirty seconds of the difficulty
   being played. Both charts are **cropped** to it, so each side ends
   by running out rather than by a timer — the end of the run, the
   results snapshot and the telemetry total then work unchanged.

**Parked** with reopen criteria (a real player population, or an
explicit request): population percentiles, automated rollout/A-B
infrastructure, ML preference and skill models, personalization.

## The first graduation (2026-08-30)

The loop produced its first generator improvement: the escalation
pattern that won two by-ear A/Bs is now the generator's default
(`beatbyte-chart::escalation`). This is the intended shape of
progress — a pattern earns its place through the gate, then stops
needing the gate for every new song. Chart versioning is what made it
safe to try: existing files were untouched, and the pattern's first
generator-born chart went into the library as an *inactive* version.

## The rule that outranks the loop

From ADR-0009, unchanged and now structural: **metrics are a guard,
the ear is the arbiter.** No layer of this system may replace the
by-ear comparison against the current reference — the loop exists to
make that comparison better-informed, cheaper and rarer, not to
automate it away.
