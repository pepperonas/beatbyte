# The menu system

Every menu with a row cursor — settings, the pause menu, the main menu,
about, controls, players, achievements — runs on one renderer:
`crates/beatbyte-game/src/menu_list/`. The decision and its
alternatives: [ADR-0023](../decisions/ADR-0023-menus-as-data.md). The
look itself (type scale, row states, panels) is `ui_kit`'s and is
described in [design-system.md](design-system.md); this document is
about how a list is *wired*.

Two halves, deliberately separate:

| Module | Knows Bevy | What it is |
|---|---|---|
| `menu_list::spec` | no | a row as **data**: label, kind, subtitle; pure functions for its value, a step, its sound |
| `menu_list::list` | yes | the **renderer**: spawning rows, reading input, painting rows, keeping the cursor in view |

A screen may use the renderer without the row model (the roster and the
achievements draw their own row contents), but not the other way round.

## Rows as data (`spec`)

```rust
pub struct RowSpec<S, X> {
    pub label: &'static str,   // unique in its list
    pub kind: Kind<S, X>,
    pub subtitle: Subtitle,    // None | Text(&str) | Live(fn() -> String)
}
```

`S` is the state the rows edit (`Settings`); `X` names what a row does
that is not editing a value (`Act` on the settings screen: open a
screen, the library move, the model download).

| Kind | Holds | Left / right | Enter | Sound |
|---|---|---|---|---|
| `Toggle { get, set, words }` | a `bool` | flips | flips | click |
| `Slider { get, set, min, max, step, unit }` | an `f32` | ± one step, clamped | + one step | tick |
| `Choice { labels, get, set, ends }` | a position | previous / next; `Ends::Wrap` goes round, `Stop` stops | next | click for two values, tick for more |
| `Door(X)` | nothing | — | the screen's handler for `X` | click |
| `Custom { id, value, adjust, feel }` | its own | `adjust`, if any | the screen's handler for `id` | `feel` |

The accessors are plain function pointers, so a toggle bound to a float
does not compile. `Unit` formats a slider: `Percent`, `SignedMs`,
`PxPerS`, `SecondsOfMs`.

A value row also knows its default: `differs(state, default)` and
`reset(state, default)` — the settings screen marks a changed value with
a dot and resets the selected row on BACKSPACE, against
`Settings::default()`. Doors and custom rows have no default.

Everything a screen does with a row is a pure function on the spec —
`value`, `step`, `feel`, `action`, `is_door`, `subtitle`,
`next_index`, `format_unit` — tested on a toy state in `spec.rs`.

### The settings table

`settings_ui.rs` writes each row as one constant, with three macros for
the common cases:

```rust
pub(crate) const HIT_LABELS: Row = row!("HIT LABELS", toggle!(hit_labels));
pub(crate) const LATENCY_OFFSET: Row = row!(
    "LATENCY OFFSET",
    slider!(latency_offset_ms, -250.0, 250.0, 5.0, SignedMs)
);
pub(crate) const CONTROLS: Row = row!("CONTROLS", Kind::Door(Act::Open(AppState::Controls)));
```

and lists them in `Row::ALL`, **alphabetically by label** (a test keeps
it so). The pause menu reuses a safe subset of the same constants, so a
step size or a clamp is defined once. Matching on `Row` constants in a
`match` pattern is not possible (they hold function pointers) — dispatch
goes through `row.spec().action()`.

## The renderer (`list`)

Every component is generic over a **marker type**, one per list, so two
lists never see each other's rows:

```rust
struct BindingRows;                       // the marker
ListRow::<BindingRows>(index, …)          // a row (Button, RelativeCursorPosition, ui_kit::row())
ListLabel / ListValue::<BindingRows>      // its two texts
ListPanel::<BindingRows>                  // the scrolling panel
```

| Piece | Kind | Does |
|---|---|---|
| `spawn_rows::<L>(panel, font, labels)` | fn | label + value rows |
| `spawn_label_rows::<L>(…)` | fn | label-only rows (the main menu's layout) |
| `row_frame::<L>(index)` | bundle | just the frame, for rows with their own contents |
| `ListInput<L>` | `SystemParam` | keys, pad and guitar through `MenuNav`, wheel, pointer; `read(&mut cursor, count) -> ListEvents` |
| `ListEvents` | struct | `nav` (up/down/left/right/confirm/back), `moved`, `clicked`, `step` (−1/+1 from keys or the clicked half), `right_click` |
| `ListPaint<L>` | `SystemParam` | `paint` (dress rows, write values), `paint_full` (a list whose visible length changes, labels too), `paint_armed` (one row drawn waiting — the controls capture) |
| `follow_cursor` | fn | the measured whole-row window every scrolling list uses |
| `sound_for(feel, moved)` | fn | click / tick / navigate |
| `HeldRow` | resource | holds every list's cursor on one row (`BEATBYTE_SHOT_ROW`) |

The rules it enforces for every list: the cursor **stops at both ends**;
Page Up / Page Down move eight rows and Home / End reach the ends
(`jump_to`);
the wheel moves the cursor (it never steps a value); the pointer selects
a row only when it **moved** (a window opening under a resting mouse
takes nothing); a click on a value row steps by the **half** it lands
on; LEFT+RIGHT in one frame is one step.

A screen leaves through `DespawnOnExit(state)` on its root — no
per-screen despawn system.

## Recipes

### Add a setting

1. A field on `Settings` (`config.rs`) and its value in `Default`. A
   value that must reach files that already exist needs
   `#[serde(default = "…")]` naming a function (CLAUDE.md gotcha).
2. Classify it in `beatbyte-sync/src/settings.rs`: `SHARED` (follows the
   player) or `DEVICE` (stays on the machine). A test fails until you do.
3. One constant in `impl Row` (`settings_ui.rs`) — `row!` with
   `toggle!` / `slider!`, or a `Kind` written out.
4. List it in `Row::ALL` where its label falls alphabetically, and bump
   the array length.
5. If the pause menu should offer it, add `PauseItem::Setting(Row::…)`
   to `PAUSE_ROWS` in `gameplay/mod.rs`.

That is the whole change for a toggle: the demo toggle that proved it
(phase 6) was **11 lines in 3 files**, and the existing pin that every
row edits exactly one field of its own checked it without being told.

### Add a list screen

1. A marker type (`struct FooRows;`) and the panel:
   `(ListPanel::<FooRows>::new(), ui_kit::scroll_panel(…))`, rows from
   `spawn_rows` / `spawn_label_rows`, or `row_frame` around your own
   contents. Root carries `DespawnOnExit(AppState::Foo)`.
2. An input system with `ListInput<FooRows>`: `read`, then act on
   `events` (`confirm`/`clicked`, `step`, `back` via
   `ui_kit::wants_leave`), sound from `sound_for`.
3. A paint system with `ListPaint<FooRows>` and a **separate** follow
   system with `follow_cursor` — both take `&mut Node`, so they cannot
   share a system.
4. Any other query in the paint system that takes `&mut Text` (a hint
   line, a status line) needs `Without<ListLabel<FooRows>>` and
   `Without<ListValue<FooRows>>`. Forgetting it compiles, passes every
   wired test, and **stops the game at launch** (B0001).
5. Add the screen's plugin to `access_tests` in `lib.rs`, which
   initializes every schedule and catches exactly that.
6. Before the change, `tools/shot-check.sh record <dir>`; after it,
   `SHOTS="… foo foo:3" tools/shot-check.sh compare <dir>` — and **look
   at both pictures** when anything changed. 0 changed pixels is the
   default; a difference is a bug until it is explained.

## What is not on the renderer

The song browser (a tree with columns, search and sorting), statistics,
calibration, the input test, results, join, song info and the chart
editor. They are not row-cursor lists; they use `ui_kit` and `MenuNav`
directly. The editor's case is recorded in
[ADR-0024](../decisions/ADR-0024-editor-stays-on-bevy-ui.md).
