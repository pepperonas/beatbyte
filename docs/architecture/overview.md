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

## Key invariants

- **Gameplay timing derives from the song clock, never from frame
  counts.** The renderer asks "where is note X at song time T", not the
  other way around. See ADR-0005 (gameplay timing).
- **The chart model is engine-free.** `beatbyte-chart` and
  `beatbyte-core` compile without Bevy; the editor, CLI and future WASM
  builds reuse them unchanged.
- **Audio analysis is a pipeline of pure stages.** Decoding produces
  samples; feature extraction produces beats/onsets; chart generation
  consumes musical events. Each stage is testable in isolation.
- **Players are data.** Multiplayer works by instantiating N player
  states, not by duplicating systems.

## Documentation map

- `docs/decisions/` — Architecture Decision Records (start at ADR-0001)
- `docs/gameplay/` — gameplay rules, judgment windows, scoring
- `docs/audio/` — analysis pipeline, known limitations
- `docs/chart-format/` — the versioned chart file format
- `docs/development/` — developer workflow, asset licensing
- `docs/releases/` — release process
