# ADR-0008: Data-Driven Theme System

- **Status**: accepted
- **Date**: 2026-08-23

## Context

BeatByte's stages should channel different eras of rock — garage,
punk, metal, stadium, psychedelic, cyber — without per-theme code
paths in gameplay and without shipping art assets.

## Decision

A theme is **data plus one animation system**
(`beatbyte-game::theme`):

```rust
struct Theme {
    background, surface, accent: Color,
    lane_colors: [Color; 5],
    backdrop: Backdrop,       // Starfield | Grid | Flames | Crowd
                              // | Bubbles | Spotlights
    pulse_strength: f32,
}
```

- **Six built-in themes** map palettes to procedural backdrops:
  Garage (warm amber + starfield), Punk (hot pink + pogo crowd),
  Metal (steel + rising embers), Stadium (deep blue + sweeping
  spotlights), Psychedelic (violet + drifting bubbles), Cyber
  (neon + rolling synth grid).
- **Backdrops are engine-drawn pixel sprites** animated by one
  system; beat-aware where it reads well (crowd bounces, stars
  twinkle, grid flashes on the beat). No textures, no shaders, no
  assets — consistent with the "everything generated" policy
  (ADR-0006).
- **Selection is a setting**: a specific theme, or `auto`, which
  rotates deterministically by song title hash — same song, same
  stage, every time.
- **Gameplay reads `ActiveTheme`** for bed/lane/receptor/particle
  colors and the pulse strength; menus keep the constant brand
  palette. Judgment colors never change — readability first.

## Consequences

- Adding a theme is adding one `Theme` literal (and, at most, one
  `Backdrop` variant with its animation arm).
- Lane colors may be tinted per theme but must stay five clearly
  distinct hues; the classic mapping is the default.
- Per-song theme overrides (a chart field) are a format addition
  later, not a redesign.
