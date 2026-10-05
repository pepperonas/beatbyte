# ADR-0023 — Menus as data: a typed row table and one list renderer

**Status: Accepted** (2026-10-05, the user's brief "Datengetriebenes
Menüsystem für BeatByte", slimmed after the phase-0 analysis and
agreed with "ja"; scope of phase 4 by the user: "alle Menüs". Plan:
`docs/plans/menu-system.md`; reference: `docs/ui/menu-system.md`.)

## Context

A new setting meant touching seven places in `settings_ui.rs`: the row
enum, its label, its value, its step, its sound, its subtitle and the
row list — plus the pause menu, which reused the rows through its own
match arms. Every other screen with a row cursor (main menu, about,
controls, players, achievements) had its own copy of the same input
code: keys and pad through `MenuNav`, the wheel, hover, click, the
cursor stopping at the ends, painting the selected row, scrolling it
into view, and a despawn system. The copies had drifted: the pause menu
stepped differently from settings, controls followed a pointer that had
not moved, only achievements could hold a row for the screenshot
harness.

The user's brief asked for menus in RON files with hot reload and every
screen wrapped in one framework.

## Decision

1. **Rows are a typed Rust table, not files** (`menu_list::spec`).
   `RowSpec<S, X>` = label + `Kind` (Toggle, Slider, Choice, Door,
   Custom) + subtitle; accessors are function pointers into the state,
   so a type mismatch does not compile. Everything a screen does with a
   row is a pure, tested function on the spec. The forty settings rows
   are constants written with three small macros.
2. **One renderer** (`menu_list::list`), generic over a marker type per
   list: spawning, `ListInput` (one input path for keyboard, pad,
   guitar, wheel and pointer), `ListPaint` (row states from `ui_kit`),
   `follow_cursor`, `HeldRow`. Screens despawn through
   `DespawnOnExit`.
3. **Every screen with a row cursor uses the renderer**: settings,
   pause, main menu, about, controls, players, achievements. Rows with
   their own contents (the roster, the achievements) keep them inside
   `row_frame`.
4. **Special screens are left alone**: the song browser, statistics,
   calibration, the input test, results, join, song info, the editor.
5. **Behaviour is unchanged unless written down**: each migration is
   proven with `tools/shot-check.sh` at 0 changed pixels, and the
   deliberate changes are listed in the CHANGELOG (pause steps like
   settings; controls follow only a moving pointer; two rows changed
   their sound).

## Alternatives considered

- **RON files with hot reload** (the brief). Rejected: a row's
  behaviour is code (what a toggle writes, what a door opens), so a
  file can only name it, and a typo becomes a runtime error instead of
  a compile error. Hot reload serves designers editing layouts; here a
  menu changes a few times a month, through a compile anyway.
- **Wrapping every screen** in one framework. Rejected after phase 0:
  the browser, statistics and the editor are not lists, and a wrapper
  would add code there without removing any.
- **Cursors that wrap around**, as the brief proposed. Rejected: every
  list in the game stops at its ends (an earlier, deliberate decision
  pinned by tests), so a held key or strum lands on the last row
  instead of jumping back to the first.
- **Leave the copies as they were.** Rejected: they had already drifted
  in four ways (above), and each new screen added another copy.

## Consequences

Good:
- A new toggle is one `Settings` field, one sync classification and one
  table entry — measured: the demo toggle was 11 lines in 3 files, and
  the existing pin "every row edits exactly one field of its own"
  covered it unasked.
- Every list behaves the same on every device; `BEATBYTE_SHOT_ROW`
  works on all of them.
- Screenshot comparison is exact (`tools/shot-check.sh`, tolerance 0),
  because the noise it first showed turned out to be three defects.

Costs:
- Net code is about even: the screens lost 510 code lines (non-blank,
  non-comment, without tests), the model and renderer added 471, the
  autopilot 13 — 7 526 → 7 500 over the files involved. The gain is
  one implementation instead of six, not fewer lines.
- A shared `SystemParam` that writes common components (`Text`,
  `Node`) makes query conflicts easy to introduce and invisible to
  wired tests; `access_tests` in `lib.rs` initializes every schedule of
  the list screens to catch them (one such conflict stopped the game at
  launch during phase 4).
- `Row` constants cannot be `match`ed on (they hold function pointers);
  dispatch goes through `spec().action()`.

## Verification

Per phase, in `docs/ROADMAP.md` ("Menus as data"): the settings rows
0/13/39, the main menu, about, controls, players and achievements
pixel-identical before and after; the pause drill and the model drill
passing through the new code with real keys; the new pins (one field
per row, sounds, exact UI scale, the shot waiting for the tube, the
capture-escape, the schedule access test) each seen to fail under a
mutation.
