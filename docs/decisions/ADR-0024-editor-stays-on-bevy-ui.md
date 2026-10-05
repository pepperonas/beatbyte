# ADR-0024 — The chart editor stays on Bevy UI; `bevy_egui` only if it grows forms

**Status: Accepted** (2026-10-05, phase 5 of the menu-system programme,
`docs/plans/menu-system.md`: "an ADR draft on whether `bevy_egui` would
make the chart editor easier to maintain, with an effort estimate — no
implementation". The user accepted the recommendation the same day.)

## Context

The menu programme moved every row-cursor screen onto one renderer
(`menu_list`, ADR-0023). The chart editor was left out on purpose: it is
not a list, it is a timeline with a few forms around it. The brief asked
whether an immediate-mode UI library — `bevy_egui` — would make it
easier to keep alive.

What the editor is, measured on 2026-10-05 (`crates/beatbyte-game/src/editor_ui*`,
code lines without tests):

| Part | Where | Lines | Nature |
|---|---|---:|---|
| Keyboard dispatch | `editor_input` | 447 | every action has a key |
| Timeline drawing | `draw::redraw` | 403 | notes, grid, lanes, drag previews, as world sprites |
| Pointer | `pointer::editor_pointer` | 307 | place, select, drag, box, scrub, zoom |
| Waveform | `collect_waveform` + pool | 142 | fixed row pool, resized not respawned |
| Opening a song | `open_editor` | 159 | loading, not UI |
| **Form-like parts** | | **≈ 650** | |
| — HUD / info block | `draw::refresh_hud` | 159 | text lines |
| — layout | `draw::spawn_editor` | 179 | toolbar chips, panels |
| — dialogs | `dialog::sync_dialog` + clicks | ≈ 150 | SAVE / DISCARD / CANCEL, NEW / OVERWRITE |
| — right-click menu | `draw::sync_menu` | 66 | delete, HOPO, copy, … |
| — typed fields | `editor_typing` | 78 | type time / lane / length |
| — toolbar | `editor_chips` | 54 | `ui_kit` chips |

The logic underneath is already engine-free and tested: `beatbyte-editor`
(4 026 lines, 85 tests) holds the geometry (what the pointer is over,
what a drag means), the operations with their inverses, the inspector,
lint, clipboard and the saver; `dialog.rs` decides its steps purely
(127 test lines). The Bevy side asks and draws.

`bevy_egui` 0.42.0 (2026-08-16) targets Bevy 0.19 and egui 0.36;
0.43.0-rc.1 already follows Bevy 0.20's release candidate.

## Decision

**Keep the editor on Bevy UI and `ui_kit`.** Do not add `bevy_egui` now.

Revisit when the editor gains **real forms** — metadata editing,
tables of tempo or section markers, many numeric fields at once. Then
adopt it **for those panels only** (option B below), never for the
timeline.

## Why

1. **The hard part is not the part egui helps with.** About 1 300 of the
   editor's lines are the timeline: drawing it, pointing at it, driving
   it from the keyboard. egui's strength is forms; a timeline in egui is
   a custom `Painter` widget, i.e. the same geometry code rewritten
   against another drawing API — and away from the world sprites that
   share the game's highway look.
2. **The forms are small and already tested.** About 650 lines, and the
   one piece of them that had real decisions in it (the dialogs) is
   pure and pinned. Replacing them would trade tested code for new
   untested code to save perhaps 300 lines.
3. **It would be a second design system.** CLAUDE.md: "menus share one
   design, not just one font" — `ui_kit` owns the type scale, spacing,
   row states and chips, and a test forbids near-duplicate sizes. egui
   brings its own style, focus model, widgets and text rendering; making
   it look like BeatByte means a custom `egui::Style` that mirrors
   `ui_kit` and has to be kept in step by hand.
4. **Two input owners.** Every editor system would have to ask egui
   first (`wants_pointer_input`, `wants_keyboard_input`) — the same
   class of bug as a dialog that lets keys through to the timeline,
   which the editor just fixed by making dialogs swallow input.
5. **The camera contract.** The game runs two cameras on one window,
   both HDR with bloom (CLAUDE.md, "Two cameras on one window carry TWO
   contracts"). In `bevy_egui` 0.42 the 2D pass is ordered
   `.after(Core2dSystems::MainPass).before(upscaling)` (`src/lib.rs`,
   ~line 1215) — **not** after post-processing, so whether egui panels
   pass through bloom and tonemapping on the HDR 2D camera is open and
   would have to be measured before adoption. A third rendering path is
   exactly where this project has lost whole frames before.
6. **Upgrade coupling.** Every Bevy upgrade then waits for a matching
   `bevy_egui` release (historically days to weeks after Bevy's).

## Alternatives considered

**A — Rewrite the whole editor in egui** (timeline as a `Painter`
widget). Rejected: ≈ 2 500 lines touched, the timeline loses the world
sprites and the highway look, the autopilot's edit drill (which drives
keys and an injected pointer, `InjectedPointer`) needs rewriting, and
the screenshot harness gains a renderer it cannot yet trust (point 5).
Estimate: **5–8 working sessions**, plus the visual re-verification of
every editor state.

**B — egui only for the form-like panels** (HUD, dialogs, menu, typed
fields; timeline stays). Not now: ≈ 650 lines replaced by perhaps
≈ 350, but with a second design system, an input arbitration layer and
the camera question in exchange. Estimate: **2–3 sessions**, of which
one is the camera/HDR measurement and one the style mirror of `ui_kit`.
This becomes worth it when the forms grow (see Decision).

**C — Do nothing beyond keeping the forms on `ui_kit`** (chosen).
Further tidying can follow the menu programme's pattern: move the
remaining decisions (typed-field parsing, menu entries per context)
into `beatbyte-editor` as pure functions, as the dialogs already are.

## Consequences

Good:
- One design system and one input path stay the rule across the game.
- No new dependency, no new render path, no upgrade coupling.

Costs:
- The editor's forms stay hand-built; a future metadata editor would be
  the first real reason to revisit this.
- egui's ready-made widgets (sliders with typed input, combo boxes,
  tables) remain unavailable.

## Verification

This is a decision not to build; it is verified by the measurements
above (line counts by function, `bevy_egui` 0.42 manifest and pass
ordering read from its published source on 2026-10-05). If option B is
ever taken, its first step is the measurement in point 5: an egui panel
over the HDR 2D camera, screenshotted and luma-checked with bloom on.
