# Plan: declarative settings rows and one list renderer

Status: **phases 1–3 done (v0.18.67–0.18.69)**; stop and report before phase 4 (2026-10-05).

## The brief (rewritten, slim form)

The user's original brief asked for a data-driven menu system: RON
files under `assets/ui/screens/`, hot reload, a generic renderer for
every list screen, special screens wrapped as custom widgets, a
pixel-diff harness, and wrap-around navigation. The analysis below
showed that most of the plumbing already exists in `ui_kit` and
`MenuNav`, that RON would replace compile-time checks with runtime
ones, and that the code to save sits almost entirely in the settings
screen. The agreed, slimmer brief:

**Goal.** A new setting is ONE table entry (plus its field in
`Settings`), checked by the compiler and by tests — instead of seven
match arms spread over `settings_ui.rs`.

**Keep unchanged.** The look (tokens, themes, type scale, row states,
animations), input on keyboard / gamepad / guitar / mouse, the
alphabetical order of the settings rows, **cursors that stop at the
ends** (pinned rule — no wrap-around), gameplay, judgment, the audio
clock.

**Phases.**

1. **Model (engine-free).** A declarative row table in Rust: label,
   kind, binding, sound, subtitle. Kinds: `Toggle`, `Slider { min,
   max, step, format }`, `Cycle` (enum values), `Door` (opens a
   screen), `Info` (read-only), `Custom` (a named row that owns its
   own value and Enter — library move, lyrics model, export, watch
   folder). Bindings are typed accessors on `Settings`, never strings.
   Pure functions for stepping a value; unit tests for every kind,
   for the clamp of every slider, for the alphabetical order, and that
   every row renders a value.
2. **Renderer.** One generic list builder on top of `ui_kit` (it
   already owns tokens, row styles, `step_cursor`, scrolling, hover
   and back) driven by the table: spawn, refresh, input, sounds — in
   one place. Leaving a screen despawns through Bevy's state-scoped
   entities instead of a per-screen despawn system, where the screen
   moves to the renderer.
3. **Reference migration: the settings screen** (there is no separate
   audio-settings screen; it is one screen of 40 rows). Engine-side
   screenshots before and after via `BEATBYTE_SHOT_STATE=settings`
   with `BEATBYTE_WINDOW` pinned, compared by eye and by a pixel diff
   with a small tolerance. The pause menu, which already reuses the
   settings rows, follows in the same step. **Stop and report.**
4. **Further list screens, only where it pays:** the main menu, then
   possibly the about screen's info rows and the results actions —
   one screen per commit, old code deleted in the same commit,
   screenshot comparison each. Special screens (song browser,
   statistics, achievements, players, controls, calibration, input
   test, editor) are NOT wrapped: they already use `ui_kit` and
   `MenuNav`, and a wrapper would add code, not remove it.
5. **Editor (analysis only):** an ADR draft on whether `bevy_egui`
   would make the chart editor easier to maintain, with an effort
   estimate. No implementation without an OK.
6. **Docs:** `docs/ui/menu-system.md` (table format, kinds,
   examples), CLAUDE.md "Add a setting" / "Add a list screen" step by
   step with the duty to run the screenshot harness and look at the
   image, an ADR, the ADR index, README test counts, CHANGELOG,
   version per the project rule. Proof in the commit: a demo toggle
   added with one table entry plus one `Settings` field, then removed.

**Acceptance.** Full gate green; settings and pause on the new table;
screenshots identical or the difference explained; autopilot and menu
navigation unchanged on all devices; net fewer lines in
`settings_ui.rs` + `gameplay/mod.rs` pause code, numbers in the
commit.

## Phase 0: what is there today

### Screens

| Screen (`AppState`) | File | Lines | Kind |
|---|---|---|---|
| MainMenu | `menu.rs` | 310 | generic list (8 actions, enum + `ALL` + `label()`) |
| Settings | `settings_ui.rs` | 1119 | generic list (40 rows) with four custom rows |
| Pause (`GamePhase::Paused`) | `gameplay/mod.rs` | part of 2034 | generic list: practice rows + a subset of the settings rows (`PauseItem::Setting(Row)`) |
| Results | `results.rs` | 1496 | mostly a report; a short action list |
| About | `about.rs` | 911 | info rows (`InfoRow::ALL`, 9) plus logo/credits |
| MultiplayerSetup | `multiplayer.rs` | 329 | status slots, not a list of choices |
| SongInfo | `song_info.rs` | 396 | report of a song document |
| Players | `players_ui.rs` | 957 | special: roster + name field |
| Stats | `stats_ui.rs` | 2404 | special: tabs, filters, charts |
| Achievements | `achievements_ui.rs` | 1581 | special: tabs, sort, secrets |
| SongSelect | `song_select.rs` | 4726 | special: browser tree, search, preview |
| Controls | `controls_ui.rs` | 562 | special: binding capture |
| InputTest | `input_test.rs` | 344 | special: lamps |
| Calibration | `calibration.rs` | 429 | special: tap test |
| Editor | `editor_ui.rs` + `editor_ui/` | 1660 + | special: timeline editor |

Seven screens are lists of some kind; only settings (with pause)
and the main menu are pure "row of label + value" lists.

### How it works today

- **Look:** `ui_kit.rs` (1662 lines) owns the type scale, spacing,
  panels, row states (`row_style`, `state_for`), header/footer, the
  back button, and `docs/ui/design-system.md` documents it. A test
  forbids near-duplicate font sizes.
- **Focus and navigation:** `controls::MenuNav::read` maps keyboard,
  gamepad and guitar through the player's bindings onto up / down /
  left / right / confirm / back (Enter and Esc hard-wired, Tab cycles).
  `ui_kit::step_cursor` stops at both ends (pinned). Mouse:
  `ui_kit::read_rows` + `hover_moves_cursor`; the wheel scrolls, it
  does not step values (user report 2026-09-01).
- **Scrolling:** `ui_kit::scroll_to_show`, `whole_rows_height`,
  `follow_list`, `list_view`, `scroll_panel`.
- **Sounds:** `sfx::UiSound` messages written by each screen
  (Navigate / Confirm / Back / Error); settings picks a sound per row
  via `Row::sound()`.
- **State changes and cleanup:** each screen registers its own
  `OnEnter` spawn and `OnExit` despawn system (14 despawn systems).

### Where the code repeats

Adding one toggle to settings today touches **seven places** in
`settings_ui.rs`: the enum variant, `ALL` (and its length), `label()`,
`value()`, `adjust()`, `sound()`, the toggle test's list — plus
`subtitle()` when it needs one. The 40 rows are:

- 17 toggles (`settings.x = !settings.x`, `on_off(settings.x)`; AI
  SEARCH words its value differently),
- 11 sliders (clamped step with a unit: %, ms, px/s, s),
- 5 cycles (telemetry, vocal pitch, flash sync, lyrics size, theme),
- 3 doors (controls, calibration, input test),
- 4 custom rows (watch folder, library, export history, lyrics model).

36 of the 40 are table material; the four custom rows keep their own
code. `settings_input` repeats the cursor/sound/mouse handling that
`menu.rs`, `about.rs` and the pause menu each write again, with small
differences.

## Proposal

**Format: a Rust table, not RON.** `Row` stays an enum (so the pause
menu, tests and the autopilot keep naming rows by type), and one
`const SPEC` table describes each row:

```rust
RowSpec {
    row: Row::HitLabels,
    label: "HIT LABELS",
    kind: Kind::Toggle(bind!(hit_labels)),
    subtitle: None,
}
RowSpec {
    row: Row::ScrollSpeed,
    label: "SCROLL SPEED",
    kind: Kind::Slider { bind: bind!(scroll_speed), min: 240.0, max: 900.0, step: 30.0, unit: Unit::PxPerS },
    subtitle: None,
}
```

`bind!` expands to a pair of plain functions (`get`, `set`) on
`Settings` — typed, so a toggle on a float does not compile. The
`label()`, `value()`, `adjust()` and `sound()` matches become lookups
in the table; `ALL` is derived from it and the alphabetical test keeps
its job. Why not RON: the enum and the compiler already catch a wrong
key; RON would move that to a test and to runtime, and add an asset
that, if it ever fails to load, never retries (the v0.8.1 gotcha) —
an invisible settings screen. Hot reload is the only thing RON adds,
and a settings list changes rarely.

**Modules.**

- `crates/beatbyte-game/src/menu_list/spec.rs` — `RowSpec`, `Kind`,
  `Unit`, the value formatting and stepping as pure functions (tested
  without Bevy, though the module lives in the game crate because
  `Settings` does).
- `crates/beatbyte-game/src/menu_list/render.rs` — spawn / refresh /
  input / sounds for a list of specs, on top of `ui_kit`, with a hook
  for custom rows.
- `settings_ui.rs` shrinks to the table, the four custom rows and the
  screen's frame; `gameplay/mod.rs` keeps its practice rows and uses
  the same table for its settings subset.

**Risks.** The settings screen is reached by the autopilot only
through `BEATBYTE_SHOT_STATE`; the pause drill covers pause. Screens
photographed on this Mac go black when the window is occluded, on the
second display or the screen is locked — the comparison uses the
engine's own screenshot and checks luma first.

## Phase 1 result (v0.18.67)

- `menu_list/spec.rs`: `RowSpec<S, X>` with `Kind` (Toggle, Slider,
  Choice, Door, Custom), `Unit`, `Ends`, `Feel`, `Subtitle`, and the
  pure `value` / `step` / `feel` / `action` functions — 6 tests on a
  toy state.
- `settings_ui.rs`: the forty rows as `Row::NAME` constants built with
  `row!` / `toggle!` / `slider!`; `Row` is a handle on its spec, equal
  by label. `label`, `value`, `subtitle`, `adjust`, `opens`, `index`
  and `ALL` keep their signatures, so the screen and the pause menu
  were not touched beyond the rename.
- New pins: every row edits exactly one field and no two rows the
  same one (catches a copy-pasted binding that would still compile),
  a switch clicks and a dial ticks, telemetry steps both ways. Three
  mutation probes, all caught.
- Screenshots of rows 0, 13 and 39 before and after: identical apart
  from the CRT sweep line and the mute icon, which also differ between
  two runs of the same build (measured: 0.37–0.50 % of the pixels).
- Lines: `settings_ui.rs` 865 → 875 code lines (+ new tests), plus
  251 lines of generic model. The saving comes with phase 2, when the
  list screens share one renderer.

## Phase 2 result (v0.18.68)

`menu_list::list` (spawn, input, paint, follow, `step_of`, `sound_for`);
settings and pause on it, both with `DespawnOnExit`. Pause behaves like
settings now (click halves, a dial tick, one step for LEFT+RIGHT). The
model drill, which had never run, runs. Lines: `settings_ui.rs`
865 → 698 code lines, pause −89, renderer +309.

## Phase 3 result (v0.18.69)

`tools/shot-check.sh` + `beatbyte-cli shots compare`, exact by default.
The "noise" was three bugs (CRT power-on in the shot, a UI scale off its
target by up to 1 %, the pointer moving the photographed row); with them
fixed, two runs give identical files. Counter-checked on a one-letter
change.
