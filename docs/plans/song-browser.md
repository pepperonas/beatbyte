# Plan: the song browser, rebuilt

Status: **done** (v0.18.75, 2026-10-05). The user: "das suchen und navigieren
in der songauswahl vor dem eigentlichen spielen finde ich katastrophal
dargestellt. die seite kannst du komplett neu erstellen." Decisions taken
the same day (all four recommendations): **list + detail panel**, the
rarely used tools in **one action menu**, **one row per song** (versions
chosen in the detail panel), **type to search** (no F).

## What is wrong today (photographed 2026-10-05, 1280×800)

- Thirteen chips of equal weight in one row (OPEN … DELETE), CANCEL and
  CONFIRM floating in the column header.
- Ten columns, most of them empty or "-"; names clipped ("STEVIE RAY
  VAUGHA~"); "[BG-01]" in front of nearly every title; nine rows visible.
- The selected song's facts (difficulty, stars, best) in a small footer
  line, three hint lines below it; the page jumps 170 px when the search
  opens.
- Search needs F first; no way to jump through 1 400 songs.

## What stays exactly as it is (inventory 2026-10-05)

Every behaviour of `song_select.rs` (4 726 lines, 55 tests) is kept; only
the presentation and the key layout change:
- the model: `build_order` (fuzzy filter, eleven sorts, flip, persisted
  sort), `pair_twins` / `original_in`, `song_tree`, `prepare_song`,
  `may_start`, `delete_step`, `DownloadPrompt`, the per-player preferred
  difficulty, the MC queue, the preview (rest 0.55 s, fade), the import
  note, `BrowserInputSet`, `BrowserView`/`BrowserCursor` for the harness;
- every tool: add a song, lyrics lookup, align, redesign, Bridge import,
  taste test, queue / play set, edit chart (incl. a specific revision),
  info, delete (asks, `Y` answers), activating an older revision.

## The new screen

```
┌ SONGS ────────────────────────────┬ SONG ──────────────────────────┐
│ search: queen_          4 / 1 466 │ KILLER QUEEN                   │
│ SORT  TITLE ▾                     │ Queen · Rock · 3:02 · 117 BPM  │
│ ▌Another One Bites the Dust  ★★★ │                                │
│   Queen                       82% │ ◀ EASY  MEDIUM [HARD] EXPERT ▶ │
│  Killer Queen               ★★★ │   ★★★   386 notes   best –     │
│   Queen                        –  │                                │
│  …                                │ VERSION  NORMAL · BG-01 · CL   │
│                                   │ LYRICS word by word · AUDIO ok │
└───────────────────────────────────┴────────────────────────────────┘
 ENTER play  ←/→ difficulty  [ ] version  TAB actions  ESC back
```

- **List (left):** one row per song family (original + its GS/CL/BG
  twins), two lines (title, artist), right-aligned: stars of the selected
  difficulty and the best accuracy. No "[BG-01]" prefix — the version is a
  property of the row, shown in the panel.
- **Panel (right):** title, artist, genre, length, BPM; the difficulty
  selector with stars, notes and best per difficulty; the versions of the
  song as a selector (default: the one played last, else the original);
  revisions of the chosen version (as today's tree, on demand); lyrics /
  chart / audio status in words.
- **Search:** always at the top; any printable key types into it (Space
  too); Backspace erases; Esc clears, a second Esc leaves; the count
  "4 / 1 466" beside it. Enter plays from a filtered list.
- **Sort:** one line "SORT TITLE ▾" — a click or Ctrl+S cycles, a second
  click on the same flips (all eleven sorts kept, persisted as today).
- **Actions (TAB, or the ⋯ button):** a list in the house style — Play,
  Edit chart, Info, Lyrics, Align, Redesign, Taste test, Queue / Play set,
  Add a song, Bridge import, Delete (asks; `Y` answers). Every tool keeps a
  shortcut as **Ctrl/Cmd + its old letter** (Ctrl+E edit, Ctrl+K align,
  Ctrl+T taste, Ctrl+Q queue, …), so nothing a player learned is lost.
- **Navigation:** Up/Down, wheel, pointer; Page Up/Down and Home/End; the
  cursor stops at the ends. The page does not move when the search fills.

## Steps (one commit each, gate + screenshots each)

1. **Split the module, change nothing.** `song_select.rs` → `song_select/`
   with the pure model in its own files (sort/filter, twins, difficulty,
   delete, start rule, prepare_song). The 55 tests stay green unchanged.
2. **Families and versions.** A pure `families(view) → Vec<Family>` (one
   per song, its versions in order, the default version), tested on the
   cases `pair_twins` already pins (chains, a twin without its original,
   BG gathering).
3. **The new layout** (list + panel), drawn from `ui_kit` only (type
   scale, row states, panels). The old table, its captions and the chip
   row go.
4. **Input:** type-to-search, the version selector, Page/Home/End, the
   action menu with Ctrl/Cmd shortcuts, the delete question inside it.
5. **Harness:** `BEATBYTE_SHOT_*` keep working (`SHOT_OPEN` opens the
   version list), the delete/align/taste drills move to the new keys;
   new switch `BEATBYTE_SHOT_ACTIONS` photographs the action menu.
6. **Docs:** README controls, harness reference, CHANGELOG, this plan.

Verification each step: the full gate; the browser photographed at
1280×800 and shown before the commit; the autopilot plays a song from the
new screen; the delete/align/taste drills read their own verdict line.

## Also in this round (menus)

- [x] Page Up/Down, Home/End in every list (`menu_list::list::jump_to`).
- [x] A dot on every setting that differs from the default; BACKSPACE
  resets the selected row.
- [ ] One line of explanation per main-menu item.
- [ ] A thin fill bar under slider values.
- [ ] Type a letter in Settings to jump to the first row starting with it.
