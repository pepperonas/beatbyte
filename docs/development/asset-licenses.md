# Asset Licenses

Every asset shipped in this repository must be original,
procedurally generated, CC0/public-domain, or under a license compatible
with distributing this project (MIT code + documented asset licenses).

**Never** commit copyrighted music, artwork, fonts or sounds from
commercial games.

| Asset | Source | License | Notes |
|-------|--------|---------|-------|
| *(downloaded, never in the repository)* `wav2vec2-base-960h/model.onnx` | `facebook/wav2vec2-base-960h` (Meta AI), as the ONNX export `Xenova/wav2vec2-base-960h` (`onnx/model.onnx`, revision a19f851), re-hosted unchanged as a release asset of this repository; SHA-256 `e46614…5490d` pinned in `beatbyte-ml` | [Apache-2.0](https://huggingface.co/facebook/wav2vec2-base-960h) | Fetched only by `beatbyte-cli models install` / a settings action, verified against size and hash. The English acoustic model behind the lyric aligner (`beatbyte-lyrics`). |
| *(downloaded, never in the repository)* `beat-this-mel/model.onnx`, `beat-this-small/model.onnx`, `beat-this/model.onnx` | *Beat This!* (Foscarin, Schlüter & Widmer, ISMIR 2024; Institute of Computational Perception, JKU Linz — "the code and the published model weights are released under the MIT license"), as the ONNX exports of the Rust port `danigb/beat-this-rs` (`models/mel_spectrogram.onnx` + `models/beat_this_small.onnx` at commit 089b509, `beat_this.onnx` from its `model-large` release), re-hosted unchanged as release assets of this repository with a NOTICE; SHA-256 `fdd59e…e3de9`, `a5f8d3…9172f`, `5f810d…70f02` pinned in `beatbyte-ml` | [MIT](https://github.com/CPJKU/beat_this/blob/main/LICENSE) | Fetched only by `beatbyte-cli models install`, verified against size and hash. The log-mel front end and the two beat/downbeat models behind `beatbyte-meter`. Training data of the upstream models is partly copyrighted (their README says so); the weights' licence is MIT. |
| *(downloaded, never in the repository)* `basic-pitch/model.onnx` | *Basic Pitch* (Bittner, Bosch, Rubinstein, Meseguer-Brocal & Ewert, ICASSP 2022; Spotify), the ONNX file published in its own repository (`basic_pitch/saved_models/icassp_2022/nmp.onnx` at commit `dfb20ef`), fetched from that pinned upstream URL — NOT re-hosted; SHA-256 `2c3c1d…9a0ec`, 230 444 bytes, pinned in `beatbyte-ml` | [Apache-2.0](https://github.com/spotify/basic-pitch/blob/main/LICENSE) | Fetched only by `beatbyte-cli models install basic-pitch`, verified against size and hash. Polyphonic note transcription behind `beatbyte-poly`: which notes were struck together, read by the classic `chords` ingredient. Re-hosting it as a release asset of this repository is a publish and left to the maintainer. |
| *(generated at startup)* stage surfaces — tolex, grille cloth, brushed metal, driver cone, stage deck | `crates/beatbyte-game/src/surfaces.rs` (this repository) | original | Colour, normal and roughness tiles from tileable value noise and hand-written height fields; no image file exists, the generator is the asset. |
| *(generated at startup)* the crowd and the band | `crates/beatbyte-game/src/gameplay/figure.rs`, `crowd.rs`, `band.rs` (this repository) | original | People from primitives (capsules, frustums, spheres, boxes) under a hash-chosen look; instruments from the same primitives. No likeness, no costume, no logo. |
| `assets/fonts/BebasNeue-Regular.ttf` | [Bebas Neue](https://fonts.google.com/specimen/Bebas+Neue) by Dharma Type (via google/fonts) | [SIL OFL 1.1](../../assets/fonts/BebasNeue.OFL.txt) | License text bundled next to the font as required. The round style's display face (HUD, menus); chosen for tabular digits — measured, all ten at the same advance — so the score counter never jitters. |

When adding an asset:

1. Add a row to the table above (path, origin, license, attribution
   requirements).
2. If the license requires bundling its text (e.g. OFL fonts), place it
   next to the asset as `<name>.LICENSE.txt`.
3. Procedurally generated assets should reference the generator script.
