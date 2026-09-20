# BeatByte Architecture Overview

BeatByte follows a layered architecture. Dependencies point downward only.

```text
┌───────────────────────────────────────┐
│              Presentation             │  beatbyte-game, apps/beatbyte
│       UI / Rendering / Effects        │
├───────────────────────────────────────┤
│               Gameplay                │  beatbyte-core (players, scoring,
│  Notes / Scoring / Combos / Players   │  judgment, special meter)
├───────────────────────────────────────┤
│                Domain                 │  beatbyte-core (timing model),
│  Chart / Timing / Events / Rules      │  beatbyte-chart (format)
├───────────────────────────────────────┤
│             Infrastructure            │  beatbyte-audio (decode, playback,
│   Audio / Files / Input / Platform    │  analysis), platform glue in game
└───────────────────────────────────────┘
```

Three crates sit beside that stack rather than inside it, because
each is built ON the domain layer and nothing in the stack depends on
them: `beatbyte-editor` (invertible chart edits), `beatbyte-telemetry`
(the gameplay event store — the presentation layer writes to it, and
that is the only edge that points at it) and `beatbyte-cli`
(analyze / generate / validate / inspect / review / read the store).

## The loop the data closes

Everything above is one direction: a song becomes a chart becomes a
game. Telemetry is what makes the arrow come back.

```text
  SONG ──▶ ANALYSIS ──▶ SONG GRAPH ──▶ CHART GENERATOR ──▶ CHART
                                                             │
                                                             ▼
                                                         GAMEPLAY
                                                             │
                                                             ▼
   IMPROVEMENT ◀── EVIDENCE ◀── ANALYTICS ◀────────────── TELEMETRY
        │                           ▲                        │
        │                           └── chart + note context ┘
        ▼
  A NEW CHART VERSION  ──▶  the ear decides  ──▶  measured again
```

Two things about that last arrow, and they are the whole point:

- **Analytics produce evidence, never a change.** Nothing in
  `beatbyte-telemetry` writes a chart, a score, a difficulty or a
  setting, and the game never reads the store back to decide
  anything. A note that is missed by everyone becomes a candidate on
  a list; a person listens, and a new chart VERSION is written beside
  the old one. That is [ADR-0011](../decisions/ADR-0011-adaptive-charting.md)'s
  rule and [ADR-0018](../decisions/ADR-0018-gameplay-telemetry-store.md)
  keeps it.
- **There is no self-learning here, deliberately.** No training, no
  automatic regeneration, no adaptive difficulty. Those would each be
  their own decision, with their own versioning, and none of them is
  made easier by pretending the loop already closes itself.

The fourth arrow — `chart + note context` — is why a
`*.context.json` sidecar sits beside each chart version: the analysis
that made the notes is otherwise gone by the time anybody misses one,
and "was that a weak onset in a loud passage?" is unanswerable
without it.

## Key invariants

- **Gameplay timing derives from the song clock, never from frame
  counts.** The renderer asks "where is note X at song time T", not the
  other way around. See [ADR-0004](../decisions/ADR-0004-gameplay-timing.md).
- **The chart model is engine-free.** `beatbyte-chart` and
  `beatbyte-core` compile without Bevy; the editor, CLI and future WASM
  builds reuse them unchanged.
- **Audio analysis is a pipeline of pure stages.** Decoding produces
  samples; feature extraction produces beats/onsets; chart generation
  consumes musical events. Each stage is testable in isolation.
- **Players are data.** Multiplayer works by instantiating N player
  states, not by duplicating systems.
- **Menus share one kit, not just one font.** `ui_kit` owns the type
  scale, the spacing rhythm and the row states; no screen invents its
  own. See [ADR-0010](../decisions/ADR-0010-ui-design-system.md).
- **Editor operations are invertible.** `EditOp::apply` returns the
  inverse, which is what makes undo/redo correct by construction.
- **Telemetry is events, never frames — and never on the frame
  thread.** A recorded event is a thing that happened, not a sample of
  state; it names its note and its session and joins the rest. It is
  handed to a bounded queue with a non-blocking send and committed in
  batches by a worker thread. A full queue drops, counts, and says so.
  See [ADR-0018](../decisions/ADR-0018-gameplay-telemetry-store.md).
- **Nothing derivable is stored.** Not combo, not score, not accuracy,
  not "early / late" — those are computed from the raw events, and two
  copies of one fact are two facts that can disagree.

## Documentation map

- `docs/decisions/` — Architecture Decision Records ([index](../decisions/README.md))
- `docs/gameplay/` — gameplay rules, judgment windows, scoring
- `docs/audio/` — analysis pipeline, known limitations
- `docs/chart-format/` — the versioned chart file format
- `docs/development/` — developer workflow (including how to read
  the telemetry store), asset licensing
- `docs/ui/` — the menu and settings design system, and the 3D stage
- `docs/releases/` — release process
