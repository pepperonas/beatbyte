# Contributing to BeatByte

Thanks for your interest in BeatByte! Contributions of all kinds are
welcome: code, documentation, charts, themes, bug reports and playtesting
feedback.

## Getting Started

1. Install [Rust](https://rustup.rs/) 1.95 or newer.
2. Fork and clone the repository.
3. `cargo run -p beatbyte` — the game should compile and launch.
4. `cargo test --workspace` — everything should be green before you start.

On Linux, install Bevy's system dependencies first:

```bash
sudo apt-get install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

## Project Layout

The repository is a Cargo workspace. The short version:

- **`crates/beatbyte-core`** — pure domain logic (timing, notes, scoring).
  No Bevy, no I/O. If your change is a gameplay *rule*, it probably
  belongs here, with tests.
- **`crates/beatbyte-chart`** — the chart file format and validation.
- **`crates/beatbyte-audio`** — decoding, playback, music analysis.
- **`crates/beatbyte-game`** — everything Bevy: rendering, UI, effects.
- **`crates/beatbyte-cli`** — the command-line tools.
- **`apps/beatbyte`** — the thin game binary.

Architecture decisions are recorded in `docs/decisions/`. If your change
alters an architectural decision, update or add an ADR.

## Quality Gates

Every pull request must pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace
```

CI enforces all of these. Please run them locally before pushing.

## Guidelines

- **Tests**: gameplay rules, timing math, chart parsing/validation and
  analysis code need unit tests. UI/rendering changes should be manually
  verified (describe how in the PR).
- **Timing is sacred**: anything that touches the song clock, hit windows
  or judgment logic needs extra care and test coverage. Do not tie
  gameplay timing to frame counts.
- **Keep layers separated**: the chart model must not depend on Bevy;
  audio analysis must not depend on the renderer.
- **Assets**: only original, CC0/public-domain or license-compatible
  assets. Document every third-party asset in
  `docs/development/asset-licenses.md`. Never commit copyrighted music or
  artwork.
- **Commits**: use meaningful conventional-style messages
  (`feat: …`, `fix: …`, `docs: …`, `test: …`, `perf: …`, `chore: …`).
- **Changelog**: user-facing changes get an entry under `[Unreleased]` in
  `CHANGELOG.md`.

## Reporting Bugs

Use the GitHub issue templates. For rhythm/timing bugs, please include
your OS, audio device, display refresh rate and latency calibration
settings — they matter.

## Security

Please report security issues privately as described in
[SECURITY.md](SECURITY.md).

## License

By contributing you agree that your contributions are licensed under the
[MIT License](LICENSE).
