# ADR-0001: Rust + Bevy as the Technology Foundation

- **Status**: accepted
- **Date**: 2026-08-23

## Context

BeatByte is a cross-platform rhythm game with hard requirements on input
latency, audio synchronization, deterministic gameplay timing, and a future
WebAssembly target. The engine choice shapes everything downstream.

## Decision

- **Language: Rust** (stable channel, currently 1.95).
- **Engine: Bevy 0.19** (latest stable at project start, verified against
  crates.io on 2026-08-23).

## Rationale

- Rust gives predictable performance without GC pauses — critical for a
  game where a single dropped frame is a perceivable timing artifact.
- Bevy's ECS matches our data-driven requirements (themes, charts,
  multiplayer players as plain data) and its plugin architecture maps
  cleanly onto our crate separation.
- Bevy targets Windows/macOS/Linux and WASM from one codebase.
- Bevy 0.19 requires Rust 1.95, which defines our MSRV.

## Consequences

- Bevy has breaking releases roughly every 3–4 months. Upgrades are
  scheduled work, not drive-by changes; the engine-free crates
  (`core`, `chart`, `audio`) are insulated from them by design.
- Bevy's built-in audio (`bevy_audio`) does not expose a precise playback
  position, which a rhythm game needs. Music playback and the song clock
  live in our own `beatbyte-audio` crate (see ADR-0003); `bevy_audio`
  remains acceptable for fire-and-forget UI/menu sounds.
- Compile times are managed via the standard Bevy profile setup
  (`opt-level = 1` for our code, `opt-level = 3` for dependencies).
