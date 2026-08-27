# The 3D stage

`crates/beatbyte-game/src/gameplay/stage3d.rs` is the largest module in
the game — a little over two thousand lines — and it draws the view
most people play in. This page is the map: what the space means, what
the pieces are, and which rules must not be broken while changing it.

The other renderer, the depth view, is not a legacy path. It draws the
same session with 2D sprites and a perspective projection, and every
change here has to say what it does there.

## The one rule everything else serves

**Nothing in this file may affect judgment.**

The session decides what was hit from stamped input times against the
song clock. This module reads that decision and draws it. The proof is
not an argument, it is a measurement: the same song scores identically
in both views — 624 perfect and 0 miss on a real import, run after
every change in this file since it was written.

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

**The venue** — rear wall with a generated backdrop, side walls, a
truss with sweeping beams, speaker stacks, crowd ranks behind barriers
— is kept outside the bed so it can never occlude an approaching note.
The crowd bobs on the song's own tempo map, each head with its own
phase.

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

## Verifying a change here

```bash
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_MUTE=1 cargo run --release -p beatbyte
```

The score must be identical to before. For looks, capture by window ID
rather than the harness screenshot — an occluded window renders black,
and a full-screen terminal is enough to occlude it. The recipe is in
[the harness reference](../development/harness.md).

`BEATBYTE_SHOT_DIR` adds `gameplay-phrase` and `gameplay-hype` moments,
which exist because the fixed 24–26 s window falls between phrases on
every song in the library.

## Related

- [The UI design system](design-system.md) — menus and settings
- [How the look was arrived at](gameplay-look-plan.md) — four rounds,
  with the measurements and the wrong turns
- [ADR-0004](../decisions/ADR-0004-gameplay-timing.md) — why judgment
  cannot depend on any of this
