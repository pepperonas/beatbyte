# ADR-0002: Cargo Workspace with Layered Crates

- **Status**: accepted
- **Date**: 2026-08-23

## Context

A rhythm game mixes four concerns that change at very different speeds and
have very different testing needs: gameplay rules, file formats, audio/DSP,
and rendering/UI. A monolithic crate would couple them and make the
timing-critical logic hard to test deterministically.

## Decision

One Cargo workspace, layered so that dependencies only point "down":

```text
apps/beatbyte ─────────► beatbyte-game ──► beatbyte-core
beatbyte-cli ──► beatbyte-audio ─────────► beatbyte-core
beatbyte-editor ──► beatbyte-chart ─────► beatbyte-core
```

| Crate | Layer | May depend on Bevy? |
|-------|-------|---------------------|
| `beatbyte-core` | Domain: timing, notes, judgment, scoring, rules | **No** |
| `beatbyte-chart` | Chart format, serialization, validation | **No** |
| `beatbyte-audio` | Decode, playback, song clock, analysis | **No** |
| `beatbyte-game` | Presentation: Bevy plugins, rendering, UI | Yes |
| `beatbyte-cli` | Tooling: analyze/generate/validate | No |
| `beatbyte-editor` | Chart editor (later milestone) | Yes (later) |
| `apps/beatbyte` | Thin binary wiring `beatbyte-game` | Yes |

## Rules

1. `beatbyte-core` is pure: no I/O, no threads, no engine types. Every
   gameplay rule in it must be unit-testable with plain values.
2. `beatbyte-chart` owns the on-disk format and its versioning. It maps
   between the serialized schema and `core` domain types.
3. `beatbyte-audio` may spawn threads (playback) but exposes a
   deterministic, engine-free API. Analysis functions are pure
   `samples in → events out`.
4. `beatbyte-game` contains *presentation and orchestration only*; if a
   rule can be expressed without Bevy, it belongs in `core`.

## Consequences

- Multiplayer, difficulty and scoring logic get exhaustive fast tests
  without spinning up an engine.
- The WASM future stays open: `core`/`chart`/`audio` compile anywhere;
  platform quirks concentrate in `game` and the app shell.
- Slight ceremony cost: some types are mirrored between chart schema and
  domain model. This is deliberate — the file format must be able to
  evolve independently of in-memory representations.
