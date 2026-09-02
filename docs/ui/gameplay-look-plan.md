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

---

# Round three (2026-08-27)

Rounds one and two fixed what the screen says and how it is
proportioned. Held next to the reference, what is left is **light**.

## Measured

Sampling the venue only — the outer thirds of the frame, above the
neck, excluding the HUD:

| | Reference | BeatByte |
|---|---|---|
| Brightness, median | a lit room | **0.13** |
| Saturation, median | saturated stage light | **0.20** |

That is the whole remaining difference in one pair of numbers: the
room is dark and very nearly grey. A white key light on grey
materials returns grey, however many boxes are in the room.

## The work

### W1 — Colour the stage light

Two coloured lamps from opposite sides — warm one way, cool the
other — plus a lift to the venue's own materials. Colour separation
across the room is what makes a stage look lit rather than merely
visible.

**The neck must not brighten with it.** The lamps are placed and
ranged so the room takes the light and the fretboard does not: notes
have to keep their contrast against the board, and that is worth more
than any amount of atmosphere.

*Verify:* the same venue-only sample; brightness and saturation both
up, and the *bed's* brightness unchanged within tolerance.

### W2 — Lane separators

Neutral dividers between lanes. The genre has them, and they do real
work: five coloured lines say where lanes *are*, a divider says where
one **ends**.

*Verify:* screenshot; the coloured lane lines must stay the brighter
of the two, or the board reads as a grid instead of a highway.

### W3 — Gems that read as objects

The gems are flat-shaded discs. A generated radial texture — bright
centre, darker toward the rim — gives them a lit face without a
second entity per note, which at these note counts matters.

*Verify:* screenshot; and the 8-bit note style stays untouched.

## Round three status

| Step | State | Measured |
|---|---|---|
| W1 stage light | done | Venue brightness 0.13 → **0.20**, saturation 0.20 → **0.29**, board unchanged (0.250 → 0.262) |
| W2 lane dividers | done | Four neutral dividers, kept dimmer than the lane lines |
| W3 gem faces | done | Brightness across a gem 50 → **116**, then floored so distant notes stay legible |

W3 took three attempts, and the first two are worth recording. A
`base_color_texture` alone made the gems **flatter** — 50 → 10 —
because a gem's look is dominated by its emissive, which that map
does not touch. Adding `emissive_texture` gave the shape (116) but
dimmed the far notes, so the face got a floor: shape is only worth
having while the note stays readable at the top of the neck. Both
constraints are now tests.

## Same rules

Judgment proven identical by the autopilot, depth view unaffected,
multiplayer unchanged, every number measured before it is written.

---

# Round four (2026-08-27)

## Measured

The whole frame sat at 0.21 median brightness with 13 % of it very
nearly black. But the real finding was not a number: a screenshot
caught the exact frame in which a note landed, and **nothing
happened**. The receptor lit, a flat ring spread across the board,
and that was all.

The comment in the source that justified the flat ring said the
genre's flame "spreads across the board rather than rising off it".
That is backwards. It rises. That comment was mine, from an earlier
round, written from memory rather than from the reference.

## The work

### X1 — A flame off the fret *(the signature)*

A cone rising from the receptor in the lane's colour, white-hot at
the strike and cooling to the lane tint as it dies. A held sustain
keeps a low flame burning under the fret.

*Verify:* a screenshot with a flame actually in it — which took three
shots to catch, because the effect is a third of a second long.

### X2 — Latin letters the built-in face cannot draw

`font_safe` fixed the fullwidth look-alikes but "Skatebård" still
rendered with a box. The earlier measurement was of the WRONG FONT:
Press Start 2P has 656 glyphs and does carry `å`, but the game runs
the engine's built-in face whenever the round note style is on —
which is the default — and that face has **95 glyphs**, plain ASCII.

Folding is therefore style-dependent, not a blanket rule: turning
"Björk" into "Bjork" when the font can draw it is damage, and leaving
it when the font cannot is a box.

### X3 — A crowd that moves

Driven from the song's own tempo map, not a free timer, so the room
is on the beat the player is playing to. Each head has its own phase
so the ranks ripple rather than pumping as one block.

## Status

| Step | State | Measured |
|---|---|---|
| X1 hit flame | done | Caught on camera; shape retuned from 5:1 (a laser) to about 5:3 |
| X2 font folding | done | Built-in face measured at 95 glyphs; folding gated on the active style |
| X3 crowd bob | done | Honours Stage Motion like every other ambient movement |

Frame time with flames and a moving crowd: median **10.0 ms
(100 fps)**, 99th percentile 12.4 ms. Autopilot 98/98 perfect,
2-player PASSED.

---

# Round five — what Guitar Hero II actually did

The brief this time named a specific game rather than a genre, so the
work started with research instead of memory.

## The line we do not cross

The repository forbids trade dress that imitates another rhythm game,
and that rule is not in tension with the brief — it sharpens it. What
makes a fretboard read as one is **convention**: a ruled neck, strings
between the lanes, a decorated border, physical-looking fret buttons.
Those are the vocabulary of the genre, used by every game in it and by
the instrument the genre is imitating. What belongs to one game is its
*specific* artwork — a particular logo, typeface, character art, and
the literal motifs on its necks. So: the grammar, drawn ourselves.

## What the research found

| Trait | Guitar Hero II | BeatByte today |
|---|---|---|
| Neck border | **Every highway is decorated** — the Rock neck is tan with wave patterns down its sides, Metal red with flames and chains, Goth purple with webs. The border is how a venue announces itself. | Plain bright rails, identical in all six themes |
| Ruling | Lines mark the **beat**, dense enough to read the tempo off the neck | One line per **bar** — four times too sparse |
| Between lanes | Thin "division lines" read as strings | Four neutral dividers, matte |
| Multiplier | Bottom left, **ten dots** filling toward the next step | Already exactly this |
| Star Power | Blue gauge above the rock meter | Hype panel, four segments |
| Rock meter | Lower right, four bands green→flashing red | Deliberately absent (roadmap P3) |

Two findings are worth stating plainly. The HUD **already matches** —
the ten-dot multiplier was arrived at independently and is the genre's
own solution. And the single biggest visual gap is the one thing no
screenshot comparison had caught: the neck is *unruled*. A bar line
every four beats gives the eye nothing to keep time against.

## The work

### Y1 — Rule the neck by the beat

One line per beat, with the downbeat wider and brighter than the three
that follow. This is what makes a neck read as an instrument rather
than a road, and it hands the player a tempo reference that does not
depend on hearing the track.

*Measure:* lines visible in one frame, before and after; downbeat lines
measurably wider.

### Y2 — A decorated border per theme

Each of the six themes gets its own rail pattern, generated the way
`board_shade` is — from a hash, so it is identical every run and ships
no art asset. Motifs are drawn for this game and chosen to rhyme with
each theme's existing backdrop, not borrowed: garage rivets, punk
sawteeth, metal chevrons, stadium bands, psychedelic waves, cyber
ticks.

*Measure:* the six patterns differ from each other; each is non-flat.

### Y3 — Fret buttons, not outlines

The receptors are rings. A fret button is a physical thing: a rim that
catches light and a darker inset. Give them an inner face so a resting
receptor reads as a button waiting to be pressed.

*Measure:* brightness span across the receptor at rest.

## The rule that outranks all of this

Nothing here may touch judgment. Same proof as every round: the same
song scores identically before and after.

## Round five status

All three shipped, and the research turned out to matter: the largest
gap was the one no side-by-side screenshot had shown.

| | Before | After |
|---|---|---|
| Ruling | one line per **bar** | one per **beat**, downbeat at full width and brightness, off-beats at 0.55 depth / 0.5 strength |
| Border | plain rail, identical in six themes | rail plus a **generated decorated strip** per theme, measured at rgb(107, 49, 38) against a rgb(39, 34, 31) floor before widening, and widened again until the motif read |
| Receptors | ring on bare board | ring seated in a metal collar |

Y2 needed a second pass. The first strip was 0.10 wide and dim enough
that the motif could not be told from a plain band in a capture; the
pixel sample said so before the eye had to argue about it. At 0.17
wide and roughly twice the emission, the hatching reads.

**A measurement habit was corrected here too.** Verification had been
"the score is identical". The score is *not* identical between runs of
the same build — it moves a couple of points, because hype doubles for
a fixed number of beats and the activation frame decides whether one
more note lands inside. The invariant is the judgment: 463 perfect, 0
miss, 0 overstrum, unchanged. [The stage guide](3d-stage.md) now says
so, since the old wording invited exactly the wrong comparison.

# Round six — the neck is a guitar neck, not a light show

Commissioned 2026-09-02: "make the graphics look like Guitar Hero 2 —
research, orient on the screenshots, plan first, then build." The two
attached screenshots arrived as empty JPEG placeholders, so the
research is from public descriptions (the same WikiHero material round
five used) and from the game itself: a fresh window-ID capture of the
current build, measured, set beside what GH2 is documented to do.

## The line, restated

Same as round five. Conventions are the genre's — every guitar game
and the instrument itself share them — and are adopted. Specific
artwork (logos, typefaces, character art, neck motifs, the name) is
one game's and is not. Nothing below copies an asset; every texture
is generated from a hash as before.

## What a side-by-side actually shows

The framing is already there: the solo neck fills ~38 % of a 16:9
frame at the strike line (round five widened it from 31 %), the
vanishing region sits at ~39 % of the height, the HUD occupies the
lower corners, gems are GRYBO, phrases wear stars, hits throw flames.
What still reads as "not that game" is not layout — it is **what the
neck is made of**:

| Trait | GH2 (documented) | BeatByte, measured today | Gap |
|---|---|---|---|
| Neck surface | dark, near-neutral; the gems and buttons are the only colour on it | dark grey-blue board, **five coloured glowing lane lines** full length, glowing coloured rails, glowing coloured trim | **the biggest one** — the neck reads as a neon runway, not an instrument |
| Strings | thin pale division lines between lanes | four dim neutral dividers **plus** the five coloured lines on top | the coloured lines dominate |
| Gems | saturated cap, **dark ring**, **white centre**; HOPO ringless | flat coloured disc with a dark rim; bloom washes it pastel | no centre, low saturation |
| Far end | the neck fades into the stage dark | the neck ends hard against the crowd | notes appear "from the crowd" instead of from the dark |
| HUD plates | neutral dark chrome, white digits, badge multiplier | plates tinted and **outlined in the player colour** (yellow box, purple box) | reads arcade, not stage hardware |
| Hit feedback | flame + button light; **no per-note text** | flame + button light + "PERFECT" over the neck | the word is a different genre's vocabulary |
| Sustain | thin rail, bright core when held | thick pill, pulsing | chunkier than the reference |
| Typography | bold condensed rock face | pixel face in 8-bit; engine fallback in round | round style has no voice of its own |
| Rock meter | gauge lower right | deliberately absent | unchanged — a fail state is mechanics (P3) |
| Band on stage | four characters | none | unchanged — separate track (stage-realism plan) |

## The work

Each item is gated on the 3D stage and, where it changes materials,
on the round style — **the 8-bit mode is untouched**. Each is measured
on a window-ID capture before and after, and the judgment invariant
(perfect / miss / overstrum counts on the same run) is checked with
the autopilot after each.

### R1 — A neutral neck

Lane lines stop glowing in the lane colour and become pale strings:
thin, slightly metallic, one shade for all five. Rails and trim keep
their theme identity but lose most of their emission, so they frame
the neck rather than compete with the gems. The board goes darker and
warmer (theme hue preserved, value lowered) so that the gems are the
brightest thing on it.

*Measure:* mean saturation of the neck region between the rails,
before and after; gem-to-board contrast.

### R2 — Gems are buttons

A white centre on every gem face, a darker ring, and a more saturated
cap — the genre's marking, which the 2D views already carry and the
3D gem does not. HOPOs keep their ringless, smaller form.

*Measure:* luminance profile across a gem (centre bright, ring dark,
cap in between); saturation of the cap.

### R3 — The far end fades

Distance fog on the stage camera, tuned so the last third of the neck
darkens into the venue and notes emerge from the dark. The venue
behind stays visible but recedes.

*Measure:* board luminance at the far end vs at the strike line.

### R4 — HUD plates in stage chrome

Solo plates lose the player-colour outline and tint: dark neutral
plate, white digits, the multiplier a lit badge. Multiplayer keeps the
tint — with four necks the colour is how you find your numbers.

*Measure:* the plate border is no longer the accent hue.

### R5 — Hit labels become a setting

`Hit labels: On / Off` in settings. On stays the default (nothing
changes for anyone who did not ask); Off gives the flame-only feedback
of the reference. The label also moves off the neck centre and shrinks
when on, so it stops sitting where the notes are.

*Measure:* setting round-trips; with Off, no popup entity is spawned.

### R6 — Thinner sustains with a core

Tail width down, a brighter core strip inside it while held.

### R7 — A voice for the round style (last, its own decision)

An OFL bold condensed face for the round style's HUD and menus, bundled
with its licence and recorded in `asset-licenses.md`. Last because it
touches the whole design kit (`ui_kit` owns the type scale) and the
karaoke tracking (`UiFont::glyph_em` must be measured for it), and
because a typeface is the most opinionated change on this list.

## Not done, and why

- **Rock meter** — a fail state is a mechanic, not a look (roadmap P3).
- **Band characters** — a stage-realism item with its own plan.
- **Any GH2 asset** — motifs, fonts, logo, name. The rule, and it is
  not in tension with the brief: what makes the reference read as a
  guitar game is convention, and convention is what is being adopted.

## Round six status

R1–R6 shipped (v0.13.24); R7 is open, see below. Every number is
from a window-ID capture at a pinned 1280×720, same song, same
moment, before and after. The judgment invariant held on every run:
98 perfect, 0 miss, 0 overstrum.

| | Before | After | How measured |
|---|---|---|---|
| Neck saturation (board between the rails) | 0.202 | **0.111** | mean HSV S over the mid-neck band |
| Neck brightness | 0.330 | **0.176** | mean V over the same band |
| Plate border saturation (solo, round) | 0.649 | **0.113** | along the left plate's top edge |
| Crowd brightness (fog, untouched by R1) | 0.194 | **0.148** | left crowd ranks |
| Back wall brightness (fog) | 0.275 | **0.130** | LED-wall band |

The fog number is read off the *venue* rather than the neck on
purpose: R1 darkened the board too, so a board sample could not
attribute its drop to fog. The crowd and the wall were not touched by
anything but the fog, and the wall recedes further than the crowd —
which is the linear falloff doing what it states.

**A pin found a real bug.** `saturate()` — the cap-saturation helper
for R2 — turned a mid grey into pure red, because HSL stores a grey's
hue as 0 and the function pushed the saturation regardless. The
test's "a grey stays grey" clause caught it before any theme with a
neutral lane colour could have.

**One mutation probe missed.** The `shows_label` mutation did not
bite on the first attempt — because `rustfmt` had reflowed the match
arm and the probe's text anchor no longer matched, so the mutation
was never applied. Re-anchored on the real line, the pin failed as
it should. Same lesson as the repository's earlier ones: a mutation
that does not change the file is not a mutation.

**Screenshots.** The two attached reference images arrived as empty
JPEG placeholders and could not be used; the round is grounded in
the documented conventions instead. If there is a specific detail in
them this round missed, they are worth re-sending.

### R7 — open

Not started. It changes every piece of text in the round style, and
the karaoke tracking depends on the face's advance width, so it is a
decision to take on its own rather than the tail of a rendering
round. Candidates that fit the licence rule: Oswald, Anton, Bebas
Neue (all OFL).
