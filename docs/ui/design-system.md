# The BeatByte UI: menus and settings

Every menu in BeatByte is built from one kit, `beatbyte-game/src/ui_kit.rs`.
This page says what the kit guarantees, why each rule exists, and how to
add a screen without breaking the set.

If you only read one line: **a screen may not invent a font size, a
frame, a spacing value or a selection cue of its own.**

## Why the kit exists

Before it, the screens shared a font and nothing else. Measured across
the eight menu surfaces:

| Symptom | Measured |
|---|---|
| Title-to-content gaps | 26 / 20 / 18 / 18 / 14 px, and twice none at all |
| Font sizes for rows that do the same job | 11, 12, 13, 14, 18 px |
| Sizes for incidental text | 9 px *and* 10 px |
| Separator styles in footer hints | four (`"   "`, `"    "`, `"   \|   "`, `"  "`) |
| Screens with a panel frame | four of eight |
| Selection cue | the colour of the row's letters, and nothing else |

Two of those were not cosmetic. The settings label column was padded to
16 characters while `TAP MODE (NO STRUM)` is 19, so that one row's value
hung outside the column. And the controls screen read `KeyCode::ArrowUp`
directly instead of going through `MenuNav`, so a player holding a
guitar could not reach the screen that rebinds the guitar.

## The look

**An arcade cabinet, not a settings app.** The pixel face (Press Start
2P) and the navy/brand-yellow palette were already the game's voice, so
the kit adds *structure* — a frame, an accent bar, one spacing rhythm —
rather than a new style. A modern flat look would have made the pixel
face a foreign object on its own screen. See
[ADR-0010](../decisions/ADR-0010-ui-design-system.md).

## Type scale

Three sizes, plus one for the wordmark. A test
(`the_type_scale_has_no_near_duplicates`) rejects any pair closer than a
factor of 1.2, which is what caught 9 px against 10 px.

| Token | px | Used for |
|---|---|---|
| `WORDMARK` | 52 | The game's name on the main menu, the pause banner |
| `TITLE` | 28 | Screen headings, and big transient readouts (the tester's HIT flash) |
| `ROW` | 13 | Every selectable row: menu entries, settings, bindings, songs |
| `SMALL` | 10 | Subtitles, footer hints, incidental notes |

Press Start 2P runs wide, so these are roughly half what a proportional
face would use at the same apparent size.

**Two faces, one scale.** The 8-bit style sets everything in Press
Start 2P. The round style sets its display text — headings, rows,
HUD readouts — in **Bebas Neue** (OFL, bundled): bold, condensed,
all-caps, chosen for measured tabular digits so the score counter
never jitters. Because its capitals reach 70 % of the em where Press
Start 2P's fill it, `UiFont::text` sets the display face at
`DISPLAY_SCALE` (1.3×) the nominal size; the tokens above stay the
single scale, and no screen picks a size of its own. Two jobs keep the
engine's monospace face in the round style, through
`UiFont::mono_text`: the karaoke line (laid out glyph by glyph on a
fixed advance) and data text such as the watch-folder path (all-caps
would misrepresent it).

## Spacing

A 4 px grid.

| Token | px | Between |
|---|---|---|
| `HEADER_GAP` | 24 | The header block and the panel |
| `ROW_GAP` | 4 | Rows inside a panel |
| `FOOTER_GAP` | 20 | The panel and the footer hint |
| `PANEL_WIDTH` | 620 | — |

`PANEL_WIDTH` is not a taste value: a test computes the widest row the
settings screen can produce (`TAP MODE (NO STRUM)` beside
`8-BIT SHAPES`) at `ROW` px and asserts it fits inside the panel with
its padding, the row padding, the accent bar and the column gap.

## Row states

| State | Accent bar | Fill | Label | Value |
|---|---|---|---|---|
| `Idle` | none | none | dim | dimmer |
| `Selected` | brand | brand tint | brand | bright |
| `Armed` | hype violet | violet tint | violet | bright |

`Armed` is the controls screen waiting for a key or button. It has its
own colour because a modal "press something now" state must not look
like an ordinary highlight.

**Hover is deliberately absent.** On every screen hovering *moves the
cursor*, so a hovered row is a selected row; a separate hover state
would contradict the cursor. There is no `Disabled` state because no
screen has one — the kit does not model states the game does not have.

A test asserts that `Selected` differs from `Idle` in the bar, the fill
*and* the label. Colour alone is a weak cue on a dark background, and
signalling selection only by letter colour is exactly what the screens
used to do.

### The fill weight is measured, not chosen

`FILL_ALPHA` is 0.055. The first attempt was 0.12, which *looks* subtle
written down and rendered as sRGB (99, 84, 35) — a solid olive bar that
shouted over the accent it was meant to support.

**Bevy blends `BackgroundColor` alpha in linear space**, so sRGB
intuition badly underestimates it. Sample the rendered pixel and solve
for the target rather than reasoning about the constant. The current
value measures (69, 58, 32).

## Pointer behaviour

One rule, in `read_rows`: **hovering selects, clicking activates.**

A press implies a hover, so a click always activates the row under the
pointer and never whatever was selected before it. This is centralised
because the song browser did not follow it — it handled only
`Interaction::Pressed`, ignored hover entirely, and needed two clicks
(one to select a row, another to start it). Two lists that look
identical must not behave differently.

## Footer wording

`KEY action` pairs, two spaces between pairs, verbs lower-case:

```text
UP/DOWN choose  LEFT/RIGHT adjust  ESC back
```

Footers state what the keys *do*, which means keeping them honest. The
settings footer used to read `ENTER confirm`; ENTER steps the value,
exactly like RIGHT, so it now says neither.

## Adding a screen

```rust
commands
    .spawn((MyScreen, ui_kit::screen_root()))
    .with_children(|parent| {
        ui_kit::header(parent, &font, "MY SCREEN", "what it is for");
        parent.spawn(ui_kit::panel()).with_children(|panel| {
            for (index, item) in items.iter().enumerate() {
                panel
                    .spawn((MyRow(index), Button, ui_kit::row()))
                    .with_children(|row| {
                        row.spawn((
                            MyLabel(index),
                            Text::new(item.label()),
                            font.text(ui_kit::ROW),
                            TextColor(palette::TEXT_DIM),
                            ui_kit::label_node(),
                        ));
                        row.spawn((
                            MyValue(index),
                            Text::new(""),
                            font.text(ui_kit::ROW),
                            TextColor(palette::TEXT_DIM),
                            ui_kit::value_node(),
                        ));
                    });
            }
        });
        ui_kit::footer(parent, &font, "UP/DOWN choose  ENTER confirm  ESC back");
    });
```

Notes on the parts that are easy to get wrong:

- **`Button` is added by the caller, not by `row()`.** A row that only
  reports status — the multiplayer slots — must not carry a button,
  because that promises an interaction it does not have.
- **The label never shrinks** (`label_node`); the value is bounded and
  wraps right-aligned (`value_node`). Unbounded, a long value collides
  with its label: `Enter / PAD Select / PAD RightTrigger` ran straight
  into the word HYPE.
- **A screen whose content is an instrument rather than a list** — the
  calibration dot, the input tester's lamps — uses `panel_centered()`,
  which is the same frame with more air.

Paint the rows from one place:

```rust
let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
if let Some(index) = pointer.hovered {
    cursor.0 = index;
}
let style = ui_kit::row_style(ui_kit::state_for(index == cursor.0, false));
```

## Verifying a change

The autopilot only ever reaches the menu, the song browser and the
results screen, which used to leave the settings, controls, calibration
and input-test screens as the ones least likely to be checked. Boot
straight into any of them:

```bash
BEATBYTE_SHOT_STATE=settings BEATBYTE_SHOT_DIR=/tmp/shots \
  BEATBYTE_WINDOW=1280x800 cargo run --release -p beatbyte
```

That opens the screen, photographs it once the transition fade has
finished, and quits. Take screenshots in a *separate* run from any
pass/fail verdict: capturing a frame stalls it long enough for the
autopilot's key injector to miss a note.

## Related

- [ADR-0010 — UI design system](../decisions/ADR-0010-ui-design-system.md)
- [ADR-0008 — theme system](../decisions/ADR-0008-theme-system.md) (stage
  themes; the menus deliberately do not follow them)
- [Architecture overview](../architecture/overview.md)
