# The song browser, made solid

Diagnosis and plan, 2026-08-30. The browser gained sorting, search and
seven fact columns in two fast rounds; the user's verdict — better,
but still buggy and not optimal — is correct, and the causes are
identified in the code, not guessed.

## The diagnosis — confirmed defects

**D1 — Every change rebuilds the whole screen.** `sync_view` despawns
and respawns the *entire* browser — header, status, captions, panel,
rows, details, footer — on every search keystroke, every sort click,
and every LEFT/RIGHT difficulty step. One frame of full re-layout per
keypress: the scroll snaps, every `Interaction` resets, the screen
breathes. This is the core of "feels buggy".

**D2 — The resting mouse fights the keyboard.** Hover selects
(`pointer.hovered → cursor`), and hover fires on `Changed<Interaction>`.
After every respawn from D1, the row that spawns under the *resting*
pointer fires `None→Hovered` — so typing a letter or stepping the
difficulty can yank the selection to wherever the mouse happens to
lie. The two defects multiply each other.

**D3 — Delete-arming is bound to a view POSITION.** `delete_armed`
stores `cursor.0`. Re-sort or filter between the first and second
Backspace and the position names a different song. The confirm
question was asked about one track and the deletion armed for another
— exactly the class of bug the two-press confirmation exists to
prevent.

**D4 — While filtering, the cursor stays glued to the old song.**
`stable_cursor` is right for *sort* changes (the selection must follow
its song) but wrong for *typing*: when the filter narrows, the
expectation of every search UI is that the selection sits on the
first match, ready for Enter.

**D5 — Small confirmed roughnesses.** Empty filter result shows a
bare empty panel with no "no match — ESC clears" hint. Backspace in
search only reacts to `just_pressed` (no key repeat). Sort mode and
direction reset on every app start. Sort/search/header actions give
no audio feedback where every other menu key blips.

## The shape of the fix

**One spawn, targeted updates.** The screen spawns once per entry.
Three kinds of dynamic content update **in place**:

- status line, header captions (label + marker + colour), details
  line: plain `Text`/`TextColor` writes from a refresh system;
- the **rows** rebuild only when their *content* actually changes —
  the display order, the filter, or the selected difficulty (cells
  show per-difficulty facts). Rebuilding means: despawn/respawn the
  children of the list node only. Header, footer, panel and scroll
  state stay alive;
- everything else never respawns.

This alone dissolves D1 and starves D2 (fewer respawns = fewer
phantom hovers).

## The plan

- **P1 — Rows-only rebuild.** `sync_view` keeps the screen; a
  `rebuild_rows` path replaces only `SongList`'s children, keyed on
  `(order, filter, difficulty)` actually differing. Status, captions
  and details become updated-in-place `Text`s with marker components.
  DoD: a search keystroke leaves header/footer entities untouched
  (asserted by entity-id stability in a test where feasible, else by
  the absence of the respawn path), scroll survives typing.
- **P2 — Hover requires motion.** Hover-select only acts when the
  mouse actually moved since the last frame (`CursorMoved` events),
  never on a freshly spawned row under a resting pointer. DoD: pin on
  the pure decision (`hover_may_select(mouse_moved)`), plus the D1
  fix removing the trigger.
- **P3 — Delete arms on the SONG.** `delete_armed` stores the library
  index (or title), not the view position; any mismatch disarms. DoD:
  test — arm, re-sort, the armed target still names the same song or
  disarms.
- **P4 — Filter typing selects the first match.** In `sync_view`:
  filter changed → cursor 0; sort changed → `stable_cursor` as today.
  DoD: pinned both ways.
- **P5 — Empty state speaks.** No matches → one dimmed line in the
  panel: `no match for "…" — ESC clears`. DoD: shot or spawn test.
- **P6 — Persist the sort.** `sort` + `flipped` join `settings.json`
  (sanitized like every field; filter deliberately NOT persisted — a
  stale invisible filter across sessions is a trap). DoD: round-trip
  test + sanitize test.
- **P7 — Feedback blips.** S / F / header clicks / sort flip play the
  existing `ui_move` blip. DoD: by ear, code inspection.

Order: P1 → P2 → P3/P4 (independent) → P5 → P6 → P7. P1 first because
it changes the structure the others touch.

## Explicitly not planned

- Type-to-search without pressing `F` (single letters collide with
  E/S/DEL shortcuts; a modal search with a visible prompt is the
  honest version).
- Virtualized rows (27 songs do not need it; revisit past ~200).
- Album art / preview audio on hover (real features, not polish —
  they belong on the roadmap as their own items if wanted).
