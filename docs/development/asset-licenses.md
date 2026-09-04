# Asset Licenses

Every asset shipped in this repository must be original,
procedurally generated, CC0/public-domain, or under a license compatible
with distributing this project (MIT code + documented asset licenses).

**Never** commit copyrighted music, artwork, fonts or sounds from
commercial games.

| Asset | Source | License | Notes |
|-------|--------|---------|-------|
| `assets/fonts/PressStart2P-Regular.ttf` | [Press Start 2P](https://fonts.google.com/specimen/Press+Start+2P) by CodeMan38 (via google/fonts) | [SIL OFL 1.1](../../assets/fonts/PressStart2P.OFL.txt) | License text bundled next to the font as required. |
| `assets/fonts/BebasNeue-Regular.ttf` | [Bebas Neue](https://fonts.google.com/specimen/Bebas+Neue) by Dharma Type (via google/fonts) | [SIL OFL 1.1](../../assets/fonts/BebasNeue.OFL.txt) | License text bundled next to the font as required. The round style's display face (HUD, menus); chosen for tabular digits — measured, all ten at the same advance — so the score counter never jitters. |

When adding an asset:

1. Add a row to the table above (path, origin, license, attribution
   requirements).
2. If the license requires bundling its text (e.g. OFL fonts), place it
   next to the asset as `<name>.LICENSE.txt`.
3. Procedurally generated assets should reference the generator script.
