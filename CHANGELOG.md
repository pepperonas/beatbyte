# Changelog

All notable changes to BeatByte are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.1] - 2026-08-23

### Fixed

- **All text was invisible when the game was launched directly from
  `target/` or any layout Bevy's default asset resolution misses**: the
  pixel font failed to load (a failed asset never retries in Bevy), so
  HUD, judgment popups, count-in, menus and results rendered no glyphs.
  The game now resolves its asset root explicitly across every
  supported layout — portable (assets next to the executable), macOS
  .app bundle (`../Resources`), development (current directory), and
  the workspace `target/` tree.
- Autopilot screenshots taken on state entry no longer capture the
  transition fade (short settle delay before each capture).
- Release CI: the arm64 macOS runner ran out of disk while creating
  the DMG — the packaging script now reclaims the build tree (CI only)
  before `hdiutil` runs.

### Changed

- README media refreshed: gameplay and results screenshots with the
  full HUD/text actually visible.

## [0.8.0] - 2026-08-23

**The polish milestone** (Milestone 13).

### Added

- **Screen-transition fades**: every state change fades in over a
  quarter second instead of cutting.
- **Count-in**: songs start with a two-second pre-roll — the first
  notes scroll in over a 3-2-1 banner and the music starts exactly at
  zero. No song opens with a wall anymore.

### Changed

- README and docs brought up to the finished-milestone state.

## [0.7.0] - 2026-08-23

**The editor and packaging milestones** (Milestones 11 + 12).

### Added

- **Chart editor** — engine-free core in `beatbyte-editor`
  (invertible edit operations, an `EditorSession` with undo/redo and
  dirtiness tracking, all unit-tested) plus an in-game screen: open
  any file-based song from the browser with `E`, step the beat grid
  (1/1 · 1/2 · 1/4), place/remove notes per lane, toggle HOPOs,
  preview the audio from the cursor, undo/redo, and save — saving is
  gated on chart validation. Leaving with unsaved changes asks twice.
- **macOS .app + DMG**: proper bundle with a procedurally generated
  pixel icon (hand-rolled PNG encoder, stdlib only), Info.plist,
  ad-hoc signature; assets resolve from `Contents/Resources`.
- **Linux AppImage** with desktop entry and icon.
- Release CI now attaches DMG and AppImage next to the portable
  tar.gz/zip archives.
- Songs are also scanned from the user data directory
  (`…/beatbyte/songs`), so installed builds have a place for music.
- Autopilot editor mode (`BEATBYTE_AUTOPILOT_EDIT=1`): opens the
  editor on a real file, edits, undoes, redoes, saves and verifies
  the file on disk.

## [0.6.0] - 2026-08-23

**The themes milestone** (Milestone 10): six original stages, all
data (ADR-0008).

### Added

- **Six stage themes**: Garage (warm amber, twinkling starfield),
  Punk (hot pink, pogo crowd), Metal (steel, rising embers), Stadium
  (deep blue, sweeping spotlights), Psychedelic (violet, drifting
  bubbles), Cyber (neon, rolling synth grid).
- **Procedural backdrops** — engine-drawn pixel sprites animated by
  one system, beat-aware where it reads well; no textures, no
  assets.
- Theme selection in settings: a fixed stage or **AUTO**, which picks
  deterministically per song title (same song, same stage).
- Highway beds, lane guides, receptors, notes, sustain tails and hit
  particles all take the active theme's palette; the beat pulse
  strength is per theme. Judgment colors stay constant — readability
  first.

## [0.5.0] - 2026-08-23

**The multiplayer milestone** (Milestone 9): 2–4 players, one machine.

### Added

- **Join screen** (main menu → Multiplayer): the keyboard and every
  connected gamepad claim player slots by pressing fret 1; mode
  toggle between **Versus** and **Co-op**; player accent colors.
- **Split highways**: the layout scales for 1–4 players (lane
  spacing, note sizes and receptor sizes shrink as highways
  multiply); every player gets their own receptors, notes, sustain
  tails and lane guides.
- **Per-device input routing**: a keyboard player only hears the
  keyboard, a pad player only their own pad — through the same
  bindings table.
- **Per-player everything**: world-space HUD blocks (score, combo,
  multiplier, Hype bar) above each highway, judgment popups, hit
  particles, Hype overlays and sustain sparks all follow their
  player. The stage pulse hardens when *anyone* is in Hype.
- **Multiplayer results**: ranked list for Versus, band total plus
  breakdown for Co-op; solo results (grade slam, count-up,
  NEW RECORD) unchanged. High scores stay solo-only by design.
- Autopilot can now simulate N players
  (`BEATBYTE_AUTOPILOT_PLAYERS=2..4`) and requires a flawless run
  from every one of them.

### Changed

- Sessions, spawn cursors and feedback messages are fully per-player;
  the gameplay systems iterate players instead of assuming one
  (ADR-0002's "players are data" delivered end to end).

## [0.4.0] - 2026-08-23

**The controllers milestone** (Milestone 8).

### Added

- **Input abstraction**: physical input → binding → game action.
  Bindings are data, persisted with the settings; gameplay only ever
  sees actions (ADR-0004's input model).
- **Gamepad support** on every connected pad: frets on the face
  buttons + left shoulder (the common guitar-controller layout —
  green=South … orange=LB), strum on the D-pad, Hype on Select/RT,
  pause on Start. Guitar-style controllers that enumerate as gamepads
  work out of the box.
- **Menus speak gamepad**: D-pad navigation, South=confirm,
  East=back on all menu screens.
- **Remapping screen** (Settings → Controls): every action listed
  with its bindings; Enter captures the next key or button (stealing
  it from whichever action held it), Backspace restores a row's
  defaults. Persisted with the settings; invalid entries in edited
  config files are dropped safely.

### Changed

- Bevy's `serialize` feature is enabled so input types persist
  naturally.

## [0.3.0] - 2026-08-23

**The UI milestone** (Milestone 7): BeatByte grows its screens — and
its voice.

### Added

- **Pixel font identity**: Press Start 2P (OFL 1.1, license bundled)
  across every screen — boot, menus, HUD, popups, results.
- **Main menu**: Play / Settings / Calibration / Quit with keyboard
  navigation.
- **Song browser**: the bundled demo plus every valid chart found in
  `songs/` (invalid charts are skipped with a log line, never a
  crash). Difficulty stepping is constrained to what each chart
  offers; the details line shows BPM, duration and your best score.
  File songs stream from disk; the demo plays from memory.
- **Settings screen**: music/SFX volume, scroll speed, latency
  offset, particles/shake/beat-pulse toggles, fullscreen — changes
  apply immediately and persist to the platform config directory.
  Corrupt settings files fall back to defaults instead of crashing.
- **Latency calibration**: tap along with a click track, the median
  offset (8+ taps) becomes your setting. Gameplay subtracts the
  offset from input timestamps (ADR-0004's calibration model).
- **High scores**: best score/accuracy/streak per song + difficulty,
  saved to the platform data directory; the results screen celebrates
  new records, the browser shows your best.
- Scroll speed and latency offset now actually drive gameplay
  rendering and input timestamping.

### Fixed

- A startup ordering crash (system reading a resource before its
  startup command applied) — caught by the autopilot harness; shared
  UI resources are now inserted at plugin build time.
- Strict-docs CI failure (private intra-doc link).

## [0.2.0] - 2026-08-23

**The game feel milestone** (Milestone 6): BeatByte stops feeling like
a tech demo.

### Added

- **Session feedback bus**: judgment events are broadcast as engine
  messages once per frame; note visuals, particles, sounds and popups
  are independent subscribers (multiplayer-ready fan-out).
- **Pixel-confetti hit particles**: bursts sized by judgment (Perfect
  adds white sparks), sustain hold sparks at the receptor, a Hype
  activation salvo across all lanes — deterministic seeding, hard
  particle cap, zero allocations in steady state beyond spawns.
- **Trauma-based screen shake** on misses, overstrums and Hype
  activation (decaying, squared response — subtle by design).
- **The stage breathes**: highway brightness pulses on the beat grid
  (stronger under Hype), and a translucent Hype overlay glows when the
  meter is ready and breathes while it burns.
- **Combo-break flash**: a brief red wash so a dropped streak is felt
  without reading the HUD.
- **Procedural sound effects** — synthesized at startup, no audio
  binaries: menu move/confirm blips, a dry miss thud (rate-limited),
  a rising Hype sweep. Note hits stay deliberately silent: the music
  is the hit sound.
- **Menu & results juice**: the title breathes, the grade letter
  slams in with overshoot, the score counts itself up.
- `EffectSettings` resource (particles / shake / beat pulse toggles)
  ready for the accessibility settings screen.

## [0.1.0] - 2026-08-23

**BeatByte is playable.** First playable prototype (Milestone 5).

### Added

- **The gameplay screen**: five-lane highway with receptors, falling
  notes (chords, HOPO markers, sustain tails), all note positions
  derived from the song clock every frame — never from frame counts.
- **Keyboard play**: frets `A S D F G`, strum `↑`/`↓`, Hype `Space`,
  pause `Esc`. Inputs are timestamped with song time and fed to the
  deterministic judgment engine from Milestone 2.
- **Live HUD**: score, combo, multiplier (with Hype state), accuracy,
  Hype meter with activation hint; judgment popups and receptor
  flashes on every hit.
- **Screen flow** as explicit states: boot (background demo build) →
  main menu (difficulty select) → gameplay (with pause sub-state) →
  results (grade, score, judgment breakdown).
- **Players are entities**: each carries its own session component —
  the multiplayer-ready shape from day one.
- **Autopilot mode** (`BEATBYTE_AUTOPILOT=1`): the game plays itself
  perfectly through the real screens and input path, then exits with
  success only on a flawless run — the end-to-end validation harness
  used before every release.
- The music thread bridge: song clock reconciliation against the
  audio device every frame; missing audio devices degrade gracefully.

### Changed

- Dev profile builds `beatbyte-audio` at full optimization (demo
  synthesis + analysis: ~30 s → ~3 s at boot).

## [0.0.3] - 2026-08-23

### Added

- **Audio infrastructure** (`beatbyte-audio`):
  - Decoding of OGG/WAV/FLAC/MP3 into analyzable mono buffers with
    untrusted-input caps, plus a half-band FIR downsampler.
  - The `SongClock`: an anchored, monotonic, fully unit-testable song
    timeline with snap/slew reconciliation against the audio device
    (ADR-0005).
  - Music playback on a dedicated thread (rodio) behind a `Send`
    handle: play file/buffer, pause, seek, volume, atomic position;
    the game runs silently instead of crashing when no output exists.
  - The analysis pipeline: spectral-flux onset detection (with
    per-onset strength and brightness), autocorrelation tempo
    estimation with octave prior and sub-BPM interpolation, beat-grid
    phase fitting, RMS energy envelope — all pure and tested against
    synthesized ground truth.
  - Deterministic signal synthesis (`synth`) and the original bundled
    demo track "Circuit Breaker" by The Null Pointers, rendered
    entirely by code (ADR-0006) — no audio binaries in the repository.
- **Automatic chart generation** (`beatbyte-chart::generate`):
  difficulty-profile-driven and deterministic — grid quantization with
  raw-onset fallback, strength filtering, density limits,
  brightness-driven lane assignment with jump limiting, chords on
  strong hits, auto-HOPO for fast runs, energy-aware sustains, phrase
  placement, loudest-window preview selection.
- **Real CLI** (`beatbyte-cli`): `analyze`, `generate`, `validate`,
  `inspect` now do real work, plus `demo` (renders the demo song and
  charts it through the actual pipeline). Proper exit codes.
- Analysis types (`SongAnalysis`, `Onset`) in `beatbyte-core::music`
  as the shared vocabulary between analysis and generation.
- Documentation: ADR-0005 (audio architecture), ADR-0006 (synthesized
  demo content), `docs/audio/analysis.md` including honest known
  limitations.

## [0.0.2] - 2026-08-23

### Added

- **Core domain model** (`beatbyte-core`), engine-free and fully
  unit-tested:
  - Lanes and lane sets (chords, held frets) with bitmask semantics.
  - Tempo maps (beats ↔ seconds, tempo changes ready), configurable
    symmetric hit windows and Perfect/Great/Good/Miss judgment.
  - Note events (taps, chords, sustains, HOPOs), special phrases and
    validated playable tracks.
  - Data-driven scoring: judgment-tiered points, streak multiplier,
    per-beat sustain scoring, weighted accuracy, and the Hype special
    meter (phrase gains, activation, beat-based drain).
  - The deterministic gameplay session (`TrackSession`): strum matching
    with anchoring, note skipping, overstrums, hammer-ons and pull-offs,
    sustain lifecycles, phrase tracking — identical inputs always
    produce identical outcomes.
- **Chart format v1** (`beatbyte-chart`): versioned JSON schema,
  tolerant reader with strict all-issues validation (version gate,
  numeric ranges, duplicate notes, phrase overlaps, note-count and
  file-size caps), path-traversal-safe audio resolution, chord grouping
  into gameplay events, and load/save helpers.
- Chart format specification (`docs/chart-format/`), gameplay rules
  documentation, ADR-0003 (chart format) and ADR-0004 (gameplay timing).

## [0.0.1] - 2026-08-23

### Added

- Cargo workspace with the full crate architecture:
  `beatbyte-core`, `beatbyte-chart`, `beatbyte-audio`, `beatbyte-game`,
  `beatbyte-cli`, `beatbyte-editor` and the `beatbyte` application.
- Minimal Bevy 0.19 application that opens the BeatByte window and shows
  the boot screen.
- Continuous integration (formatting, clippy, tests, multi-platform build).
- Release workflow scaffolding for macOS, Windows and Linux.
- Project documentation structure with the first Architecture Decision
  Records (Rust + Bevy, workspace layout).
- README, MIT license, contributing guide, code of conduct, security policy.

[Unreleased]: https://github.com/pepperonas/beatbyte/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/pepperonas/beatbyte/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pepperonas/beatbyte/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/pepperonas/beatbyte/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/pepperonas/beatbyte/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/pepperonas/beatbyte/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/pepperonas/beatbyte/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pepperonas/beatbyte/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/pepperonas/beatbyte/compare/v0.0.3...v0.1.0
[0.0.3]: https://github.com/pepperonas/beatbyte/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/pepperonas/beatbyte/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/pepperonas/beatbyte/releases/tag/v0.0.1
