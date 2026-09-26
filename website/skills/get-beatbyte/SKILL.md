---
name: get-beatbyte
description: Download, verify and install the newest BeatByte — the free, open-source five-lane rhythm game for macOS, Windows and Linux that charts your own music. Use when someone asks for the game, its latest version, a download link for their platform, or how to check the file is genuine.
license: MIT
---

# Get BeatByte

BeatByte is a free, open-source (MIT) five-lane rhythm game for macOS, Windows and Linux, written in Rust. It turns the player's own music files into playable charts in four difficulties, shows karaoke lyrics, runs on a 3D concert stage, and plays with a keyboard, gamepads or a guitar controller, for one to four local players. No song ships with the game; nothing is uploaded.

## 1. Find the newest release

`GET https://beatbyte.celox.io/latest.json` returns `version`, `published`, `notes` and `assets[]`, each with `target`,
`name`, `url`, `size` (bytes) and `sha256`. It is refreshed from GitHub Releases every 15 minutes. On the
page itself, browsers with WebMCP expose the same data as the tools `get_latest_release`,
`get_download_url` and `get_checksums`.

## 2. Download

- Stable link, always the newest file for the visitor's platform: <https://beatbyte.celox.io/download>
- macOS (Apple silicon): <https://beatbyte.celox.io/download/macos> — macOS 11+ · Apple silicon
- macOS (Intel): <https://beatbyte.celox.io/download/macos-intel> — macOS 11+ · Intel
- Windows: <https://beatbyte.celox.io/download/windows> — Windows 10/11 · 64-bit · portable zip
- Linux AppImage: <https://beatbyte.celox.io/download/linux-appimage> — Linux · x86_64

## 3. Verify

- The file's SHA-256 must equal the matching `assets[].sha256` in `latest.json`.

- The checksums are the ones GitHub computed for the release files. Compare with `shasum -a 256` on macOS and Linux or `Get-FileHash` on Windows.

## 4. Install

1. The button above picks your platform: a DMG for Apple silicon or Intel Macs, a portable zip for Windows, an AppImage for Linux.
2. macOS: drag it to Applications, then right-click → *Open* the first time (the build is not notarized). Windows: unzip and run `beatbyte.exe`. Linux: `chmod +x` the AppImage and run it.
3. Drop an audio file or a whole folder onto the window. BeatByte charts it in the background and it appears in the song browser.

## Limits

- No songs ship with the game; you bring your own music
- Generated charts are playable, not hand-authored; the editor is the correction pass
- No online multiplayer
- No mobile or web version
- The macOS builds are not notarized: open them once with right-click → Open
- Xbox 360 wireless guitars are not supported on macOS

More: [product page](https://beatbyte.celox.io/) · [Markdown version](https://beatbyte.celox.io/index.md) · [changelog](https://beatbyte.celox.io/changelog.md) · [source](https://github.com/pepperonas/beatbyte)
