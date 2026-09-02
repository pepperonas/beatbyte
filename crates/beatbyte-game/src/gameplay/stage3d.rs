//! The solid 3D stage: a lit highway with real geometry.
//!
//! This is a third view alongside the flat and depth ones rather than
//! a replacement, for the same reason the depth view was: judgment is
//! input-stamp driven and must not change when the presentation does.
//! A run in 3D has to score exactly what the same run scores in 2D,
//! and keeping all three alive is what makes that testable.
//!
//! Geometry conventions, once, so nothing downstream has to guess:
//!
//! - **+X** is right across the neck, **+Y** is up out of the
//!   highway, **−Z** runs away from the player toward the horizon.
//! - The hit line is `z = 0`. A note `t` seconds in the future sits at
//!   `z = -t * scroll_speed * WORLD_PER_PIXEL`, so the existing
//!   scroll-speed setting keeps meaning what it meant.
//! - One world unit is [`WORLD_PER_PIXEL`] of the 2D layout, so lane
//!   spacing, receptor sizes and scroll speed all carry over instead
//!   of being re-tuned by eye.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use beatbyte_core::Lane;

use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::palette;
use crate::states::AppState;

/// How deep an off-beat line is drawn, against the downbeat's full
/// depth. Thinner rather than absent: the beat should be felt, the bar
/// should be read.
const OFFBEAT_DEPTH: f32 = 0.55;

/// How bright an off-beat line is, against the downbeat's full
/// strength. Four equally loud lines per bar is a ladder, not a ruling.
const OFFBEAT_WEIGHT: f32 = 0.5;

/// World units per pixel of the 2D layout.
///
/// The 2D stage is laid out in a 1280x720 pixel space; dividing by
/// 220 puts a five-lane neck at roughly 2.2 units wide, which is a
/// comfortable size for a perspective camera with a 45° field of
/// view and keeps the numbers readable while tuning.
pub const WORLD_PER_PIXEL: f32 = 1.0 / 220.0;

/// World units per pixel ALONG THE NECK.
///
/// Deliberately not [`WORLD_PER_PIXEL`]: the two axes answer
/// different questions. Across the neck, the scale sets how wide five
/// lanes look. Along it, the scale sets how long a note is on screen
/// before it must be played, and that has to match the 2D views or
/// the game feels like a different game. Sharing one scale made notes
/// take 13.7 s to cross a highway they should cross in
/// [`super::SPAWN_LOOKAHEAD_S`] — they crawled.
///
/// Chosen so that a note covers the highway's full length in exactly the
/// spawn lookahead at the default scroll speed.
pub const Z_PER_PIXEL: f32 = HIGHWAY_LENGTH / (2.6 * 420.0);

/// The two scales answer different questions and must stay separate.
/// A compile-time check rather than a test, because collapsing them
/// again should not build at all — the last time they were shared,
/// notes crawled and it took a screenshot to notice.
const _: () = assert!(Z_PER_PIXEL > WORLD_PER_PIXEL * 3.0);

/// How far ahead the highway is drawn, in world units.
const HIGHWAY_LENGTH: f32 = 26.0;

/// How far behind the hit line the highway continues, so the near
/// edge is never visible as a cut-off.
const HIGHWAY_BEHIND: f32 = 2.5;

/// Where the instrument neck's fog begins and ends, in camera
/// distance. The camera is ~6 from the strike line and ~31 from the
/// far end; the back wall is ~45.
const FOG_START: f32 = 12.0;
const FOG_END: f32 = 52.0;

/// Radius of a note's coloured face, in world units. Large relative
/// to the lane spacing, as in the games this borrows from — a gem
/// nearly fills its lane, which is what makes a chord read as one
/// shape rather than three dots.
const GEM_RADIUS: f32 = 0.17;

/// Render layer for the 3D stage. The HUD camera renders layer 0 and
/// the stage camera renders this one, so neither draws the other's
/// entities — without it the 2D backdrop appears inside the 3D scene.
pub const STAGE_LAYER: usize = 1;

/// Marker for everything belonging to the 3D stage.
#[derive(Component)]
pub struct Stage3d;

/// A note rendered as 3D geometry.
#[derive(Component)]
pub struct Note3d {
    /// Which player's highway this belongs to.
    pub player: usize,
    /// Index into the track's events.
    pub event_index: usize,
    /// Lane, which fixes the x position.
    pub lane: Lane,
}

/// A bar line across the neck, scrolling with the song.
///
/// The fretboard's cross-bars are not decoration: they are the only
/// thing that tells a player how far away a note is in BEATS rather
/// than in pixels, which is what makes a gap readable at speed.
#[derive(Component)]
pub struct FretBar {
    /// Song time this bar sits on.
    pub time_s: f64,
    /// Whether this is the first beat of a bar.
    ///
    /// Every beat gets a line, because a neck ruled only once per bar
    /// gives the eye nothing to keep time against - it reads as a road.
    /// The downbeat is drawn wider and brighter so the bar structure
    /// still stands out of the ruling rather than being lost in it.
    pub downbeat: bool,
}

/// The disc inside a receptor ring.
///
/// The ring alone is too thin to read as "pressed" — the 2D stage
/// learned the same lesson: a button that FILLS is unmistakable,
/// a button that merely glows is haze.
#[derive(Component)]
pub struct ReceptorFill {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
}

/// The flame that leaps off a fret when a note lands on it.
///
/// The genre's signature moment, and the one thing the stage still
/// did not do: a hit produced a flat ring spreading across the board
/// and nothing else. The comment that justified that said the flame
/// "spreads across the board rather than rising off it", which is
/// backwards — it rises.
#[derive(Component)]
pub struct HitFlame {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
    /// 1.0 at the strike, decaying to 0.
    pub life: f32,
}

/// The burst that fires out of a fret when a note lands on it.
#[derive(Component)]
pub struct HitBurst {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
    /// 1.0 at the strike, decaying to 0.
    pub life: f32,
}

/// A receptor rendered as 3D geometry.
#[derive(Component)]
pub struct Receptor3d {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
}

/// Whether the 3D stage is the active view.
#[must_use]
pub fn active(settings: &Settings) -> bool {
    settings.stage_3d
}

/// What the neck is made of.
///
/// The genre's neck is an instrument: a dark, near-neutral board on
/// which the gems and the fret buttons are the only colour, with pale
/// strings between the lanes. The 8-bit style keeps its own idea — a
/// neon runway with a glowing line per lane — because that IS its
/// look, and the rule is that the 8-bit mode stays untouched. Pure —
/// tested, so the gate cannot silently invert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeckStyle {
    /// Five glowing lane lines, bright rails: the 8-bit stage.
    Neon,
    /// Dark board, pale strings, quiet rails: the round style.
    Instrument,
}

/// Which neck the settings ask for.
#[must_use]
pub fn neck_style(settings: &Settings) -> NeckStyle {
    if settings.round_gems {
        NeckStyle::Instrument
    } else {
        NeckStyle::Neon
    }
}

/// The colour of a string on the instrument neck: one pale, slightly
/// warm shade for all five, so lane identity comes from the buttons
/// and the gems — where the player's eye already is — and not from
/// the line the note travels down.
#[must_use]
pub fn string_color(stage: crate::theme::Theme) -> Color {
    stage.background.mix(&Color::srgb(0.86, 0.84, 0.80), 0.62)
}

/// Radius of a gem's white centre, from the gem's own radius.
///
/// The genre's gem, as its own documentation describes it: "in the
/// middle of the coloured note is a white circle; regular notes have
/// a black circle AROUND this white circle, hammer-ons don't" — so a
/// strum note reads as a **black ring on top** with a white point
/// inside it, and a HOPO as a **solid white top**. The strum centre
/// is therefore small (the ring is the feature); the HOPO centre is
/// a cap of its own, most of the smaller face. Both stay inside
/// their faces: the strum face is `gem`, the HOPO face
/// `HOPO_FACE * gem`. Pure — tested.
///
/// The first version put a naked white dot on every gem and the
/// dark ring OUTSIDE the cap — "all buttons now have a white dot" was
/// the user's exact report, and the sources say the ring belongs on
/// the face, around the dot.
#[must_use]
pub fn centre_radius(gem: f32, hopo: bool) -> f32 {
    if hopo {
        gem * HOPO_FACE * 0.68
    } else {
        gem * 0.16
    }
}

/// The black ring on a strum note's face: `(inner, outer)` radii.
/// It starts at the white centre's edge and is wide enough to be the
/// thing the eye lands on. Pure — tested.
#[must_use]
pub fn face_ring_radii(gem: f32) -> (f32, f32) {
    let inner = centre_radius(gem, false);
    (inner, gem * 0.44)
}

/// A HOPO's face radius relative to a strum note's.
const HOPO_FACE: f32 = 0.62;

/// The board of the instrument neck: the theme's hue, pulled well
/// down toward a dark warm wood so the gems are the brightest thing
/// on it. Measured before this existed: the board sat light enough
/// that a yellow gem under bloom read pastel against it.
#[must_use]
pub fn instrument_board_color(stage: crate::theme::Theme) -> Color {
    stage.background.mix(&Color::srgb(0.11, 0.08, 0.06), 0.62)
}

/// Lane centre in world units for a player's highway.
#[must_use]
pub fn lane_x(layout: &HighwayLayout, player: usize, lane: Lane) -> f32 {
    layout.lane_x(player, lane) * WORLD_PER_PIXEL * neck_spread(layout)
}

/// How much wider the 3D neck is drawn than the 2D layout implies.
///
/// Measured against the reference: a solo neck filled 31 % of the
/// frame where the genre's fills about half, which left the eye
/// nothing to do with the other two thirds and made the gems read as
/// beads on a thread rather than buttons on a board.
///
/// **Solo only.** Two to four necks side by side already use the
/// room, and widening them would run them into each other. Taking the
/// count from the layout rather than a flag means the two can never
/// drift apart.
///
/// Deliberately not done by moving the camera in: that magnifies the
/// board but shortens how far up the neck you can see, and reading
/// ahead is the game.
#[must_use]
pub fn neck_spread(layout: &HighwayLayout) -> f32 {
    if layout.players() == 1 { 1.45 } else { 1.0 }
}

/// Distance ahead of the hit line for a note `seconds` away.
#[must_use]
pub fn note_z(seconds: f64, scroll_speed: f32) -> f32 {
    -(seconds as f32) * scroll_speed * Z_PER_PIXEL
}

/// How brightly a held sustain keeps its strike burning, before the
/// breath is added. Below the peak of a fresh hit — holding a note is
/// a sustained state, not a repeated impact.
const SUSTAIN_HIT_FLOOR: f32 = 0.46;
/// How fast that glow breathes, in radians per second.
const SUSTAIN_PULSE_HZ: f32 = 7.5;
/// How fast the ring fades between re-blooms while a hold runs.
const SUSTAIN_BLOOM_RATE: f32 = 2.4;

/// Brightness range the board texture is allowed to occupy.
///
/// A fretboard has to read as a surface without competing with what
/// lies on it: the gems, the lane lines and the hit line all need
/// their contrast. Pinned by a test, because "subtle" is exactly the
/// kind of intent that erodes one tweak at a time.
const BOARD_SHADE: (f32, f32) = (0.72, 1.0);

/// How one cell of the board pattern is shaded, in 0..1.
///
/// Lengthwise grain plus a faint transverse band — the two things
/// that make a plank read as a plank. Deterministic: a hash, not a
/// random number, so the board is the same every run.
#[must_use]
pub fn board_shade(u: f32, v: f32) -> f32 {
    // Grain: fine stripes along the neck, slightly wandering.
    let wander = (v * 9.0).sin() * 0.02;
    let grain = ((u + wander) * 46.0).sin() * 0.5 + 0.5;
    // Bands across it, much softer, to break the stripes up.
    let band = (v * 5.0).sin() * 0.5 + 0.5;
    // Speckle, so neither pattern reads as a printed texture.
    let hash = ((u * 311.7 + v * 191.3).sin() * 43_758.545).fract().abs();
    let mixed = 0.55f32.mul_add(grain, 0.28 * band) + 0.17 * hash;
    let (low, high) = BOARD_SHADE;
    low + (high - low) * mixed.clamp(0.0, 1.0)
}

/// How one cell of a neck border is shaded, in 0..1, for a theme.
///
/// The decorated border is the trait that most makes a fretboard look
/// like one: in the genre, every stage announces itself along the
/// edges of the neck rather than only behind it. These motifs are
/// drawn for this game - a border that says "garage" has to be OUR
/// garage, not a copy of somebody else's - and each is chosen to rhyme
/// with the backdrop its theme already has.
///
/// `u` runs across the strip, `v` along the neck. The motif is carried
/// by `v`, because at this width the strip is read as a rhythm going
/// away from you, not as a picture.
#[must_use]
pub fn rail_shade(theme_id: &str, u: f32, v: f32) -> f32 {
    // Every motif sits on the same cross-section: brighter toward the
    // outer edge, so the strip reads as a bevelled piece of trim
    // rather than a flat decal.
    let bevel = 0.72 + 0.28 * (u * core::f32::consts::PI).sin();
    let motif = match theme_id {
        // Rivets, punched at regular intervals down a plate.
        "garage" => {
            let along = ((v * 22.0).fract() - 0.5).abs() * 2.0;
            let across = ((u - 0.5).abs() * 2.4).min(1.0);
            let head = (1.0 - (along / 0.55).min(1.0)) * (1.0 - across);
            0.30 + 0.70 * head
        }
        // Sawteeth - the studded strap, abstracted to its rhythm.
        "punk" => {
            let saw = (v * 30.0).fract();
            0.22 + 0.78 * (1.0 - (saw * 2.0 - 1.0).abs()).powf(0.6)
        }
        // Chevrons, leaning because a straight bar reads as a fret.
        "metal" => {
            let lean = (v * 26.0 + u * 3.0).fract();
            0.24 + 0.76 * (1.0 - (lean * 2.0 - 1.0).abs()).powf(1.4)
        }
        // Broad clean bands, the way seating tiers stripe an arena.
        "stadium" => {
            let band = ((v * 9.0).sin() * 0.5 + 0.5).powf(1.8);
            0.34 + 0.66 * band
        }
        // Two waves of incommensurable length, so the pattern never
        // visibly repeats as it slides past.
        "psychedelic" => {
            let a = (v * 13.0).sin() * 0.5 + 0.5;
            let b = (v * 20.0 + u * 2.0).sin() * 0.5 + 0.5;
            0.26 + 0.74 * (0.6f32.mul_add(a, 0.4 * b))
        }
        // A measuring scale: a long tick every fourth, short between.
        "cyber" => {
            let cell = (v * 34.0).fract();
            let long = ((v * 34.0) as i32).rem_euclid(4) == 0;
            let reach = if long { 0.85 } else { 0.40 };
            let lit = f32::from(cell < 0.30 && u < reach);
            0.20 + 0.80 * lit
        }
        // An unknown theme still gets a surface, never a flat bar.
        _ => 0.35 + 0.65 * ((v * 18.0).sin() * 0.5 + 0.5),
    };
    (bevel * motif).clamp(0.0, 1.0)
}

/// How one cell of the rear-wall backdrop is shaded, in 0..1.
///
/// Vertical bands with a soft vignette toward the floor — a stage
/// backdrop, not a blank screen. The wall is the largest single
/// surface in the frame and it was reading as an unlit slab hung
/// behind the vanishing point.
#[must_use]
pub fn backdrop_shade(u: f32, v: f32) -> f32 {
    // Bands of two widths, so the rhythm is not a picket fence.
    let wide = (u * 11.0).sin() * 0.5 + 0.5;
    let fine = (u * 37.0).sin() * 0.5 + 0.5;
    // Darker toward the bottom, where the crowd and stacks sit.
    let fall = (v * 1.4).clamp(0.0, 1.0);
    let mixed = 0.22f32.mul_add(fine, 0.55 * wide) + 0.23 * fall;
    0.18 + 0.62 * mixed.clamp(0.0, 1.0)
}

/// Build a greyscale tile from a shading function.
fn shaded_tile(size: usize, shade: impl Fn(f32, f32) -> f32, repeat: bool) -> Image {
    let mut data = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let value = shade(
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let v = (value.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    if repeat {
        image.sampler =
            bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                address_mode_u: bevy::image::ImageAddressMode::Repeat,
                address_mode_v: bevy::image::ImageAddressMode::Repeat,
                ..bevy::image::ImageSamplerDescriptor::linear()
            });
    }
    image
}

/// How one cell of a gem's face is shaded, in 0..1.
///
/// Bright toward the centre, falling off toward the rim, so a gem
/// reads as a lit object rather than a flat disc. Done as a texture
/// rather than a second mesh per note: at these note counts an extra
/// entity each is real cost, and a face is what was missing, not
/// geometry.
#[must_use]
pub fn gem_shade(u: f32, v: f32) -> f32 {
    let (dx, dy) = (u - 0.5, v - 0.5);
    let radius = (dx * dx + dy * dy).sqrt() * 2.0;
    // Highlight offset up-left, where the key light comes from.
    let hx = u - 0.36;
    let hy = v - 0.34;
    let highlight = (1.0 - (hx * hx + hy * hy).sqrt() * 3.4).clamp(0.0, 1.0);
    let body = (1.0 - radius * radius * 0.55).clamp(0.0, 1.0);
    let shaped = (0.62f32.mul_add(body, 0.38 * highlight)).clamp(0.0, 1.0);
    // Lifted off zero. The face modulates the gem's EMISSIVE, so a
    // dark rim dims the whole note at distance — measured, the first
    // version shaped the near gems nicely and made the far ones hard
    // to see, and reading ahead is the game. Shape is worth having
    // only while the note stays legible.
    GEM_FACE_FLOOR + (1.0 - GEM_FACE_FLOOR) * shaped
}

/// How dark a gem's face may get at the rim.
const GEM_FACE_FLOOR: f32 = 0.62;

/// Build the board texture. Generated, never loaded: every asset in
/// this repository has to be original, and a plank is arithmetic.
fn board_texture() -> Image {
    shaded_tile(128, board_shade, true)
}

/// The decorated border strip for a theme.
fn rail_texture(theme_id: &'static str) -> Image {
    shaded_tile(128, move |u, v| rail_shade(theme_id, u, v), true)
}

/// A stage surface that takes the energy tint while hype runs.
///
/// It carries its own resting colours, because tinting is a MIX
/// toward the hype tone and back — reading the current colour to
/// restore it later would drift a little further every frame.
#[derive(Component)]
pub struct HypeTinted {
    /// Owning player, so a solo hype does not light a rival's neck.
    pub player: usize,
    /// The surface's colour with no hype running.
    pub base: Color,
    /// Emissive strength at rest.
    pub base_glow: f32,
    /// Extra emission while hype runs.
    ///
    /// Zero for surfaces that are LIT rather than lighting. The first
    /// version lifted every tinted surface, which made the fretboard
    /// — by far the largest of them — a lamp: measured, the bloom
    /// pass then washed the entire venue violet, including a wall
    /// forty units behind the neck that nothing had tinted.
    pub glow_lift: f32,
    /// How far toward the hype tone this surface goes, 0..1.
    pub reach: f32,
}

// ── Burning edges while Star Power runs ─────────────────────────────
//
// The genre's classic Star-Power tell: the highway's edges catch
// BLUE fire while the boost is active. Ours is a row of additive
// flame cones licking up from each rail — animated purely through
// transforms (the per-frame channel this stage reserves for motion;
// the shared flame material is never written after creation), grown
// in and out with the same eased feel as the hype tint so the fire
// arrives with the color instead of popping.

/// One flame lick on a highway edge.
#[derive(Component)]
pub struct EdgeFlame {
    /// The player whose Hype this flame answers.
    pub player: usize,
    /// Phase offset so the row never moves as one block.
    pub phase: f32,
    /// This lick's resting height scale (raggedness).
    pub base: f32,
}

/// Spacing of the flame licks along the rail.
const EDGE_FLAME_SPACING: f32 = 0.9;
/// A lick's resting height in world units (scaled by the flicker).
/// Sized against the rail's own glow: the first pass at 0.34 was
/// rendered and CONFIRMED on screen coordinates, yet visually
/// drowned in the rail's bloom - fire that must read from the
/// receptor line to mid-neck needs this much body.
const EDGE_FLAME_HEIGHT: f32 = 0.62;
/// The blue the edges burn with (commissioned: blue, the genre's
/// Star-Power color — not the house Hype purple).
const EDGE_FLAME_BLUE: Color = Color::srgb(0.35, 0.65, 1.0);

/// How tall a lick stands right now, as a factor on its rest: two
/// incommensurable sines layered so the row flickers without a
/// visible loop, bounded well away from zero — a flame that blinks
/// out reads as the boost dropping. Pure — tested.
#[must_use]
pub fn flame_lick(seconds: f32, phase: f32) -> f32 {
    let a = (seconds * 9.3 + phase).sin();
    let b = (seconds * 14.1 + phase * 1.7).sin();
    1.0 + 0.24 * a + 0.14 * b
}

/// Drive the edge fire: visible and licking while the player's Hype
/// runs, eased away when it ends. Transforms and visibility only.
pub fn burn_edges_for_hype(
    settings: Res<Settings>,
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut flames: Query<(&EdgeFlame, &mut Transform, &mut Visibility)>,
    mut blend: Local<Vec<f32>>,
) {
    if !active(&settings) {
        return;
    }
    let delta = time.delta_secs();
    // The same 6/s ease the hype tint uses, advanced once per player.
    for (index, player) in &players {
        if blend.len() <= index.0 {
            blend.resize(index.0 + 1, 0.0);
        }
        let target = if player.session.performance().hype_active() {
            1.0
        } else {
            0.0
        };
        blend[index.0] += (target - blend[index.0]) * (6.0 * delta).min(1.0);
    }
    let now = time.elapsed_secs();
    for (flame, mut transform, mut visibility) in &mut flames {
        let grown = blend.get(flame.player).copied().unwrap_or(0.0);
        // Fully out: hide and stop writing transforms - the resting
        // edge look is exactly the pre-existing rails.
        let wanted = if grown < 0.02 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if wanted == Visibility::Hidden {
            continue;
        }
        let height = EDGE_FLAME_HEIGHT * flame.base * flame_lick(now, flame.phase) * grown;
        transform.scale = Vec3::new(grown, height, grown);
        // The cone's origin is its centre: keep the BASE seated on
        // the rail while the tip licks upward.
        transform.translation.y = 0.015 + height / 2.0;
    }
}

/// Wash the neck with the energy colour while hype is running — and
/// turn the notes themselves toward it, the genre's star-power
/// signature (all notes change colour while the power runs).
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn tint_stage_for_hype(
    settings: Res<Settings>,
    time: Res<Time>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    surfaces: Query<(&HypeTinted, &MeshMaterial3d<StandardMaterial>)>,
    assets: Option<Res<NoteAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blend: Local<Vec<f32>>,
    mut written: Local<Vec<f32>>,
) {
    if !active(&settings) {
        return;
    }
    let delta = time.delta_secs();
    // The eased value is advanced ONCE per player, before any surface
    // is painted. Advancing it inside the surface loop made the ease
    // rate depend on how many surfaces a neck happens to have — three
    // today, and silently faster the moment one is added.
    for (index, player) in &players {
        if blend.len() <= index.0 {
            blend.resize(index.0 + 1, 0.0);
        }
        let target = if player.session.performance().hype_active() {
            1.0
        } else {
            0.0
        };
        blend[index.0] += (target - blend[index.0]) * (6.0 * delta).min(1.0);
    }
    // Touching a material marks it changed and re-uploads it, so a
    // settled blend must write NOTHING — before this gate, every
    // tinted surface was re-uploaded every frame of every song,
    // hype or no hype.
    let moved = |written: &mut Vec<f32>, slot: usize, eased: f32| {
        if written.len() <= slot {
            written.resize(slot + 1, f32::NAN);
        }
        let changed = (written[slot] - eased).abs() > 0.0005 || written[slot].is_nan();
        if changed {
            written[slot] = eased;
        }
        changed
    };
    for (surface, material) in &surfaces {
        let Some(&eased) = blend.get(surface.player) else {
            continue;
        };
        if !moved(&mut written, surface.player, eased) {
            continue;
        }
        let amount = eased * surface.reach;
        if let Some(mut paint) = materials.get_mut(&material.0) {
            paint.base_color = surface.base.mix(&palette::HYPE, amount);
            let glow = surface.glow_lift.mul_add(amount, surface.base_glow);
            paint.emissive = paint.base_color.to_linear() * glow;
        }
    }
    // Solo only: the lane materials are shared by every player's
    // gems, so in multiplayer one player's hype would recolour the
    // other's notes — there the neck wash alone carries the state.
    let mut solo_players = players.iter();
    let (solo, more) = (solo_players.next(), solo_players.next());
    if let (Some((index, _)), None, Some(assets)) = (solo, more, assets) {
        let eased = blend.get(index.0).copied().unwrap_or(0.0);
        if moved(
            &mut written,
            crate::multiplayer::MAX_PLAYERS + index.0,
            eased,
        ) {
            for (lane, handle) in Lane::ALL.iter().zip(&assets.lane_material) {
                if let Some(mut paint) = materials.get_mut(handle) {
                    let colour = theme.0.lane_color(*lane).mix(&palette::HYPE, eased * 0.75);
                    paint.base_color = colour;
                    paint.emissive = colour.to_linear() * 2.2;
                }
            }
        }
    }
}

/// A stretch of neck carrying an energy phrase.
#[derive(Component)]
pub struct PhraseBand {
    /// Owning player.
    pub player: usize,
    /// Phrase bounds on the song timeline.
    pub start_s: f64,
    /// Phrase end.
    pub end_s: f64,
}

/// Lay a tinted band over every energy phrase, so a run of marked
/// notes can be seen coming rather than recognised as it arrives.
///
/// All bands are spawned once — a song has a handful — and simply
/// move with the neck. Spawning them on approach would need a cursor
/// per player and buys nothing at this count.
pub fn spawn_phrase_bands(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let band = meshes.add(Cuboid::new(1.0, 0.008, 1.0));
    let material = materials.add(StandardMaterial {
        base_color: palette::HYPE.with_alpha(0.14),
        emissive: palette::HYPE.to_linear() * 0.5,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    for (index, player) in &players {
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.12 * neck_spread(&layout);
        for phrase in player.session.track().phrases() {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                PhraseBand {
                    player: index.0,
                    start_s: phrase.start_s,
                    end_s: phrase.end_s,
                },
                Mesh3d(band.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(layout.origin(index.0) * WORLD_PER_PIXEL, 0.012, 0.0)
                    .with_scale(Vec3::new(width, 1.0, 0.0)),
                Visibility::Hidden,
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }
}

/// Slide the phrase bands with the neck and hide the ones off it.
pub fn move_phrase_bands(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut bands: Query<(&PhraseBand, &mut Transform, &mut Visibility)>,
) {
    if !active(&settings) {
        return;
    }
    let Some(now) = game_clock.visual_time(&time, &settings) else {
        return;
    };
    for (band, mut transform, mut visibility) in &mut bands {
        let near = note_z(band.start_s - now, settings.scroll_speed);
        let far = note_z(band.end_s - now, settings.scroll_speed);
        // Clipped to the drawn highway, so a long phrase does not
        // stretch a band past the end of the neck.
        let near = near.min(1.0);
        let far = far.max(-HIGHWAY_LENGTH);
        let length = near - far;
        if length <= 0.0 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        transform.translation.z = (near + far) / 2.0;
        transform.scale.z = length;
    }
}

/// Whether a note at this time belongs to an energy phrase.
///
/// Phrases are sorted and non-overlapping (a `Track` invariant), and
/// their bounds are INCLUSIVE — a note exactly on the last instant of
/// a phrase is part of it, and marking it is what makes the run of
/// marked notes end where the meter step happens.
#[must_use]
pub fn in_energy_phrase(phrases: &[beatbyte_core::Phrase], time_s: f64) -> bool {
    phrases.iter().any(|phrase| phrase.contains(time_s))
}

/// The tube behind a sustain's gem.
///
/// Marked, because a hit sustain must survive the strike: the gem
/// lands and goes, but the tail is the part still being played.
#[derive(Component)]
pub struct SustainTail3d;

/// The pale core strip inside a sustain tail (instrument neck). Also
/// a tail — it scrolls, shrinks and greys with the tail — but the
/// held-tail throb must light it pale, not lane-coloured.
#[derive(Component)]
pub struct SustainCore;

/// The core's colour: the lane's, pulled most of the way to white.
#[must_use]
pub fn core_color(lane: &Lane, stage: crate::theme::Theme) -> Color {
    stage.lane_color(*lane).mix(&Color::WHITE, 0.7)
}

/// Where a sustain's remaining tail sits, and how long it still is.
///
/// Returns `None` once nothing is left to hold. Pure, because the
/// half-length offset is exactly the kind of arithmetic that has gone
/// wrong here before — the depth view once drew tails that stood
/// A lane's resting emission, the value the pulse multiplies.
fn base_emissive(lane: &Lane, stage: crate::theme::Theme) -> LinearRgba {
    stage.lane_color(*lane).to_linear()
}

/// How brightly a sustain glows while it is being held, as a
/// multiplier on its resting emission.
///
/// A held sustain used to show only by getting shorter, which is the
/// one thing a player cannot watch: their eyes are at the hit line.
/// This gives the tail a fast throb so holding a note LOOKS like
/// playing one.
///
/// Fast enough to read as energy rather than as breathing, and never
/// reaching zero - a tail that blinks out would read as a dropped
/// hold, which is a different thing entirely.
#[must_use]
pub fn sustain_pulse(seconds: f64) -> f32 {
    const RATE_HZ: f64 = 7.0;
    let wave = (seconds * RATE_HZ * core::f64::consts::TAU).sin() * 0.5 + 0.5;
    1.6f32 + 2.4 * wave as f32
}

/// vertical while the lane leaned.
#[must_use]
pub fn sustain_tail_span(
    time_s: f64,
    sustain_s: f64,
    now: f64,
    scroll_speed: f32,
) -> Option<(f32, f32)> {
    let end_z = note_z(time_s + sustain_s - now, scroll_speed);
    // The head end is pinned to the hit line once the note has been
    // struck: the part that has already been played is gone, so the
    // tail runs from z = 0 back up the neck.
    let head_z = note_z(time_s - now, scroll_speed).min(0.0);
    let length = head_z - end_z;
    if length <= 0.0 {
        return None;
    }
    Some(((head_z + end_z) / 2.0, length))
}

/// One head in the crowd, with its own phase so the ranks do not
/// bounce as one block.
#[derive(Component)]
pub struct CrowdHead {
    /// Radians of offset into the beat.
    pub phase: f32,
    /// Resting height, so the bob is an offset and not a drift.
    pub rest: f32,
}

/// Bob the crowd on the beat.
///
/// The ranks were a static grey mass in a room that is otherwise
/// moving. Driven from the song's own tempo map rather than a free
/// timer, so the room is on the beat the player is playing to.
pub fn bob_crowd(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mut heads: Query<(&CrowdHead, &mut Transform)>,
) {
    if !active(&settings) || !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player)) = (game_clock.song_time(&time), players.iter().next()) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    for (head, mut transform) in &mut heads {
        // Half a beat up, half a beat down, and never below the rest
        // height — a crowd jumps, it does not sink into the floor.
        let swing = (beats * core::f32::consts::PI + head.phase).sin().max(0.0);
        transform.translation.y = 0.22f32.mul_add(swing, head.rest);
    }
}

/// One panel of the LED wall behind the stage.
#[derive(Component)]
pub struct LedPanel {
    /// The panel's own phase, so the wall ripples instead of
    /// slamming as one block.
    pub phase: f32,
    /// Resting scale (the pulse multiplies it).
    pub rest: f32,
}

/// The LED wall's scale pulse on the beat: a quick swell that never
/// dips below rest. Pure — the wall must breathe with the song, and
/// the arithmetic should be provable without a stage.
#[must_use]
pub fn led_pulse(beats: f32, phase: f32) -> f32 {
    let swing = (beats * core::f32::consts::PI + phase).sin().max(0.0);
    0.16f32.mul_add(swing, 1.0)
}

/// Swell the LED wall with the beat — transforms only: a per-frame
/// material write would re-upload the material every frame, the
/// exact waste the hype tint just got rid of.
pub fn pulse_led_wall(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mut panels: Query<(&LedPanel, &mut Transform)>,
) {
    if !active(&settings) || !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player)) = (game_clock.song_time(&time), players.iter().next()) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    for (panel, mut transform) in &mut panels {
        let scale = panel.rest * led_pulse(beats, panel.phase);
        transform.scale = Vec3::new(scale, scale, 1.0);
    }
}

/// The positions of one ring of a beam cone's base, radius 1 at
/// height −1, `segments` points around. Pure — the mantle's UV seam
/// and winding live here, and both have bitten before in hand-typed
/// mesh data.
fn beam_ring(segments: usize) -> Vec<[f32; 3]> {
    (0..=segments)
        .map(|i| {
            let angle = core::f32::consts::TAU * (i as f32) / (segments as f32);
            [angle.cos(), -1.0, angle.sin()]
        })
        .collect()
}

/// A unit light-cone MANTLE: apex at the origin, base ring of radius
/// 1 at y = −1, no cap. UVs run u around the shaft and v from apex
/// (0) to base (1), which is what the beam gradient texture expects
/// — the engine's stock cone centres its origin and buries its UV
/// layout, and a beam has to hang from its lamp.
fn beam_cone_mesh(segments: usize) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let ring = beam_ring(segments);
    // Apex vertices: one per segment so each triangle gets a clean
    // u coordinate (a shared apex smears the texture at the tip).
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    for (i, base) in ring.iter().enumerate() {
        let u = i as f32 / segments as f32;
        positions.push([0.0, 0.0, 0.0]);
        uvs.push([u, 0.0]);
        positions.push(*base);
        uvs.push([u, 1.0]);
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()]; // unlit: unused
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..segments as u32 {
        let apex = i * 2;
        let base = i * 2 + 1;
        let next_base = (i + 1) * 2 + 1;
        indices.extend([apex, base, next_base]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// A piece of the venue behind the neck.
#[derive(Component)]
struct Venue;

/// A moving spotlight beam, with its own phase so the rig does not
/// sweep in lockstep.
#[derive(Component)]
struct SpotBeam {
    /// The beam's resting tilt. Kept, because the sweep is a swing
    /// AROUND it — the first version assigned the swing straight to
    /// the rotation and threw the rig's fan-out away on frame one.
    base: f32,
    phase: f32,
    speed: f32,
}

/// How far back the rear wall stands. Beyond the neck's far end, so
/// it can never occlude an approaching note.
const VENUE_BACK: f32 = -40.0;
/// Half the distance between the side walls.
const VENUE_SIDE: f32 = 9.0;

/// Build the room the neck sits in.
///
/// Before this, the 3D stage was a fretboard in a void: outside the
/// bed the screen was black, and the only thing in it was the 2D
/// sprite backdrop, which the stage camera renders BEHIND rather than
/// in front of — so it read as specks over the board.
///
/// This is deliberately simple geometry — walls, a truss, cones,
/// boxes — tinted from the active theme. There is no band and no
/// venue art, because every asset in this repository has to be
/// original or CC0, and a stage that reads as "a room with lights" is
/// what the space actually needs.
fn spawn_venue(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shapes: &crate::shapes::LaneShapes,
    stage: crate::theme::Theme,
    motion: bool,
) {
    let dark = stage.background.mix(&Color::BLACK, 0.35);
    // Deep, not grey. The first attempt lit the rear wall to roughly
    // the brightness of the fretboard, so it read as a blank screen
    // hung behind the vanishing point and flattened the contrast of
    // the notes furthest away.
    let backdrop = images.add(shaded_tile(128, backdrop_shade, false));
    let wall_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.62).mix(&stage.accent, 0.06),
        perceptual_roughness: 0.9,
        ..default()
    });
    let backdrop_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.45).mix(&stage.accent, 0.10),
        base_color_texture: Some(backdrop),
        perceptual_roughness: 0.92,
        ..default()
    });
    let box_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::WHITE, 0.16),
        perceptual_roughness: 0.75,
        ..default()
    });

    // Rear wall. Wide and tall enough to fill the frame at this
    // distance: at 45 units out with a 50° field of view the visible
    // height is roughly 42 units, so anything smaller shows its edges.
    let wall = meshes.add(Cuboid::new(90.0, 46.0, 0.6));
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(wall),
        MeshMaterial3d(backdrop_material),
        Transform::from_xyz(0.0, 12.0, VENUE_BACK),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // A stage floor. Everything here — speakers, barriers, crowd —
    // used to float over a void; a floor is also what the light
    // pools land on, and its faint sheen is what sells them.
    let floor = meshes.add(Cuboid::new(90.0, 0.3, 64.0));
    let floor_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.55),
        perceptual_roughness: 0.42,
        metallic: 0.12,
        ..default()
    });
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(floor),
        MeshMaterial3d(floor_material),
        Transform::from_xyz(0.0, -1.45, -18.0),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // Haze (stage-realism plan P3): a few large, faint, additive
    // soft sheets low in the room. Static — the beams get a body to
    // live in at zero per-frame cost.
    let haze_quad = meshes.add(Rectangle::new(46.0, 12.0));
    let haze_material = materials.add(StandardMaterial {
        base_color: stage.accent.mix(&Color::WHITE, 0.3).with_alpha(0.045),
        base_color_texture: Some(shapes.soft_dot()),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for (y, z, width) in [(1.5, -36.0, 1.0f32), (2.5, -26.0, 0.8), (1.0, -17.0, 0.6)] {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Venue,
            Mesh3d(haze_quad.clone()),
            MeshMaterial3d(haze_material.clone()),
            Transform::from_xyz(0.0, y, z).with_scale(Vec3::new(width, 1.0, 1.0)),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }

    // The stage riser (P6): the highway STANDS on something — a
    // dark platform with a visible front edge, instead of a board
    // floating in the void.
    let riser = meshes.add(Cuboid::new(13.0, 0.9, 30.0));
    let riser_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.7),
        perceptual_roughness: 0.7,
        ..default()
    });
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(riser),
        MeshMaterial3d(riser_material),
        Transform::from_xyz(0.0, -0.75, -14.0),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // An LED wall instead of the old accent band (which read as a
    // stray red line and was removed on report): a grid of dim
    // emissive panels well above the horizon, swelling with the
    // beat. Two alternating tones so the wall has texture at rest;
    // the pulse is pure transform — no per-frame material writes.
    // The panels sit on a dark cabinet board, and each one carries
    // the LED-module dot matrix in its base AND emissive texture —
    // the pixel structure is what tells a screen from a lamp.
    let cabinet = meshes.add(Cuboid::new(38.0, 7.4, 0.2));
    let cabinet_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.65),
        perceptual_roughness: 0.8,
        ..default()
    });
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(cabinet),
        MeshMaterial3d(cabinet_material),
        Transform::from_xyz(0.0, 8.6, VENUE_BACK + 0.35),
        RenderLayers::layer(STAGE_LAYER),
    ));
    let panel = meshes.add(Cuboid::new(3.2, 1.5, 0.15));
    let panel_bright = materials.add(StandardMaterial {
        base_color: dark.mix(&stage.accent, 0.5),
        base_color_texture: Some(shapes.led_module()),
        emissive: stage.accent.to_linear() * 0.55,
        emissive_texture: Some(shapes.led_module()),
        perceptual_roughness: 0.6,
        ..default()
    });
    let panel_dim = materials.add(StandardMaterial {
        base_color: dark.mix(&stage.accent, 0.28),
        base_color_texture: Some(shapes.led_module()),
        emissive: stage.accent.to_linear() * 0.22,
        emissive_texture: Some(shapes.led_module()),
        perceptual_roughness: 0.7,
        ..default()
    });
    for row in 0..3 {
        for column in 0..9 {
            let x = (column as f32 - 4.0) * 4.0;
            let y = 6.5 + row as f32 * 2.1;
            let checker = (row + column) % 2 == 0;
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                LedPanel {
                    // Phase runs outward from the middle, so the
                    // wall ripples from the centre like a wave.
                    phase: (column as f32 - 4.0).abs() * 0.55,
                    rest: 1.0,
                },
                Mesh3d(panel.clone()),
                MeshMaterial3d(if checker {
                    panel_bright.clone()
                } else {
                    panel_dim.clone()
                }),
                Transform::from_xyz(x, y, VENUE_BACK + 0.5),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }

    // Side walls, well outside the bed so they frame without crowding.
    let side = meshes.add(Cuboid::new(0.6, 30.0, 46.0));
    for sign in [-1.0f32, 1.0] {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Venue,
            Mesh3d(side.clone()),
            MeshMaterial3d(wall_material.clone()),
            Transform::from_xyz(sign * VENUE_SIDE, 6.0, VENUE_BACK / 2.0 + 4.0),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }

    // Lighting truss overhead — LATTICE, not a stick (P5): two
    // chords with diagonal bracing, the way real rigs are built.
    let truss_len = VENUE_SIDE * 1.7;
    let chord = meshes.add(Cuboid::new(truss_len, 0.12, 0.12));
    let brace = meshes.add(Cuboid::new(0.08, 0.55, 0.08));
    let spawn_lattice =
        |commands: &mut Commands, materials_handle: &Handle<StandardMaterial>, y: f32, z: f32| {
            for dy in [0.0f32, 0.45] {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Venue,
                    Mesh3d(chord.clone()),
                    MeshMaterial3d(materials_handle.clone()),
                    Transform::from_xyz(0.0, y + dy, z),
                    RenderLayers::layer(STAGE_LAYER),
                ));
            }
            let braces = 10;
            for i in 0..braces {
                let x = ((i as f32 + 0.5) / braces as f32 - 0.5) * truss_len;
                let lean = if i % 2 == 0 { 0.5 } else { -0.5 };
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Venue,
                    Mesh3d(brace.clone()),
                    MeshMaterial3d(materials_handle.clone()),
                    Transform::from_xyz(x, y + 0.225, z).with_rotation(Quat::from_rotation_z(lean)),
                    RenderLayers::layer(STAGE_LAYER),
                ));
            }
        };
    spawn_lattice(commands, &box_material, 9.0, -13.0);

    // The light rig. Each fixture is a PIVOT hanging from the truss
    // — a moving-head housing with a bright lens, and under it a
    // pair of nested cone mantles wearing the beam-gradient texture,
    // additively blended: dense at the lamp, dissolving into the
    // air, with faint striations around the shaft. The old stock
    // cones were uniform alpha-blended triangles that read as
    // coloured glass; a light is a THING with a source, a hot core
    // and a soft sheath, and it swings from its hanger, not around
    // its own middle.
    let mantle = meshes.add(beam_cone_mesh(28));
    let housing = meshes.add(Cuboid::new(0.34, 0.5, 0.34));
    let lens = meshes.add(Sphere::new(0.13).mesh().uv(10, 8));
    let housing_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.5),
        perceptual_roughness: 0.55,
        metallic: 0.4,
        ..default()
    });
    // Every second fixture runs a paler, whiter tone — a rig of six
    // identical colours reads as a texture, two tones read as lamps.
    let tones = [stage.accent, stage.accent.mix(&Color::WHITE, 0.45)];
    let beam_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|tone| {
            materials.add(StandardMaterial {
                base_color: tone.with_alpha(0.16),
                base_color_texture: Some(shapes.beam_gradient()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    let lens_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|tone| {
            materials.add(StandardMaterial {
                base_color: *tone,
                emissive: tone.to_linear() * 6.0,
                unlit: true,
                ..default()
            })
        })
        .collect();
    let halo_quad = meshes.add(Rectangle::new(1.5, 1.5));
    let halo_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|tone| {
            materials.add(StandardMaterial {
                base_color: tone.with_alpha(0.55),
                base_color_texture: Some(shapes.soft_dot()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    let spot_quad = meshes.add(Rectangle::new(3.4, 2.2));
    let spot_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|tone| {
            materials.add(StandardMaterial {
                base_color: tone.with_alpha(0.30),
                base_color_texture: Some(shapes.soft_dot()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    // The backline (P4): a second lattice above the LED wall with
    // four fixtures firing SHORT, wide cones toward the camera in
    // the accent's complementary tone — the warm/cold opposition a
    // one-colour rig never has, kept high so it rims the room
    // without washing the fretboard.
    spawn_lattice(commands, &box_material, 12.6, VENUE_BACK + 1.6);
    let rim_tone = complementary(stage.accent);
    let rim_material = materials.add(StandardMaterial {
        base_color: rim_tone.with_alpha(0.10),
        base_color_texture: Some(shapes.beam_gradient()),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let rim_lens_material = materials.add(StandardMaterial {
        base_color: rim_tone,
        emissive: rim_tone.to_linear() * 5.0,
        unlit: true,
        ..default()
    });
    for i in 0..4 {
        let x = ((i as f32 + 0.5) / 4.0 - 0.5) * 22.0;
        let pivot = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                // Tipped toward the audience: the shaft leans out of
                // the wall plane instead of hanging straight down.
                Transform::from_xyz(x, 12.5, VENUE_BACK + 1.8)
                    .with_rotation(Quat::from_rotation_x(-0.55)),
                Visibility::default(),
                RenderLayers::layer(STAGE_LAYER),
            ))
            .id();
        commands.entity(pivot).with_children(|fixture| {
            fixture.spawn((
                Mesh3d(housing.clone()),
                MeshMaterial3d(housing_material.clone()),
                Transform::from_xyz(0.0, -0.2, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            fixture.spawn((
                Mesh3d(lens.clone()),
                MeshMaterial3d(rim_lens_material.clone()),
                Transform::from_xyz(0.0, -0.45, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            fixture.spawn((
                Mesh3d(mantle.clone()),
                MeshMaterial3d(rim_material.clone()),
                Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(1.6, 9.0, 1.6)),
                RenderLayers::layer(STAGE_LAYER),
            ));
        });
    }

    // Three a side, none closer to the centre than the speaker
    // stacks — the neck keeps a clear corridor.
    for index in 0..6 {
        let side = if index % 2 == 0 { -1.0 } else { 1.0 };
        let x = side * (3.4 + 1.7 * (index / 2) as f32);
        let tone = index % 2;
        let pivot = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                Transform::from_xyz(x, 8.9, -13.0).with_rotation(Quat::from_rotation_z(x * 0.05)),
                Visibility::default(),
                RenderLayers::layer(STAGE_LAYER),
            ))
            .id();
        if motion {
            commands.entity(pivot).insert(SpotBeam {
                base: x * 0.05,
                phase: index as f32 * 1.1,
                speed: 0.35 + 0.05 * index as f32,
            });
        }
        // The pool the shaft throws on the floor: an additive soft
        // ellipse whose x follows the SAME beam_angle as the sweep.
        let drop = 8.9 - (-1.3);
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Venue,
            FloorSpot {
                base: x * 0.05,
                phase: index as f32 * 1.1,
                speed: 0.35 + 0.05 * index as f32,
                pivot_x: x,
                drop,
            },
            Mesh3d(spot_quad.clone()),
            MeshMaterial3d(spot_materials[tone].clone()),
            Transform::from_xyz(x, -1.28, -13.0)
                .with_rotation(Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2)),
            RenderLayers::layer(STAGE_LAYER),
        ));
        commands.entity(pivot).with_children(|fixture| {
            fixture.spawn((
                Mesh3d(housing.clone()),
                MeshMaterial3d(housing_material.clone()),
                Transform::from_xyz(0.0, -0.25, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            // A soft halo around the lens: a lamp blooms in air.
            fixture.spawn((
                Mesh3d(halo_quad.clone()),
                MeshMaterial3d(halo_materials[tone].clone()),
                Transform::from_xyz(0.0, -0.52, 0.05),
                RenderLayers::layer(STAGE_LAYER),
            ));
            fixture.spawn((
                Mesh3d(lens.clone()),
                MeshMaterial3d(lens_materials[tone].clone()),
                Transform::from_xyz(0.0, -0.52, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            // The hot core and the soft sheath: same mantle, same
            // gradient, different girth — their addition is what
            // fakes the volumetric falloff across the shaft.
            fixture.spawn((
                Mesh3d(mantle.clone()),
                MeshMaterial3d(beam_materials[tone].clone()),
                Transform::from_xyz(0.0, -0.55, 0.0).with_scale(Vec3::new(0.42, 7.6, 0.42)),
                RenderLayers::layer(STAGE_LAYER),
            ));
            fixture.spawn((
                Mesh3d(mantle.clone()),
                MeshMaterial3d(beam_materials[tone].clone()),
                Transform::from_xyz(0.0, -0.55, 0.0).with_scale(Vec3::new(1.05, 7.9, 1.05)),
                RenderLayers::layer(STAGE_LAYER),
            ));
        });
    }

    // Speaker stacks flanking the near end: they give the neck a
    // sense of scale. Real PA is near-black boxes with DRIVERS in
    // the front — the sub at the bottom carries one big cone, the
    // tops a woofer and tweeter — and the fronts breathe with the
    // beat, because a PA that stands dead still gives a stage away.
    let cab = meshes.add(Cuboid::new(1.3, 0.9, 1.1));
    let cab_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.6),
        perceptual_roughness: 0.85,
        ..default()
    });
    let front = meshes.add(Rectangle::new(1.14, 0.76));
    let front_materials: Vec<Handle<StandardMaterial>> = [true, false]
        .iter()
        .map(|sub| {
            materials.add(StandardMaterial {
                base_color: dark.mix(&Color::WHITE, 0.55),
                base_color_texture: Some(if *sub {
                    shapes.speaker_sub()
                } else {
                    shapes.speaker_top()
                }),
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .collect();
    for sign in [-1.0f32, 1.0] {
        for level in 0..3 {
            let position = Vec3::new(sign * 4.4, 0.42 + 0.95 * level as f32, -7.0);
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                Mesh3d(cab.clone()),
                MeshMaterial3d(cab_material.clone()),
                Transform::from_translation(position),
                RenderLayers::layer(STAGE_LAYER),
            ));
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                WooferFront {
                    phase: sign.mul_add(0.6, level as f32 * 0.8),
                },
                Mesh3d(front.clone()),
                MeshMaterial3d(front_materials[usize::from(level > 0)].clone()),
                Transform::from_translation(position + Vec3::new(0.0, 0.0, 0.56)),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }

    // Crowd. The first attempt was loose spheres at varying heights,
    // which read as scattered rubble rather than people: a head needs
    // something to stand behind. Each side gets a dark barrier, and
    // the heads sit in a line just above its top edge.
    let barrier = meshes.add(Cuboid::new(0.5, 1.1, 22.0));
    let barrier_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.7),
        perceptual_roughness: 1.0,
        ..default()
    });
    for sign in [-1.0f32, 1.0] {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Venue,
            Mesh3d(barrier.clone()),
            MeshMaterial3d(barrier_material.clone()),
            Transform::from_xyz(sign * 2.9, -0.5, -22.0),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }
    // A crowd is a SILHOUETTE mass, not a rock pile: near-black
    // people (torso + head, one in four with an arm up), jittered
    // off the grid in place and height by the deterministic hash —
    // a perfect grid is what gave the spheres away as props. The
    // whole person bobs: CrowdHead sits on the parent and the bob
    // moves everything it carries.
    let head = meshes.add(Sphere::new(0.30).mesh().uv(8, 6));
    let torso = meshes.add(
        Capsule3d::new(0.26, 0.62)
            .mesh()
            .latitudes(6)
            .longitudes(10),
    );
    let arm = meshes.add(Cuboid::new(0.10, 0.7, 0.10));
    let silhouette_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.82),
        perceptual_roughness: 1.0,
        ..default()
    });
    for index in 0..48u32 {
        let row = index % 3;
        let seat = index / 6;
        let sign = if index % 2 == 0 { -1.0f32 } else { 1.0 };
        let jitter = |salt: usize| super::fx::hash01(index as usize * 97 + salt) - 0.5;
        let x = sign * (3.2 + 0.85 * row as f32) + 0.5 * jitter(1);
        let z = 0.6f32.mul_add(row as f32, (-2.4f32).mul_add(seat as f32, -13.5)) + 0.7 * jitter(2);
        let rest = 0.16f32.mul_add(row as f32, 0.12) + 0.14 * jitter(3);
        let person = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                CrowdHead {
                    // Spread through the beat by seat, so the ranks
                    // ripple instead of pumping as one block.
                    phase: index as f32 * 0.7,
                    rest,
                },
                Transform::from_xyz(x, rest, z),
                Visibility::default(),
                RenderLayers::layer(STAGE_LAYER),
            ))
            .id();
        commands.entity(person).with_children(|body| {
            body.spawn((
                Mesh3d(torso.clone()),
                MeshMaterial3d(silhouette_material.clone()),
                Transform::from_xyz(0.0, -0.30, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            body.spawn((
                Mesh3d(head.clone()),
                MeshMaterial3d(silhouette_material.clone()),
                Transform::from_xyz(0.0, 0.42, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            if index % 4 == 0 {
                body.spawn((
                    Mesh3d(arm.clone()),
                    MeshMaterial3d(silhouette_material.clone()),
                    Transform::from_xyz(sign * 0.30, 0.55, 0.0)
                        .with_rotation(Quat::from_rotation_z(sign * -0.25)),
                    RenderLayers::layer(STAGE_LAYER),
                ));
            }
        });
    }
}

/// Push a colour toward full saturation by `amount` (0 = unchanged,
/// 1 = fully saturated), keeping its hue and lightness. Pure —
/// tested: a grey stays grey, and the result is never less saturated
/// than the input.
#[must_use]
pub fn saturate(color: Color, amount: f32) -> Color {
    let hsl: bevy::color::Hsla = color.into();
    // A grey carries no hue — HSL stores 0, which is red — so
    // pushing its saturation would INVENT a colour. The test for this
    // function found exactly that: a mid grey came back pure red.
    if hsl.saturation < 1e-3 {
        return color;
    }
    let target = (hsl.saturation + (1.0 - hsl.saturation) * amount.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    hsl.with_saturation(target).into()
}

/// The complementary stage tone: the accent's hue swung half the
/// wheel, keeping its lightness — the warm/cold opposition concert
/// light lives on. Pure — pinned.
#[must_use]
pub fn complementary(color: Color) -> Color {
    use bevy::color::Hue;
    Color::from(bevy::color::Hsla::from(color.to_srgba()).rotate_hue(180.0))
}

/// A fixture's beam angle at a moment — ONE formula for the pivot's
/// rotation and the floor spot's position, so the pool of light can
/// never drift away from the shaft that casts it. Pure.
#[must_use]
pub fn beam_angle(now: f32, base: f32, phase: f32, speed: f32) -> f32 {
    let swing = (now * speed + phase).sin();
    swing.mul_add(0.30, base)
}

/// Sweep the light beams. Gated on the Stage Motion setting, like
/// every other ambient movement in the game.
fn sweep_beams(time: Res<Time>, mut beams: Query<(&SpotBeam, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (beam, mut transform) in &mut beams {
        transform.rotation =
            Quat::from_rotation_z(beam_angle(now, beam.base, beam.phase, beam.speed));
    }
}

/// The pool of light a fixture throws on the stage floor.
#[derive(Component)]
pub struct FloorSpot {
    /// The fixture's swing parameters (mirroring its beam pivot).
    pub base: f32,
    /// Phase offset.
    pub phase: f32,
    /// Swing speed.
    pub speed: f32,
    /// The fixture's hanger x.
    pub pivot_x: f32,
    /// Vertical drop from hanger to floor.
    pub drop: f32,
}

/// Slide each floor pool under its swinging shaft.
fn slide_floor_spots(time: Res<Time>, mut spots: Query<(&FloorSpot, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (spot, mut transform) in &mut spots {
        let angle = beam_angle(now, spot.base, spot.phase, spot.speed);
        transform.translation.x = angle.tan().mul_add(spot.drop, spot.pivot_x);
    }
}

/// A speaker front whose woofer breathes with the beat.
#[derive(Component)]
pub struct WooferFront {
    /// Per-cabinet phase, so the stacks do not pump as one.
    pub phase: f32,
}

/// Pump the speaker fronts on the beat — a PA that stands dead still
/// while the song plays is what gives a fake stage away. Same
/// rectified-sine pulse as the LED wall, scaled down to a breath.
pub fn pulse_woofers(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mut fronts: Query<(&WooferFront, &mut Transform)>,
) {
    if !active(&settings) || !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player)) = (game_clock.song_time(&time), players.iter().next()) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    for (front, mut transform) in &mut fronts {
        let swell = (led_pulse(beats, front.phase) - 1.0).mul_add(0.30, 1.0);
        transform.scale = Vec3::new(swell, swell, 1.0);
    }
}

/// Set up camera, lights, the venue and the highway geometry.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn setup_stage(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    shapes: Res<crate::shapes::LaneShapes>,
) {
    if !active(&settings) {
        return;
    }
    let stage = theme.0;
    let neck = neck_style(&settings);

    // The camera sits behind and above the hit line, tilted down the
    // neck. This is the framing the genre settled on: close enough
    // that the receptors are large and readable, high enough that the
    // approaching notes separate instead of overlapping.
    let mut stage_camera = commands.spawn((
        GameplayScreen,
        Stage3d,
        Camera3d::default(),
        Camera {
            order: -1,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0f32.to_radians(),
            ..default()
        }),
        // Further back and higher than the first attempt: at 2.35/3.6
        // the nearest gems filled a third of the screen and the row of
        // receptors ran off both edges of the bed.
        Transform::from_xyz(0.0, 3.1, 5.2).looking_at(Vec3::new(0.0, 0.05, -7.5), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
    ));
    // Bloom (and the Hdr it requires) belongs to the ROUND style
    // only, and every camera on the window must agree on HDR — an
    // SDR 2D camera over an HDR stage camera silently drops the
    // stage's whole pass (the invisible-stage bug). sync_bloom keeps
    // this in step when the style is toggled at runtime.
    if settings.round_gems {
        stage_camera.insert((
            bevy::camera::Hdr,
            bevy::post_process::bloom::Bloom {
                intensity: 0.18,
                ..bevy::post_process::bloom::Bloom::NATURAL
            },
        ));
    }
    if neck == NeckStyle::Instrument {
        // The far end of the neck fades into the venue's dark, so
        // notes emerge from it rather than from the crowd. Linear,
        // because a curve with a knee is a thing to tune by eye and
        // a line is a thing to state: the neck's last third (camera
        // distance ~21–31) darkens to about half, the back wall (~45)
        // recedes to a fifth. Never to full black, or the venue would
        // be a hole.
        stage_camera.insert(bevy::pbr::DistanceFog {
            color: stage.background.mix(&Color::BLACK, 0.85),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: bevy::pbr::FogFalloff::Linear {
                start: FOG_START,
                end: FOG_END,
            },
        });
    }

    // Key light down the neck plus a soft fill, so gems read as
    // spheres rather than flat discs.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        DirectionalLight {
            // A club, not an exhibition hall: the key is just
            // enough to model the gems; the ROOM is allowed to
            // vanish into darkness (stage-realism plan P1).
            illuminance: 2_600.0,
            ..default()
        },
        Transform::from_xyz(2.0, 6.0, 2.0).looking_at(Vec3::new(0.0, 0.0, -8.0), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
    ));
    // Two coloured lamps from opposite sides. A white key light on
    // grey materials returns grey however many boxes are in the room:
    // measured, the venue sat at 0.13 brightness and 0.20 saturation,
    // which is "visible" rather than "lit".
    //
    // Both are RANGED so the room takes the light and the fretboard
    // does not — notes keep their contrast against the board, and
    // that is worth more than any amount of atmosphere.
    for (side, tint, strength) in [
        (-1.0f32, stage.accent, 1.0f32),
        (1.0, stage.lane_color(Lane::Four), 0.85),
    ] {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            PointLight {
                color: tint,
                intensity: 1_500_000.0 * strength,
                range: 34.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(side * 7.2, 6.5, -17.0),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }
    // A soft fill on the crowd, so the ranks are not a black mass.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        PointLight {
            color: stage.background.mix(&Color::WHITE, 0.7),
            intensity: 350_000.0,
            range: 26.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 5.0, -26.0),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // Ambient light is a component on the camera in this version.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        AmbientLight {
            color: stage.accent,
            brightness: 90.0,
            ..default()
        },
        RenderLayers::layer(STAGE_LAYER),
    ));

    spawn_venue(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        &shapes,
        stage,
        settings.backdrop_motion,
    );

    let board = images.add(board_texture());
    let bed = meshes.add(Cuboid::new(1.0, 0.06, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let rail = meshes.add(Cuboid::new(0.035, 0.05, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    // The decorated border sits OUTSIDE the bright rail, so it costs
    // no playfield: the rail still marks exactly where the neck ends.
    let trim = meshes.add(Cuboid::new(0.17, 0.035, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let trim_texture = images.add(rail_texture(stage.id));
    let lane_strip = meshes.add(Cuboid::new(0.018, 0.012, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    // A ring, not a disc: with both drawn as discs a resting receptor
    // and an approaching note were the same shape.
    let receptor_mesh = meshes.add(Torus::new(GEM_RADIUS * 0.82, GEM_RADIUS * 1.12));
    let fill_mesh = meshes.add(Cylinder::new(GEM_RADIUS * 0.88, 0.03));
    // The housing the button sits in. A coloured ring on a bare board
    // reads as a drawn outline; a ring seated in a metal collar reads
    // as a thing you could press.
    let collar_mesh = meshes.add(Torus::new(GEM_RADIUS * 1.14, GEM_RADIUS * 1.46));
    // The ring on the board — the impact — and the flame that leaps
    // off it. Two halves of one moment: the ring says WHERE, the
    // flame says HOW MUCH.
    let burst_mesh = meshes.add(Cylinder::new(GEM_RADIUS * 1.9, 0.012));
    let flame_mesh = meshes.add(Cone {
        radius: GEM_RADIUS * 1.15 * neck_spread(&layout),
        height: 1.0,
    });
    let hit_bar = meshes.add(Cuboid::new(1.0, 0.02, 0.06));
    // The Star-Power edge fire: one cone and ONE material shared by
    // every lick on every rail; the burn system animates transforms
    // only and never touches this material again.
    let edge_flame_mesh = meshes.add(Cone {
        radius: 0.15,
        height: 1.0,
    });
    let edge_flame_material = materials.add(StandardMaterial {
        // The house additive recipe (the beams, halos and haze all
        // use it): an UNLIT material renders its BASE color and
        // ignores emissive entirely - the first version wrote alpha
        // 0.0 plus emissive and the fire was invisible.
        base_color: EDGE_FLAME_BLUE.with_alpha(0.85),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });

    for index in &players {
        let player = index.0;
        let origin = layout.origin(player) * WORLD_PER_PIXEL;
        // A little wider than the lane span so the outer receptors
        // sit ON the neck rather than half off it.
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.18 * neck_spread(&layout);
        let centre = -HIGHWAY_LENGTH / 2.0 + HIGHWAY_BEHIND / 2.0;

        // The bed. Dark and slightly reflective so the lights and the
        // gems have something to sit on.
        //
        // Neon: light enough to read AS a fretboard — against a black
        // bed the gems floated in a void. Instrument: darker and
        // warmer, because on that neck the gems must be the brightest
        // thing and the grain texture gives the surface its presence.
        let board_base = match neck {
            NeckStyle::Neon => stage.background.mix(&Color::WHITE, 0.16),
            NeckStyle::Instrument => instrument_board_color(stage),
        };
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(bed.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: board_base,
                base_color_texture: Some(board.clone()),
                // Tiled far more down the neck than across it, so the
                // grain runs the way a plank's does.
                uv_transform: bevy::math::Affine2::from_scale(Vec2::new(1.0, 26.0)),
                perceptual_roughness: 0.42,
                metallic: 0.2,
                ..default()
            })),
            HypeTinted {
                player,
                base: board_base,
                base_glow: 0.0,
                glow_lift: 0.0,
                reach: 0.55,
            },
            Transform::from_xyz(origin, -0.03, centre).with_scale(Vec3::new(width, 1.0, 1.0)),
            RenderLayers::layer(STAGE_LAYER),
        ));

        // Rails down both edges. They frame the neck and, more
        // usefully, give the eye a fixed reference for how wide the
        // playfield is as it recedes. On the instrument neck they
        // keep the theme's colour but most of their glow goes: a
        // frame is read at the edge of vision, and a bright one pulls
        // the eye off the gems.
        // Neon: the rail IS the accent. Instrument: the rail is the
        // neck's binding — pale chrome with a trace of the theme —
        // and the theme's colour lives in the decorated trim outside
        // it. Measured with a coloured rail at 0.7 glow: still the
        // loudest line on the board.
        let (rail_base, rail_glow, trim_glow) = match neck {
            NeckStyle::Neon => (stage.accent, 2.6, 1.05),
            NeckStyle::Instrument => (string_color(stage).mix(&stage.accent, 0.3), 0.45, 0.32),
        };
        for side in [-1.0f32, 1.0] {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(rail.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: rail_base,
                    emissive: rail_base.to_linear() * rail_glow,
                    perceptual_roughness: 0.3,
                    metallic: 0.6,
                    ..default()
                })),
                HypeTinted {
                    player,
                    base: rail_base,
                    base_glow: rail_glow,
                    glow_lift: 0.8,
                    reach: 0.9,
                },
                Transform::from_xyz(origin + side * width / 2.0, 0.015, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));

            // The Star-Power fire: a row of blue flame licks along
            // this rail, hidden until the boost runs (see
            // `burn_edges_for_hype`). One shared additive material
            // for every lick - it is never written again.
            let mut z = -HIGHWAY_LENGTH + HIGHWAY_BEHIND / 2.0;
            let mut lick = 0usize;
            while z < HIGHWAY_BEHIND {
                let jitter = super::fx::hash01(lick * 73 + if side < 0.0 { 0 } else { 1 });
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    EdgeFlame {
                        player,
                        phase: jitter * core::f32::consts::TAU,
                        base: 0.75 + 0.5 * jitter,
                    },
                    Mesh3d(edge_flame_mesh.clone()),
                    MeshMaterial3d(edge_flame_material.clone()),
                    Visibility::Hidden,
                    Transform::from_xyz(origin + side * (width / 2.0 + 0.09), 0.015, z)
                        .with_scale(Vec3::ZERO),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                z += EDGE_FLAME_SPACING;
                lick += 1;
            }

            // The decorated trim. Dimmer than the rail on purpose:
            // the border should be seen, the edge should be read.
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(trim.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.accent.mix(&stage.background, 0.28),
                    base_color_texture: Some(trim_texture.clone()),
                    emissive: stage.accent.to_linear() * trim_glow,
                    emissive_texture: Some(trim_texture.clone()),
                    // Repeated far more down the neck than across it:
                    // the motif is a rhythm going away from you.
                    uv_transform: bevy::math::Affine2::from_scale(Vec2::new(1.0, 34.0)),
                    perceptual_roughness: 0.55,
                    metallic: 0.35,
                    ..default()
                })),
                HypeTinted {
                    player,
                    base: stage.accent.mix(&stage.background, 0.28),
                    base_glow: trim_glow,
                    glow_lift: 0.5,
                    reach: 0.9,
                },
                Transform::from_xyz(origin + side * (width / 2.0 + 0.115), 0.004, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // Dividers BETWEEN the lanes. Five coloured lines say where
        // the lanes are; a divider says where one ENDS, which is the
        // difference between a highway and five parallel wires. Kept
        // dimmer than the lane lines, or the board reads as a grid.
        for gap in 0..4 {
            let left = Lane::from_index(gap).expect("four gaps between five lanes");
            let right = Lane::from_index(gap + 1).expect("four gaps between five lanes");
            let middle = (lane_x(&layout, player, left) + lane_x(&layout, player, right)) / 2.0;
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(lane_strip.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.background.mix(&Color::WHITE, 0.30),
                    perceptual_roughness: 0.8,
                    ..default()
                })),
                Transform::from_xyz(middle, 0.004, centre).with_scale(Vec3::new(0.6, 1.0, 1.0)),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // One strip per lane, which is what gives the neck its sense
        // of depth as it recedes. Neon: a glowing line in the lane's
        // colour. Instrument: a STRING — one pale metallic shade for
        // all five, barely emissive, so it catches the key light the
        // way a wound string does and says nothing about which lane
        // it is. That is the buttons' and the gems' job.
        for lane in Lane::ALL {
            let colour = stage.lane_color(lane);
            let string_material = match neck {
                NeckStyle::Neon => StandardMaterial {
                    base_color: colour,
                    emissive: colour.to_linear() * 1.4,
                    unlit: false,
                    ..default()
                },
                NeckStyle::Instrument => StandardMaterial {
                    base_color: string_color(stage),
                    emissive: string_color(stage).to_linear() * 0.22,
                    perceptual_roughness: 0.3,
                    metallic: 0.7,
                    ..default()
                },
            };
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(lane_strip.clone()),
                MeshMaterial3d(materials.add(string_material)),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.005, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));

            // The fill sits inside the ring and is what actually
            // shows a press.
            // The collar first, so the coloured ring sits inside it.
            // Deliberately NOT hype-tinted and NOT lane-coloured: it
            // is hardware, and hardware does not change colour when
            // the song does.
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(collar_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.background.mix(&Color::WHITE, 0.30),
                    perceptual_roughness: 0.30,
                    metallic: 0.85,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.006, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            commands.spawn((
                GameplayScreen,
                Stage3d,
                ReceptorFill { player, lane },
                Mesh3d(fill_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.background,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.018, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
            // The burst, parked invisible until a note lands.
            commands.spawn((
                GameplayScreen,
                Stage3d,
                HitBurst {
                    player,
                    lane,
                    life: 0.0,
                },
                Mesh3d(burst_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour.with_alpha(0.0),
                    emissive: colour.to_linear() * 4.0,
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.035, 0.0)
                    .with_scale(Vec3::splat(0.01)),
                RenderLayers::layer(STAGE_LAYER),
            ));
            // The flame, parked flat until a note lands on this fret.
            commands.spawn((
                GameplayScreen,
                Stage3d,
                HitFlame {
                    player,
                    lane,
                    life: 0.0,
                },
                Mesh3d(flame_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour.with_alpha(0.0),
                    emissive: colour.to_linear() * 5.0,
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })),
                // Sits ON the fret, base at the board, tip upward.
                Transform::from_xyz(lane_x(&layout, player, lane), 0.5, 0.0)
                    .with_scale(Vec3::splat(0.01)),
                RenderLayers::layer(STAGE_LAYER),
            ));
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Receptor3d { player, lane },
                Mesh3d(receptor_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour,
                    emissive: colour.to_linear() * 0.35,
                    perceptual_roughness: 0.35,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.02, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // The hit line itself.
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(hit_bar.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::rgb(2.4, 2.4, 2.8),
                ..default()
            })),
            // Wider than the bed and lifted just clear of it: the line
            // the notes are struck ON has to be the brightest thing
            // on the neck, not a pair of stubs beside the receptors.
            Transform::from_xyz(origin, 0.014, 0.0).with_scale(Vec3::new(width * 1.12, 1.0, 1.6)),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }
}

/// Press and hit feedback for the 3D receptors.
///
/// The same two decays the 2D stage animates — how hard the fret is
/// held and how recently a note landed — expressed in geometry and
/// emission instead of sprite colour, so both views say the same
/// thing in their own vocabulary.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
pub fn update_receptors(
    time: Res<Time>,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut feedback: MessageReader<super::SessionFeedback>,
    // Disjoint by construction: the receptor ring and the burst both
    // want &mut Transform, and Bevy rejects overlapping mutable
    // access rather than risking aliasing. The Without filters are
    // what make the two provably different sets.
    mut receptors: Query<
        (
            &Receptor3d,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<HitBurst>,
    >,
    mut fills: Query<(&ReceptorFill, &MeshMaterial3d<StandardMaterial>)>,
    mut bursts: Query<
        (
            &mut HitBurst,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<Receptor3d>, Without<HitFlame>),
    >,
    mut flames: FlameQuery,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<Vec<(usize, Lane, f32, f32)>>,
) {
    if !active(&settings) {
        return;
    }
    // Which frets were struck this frame, and how cleanly.
    let mut struck: Vec<(usize, Lane, f32)> = Vec::new();
    for message in feedback.read() {
        let beatbyte_core::SessionEvent::NoteHit {
            event_index,
            judgment,
            ..
        } = message.event
        else {
            continue;
        };
        let Some((_, player)) = players
            .iter()
            .find(|(index, _)| index.0 == message.player_index)
        else {
            continue;
        };
        let Some(event) = player.session.track().events().get(event_index) else {
            continue;
        };
        let force = match judgment {
            beatbyte_core::Judgment::Perfect => 1.0,
            beatbyte_core::Judgment::Great => 0.8,
            _ => 0.62,
        };
        for lane in event.lanes.iter() {
            struck.push((message.player_index, lane, force));
        }
    }

    // Which frets are mid-sustain. A held note is not merely a
    // pressed fret: the strike is still happening, so its feedback
    // has to keep running for as long as the key is down.
    let mut sustaining: Vec<(usize, Lane)> = Vec::new();
    for (index, player) in &players {
        let Some(event_index) = player.session.active_sustain() else {
            continue;
        };
        let Some(event) = player.session.track().events().get(event_index) else {
            continue;
        };
        for lane in event.lanes.iter() {
            sustaining.push((index.0, lane));
        }
    }
    let holds =
        |player: usize, lane: Lane| sustaining.iter().any(|(p, l)| *p == player && *l == lane);

    let delta = time.delta_secs();
    let now = time.elapsed_secs();
    // Press/hit per fret, collected once so the fill and the burst
    // read exactly the numbers the ring does.
    let mut remembered: Vec<(usize, Lane, f32, f32)> = Vec::new();
    for (receptor, mut transform, material) in &mut receptors {
        let held = players
            .iter()
            .find(|(index, _)| index.0 == receptor.player)
            .is_some_and(|(_, player)| player.session.held().contains(receptor.lane));

        let slot = state
            .iter()
            .position(|(p, l, _, _)| *p == receptor.player && *l == receptor.lane)
            .unwrap_or_else(|| {
                state.push((receptor.player, receptor.lane, 0.0, 0.0));
                state.len() - 1
            });
        let (_, _, press, hit) = &mut state[slot];
        let rate = if held { 26.0 } else { 13.0 };
        let target = if held { 1.0 } else { 0.0 };
        *press += (target - *press) * (rate * delta).min(1.0);
        if let Some((_, _, force)) = struck
            .iter()
            .find(|(p, l, _)| *p == receptor.player && *l == receptor.lane)
        {
            *hit = hit.max(*force);
        }
        *hit = (*hit - 7.5 * delta).max(0.0);
        // While the hold runs, the strike is held ALIVE and breathing
        // rather than pinned: a constant maximum would be a static
        // bright ring, which is a state, not an animation.
        if holds(receptor.player, receptor.lane) {
            let breath = 0.22f32.mul_add((now * SUSTAIN_PULSE_HZ).sin(), SUSTAIN_HIT_FLOOR);
            *hit = hit.max(breath);
        }
        let (press, hit) = (*press, *hit);

        // Pressed frets sink into the neck; a hit makes one jump.
        transform.translation.y = 0.045f32.mul_add(-press, 0.02) + 0.06 * hit;
        transform.scale = Vec3::splat(0.18f32.mul_add(hit, 1.0) + 0.08 * press);

        if let Some(mut surface) = materials.get_mut(&material.0) {
            let colour = theme.0.lane_color(receptor.lane);
            let glow = 4.5f32.mul_add(press, 0.4) + 9.0 * hit;
            surface.emissive = colour.to_linear() * glow;
            surface.base_color = colour.mix(&Color::WHITE, 0.6 * hit);
        }
        remembered.push((receptor.player, receptor.lane, press, hit));
    }

    // The fill is what makes a press unmistakable: the button goes
    // SOLID, not merely brighter. The thin ring alone was the same
    // mistake the 2D stage already made once with a soft halo.
    for (fill, fill_material) in &mut fills {
        let Some((_, _, press, hit)) = remembered
            .iter()
            .find(|(p, l, _, _)| *p == fill.player && *l == fill.lane)
        else {
            continue;
        };
        if let Some(mut surface) = materials.get_mut(&fill_material.0) {
            let colour = theme.0.lane_color(fill.lane);
            surface.base_color = theme
                .0
                .background
                .mix(&colour, *press)
                .mix(&Color::WHITE, *hit);
            surface.emissive = colour.to_linear() * 3.2f32.mul_add(*press, 8.0 * hit);
        }
    }

    // The burst: a flat ring of light spreading ACROSS the board from
    // the fret and gone in about a fifth of a second — the genre's
    // flame, which is what tells you the note actually landed.
    for (mut burst, mut burst_transform, burst_material) in &mut bursts {
        if holds(burst.player, burst.lane) {
            // A sustained hold re-blooms instead of dying: the ring
            // spreads, fades, and starts again about three times a
            // second, so the fret reads as still burning. The one-shot
            // path below would just decay to nothing under the key.
            burst.life -= SUSTAIN_BLOOM_RATE * delta;
            if burst.life <= 0.12 {
                burst.life = 0.9;
            }
        } else {
            if let Some((_, _, _, hit)) = remembered
                .iter()
                .find(|(p, l, _, _)| *p == burst.player && *l == burst.lane)
                && *hit > burst.life
            {
                burst.life = *hit;
            }
            burst.life = (burst.life - 5.0 * delta).max(0.0);
        }
        let progress = 1.0 - burst.life;
        let spread = 2.2f32.mul_add(progress, 0.35);
        burst_transform.scale = Vec3::new(spread, 1.0, spread);
        if let Some(mut surface) = materials.get_mut(&burst_material.0) {
            let colour = theme.0.lane_color(burst.lane);
            surface.base_color = colour.with_alpha(burst.life.powf(1.3));
            surface.emissive = colour.to_linear() * (7.0 * burst.life);
        }
    }

    drive_flames(
        delta,
        &remembered,
        &holds,
        &mut flames,
        &mut materials,
        theme.0,
    );
}

/// The flame query, named because clippy is right that spelling it
/// out twice is unreadable.
type FlameQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut HitFlame,
        &'static mut Transform,
        &'static MeshMaterial3d<StandardMaterial>,
    ),
    (Without<Receptor3d>, Without<HitBurst>),
>;

/// Grow, lean and die: the flame is the loudest part of a hit, so it
/// has to be short. A quarter of a second, and gone before the next
/// note needs the space.
fn drive_flames(
    delta: f32,
    remembered: &[(usize, Lane, f32, f32)],
    sustaining: &dyn Fn(usize, Lane) -> bool,
    flames: &mut FlameQuery,
    materials: &mut Assets<StandardMaterial>,
    theme: crate::theme::Theme,
) {
    for (mut flame, mut transform, material) in flames {
        if let Some((_, _, _, hit)) = remembered
            .iter()
            .find(|(p, l, _, _)| *p == flame.player && *l == flame.lane)
            && *hit > flame.life
        {
            flame.life = *hit;
        }
        // A held sustain keeps a low flame alive under the fret.
        if sustaining(flame.player, flame.lane) {
            flame.life = flame.life.max(0.34);
        }
        flame.life = (flame.life - 3.0 * delta).max(0.0);

        let life = flame.life;
        // Tall and narrow at the peak, collapsing as it dies — a
        // flame thins upward, it does not shrink uniformly.
        // Proportions matter more than size here: at 2.6 tall and
        // 0.9 across the first version read as a laser, not a flame.
        // Roughly five to three is the shape of a flare.
        let height = 1.45 * life;
        let girth = 0.55f32.mul_add(life, 0.95) * life.max(0.001);
        transform.scale = Vec3::new(girth, height.max(0.001), girth);
        transform.translation.y = 0.03 + height * 0.5;
        if let Some(mut paint) = materials.get_mut(&material.0) {
            let colour = theme.lane_color(flame.lane);
            // Whiter at the base of the strike, the lane's own colour
            // as it burns down.
            let tint = colour.mix(&Color::WHITE, 0.45 * life);
            paint.base_color = tint.with_alpha(life * 0.75);
            paint.emissive = tint.to_linear() * (6.5 * life);
        }
    }
}

/// Take hit notes off the board and grey out missed ones.
///
/// Without this a struck note simply kept flying at the camera, which
/// is the opposite of what a hit should look like: the note has to
/// VANISH at the line, and the burst is what is left of it.
pub fn apply_note_events(
    mut commands: Commands,
    settings: Res<Settings>,
    assets: Option<Res<NoteAssets>>,
    mut feedback: MessageReader<super::SessionFeedback>,
    notes: Query<(Entity, &Note3d, Has<SustainTail3d>)>,
) {
    if !active(&settings) {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    for message in feedback.read() {
        let (event_index, hit) = match message.event {
            beatbyte_core::SessionEvent::NoteHit { event_index, .. } => (event_index, true),
            beatbyte_core::SessionEvent::NoteMissed { event_index } => (event_index, false),
            _ => continue,
        };
        for (entity, note, is_tail) in &notes {
            if note.player != message.player_index || note.event_index != event_index {
                continue;
            }
            if hit {
                // The tail stays. Despawning it here was why holding a
                // long note looked dead in this view: the whole note
                // vanished at the strike, including the part the
                // player had not played yet, so there was nothing
                // left to animate while the key was down.
                if is_tail {
                    continue;
                }
                commands.entity(entity).despawn();
            } else {
                // Swap the HANDLE, never the material behind it.
                commands
                    .entity(entity)
                    .insert(MeshMaterial3d(assets.missed_material.clone()));
            }
        }
    }
}

/// Lay bar lines across the neck, one per bar of the song.
pub fn spawn_fret_bars(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    song: Res<crate::boot::LoadedSong>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let bpm = song.chart.song.bpm.clamp(20.0, 400.0);
    let beat_s = 60.0 / bpm;
    let start = song.chart.song.offset_s;
    let end = song.chart.song.duration_s.unwrap_or(start + 240.0);
    // Heavier than the first pass: a bar line is what gives the neck
    // its ruled, instrument-like surface, and at 0.045 deep it read as
    // a scratch rather than a fret.
    let mesh = meshes.add(Cuboid::new(1.0, 0.014, 0.085));
    for index in &players {
        let origin = layout.origin(index.0) * WORLD_PER_PIXEL;
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.18 * neck_spread(&layout);
        let mut t = start;
        let mut beat = 0usize;
        while t < end {
            let downbeat = beat.is_multiple_of(4);
            // Its own material, because each bar fades by its own
            // distance — sharing one handle made every bar in the song
            // pile into a solid white wedge at the horizon.
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.80, 0.82, 0.88, 1.0),
                emissive: LinearRgba::rgb(0.55, 0.56, 0.64),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            commands.spawn((
                GameplayScreen,
                Stage3d,
                FretBar {
                    time_s: t,
                    downbeat,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                // Parked off-screen until the scroll system places it.
                Transform::from_xyz(origin, 0.012, -900.0).with_scale(Vec3::new(
                    width,
                    1.0,
                    if downbeat { 1.0 } else { OFFBEAT_DEPTH },
                )),
                RenderLayers::layer(STAGE_LAYER),
            ));
            t += beat_s;
            beat += 1;
        }
    }
}

/// Scroll the bar lines with the song, and fade them with distance so
/// a wall of equal-strength lines near the horizon does not read as
/// clutter.
pub fn move_fret_bars(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut bars: Query<(
        &FretBar,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let Some(now) = game_clock.visual_time(&time, &settings) else {
        return;
    };
    for (bar, mut transform, material, mut visibility) in &mut bars {
        let z = note_z(bar.time_s - now, settings.scroll_speed);
        transform.translation.z = z;
        // Beyond the drawn highway there is nothing to see, and a
        // hundred bars stacked at the vanishing point read as a solid
        // wedge rather than as depth.
        let distance = -z;
        let visible = (-1.0..HIGHWAY_LENGTH).contains(&distance);
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !visible {
            continue;
        }
        if let Some(mut surface) = materials.get_mut(&material.0) {
            // Full strength close by, gone by the far end.
            let fade = (1.0 - (distance / HIGHWAY_LENGTH).clamp(0.0, 1.0)).powf(1.6);
            let weight = if bar.downbeat { 1.0 } else { OFFBEAT_WEIGHT };
            surface.base_color = Color::srgba(0.75, 0.78, 0.85, fade * 0.85 * weight);
            surface.emissive = LinearRgba::rgb(0.35, 0.36, 0.42) * (fade * weight);
        }
    }
}

/// The outline of a five-point star in the XZ plane: alternating
/// outer and inner vertices, counter-clockwise, starting at the top
/// point (−Z, toward the vanishing point). Pure — the star is the
/// genre's star-power marking, and its geometry should be provable
/// without a GPU.
fn star_outline(points: usize, outer: f32, inner: f32) -> Vec<[f32; 2]> {
    let count = points * 2;
    (0..count)
        .map(|i| {
            let radius = if i % 2 == 0 { outer } else { inner };
            let angle =
                core::f32::consts::TAU * (i as f32) / (count as f32) - core::f32::consts::FRAC_PI_2;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}

/// A flat five-point star plate, face up (+Y). Only the top face is
/// built: at the stage camera's angle the underside of a 0.05-unit
/// plate is never seen, and half the triangles is half the cost.
fn star_mesh(outer: f32, inner: f32) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let outline = star_outline(5, outer, inner);
    let mut positions: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    positions.extend(outline.iter().map(|[x, z]| [*x, 0.0, *z]));
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .map(|p| [0.5 + p[0] / (2.0 * outer), 0.5 + p[2] / (2.0 * outer)])
        .collect();
    let rim = outline.len() as u32;
    let mut indices: Vec<u32> = Vec::with_capacity(outline.len() * 3);
    for i in 0..rim {
        // Fan from the centre; wound so the +Y face is the front.
        indices.extend([0, 1 + (i + 1) % rim, 1 + i]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Reusable geometry and materials for the 3D notes, built once.
#[derive(Resource)]
pub struct NoteAssets {
    gem: Handle<Mesh>,
    rim: Handle<Mesh>,
    hopo: Handle<Mesh>,
    hopo_rim: Handle<Mesh>,
    sustain: Handle<Mesh>,
    /// The star plate a phrase note wears under its gem — the
    /// genre's star-power marking (star-shaped gems), in place of
    /// the round rim.
    star_rim: Handle<Mesh>,
    /// The white centre on the instrument neck's gems: the genre's
    /// button marking, which the 2D views carry and the 3D gem did
    /// not. `None` on the neon neck, whose gems are its own.
    centre: Option<Handle<Mesh>>,
    /// The HOPO's centre, larger than the strum note's: in the genre
    /// the big white cap IS what says "no strum needed".
    hopo_centre: Option<Handle<Mesh>>,
    /// The black ring on a strum note's face, around its centre —
    /// the strum note's mark. HOPOs have none.
    face_ring: Option<Handle<Mesh>>,
    face_ring_material: Handle<StandardMaterial>,
    centre_material: Handle<StandardMaterial>,
    /// A brighter core strip inside a sustain tail, instrument neck
    /// only — the held note's own light, running down its rail.
    sustain_core: Option<Handle<Mesh>>,
    missed_material: Handle<StandardMaterial>,
    rim_material: Handle<StandardMaterial>,
    hype_rim_material: Handle<StandardMaterial>,
    lane_material: Vec<Handle<StandardMaterial>>,
}

/// Build the note assets when the stage comes up.
pub fn setup_note_assets(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !active(&settings) {
        return;
    }
    let face = images.add(shaded_tile(64, gem_shade, false));
    // The genre's gem is a flat BUTTON lying on the fretboard — a
    // coloured disc inside a dark rim — not a floating sphere. Seen
    // from the player's angle it reads as an ellipse, which is what
    // makes a five-lane row scannable at speed.
    // The gem radius is a world-unit constant, so widening the neck
    // without it would leave the notes undersized in their own lanes.
    let gem = GEM_RADIUS * neck_spread(&layout);
    let neck = neck_style(&settings);
    commands.insert_resource(NoteAssets {
        gem: meshes.add(Cylinder::new(gem, 0.055)),
        rim: meshes.add(Cylinder::new(
            gem * match neck {
                NeckStyle::Neon => 1.28,
                // A thin edge only: on this neck the strum note's
                // mark is the black ring on its FACE, and a fat
                // outer bezel would compete with it.
                NeckStyle::Instrument => 1.12,
            },
            0.042,
        )),
        centre: match neck {
            NeckStyle::Neon => None,
            NeckStyle::Instrument => {
                Some(meshes.add(Cylinder::new(centre_radius(gem, false), 0.02)))
            }
        },
        hopo_centre: match neck {
            NeckStyle::Neon => None,
            NeckStyle::Instrument => {
                Some(meshes.add(Cylinder::new(centre_radius(gem, true), 0.02)))
            }
        },
        face_ring: match neck {
            NeckStyle::Neon => None,
            NeckStyle::Instrument => {
                let (inner, outer) = face_ring_radii(gem);
                Some(meshes.add(Annulus::new(inner, outer)))
            }
        },
        // Unlit: a lit near-black picks up the key light and the
        // cap's bloom and came back as dark orange (measured ~100 of
        // 255 beside a 207 cap). The ring is the strum note's mark;
        // it has to read black under every light.
        face_ring_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.01, 0.01, 0.012),
            unlit: true,
            ..default()
        }),
        // Emissive white, not lit white: a lit disc would take the
        // lane's colour from the light bouncing off the cap around
        // it and read as a paler patch of the same colour.
        centre_material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(3.0, 3.0, 2.8),
            perceptual_roughness: 0.4,
            ..default()
        }),
        // A HOPO is smaller and reads as a different object, the way
        // the 2D views distinguish it.
        hopo: meshes.add(Cylinder::new(gem * HOPO_FACE, 0.05)),
        hopo_rim: meshes.add(Cylinder::new(gem * 0.86, 0.04)),
        sustain: meshes.add(Cylinder::new(
            match neck {
                NeckStyle::Neon => 0.05,
                // Thinner: the reference's tail is a rail, not a pipe.
                NeckStyle::Instrument => 0.036,
            } * neck_spread(&layout),
            1.0,
        )),
        sustain_core: match neck {
            NeckStyle::Neon => None,
            NeckStyle::Instrument => {
                Some(meshes.add(Cylinder::new(0.013 * neck_spread(&layout), 1.0)))
            }
        },
        star_rim: meshes.add(star_mesh(gem * 1.62, gem * 0.82)),
        // ONE grey material that missed notes switch TO. Repainting
        // the lane's own material instead turned every note in that
        // lane black for the rest of the song, because they all share
        // the handle — which is exactly what happened.
        missed_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.26, 0.26, 0.29),
            perceptual_roughness: 0.8,
            ..default()
        }),
        rim_material: materials.add(StandardMaterial {
            base_color: match neck {
                NeckStyle::Neon => Color::srgb(0.05, 0.05, 0.07),
                // Near-black and a touch glossy: the ring on a button
                // is a bezel, and a bezel catches a little light.
                NeckStyle::Instrument => Color::srgb(0.02, 0.02, 0.025),
            },
            perceptual_roughness: match neck {
                NeckStyle::Neon => 0.6,
                NeckStyle::Instrument => 0.35,
            },
            ..default()
        }),
        // A note inside an energy phrase wears a lit rim instead of a
        // dark one. The FACE keeps its lane colour, because the fret
        // to press is the one thing the marking must never obscure.
        hype_rim_material: materials.add(StandardMaterial {
            base_color: palette::HYPE,
            emissive: palette::HYPE.to_linear() * 3.0,
            perceptual_roughness: 0.3,
            metallic: 0.5,
            ..default()
        }),
        lane_material: Lane::ALL
            .iter()
            .map(|lane| {
                let colour = match neck {
                    NeckStyle::Neon => theme.0.lane_color(*lane),
                    // Pulled toward full saturation: under bloom the
                    // theme's lane colour read pastel on the darker
                    // board, and a button cap is a solid colour.
                    NeckStyle::Instrument => saturate(theme.0.lane_color(*lane), 0.35),
                };
                materials.add(StandardMaterial {
                    base_color: colour,
                    base_color_texture: Some(face.clone()),
                    // The face has to modulate the EMISSIVE too. A
                    // gem's look is dominated by its glow, so a
                    // base-colour texture alone is invisible: measured,
                    // it flattened the gem instead of shaping it —
                    // brightness across the disc fell from a span of
                    // 50 to 10.
                    emissive_texture: Some(face.clone()),
                    // Emissive is what makes a gem glow through the
                    // bloom pass instead of merely being lit.
                    emissive: colour.to_linear() * 2.2,
                    // Polished, so the key light down the neck leaves
                    // a highlight on the gem's face. Without it a gem
                    // is an evenly lit disc — a sticker on the board
                    // rather than an object on it.
                    perceptual_roughness: 0.16,
                    metallic: 0.55,
                    reflectance: 0.6,
                    ..default()
                })
            })
            .collect(),
    });
}

/// Spawn 3D geometry for notes coming into view.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn spawn_due_notes(
    mut commands: Commands,
    mut players: Query<(&PlayerIndex, &mut PlayerSession)>,
    layout: Res<HighwayLayout>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
    assets: Option<Res<NoteAssets>>,
    theme: Res<crate::theme::ActiveTheme>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let (Some(now), Some(assets)) = (game_clock.visual_time(&time, &settings), assets) else {
        return;
    };
    for (index, mut player) in &mut players {
        while player.spawn_cursor < player.session.track().events().len() {
            let cursor = player.spawn_cursor;
            let event = player.session.track().events()[cursor];
            if event.time_s > now + super::SPAWN_LOOKAHEAD_S {
                break;
            }
            let z = note_z(event.time_s - now, settings.scroll_speed);
            // Notes inside an energy phrase are marked. The chart has
            // carried `phrases` all along and `complete_phrase()` has
            // been paying meter for them, but nothing on screen ever
            // said which notes those were — the player earned energy
            // without being told why.
            let in_phrase = in_energy_phrase(player.session.track().phrases(), event.time_s);
            for lane in event.lanes.iter() {
                let material = assets.lane_material[lane.index()].clone();
                let hopo = event.kind == beatbyte_core::NoteKind::Hopo;
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Note3d {
                        player: index.0,
                        event_index: cursor,
                        lane,
                    },
                    Mesh3d(if hopo {
                        assets.hopo.clone()
                    } else {
                        assets.gem.clone()
                    }),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(lane_x(&layout, index.0, lane), 0.075, z),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                // The white centre, on the instrument neck. It shares
                // the note's key, so it scrolls, vanishes on a hit and
                // greys on a miss with the rest of the gem for free.
                let centre = if hopo {
                    &assets.hopo_centre
                } else {
                    &assets.centre
                };
                // The strum note's black ring, flat on the face around
                // the centre. An annulus is a 2D primitive in the XY
                // plane; laid flat it becomes the ring on top.
                if let (false, Some(ring)) = (hopo, &assets.face_ring) {
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        Note3d {
                            player: index.0,
                            event_index: cursor,
                            lane,
                        },
                        Mesh3d(ring.clone()),
                        MeshMaterial3d(assets.face_ring_material.clone()),
                        Transform::from_xyz(lane_x(&layout, index.0, lane), 0.104, z)
                            .with_rotation(Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2)),
                        RenderLayers::layer(STAGE_LAYER),
                    ));
                }
                if let Some(centre) = centre {
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        Note3d {
                            player: index.0,
                            event_index: cursor,
                            lane,
                        },
                        Mesh3d(centre.clone()),
                        MeshMaterial3d(assets.centre_material.clone()),
                        Transform::from_xyz(lane_x(&layout, index.0, lane), 0.105, z),
                        RenderLayers::layer(STAGE_LAYER),
                    ));
                }
                // The dark rim the coloured face sits in.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Note3d {
                        player: index.0,
                        event_index: cursor,
                        lane,
                    },
                    Mesh3d(if in_phrase {
                        // A phrase note IS a star: the genre marks
                        // star-power notes with star-shaped gems, not
                        // with a differently-lit circle.
                        assets.star_rim.clone()
                    } else if hopo {
                        assets.hopo_rim.clone()
                    } else {
                        assets.rim.clone()
                    }),
                    MeshMaterial3d(if in_phrase {
                        assets.hype_rim_material.clone()
                    } else {
                        assets.rim_material.clone()
                    }),
                    Transform::from_xyz(lane_x(&layout, index.0, lane), 0.06, z).with_scale(
                        if in_phrase && hopo {
                            Vec3::splat(0.72)
                        } else {
                            Vec3::ONE
                        },
                    ),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                // A sustain is a tube running back up the neck from
                // the gem — length is the note's own held time.
                if event.is_sustain() {
                    let length = (event.sustain_s as f32) * settings.scroll_speed * Z_PER_PIXEL;
                    // Its own material: the lane's is SHARED by every
                    // note in that lane, and pulsing it while a note
                    // is held would light the whole lane along with it
                    // - the same trap that once greyed a lane out.
                    let tail_material = materials
                        .get(&material)
                        .cloned()
                        .map_or_else(|| material.clone(), |own| materials.add(own));
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        Note3d {
                            player: index.0,
                            event_index: cursor,
                            lane,
                        },
                        SustainTail3d,
                        Mesh3d(assets.sustain.clone()),
                        MeshMaterial3d(tail_material),
                        // The cylinder is built along Y, so it is
                        // rotated onto the neck's Z axis and pushed
                        // back by half its length.
                        Transform::from_xyz(lane_x(&layout, index.0, lane), 0.05, z - length / 2.0)
                            .with_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(1.0, length, 1.0)),
                        RenderLayers::layer(STAGE_LAYER),
                    ));
                    // The core: same key, same length, its own
                    // material (the held-tail throb writes emissive),
                    // marked so the throb keeps it pale instead of
                    // painting it the lane's colour.
                    if let Some(core_mesh) = &assets.sustain_core {
                        let pale = core_color(&lane, theme.0);
                        commands.spawn((
                            GameplayScreen,
                            Stage3d,
                            Note3d {
                                player: index.0,
                                event_index: cursor,
                                lane,
                            },
                            SustainTail3d,
                            SustainCore,
                            Mesh3d(core_mesh.clone()),
                            MeshMaterial3d(materials.add(StandardMaterial {
                                base_color: pale,
                                emissive: pale.to_linear() * 1.6,
                                ..default()
                            })),
                            Transform::from_xyz(
                                lane_x(&layout, index.0, lane),
                                0.062,
                                z - length / 2.0,
                            )
                            .with_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(1.0, length, 1.0)),
                            RenderLayers::layer(STAGE_LAYER),
                        ));
                    }
                }
            }
            player.spawn_cursor += 1;
        }
    }
}

/// Whether this entity is a sustain tail the player is HOLDING right
/// now — in which case [`move_notes`] must not touch it at all:
/// [`consume_sustains`] pins a held tail to the hit line and eats it
/// from the front. The head-anchored march below would carry it past
/// the camera while the hold ran, and the past-the-camera cleanup
/// then despawned the very beam the player was playing (a 2 s
/// sustain lost its beam less than halfway through the hold). Pure —
/// tested against exactly those numbers.
#[must_use]
pub fn tail_is_held(is_tail: bool, active_sustain: Option<usize>, event_index: usize) -> bool {
    is_tail && active_sustain == Some(event_index)
}

/// Keep the 3D notes in step with the song.
pub fn move_notes(
    mut commands: Commands,
    mut notes: Query<(Entity, &Note3d, &mut Transform, Has<SustainTail3d>)>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    layout: Res<HighwayLayout>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    if !active(&settings) {
        return;
    }
    let Some(now) = game_clock.visual_time(&time, &settings) else {
        return;
    };
    let Some((_, reference)) = players.iter().next() else {
        return;
    };
    let events = reference.session.track().events();
    for (entity, note, mut transform, is_tail) in &mut notes {
        let Some(event) = events.get(note.event_index) else {
            continue;
        };
        // A held tail is the consumer's: pinned to the hit line,
        // throbbing beside the receptor flame, shrinking as it is
        // played — and only released (or fully eaten) may end it.
        let active = players
            .iter()
            .find(|(index, _)| index.0 == note.player)
            .and_then(|(_, player)| player.session.active_sustain());
        if tail_is_held(is_tail, active, note.event_index) {
            continue;
        }
        let head = note_z(event.time_s - now, settings.scroll_speed);
        // A sustain tube is offset back by half its own length; its
        // scale carries that length, so the offset is recomputed
        // rather than remembered.
        //
        // Asked by component, not by guessing from the scale. The old
        // test was `scale.y > 1.5`, which held only while a tail was
        // at full length — a tail partly eaten by a hold falls under
        // that threshold, and a dropped one would then have jumped
        // half its own length up the neck.
        let z = if is_tail {
            head - transform.scale.y / 2.0
        } else {
            head
        };
        transform.translation.x = lane_x(&layout, note.player, note.lane);
        transform.translation.z = z;
        // Past the camera: gone.
        if z > 4.5 {
            commands.entity(entity).despawn();
        }
    }
}

/// Eat the tail of a sustain while it is being held, and let go of it
/// when the hold ends.
///
/// The receptor keeps burning (see [`update_receptors`]); this is the
/// other half of the same feedback — the tail visibly shortens into
/// the fret for exactly as long as the key is down.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI, not an API
pub fn consume_sustains(
    mut commands: Commands,
    settings: Res<Settings>,
    time: Res<Time>,
    game_clock: Res<GameClock>,
    assets: Option<Res<NoteAssets>>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut tails: Query<
        (
            Entity,
            &Note3d,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
            Has<SustainCore>,
        ),
        With<SustainTail3d>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<crate::theme::ActiveTheme>,
) {
    let stage = theme.0;
    if !active(&settings) {
        return;
    }
    let (Some(now), Some(assets)) = (game_clock.visual_time(&time, &settings), assets) else {
        return;
    };
    for (entity, note, mut transform, material, is_core) in &mut tails {
        let Some((_, player)) = players.iter().find(|(index, _)| index.0 == note.player) else {
            continue;
        };
        // Only the sustain the engine says is running. Asking the
        // session rather than tracking a local flag keeps the picture
        // honest: if judgment dropped the hold, so does the tail.
        if player.session.active_sustain() != Some(note.event_index) {
            continue;
        }
        let Some(event) = player.session.track().events().get(note.event_index) else {
            continue;
        };
        match sustain_tail_span(event.time_s, event.sustain_s, now, settings.scroll_speed) {
            Some((centre, length)) => {
                transform.translation.z = centre;
                transform.scale.y = length;
                // Being played, and looking it. Driven from the SONG
                // clock, so the throb keeps time with the music
                // rather than with the frame rate.
                if let Some(mut surface) = materials.get_mut(&material.0) {
                    let glow = sustain_pulse(now);
                    surface.emissive = if is_core {
                        core_color(&note.lane, stage).to_linear() * (1.6 * glow)
                    } else {
                        base_emissive(&note.lane, stage) * glow
                    };
                }
            }
            // Fully played: the tail has been eaten, nothing to show.
            None => commands.entity(entity).despawn(),
        }
    }

    // A tail whose hold has ended but which still has length left was
    // DROPPED — it greys out and slides away, so letting go looks
    // different from playing it out.
    for (entity, note, transform, _, _) in &tails {
        let held = players
            .iter()
            .find(|(index, _)| index.0 == note.player)
            .and_then(|(_, player)| player.session.active_sustain());
        if held == Some(note.event_index) || transform.scale.y <= 0.0 {
            continue;
        }
        let struck = players
            .iter()
            .find(|(index, _)| index.0 == note.player)
            .and_then(|(_, player)| player.session.note_state(note.event_index))
            .is_some_and(|state| matches!(state, beatbyte_core::session::NoteState::Hit(_)));
        if struck {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(assets.missed_material.clone()));
        }
    }
}

/// The plugin: only its own systems, all gated on the 3D view.
pub struct Stage3dPlugin;

impl Plugin for Stage3dPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Gameplay),
            (
                setup_stage,
                setup_note_assets,
                spawn_fret_bars,
                spawn_phrase_bands,
                super::band::spawn_band,
            )
                .chain()
                .after(super::setup_gameplay),
        )
        .add_systems(
            Update,
            (
                spawn_due_notes,
                move_notes,
                consume_sustains,
                move_fret_bars,
                move_phrase_bands,
                tint_stage_for_hype,
                burn_edges_for_hype,
                bob_crowd,
                super::band::animate_band,
                pulse_led_wall,
                pulse_woofers,
                slide_floor_spots,
                update_receptors,
                apply_note_events,
                sweep_beams,
            )
                .chain()
                .run_if(
                    // The venue plays on through the outro - a stage
                    // that freezes behind "YOU ROCK!!!" reads as a
                    // crash, not a celebration.
                    in_state(crate::states::GamePhase::Playing)
                        .or_else(in_state(crate::states::GamePhase::Outro)),
                ),
        );
    }
}

#[cfg(test)]
mod held_tail_tests {
    use super::{note_z, sustain_tail_span, tail_is_held};

    #[test]
    fn a_held_tail_is_left_to_the_consumer_or_it_dies_mid_hold() {
        // A 2 s sustain at 420 px/s, 1.2 s into the hold: the
        // consumer still has 0.8 s of tail to draw at the hit line -
        // but the head-anchored centre move_notes would compute is
        // already past the despawn line. Before the tail_is_held
        // gate, move_notes despawned exactly this entity: the beam
        // vanished while the key was still down.
        let scroll = 420.0;
        let (time_s, sustain_s, now) = (10.0, 2.0, 11.2);
        let span = sustain_tail_span(time_s, sustain_s, now, scroll);
        let (_, remaining) = span.expect("the hold still has tail to play");
        let head = note_z(time_s - now, scroll);
        let head_anchored_centre = head - remaining / 2.0;
        assert!(
            head_anchored_centre > 4.5,
            "the head-anchored centre ({head_anchored_centre}) sits past the 4.5 despawn              line while the hold runs - which is why a held tail must be skipped"
        );
        // The gate itself: only a TAIL of the ACTIVE sustain is held.
        assert!(tail_is_held(true, Some(7), 7));
        assert!(!tail_is_held(true, Some(8), 7), "a different sustain");
        assert!(!tail_is_held(true, None, 7), "no hold running");
        assert!(!tail_is_held(false, Some(7), 7), "the gem is not a tail");
    }
}

#[cfg(test)]
mod tests {
    use super::{backdrop_shade, gem_shade};

    #[test]
    fn a_gem_face_is_brightest_near_its_highlight() {
        // The point of the face is that a gem reads as lit from
        // somewhere. If the centre and the rim matched, the texture
        // would be a wasted sampler.
        let highlight = gem_shade(0.36, 0.34);
        let rim = gem_shade(0.96, 0.5);
        assert!(
            highlight > rim + 0.12,
            "highlight {highlight} barely beats rim {rim}"
        );
    }

    #[test]
    fn a_gem_face_never_dims_the_note_out_of_readability() {
        // The face modulates emissive, so its darkest point is how
        // dim a note gets at distance. Shape is only worth having
        // while the note stays legible on the far end of the neck.
        let mut darkest = f32::MAX;
        for i in 0..40 {
            for j in 0..40 {
                darkest = darkest.min(gem_shade(i as f32 / 40.0, j as f32 / 40.0));
            }
        }
        assert!(darkest >= 0.5, "gems can dim to {darkest} of their glow");
    }

    #[test]
    fn a_gem_face_stays_inside_its_range() {
        for i in 0..40 {
            for j in 0..40 {
                let value = gem_shade(i as f32 / 40.0, j as f32 / 40.0);
                assert!(
                    (0.0..=1.0).contains(&value),
                    "gem shade {value} out of range"
                );
            }
        }
    }

    #[test]
    fn the_backdrop_is_banded_and_bounded() {
        let samples: Vec<f32> = (0..128)
            .map(|i| backdrop_shade(i as f32 / 128.0, 0.5))
            .collect();
        let min = samples.iter().copied().fold(f32::MAX, f32::min);
        let max = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(max - min > 0.15, "backdrop is nearly flat: {min}..{max}");
        assert!(min >= 0.0 && max <= 1.0, "backdrop out of range");
    }

    #[test]
    fn the_backdrop_is_darker_at_the_floor() {
        // The crowd and the speaker stacks sit along the bottom of
        // it; a wall that is brightest there fights them.
        assert!(backdrop_shade(0.5, 0.05) < backdrop_shade(0.5, 0.95));
    }

    use super::{BOARD_SHADE, board_shade, neck_spread};

    #[test]
    fn the_board_never_leaves_its_brightness_band() {
        // The board must read as a surface without competing with the
        // gems, the lane lines or the hit line that sit ON it. This is
        // the constraint "subtle" actually means, and it is exactly
        // the kind of intent that erodes one tweak at a time.
        let (low, high) = BOARD_SHADE;
        for i in 0..64 {
            for j in 0..64 {
                let u = f64::from(i) as f32 / 64.0;
                let v = f64::from(j) as f32 / 64.0;
                let shade = board_shade(u, v);
                assert!(
                    (low..=high).contains(&shade),
                    "shade {shade} at ({u}, {v}) is outside {low}..={high}"
                );
            }
        }
    }

    #[test]
    fn the_board_is_actually_patterned() {
        // A texture that came out flat would be a wasted draw and a
        // silently missing feature — it would still pass the band test
        // above, which is why this one exists.
        let samples: Vec<f32> = (0..256)
            .map(|i| board_shade((i % 16) as f32 / 16.0, (i / 16) as f32 / 16.0))
            .collect();
        let min = samples.iter().copied().fold(f32::MAX, f32::min);
        let max = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(max - min > 0.10, "board is nearly flat: {min}..{max}");
    }

    #[test]
    fn the_board_pattern_is_the_same_every_run() {
        // Built from a hash, not a random number: a fretboard that
        // reshuffled itself between runs would be a distraction.
        assert!((board_shade(0.31, 0.62) - board_shade(0.31, 0.62)).abs() < f32::EPSILON);
    }

    #[test]
    fn only_a_solo_neck_is_widened() {
        // Two to four necks side by side already use the room; the
        // count comes from the layout so the two cannot drift apart.
        let solo = crate::gameplay::HighwayLayout::for_players(1);
        let duo = crate::gameplay::HighwayLayout::for_players(2);
        assert!(neck_spread(&solo) > 1.0, "solo should be widened");
        assert!(
            (neck_spread(&duo) - 1.0).abs() < f32::EPSILON,
            "multiplayer must keep the layout's own spacing"
        );
    }

    use super::in_energy_phrase;
    use beatbyte_core::Phrase;

    fn phrases() -> Vec<Phrase> {
        vec![
            Phrase {
                start_s: 4.0,
                end_s: 8.0,
            },
            Phrase {
                start_s: 20.0,
                end_s: 24.0,
            },
        ]
    }

    #[test]
    fn notes_inside_a_phrase_are_marked() {
        assert!(in_energy_phrase(&phrases(), 6.0));
        assert!(in_energy_phrase(&phrases(), 22.5));
    }

    #[test]
    fn notes_outside_every_phrase_are_not() {
        assert!(!in_energy_phrase(&phrases(), 3.9));
        assert!(!in_energy_phrase(&phrases(), 12.0));
        assert!(!in_energy_phrase(&phrases(), 24.1));
        assert!(!in_energy_phrase(&[], 6.0), "no phrases marks nothing");
    }

    #[test]
    fn the_bounds_themselves_count_as_inside() {
        // Phrase bounds are inclusive in the core, and the run of
        // marked notes has to end exactly where the meter is awarded
        // — a note on the last instant belongs to the phrase.
        assert!(in_energy_phrase(&phrases(), 4.0), "start is inside");
        assert!(in_energy_phrase(&phrases(), 8.0), "end is inside");
    }

    use super::sustain_tail_span;

    /// A note at t = 10 s that is held for 2 s, at the default speed.
    fn span(now: f64) -> Option<(f32, f32)> {
        sustain_tail_span(10.0, 2.0, now, 420.0)
    }

    #[test]
    fn an_untouched_tail_keeps_its_full_length() {
        // Before the note reaches the line the tail is its whole
        // musical length, whatever the scroll speed.
        let (_, length) = span(9.0).expect("tail exists");
        let expected = 2.0 * 420.0 * super::Z_PER_PIXEL;
        assert!(
            (length - expected).abs() < 1e-3,
            "expected {expected}, got {length}"
        );
    }

    #[test]
    fn holding_eats_the_tail_from_the_hit_line() {
        // Half a second into a two-second hold, three quarters should
        // be left — and it must be SHORTER than a moment earlier, or
        // the hold has no visible progress at all.
        let (_, full) = span(10.0).expect("tail at the strike");
        let (_, later) = span(10.5).expect("tail mid-hold");
        assert!(later < full, "tail did not shrink: {full} -> {later}");
        let expected = 1.5 * 420.0 * super::Z_PER_PIXEL;
        assert!(
            (later - expected).abs() < 1e-3,
            "expected {expected}, got {later}"
        );
    }

    #[test]
    fn a_played_out_tail_reports_nothing_left() {
        // At and past the end there is no tail. Returning a zero or
        // negative length instead would draw an inside-out cylinder.
        assert!(span(12.0).is_none(), "tail should be gone at the end");
        assert!(span(12.5).is_none(), "tail should stay gone after it");
    }

    #[test]
    fn the_tail_stays_centred_between_its_own_ends() {
        // The cylinder is positioned by its middle, so a wrong centre
        // makes the tail float off the fret it belongs to. Its near
        // end must sit exactly on the hit line during a hold.
        let (centre, length) = span(10.4).expect("tail mid-hold");
        let near = centre + length / 2.0;
        assert!(near.abs() < 1e-4, "near end should be at z=0, was {near}");
    }

    #[test]
    fn the_tail_never_reaches_past_the_hit_line() {
        // Once struck, the played part is gone: nothing may hang in
        // front of the receptors, where it would sit under the camera.
        for step in 0..20 {
            let now = 10.0 + f64::from(step) * 0.1;
            if let Some((centre, length)) = span(now) {
                assert!(
                    centre + length / 2.0 <= 1e-4,
                    "tail crossed the line at t={now}"
                );
            }
        }
    }

    use super::*;

    #[test]
    fn the_hit_line_is_the_origin_and_the_future_is_negative_z() {
        assert!(note_z(0.0, 420.0).abs() < 1e-6);
        assert!(note_z(1.0, 420.0) < 0.0, "the future runs away from you");
        assert!(note_z(-0.2, 420.0) > 0.0, "the past runs past the camera");
    }

    #[test]
    fn scroll_speed_still_means_what_it_meant() {
        // Doubling the setting must double the distance a note covers
        // in a second, exactly as in the 2D views.
        let slow = note_z(1.0, 300.0).abs();
        let fast = note_z(1.0, 600.0).abs();
        assert!((fast - slow * 2.0).abs() < 1e-5);
    }

    #[test]
    fn the_drawn_highway_is_exactly_the_spawn_lookahead() {
        // The neck must show precisely the notes that exist: shorter
        // and they pop in mid-flight, longer and they crawl toward a
        // hit line that never arrives. Sharing the width scale for
        // depth once made a note take 13.7 s to cross a highway it
        // should cross in 2.6 s.
        let travelled = note_z(crate::gameplay::SPAWN_LOOKAHEAD_S, 420.0).abs();
        assert!(
            (travelled - HIGHWAY_LENGTH).abs() < 0.01,
            "a note covers {travelled} units in the lookahead, \
             but the highway is {HIGHWAY_LENGTH}"
        );
    }
}

#[cfg(test)]
mod edge_flame_tests {
    use super::flame_lick;

    #[test]
    fn the_lick_flickers_but_never_dies() {
        // A flame that blinks out reads as the boost dropping; one
        // that stands still reads as a painted edge. The factor must
        // MOVE and must stay well clear of zero.
        let samples: Vec<f32> = (0..400).map(|i| flame_lick(i as f32 * 0.02, 1.3)).collect();
        let lo = samples.iter().copied().fold(f32::MAX, f32::min);
        let hi = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo > 0.5, "a lick collapsed to {lo}");
        assert!(hi < 1.5, "a lick blew up to {hi}");
        assert!(hi - lo > 0.4, "the fire barely moves: {lo}..{hi}");
        // Two licks with different phases must not move in step - a
        // row breathing as one block is a curtain, not a fire.
        let other = flame_lick(1.0, 4.0);
        let this = flame_lick(1.0, 1.3);
        assert!((other - this).abs() > 0.01, "phases collapsed");
    }
}

#[cfg(test)]
mod rail_tests {
    use super::rail_shade;

    /// The theme ids that ship, in the order they are declared.
    const THEMES: [&str; 6] = ["garage", "punk", "metal", "stadium", "psychedelic", "cyber"];

    /// A coarse sampling of one strip, along its length.
    fn strip(theme: &str) -> Vec<f32> {
        (0..192)
            .map(|i| rail_shade(theme, 0.5, i as f32 / 192.0))
            .collect()
    }

    #[test]
    fn every_theme_has_its_own_border() {
        // A decorated border only says which stage you are on if the
        // stages do not share one. Compared as a whole strip rather
        // than at a point, because two different motifs can of course
        // cross at a sample.
        for (a, first) in THEMES.iter().enumerate() {
            for second in THEMES.iter().skip(a + 1) {
                let (x, y) = (strip(first), strip(second));
                let difference: f32 =
                    x.iter().zip(&y).map(|(p, q)| (p - q).abs()).sum::<f32>() / x.len() as f32;
                assert!(
                    difference > 0.05,
                    "{first} and {second} draw nearly the same border ({difference:.3})"
                );
            }
        }
    }

    #[test]
    fn no_border_is_a_flat_bar() {
        // A flat strip would satisfy "they differ" happily while being
        // a plain painted stripe - the exact thing this replaces.
        for theme in THEMES {
            let values = strip(theme);
            let low = values.iter().copied().fold(f32::MAX, f32::min);
            let high = values.iter().copied().fold(f32::MIN, f32::max);
            assert!(
                high - low > 0.25,
                "{theme}'s border spans only {:.3} - it is a painted stripe",
                high - low
            );
        }
    }

    #[test]
    fn an_unknown_theme_still_gets_a_surface() {
        let values = strip("no-such-theme");
        let low = values.iter().copied().fold(f32::MAX, f32::min);
        let high = values.iter().copied().fold(f32::MIN, f32::max);
        assert!(high - low > 0.25, "the fallback border is flat");
    }

    #[test]
    fn shading_stays_in_range_and_repeats() {
        for theme in THEMES {
            for i in 0..64 {
                for j in 0..64 {
                    let (u, v) = (i as f32 / 64.0, j as f32 / 64.0);
                    let shade = rail_shade(theme, u, v);
                    assert!(
                        (0.0..=1.0).contains(&shade),
                        "{theme} left the range at ({u}, {v}): {shade}"
                    );
                }
            }
            // Deterministic: the texture is generated once per run and
            // has to be the same texture every run.
            assert!(
                (rail_shade(theme, 0.3, 0.7) - rail_shade(theme, 0.3, 0.7)).abs() < f32::EPSILON
            );
        }
    }
}

#[cfg(test)]
mod sustain_pulse_tests {
    use super::sustain_pulse;

    #[test]
    fn a_held_sustain_never_goes_dark() {
        // Blinking out would read as a DROPPED hold, which is a
        // different thing entirely and already has its own picture.
        for step in 0..400 {
            let glow = sustain_pulse(f64::from(step) * 0.01);
            assert!(glow >= 1.5, "the tail dimmed to {glow}");
        }
    }

    #[test]
    fn the_glow_actually_moves() {
        // A constant would satisfy the floor above perfectly well and
        // be a missing feature.
        let values: Vec<f32> = (0..400)
            .map(|s| sustain_pulse(f64::from(s) * 0.01))
            .collect();
        let low = values.iter().copied().fold(f32::MAX, f32::min);
        let high = values.iter().copied().fold(f32::MIN, f32::max);
        assert!(high - low > 2.0, "the pulse spans only {}", high - low);
    }

    #[test]
    fn the_pulse_keeps_time() {
        // Driven from the song clock, so one period is one period
        // however the frame rate wanders.
        let period = 1.0 / 7.0;
        for beat in 0..5 {
            let a = sustain_pulse(0.123);
            let b = sustain_pulse(f64::from(beat).mul_add(period, 0.123));
            assert!((a - b).abs() < 1e-3, "period drifted at {beat}");
        }
    }
}

#[cfg(test)]
mod star_tests {
    use super::{led_pulse, star_outline};

    #[test]
    fn the_backline_tone_really_opposes_the_accent() {
        use super::complementary;
        // Half the hue wheel away, lightness kept: red must come
        // back cyan-ish, not darker red and not grey.
        let red = bevy::color::Color::srgb(0.9, 0.15, 0.1);
        let opposite = bevy::color::Hsla::from(complementary(red).to_srgba());
        let original = bevy::color::Hsla::from(red.to_srgba());
        let delta = (opposite.hue - original.hue).rem_euclid(360.0);
        assert!(
            (delta - 180.0).abs() < 1.0,
            "hue must swing half the wheel, swung {delta}"
        );
        assert!((opposite.lightness - original.lightness).abs() < 1e-4);
        assert!(opposite.saturation > 0.5, "the opposite must stay a colour");
    }

    #[test]
    fn the_floor_pool_and_the_shaft_share_one_angle() {
        use super::beam_angle;
        // The pool slides by tan(angle) x drop from the hanger; the
        // shaft rotates by the same angle — both take it from ONE
        // function, and this pins that the function actually swings
        // (a constant would keep both technically "in sync" while
        // freezing the rig).
        let a = beam_angle(0.0, 0.1, 0.0, 0.5);
        let b = beam_angle(3.0, 0.1, 0.0, 0.5);
        assert!((a - b).abs() > 0.05, "the rig must swing: {a} vs {b}");
        // And the swing stays inside +-0.30 around its base: a shaft
        // past that would rake across the fretboard.
        for step in 0..60 {
            let angle = beam_angle(step as f32 * 0.37, 0.1, 1.1, 0.45);
            assert!((angle - 0.1).abs() <= 0.30 + 1e-6);
        }
    }

    #[test]
    fn the_led_wall_swells_on_the_beat_and_never_shrinks_below_rest() {
        // On the beat: full swell; off the beat: at rest — and the
        // rectified sine never dips under 1.0 (a wall shrinking
        // below its sockets would read as broken panels).
        assert!((led_pulse(0.5, 0.0) - 1.16).abs() < 1e-6);
        assert!((led_pulse(0.0, 0.0) - 1.0).abs() < 1e-6);
        for step in 0..40 {
            let beats = step as f32 * 0.173;
            assert!(led_pulse(beats, 1.3) >= 1.0 - 1e-6);
        }
    }

    #[test]
    fn the_star_is_five_points_of_alternating_radius() {
        let outer = 1.0f32;
        let inner = 0.5f32;
        let outline = star_outline(5, outer, inner);
        assert_eq!(outline.len(), 10, "five points, five valleys");
        for (i, [x, z]) in outline.iter().enumerate() {
            let radius = (x * x + z * z).sqrt();
            let wanted = if i % 2 == 0 { outer } else { inner };
            assert!(
                (radius - wanted).abs() < 1e-5,
                "vertex {i} sits at radius {radius}, wanted {wanted}"
            );
        }
        // The first vertex is the tip pointing up the neck (−Z), so
        // the star reads upright from the player's seat.
        assert!(outline[0][0].abs() < 1e-5 && outline[0][1] < 0.0);
    }
}

#[cfg(test)]
mod instrument_neck_tests {
    use super::*;
    use crate::config::Settings;

    fn round(on: bool) -> Settings {
        Settings {
            round_gems: on,
            ..Default::default()
        }
    }

    #[test]
    fn the_instrument_neck_belongs_to_the_round_style_only() {
        // The rule this round was built under: the 8-bit mode stays
        // untouched. If this gate ever inverted, every 8-bit stage
        // would quietly turn into a wooden neck.
        assert_eq!(neck_style(&round(true)), NeckStyle::Instrument);
        assert_eq!(neck_style(&round(false)), NeckStyle::Neon);
    }

    #[test]
    fn the_instrument_board_is_darker_than_the_neon_one_in_every_theme() {
        // The point of the neck: the gems must be the brightest thing
        // on it. Checked per theme, because the board keeps the
        // theme's hue and a light theme could have slipped through.
        for theme in &crate::theme::THEMES {
            let neon = theme.background.mix(&Color::WHITE, 0.16).luminance();
            let instrument = instrument_board_color(*theme).luminance();
            assert!(
                instrument < neon * 0.6,
                "{}: instrument board {instrument} is not well under neon {neon}",
                theme.id
            );
        }
    }

    #[test]
    fn strings_are_one_shade_for_all_five_lanes() {
        // Lane identity lives in the buttons and the gems; a string
        // that told you its lane would be the neon line back again.
        let theme = crate::theme::THEMES[0];
        let string = string_color(theme);
        for lane in Lane::ALL {
            assert_ne!(
                string,
                theme.lane_color(lane),
                "a string must not be a lane colour"
            );
        }
        // And pale: brighter than the board it lies on.
        assert!(string.luminance() > instrument_board_color(theme).luminance() * 2.0);
    }

    #[test]
    fn saturate_pushes_toward_full_and_never_desaturates() {
        let dull = Color::hsl(30.0, 0.4, 0.5);
        let boosted: bevy::color::Hsla = saturate(dull, 0.5).into();
        assert!(
            (boosted.saturation - 0.7).abs() < 1e-4,
            "got {}",
            boosted.saturation
        );
        // Zero amount is the identity; a full amount is fully saturated.
        let same: bevy::color::Hsla = saturate(dull, 0.0).into();
        assert!((same.saturation - 0.4).abs() < 1e-4);
        let full: bevy::color::Hsla = saturate(dull, 1.0).into();
        assert!((full.saturation - 1.0).abs() < 1e-4);
        // A grey has no hue to saturate toward — it stays grey rather
        // than inventing one. (The first version of `saturate` turned
        // it red: HSL stores a grey's hue as 0.)
        let grey: bevy::color::Srgba = saturate(Color::srgb(0.5, 0.5, 0.5), 1.0).into();
        assert!(
            (grey.red - grey.green).abs() < 1e-4 && (grey.green - grey.blue).abs() < 1e-4,
            "a grey came back as {grey:?}"
        );
        // Out-of-range amounts are clamped, not trusted.
        let over: bevy::color::Hsla = saturate(dull, 7.0).into();
        assert!(over.saturation <= 1.0 + 1e-6);
    }

    #[test]
    fn a_hopo_wears_a_bigger_centre_than_a_strum_note_and_both_fit() {
        // User, 2026-09-02: "der weiße knopf in den hammer button soll
        // größer sein." Larger in absolute terms, not merely relative
        // to the smaller face — and still inside it, or the cap would
        // swallow the coloured ring that names the lane.
        let gem = 0.17f32;
        let strum = centre_radius(gem, false);
        let hopo = centre_radius(gem, true);
        assert!(
            hopo > strum,
            "hopo {hopo} must be larger than strum {strum}"
        );
        assert!(
            hopo < gem * HOPO_FACE,
            "the HOPO centre must sit inside the HOPO face"
        );
        assert!(
            strum < gem,
            "the strum centre must sit inside the strum face"
        );
        // And the HOPO keeps a visible coloured ring around its cap.
        assert!(
            gem * HOPO_FACE - hopo > gem * 0.15,
            "a coloured ring must remain"
        );
    }

    #[test]
    fn the_strum_note_is_a_black_ring_around_a_small_point() {
        // The genre's own description, verified against two sources
        // after the user reported "all buttons now have a white dot":
        // regular notes have a black circle AROUND the white circle.
        // So the ring must start at the centre's edge, be wider than
        // the centre is, and leave coloured cap outside it.
        let gem = 0.17f32;
        let centre = centre_radius(gem, false);
        let (inner, outer) = face_ring_radii(gem);
        assert!(
            (inner - centre).abs() < 1e-6,
            "the ring starts where the point ends"
        );
        assert!(
            outer - inner > centre,
            "the ring is the feature, wider than the point"
        );
        assert!(outer < gem * 0.6, "and coloured cap remains outside it");
    }

    #[test]
    fn the_sustain_core_is_paler_than_its_lane() {
        let theme = crate::theme::THEMES[0];
        for lane in Lane::ALL {
            let core: bevy::color::Hsla = core_color(&lane, theme).into();
            let own: bevy::color::Hsla = theme.lane_color(lane).into();
            assert!(
                core.lightness > own.lightness,
                "the core must be the lighter one"
            );
        }
    }

    #[test]
    fn the_fog_leaves_the_strike_line_clear_and_the_far_end_dim() {
        // Camera distances from setup_stage's placement.
        let strike = 6.1f32;
        let far_end = 31.4f32;
        let back_wall = 45.0f32;
        let intensity = |d: f32| 1.0 - ((FOG_END - d) / (FOG_END - FOG_START)).clamp(0.0, 1.0);
        assert!(
            intensity(strike) < 1e-6,
            "the strike line must not be fogged at all"
        );
        let end = intensity(far_end);
        assert!(
            (0.35..=0.65).contains(&end),
            "the far end should be about half fogged, got {end}"
        );
        let wall = intensity(back_wall);
        assert!(
            wall < 1.0,
            "the venue must never vanish into a hole, got {wall}"
        );
        assert!(wall > end, "the venue must recede further than the neck");
    }
}
