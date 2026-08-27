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

/// Build the board texture. Generated, never loaded: every asset in
/// this repository has to be original, and a plank is arithmetic.
fn board_texture() -> Image {
    const SIZE: usize = 128;
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let shade = board_shade(
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            );
            let v = (shade * 255.0) as u8;
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    // Repeat, because the texture is tiled many times down the neck.
    image.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..bevy::image::ImageSamplerDescriptor::linear()
    });
    image
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

/// Wash the neck with the energy colour while hype is running.
pub fn tint_stage_for_hype(
    settings: Res<Settings>,
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    surfaces: Query<(&HypeTinted, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut blend: Local<Vec<f32>>,
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
    for (surface, material) in &surfaces {
        let Some(&eased) = blend.get(surface.player) else {
            continue;
        };
        let amount = eased * surface.reach;
        if let Some(mut paint) = materials.get_mut(&material.0) {
            paint.base_color = surface.base.mix(&palette::HYPE, amount);
            let glow = surface.glow_lift.mul_add(amount, surface.base_glow);
            paint.emissive = paint.base_color.to_linear() * glow;
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
    let Some(now) = game_clock.song_time(&time) else {
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

/// Where a sustain's remaining tail sits, and how long it still is.
///
/// Returns `None` once nothing is left to hold. Pure, because the
/// half-length offset is exactly the kind of arithmetic that has gone
/// wrong here before — the depth view once drew tails that stood
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
    stage: crate::theme::Theme,
    motion: bool,
) {
    let dark = stage.background.mix(&Color::BLACK, 0.35);
    // Deep, not grey. The first attempt lit the rear wall to roughly
    // the brightness of the fretboard, so it read as a blank screen
    // hung behind the vanishing point and flattened the contrast of
    // the notes furthest away.
    let wall_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.55).mix(&stage.accent, 0.07),
        perceptual_roughness: 1.0,
        ..default()
    });
    let trim_material = materials.add(StandardMaterial {
        base_color: stage.accent,
        emissive: stage.accent.to_linear() * 0.55,
        ..default()
    });
    let box_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::WHITE, 0.06),
        perceptual_roughness: 0.9,
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
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(0.0, 12.0, VENUE_BACK),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // A lit band across the wall, level with the horizon — it gives
    // the room a floor line and stops the wall reading as fog.
    let band = meshes.add(Cuboid::new(90.0, 0.28, 0.2));
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(band),
        MeshMaterial3d(trim_material.clone()),
        Transform::from_xyz(0.0, 0.12, VENUE_BACK + 0.5),
        RenderLayers::layer(STAGE_LAYER),
    ));

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

    // Lighting truss overhead, with a rig of beams hanging from it.
    let truss = meshes.add(Cuboid::new(VENUE_SIDE * 1.7, 0.18, 0.18));
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Venue,
        Mesh3d(truss),
        MeshMaterial3d(box_material.clone()),
        Transform::from_xyz(0.0, 9.0, -13.0),
        RenderLayers::layer(STAGE_LAYER),
    ));

    // Cones stand in for beams. Tall, narrow, unlit-bright and
    // alpha-blended, which is what a light shaft looks like without a
    // volumetric pass.
    // Narrow, and deliberately kept outside the bed: the first
    // attempt used radius 1.5 cones starting at the centre, which
    // washed a red haze straight across the fretboard.
    let beam = meshes.add(Cone {
        radius: 0.7,
        height: 7.0,
    });
    let beam_material = materials.add(StandardMaterial {
        base_color: stage.accent.with_alpha(0.045),
        emissive: stage.accent.to_linear() * 0.5,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    // Three a side, none closer to the centre than the speaker
    // stacks — the neck keeps a clear corridor.
    for index in 0..6 {
        let side = if index % 2 == 0 { -1.0 } else { 1.0 };
        let x = side * (3.4 + 1.7 * (index / 2) as f32);
        let entity = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                Mesh3d(beam.clone()),
                MeshMaterial3d(beam_material.clone()),
                // Point down and slightly away from the neck, so the
                // shafts never wash over the notes.
                Transform::from_xyz(x, 6.4, -13.0).with_rotation(Quat::from_rotation_z(x * 0.05)),
                RenderLayers::layer(STAGE_LAYER),
            ))
            .id();
        if motion {
            commands.entity(entity).insert(SpotBeam {
                base: x * 0.05,
                phase: index as f32 * 1.1,
                speed: 0.35 + 0.05 * index as f32,
            });
        }
    }

    // Speaker stacks flanking the near end: they give the neck a sense
    // of scale, which a bare board in a room does not have.
    let cab = meshes.add(Cuboid::new(1.3, 0.9, 1.1));
    for sign in [-1.0f32, 1.0] {
        for level in 0..3 {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Venue,
                Mesh3d(cab.clone()),
                MeshMaterial3d(box_material.clone()),
                Transform::from_xyz(sign * 4.4, 0.42 + 0.95 * level as f32, -7.0),
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
    let head = meshes.add(Sphere::new(0.36).mesh().uv(8, 6));
    let head_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.55),
        perceptual_roughness: 1.0,
        ..default()
    });
    for index in 0..48 {
        let row = index % 3;
        let seat = index / 6;
        let sign = if index % 2 == 0 { -1.0 } else { 1.0 };
        // Three ranks deep on each side, the back ranks a little
        // higher, as a floor sloping away from the stage would put
        // them.
        let x = sign * (3.2 + 0.85 * row as f32);
        let z = -13.5 - 2.4 * seat as f32 + 0.6 * row as f32;
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Venue,
            Mesh3d(head.clone()),
            MeshMaterial3d(head_material.clone()),
            Transform::from_xyz(x, 0.12 + 0.16 * row as f32, z),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }
}

/// Sweep the light beams. Gated on the Stage Motion setting, like
/// every other ambient movement in the game.
fn sweep_beams(time: Res<Time>, mut beams: Query<(&SpotBeam, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (beam, mut transform) in &mut beams {
        let swing = (now * beam.speed + beam.phase).sin();
        transform.rotation = Quat::from_rotation_z(beam.base + swing * 0.30);
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
) {
    if !active(&settings) {
        return;
    }
    let stage = theme.0;

    // The camera sits behind and above the hit line, tilted down the
    // neck. This is the framing the genre settled on: close enough
    // that the receptors are large and readable, high enough that the
    // approaching notes separate instead of overlapping.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Camera3d::default(),
        Camera {
            order: -1,
            ..default()
        },
        // HDR is a marker component in this Bevy version; bloom
        // requires it.
        bevy::camera::Hdr,
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0f32.to_radians(),
            ..default()
        }),
        // Further back and higher than the first attempt: at 2.35/3.6
        // the nearest gems filled a third of the screen and the row of
        // receptors ran off both edges of the bed.
        Transform::from_xyz(0.0, 3.1, 5.2).looking_at(Vec3::new(0.0, 0.05, -7.5), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
        bevy::post_process::bloom::Bloom {
            intensity: 0.18,
            ..bevy::post_process::bloom::Bloom::NATURAL
        },
    ));

    // Key light down the neck plus a soft fill, so gems read as
    // spheres rather than flat discs.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        DirectionalLight {
            illuminance: 5_500.0,
            ..default()
        },
        Transform::from_xyz(2.0, 6.0, 2.0).looking_at(Vec3::new(0.0, 0.0, -8.0), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
    ));
    // Ambient light is a component on the camera in this version.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        AmbientLight {
            color: stage.accent,
            brightness: 220.0,
            ..default()
        },
        RenderLayers::layer(STAGE_LAYER),
    ));

    spawn_venue(
        &mut commands,
        &mut meshes,
        &mut materials,
        stage,
        settings.backdrop_motion,
    );

    let board = images.add(board_texture());
    let bed = meshes.add(Cuboid::new(1.0, 0.06, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let rail = meshes.add(Cuboid::new(0.035, 0.05, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let lane_strip = meshes.add(Cuboid::new(0.018, 0.012, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    // A ring, not a disc: with both drawn as discs a resting receptor
    // and an approaching note were the same shape.
    let receptor_mesh = meshes.add(Torus::new(GEM_RADIUS * 0.82, GEM_RADIUS * 1.12));
    let fill_mesh = meshes.add(Cylinder::new(GEM_RADIUS * 0.88, 0.03));
    // Flat and wide: the burst spreads ACROSS the board rather than
    // rising off it, which is what the genre's flame does.
    let burst_mesh = meshes.add(Cylinder::new(GEM_RADIUS * 1.9, 0.012));
    let hit_bar = meshes.add(Cuboid::new(1.0, 0.02, 0.06));

    for index in &players {
        let player = index.0;
        let origin = layout.origin(player) * WORLD_PER_PIXEL;
        // A little wider than the lane span so the outer receptors
        // sit ON the neck rather than half off it.
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.18 * neck_spread(&layout);
        let centre = -HIGHWAY_LENGTH / 2.0 + HIGHWAY_BEHIND / 2.0;

        // The bed. Dark and slightly reflective so the lights and the
        // gems have something to sit on.
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(bed.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                // Light enough to read AS a fretboard: against a
                // black bed the gems floated in a void and the neck
                // had no surface for the lights to land on.
                base_color: stage.background.mix(&Color::WHITE, 0.16),
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
                base: stage.background.mix(&Color::WHITE, 0.16),
                base_glow: 0.0,
                glow_lift: 0.0,
                reach: 0.55,
            },
            Transform::from_xyz(origin, -0.03, centre).with_scale(Vec3::new(width, 1.0, 1.0)),
            RenderLayers::layer(STAGE_LAYER),
        ));

        // Bright rails down both edges. They frame the neck and, more
        // usefully, give the eye a fixed reference for how wide the
        // playfield is as it recedes.
        for side in [-1.0f32, 1.0] {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(rail.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.accent,
                    emissive: stage.accent.to_linear() * 2.6,
                    ..default()
                })),
                HypeTinted {
                    player,
                    base: stage.accent,
                    base_glow: 2.6,
                    glow_lift: 0.8,
                    reach: 0.9,
                },
                Transform::from_xyz(origin + side * width / 2.0, 0.015, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // One glowing strip per lane, which is what gives the neck its
        // sense of depth as it recedes.
        for lane in Lane::ALL {
            let colour = stage.lane_color(lane);
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(lane_strip.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour,
                    emissive: colour.to_linear() * 1.4,
                    unlit: false,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.005, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));

            // The fill sits inside the ring and is what actually
            // shows a press.
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
        Without<Receptor3d>,
    >,
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
    let bar_s = 240.0 / bpm;
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
        while t < end {
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
                FretBar { time_s: t },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                // Parked off-screen until the scroll system places it.
                Transform::from_xyz(origin, 0.012, -900.0).with_scale(Vec3::new(width, 1.0, 1.0)),
                RenderLayers::layer(STAGE_LAYER),
            ));
            t += bar_s;
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
    let Some(now) = game_clock.song_time(&time) else {
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
            surface.base_color = Color::srgba(0.75, 0.78, 0.85, fade * 0.85);
            surface.emissive = LinearRgba::rgb(0.35, 0.36, 0.42) * fade;
        }
    }
}

/// Reusable geometry and materials for the 3D notes, built once.
#[derive(Resource)]
pub struct NoteAssets {
    gem: Handle<Mesh>,
    rim: Handle<Mesh>,
    hopo: Handle<Mesh>,
    hopo_rim: Handle<Mesh>,
    sustain: Handle<Mesh>,
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
) {
    if !active(&settings) {
        return;
    }
    // The genre's gem is a flat BUTTON lying on the fretboard — a
    // coloured disc inside a dark rim — not a floating sphere. Seen
    // from the player's angle it reads as an ellipse, which is what
    // makes a five-lane row scannable at speed.
    // The gem radius is a world-unit constant, so widening the neck
    // without it would leave the notes undersized in their own lanes.
    let gem = GEM_RADIUS * neck_spread(&layout);
    commands.insert_resource(NoteAssets {
        gem: meshes.add(Cylinder::new(gem, 0.055)),
        rim: meshes.add(Cylinder::new(gem * 1.28, 0.042)),
        // A HOPO is smaller and reads as a different object, the way
        // the 2D views distinguish it.
        hopo: meshes.add(Cylinder::new(gem * 0.62, 0.05)),
        hopo_rim: meshes.add(Cylinder::new(gem * 0.86, 0.04)),
        sustain: meshes.add(Cylinder::new(0.05 * neck_spread(&layout), 1.0)),
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
            base_color: Color::srgb(0.05, 0.05, 0.07),
            perceptual_roughness: 0.6,
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
                let colour = theme.0.lane_color(*lane);
                materials.add(StandardMaterial {
                    base_color: colour,
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
) {
    if !active(&settings) {
        return;
    }
    let (Some(now), Some(assets)) = (game_clock.song_time(&time), assets) else {
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
                // The dark rim the coloured face sits in.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Note3d {
                        player: index.0,
                        event_index: cursor,
                        lane,
                    },
                    Mesh3d(if hopo {
                        assets.hopo_rim.clone()
                    } else {
                        assets.rim.clone()
                    }),
                    MeshMaterial3d(if in_phrase {
                        assets.hype_rim_material.clone()
                    } else {
                        assets.rim_material.clone()
                    }),
                    Transform::from_xyz(lane_x(&layout, index.0, lane), 0.06, z),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                // A sustain is a tube running back up the neck from
                // the gem — length is the note's own held time.
                if event.is_sustain() {
                    let length = (event.sustain_s as f32) * settings.scroll_speed * Z_PER_PIXEL;
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
                        MeshMaterial3d(material),
                        // The cylinder is built along Y, so it is
                        // rotated onto the neck's Z axis and pushed
                        // back by half its length.
                        Transform::from_xyz(lane_x(&layout, index.0, lane), 0.05, z - length / 2.0)
                            .with_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(1.0, length, 1.0)),
                        RenderLayers::layer(STAGE_LAYER),
                    ));
                }
            }
            player.spawn_cursor += 1;
        }
    }
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
    let Some(now) = game_clock.song_time(&time) else {
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
pub fn consume_sustains(
    mut commands: Commands,
    settings: Res<Settings>,
    time: Res<Time>,
    game_clock: Res<GameClock>,
    assets: Option<Res<NoteAssets>>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut tails: Query<(Entity, &Note3d, &mut Transform), With<SustainTail3d>>,
) {
    if !active(&settings) {
        return;
    }
    let (Some(now), Some(assets)) = (game_clock.song_time(&time), assets) else {
        return;
    };
    for (entity, note, mut transform) in &mut tails {
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
            }
            // Fully played: the tail has been eaten, nothing to show.
            None => commands.entity(entity).despawn(),
        }
    }

    // A tail whose hold has ended but which still has length left was
    // DROPPED — it greys out and slides away, so letting go looks
    // different from playing it out.
    for (entity, note, transform) in &tails {
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
                update_receptors,
                apply_note_events,
                sweep_beams,
            )
                .chain()
                .run_if(in_state(crate::states::GamePhase::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
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
