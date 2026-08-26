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

Two crates sit beside that stack rather than inside it, because both
are tools built ON the domain layer and nothing depends on them:
`beatbyte-editor` (invertible chart edits) and `beatbyte-cli`
(analyze / generate / validate / inspect / demo).

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

## Documentation map

- `docs/decisions/` — Architecture Decision Records ([index](../decisions/README.md))
- `docs/gameplay/` — gameplay rules, judgment windows, scoring
- `docs/audio/` — analysis pipeline, known limitations
- `docs/chart-format/` — the versioned chart file format
- `docs/development/` — developer workflow, asset licensing
- `docs/ui/` — the menu and settings design system
- `docs/releases/` — release process
