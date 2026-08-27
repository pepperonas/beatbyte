# Making the gameplay screen read like the genre

Written 2026-08-27, from the reference screenshot Martin supplied, a
measured pass over what BeatByte currently draws, and the genre's
documented conventions.

## The boundary this plan does not cross

Every asset in this repository must be original, generated, CC0 or
OFL, and `CLAUDE.md` forbids "rhythm-game trademarks or lookalike
trade dress". So this plan adopts the **conventions** the genre
settled on — an odometer score, a segmented energy meter, a boxed
multiplier, marked energy phrases, prominent fret bars — and none of
the specific art: no logos, no borrowed fonts, no copy of a particular
game's plates, dials or note skins. BeatByte's own palette and pixel
face stay the voice throughout.

Also out of scope: **a rock meter**. Its needle is the most
recognisable part of the reference, but it measures a fail state, and
BeatByte has none — adding one changes whether a song can be lost,
which is gameplay, not looks. It stays where it already is, as P3 in
`docs/optimization-plan.md`, for a decision of its own.

## What the genre actually does

Confirmed rather than remembered:

- **Energy meter** fills a quarter per completed phrase, is activatable
  at half, and doubles the multiplier while it runs.
- **Multiplier** climbs x1 → x2 → x3 → x4 on consecutive notes, resets
  on a miss or an overstrum, and doubles under active energy.
- **Score** reads as a mechanical counter, digit by digit.

BeatByte's *mechanics* already match this almost exactly:
`hype_per_phrase` is 0.25, `streak_multiplier` steps per streak level,
and hype doubles the whole multiplier. Nothing in this plan changes
any of it. The gap is entirely in what the screen shows.

## Measured gaps

| # | Gap | Evidence |
|---|---|---|
| 1 | **Energy phrases are invisible** | Charts carry `phrases` (11 in one real track) and `complete_phrase()` awards meter, but no phrase is drawn anywhere. The player earns energy with no idea why. |
| 2 | Score is plain text | `hud.rs` spawns one `Text2d` of `score.to_string()`. |
| 3 | Multiplier gives no sense of progress | "x4" appears; how close the next level is, or how much a miss costs, is never shown. |
| 4 | Energy meter is an undivided bar | It cannot show the quarters it actually fills in, nor that half is the threshold to use it. |
| 5 | Activating energy changes almost nothing | Only the multiplier text tints. In the genre this is the moment the stage transforms. |
| 6 | Fret bars and hit line are faint | 0.18-unit bars and white stubs at the rails; the reference's neck is ruled and its hit line is unmistakable. |
| 7 | Gems are flat discs | A coloured face in a dark rim, with no highlight to catch the stage lights. |

## The work, in order

Each step is independently verifiable and leaves the game playable.

### U1 — Energy phrases on the highway *(the biggest gap)*

Notes inside a phrase get a distinct treatment (a bright rim and a
lit core), and the stretch of neck they sit on is tinted. Completing
the phrase flashes the meter.

*Why first:* it is the only gap where the player is missing
**information**, not polish. Everything else makes what they already
know look better.

*Verify:* a chart with known phrase bounds shows marked notes exactly
inside them; the meter step and the marked notes agree.

### U2 — The score plate becomes a counter

Fixed digit cells, right-aligned, leading cells dim — a readout rather
than a sentence. The multiplier moves into its own box beneath, and a
row of beads shows progress toward the next level, emptying on a miss.

*Verify:* digit count stable as the score grows; beads match
`streak % streak_per_level`; a miss visibly empties them.

### U3 — The energy meter shows its quarters

Four segments rather than one continuous fill, a clear threshold mark
at half, and a distinct "ready" state once it can be used.

*Verify:* completing a phrase lights exactly one more segment; the
ready state appears at half and not before.

### U4 — Activating energy transforms the stage

While hype runs: the neck and rails take the hype tint, the meter
drains visibly, and the multiplier box shows the doubling.

*Verify:* the tint follows `hype_active()` exactly, and reverts when
it ends.

### U5 — A ruled neck and a real hit line

Bar lines get weight and a metallic tone; the hit line becomes a
continuous lit bar across the neck instead of stubs at the rails.

*Verify:* screenshot; bars still fade with distance; no new occlusion
of approaching notes.

### U6 — Gems catch the light

A rim plus a top highlight, so a gem reads as an object under the
stage lights rather than a sticker.

*Verify:* screenshot in two themes; the 8-bit note style stays
pixel-exact and untouched.

## Status

All six landed in one pass (2026-08-27).

| Step | State | Note |
|---|---|---|
| U1 energy phrases | done | Marked notes plus a tinted band on the neck |
| U2 score counter | done | Fixed-width odometer, boxed multiplier, streak beads |
| U3 quartered meter | done | Four wells, part-quarter hairline, ready/running line |
| U4 hype transforms the stage | done | Neck washes to the energy tone and back |
| U5 ruled neck, real hit line | done | Bars roughly doubled in depth; the line spans the neck |
| U6 gems catch the light | done | Polished material rather than extra geometry |

Two defects fell out of the work rather than being looked for:

- **The 2D hype overlay was washing the venue.** It is a 900-pixel
  vertical band the width of the bed — the shape of a highway in the
  flat and depth views, and nothing like one in 3D, where the neck is
  a receding plane. Measured, it left the rails untouched and turned
  a wall forty units behind the vanishing point violet. It is now
  skipped in 3D, which supplies its own tint.
- **The stage tint eased at a rate that depended on entity count.**
  The blend advanced once per *surface* rather than once per player,
  so a neck with three tinted surfaces eased three times as fast, and
  adding a fourth would have changed the timing silently.

Still open, deliberately: the rock meter (P3 in
`docs/optimization-plan.md`), because it is a fail state rather than
a look.

## Rules for the whole pass

- **Judgment is untouchable.** After every step the autopilot must
  score identically — that is the proof presentation cannot reach the
  engine.
- **The depth view keeps working.** It is a second renderer, not a
  legacy path, and every step must state what it does there.
- **Multiplayer keeps its layout.** Corner plates are a solo
  arrangement; two to four necks have no free corners.
- **Measure before claiming.** Screenshots for looks, the autopilot
  for behaviour, and numbers only after the measurement.

---

# Round two (2026-08-27)

The first round fixed what the screen *said*. Comparing the result
against the reference side by side, what is left is what the screen
*is*: proportion and surface.

## Measured, not eyeballed

| | Reference | BeatByte after round one |
|---|---|---|
| Neck width at the hit line | ~50 % of the frame | **31 %** (rails measured at 793 px of 2560) |
| Board surface | a patterned fretboard | flat dark fill, lane lines only |
| Gem size | fills its lane | 72 px in a lane roughly twice that |

Everything else about the frame — venue, HUD, hit line, phrases —
now holds up. These three do not, and the first is why: a neck at
31 % leaves the eye nothing to do with the other 69 %, and it makes
the gems look like beads on a thread instead of buttons on a board.

## The work

### V1 — Widen the neck *(solo only)*

One spread factor applied where the width is actually derived —
`lane_x()` and the three `bed_width()` sites — so rails, lane strips,
receptors, bursts, bar lines, phrase bands and notes all follow from
one number.

**Solo only.** Two to four necks side by side already use the room;
widening them would run them into each other. The factor is taken
from the layout's player count, so it cannot drift out of step.

**Not by moving the camera**, which was the obvious alternative:
pulling in magnifies the board but shortens how far up the neck you
can see, and reading ahead is the game.

*Verify:* rails measured again at the same height; autopilot scores
identically, because lane geometry is presentation and judgment reads
the chart.

### V2 — The board gets a surface

A generated texture — procedural, original, no asset file — with a
lengthwise grain and faint bar shading, tiled down the neck. A
fretboard is a *thing*; the current bed is an absence of one.

*Verify:* screenshot; the bed must stay dark enough that gems and
lane lines keep their contrast.

### V3 — Gems sized to their lane

The gem radius is a constant in world units, so widening the neck
without touching it leaves the notes looking undersized. It scales
with the same factor.

*Verify:* screenshot; the row of five receptors still reads as five
distinct buttons, not a bar.

## Round two status

All three landed (2026-08-27).

| Step | State | Measured |
|---|---|---|
| V1 neck width | done | 31 % → **45 %** of the frame at the hit line |
| V2 board surface | done | Generated grain, brightness pinned to 0.72–1.0 |
| V3 gem size | done | Scales with the same factor, so lanes stay filled |

Judgment untouched: the same song scores 98/98 perfect before and
after. Multiplayer keeps the layout's own spacing, pinned by a test
that reads the factor straight from a two-player layout.

## Same rules as round one

Judgment untouched and proven by an identical autopilot score, the
depth view unaffected, multiplayer layout unchanged, and every number
in this document measured before it was written down.
