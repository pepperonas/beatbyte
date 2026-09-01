# Asset Licenses

Every asset shipped in this repository must be original,
procedurally generated, CC0/public-domain, or under a license compatible
with distributing this project (MIT code + documented asset licenses).

**Never** commit copyrighted music, artwork, fonts or sounds from
commercial games.

| Asset | Source | License | Notes |
|-------|--------|---------|-------|
| `assets/lyrics/circuit-breaker.lrc` | Original lyrics written for this project (enhanced LRC, hand-timed to the demo song's 128 BPM grid) | MIT (project license) | Text and timing are original; the song itself is synthesized by `beatbyte-audio/src/demo.rs`. |
| `assets/fonts/PressStart2P-Regular.ttf` | [Press Start 2P](https://fonts.google.com/specimen/Press+Start+2P) by CodeMan38 (via google/fonts) | [SIL OFL 1.1](../../assets/fonts/PressStart2P.OFL.txt) | License text bundled next to the font as required. |

When adding an asset:

1. Add a row to the table above (path, origin, license, attribution
   requirements).
2. If the license requires bundling its text (e.g. OFL fonts), place it
   next to the asset as `<name>.LICENSE.txt`.
3. Procedurally generated assets should reference the generator script.
