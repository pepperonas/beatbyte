<div align="center">

# 🎸 BeatByte

**An original 8-bit rhythm game. Five lanes. Your music. Pure pixels.**

[![CI](https://github.com/pepperonas/beatbyte/actions/workflows/ci.yml/badge.svg)](https://github.com/pepperonas/beatbyte/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pepperonas/beatbyte?include_prereleases&sort=semver)](https://github.com/pepperonas/beatbyte/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.95%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Bevy](https://img.shields.io/badge/Bevy-0.19-blueviolet)](https://bevy.org/)
[![SemVer](https://img.shields.io/badge/versioning-SemVer-blue)](CHANGELOG.md)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](#supported-platforms)

</div>

---

BeatByte is an original, open-source rhythm game in the classic five-lane guitar
tradition — rebuilt from scratch in **Rust** and **Bevy** with an 8-bit /
pixel-art identity of its own. Drop in your own music, let BeatByte analyze it
and generate a playable chart, then shred it solo or with up to four players on
one machine.

> **Status: all thirteen build milestones complete** — playable solo or
> with friends, themed, packaged for all three platforms. 0.x versions
> until the tuning settles; see the [roadmap](#roadmap).

<p align="center">
  <img src="docs/media/beatbyte-gameplay.png" alt="BeatByte gameplay: five-lane highway with falling notes, score, combo, multiplier and judgment popup" width="800"/>
</p>

*A flawless run in progress: x4 multiplier, 75-note combo, PERFECT
popup, hit particles on the blue receptor — with the Metal theme's
ember backdrop.*

<p align="center">
  <img src="docs/media/beatbyte-results.png" alt="BeatByte results screen: S rank, 100% accuracy, 117 perfect notes" width="800"/>
</p>

## Features

- 🎸 **Classic five-lane gameplay** — single notes, chords, sustains,
  hammer-ons/pull-offs, combos, multipliers and the Hype meter
- 🎵 **Bring your own music** — OGG / WAV / FLAC / MP3; drop generated
  charts into `songs/` and they appear in the browser
- 🤖 **Automatic chart generation** — BPM & onset analysis turns any song
  into a playable chart across four difficulties (playable, not perfect —
  the built-in editor is the correction pass)
- ✏️ **Chart editor** — beat grid, note placement, HOPOs, audio preview,
  undo/redo, validated saves
- 👾 **8-bit identity** — pixel font, six themed stages with procedural,
  beat-reactive backdrops; every asset generated or openly licensed
- 🕹️ **Controllers** — keyboard, gamepads and guitar-style controllers,
  fully remappable in-game
- 👥 **Local multiplayer** — 2–4 players, versus and co-op, split highways
- ⏱️ **Serious timing** — deterministic judgment engine, reconciled song
  clock, configurable hit windows, in-game latency calibration
- 🏆 **High scores**, persistent settings, procedural SFX, an original
  synthesized demo track — and an autopilot that must play every build
  flawlessly before release
- 🖥️ **Cross-platform** — macOS (.dmg), Windows (portable), Linux
  (AppImage); WebAssembly on the horizon

## Supported Platforms

| Platform | Status |
|----------|--------|
| macOS (Apple Silicon & Intel) | ✅ primary target |
| Windows | ✅ primary target |
| Linux | ✅ primary target |
| Web (WASM) | 🔮 future |

## Installation

Grab a build from the
[Releases](https://github.com/pepperonas/beatbyte/releases) page:

- **macOS**: `BeatByte-<version>-<arch>.dmg` (or the portable tar.gz).
  The binaries are unsigned — right-click → Open on first launch.
- **Windows**: portable zip — unzip, run `beatbyte.exe`.
- **Linux**: `BeatByte-<version>-x86_64.AppImage` (`chmod +x`, run) or
  the portable tar.gz.

Or build from source (below).

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
| Strum | `↑` / `↓` |
| Hype (special) | `Space` |
| Pause | `Esc` |
| Menus | arrows + `Enter` / `Esc` |

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
See [`docs/audio/`](docs/audio/) for analysis quality notes.

## CLI

The tooling binary is `beatbyte-cli` (the game itself owns the `beatbyte`
name):

```bash
beatbyte-cli analyze song.ogg      # BPM, beats, onsets
beatbyte-cli generate song.ogg     # produce a BeatByte chart
beatbyte-cli validate chart.json   # validate a chart file
beatbyte-cli inspect chart.json    # summarize a chart
beatbyte-cli demo                  # render the built-in demo song + chart
```

## Testing

```bash
cargo test --workspace
```

Core gameplay logic (timing, scoring, judgment, chart validation, analysis)
is covered by unit tests; integration tests live next to the crates they
exercise.

## Release Process

Releases follow [Semantic Versioning](https://semver.org/) and
[Keep a Changelog](https://keepachangelog.com/). Tagged versions (`v*`)
trigger the release workflow, which builds and publishes native binaries for
all supported platforms. See [`docs/releases/`](docs/releases/).

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md)
and the [Code of Conduct](CODE_OF_CONDUCT.md) first. Security issues: see
[SECURITY.md](SECURITY.md).

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
