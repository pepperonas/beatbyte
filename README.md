<div align="center">

# 🎸 BeatByte

**An original five-lane rhythm game. Your music. Pixel-art or polished — your call.**

### Status

[![CI](https://github.com/pepperonas/beatbyte/actions/workflows/ci.yml/badge.svg)](https://github.com/pepperonas/beatbyte/actions/workflows/ci.yml)
[![Release Build](https://github.com/pepperonas/beatbyte/actions/workflows/release.yml/badge.svg)](https://github.com/pepperonas/beatbyte/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/pepperonas/beatbyte?include_prereleases&sort=semver)](https://github.com/pepperonas/beatbyte/releases)
[![Release Date](https://img.shields.io/github/release-date-pre/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/releases)
[![Downloads](https://img.shields.io/github/downloads/pepperonas/beatbyte/total)](https://github.com/pepperonas/beatbyte/releases)
[![Last Commit](https://img.shields.io/github/last-commit/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/commits/main)
[![Commit Activity](https://img.shields.io/github/commit-activity/m/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/commits/main)
[![Issues](https://img.shields.io/github/issues/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/issues)
[![Closed Issues](https://img.shields.io/github/issues-closed/pepperonas/beatbyte?color=blue)](https://github.com/pepperonas/beatbyte/issues?q=is%3Aissue+is%3Aclosed)
[![Pull Requests](https://img.shields.io/github/issues-pr/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/pulls)
[![Stars](https://img.shields.io/github/stars/pepperonas/beatbyte?style=flat)](https://github.com/pepperonas/beatbyte/stargazers)
[![Forks](https://img.shields.io/github/forks/pepperonas/beatbyte?style=flat)](https://github.com/pepperonas/beatbyte/network/members)
[![Watchers](https://img.shields.io/github/watchers/pepperonas/beatbyte?style=flat)](https://github.com/pepperonas/beatbyte/watchers)
[![Contributors](https://img.shields.io/github/contributors/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte/graphs/contributors)
[![Repo Size](https://img.shields.io/github/repo-size/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte)
[![Code Size](https://img.shields.io/github/languages/code-size/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte)
[![Top Language](https://img.shields.io/github/languages/top/pepperonas/beatbyte)](https://github.com/pepperonas/beatbyte)
[![Maintained](https://img.shields.io/badge/maintained-yes-brightgreen)](https://github.com/pepperonas/beatbyte/commits/main)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](CONTRIBUTING.md)

### Quality

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![SemVer](https://img.shields.io/badge/versioning-SemVer-blue)](CHANGELOG.md)
[![Keep a Changelog](https://img.shields.io/badge/changelog-Keep%20a%20Changelog-E05735)](CHANGELOG.md)
[![Conventional Commits](https://img.shields.io/badge/commits-Conventional-FE5196)](https://www.conventionalcommits.org/)
[![Tests](https://img.shields.io/badge/tests-219%20passing-brightgreen)](#testing)
[![Clippy](https://img.shields.io/badge/clippy-%E2%80%91D%20warnings-brightgreen?logo=rust)](Cargo.toml)
[![rustfmt](https://img.shields.io/badge/style-rustfmt-orange?logo=rust)](Cargo.toml)
[![Rustdoc](https://img.shields.io/badge/public%20API-documented-blue)](Cargo.toml)
[![Unsafe](https://img.shields.io/badge/unsafe-1%20audited%20block-yellow)](crates/beatbyte-game/src/lib.rs)
[![Deterministic](https://img.shields.io/badge/engine-deterministic-blueviolet)](#how-your-music-becomes-a-playable-track)
[![Autopilot](https://img.shields.io/badge/releases-autopilot%20verified-success)](#testing)
[![ADRs](https://img.shields.io/badge/decisions-ADRs-lightgrey)](docs/decisions/)

### Tech

[![Rust](https://img.shields.io/badge/Rust-1.95%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-orange?logo=rust)](Cargo.toml)
[![Bevy](https://img.shields.io/badge/Bevy-0.19-blueviolet)](https://bevy.org/)
[![Audio](https://img.shields.io/badge/audio-rodio%20%2B%20Symphonia-9cf)](crates/beatbyte-audio)
[![USB](https://img.shields.io/badge/guitar%20driver-libusb%20(rusb)-9cf)](crates/beatbyte-game/src/xplorer.rs)
[![Workspace](https://img.shields.io/badge/workspace-7%20crates-informational)](#development)
[![Toolchain](https://img.shields.io/badge/build%20deps-Rust%20only-informational)](#building-from-source)
[![Made with Rust](https://img.shields.io/badge/made%20with-%F0%9F%A6%80%20Rust-red)](https://www.rust-lang.org/)

### Platforms

[![macOS](https://img.shields.io/badge/macOS-DMG%20%2B%20portable-black?logo=apple)](https://github.com/pepperonas/beatbyte/releases)
[![Windows](https://img.shields.io/badge/Windows-portable%20zip-0078D6)](https://github.com/pepperonas/beatbyte/releases)
[![Linux](https://img.shields.io/badge/Linux-AppImage%20%2B%20portable-FCC624?logo=linux&logoColor=black)](https://github.com/pepperonas/beatbyte/releases)
[![Apple Silicon](https://img.shields.io/badge/arch-arm64-lightgrey?logo=apple)](https://github.com/pepperonas/beatbyte/releases)
[![Intel](https://img.shields.io/badge/arch-x86__64-lightgrey)](https://github.com/pepperonas/beatbyte/releases)

### Gameplay

[![Players](https://img.shields.io/badge/players-1%E2%80%934%20local-ff69b4)](#features)
[![Lanes](https://img.shields.io/badge/lanes-5-blue)](#features)
[![Difficulties](https://img.shields.io/badge/difficulties-4-blue)](#features)
[![Audio Formats](https://img.shields.io/badge/your%20music-WAV%20%C2%B7%20OGG%20%C2%B7%20FLAC%20%C2%B7%20MP3%20%C2%B7%20M4A-blue)](#how-your-music-becomes-a-playable-track)
[![Keyboard](https://img.shields.io/badge/input-keyboard-9cf)](#controls)
[![Gamepad](https://img.shields.io/badge/input-gamepad-9cf)](#controls)
[![Guitar](https://img.shields.io/badge/input-guitar%20controller-9cf)](#supported-guitars--controllers)
[![X-plorer](https://img.shields.io/badge/X%E2%80%91plorer-native%20driver-success)](#supported-guitars--controllers)
[![Calibration](https://img.shields.io/badge/latency-in%E2%80%91game%20calibration-blueviolet)](#controls)
[![Editor](https://img.shields.io/badge/chart%20editor-built%E2%80%91in-blueviolet)](#features)
[![Themes](https://img.shields.io/badge/stage%20themes-6-blueviolet)](#features)
[![Colorblind](https://img.shields.io/badge/colorblind-safe%20by%20default-brightgreen)](#features)
[![Offline](https://img.shields.io/badge/offline-no%20telemetry-brightgreen)](#legal)
[![DRM](https://img.shields.io/badge/DRM-free-brightgreen)](#legal)
[![Assets](https://img.shields.io/badge/assets-100%25%20original%20%2F%20CC0%20%2F%20OFL-brightgreen)](#legal)

### Support

[![Donate](https://img.shields.io/badge/Donate-PayPal-00457C?logo=paypal&logoColor=white)](https://www.paypal.com/donate/?business=martin.pfeffer%40celox.io)
[![Review](https://img.shields.io/badge/Review%20celox.io-on%20Google%20Maps%20%E2%AD%90-4285F4?logo=googlemaps&logoColor=white)](https://g.page/r/CXgdRV3QysvxEBM/review)

</div>

---

BeatByte is an original, open-source rhythm game in the classic five-lane guitar
tradition — rebuilt from scratch in **Rust** and **Bevy**. It ships two
looks: a crisp 8-bit / pixel-art identity, and a smooth high-res style
with round gems, a depth-view highway and bloom — switchable in-game.
Drop in your own music, let BeatByte analyze it
and generate a playable chart, then shred it solo or with up to four players on
one machine.


> **Status: all thirteen build milestones complete** — playable solo or
> with friends, themed, packaged for all three platforms. 0.x versions
> until the tuning settles; see the [roadmap](#roadmap).

<p align="center">
  <img src="docs/media/beatbyte-gameplay.png" alt="BeatByte gameplay: five-lane highway with falling notes, score, combo, multiplier and judgment popup" width="800"/>
</p>

*A flawless run in progress: x4 multiplier, 75-note combo, PERFECT
popup, hit particles on the triangle receptor — with the Metal theme's
ember backdrop. Every lane has its own shape (square, circle, diamond,
triangle, cross), so color is never the only signal.*

<p align="center">
  <img src="docs/media/beatbyte-round-style.png" alt="BeatByte round note style: glowing lanes with bloom, lit glossy gem spheres, shaded receptor rings" width="800"/>
</p>

*The same engine with the 8-bit mode switched off and the depth view
on: a vanishing-point highway, HDR bloom, lit glossy gems growing out
of the distance. Both views and both styles are switchable in the
settings — and the projection is presentation only (identical
autopilot scores prove it).*

<p align="center">
  <img src="docs/media/beatbyte-results.png" alt="BeatByte results screen: S rank, 100% accuracy, 117 perfect notes" width="800"/>
</p>

## Features

- 🎸 **Classic five-lane gameplay** — single notes, chords, sustains,
  hammer-ons/pull-offs, combos, multipliers and the Hype meter
- ♿ **Colorblind-safe by default** — every lane has a distinct gem
  shape on notes and receptors; a Stage Motion setting stills the
  backdrop for reduced motion
- 🎵 **Bring your own music** — WAV / OGG / FLAC / MP3 / M4A; drop generated
  charts into `songs/` and they appear in the browser
- 🤖 **Automatic chart generation** — BPM & onset analysis turns any song
  into a playable chart across four difficulties (playable, not perfect —
  the built-in editor is the correction pass)
- ✏️ **Chart editor** — beat grid, note placement, HOPOs, audio preview,
  undo/redo, validated saves
- 👾 **Two looks** — an 8-bit pixel identity (pixel font, six themed
  stages, procedural beat-reactive backdrops) and a smooth high-res
  style (round gems, perspective highway, bloom); every asset
  generated or openly licensed
- 🕹️ **Controllers** — keyboard, gamepads and guitar-style controllers,
  fully remappable in-game
- 👥 **Local multiplayer** — 2–4 players, versus and co-op, split highways
- ⏱️ **Serious timing** — deterministic judgment engine, reconciled song
  clock, configurable hit windows, in-game latency calibration
- 🏆 **High scores**, persistent settings, procedural SFX, two original
  synthesized tracks ("Circuit Breaker" at 128 BPM, "Solder Groove" at
  92) — and an autopilot that must play every build flawlessly before
  release
- 🖥️ **Cross-platform** — macOS (.dmg), Windows (portable), Linux
  (AppImage); WebAssembly on the horizon

## Supported Platforms

| Platform | Status |
|----------|--------|
| macOS (Apple Silicon & Intel) | ✅ primary target |
| Windows | ✅ primary target |
| Linux | ✅ primary target |
| Web (WASM) | 🔮 future |

## Supported Guitars & Controllers

BeatByte plays with a keyboard out of the box — but it is a guitar game
at heart, and it ships its own native guitar driver.

| Controller | Connection | Status |
|------------|------------|--------|
| **Guitar Hero X-plorer** (RedOctane, wired Xbox 360, USB `1430:4748`) | **built-in native driver** (userspace libusb — no kernel driver, no config) | ✅ **verified on real hardware** |
| Any controller your OS shows as a gamepad (Xbox, PlayStation, generic HID pads) | system gamepad support | ✅ works, fully remappable in-game |
| PS3 Guitar Hero / Rock Band guitars (USB dongle) | standard USB HID | 🟡 expected to work through the gamepad path + remapping — unverified |
| Xbox 360 **wireless** guitars | Xbox 360 Wireless Receiver + OS driver | 🟡 Windows/Linux where the OS exposes them; ❌ macOS (no driver exists) |
| Wii, PS4 and Xbox One (GH Live) guitars | Bluetooth / proprietary wireless | ❌ not supported |

**Why the X-plorer needs (and gets) special treatment:** it speaks the
Xbox 360 *vendor* protocol, not standard HID — macOS never shows it as
a game controller, so generic gamepad layers are blind to it. BeatByte
includes a userspace USB reader that claims the device, decodes its
20-byte interrupt reports and feeds them into the engine as a
first-class gamepad: green–orange frets, strum bar (d-pad), Start =
pause, Back = Hype. Tilt and whammy are not game mechanics in BeatByte
(Hype is a button), so nothing is lost. Verify any device on the
**INPUT TEST** screen in the main menu: five fret lamps, strum flash,
Hype lamp, and a would-hit indicator that applies the active mode's
rule.

## Installation

Grab a build from the
[Releases](https://github.com/pepperonas/beatbyte/releases) page:

- **macOS**: `BeatByte-<version>-<arch>.dmg` (or the portable tar.gz).
  The binaries are unsigned — right-click → Open on first launch.
- **Windows**: portable zip — unzip, run `beatbyte.exe`.
- **Linux**: `BeatByte-<version>-x86_64.AppImage` (`chmod +x`, run) or
  the portable tar.gz.

Or build from source (below).

## How Your Music Becomes a Playable Track

Drop audio files onto the window (or use `beatbyte-cli`) and BeatByte
turns them into charts — locally, deterministically, no cloud. The
pipeline, in order:

1. **Import** — dropped files are checked against the supported
   extensions (`.wav .ogg .flac .mp3 .m4a`), queued as a batch (with an
   animated progress panel), de-duplicated, and copied into
   `songs/imported/<sanitized-name>/`. Your files never leave your
   machine and are never committed anywhere.
2. **Decode** — [rodio](https://github.com/RustAudio/rodio) with
   [Symphonia](https://github.com/pdeljanov/Symphonia) decoders opens
   the file (for `.m4a`: the ISO-MP4 demuxer + AAC codec), downmixes to
   mono `f32` at the file's native sample rate. Analysis decodes at most
   20 minutes into memory (a deliberate cap — untrusted input);
   playback later *streams* from disk instead.
3. **Onset detection** — a from-scratch spectral-flux pipeline:
   STFT → log-compressed magnitude spectra → half-wave-rectified flux →
   adaptive median threshold → local-maximum peak picking. Every stage
   is a pure function over sample buffers.
4. **Tempo & beat grid** — BPM from the autocorrelation of the flux
   envelope, weighted by a log-normal prior around 120 BPM (to pick the
   right tempo octave), refined with parabolic interpolation for
   sub-BPM resolution; the beat grid is then phase-fitted so beats land
   on actual onsets.
5. **Chart generation** — deterministic and data-driven: per-difficulty
   profiles quantize onsets to the beat grid (beats → sixteenths),
   thin them by strength and spacing, assign lanes by spectral
   brightness with jump limiting, promote strong onsets to chords,
   turn fast runs into HOPOs, and carve **sustains energy-first** out
   of the gaps between strong onsets. Same audio in → bit-identical
   charts out. There is no randomness — variety comes from a hash of
   each note's own timestamp.
6. **Validation** — charts are treated as untrusted input even though
   we just generated them: BPM clamped to 20–400, size caps, path
   traversal and Windows-drive rejection.
7. **Play (and correct)** — the song appears in the browser with all
   four difficulties. Generated charts are *playable, not perfect* —
   the built-in editor is the correction pass.

Full walkthrough: [docs/importing-songs.md](docs/importing-songs.md) ·
analysis details: [`docs/audio/`](docs/audio/).

## Building from Source

Requirements: [Rust](https://rustup.rs/) 1.95 or newer. That's it — no
Python, no Node.js.

```bash
git clone https://github.com/pepperonas/beatbyte.git
cd beatbyte
cargo run --release -p beatbyte
```

On Linux you additionally need Bevy's system dependencies:

```bash
sudo apt-get install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

## Development

```bash
cargo run -p beatbyte        # run the game (dev profile)
cargo test --workspace       # run all tests
cargo fmt --all              # format
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The repository is a Cargo workspace:

| Crate | Purpose |
|-------|---------|
| `beatbyte-core` | Domain model: timing, notes, scoring, rules. Engine-free. |
| `beatbyte-chart` | Versioned chart format, serialization, validation. |
| `beatbyte-audio` | Decoding, playback, song clock, music analysis. |
| `beatbyte-game` | Bevy plugins: gameplay, rendering, UI, effects. |
| `beatbyte-cli` | `beatbyte` command line: analyze, generate, validate. |
| `beatbyte-editor` | Chart editor (architecture prepared, built later). |
| `apps/beatbyte` | The shippable game binary. |

Architecture decisions are documented as ADRs in
[`docs/decisions/`](docs/decisions/). Start with
[ADR-0001](docs/decisions/ADR-0001-rust-and-bevy.md).

## Controls

*Default mapping — everything is remappable (Milestone 8).*

| Action | Keyboard |
|--------|----------|
| Frets 1–5 | `A` `S` `D` `F` `G` |
| Strum | `Space` (or `↑`/`↓`) |
| Hype (special) | `Enter` |
| Pause | `Esc` |
| Mute / unmute all audio | `M` (or click the corner badge) |
| Menus | arrows + `Enter` / `Esc` — or the **mouse** (hover, click, wheel, right-click = back) |

Verify any device on the **INPUT TEST** screen (main menu): fret
lamps, strum flash, Hype lamp, and a would-hit indicator for the
active mode (tap / strum), toggleable on the spot with `T`.

Settings (volumes, scroll speed, effect toggles, fullscreen) and the
latency calibration live in-game and persist between sessions. Put
charts into `songs/` (e.g. from `beatbyte-cli generate`) and they appear
in the song browser.

## Chart Format & Song Importing

BeatByte uses a versioned JSON chart format documented in
[`docs/chart-format/`](docs/chart-format/). Songs are imported from common
audio formats and analyzed automatically:

```text
song.mp3 → analyze → generate → preview → play
```

Automatic charts are meant to be **playable, not perfect** — the toolchain is
designed for a `generated chart → human correction → final chart` workflow.
**[docs/importing-songs.md](docs/importing-songs.md)** walks through the whole
flow (every command in it verified end-to-end); see
[`docs/audio/`](docs/audio/) for analysis quality notes.

## CLI

The tooling binary is `beatbyte-cli` (the game itself owns the `beatbyte`
name):

```bash
beatbyte-cli analyze song.ogg      # BPM, beats, onsets
beatbyte-cli generate song.ogg     # produce a BeatByte chart
beatbyte-cli validate chart.json   # validate a chart file
beatbyte-cli inspect chart.json    # summarize a chart
beatbyte-cli demo                  # render the built-in songs + charts
```

## Testing

```bash
cargo test --workspace          # 219 tests
```

Core gameplay logic (timing windows down to their exact boundaries,
scoring, judgment, HOPO/tap rules, chart validation and generation,
onset/tempo analysis, editor op inverses, the depth projection, the
X-plorer report decoder) is covered by unit tests; integration tests
decode real fixture files for every advertised audio format (including
`.m4a`). On top of that sit two harnesses that run a full build:

```bash
BEATBYTE_SMOKE_TEST=1 cargo run -p beatbyte   # boots to menu, exits 0
BEATBYTE_AUTOPILOT=1  cargo run -p beatbyte   # plays a song — must be flawless
```

The autopilot injects timestamped inputs and fails on ANY miss or
overstrum; because judgment is input-stamp-driven, the verdict is
frame-rate independent. Every release must pass it.

## Release Process

Releases follow [Semantic Versioning](https://semver.org/) and
[Keep a Changelog](https://keepachangelog.com/). Tagged versions (`v*`)
trigger the release workflow, which builds and publishes native binaries for
all supported platforms. See [`docs/releases/`](docs/releases/).

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md)
and the [Code of Conduct](CODE_OF_CONDUCT.md) first. Security issues: see
[SECURITY.md](SECURITY.md).

## Support the Project

BeatByte is free, open source and ad-free. If it made you smile:

<a href="https://www.paypal.com/donate/?business=martin.pfeffer%40celox.io"><img src="https://img.shields.io/badge/Donate-PayPal-00457C?logo=paypal&logoColor=white&style=for-the-badge" alt="Donate via PayPal"/></a>
<a href="https://g.page/r/CXgdRV3QysvxEBM/review"><img src="https://img.shields.io/badge/Review%20celox.io-on%20Google%20Maps%20%E2%AD%90-4285F4?logo=googlemaps&logoColor=white&style=for-the-badge" alt="Review celox.io on Google Maps"/></a>

- 💛 **Donate** via PayPal: [martin.pfeffer@celox.io](https://www.paypal.com/donate/?business=martin.pfeffer%40celox.io)
- ⭐ **Rate my work** on Google Maps: [g.page/r/CXgdRV3QysvxEBM/review](https://g.page/r/CXgdRV3QysvxEBM/review)
- 🌟 Star the repo, file issues, send PRs — all equally appreciated.

## Roadmap

- [x] **M1 — Foundation**: workspace, CI, docs, minimal Bevy app
- [x] **M2 — Core domain**: chart model, timing, scoring, validation
- [x] **M3 — Audio**: decoding, playback, song clock, BPM/onset analysis
- [x] **M4 — Chart generation**: audio → analysis → playable chart
- [x] **M5 — First playable**: five lanes, hits, scoring, combos
- [x] **M6 — Game feel**: particles, shake, lighting, feedback
- [x] **M7 — UI**: menus, song browser, settings, calibration, results
- [x] **M8 — Controllers**: gamepads, guitar controllers, remapping
- [x] **M9 — Local multiplayer**: 2–4 players, versus & co-op
- [x] **M10 — Themes**: Garage, Punk, Metal, Stadium, Psychedelic, Cyber
- [x] **M11 — Chart editor**
- [x] **M12 — Packaging**: .dmg, portable builds, AppImage
- [x] **M13 — Polish pass**

## Legal

BeatByte is an original work inspired by the classic guitar rhythm game
genre. It contains no copyrighted assets, music, artwork or trademarks from
any commercial rhythm game. All bundled assets are original, procedurally
generated, or appropriately licensed (CC0 / public domain / OFL) — see
[`docs/development/asset-licenses.md`](docs/development/asset-licenses.md).

## License

[MIT](LICENSE) © 2026 [Martin Pfeffer](AUTHORS.md)
