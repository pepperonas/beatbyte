# ADR-0010: One UI Kit for Every Menu

**Status:** Accepted (2026-08-26)
**Context:** Milestone G22 — menu and settings unification

## Context

BeatByte had eight menu surfaces — main menu, settings, controls, song
browser, multiplayer join, calibration, input tester, pause overlay —
each of which built its own layout. `ui.rs`, the only shared UI module,
was 63 lines and supplied nothing but the font handle.

The result was measurable drift: five different title-to-content gaps,
five font sizes for rows that do the same job, two sizes for incidental
text that differ by one pixel, four separator styles in footer hints,
and a panel frame on four screens out of eight. Selection was signalled
by the colour of a row's letters and nothing else.

Two consequences were functional rather than cosmetic:

- The settings screen laid its rows out as a single padded string,
  `{:<16}{:>9}`. `TAP MODE (NO STRUM)` is 19 characters, so that row —
  and only that row — pushed its value out of the column.
- The controls screen read `KeyCode::ArrowUp` directly instead of going
  through `MenuNav`, and carried no `Button` component. It was
  unreachable by gamepad and ignored the mouse, which meant a player
  holding a guitar could not open the screen that rebinds the guitar.

Left alone, every new screen would have added a sixth spacing value.

## Decision

Introduce `beatbyte-game/src/ui_kit.rs`: a module of **tokens and
scaffolding, never behaviour**. It owns the type scale, the spacing
rhythm, the panel frame, the row chrome, the row states and the pointer
rule. Screens keep their own state, input handling and marker
components; they borrow only the shell.

The visual direction is an **arcade cabinet**: the existing pixel face
and navy/brand-yellow palette, given structure through a framed panel,
an accent bar on the selected row and one spacing rhythm.

## Alternatives considered

**A modern flat/neutral UI** (the obvious reading of "make it look
cool"). Rejected: the game's voice is Press Start 2P on deep navy, and a
flat neutral chrome would make the pixel face a foreign object on its
own screen. The inconsistency was never that the style was dated; it was
that there was no style, only eight approximations of one.

**A general-purpose widget library** (buttons, sliders, checkboxes as
components). Rejected as premature: the game has exactly one interactive
pattern — a vertical list of rows with an optional value — and every
screen is a variation on it. Building widgets nobody needs would add
API surface without removing a single inconsistency.

**Adopt an off-the-shelf Bevy UI crate.** Rejected: it would be a
dependency carrying its own aesthetic, which is the opposite of the
problem being solved, and the game's needs are eight screens of one
pattern.

**Fix each screen in place, without a shared module.** Rejected: this
is what produced the drift. Consistency that lives in review notes
rather than in code decays on the next screen.

## Consequences

**Good**

- A new screen inherits the design by construction; there is no correct
  way to build one that looks different.
- Design decisions became testable. Six unit tests assert the contrast
  relationships, the legibility of an idle row, that the widest settings
  row fits the panel, and that no two sizes in the scale are close
  enough to read as a mistake — a test that immediately caught 9 px
  against 10 px and collapsed them into one token.
- Centralising the pointer rule (`read_rows`) fixed a behaviour split
  the styling pass had left in place: the song browser ignored hover and
  needed two clicks where every other screen selected on hover.
- Two hard-coded copies of the lane palette disappeared, restoring
  `palette.rs` as the single source for colour that it documents itself
  to be.

**Costs and limits**

- Screens are now coupled to a shared module: changing a token moves
  every menu. That is the point, but it makes the kit a place where
  careless edits are expensive.
- The kit models only what the game has. There is no `Disabled` row
  state and no hover state distinct from selection, because no screen
  needs either; adding one later means touching the kit rather than a
  screen.
- The gameplay HUD, the results screen and the editor are **not** in
  scope. They are not menus, they have their own spatial logic, and
  forcing them into a row-and-panel kit would be a worse fit than the
  drift it removed.

## Verification

- Gate clean: fmt, clippy `-D warnings`, full test suite, rustdoc with
  no warnings.
- Behaviour unchanged: smoke test plus autopilot on the bundled song
  (98/98 perfect), a real import (624/624), two players, and the editor
  cycle.
- All seven menu screens photographed and inspected via the new
  `BEATBYTE_SHOT_STATE` hook. Three defects came out of the pictures
  rather than the code: colliding bindings on the controls screen, dead
  space inside the tester's panel, and a selection fill far heavier than
  intended.

The last of those produced a reusable finding, recorded in `CLAUDE.md`:
**Bevy blends `BackgroundColor` alpha in linear space.** A fill written
as `BRAND.with_alpha(0.12)` rendered as sRGB (99, 84, 35). The constant
now carries its measured value.

## Related

- [The UI design system](../ui/design-system.md) — the working reference
- [ADR-0008](ADR-0008-theme-system.md) — stage themes, which the menus
  deliberately do not follow
