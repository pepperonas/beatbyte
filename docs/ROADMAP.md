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

- [~] A1 **Human playtest protocol + first pass.** Protocol written (`docs/playtest.md`: session script over both songs × four difficulties, feel questions mapped to A2/A3/A4, findings-log format). **Blocked on human hands for the first pass** — the harness proves correctness, not feel. *Verify: findings recorded in `docs/playtest.md` with date + build.*
- [ ] A2 **Tuning adjustments from findings.** Apply window/scoring/HOPO changes A1 demands, or record explicitly that none were needed. dep: A1. *Verify: updated tests pin new values; autopilot flawless; CHANGELOG entry.*
- [ ] A3 **Calibration validated against real latency.** Play through a deliberately delayed audio path (e.g. Bluetooth) and confirm the calibrated offset lands within one timing window of measured latency. *Verify: measured vs calibrated numbers noted in `docs/playtest.md`.*
- [ ] A4 **Difficulty curve review.** Check Easy→Expert of both built-in charts reads as a curve (density, chords, HOPO runs); adjust `DifficultyProfile` if not. Known input RESOLVED early (2026-08-25, user playtest report: live track got no sustains): sustain generation is energy-first now — gap only bounds length, an ABSOLUTE strong-onset bar cuts the hold (a relative bar cut everything in live reverb and cost Rick's medium chart 53→37 before the fix). Measured: live track medium 3→51, Rick 53→92, Solder Groove hard/expert 1/2→14/12; both real tracks replayed flawlessly. Remaining for A4: the density/chord curve itself. dep: A1. *Verify: note-count/density table in findings; generation tests updated.*

### B — Content & chart workflow

- [x] B1 **Second synthesized demo song.** "Solder Groove", 92 BPM half-time groove (Dm–Bb–F–C, syncopated bass, held pad bars → real sustains). Multi-builtin refactor: `SongSource::Builtin(usize)` + `BuiltinSongs`; CLI `demo` writes both. *Verified: analyzes to 92.06 BPM (a tresillo bass first read as ~122 — reshaped to keep the quarter pulse dominant); autopilot flawless 1P (89/89) + 2P; Circuit Breaker unaffected; 158 tests.*
- [x] B2 **Autopilot over any library song.** `BEATBYTE_AUTOPILOT_SONG=<index|title-substring>` (bad selector = loud failure). *Verified: 4 unit tests (one mutation-checked); live run with `=circuit` flawless, `=nonexistent` exits 1. Second-song run covered under B1.*
- [x] B3 **Import walkthrough verified end-to-end.** No guide existed — written as `docs/importing-songs.md` with every command executed for real (an MP3 through analyze → generate → validate → library → play → editor). **Two real defects found:** the library scan missed the documented `songs/imported/<song>/` layout (one level too shallow — fixed with a bounded symlink-safe walk + unit test), and the CLI help still claimed ogg/wav/flac/mp3 only. *Verified: autopilot flawless on the imported song (87/87), editor cycle PASSED on it.*
- [x] B4 **Supported-audio-formats truth.** 5 decode tests against committed synthesized fixtures (~5-22 KB each, recipe in `tests/fixtures/README.md`); table in chart-format spec. Finding: **M4A/AAC decodes** — the "not supported" pin failed immediately, README undersold the decoder; docs updated, pin flipped positive. The ogg fixture is stereo on purpose (pins the downmix). *Verified: 5/5 passing.*

### C — Robustness & platform coverage

- [x] C1 **Linux smoke in CI.** `linux-smoke` job: the real game boots to the menu under Xvfb + mesa software Vulkan (lavapipe) on a GPU-less, audio-less runner — also validating the music thread's graceful no-device degradation. Two iterations to green: `libxkbcommon-x11-0` missing at runtime (winit dlopens it). *Verified: job green in 16m28s on main.*
- [x] C2 **Windows artifact validated.** `windows-smoke` CI job boots the real game to the menu on a GPU-less Windows runner (DX12 WARP software rasterizer), no audio device — the same code path the zip artifact ships. *Verified: job green (40m54s cold; caching shrinks it). Real-hardware run of a release zip remains a nice-to-have under F2.*
- [x] C3 **CI actions off deprecated Node runtimes.** Two rounds: v5 of upload-artifact turned out to STILL be Node 20 (release-run annotation) — the reals, verified against each action's own `action.yml`: checkout v5, upload-artifact v7, download-artifact v8 (keeps `merge-multiple`), action-gh-release v2→v3. *Verified: v0.9.0 release run clean except gh-release (bumped after; confirms next release).*
- [ ] C4 **Gamepad hot-plug.** Define + implement behavior for pad disconnect mid-song (pause + reconnect prompt is the target); reconnection resumes the same `PlayerDevice`. *Verify: manual unplug test 1P and 2P; no panic, session intact.*
- [x] C5 **Settings/chart forward-compat reads.** Both were already tolerant (`#[serde(default)]` on Settings, selective defaults on chart schema; corrupt settings fall back to defaults with a warning) — now PINNED: settings load with unknown+missing fields, malformed JSON errors cleanly, charts with unknown fields at file/song/chart/note level parse and stay valid. *Verified: 3 tests; deny_unknown_fields mutation makes them fail.*

- [ ] C5 **Score keys collide on a pipe character.** `ScoreBoard::key` is `title|artist|difficulty` with no escaping, so a title containing `|` can produce the same key as a different song and silently overwrite its record. Titles come from file names, where `|` is legal on macOS and Linux. Found while writing scoreboard tests; pinned by `known_limitation_a_pipe_in_a_title_can_collide`, which must flip to asserting `None` when this is fixed. Not fixed on sight because the repair changes the key format and would orphan every record already on disk — needs a load-time migration and the player's say-so. *Verify: colliding titles keep separate records; an existing scores file survives the migration with every record intact.*

### D — Accessibility & options

- [x] D1 **Colorblind-safe lane identity.** Always-on per-lane shapes (square/circle/diamond/triangle/X) as generated 16×16 pixel masks (`shapes.rs`, pure + 3 geometry tests) on gems AND receptors (shaped rings). Bonus catch: HOPOs had NO visual distinction — now smaller gem + bright core. *Verified: flawless autopilot; screenshot; grayscale (total color removal) keeps all five receptors trivially distinct; README media updated.*
- [x] D2 **Reduced-motion option.** New `backdrop_motion` setting ("STAGE MOTION" row) wired Settings → EffectSettings → `animate_backdrop` early-out; particles/shake/beat-pulse toggles already existed. *Verified by transform probe: ON = bit drifts continuously, OFF = position bit-identical over 25 s; both runs flawless. Side catch: the verification exposed that a window-closed autopilot run faked exit 0 — harness hardened (DontExit + fail-on-vanished-window).*
- [x] D3 **Audio mix options.** Already fully implemented (M7-era): `music_volume`/`sfx_volume` settings rows, persisted, music applied centrally in `apply_settings` on any change, SFX per-play. Closed by code-trace audit during C5/D2 work; no change needed.
- [x] D4 **Scroll-speed option.** Global setting existed (240–900 px/s row, persisted); scroll speed touches only note Y/tail height, never judgment. *Verified: identical flawless score (23399) at 240 solo and 900 2P.* Scope amended: per-player speed deferred until a playtest (A1) actually asks for it — speculative multiplayer-UI complexity otherwise.

### E — Editor v2 (quality-of-life)

- [x] E1 **Move/nudge op.** `EditOp::MoveNote` (keeps len/hopo, sorted invariant, Occupied unless the destination is the note itself) + M grab/place UI with ESC cancel. *Verified: 4 op tests incl. exact-chart inverse round-trip; editor autopilot extended with move+undo+redo position checks — which exposed that the harness had been SAVING its probe note into the chart (now sweeps its slots first; passes twice in a row).*
- [x] E2 **Selection + bulk ops.** `EditorSession::edit_batch` (grouped undo stacks, atomic — a failing op rolls back the whole batch) + V/X/H-selection UI, ESC clears. *Verified: 2 session tests (one-step undo, atomicity incl. no-residue on failure, empty batch no-op); editor autopilot extended (batch add ×2 = exactly one undo step, both notes appear and vanish together); cycle PASSED.*
- [x] E3 **Audition playback in editor.** Play-from-cursor already existed (M11); added the beat-click overlay (`preview_clicks` + synthesized tick in SfxLib) and an audition phase in the editor harness that counts real ticks via `AuditionClicks`. *Verified: 7 clicks in ~4 s at 92 BPM (exactly one per beat); mutation (counter disabled) fails the harness; gameplay autopilot still flawless.*

### G — User-requested (2026-08-25, from first hands-on session)

- [x] G1 **Tap mode (keyboard-friendly, no strum).** Core: `tap_mode` generalizes the HOPO rule to every note; settings row; tap runs skip the scoreboard. *Verified: 4 core tests; key-harness proof both directions (tap+no-strum flawless, no-tap+no-strum = all misses).*
- [x] G2 **Real-keyboard harness.** `BEATBYTE_AUTOPILOT_KEYS` presses actual KeyCodes (press-lead, re-tap on held keys in tap mode, missed events skip without phantom strums). Found: the first registration silently no-opped (replace-anchor drift) — every earlier "keyboard proof" had been the direct feed; asserts on anchors now. *Verified: classic PASS / tap PASS / neither FAIL.*
- [x] G3 **Drag-and-drop song import.** `import.rs`: FileDragAndDrop → copy to `songs/imported/` → async analyze+chart → library rescan; browser rebuilds on the changed resource and shows the status; name heuristic strips download noise (4 unit tests). *Verified end to end: injected drop of "Drop Test - Import Works (Official Audio) [abc123].wav" → title "Import Works" by "Drop Test" → chart on disk → autopilot waits for the import and plays it flawlessly.*
- [x] G4 **Sustain-note animation.** `animate_sustains` after `move_notes`: gem pinned + white-pulsing, tail consumed bottom-up with glow, spent tails dim; hold sparks existed (fx). *Verified by probe: head constant at RECEPTOR_Y while tails shrink monotonically (1026→914→809…), run flawless.*

- [x] G5 **Delete library entries in-game.** Backspace/Del with a 3-s double-press confirm; imported folders removed whole, hand-managed charts chart-only, built-ins refused. *Verified: 3 fs tests + real-key E2E (arrow-navigate, double Backspace, folder physically gone).*

- [x] G7 **Round style = whole-game look; tap mode default.** Playtest findings (first real hands-on): smooth font (UiFont style switch), scrolling fret lines from the chart's tempo, soft-disc particles + backdrop dots; tap default ON (scoreboard gate removed), autopilot direct feed made tap-aware (strum only while pending — it double-hit into 107 overstrums otherwise). *Verified: real-key no-strum run flawless on a FRESH profile (proves the default); round + 8-bit direct-feed runs flawless; screenshots.*
- [x] G6 **Note Style: round gems (8-bit off).** `round_gems` setting + "NOTE STYLE" row; round = disc/white-core/dark-ring (strum) vs no ring (HOPO), 128-px AA textures with linear sampling. D1 amendment: colorblind shapes go from "always on" to DEFAULT on — an explicit player choice may prefer color-only. *Verified: 7 mask/coverage tests; screenshots of both styles; flawless autopilot in both.*

- [x] G8 **Multi-track drag-and-drop + import feedback.** ImportQueue (batch counters, skip reasons counted, per-song library rescan) + animated overlay panel (pulse/easing bar/flash/summary, screen-independent). Root cause of "songs seemed lost": the old code imported one file per gesture and SILENTLY discarded the rest — plus the user's own drop had landed in the user songs dir (app ran as a bundle), which then double-listed against the repo copy → scan-level title dedupe. *Verified: 4-file drop E2E (3 real m4a + 1 txt → 3 imported, 1 skipped), The Passenger 568/568 and Two of Hearts 956/956 flawless, panel screenshot mid-batch (3/4, bar at a third).*

- [x] G9 **AAA polish for the round style.** HDR Bloom component synced to the style at runtime; shading-function texture suite (lit sphere, gloss, gaussian dot, tube, glow strip, bed gradient — 5 geometry/lighting tests); emissive gem tint feeds the bloom. Honest scope note: this is 2D done well, not photorealism — a 3D/PBR look would be a renderer rewrite. *Verified: flawless runs both styles; screenshots; 8-bit unchanged.*

- [x] G11 **Depth-view geometry fix + stage polish.** User screenshot: notes ran beside the lines — the guides used a DIFFERENT straight line than the note path; now they are its exact extension (`depth::extend_below`, collinearity-pinned). Polish: flattened receptor rings, glowing hit line, per-gem halos, corner vignette, distance-faded fret lines. *Verified: identical score again (23640), screenshot shows notes ON strings.*
- [x] G10 **Depth view (vanishing-point highway).** `depth::project` (pure, 3 tests: identity at the hit line, monotonic climb/shrink, lane convergence), per-frame projected notes/fret lines/sustains, leaning lane guides, trapezoid bed as a real 2D mesh. Both views coexist as a settings row. *Verified: IDENTICAL autopilot score flat vs depth (23640) — presentation only; screenshots; 8-bit untouched.*

- [x] G12 **Window-independent scaling.** Ortho projection `AutoMin{1280,720}` + `UiScale` synced to window height (0.6..2.5) + `BEATBYTE_WINDOW=WxH` for pinned sizes. *Verified: flawless runs with full stage/HUD at 800x500 and 1920x1200 (screenshots).*

- [x] G13 **Guitar Hero X-plorer support + controller tester.** gilrs cannot see the guitar (Xbox-360 vendor protocol, no macOS driver — verified empirically); own libusb reader thread (rusb vendored) decodes the documented 20-byte reports and injects RawGamepad events, making it a first-class Bevy gamepad matching the default bindings exactly. Controls screen: connected-device line + five live fret lamps through the real InputMap. *Verified: real guitar detected and claimed; USER pressed frets and the lamps lit (screenshot: red+yellow held, both lamps on — "perfekt!!! das funktioniert schon mal!!!"); 2 decode tests. Still open informally: a full song played on the guitar + C4's mid-song unplug.*

- [x] G14 **Strum UX after the guitar session.** Field finds: solo routed ONLY the keyboard (guitar played into the void — receptors dark, no hits) → solo hears all devices; Space strums on keyboard (Hype → Enter, user map migrated); a rate-limited "STRUM!" coach appears when a held-fret note dies with tap off. Key-injector made tap-aware (never strums when tap hits already). *Verified: tap-off + Space-strum flawless twice via real keys; tap-off no-strum = 117 misses; tap-on regression flawless.*
- [x] G15 **Mouse-driven menus + honest tagline.** User: no INPUT TEST entry visible (root cause: `open` foregrounds the stale running instance — killed + relaunched), mouse control requested, "8bit game" subtitle outdated. Menus now fully mouse-operable (hover select, click activate/adjust, wheel scroll/step, right-click back everywhere incl. key-capture cancel); tagline "five lanes. your music."; README rewritten for both looks. *Verified: gate + interactive build.*
- [x] G16 **Depth-view sustain fix + docs sprint.** User screenshot: sustain tails stood vertical while the lane leaned (child sprite kept flat geometry) — tails now connect gem to the projected far-end point on the exact note path (`depth::point` + `align_tail`, collinearity-pinned). Docs: README rebuilt (60+ badges, guitar support matrix, import pipeline walkthrough, donate/review), ~17 new unit tests incl. real-file decode fixtures for m4a/mp3/ogg/flac. *Verified: gate + depth-view screenshots + mutation checks on new pins.*
- [x] G17 **Live mute toggle.** User: "bei testläufen toggle button für (un)mute" — `Muted` resource + always-present clickable corner badge + `M` key (editor excluded: metronome owns M); music thread scaled, SFX via `GlobalVolume`; env var only seeds the start state, every later volume write goes through `factor()`. *Verified: gate, badge screenshot, muted autopilot still silent.*
- [x] G18 **Guitar-Hero-style conversion (melody charting + master derivation).** User: m4a conversion must capture guitar tones + held notes perfectly, research GH and adapt. Research: official charts are HAND-authored against stems; adaptable = the conventions (contour lanes, true-length sustains with tempo-scaled trailing gaps [CSC: 1/32|1/24|1/16 whole note], lead owns the highway). Implementation: melody stage in beatbyte-audio (STFT 2048 → HPSS → register-weighted semitone salience → DP contour → segmentation with sustained-ratio + loneliness guards), `SongAnalysis.melody` (additive, serde-default), generator rebuilt master-first (one 5-lane master, difficulties = thinned/remapped derivations, subsets pinned). *Verified: 5 melody unit tests incl. clicks-vs-tone separation; real track: coverage 22.5%→85.7%, held notes 8→147, vocal register tracked instead of bass; hard/expert sustains 2→34/24; all imported charts regenerated; autopilot flawless.* Follow-up: per-difficulty tuning session by ear (medium first — user decision), then derive tweaks.
- [x] G19 **Difficulty curve + import-path proof.** Measured the G18 curve on all five real imports and found a genuine defect: absolute strength floors made the easy→medium jump song-dependent (1.4x…3.6x; easy 0.42…1.40 notes/s). Fixed by density targeting (notes-per-beat targets, rank-based selection — a threshold bisection has a pathological case where NO threshold hits the target and it empties the chart) plus a reduction CHAIN (expert→hard→medium→easy), which makes nesting structural. *Verified: curve now exactly 2.0x easy→medium on every track, easy 0.66–0.79 n/s; nesting pinned with a fixture proven to break one-shot thinning; **real in-app drag-and-drop import (autopilot DROP mode) produces bit-identical charts to the CLI path** — user's question, and the right check: the CLI does not exercise import.rs.*
- [x] G20 **Chart-feel regression caught and reverted; stronger fret feedback.** The transcription rework (eval harness + tempo/pitch/onset/segmentation) measured better on eight synthetic scenes and played WORSE — user: "das ging gerade komplett in die falsche richtung!! ich fühle es nicht". Rolled his charts back ("jetzt macht das spiel wieder spaß und sinn!! mega!!!"), preserved that state three ways (tag `chart-feel-good-20260826`, `~/backups/beatbyte-charts-good-20260826/`, memory), and reverted main so NEW imports get it too; the work is parked on branch `transcription-v2`. Root cause measured afterwards: candidate ranking by loudness alone moves 59 % of note positions and ignores the beat structure a listener feels. Then: fret buttons swell/brighten/halo on press and punch on a hit, judgment-scaled. *Verified: freshly generated chart byte-identical to the one he enjoyed; autopilot flawless (624 perfect); screenshot shows the hit punch.*
- [x] G22 **One design across all menus.** User: unify the menus and settings and make them look modern. Audit found the screens shared only a font: seven title-to-content gaps (14–26 px), six row sizes (11/12/13/14 px) for the same job, 9 px *and* 10 px for hint text, selection signalled by letter colour alone, no frame anywhere, and hint lines in four separator styles. Two functional defects fell out of it: the controls screen read arrow keys directly (unreachable with a guitar) and ignored the mouse entirely, and the settings label column was padded to 16 characters while "TAP MODE (NO STRUM)" is 19, so that row's value hung outside the column. New `ui_kit` owns the scale, the spacing and the row states (Idle/Selected/Armed); all seven screens plus the pause overlay draw from it; two hard-coded copies of the lane palette removed. *Verified: gate (249 tests, +9); smoke + autopilot bundled/real-track/2-player/editor all PASS; new `BEATBYTE_SHOT_STATE` photographed all seven screens, and the measured selection fill (sRGB 99,84,35 → 69,58,32) fixed a tint that Bevy's linear-space alpha had made far heavier than intended.*
- [x] G23 **A venue behind the stage, and readouts in the corners.** User showed a Guitar Hero screenshot: "ich möchte dass hintergrund und zähler zu sehen sind wie im screenshot. gehe sorgsam vor." Two separate problems. (a) The 3D fretboard ran through a void, and the only thing outside it was the 2D sprite backdrop — which the stage camera renders at order −1, so those sprites were drawn IN FRONT of the board: confetti, not a backdrop. They are now skipped in 3D, and the stage builds a room instead (rear wall, side walls, truss with sweeping beams, speaker stacks, crowd ranks behind barriers), themed and kept outside the bed so it can never occlude a note. (b) The HUD was world-space text above the highway, which in 3D means above the vanishing point — the numbers floated in the middle of the screen. Solo now uses framed corner plates (score/multiplier/combo left, hype meter right); multiplayer keeps per-highway blocks, because four necks leave no free corners. Three self-corrections along the way, each caught by looking at the result: the first venue had cones washing red haze across the fretboard and speaker stacks looming at eye level; the rear wall was lit to fretboard brightness and read as a blank screen; the crowd was loose spheres that read as rubble until they were given barriers to stand behind. *Verified: gate + 271 tests + rustdoc clean; autopilot PASSED solo (624/624), in the depth view, and 2-player; frame time median 16.6 ms / 99th 19 ms (vsync-bound, venue costs nothing measurable); depth view and multiplayer screenshotted to confirm neither regressed.*

- [x] G24 **Sustain hold feedback on the 3D stage.** User: "wenn lang gezogene note getroffen wird, soll der treffer solange animiert werden wie lange die taste gedrückt bleibt." Root cause was not a missing animation but a despawn: `apply_note_events` removed every entity of a hit note, and the sustain tube is one of them — the part still to be played disappeared at the strike, so there was nothing left to animate. The depth view already did this correctly (`animate_sustains`), and `TrackSession::active_sustain` already existed with a doc comment saying presentation uses it for hold feedback; the 3D view simply never asked. Now: tails survive the strike and are consumed from the hit line by `consume_sustains`, the receptor's strike is kept alive and breathing while the hold runs, and the burst re-blooms instead of decaying. New pure `sustain_tail_span` carries the half-length arithmetic. Also replaced `move_notes`' `scale.y > 1.5` guess for "is this a tube" with the real component — the heuristic held only at full length, so a partly eaten tail would have jumped half its length when dropped. *Verified: gate + 276 tests (+5, mutation-checked: removing the hit-line clamp turns 4 red) + rustdoc clean; autopilot PASSED on three songs with identical scores (98/98, 282/282, 624/624); screenshot of a held sustain shows the shortened tail running into a blooming fret.*

- [x] G25 **Gameplay screen reads like the genre (plan U1-U6).** User: "ich möchte dass die aussieht wie bei guitar hero. erstelle dazu einen plan und arbeite ihn dann ab." Plan in `docs/ui/gameplay-look-plan.md`, grounded in the reference screenshot, a measured pass over what the game draws, and the genre's documented conventions — and explicit about the boundary: conventions (odometer, quartered meter, boxed multiplier, marked phrases) are adopted, specific art is not, and the rock meter stays out because it is a fail state rather than a look. Landed: energy phrases marked on the highway with a tinted band (the biggest gap — charts carried `phrases` and paid meter for them while nothing on screen said which notes they were), a fixed-width score counter with dim leading zeros, a boxed multiplier with streak beads, a meter showing its four quarters plus a ready line, a stage that washes to the energy tone while hype runs, a hit line that spans the neck, heavier bar lines, polished gems. Two defects surfaced by the work: the 2D hype overlay is a 900-px vertical band that in 3D missed the receding neck entirely and washed a wall forty units behind the vanishing point (now skipped in 3D, measured before and after), and my own stage tint eased once per SURFACE rather than per player, so the rate depended on entity count. Harness gained `gameplay-phrase` and `gameplay-hype` capture moments — the fixed 24-26 s window falls between phrases on every song in the library, so nothing automated had ever pictured either state. *Verified: gate + 279 tests + rustdoc clean; autopilot PASSED solo, in the depth view and 2-player; screenshots of phrase, hype and normal play in both views.*

- [x] G26 **Results screen rebuilt; unreadable titles fixed.** User: "optimiere die gestaltung der app weiter, gib dir maximal viel mühe." Swept every screen first. The results screen was by far the weakest — bare text floating in a void, using none of the shared kit, at the one moment that is supposed to be the payoff. Rebuilt on the kit: the song is the heading, a grade badge sits beside the score, accuracy gets a bar as well as a figure, and the judgment breakdown uses the SAME colours the popups used during the song. The song list showed two titles with empty boxes in them: measured against the font's cmap, Press Start 2P has 656 glyphs including `å`, `ü` and `ß`, but nothing from the fullwidth or mathematical blocks — exactly the characters a downloader substitutes for `|` and `/` in file names. `ui::font_safe` maps only those look-alikes, at DISPLAY time, so the chart keeps its true title and a script the font cannot draw is left alone rather than turned into question marks. Also fixed a panic in my own screenshot hook (it entered a screen at `Startup`, before boot had created the song library). *Verified: gate + 284 tests (+5) + rustdoc clean; autopilot PASSED; results screenshot. ⚠️ Two hours of black screenshots during this task were the LOCKED SCREEN, not the code — two plausible code hypotheses were built and disproven before checking `CGSSessionScreenIsLocked`; now recorded in CLAUDE.md and the harness doc.*

- [x] G27 **Playfield proportion and surface (plan V1-V3).** Second round on the same request. Round one fixed what the screen SAID; comparing against the reference showed what was left is what the screen IS. Measured: the solo neck filled 31 % of the frame (rails at 793 px of 2560) against the reference's ~50 %, the bed was a flat fill, and the gems were undersized in their own lanes. One `neck_spread` factor applied at `lane_x()` and the three `bed_width()` sites carries rails, lane strips, receptors, bursts, bar lines, phrase bands and notes together — solo only, read from the layout's player count so it cannot drift, and deliberately NOT done by moving the camera in, which magnifies the board but shortens how far up the neck you can read. Gems scale with it. The bed gained a generated grain (`board_shade`, a hash not a random number, so the board is identical every run) with its brightness band pinned, plus a test that it is not flat — a flat texture would pass the band test and be a silently missing feature. *Verified: 31 % → 45 %; gate + 288 tests (+4) + rustdoc clean; autopilot 98/98 perfect before AND after, and 2-player PASSED. ⚠️ Black screenshots struck again mid-task and I again blamed my own change before re-running — it was transient window occlusion; the same run repeated came out correct.*

- [x] G28 **Stage light, lane dividers, gem faces (plan W1-W3).** Third round. Measured the venue alone — outer thirds, above the neck, HUD excluded — at 0.13 brightness and 0.20 saturation: dark and very nearly grey, which is the whole remaining difference from the reference in one pair of numbers. Two coloured lamps from opposite sides (ranged so the room takes the light and the fretboard does not), a generated backdrop on the rear wall (the largest surface in the frame, previously an unlit slab), materials that accept light, four neutral lane dividers, and a generated radial face on the gems. *Verified: venue 0.13 → 0.20 brightness and 0.20 → 0.29 saturation with the board unchanged (0.250 → 0.262); gate + 293 tests (+9) + rustdoc clean; autopilot 98/98 perfect and 2-player PASSED.* ⚠️ W3 took three attempts and both failures are recorded: a `base_color_texture` alone made gems FLATTER (span 50 → 10) because their look is dominated by emissive, which that map does not touch; adding `emissive_texture` gave the shape (116) but dimmed distant notes, so the face got a floor — both constraints are now tests. ⚠️ Also solved for good: the recurring black screenshots are window occlusion by the terminal, and `screencapture -l<window-id>` captures regardless of stacking — matching the OWNING PROCESS, because a terminal in this directory is itself titled "BeatByte rhythm game".

- [x] G29 **The hit flame, and the right font measured at last (plan X1-X3).** Fourth round. A screenshot caught the exact frame a note landed and showed that nothing happened beyond a lit receptor and a flat ring — and the source comment justifying that claimed the genre's flame "spreads across the board rather than rising off it", which is backwards and was my own writing from memory rather than the reference. Added: a cone flame off the fret, white-hot at the strike and cooling to the lane colour, with a low flame held under a sustain; a crowd bobbing on the song's tempo map with per-head phase. Also fixed the diacritic boxes properly: the earlier cmap measurement was of Press Start 2P (656 glyphs, has `å`), but the game runs Bevy's built-in face whenever the round note style is on — the default — and that has **95 glyphs**, plain ASCII. Folding is now style-gated, because folding letters a font CAN draw is damage. *Verified: flame caught on camera after retuning from 5:1 (a laser) to 5:3; gate + 296 tests (+3) + rustdoc clean; autopilot 98/98 perfect, 2-player PASSED; frame time median 10.0 ms (100 fps), 99th 12.4 ms with flames and a moving crowd.* ⚠️ Also built a reliable capture path — `screencapture -l<window-id>`, matching the owning process — after occlusion produced black frames repeatedly.

- [x] G30 **Documentation enforced, versioning stated, v0.11.0 released.** User asked for documentation that stays true, and for the version, changelog and push to be guaranteed rather than remembered — which is one problem: a fact nobody checks decays. Evidence was in the repository: the README's test count had been corrected twice the previous day and was stale again by the evening (271 claimed vs 296 real; beatbyte-game 77 vs 105). `apps/beatbyte/tests/docs_stay_true.rs` now reads the repository as data and fails on disagreement — the per-crate test table and its total, the manifest version against the CHANGELOG and the internal pins, the checkable badges, the ADR index against the files on disk, every `BEATBYTE_*` switch against the harness reference, every repository link, the absence of the retired `[Unreleased]` section, and the figures `docs/gameplay/rules.md` quotes from `ScoreConfig`. Versioning rule agreed and written down: the patch number rises with every user-visible change in the same commit; a tag stays a separate act at milestones (the alternative, a release per change, would have meant 37 full pipeline runs for the work already sitting here). Also new: `docs/ui/3d-stage.md` for the largest module in the game, which was documented only inside itself. *Verified: gate + 313 tests (+17) + rustdoc clean; smoke, autopilot 98/98, 2-player and editor cycle all PASSED; three drift tests were red on real drift when first run, two more proven by mutation.* ⚠️ The counter's first version searched for the substring `#[test]` and counted its own two mentions of the attribute while explaining how it counts — it matches whole lines now.

- [ ] G21 **Optimization plan adopted.** `docs/optimization-plan.md` researched and written (game-feel literature + what Clone Hero actually ships). Proposals in order: P2 early/late feedback, P1 practice mode with speed + section loop, P4 song browser for a growing library, P3 rock meter with No Fail, P6 the open A1–A4 playtest pass, P5 charting revisited BY EAR against `chart-feel-good-20260826`, P7 the 3D renderer last. Explicit non-goals recorded: no online leaderboards, no neural separation, no further visual polish before P1–P3. P4 shipped as the library browser (v0.12.2–v0.12.4); P2 is G31; P1 shipped as G32+G33. *Next in order: P3 — pending the question whether a one-player audience wants failure at all.*

- [x] G31 **Early/late feedback (optimization plan P2)** *(v0.12.16)*. The judgment popup tags non-perfect hits with the side they landed on — `GREAT (EARLY)` / `GOOD (LATE)` — exactly where the information is actionable (a PERFECT needs no lecture, a miss has no side; negative offset = early, pinned). The solo results screen gains a TIMING row with the run's mean signed drift ("32 ms early" / "+18 ms late" / "on time" inside ±3 ms) fed by new `PlayerPerformance::mean_offset_ms()` (NaN/∞ guarded), plus a recalibration hint at ±15 ms — half the perfect window. *Verified: three pins, each mutation-checked (flipped tag sign, one-sided hint threshold, removed NaN guard — all seen to fail); results screenshot shows the TIMING row live; autopilot flawless.*

- [x] G32 **Practice speed (optimization plan P1, first half)** *(v0.12.17)*. Pause-menu SPEED row, 50–150 % in 5 % steps, applied live to audio and clock together; runs that used it are marked practice and stay out of scoreboard and telemetry, and the results screen says so. Two traps found and pinned: rodio's reported position lives in the OUTPUT timeline (wall-clock pace at any factor), so a mid-song speed change makes the naive `position × factor` wrong by a constant — the position map re-bases on every play/seek/speed change (pure `map_source_position`, the teleport counter-example in its test); and the clock re-anchors before changing its rate so song time is continuous (degenerate rates refused). *Verified: autopilot at 75 % and 125 % — measured slope 0.750x/1.251x, flawless runs, and live proof that a practice run writes NO score (mtime) and NO telemetry (file count); three pins mutation-checked; pause-menu screenshot shows the row.*

- [x] G33 **Section loop (optimization plan P1, second half)** *(v0.12.18)*. LOOP FROM/TO rows on the pause menu (RIGHT pins the paused moment, LEFT clears; a span under 1 s never arms — it would wrap faster than the lead-in plays); reaching the end jumps music, clock, sessions and in-flight note entities back to a 1.5 s lead-in, via the new `TrackSession::rewind_to` (events at/after the point reopen, session clock moves back so nothing phantom-misses, transients reset, phrases recomputed — three core pins). A seek travels to the music thread asynchronously, so reconciliation is held for 0.25 s after a wrap — reconciling against the not-yet-seeked device position would snap the clock straight back and wrap forever. *Verified: loop drill (`BEATBYTE_AUTOPILOT_LOOP`) live — two wraps at exactly the expected 11.5 s cadence, section notes reopened; drill seen to fail with the rewind removed; span pin seen to fail with the min-span dropped. With P4, P2 and P1 shipped, the plan's next in order is P3 (rock meter with No Fail) — deliberately not started without a fresh look at whether the one-player audience wants failure at all.*

- [x] G34 **Stage reads closer to the genre's classics; the red back-wall line is gone** *(v0.12.20)*. User commission (GH2-orientation): sourced the genre's visual language (WikiHero: gems = coloured circles with white centre, black ring on strum notes, ringless HOPOs; star-power notes are star-shaped; rock meter bottom-right; SP meter a tube above it; activation turns all notes toward the power colour) — inventory showed most of it already shipped (GRYBO lanes, white-centre/ring gem distinction, gem-as-button + rim in 3D, themed side rails, flames, crowd, multiplier dots). Landed: phrase notes wear a five-point star plate (pure `star_outline`, pinned + mutation-checked), note gems shift toward the energy colour while Hype runs (solo — shared lane materials would cross-tint in multiplayer), and the emissive accent band across the back wall (the reported red line) is removed. Perf: the hype-tint materials were re-uploaded EVERY frame (settled blend now writes nothing); measured before/after at both display states — the numbers are vsync-paced (8.3 ms @120 Hz, 16.7 ms @60 Hz medians) and ProMotion switches states mid-session, so like-for-like p99 comparison is confounded; the mechanism-level wins (no per-frame material uploads, one draw less) stand on inspection. Deliberately NOT done (asset rule / P3): GH2 highway artwork, fonts, rock meter + SP tube (mechanics first), exact cyan (house hue = Hype purple).

- [x] G35 **The venue becomes a concert** *(v0.12.20–v0.12.27, user-driven visual programme)*. Across seven passes: the reported red back-wall line removed; star phrase notes + Hype note tint (genre-sourced); HUD as an instrument panel (glowing streak bulbs, popping counter, half-circle Hype gauge with the activation tick straight up); beat-pulsing LED wall, then with cabinet + dot-matrix pixel structure; light rig rebuilt as moving-head fixtures (housing, lens, additive double-mantle beams with procedural falloff, swing around the hanger) with lens halos and floor pools sharing ONE pinned beam angle; near-black PA cabinets with procedural driver fronts breathing on the beat; and the realism pass from `docs/ui/stage-realism-plan.md` — club darkness, silhouette crowd, haze sheets, complementary backline, lattice trusses, stage riser. Every pass verified by engine screenshots with luma checks in the user's own settings, pins mutation-checked, autopilot + pause drill green throughout. Ongoing by-eye tuning belongs to the user.

- [x] G36 **The HUD plates become instruments** *(v0.12.28)*. Second HUD pass on commission ("optimiere das hud nochmal deutlich mehr"): corner plates re-skinned as brushed dark metal (top-edge light catch, corner rivets, vignette — `plate_shading`, pinned), the score digits in a recessed scanline well (`well_shading`, pinned), the gauge band gains a sweep gradient with a brighter READY zone past the activation tick, the needle a counterweight tail past the hub (custom sprite anchor). Motion, all transforms + sprite tints: multiplier pops on every change, the next streak bulb carries a faint ember, the gauge breathes toward the Hype tone when it can fire (`ready_glow`, pinned + mutation-checked) and blazes white-hot while Hype runs. *Verified: engine shots in the user's settings (plates/rivets/gradient/counterweight/blaze all visible in late + hype moments), 192 game-crate tests, all new pins seen failing under mutation, pause drill + full gate green.*

- [ ] G37 **UX/input/menu system programme** *(commissioned 2026-09-01;
  inventory + phases in `docs/ui/input-ux-plan.md`)*. Shipped:
  **phase 1** *(v0.12.29)* — remappable `UiAction` navigation table
  (WASD + Space + Tab, Enter/Escape hard fallbacks, typing mode for
  the browser search, conflict confirmation on rebind, scrolling
  controls list); **phase 2** *(v0.12.30)* — `UiSound` event system
  (Navigate/Confirm/Back/Error/Toggle/Slider, two new voices, every
  device sounds alike); **phase 3** *(v0.12.31)* — device-aware
  prompts (`ActiveDevice` + `DeviceHint`, every footer swaps wording
  live; pad players can finally leave the results screen). Open:
  accessibility completion (4), video-offset calibration (5),
  renderer-boundary ADR + 2D-path prune (6).

### F — Release engineering to 1.0

- [x] F0 **v0.9.0 — content, accessibility, editor v2.** Amendment (2026-08-24): every unblocked B/C/D/E task landed while the A tasks wait on human playtesting — that body of work (second song, song-selector harness, import guide + scan fix, format truth, forward-compat pins, Linux/Windows CI smokes, Stage Motion, colorblind lane shapes, HOPO visibility, editor move/bulk/audition, harness integrity) ships now instead of idling behind A2. *SHIPPED 2026-08-24: all 7 assets auto-attached via the recursive glob; aarch64 tarball smoke + flawless artifact autopilot from neutral CWD + DMG .app smoke; two release-run attempts (spurious hdiutil ENOSPC → retry mitigation).*
- [ ] F1 **The tuning release** (version picked at release time). Ship A1–A4 findings. dep: A2. *Verify: full release procedure incl. artifact smoke tests.*
- [ ] F2 **1.0 readiness checklist.** All A tasks done; C1–C3 done; chart format v1 documented as frozen (changes now = format v2); README/docs audit; all four platform artifacts verified at least once on their real OS. dep: F1, A*, C1–C3. *Verify: checklist appended here with evidence links.*
- [ ] F3 **v1.0.0.** Tag, release, drop `--prerelease`. dep: F2. *Verify: published, artifacts tested, CHANGELOG sectioned.*

---

### H — True 3D renderer (planned follow-up, user-approved direction)

The depth view is a projection inside the 2D pipeline. A real 3D
renderer (meshes, PBR materials, lights, camera tilt) is a separate
project. Milestones when picked up:

- [x] H1 3D/2D camera stack (Camera3d for the stage, Camera2d overlay for HUD/text) coexisting with both current views.
- [x] H2 Highway as a lit 3D plane + gem spheres as meshes with emissive PBR materials; judgment stays input-stamp-driven (identical-score proof again).
- [x] H2b Sustains as tube meshes + 3D receptor feedback (press sinks the fret into the neck, a hit makes it flare through the bloom pass). *Verified: identical judgment to 2D (624 perfect / 0 miss / 0 overstrum on the same chart).*
- [ ] H3 Sustains as tube meshes, 3D particles, depth-of-field/bloom tuning.
- [~] H4 Performance pass + packaging size check. Measured on this machine: the 3D stage holds a vsync-locked 60 fps during a full song with a 99th-percentile frame of 19.4 ms — no stalls, so it costs no notes. `BEATBYTE_FPS=1` now reports median and 99th-percentile frame times (an average would hide exactly the stutters that lose notes). Still open: a low-end GPU and the artifact size check.

## Phase 3 — Adaptive charting (DECIDED 2026-08-30, not started)

The architecture is [ADR-0011](decisions/ADR-0011-adaptive-charting.md);
the spec is [`adaptive-charting.md`](adaptive-charting.md). Tasks in
dependency order — each is independently shippable and none may start
before the one it depends on is checked:

- [x] **A1 — Telemetry recorder** *(v0.11.10)*. Per-note session log
  (judgment, signed offset, sustain endings) + session header with
  **chart content hash**, append-only JSONL beside `scores.json`,
  schema v1, autopilot sessions marked. Verified: 7 schema tests (two
  mutated), five autopilot runs each leaving a complete readable log
  (463/463 judged on a real import) with judgment unchanged — and the
  very first log already earned its keep by locating a one-off
  injector overstrum at line 1, during the count-in.
- [x] **A2 — `beatbyte-cli review`** *(v0.11.11)*. Per-section
  accuracy / timing / dropped-sustain / overstrum report over all
  sessions of one (song, difficulty, chart_hash); directives once the
  evidence threshold is met (default 3 sessions, all thresholds in
  one struct, `--min-sessions` exposed). Verified on real logs: four
  autopilot sessions of a real import produced the correct
  `trivially_mastered` directive with `--include-autopilot` and were
  correctly excluded without it. Two green-blind tests caught by
  mutation and rewritten (the mastery veto needed perfect accuracy
  with dropped holds to be exercised at all).
- [x] **A3 — Chart versioning** *(v0.11.12)*. Sibling versions with
  provenance (validated; excluded from the chart hash, with a
  golden-hash pin so no schema change can orphan telemetry as a side
  effect), pointer file the library respects with every failure mode
  falling back to the original, import never overwrites ANY existing
  chart (stronger than the telemetry-conditional wording — simpler
  and strictly safer). Verified live: a hand-made v2 of a real import
  played as 232 events with the pointer and 463 without, while the
  browser showed one entry throughout; scan and import pins mutated
  and seen to fail.
- [x] **A4 — `beatbyte-cli dossier`** *(v0.11.13)*. One
  self-contained briefing per song: active chart (pointer resolved),
  per-bar structure, melody with true held lengths, per-difficulty
  constraints from the generator's own profiles, open directives via
  the same code path as `review`, and the write instructions (next
  version name + parent hash). Workflow in
  `docs/workflow/design-session.md`, bound by a drift test that
  checks every `beatbyte-cli` invocation it teaches against the
  command enum (mutated with a fake subcommand and seen to fail).
  Verified on a real import: 166 bars, 485 melody notes, the open
  directive included, 352 KB.
- [x] **A5 — In-game feedback** *(v0.12.15)*. Results screen: 1–5
  rates the fun, LEFT/RIGHT records worse/better than the parent
  version (offered only when the played chart carries provenance);
  both append to the session log this run just wrote, `review`
  surfaces mean fun + the better/worse tally, and zero friction
  holds both ways (no log = no hint, ENTER always exits untouched).
  Verified: new `BEATBYTE_AUTOPILOT_RATE` drill presses the REAL
  keys and parses the log back — builtin (fun only) and Maria v4
  (fun + versus) both land; four pins mutation-checked, including
  the untagged-serde collision (a field named `o` on the new line
  kind gets mis-parsed as an overstrum — seen to fail).

Parked with reopen criteria (a real player population, or an explicit
request): population percentiles, automated rollout/A-B
infrastructure, ML preference and skill models, personalization.

## Library browser (DONE 2026-08-30, v0.12.2)

Sorting (S cycles standard/title/artist/genre/length/best), search
filter (`/`, folded matching), and per-row facts: genre, length,
notes, 1-5 density rating, personal best — all per selected
difficulty. Genre lives in the chart format (hash-neutral, validated),
filled from audio tags on import and via `beatbyte-cli set-genre`.

## Browser polish (DONE 2026-08-30, v0.12.4 — docs/ui/browser-polish-plan.md)

User verdict on the new browser was "better, still buggy"; the causes
were confirmed in code, not guessed, and all seven plan items landed:
P1 rows-only rebuild (screen spawns once, status/captions/details
update in place, rows respawn only on order/difficulty change — the
scroll survives typing), P2 hover selects only when the pointer moved,
P3 delete arms on the SONG not the view position, P4 filter typing
selects the first match, P5 the empty result says so, P6 sort mode +
direction persist in settings.json (the filter deliberately not), P7
sort/search actions blip like every other menu key. Backspace repeats
via the OS key-repeat stream. Verified: staged shots (sorted browser,
live search with first-match selection, empty-search hint via new
`BEATBYTE_SHOT_SEARCH`), autopilot on Maria (504/504 perfect), delete
harness on a disposable song, three new pins mutation-checked (one
fixture had to be sharpened — mutant and original coincided at
position 0).

## Difficulty redesign: hard + expert (DONE 2026-08-30, v0.12.5–v0.12.10 — docs/difficulty-redesign-plan.md)

User commission: only medium was a designed difficulty. Measured
diagnosis first (hard: 15/25 songs with zero HOPOs, characterless;
expert: median 106 machine-gun jacks per song, 55-event streams,
almost no chords, no escalation target), then P1–P7: master-level
lane flow turns fast repeated pitches into trills, streams get a
per-difficulty length budget, chords are a percentile of the song's
own accents, hard's HOPO gap matches its real densities, expert
escalates toward the master where the song rises, and
`beatbyte-cli redesign --all` rolled sibling versions over all 25
imported songs (easy+medium carried note-for-note; legacy layout
skipped; per-song revert = pointer; restorable copy in
`~/backups/beatbyte-charts-pre-hardexpert-20260830/`). Verified: 16
new pins, every one mutation-checked; post-rollout measurement
flags ZERO charts (jacks 0, bursts ≤ cap, HOPOs everywhere on hard,
floors respected); autopilot via the new
`BEATBYTE_AUTOPILOT_DIFFICULTY` switch — builtin expert 309/309,
Maria hard 870/870, Immer expert 1064/1064, Lille Vals expert
1963/1963, all perfect. **Open: the by-ear gate** — the user plays
hard/expert and keeps or reverts pointers; Maria's pending v3
listening A/B stayed open (her redesign is v4 on parent v2).

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
