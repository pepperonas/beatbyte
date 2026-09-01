# ADR-0012 — The 8-bit look is data behind the style boundary, not a second renderer

**Status: Accepted** (2026-09-01)

## Context

BeatByte ships two note looks: the colorblind-safe **8-bit per-lane
shapes** (the default) and **round gems**. The UX commission of
2026-09-01 asked for the retro look to be isolated behind a
rendering boundary so that retro-mode conditionals never spread
through gameplay, input or UI code, with the modern renderer as the
primary target.

## Decision

There is **one renderer**. The 8-bit look is *data* flowing through
it: per-style textures (`LaneShapes` — procedural, generated at
startup), a per-style particle sprite, and a per-style camera
contract. The style switch is a single settings field
(`round_gems`), consumed in exactly three places:

1. **Texture selection** at spawn/build time (`LaneShapes`
   accessors, `NoteAssets`).
2. **Particle sprite selection** (`EffectSettings.round_particles`,
   mirrored from settings by the one `apply_settings` system).
3. **The camera contract** (`sync_bloom`): the round style runs
   HDR + bloom, the 8-bit style runs SDR — and because two cameras
   sharing a window must agree on HDR, this ONE system keeps both
   cameras in step per style. It exists precisely so the agreement
   lives in one place instead of leaking into every camera setup.

Gameplay, input, judgment, the UI kit and the menus contain **no
note-style conditionals**. Judgment is input-stamp-driven and
identical under both looks (the autopilot proves it in both).

## Alternatives considered

- **A forked renderer** (`RetroRenderer` / `ModernRenderer` behind a
  trait): rejected — the two looks share the highway, the lanes, the
  sustains, the timing and the layout; a fork would duplicate all of
  it to vary textures and one camera flag.
- **Retiring the 8-bit look**: rejected — it is the colorblind-safe
  presentation; turning it off makes colour the only lane signal.

## Consequences

- A new style is a new texture set + a camera-contract entry, not a
  new code path.
- The dormant flat-2D highway path in `notes.rs` (the removed
  "depth" view) is unrelated to the style boundary; its pruning
  stays a filed roadmap task and must not be confused with the
  8-bit look, which is alive and default.
- The rule for changes: touch the 8-bit data only for compilation,
  shared APIs, bug fixes or boundary changes — the modern (round)
  style must never be compromised to keep the retro data simple,
  and vice versa.
