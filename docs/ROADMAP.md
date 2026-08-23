# BeatByte Roadmap

**Persistent source of truth** for what gets built, in what order.
Rules of engagement live in [`CLAUDE.md`](../CLAUDE.md); this file
holds the work itself.

## How to work this roadmap

- Tasks are **small and independently verifiable**: each one names how
  to prove it done (*Verify:*). A task without a passed verification
  is not done.
- Tasks are **dependency-aware**: `dep:` names the task IDs that must
  be complete first. Never start a blocked task.
- Statuses: `[x]` done · `[ ]` open · `[~]` in progress (at most one
  `[~]` at a time).
- Completing a task = check it here, meet the Definition of Done in
  CLAUDE.md, commit. Scope changes are edited into this file, with a
  one-line rationale, in the same commit.
- IDs are stable (`M5.3`, `A2`, …) — never renumber, only append.

**Status: all 13 foundation milestones shipped (v0.8.1, 2026-08-23).
Current phase: Road to 1.0.**

---

## Phase 1 — Foundation milestones (SHIPPED)

Kept as the dependency record and the map of where things live.

### M1 — Foundation (v0.0.1)

- [x] M1.1 Cargo workspace: `beatbyte-{core,chart,audio,editor,cli,game}` + `apps/beatbyte` launcher. *Verify: `cargo check --workspace`.*
- [x] M1.2 Lints as policy: clippy `-D warnings`, `unsafe_code`/`missing_docs` warn, unwrap/dbg/todo denied in non-test code. *Verify: gate passes.*
- [x] M1.3 CI (fmt/clippy/test/check on latest stable) + repo scaffolding (LICENSE MIT, AUTHORS, CONTRIBUTING, SECURITY, CoC, CHANGELOG). *Verify: CI green on main.*
- [x] M1.4 Press Start 2P bundled with OFL license; `docs/asset-licenses.md`. *Verify: license file next to font.*

### M2 — Core domain (v0.0.2) — dep: M1

- [x] M2.1 `Lane`/`LaneSet` bitmask (incl. `highest()` for anchoring), `Difficulty`. *Verify: unit tests.*
- [x] M2.2 `TempoMap`, `TimingWindows` (30/60/100 ms), `Judgment`. *Verify: unit tests.*
- [x] M2.3 `NoteEvent`/`Phrase`/`Track` invariants (sorted, no simultaneous same-lane). *Verify: constructor tests.*
- [x] M2.4 Scoring: 50/35/20 pts, streak→multiplier (10/level, max 4), Hype (25 %/phrase, 0.5 activation, 32-beat drain). *Verify: score tests.*
- [x] M2.5 `TrackSession` judgment engine: input-stamp-driven hits, misses, overstrums, HOPO/pull-off, per-beat sustains. *Verify: 58-test suite.*

### M3 — Audio engine (v0.0.3) — dep: M1

- [x] M3.1 Decode + resample (rodio decoder, 31-tap FIR half-band), WAV in/out helpers. *Verify: round-trip tests.*
- [x] M3.2 `SongClock`: anchored monotonic time, snap ≥30 ms / slew 10 % vs device position. *Verify: clock tests.*
- [x] M3.3 Music on a dedicated thread behind Send `MusicHandle` (mpsc + atomics): play/pause/seek/volume/position. *Verify: playback tests + game integration.*
- [x] M3.4 Synth: click track + burst synthesis (15 % release ramp — hard truncation is an audible/detectable click). *Verify: onset tests.*
- [x] M3.5 Build-time synthesized demo song (128 BPM, no copyrighted audio). *Verify: `beatbyte-cli demo`.*

### M4 — Analysis & chart generation (v0.0.3) — dep: M3

- [x] M4.1 STFT spectral flux onsets, adaptive median threshold, brightness per onset. *Verify: synthetic-fixture tests; frame_offset_s convention documented.*
- [x] M4.2 Tempo: autocorrelation + log-normal prior + parabolic interpolation; beat-grid phase fit. *Verify: demo detects 128.3 BPM.*
- [x] M4.3 Deterministic generation via `DifficultyProfile`: grid quantization (raw-onset fallback), brightness lane walk, chords/HOPOs/sustains, splitmix variety. *Verify: 103/117/249/491 notes across difficulties, stable across runs.*
- [x] M4.4 Chart format v1 + validation: BPM 20–400, 32 MB cap, path traversal + Windows-drive-`:` rejection. *Verify: adversarial tests (incl. `C:\music.ogg` on Unix).*
- [x] M4.5 CLI: analyze/generate/validate/inspect/demo, exit codes 0/1/2. *Verify: end-to-end CLI run.*

### M5 — First playable (v0.1.0) — dep: M2, M3, M4

- [x] M5.1 App states (Boot/MainMenu/SongSelect/Gameplay/Results + GamePhase substate). *Verify: smoke test.*
- [x] M5.2 Highway rendering: lanes, receptors, scrolling notes, sustains. *Verify: screenshot.*
- [x] M5.3 Input → stamped judgment wiring; SessionFeedback message bus. *Verify: autopilot flawless run (117/117).*
- [x] M5.4 Autopilot harness (`BEATBYTE_AUTOPILOT=1`): perfect play through real screens, non-zero exit on any miss/overstrum, `.before(advance_sessions)`. *Verify: exit 0; deliberate offset ⇒ exit non-zero.*
- [x] M5.5 Smoke harness (`BEATBYTE_SMOKE_TEST=1`). *Verify: exit 0.*

### M6 — Game feel (v0.2.0) — dep: M5

- [x] M6.1 Judgment popups, hit particles, receptor flash, combo/multiplier HUD. *Verify: screenshots + autopilot.*
- [x] M6.2 Hype meter + activation flow; `active_sustain()` accessor. *Verify: tests + visual.*
- [x] M6.3 SFX (synthesized, no copyrighted audio). *Verify: audible + no asset-license change.*

### M7 — UI (v0.3.0) — dep: M5

- [x] M7.1 Pixel-font UI kit (`UiFont`, loaded at plugin build — OnEnter can run before startup flushes). *Verify: regression noted; menus render.*
- [x] M7.2 Main menu, song browser (library scan + user songs dir), settings, results (grade + count-up). *Verify: screenshots.*
- [x] M7.3 Latency calibration screen (120 BPM clicks, median of 8+ taps → settings). *Verify: manual calibration run.*
- [x] M7.4 Settings persistence (`Settings` + sanitize + ConfigPlugin). *Verify: restart retains values.*
- [x] M7.5 First published release with CI artifacts. *Verify: v0.3.0 prerelease, artifact smoke-tested.*

### M8 — Controllers (v0.4.0) — dep: M7

- [x] M8.1 Input abstraction: `GameAction`/`Binding`/`InputMap` persisted in Settings (bevy `serialize` feature for KeyCode/GamepadButton). *Verify: remap survives restart.*
- [x] M8.2 Gamepad play (South/East/North/West/LeftTrigger defaults) + `MenuNav` merged keyboard/pad menu input. *Verify: manual pad run.*
- [x] M8.3 Controls remap UI. *Verify: remap flow end-to-end.*

### M9 — Local multiplayer (v0.5.0) — dep: M8

- [x] M9.1 Players as entities (`PlayerSession`/`PlayerIndex`/`PlayerDevice`), input routed by `DeviceId`. *Verify: tests + 2P autopilot.*
- [x] M9.2 `HighwayLayout` for 1–4 highways; `PlayerRoster`, MultiplayerSetup screen, per-player colors. *Verify: 2P/4P screenshots.*
- [x] M9.3 Multi results: ranked + band totals. *Verify: 2P autopilot to results.*

### M10 — Themes (v0.6.0) — dep: M6

- [x] M10.1 Six data-driven themes + procedural backdrop animation (ADR-0008); theme in settings. *Verify: solo + 4P autopilot across themes; screenshots.*

### M11 — Chart editor (v0.7.0) — dep: M4, M7

- [x] M11.1 Invertible `EditOp` (Add/Remove/ToggleHopo/SetLen; apply returns inverse; strict misses = errors). *Verify: inverse round-trip tests.*
- [x] M11.2 `EditorSession`: undo/redo/dirty/validity. *Verify: tests.*
- [x] M11.3 Editor UI (open from song select, grid division stepping, save through chart validation). *Verify: editor autopilot cycle (add/undo/redo/save).*

### M12 — Packaging (v0.7.0) — dep: M7

- [x] M12.1 Hand-rolled PNG icon generator (python stdlib, no deps). *Verify: 1024×1024 PNG parses.*
- [x] M12.2 macOS `.app` + DMG (`packaging/macos.sh`: iconutil, ad-hoc codesign, hdiutil). *Verify: mounted .app smoke from neutral CWD.*
- [x] M12.3 AppImage (`packaging/appimage.sh`). *Verify: CI artifact exists.*
- [x] M12.4 Release workflow: 4 targets, DMG/AppImage, draft release. *Verify: green run + published release.*
- [x] M12.5 Explicit asset-root resolution for bundle layout. *Verify: superseded by M13.4 (full fix).*

### M13 — Polish (v0.8.0, patched v0.8.1) — dep: M10, M11, M12

- [x] M13.1 Screen-transition fades (0.25 s). *Verify: visual; screenshot settle delay added.*
- [x] M13.2 Count-in: 2 s pre-roll, 3-2-1 banner, music starts at exactly zero. *Verify: identical score with/without pre-roll (23399) proves judgment unshifted.*
- [x] M13.3 Docs/README brought to finished-milestone state. *Verify: review.*
- [x] M13.4 **v0.8.1**: asset root resolved across all launch layouts (portable/bundle/CWD/target ancestors) — a failed font never retries and had made all text invisible. *Verify: font `Loaded`, glyph counts > 0, flawless artifact autopilot from neutral CWD, fresh screenshots.*
- [x] M13.5 Release engineering fixes: macOS runner disk reclaim before hdiutil; recursive publish glob; `dist/` untracked + ignored. *Verify: green release run, all 7 assets attached.*

---

## Phase 2 — Road to 1.0 (OPEN)

Exit criterion for 0.x (from CLAUDE.md): **the gameplay tuning
settles** — real hands played it, the numbers stopped moving, and the
chart format v1 can be frozen as a promise.

### A — Feel & tuning

- [ ] A1 **Human playtest protocol + first pass.** Write `docs/playtest.md` (what to test: timing windows, HOPO feel, sustain scoring, Hype pacing, calibration flow; how to record findings), then run it on the demo song at all difficulties. *Verify: findings recorded in `docs/playtest.md` with date + build.*
- [ ] A2 **Tuning adjustments from findings.** Apply window/scoring/HOPO changes A1 demands, or record explicitly that none were needed. dep: A1. *Verify: updated tests pin new values; autopilot flawless; CHANGELOG entry.*
- [ ] A3 **Calibration validated against real latency.** Play through a deliberately delayed audio path (e.g. Bluetooth) and confirm the calibrated offset lands within one timing window of measured latency. *Verify: measured vs calibrated numbers noted in `docs/playtest.md`.*
- [ ] A4 **Difficulty curve review.** Check Easy→Expert of both built-in charts reads as a curve (density, chords, HOPO runs); adjust `DifficultyProfile` if not. Known input: Hard/Expert barely generate sustains even over held pad bars (B1 measurement: 1–2 vs Medium's 6) — likely their low strength floors pick up spurious onsets inside held chords, breaking the gaps. dep: A1. *Verify: note-count/density table in findings; generation tests updated.*

### B — Content & chart workflow

- [x] B1 **Second synthesized demo song.** "Solder Groove", 92 BPM half-time groove (Dm–Bb–F–C, syncopated bass, held pad bars → real sustains). Multi-builtin refactor: `SongSource::Builtin(usize)` + `BuiltinSongs`; CLI `demo` writes both. *Verified: analyzes to 92.06 BPM (a tresillo bass first read as ~122 — reshaped to keep the quarter pulse dominant); autopilot flawless 1P (89/89) + 2P; Circuit Breaker unaffected; 158 tests.*
- [x] B2 **Autopilot over any library song.** `BEATBYTE_AUTOPILOT_SONG=<index|title-substring>` (bad selector = loud failure). *Verified: 4 unit tests (one mutation-checked); live run with `=circuit` flawless, `=nonexistent` exits 1. Second-song run covered under B1.*
- [x] B3 **Import walkthrough verified end-to-end.** No guide existed — written as `docs/importing-songs.md` with every command executed for real (an MP3 through analyze → generate → validate → library → play → editor). **Two real defects found:** the library scan missed the documented `songs/imported/<song>/` layout (one level too shallow — fixed with a bounded symlink-safe walk + unit test), and the CLI help still claimed ogg/wav/flac/mp3 only. *Verified: autopilot flawless on the imported song (87/87), editor cycle PASSED on it.*
- [x] B4 **Supported-audio-formats truth.** 5 decode tests against committed synthesized fixtures (~5-22 KB each, recipe in `tests/fixtures/README.md`); table in chart-format spec. Finding: **M4A/AAC decodes** — the "not supported" pin failed immediately, README undersold the decoder; docs updated, pin flipped positive. The ogg fixture is stereo on purpose (pins the downmix). *Verified: 5/5 passing.*

### C — Robustness & platform coverage

- [ ] C1 **Linux smoke in CI.** Run `BEATBYTE_SMOKE_TEST=1` under xvfb (or headless winit) in the CI matrix. *Verify: CI job green on PR and main.*
- [ ] C2 **Windows artifact validated.** Smoke-test the Windows zip on real Windows (or a CI windows job running the smoke harness). *Verify: run log/screenshot noted in release notes of the next release.*
- [ ] C3 **CI actions off deprecated Node runtimes.** Bump `actions/checkout` / `upload-artifact` etc. until release + CI runs show zero deprecation annotations. *Verify: annotations gone on a green run.*
- [ ] C4 **Gamepad hot-plug.** Define + implement behavior for pad disconnect mid-song (pause + reconnect prompt is the target); reconnection resumes the same `PlayerDevice`. *Verify: manual unplug test 1P and 2P; no panic, session intact.*
- [ ] C5 **Settings/chart forward-compat reads.** Unknown fields in settings and chart JSON are tolerated (serde defaults / deny-unknown only where security demands). *Verify: fixture with extra fields loads; test pins it.*

### D — Accessibility & options

- [ ] D1 **Colorblind-safe lane identity.** Per-lane shapes/symbols on notes + receptors (color no longer the only channel), always on or as an option. *Verify: screenshot; deuteranopia simulation check.*
- [ ] D2 **Reduced-motion option.** Setting that stills backdrop animation and particle bursts; respects it everywhere. *Verify: setting persists; visual check per theme.*
- [ ] D3 **Audio mix options.** Separate music vs SFX volume in settings (add whichever is missing). *Verify: settings rows work + persist; audible difference.*
- [ ] D4 **Scroll-speed option.** Per-player highway scroll speed independent of difficulty. *Verify: setting persists; autopilot still flawless at extremes (judgment must be unaffected).*

### E — Editor v2 (quality-of-life)

- [ ] E1 **Move/nudge op** (time and lane), invertible like the rest. *Verify: inverse round-trip test; editor autopilot extended.*
- [ ] E2 **Selection + bulk ops** (delete range, toggle HOPO on selection) composed of primitive ops so undo stays exact. *Verify: undo restores byte-identical chart; tests.*
- [ ] E3 **Audition playback in editor** (play from cursor with click overlay). dep: E1 not required. *Verify: manual; no clock regressions (autopilot).*

### F — Release engineering to 1.0

- [ ] F1 **v0.9.0 — the tuning release.** Ship A1–A4 + whatever of B/C/D landed. dep: A2. *Verify: full release procedure incl. artifact smoke tests.*
- [ ] F2 **1.0 readiness checklist.** All A tasks done; C1–C3 done; chart format v1 documented as frozen (changes now = format v2); README/docs audit; all four platform artifacts verified at least once on their real OS. dep: F1, A*, C1–C3. *Verify: checklist appended here with evidence links.*
- [ ] F3 **v1.0.0.** Tag, release, drop `--prerelease`. dep: F2. *Verify: published, artifacts tested, CHANGELOG sectioned.*

---

## Backlog (explicitly out of scope until after 1.0)

Not started without a deliberate roadmap edit pulling them forward:

- Practice mode (section looping, speed adjustment without pitch).
- Replays (input-stamp recordings are naturally replayable — the
  architecture already permits it).
- Song-pack distribution format / in-game chart sharing.
- Online features of any kind (leaderboards, netplay).
- Additional themes and theme editor.
- Localization (UI is English-only by decision until then).
- Mod/plugin hooks.

## Known non-issues (documented so they aren't re-investigated)

- Black screenshots in the first seconds of a run: macOS window
  occlusion — frames aren't rendered, nothing is broken.
- `v0.3.0` remains published as a historical milestone prerelease;
  earlier drafts were deliberately deleted (tags remain).
- Menu screenshot in the autopilot set is often black for the same
  occlusion reason; gameplay/results shots are the useful ones.
