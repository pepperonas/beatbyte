# Plan: statistics that render well, and the next round for the menus

Status: **proposed** (2026-10-05, user: "recherchiere wie man in der app
am besten die statistiken rendern kann. und prüfe auch ob man das menü
noch weiter verbessern kann. erstelle plan."). Nothing built yet.

## What the statistics screen looks like today

Photographed with the real play log (copied into a scratch home, so the
run could not touch the player's files), all six views at 1280×800:

| View | What is wrong with the picture |
|---|---|
| all | **The header jumps** — MARTIN sits at y 85, 190, 225, 210 and 360 px in five views, because the screen is centred on its content height. Switching tabs moves the tabs themselves. |
| all | **The panel runs edge to edge** (the full window), unlike every other screen (`ui_kit::PANEL_WIDTH` / `PANEL_WIDE`), while the plots inside it stop at about two thirds; the right third is empty. |
| overview | The accuracy line jumps between 0 % and 80 % from run to run — no trend is readable, which is the one question the view asks ("am I getting better?"). No dates on the x axis, no 100 % label. |
| overview, technique | **Numbers that look wrong:** "misses cluster where the song is heard once (100 % miss)", and all three musical contexts at exactly 100 % miss; finished runs plotted at 0 % accuracy. Either the data or the query is off — this has to be checked before anything is made prettier. |
| timing | The histogram is 500 px wide in a 2000 px panel; the drift line is a zig-zag of 20 points with no smoothing and no ± window for reference. |
| technique, difficulty, songs | Bars use 40 % of the width; the value labels float in the middle. |
| versus | One sentence in a full-width panel ("no song both have finished…") and nothing that says what would fill it. |

## Part A — how to render the diagrams

ADR-0016 (2026-09-15) decided: no plotting library, diagrams built from
UI nodes (`plot.rs`), a line = N rotated nodes. That decision still
holds against the libraries. What has changed is that Bevy 0.19 itself
offers two things `plot.rs` does not use yet.

### Options, checked on 2026-10-05

| Option | Fits Bevy 0.19 | Stays in `ui_kit` layout and fonts | Cost / risk | Verdict |
|---|---|---|---|---|
| **A. Nodes as today** (`plot.rs`) | yes | yes | lines are rotated rectangles: no anti-aliasing, no area fill, one entity per segment; markers stop at 60 points | keep for bars, labels, axes |
| **B. `UiMaterial` plot shader** (`bevy_ui_render` 0.19: `MaterialNode<M>` + WGSL fragment shader) | yes, in-tree | **yes** — it is a UI node; labels stay text nodes | one WGSL file + one material type; anti-aliased lines by distance field, area fills, bands and thousands of points in ONE node; already used pattern in the engine, no new dependency | **recommended** for lines, areas, histograms, heat maps |
| **C. `BackgroundGradient`** (`bevy_ui` 0.19: linear, radial, **conic**) | yes, in-tree (the CRT uses it) | yes | free; a conic gradient is a donut/pie, a linear one an area fade or a heat strip | use where it suffices (shares, fades) |
| D. `bevy_vello` 0.14 (`UiVelloScene`) | yes (`bevy ^0.19`) | node yes, text no (own font stack) | vello 0.9 is a compute renderer drawing into an `Rgba8Unorm` texture: new render path next to two HDR cameras with bloom (the contract this project has lost frames to), compute shaders on the 2015 MacBook unproven, large dependency | no |
| E. `bevy_egui` 0.42 + `egui_plot` 0.37 | yes | no | second UI toolkit (ADR-0016, ADR-0024) | no |
| F. `bevy_prototype_lyon` 0.17, `bevy_vector_shapes` 0.13, `bevy_svg` 0.19 | yes | no — world-space meshes, would need a third camera rendering into a texture | camera contract again | no |
| G. `plotters` into an image | n/a | no (own text, matplotlib look) | as in ADR-0016 | no |

**Recommendation: B + C, A for the rest.** A `PlotMaterial`
(`menu`-independent, in `plot.rs` or `plot/shader.rs`) takes up to a
fixed number of points (e.g. 512, decimated by a pure, tested function)
plus colours and style flags, and its fragment shader draws: the line
(anti-aliased, `smoothstep` on the distance to the nearest segment), an
area fill with a vertical fade, an optional band (min–max or ± window)
and a zero rule. Axes, ticks and labels stay text nodes, so the type
scale and the font stay `ui_kit`'s. ADR-0016 gets an amendment, not a
replacement.

Risks to measure first (step S2): the shader must look the same under
the HDR 2D camera with bloom (luma-check a screenshot), and a uniform
array of 512 points must fit the 2015's limits (WebGPU minimum for a
uniform buffer is 64 KiB; 512 × vec2 f32 = 4 KiB).

### Steps

- **S0 Data truth first.** Reproduce the three suspicious numbers
  (100 % miss in every context, finished runs at 0 %, 138 runs without
  a player) against `telemetry.db` with `beatbyte-cli telemetry`, and
  fix what is wrong in the query or the data. A beautiful chart of a
  wrong number is worse than today's.
- **S1 Layout.** Header anchored at the top (no centring on content
  height), panel width from `ui_kit` like every screen, plots using the
  panel's full width, two columns where a view has two diagrams.
  shot-check on all six views.
- **S2 Plot shader spike.** `PlotMaterial` with one line + area on the
  overview; measure: HDR/bloom look (luma), frame time, the 2015 Mac.
  Stop if it fails; A stays.
- **S3 Diagrams that answer their question.**
  - overview: accuracy per difficulty as a rolling mean (e.g. last 10
    finished runs) with the raw runs as faint dots; x axis in dates.
  - timing: histogram full width with the PERFECT window shaded; drift
    over time with the ± window as a band.
  - technique: miss rate per fret as a five-cell heat strip in the fret
    colours; note kinds as bars with the value at the bar's end.
  - songs: a sparkline per personal best (how it got there).
  - versus: when empty, say what would fill it (one song, same
    difficulty, both finished) and offer the nearest candidate.
- **S4 Reading a value without a mouse.** A cursor on the plot
  (left/right through runs, a readout line with date, song, value) —
  hover tooltips do not exist on a gamepad.
- **S5 Docs and verification.** ADR-0016 amendment, `plot.rs` docs,
  pins for the decimation and the rolling mean, shot-check per view,
  frame time unchanged within noise.

## Part B — the menus, next round

The menu programme (ADR-0023) unified how lists work. Looking at them
as a player, these remain:

| # | Finding | Proposal | Effort |
|---|---|---|---|
| M1 | **Settings: 40 rows in one alphabetical list**; finding "LYRICS OFFSET" means scrolling past AI SEARCH … LIBRARY. | Group headers (Sound, Display, Gameplay, Lyrics, Library, Input, Data) with alphabetical order **inside** each group — or keep one list and add type-to-jump (a letter moves to the first row starting with it). The alphabetical rule was a deliberate decision, so this is the user's call. | M |
| M2 | **No Page Up/Down, Home/End** in any list (`ListInput` reads none). | Add them to `ListInput`: one change, every list gets it. | S |
| M3 | Nothing shows **which settings differ from the default**, and there is no way back to a default but remembering it. | A dot on changed rows; Backspace resets the selected row (the default comes from `Settings::default()`, so it cannot drift). | S |
| M4 | Sliders show a number only. | A thin fill bar under the value (`ui_kit`), so 80 % reads at a glance. | S |
| M5 | The main menu explains nothing; Settings has a subtitle line, the main menu does not. | One line per item ("Pick a song and play", "Who is at the guitar") using the existing `Subtitle`. | S |
| M6 | The statistics screen is the one screen whose header moves (see S1). | covered by S1 | — |
| M7 | The renderer still repeats a pattern per screen: paint system + follow system + input system with the same wiring. | A `ListScreen` helper plugin (`ListPlugin::<L>::new(state)`) that registers paint/follow; screens keep their input. Only if a new list screen comes — not worth it for its own sake. | M, later |

Suggested order: S0 → S1 → M2 → M3 → M5 → M4 → S2 → S3 → S4 → M1 (after
the user's choice) → S5. S0 and S1 fix things that are wrong now; M2–M5
are small and independent; S2 is the one with a measured risk and a
stop point.

## Found while researching: the vanishing `bindgen.rs` (solved)

The file that kept disappearing from `target/*/build/libsqlite3-sys-*/out/`
is deleted by **`~/bin/platzwaechter.sh`**, started hourly by
`~/Library/LaunchAgents/com.celox.platzwaechter.plist`: below 40 GB free
it runs `find <repo>/target -type f -mtime +3 -delete`. libsqlite3-sys
copies its bundled bindings with the **original file date — 24 July
2006** — so the fresh copy is "older than 3 days" the moment it exists.
Caught in the act (process snapshot at 15:38:30: `find …/beat-byte/target
-type f -mtime +3 -delete` under `platzwaechter.sh`), and both of today's
vanishings (14:38, 15:38) match the script's log.

Fix (outside this repository, needs the user's OK): use `-ctime` instead
of `-mtime` (the change time is set by the copy and cannot be carried
over), or skip `*/build/*/out/*`. Better still, the house rule from
CLAUDE.md: prune `incremental/` (`tools/prune-incremental.py`) or whole
profiles, never files by age.

## Decisions for the user

1. Part A: go with B + C (shader for lines/areas, gradients, nodes for
   the rest), amending ADR-0016?
2. M1: group headers in Settings, or keep the one alphabetical list and
   add type-to-jump?
3. The platzwaechter fix: `-ctime`, or switch it to pruning
   `incremental/` and whole profiles?
