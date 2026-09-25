# Plan — the chart editor, from keyboard tool to full editor

Status: **in progress** (2026-09-25). Commission: a full note-chart
editor, mouse and keyboard equal, built out of the existing one.

## What exists (inventory, 2026-09-25)

- **Storage.** A song is a folder `<data>/beatbyte/songs/imported/<song>/`:
  `chart.json` (the original), `chart.vN.json` (versions),
  `chart-active.json` (the pointer the game follows, ADR-0011),
  `*.context.json` (analysis per note), `song.json` (identity), audio.
- **Model.** `ChartFile` → `charts: Vec<ChartDef>` per difficulty →
  `notes` (`time` s, `lane` 0–4, `len` s, `hopo`) and star-power
  `phrases` (time ranges); plus `provenance`, `audio_trim` (the AAC
  priming the decode skips), `grid` (the tracked beat grid).
- **Time.** Seconds on the song timeline, everywhere. Beats come from
  `ChartFile::beat_marks()` — the tracked grid, else a constant grid
  from `bpm`/`offset_s`, four beats a bar. Chords are simultaneous
  notes on different lanes.
- **Import.** Copy the audio into a folder, analyse, `generate_chart`,
  write `chart.json` + `song.json`; a re-import writes the next
  version and moves the pointer.
- **Editor.** `beatbyte-editor` (invertible `EditOp`s, `EditorSession`
  with undo/redo and atomic batches) + `beatbyte-game/src/editor_ui.rs`
  (vertical highway around a cursor, keyboard only: place/move/delete,
  HOPO, length, range select, audition with metronome, save).
  Harness: `BEATBYTE_AUTOPILOT_EDIT`.
- **Reusable.** Practice speed (`MusicHandle` speed + `SongClock::set_rate`;
  pitch drops with tempo) and loop; `LoadedSong` takes a chart IN
  MEMORY (a playtest needs no file); `play_file_from` starts mid-song;
  the taste test shows how runs stay out of the score book.

Defects found by the inventory: the editor saved OVER the loaded
file (the active version); `save_chart_file` was not atomic; four
writers of versions (`redesign`, `classic`, re-import, study twins)
would each bury a hand edit under a new active version; the editor
drill saved into the player's real library.

## Decisions (the commission's defaults)

- 0.5×/0.75× by simple slow-down (pitch drops) — the practice path.
- The beat grid is shown and snapped to, not edited.
- Best scores stay; one set on an older chart version is marked.
- The vertical highway stays the editor's view (it is what the
  playtest shows, and what the existing editor already is).

## Stages

| # | What | State |
|---|---|---|
| 0 | Foundation: atomic chart writes; a save is a NEW version marked `designer: "editor"`, one per editing session; `redesign`, `classic` and a re-import never supersede a hand-edited active version; phrase and empty-difficulty ops; musical time (`timecode::Grid`: seconds ↔ bar:beat:tick, snap by division, tempo changes, past the grid's ends); the drill edits a scratch copy | done (v0.18.43) |
| 1 | Timeline: zoom, free scroll, waveform, bar/beat lines with numbers, mouse (place, select, drag move, drag length, box select, right-click delete, wheel scroll, Cmd/Ctrl-wheel zoom, click/drag to scrub), hover info, toolbar | |
| 2 | Playback: play/pause, scrub, loop region, 0.5×/0.75×/1×, auto-follow that yields to manual scrolling | |
| 3 | Multi-select, clipboard, duplicate, inspector fields (time, lane, length), context menu, phrases, difficulty switch/creation | |
| 4 | Warnings panel (same-lane overlap, bad lengths, notes past the sounding end, validation), exit guard, older-version best scores marked | |
| 5 | Playtest: song or section, unsaved notes, in the real highway, back to the same place | |

Each stage: gate, the extended drill, version + CHANGELOG + roadmap,
committed and pushed.
