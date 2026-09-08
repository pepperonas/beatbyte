# The 3D stage

`crates/beatbyte-game/src/gameplay/stage3d.rs` is the largest module in
the game — some four thousand lines — and it draws the view most
people play in, with four satellites since the realism pass of
2026-09-07: `pa.rs` (the speaker stacks), `rig.rs` (the light rig),
`figure.rs` + `crowd.rs` (the people) and `band.rs` (the four who
play), all on the surfaces `crate::surfaces` bakes. This page is the
map: what the space means, what the pieces are, and which rules must
not be broken while changing it.

The other renderer, the depth view, is not a legacy path. It draws the
same session with 2D sprites and a perspective projection, and every
change here has to say what it does there.

## The one rule everything else serves

**Nothing in this file may affect judgment.**

The session decides what was hit from stamped input times against the
song clock. This module reads that decision and draws it. The proof is
not an argument, it is a measurement: the same song produces the same
**judgment** in both views — 624 perfect and 0 miss on a real import,
run after every change in this file since it was written.

Judgment, not the score. The score itself drifts by a couple of points
between runs of the identical build (measured: 139 968 / 139 970 /
139 971 / 139 972 on four runs, every one of them 463 perfect and 0
miss). Hype doubles for a fixed number of beats, and which frame it is
activated on decides whether one more note falls inside that window.
So compare perfect / miss / overstrum counts, never the score — a
changed score number proves nothing, and an unchanged one would not
have proved anything either.

If a change here alters a score, the change is wrong, however good it
looks.

## The space

| | |
|---|---|
| **+X** | across the neck, left to right |
| **+Y** | up |
| **−Z** | into the screen, toward the horizon |
| **z = 0** | the hit line |

A note `t` seconds in the future sits at `z = -t · scroll_speed ·
Z_PER_PIXEL`. Everything that moves — notes, sustain tails, bar lines,
phrase bands — is placed by that one function, `note_z`, so they cannot
drift apart.

### Two scales, deliberately

```rust
WORLD_PER_PIXEL   // across the neck: 1/220
Z_PER_PIXEL       // down the neck:   HIGHWAY_LENGTH / (2.6 · 420)
```

They answer different questions — *how wide is a lane* and *how long is
a second* — and a compile-time assertion keeps them apart:

```rust
const _: () = assert!(Z_PER_PIXEL > WORLD_PER_PIXEL * 3.0);
```

That assertion exists because the two were once the same value. A note
then took **13.7 seconds** to cross a highway it should cross in 2.6,
which read as the game running in treacle.

### Solo necks are drawn wider

`neck_spread()` returns 1.45 for one player and 1.0 for more. Measured,
a solo neck filled 31 % of the frame where the genre's fills about
half. The factor is applied where the width is actually derived —
`lane_x()` and the `bed_width()` sites — so everything follows from one
number, and it is read off the layout's player count rather than a
flag, so the two cannot disagree.

Two to four necks already use the room, which is why they are left
alone.

## Layers

Everything the stage draws lives on `RenderLayers::layer(STAGE_LAYER)`
and carries `Stage3d` plus `GameplayScreen`, so it despawns with the
screen.

The stage camera runs at **`order: -1`**, which has caught two bugs
worth remembering: the 2D camera draws *over* it, so 2D sprites are a
foreground layer here, not a background. The theme's sprite backdrop
and the hype overlay are both suppressed in this view for that reason —
one was confetti over the fretboard, the other washed a wall forty
units behind the vanishing point while leaving the rails untouched.

## The pieces

**The neck.** Bed, bright rails down both edges, one glowing strip per
lane, and four neutral dividers between them. The dividers are dimmer
than the lane lines on purpose: five coloured lines say where the lanes
*are*, a divider says where one *ends*.

**The board texture** is generated, not loaded (`board_shade`), from a
hash rather than a random number so it is identical every run. Its
brightness is confined to a band by a test, because it has to read as a
surface without competing with what sits on it — and a second test
asserts it is not flat, since a flat texture would satisfy the first
one happily and be a silently missing feature.

**Notes** are a coloured face in a dark rim, lying on the board. The
face is a generated radial texture applied to `base_color_texture`
*and* `emissive_texture`: a gem's look is dominated by its glow, and a
base-colour map alone made them flatter rather than rounder. The face
has a floor so a dark rim cannot dim distant notes out of readability.

**Sustains** are tubes. A struck sustain's tail *survives* the strike
and is eaten from the hit line inward for as long as the engine reports
the hold running — asked of the session, not tracked locally, so a
dropped hold drops the picture with it. Releasing early greys the
remainder and lets it slide away.

**Energy phrases** are marked on the notes (a lit rim; the face keeps
its lane colour, because the fret to press must never be obscured) and
as a tinted band on the neck, so a phrase can be seen coming.

**The receptors** carry two decays — how hard the fret is held, and how
recently a note landed. A held sustain keeps the strike *alive and
breathing* rather than pinned at maximum, because a constant maximum is
a state, not an animation.

**The flame** leaps off the fret on a hit, white-hot at the strike and
cooling to the lane's colour. It lasts about a third of a second, which
is the right length for something that happens on every note and does
mean a screenshot will usually miss it.

**The venue** — rear wall with a generated backdrop, side walls, two
lattice trusses, the stage deck, haze sheets, the LED wall — is kept
outside the bed so it can never occlude an approaching note.

**The deck** (the riser the highway stands on) is planks with seams
and scuffs from `surfaces`: a colour tile, a normal map and a
roughness map whose gloss between the scuffs is what makes the rig's
pools reflect. Everything else on stage stands ON it at `y = −0.30`.

**The PA** (`pa.rs`): two full stacks — sub, two tops, an amp head —
seated on the deck with rubber feet and stacking cleats. Tolex bodies
with a normal map, a raised frame around a recessed grille cloth
(blended, so the driver discs show through as darker circles), metal
corner caps and handles, bass ports that are unlit black, a head with
eight knobs and one accent-coloured LED. No badge. The sub's and the
woofers' cones stroke out on the beat (`pump_drivers`, transforms
only); the tweeters do not.

**The rig** (`rig.rs`): six moving heads on the front truss and four
rim fixtures on the backline. Each is a pivot carrying a housing, a
lens, the additive cone mantles that are the visible shaft in the
haze — and a real `SpotLight` on the same pivot, wearing the mantle's
cone (`cone_of_mantle`, pinned), so what the eye sees in the air and
what lights the deck can never drift apart. The rim lights fire the
accent's complementary tone toward the camera: the edge light that
separates people from the dark. There are no fake floor pools any
more; the pool is the light on the glossy deck.

**The people** (`figure.rs`, `crowd.rs`, `band.rs`): one builder makes
a person at unit height from primitives — pelvis, torso, head with a
hair silhouette, arms with elbows and hands, legs with knees and feet
— under a root whose uniform scale is the height. Fifty-six of them
stand in three staggered rows behind the barriers (row three built
simple), in front of the band's riser, each with a hash-chosen look
and a programme of dance moves that changes on phrase boundaries; the
band are four more from the same builder with instruments hung from
their joints. Every move is a pure function of the song's beat, the
bar and an energy term (Hype, the streak, whether the song is quiet),
written to the joints as transforms; the forward kinematics in the
tests prove the feet stay planted through a squash and no hand
reaches into the bed.

### The monitors (`monitors.rs`)

A dot-matrix panel standing on each amp head — as wide as the head,
three 5×7 digits of emissive dots, the way the reference rig's R4
matrix draws its figures: the left stack reads the tempo in BPM, the
right the level as **dBFS + 100** (the dB-Analyse's convention; the
meter measures dBFS, the offset is display only), both **measured**
from the machine's audio input (`beatbyte-audio::listen`) while the
song plays. No measurement, no monitor — they are spawned the frame
the listener first reports a heard sample and despawned the frame it
stops; there is no blank or zero state. Dots change by visibility
only, and only when a digit changes. (The first cut, seven-segment
cells on the head's face, was reported too small.)

### The light show (`lightshow.rs`)

The level the right monitor shows also drives the room: a threshold
that sets itself (the reference rig's duty governor, ported), a
**strobe** on the ten ceiling lamps while the level is over it, and
five white strips that run a comet or a spray of sparks every 9–18 s
on a hashed schedule.

The strobe flares a pair of lamps white in a shuffled order — every
lamp once per pass through the rig — and it flares the light AND the
fixture's own beam, because the additive mantle is what the eye sees
at a fixture and a light that flashed alone was measured not to move
the near cones' colour at all. It plays in **bursts**: one to three
flashes 0.16 s apart, then a rest of two to three seconds, both
rolled per burst. The threshold gates it (held 150 ms past the last
sample over, because the bit flickers with the music) and the
schedule FREEZES while the ceiling is dark, or a rest would run out
during every quiet passage and the pacing would follow the music
instead of the schedule. Each lamp's own colour and intensity live
in `LampBase`, handed back the frame the level drops. `rig::RigLamp` numbers the
lamps so the chase never depends on query order; the band's key
light carries no such number and never strobes; it and the venue's
colour washes (`VenueWash` on the two coloured point lights and the
crowd fill) dip to 40 % under a flash, because a white spot against
a full-strength wash is not a strobe. REDUCED FLASHING takes the
strobe away and leaves the swell. `BEATBYTE_LIGHTSHOW=1` runs the
whole show flat out, for looking at it without waiting for a loud
passage to coincide with a screenshot.

Strip bars are additive ghosts (`NotShadowCaster`) driven by
visibility and scale, 64 to a strip; a dark bar of an idle strip is
not written at all. A comet enters fast and eases out with a whisker
of bow glow ahead of its head; a sparkle is clusters of two to five
neighbouring bars that flare and **die by dimming** over 0.28 s —
sparks die, they do not switch, and the version that switched read
as television static. STAGE MOTION off leaves every strip dark and
unwritten.

## Materials

The stage's surfaces are baked once at `PreStartup` by
`crate::surfaces` (`StageSurfaces`): tolex, grille cloth, brushed
metal, a driver cone and the deck, each as the tiles a PBR material
takes. Three rules, each broken silently once:

| Rule | What happens otherwise |
|---|---|
| Colour tiles are `Rgba8UnormSrgb`; normal and roughness tiles are `Rgba8Unorm` | a data tile in sRGB is decoded through the gamma curve and every normal leans |
| A mesh with a normal map has tangents (`surfaces::tangent_mesh`) | the map renders flat, with no warning |
| Repeated tiles carry a mip chain (`surfaces::with_mips`) | the deck shimmers at a grazing angle; MSAA is edge anti-aliasing and does nothing for textures |

The roughness tile follows the layout Bevy samples — G = roughness,
B = metallic — and multiplies the material's scalars, so a material
that wants the tile to rule sets both scalars to 1.0.

## Light and shadow

Ambient light is a component **on the stage camera** (`AmbientLight`
requires `Camera` in this Bevy); spawned on its own entity, as it was
until 2026-09-07, it lit nothing and stood in the world as a phantom
camera. The key `DirectionalLight` casts the stage's one shadow map
(two cascades, to 44 units — the rear wall gets none by design), and
every piece decides its role:

- **The neck is a reading surface.** Everything on it — bed, rails,
  strings, fret bars, phrase bands, receptors, notes, tails, flames,
  sparks, arcs — wears `on_the_neck()` (`NotShadowCaster` +
  `NotShadowReceiver`). Without it the right stack's shadow reaches
  across the board.
- **A ghost casts nothing.** Blended and additive meshes cast SOLID
  shadows in this Bevy (the shadow pass discards only masked
  materials): haze sheets, mantles, halos, lenses and the grille cloth
  carry `NotShadowCaster`. A test spawns the venue and checks every
  non-opaque material for the mark.
- People and cabinets cast; the deck receives.

## Traps this module has already sprung

- **A shared material is shared.** Greying one missed note by editing
  its material turned every note in that lane grey for the rest of the
  song. Missed notes swap the *handle* to a dedicated grey material.
- **Emissive is not lighting, but bloom spreads it.** Making the bed
  emissive to tint it for hype washed the entire venue violet. Surfaces
  that are *lit* get no glow lift; only surfaces that already glow do.
- **Eased values belong to the thing, not the entity.** The hype tint
  first advanced its blend once per surface, so the ease rate depended
  on how many surfaces a neck happened to have.
- **Query disjointness is checked at runtime.** Receptors, bursts and
  flames all want `&mut Transform`; the `Without<…>` filters are what
  make them provably different sets, and Bevy panics rather than
  aliasing.
- **`Mesh` has no `Default`.** Use `Sphere::new(r).mesh().uv(n, m)`.
- **A normal map without tangents is flat.** Every mesh that takes one
  goes through `surfaces::tangent_mesh`; merge pieces first, generate
  tangents after (`Mesh::merge` wants identical attribute sets).
- **A blended mesh casts a solid shadow.** Mark every ghost
  `NotShadowCaster` (see *Light and shadow*).
- **`AmbientLight` requires a `Camera`.** It lives on the stage camera,
  never on an entity of its own.
- **`RenderLayers` is per entity, never inherited.** Every child mesh
  and every light carries the stage layer or the stage camera does not
  see it; the figure builder puts it on every joint, and a test walks
  the tree.
- **Joints carry rotation and translation, never scale.** A person's
  height is the root's uniform scale; a scaled, rotated joint shears
  its children.

## Verifying a change here

```bash
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_MUTE=1 cargo run --release -p beatbyte
```

The judgment counts must be identical to before (perfect / miss /
overstrum, never the score). For looks, raise the window and use the
engine's own screenshot (`BEATBYTE_SHOT_DIR`, `BEATBYTE_SHOT_TIMES`),
then check each frame's luma before believing it — an occluded window
renders black or stale, and a full-screen terminal is enough to
occlude it. The recipe is in
[the harness reference](../development/harness.md).

Frame time is measured, not assumed: `BEATBYTE_FPS=1` logs the median
and the 99th percentile every five seconds. The realism pass was
measured against a baseline on the same song and window
(`docs/ROADMAP.md`, *Stage realism II*).

`BEATBYTE_SHOT_DIR` adds `gameplay-phrase` and `gameplay-hype` moments,
which exist because the fixed 24–26 s window falls between phrases on
every song in the library.

## Related

- [The UI design system](design-system.md) — menus and settings
- [How the look was arrived at](gameplay-look-plan.md) — six rounds,
  with the measurements and the wrong turns
- [The stage-realism plan](stage-realism-plan.md) — the club-darkness
  pass of 2026-09-01; its "no figures" exclusion was superseded on
  2026-09-07
- [ADR-0004](../decisions/ADR-0004-gameplay-timing.md) — why judgment
  cannot depend on any of this
