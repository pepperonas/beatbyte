# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

**BeatByte** — an original 8-bit five-lane rhythm game in **Rust + Bevy
0.19** (repo `pepperonas/beatbyte`, MIT, public). Cargo workspace:
`crates/beatbyte-{core,chart,audio,editor,cli,game}` + `apps/beatbyte`.
All game logic lives in the crates; `apps/beatbyte` is a thin launcher.
UI language is English; the game is fully keyboard/gamepad driven.

## Quality gate (before EVERY commit)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace          # 152 tests
cargo check --workspace
```

CI installs the **latest stable** — keep the local toolchain current
(`rustup update stable`); a stale local clippy passes locally and fails
CI (happened twice).

## Validation harnesses (use them, they catch real bugs)

```bash
BEATBYTE_SMOKE_TEST=1 cargo run -p beatbyte     # boots to menu, exits 0
BEATBYTE_AUTOPILOT=1 cargo run -p beatbyte      # plays the demo song PERFECTLY
BEATBYTE_AUTOPILOT=1 BEATBYTE_PLAYERS=2 ...     # multiplayer variant
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_EDIT=1  # editor add/undo/redo/save cycle
BEATBYTE_SHOT_DIR=<dir>                          # + screenshots along the way
```

Autopilot exits non-zero on ANY miss/overstrum — judgment is
input-stamp-driven, so it is frame-rate independent. Its input feed
must stay `.before(advance_sessions)`. Run autopilot before every
release.

## Architecture rules that must not erode

- **Timing is input-stamp-driven, never frame-driven.** `TrackSession`
  (beatbyte-core) judges from stamped input times against the
  `SongClock` (beatbyte-audio: anchored monotonic time, snap ≥30 ms /
  slew 10% against the device position). Music runs on a dedicated
  thread behind `MusicHandle` (mpsc + atomics).
- **Charts are untrusted input**: validation caps (BPM 20–400, 32 MB),
  path traversal + Windows-drive-`:` rejection in beatbyte-chart. Never
  weaken these.
- **Players are entities** (`PlayerSession`/`PlayerIndex`/
  `PlayerDevice`); input routes by `DeviceId` (Keyboard vs Pad(Entity)).
- **Editor ops are invertible** (`EditOp::apply` returns the inverse) —
  undo/redo correctness depends on it.
- **No copyrighted assets, music, or trademarks.** Font: Press Start 2P
  (OFL, bundled). Demo song is synthesized at build time.

## Hard-won gotchas

- **Asset root**: Bevy's default exe-relative resolution misses the
  workspace `assets/` when running `target/debug/beatbyte` directly —
  and a **failed asset never retries**, so a missed font made ALL text
  silently invisible (v0.8.1 fix). `configure_asset_root()` in
  beatbyte-game/lib.rs resolves explicitly: exe dir → `../Resources`
  (macOS .app) → CWD → exe ancestors. Don't remove it; it runs before
  `App::new`, so its `info!` goes nowhere — debug with `eprintln!`.
- Bevy 0.19 renames: `MessageReader/Writer` (not Events),
  `add_message`, `FontSize::Px`, `Camera2d`, `AudioPlayer`. Plugin
  tuples cap at 15 (split `add_plugins`). KeyCode/GamepadButton serde
  needs bevy's `serialize` feature.
- **Never edit files while a cargo build is in flight** (poisons the
  cache); don't casually flip bevy features (full rebuild, huge target/).
- macOS: `timeout` doesn't exist; screenshots of an **occluded window
  are black** (first-seconds shots often black — window still coming
  up); `grep -c` exits 1 on zero matches and breaks `&&` chains.
- State-entry screenshots must wait out the 0.25 s transition fade
  (autopilot uses a 0.6 s settle delay).

## Release

Bump `version` + internal dep versions in the workspace `Cargo.toml`,
CHANGELOG entry (Keep a Changelog), commit, `git tag -a vX.Y.Z`, push
with `--tags`. The tag triggers `.github/workflows/release.yml`
(~25 min: linux/macos/windows + DMG + AppImage) and creates a **draft**
release — download an artifact, smoke-test it, then
`gh release edit vX.Y.Z --draft=false`. Local packaging:
`packaging/macos.sh` (.app + DMG), `packaging/appimage.sh`.
