# ADR-0016 — Diagrams drawn in `ui_kit`'s own hand, not by a plotting library

**Status: Accepted** (2026-09-15, the user's commission: players who can
be created, with statistics that show development over time and standing
against other players, "sehr detailliert" and using charts)

## Context

Statistics need pictures. A table of nine numbers answers "what is my
accuracy" and cannot answer "am I getting better", which is the whole
question the commission asks.

BeatByte had no diagram of any kind. The closest thing was
`song_select::spawn_chart_mark` — four bars of rising height, built
from bare `Node`s, standing for how often a song had been redesigned.
Everything else visual in this game is generated: `shapes.rs` bakes
every texture from a pure shading function, the stage is procedural
geometry, and `ui_kit` owns every font size, panel frame and selection
cue on every screen, with a test that forbids a tenth near-duplicate
size from appearing.

The word "chart" was already taken. In this repository a chart is a
note chart — the thing the generator writes and the editor edits. A
module called `chart.rs` that drew line graphs would cost somebody an
hour, once, and then keep costing it.

## Decision

**No plotting library. A new `beatbyte-game/src/plot.rs`** with two
halves: pure geometry (`Bounds`, `fraction`, `ticks`, `polyline`,
the axis label formatters) that is unit-tested like every other pure
function in this workspace, and spawners that turn it into `Node`s in
the house palette — line plots with grid and rule, horizontal bars,
stacked shares, diverging comparisons.

Lines are real lines: Bevy 0.19 has `UiTransform { rotation: Rot2 }`,
and `bevy_ui::layout` applies it about the node's own centre, so a
segment placed by its midpoint and rotated by `atan2(dy, dx)` joins
its endpoints exactly. The module is named `plot`, never `chart`.

## Alternatives considered

Every candidate was looked up on the crates.io API on 2026-09-15
rather than recalled.

| Crate | Latest stable | Licence | Why not |
|---|---|---|---|
| `bevy_egui` 0.42 + `egui_plot` 0.37 | 2026-08-16 / 2026-08-05 | MIT, MIT-or-Apache | **The only one that actually fits** — `bevy_egui` 0.42 requires `bevy_app`/`bevy_ecs` `^0.19` and `egui ^0.36`, which is exactly what `egui_plot` 0.37 wants. Rejected on design, not compatibility: it is a second complete UI toolkit with its own font stack, its own widget look and an immediate-mode, mouse-first input model. This game is navigated with `MenuNav` on a keyboard and a gamepad, and `ui_kit` exists precisely to stop a screen inventing its own frame and its own type scale. An egui panel would not be a screen of this game that happens to contain a plot; it would be a different program in a window. |
| `plotters` 0.3.7 | 2024-09-08 | MIT | Renders into a pixel buffer, knows nothing of Bevy, and draws text through `ab_glyph`/`font-kit` — so the labels would not be the game's face. Its output is matplotlib's idiom on a dark 8-bit stage. Untouched for two years. |
| `charming` 0.6 | 2025-06-17 | MIT-or-Apache | ECharts bindings; needs a browser or WebView to render. There is none. |
| `poloto` 19.1.2 | 2023-07-09 | MIT | Emits SVG. Bevy has no SVG renderer, and adding one to draw four diagrams is the tail wagging the dog. Unmaintained since 2023. |

## Consequences

**Good.** No new dependency in a workspace whose release binary is
already 111 MB. The plots wear the same palette, the same type scale
and the same panel frame as every other screen, because they are made
of the same nodes. The arithmetic worth being wrong about — where a
value sits on an axis, where a tick lands, which way a segment leans —
is pure and pinned, and two of those pins caught real errors while
being written. A diagram costs no frame time it does not deserve:
these screens spawn once and are static until the view changes.

**Costs.** Anything the module does not do yet has to be written:
there are no scatter plots, no logarithmic axes, no zoom, no
tooltips, and adding one is work rather than a call. A line is N
rotated nodes, so a series of thousands of points would be wasteful —
the markers already stop being drawn past 60 points, and a plot of a
whole year of play would want decimation. Neither limit is close:
the busiest real series here is 19 points.

## Verification

Pure geometry: 7 tests in `plot.rs`, including the two that caught
mistakes — a flat series must still have a range to draw in (equal
bounds divide by zero and collapse every point onto one line) and a
rising line must lean upward in a coordinate system where y grows
downward and `Rot2` turns clockwise. Rendering: the four statistics
views photographed with `BEATBYTE_SHOT_STATE=stats` against a real
259-line play log, and read back.
