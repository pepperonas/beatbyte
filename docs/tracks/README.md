# Track collection

Every song in the author's BeatByte library: **8,569 songs** with **8,571 charts**, 570 hours of music. Generated on 2026-10-11 by `tools/tracklist/build.py` — edit the script, not these pages.

BeatByte ships no songs. These charts were imported from the community catalogue behind the Bridge app (Encore); how that works, and how to import charts into your own library, is in [docs/bridge-bulk-import.md](../bridge-bulk-import.md). The audio is the player's own; nothing here is downloadable.

## By artist

| Letter | Songs |
|---|--:|
| [A](A.md) | 673 |
| [B](B.md) | 822 |
| [C](C.md) | 478 |
| [D](D.md) | 460 |
| [E](E.md) | 228 |
| [F](F.md) | 337 |
| [G](G.md) | 328 |
| [H](H.md) | 195 |
| [I](I.md) | 187 |
| [J](J.md) | 347 |
| [K](K.md) | 218 |
| [L](L.md) | 389 |
| [M](M.md) | 710 |
| [N](N.md) | 270 |
| [O](O.md) | 179 |
| [P](P.md) | 417 |
| [Q](Q.md) | 75 |
| [R](R.md) | 452 |
| [S](S.md) | 931 |
| [T](T.md) | 376 |
| [U](U.md) | 43 |
| [V](V.md) | 104 |
| [W](W.md) | 200 |
| [X](X.md) | 17 |
| [Y](Y.md) | 55 |
| [Z](Z.md) | 27 |
| [0-9](0-9.md) | 35 |
| [other](other.md) | 16 |

## Reading the density line

Each song gets one line of 24 cells: the song split into 24 equal slices, start on the left, and each cell shows how many expert notes fall into its slice. The scale is the same for every song, so a calm song looks calm next to a fast one.

| Cell | Notes per second |
|---|---|
| `·` | none — intro, break, outro |
| `▁` | 0–1.75 |
| `▂` | 1.75–3.5 |
| `▃` | 3.5–5.25 |
| `▄` | 5.25–7 |
| `▅` | 7–8.75 |
| `▆` | 8.75–10.5 |
| `▇` | 10.5–12.25 |
| `█` | over 12.25 |

Example: `··▂▃▃▅▅▃▃▅▇▇▅▃▃▅▅▇██▇▅▃▂·` — a quiet intro, verses and choruses that build, a dense solo near the end and a short outro.

## Genres

| Genre | Songs |
|---|--:|
| Rock | 1,241 |
| Metal | 1,004 |
| Alternative | 500 |
| Pop | 404 |
| Classic Rock | 323 |
| Indie Rock | 319 |
| Alternative Rock | 308 |
| Punk | 230 |
| Pop-Rock | 224 |
| Hard Rock | 218 |
| Pop Rock | 186 |
| Country | 144 |

## Regenerating

```bash
python3 -I tools/tracklist/build.py <library>/songs
```
