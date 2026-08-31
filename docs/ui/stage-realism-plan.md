# The stage learns what a concert looks like

**Executed 2026-09-01 (v0.12.27)** — all six points below landed;
verification per point in the commit.

Commissioned: make the venue decidedly more attractive and more
real, researched against Guitar Hero II, planned, then built.

## Research

Sourced: GH2's career runs **small dark clubs → theater → arena →
Stonehenge** — the identity venues are bars and cellars ("the Rat
Cellar has a bar feel", [WikiHero: The Rat Cellar](https://guitarhero.fandom.com/wiki/The_Rat_Cellar);
venue list at [WikiHero: Venues in Guitar Hero II](https://guitarhero.fandom.com/wiki/Category:Venues_in_Guitar_Hero_II)).
A dark club is the reference register, not an exhibition hall.

Approximation (marked as such — Fandom/TCRF detail pages were not
retrievable, the register below is standard concert lighting
grammar): a real dark-club stage reads as **blackness with lights
in it** — walls vanish, the crowd is a silhouette mass, haze gives
beams a body, and the colour drama comes from warm-vs-cold
key/backlight contrast, with backlights firing TOWARD the camera.

## The diagnosis of our own screenshots

- **D1 — No darkness.** Walls and backdrop sit at an even mid-grey;
  the room reads as a lit cardboard box. Nothing vanishes.
- **D2 — The crowd is a rock pile.** Grey spheres on a perfect
  grid, mid-brightness — neither people nor silhouettes.
- **D3 — No atmosphere.** Beams and pools sit on the air instead of
  in it; there is no haze for them to live in.
- **D4 — One-colour world.** Everything is the accent tone; concert
  light lives on warm/cold opposition.
- **D5 — No backlight.** The classic stage look is rim light firing
  from behind the band toward the audience. We have none.
- **D6 — The truss is a stick, and the stage has no edge.** Real
  rigs are lattice; real stages are risers with a front edge.

## The plan

1. **Darkness first.** Key light ~5500→2600, ambient 220→90, rear
   fill halved; walls and backdrop pushed considerably darker. The
   emissive carriers (LED wall, lenses, pools) stay — contrast is
   made by darkening the room, not brightening the lamps.
2. **A silhouette crowd.** Each person = torso + head, near-black,
   hash-jittered off the grid in position and height; roughly one
   in four raises an arm. The whole person bobs (the bob moves the
   parent, not just the head).
3. **Haze.** A handful of large, faint, additive soft-dot sheets
   low behind the stage and in the mid-room — static, cheap, and
   what gives the beams a body.
4. **A backline of rim light.** A second lattice truss above the
   LED wall carries four fixtures firing SHORT, wide cones toward
   the camera in the accent's complementary tone (pure
   `complementary()`, pinned) — the warm/cold opposition D4/D5 ask
   for, kept high so it never washes the fretboard.
5. **Lattice trusses.** The main truss becomes two chords with
   diagonal bracing; the fixtures hang from drops.
6. **A stage riser.** A dark platform under the highway's near end
   with a visible front edge — the board stands on a stage, not in
   the air.

Not planned: character models/band figures (asset rule, scope),
venue art or logos (trade dress), fog particles simulation (static
haze sheets carry the look at zero per-frame cost).
