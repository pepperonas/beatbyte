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
- [ ] G21 **Optimization plan adopted.** `docs/optimization-plan.md` researched and written (game-feel literature + what Clone Hero actually ships). Proposals in order: P2 early/late feedback, P1 practice mode with speed + section loop, P4 song browser for a growing library, P3 rock meter with No Fail, P6 the open A1–A4 playtest pass, P5 charting revisited BY EAR against `chart-feel-good-20260826`, P7 the 3D renderer last. Explicit non-goals recorded: no online leaderboards, no neural separation, no further visual polish before P1–P3. *Next: pick one and move it into this roadmap as a numbered task.*

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
- [ ] H4 Performance pass (low-end GPUs) + packaging size check.

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
