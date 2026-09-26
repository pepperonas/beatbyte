<!--# block name="none" --><!--# endblock -->
# BeatByte — Free Rhythm Game for Your Own Music

> BeatByte is a free, open-source (MIT) five-lane rhythm game for macOS, Windows and Linux, written in Rust. It turns the player's own music files into playable charts in four difficulties, shows karaoke lyrics, runs on a 3D concert stage, and plays with a keyboard, gamepads or a guitar controller, for one to four local players. No song ships with the game; nothing is uploaded.

This is the Markdown version of https://beatbyte.celox.io/ for agents and text tools. A short summary with every link lives at https://beatbyte.celox.io/llms.txt.

## Download

- **Newest release:** https://beatbyte.celox.io/download (picks the file for your platform; always the current release)
- **Current version:** <!--# include virtual="/ssi/version.txt" stub="none" --> · released <!--# include virtual="/ssi/date.txt" stub="none" -->
- **Release data as JSON:** https://beatbyte.celox.io/latest.json
- **Requirements:** macOS (Apple silicon): macOS 11+ · Apple silicon; macOS (Intel): macOS 11+ · Intel; Windows: Windows 10/11 · 64-bit · portable zip; Linux AppImage: Linux · x86_64

Files in the current release:

<!--# include virtual="/ssi/files.md" stub="none" -->

## Features

- **Your songs become charts** — MP3, M4A, FLAC, OGG or WAV: BeatByte analyses tempo, beats and onsets and builds a playable chart in Easy, Medium, Hard and Expert. Drop a folder on the window and it watches it for new tracks.
- **Karaoke on the highway** — Press `L` and BeatByte fetches synced lyrics from lrclib’s open catalogue, or drop your own `.lrc`. The words fill in on the same clock the notes fall on; an optional local aligner times them word by word.
- **A stage, not a backdrop** — A 3D concert venue with moving heads, PA stacks, fog and a crowd that dances to the song. Land a star-power phrase and lightning strikes the fret.
- **Keyboard, gamepad or guitar** — Every input is remappable in the game. Gamepads work through the system, and a built-in driver reads the wired Guitar Hero X-plorer directly. Two to four players share the screen.
- **Timing you can trust** — Judgment runs on input timestamps against a clock reconciled with the audio device, not on frames. Calibrate latency in the game, slow any section to 50 % and loop it in practice mode.
- **Edit, track, improve** — A built-in chart editor fixes what the generator got wrong and playtests it in the real highway. A local roster files every run under its player, with statistics and three hundred achievements.

## Install

1. **Download your build** — The button above picks your platform: a DMG for Apple silicon or Intel Macs, a portable zip for Windows, an AppImage for Linux.
2. **Start it** — macOS: drag it to Applications, then right-click → *Open* the first time (the build is not notarized). Windows: unzip and run `beatbyte.exe`. Linux: `chmod +x` the AppImage and run it.
3. **Bring your music** — Drop an audio file or a whole folder onto the window. BeatByte charts it in the background and it appears in the song browser.

## Verify

<!--# include virtual="/ssi/checksums.md" stub="none" -->

- The checksums are the ones GitHub computed for the release files. Compare with `shasum -a 256` on macOS and Linux or `Get-FileHash` on Windows.

## FAQ

**Is BeatByte free?** Yes. It is free and open source under the MIT licence, with no ads, no account and no in-game purchases.

**Which songs can I play?** Your own: MP3, M4A, FLAC, OGG and WAV files. No song ships with the game. With `yt-dlp` installed you can also add a song by typing its name.

**What do I need to run it?** macOS 11 or later on Apple silicon or Intel, 64-bit Windows 10 or 11, or 64-bit Linux, with a GPU that runs Metal, DirectX 12 or Vulkan. A keyboard is enough to play.

**Why does macOS warn me the first time?** The macOS builds are signed ad hoc but not notarized by Apple. Right-click the app and choose *Open* once; after that it starts normally.

**How do I update?** Download the newest build from this page and replace the old one. Your songs, charts, scores, players and settings live in your user data folder and stay where they are.

**Is the download genuine?** Every file comes straight from the project’s GitHub Releases, built by its release workflow. The SHA-256 of each file is listed on this page; compare it with the file you downloaded.

## Limits

- No songs ship with the game; you bring your own music
- Generated charts are playable, not hand-authored; the editor is the correction pass
- No online multiplayer
- No mobile or web version
- The macOS builds are not notarized: open them once with right-click → Open
- Xbox 360 wireless guitars are not supported on macOS

## Links

- Source code: https://github.com/pepperonas/beatbyte
- Changelog: https://beatbyte.celox.io/changelog.md
- Licence (MIT): https://github.com/pepperonas/beatbyte/blob/main/LICENSE
- Support the project: https://www.paypal.com/donate/?business=martin.pfeffer@celox.io&currency_code=EUR&item_name=BeatByte
- Author: Martin Pfeffer, https://celox.io — Imprint https://celox.io/impressum/ · Privacy https://celox.io/datenschutz/
