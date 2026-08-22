# Changelog

All notable changes to BeatByte are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/pepperonas/beatbyte/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/pepperonas/beatbyte/releases/tag/v0.0.1
