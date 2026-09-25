//! The Star-Power arc: lightning along the rails while Hype runs.
//!
//! The genre's edge during its power state is electric — jagged
//! bolts crackling down both sides of the neck, forking, jumping to
//! new shapes many times a second, never the same twice. (The first
//! idea here was a sheet of fire; it read as an ice fence and the
//! user asked for the bolt instead.)
//!
//! Since 0.18.49 the bolt is **electric blue with a hot core**: every
//! segment carries a thicker blue hull as a child, the core stays
//! near white so the bolt does not go dull, and the two add. Around
//! both rails lies an **aura** — a broad cyan-blue glow on the deck
//! outside the neck that pulses and flickers WITH the bolts (it reads
//! its envelope from the same flash the bolt flashes on). A steady
//! strip would read as a fence, the way the fire did; the aura is
//! never steady while the bolt crackles. And the whole thing
//! **surges toward the horizon**: a wave runs along the rail away
//! from the player, lifting the bolt's height and thickness and the
//! aura's brightness as it passes — the direction the notes come
//! from, so the power reads as running up the neck.
//!
//! Built from what the project already renders: each rail carries a
//! fixed pool of thin additive **segments** chained along z. Every
//! crackle step (24 Hz) each segment's endpoints are re-rolled from a
//! hash of `(rail, segment, step)` — so the bolt jumps like lightning
//! does — and a few segments drop out (a bolt is not a continuous
//! wire). **Forks** are a second, shorter pool, each re-anchored to a
//! random segment per step and thrown off at an angle. Brightness
//! flashes by thickness, not by material: shared materials, zero
//! material writes, zero allocation per frame. The aura's brightness
//! is a **material-handle swap** among a fixed ladder of levels, the
//! way the strobe's beams go white — never a material write.
//!
//! `reduced_flashing` turns the crackle into a slow wander (2 Hz),
//! with no gaps and no flashes — a steady arc instead of a strobe;
//! the aura holds one level and the surge crawls at half speed and
//! a third of its amplitude. `fx_intensity` scales how many segments
//! and forks are live, and the aura's brightness with them.

use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::stage3d::{STAGE_LAYER, Stage3d, rail_x};
use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession};
use crate::config::Settings;

/// Segments per bolt, chained along the rail.
pub const SEGMENTS: usize = 40;
/// Bolts per rail; they overlap and add, which is what gives the
/// arc its bright, uneven core.
pub const BOLTS_PER_RAIL: usize = 2;
/// Forks per rail.
pub const FORKS_PER_RAIL: usize = 10;
/// How often the bolt jumps to a new shape.
pub const CRACKLE_HZ: f32 = 24.0;
/// The wander rate under reduced flashing.
pub const CALM_HZ: f32 = 2.0;
/// The rail's span in z the arc runs along.
const SPAN: (f32, f32) = (-25.0, 2.0);
/// Sideways jitter of a bolt point, world units.
pub const JITTER_X: f32 = 0.22;
/// Height range of a bolt point above the rail.
const HEIGHT: (f32, f32) = (0.04, 0.34);
/// Share of segments dropped per step: the gaps in a bolt.
const GAP_SHARE: f32 = 0.12;
/// Share of steps that flash bright.
const FLASH_SHARE: f32 = 0.08;
/// The thickness factor of a flash step, and the floor of an
/// ordinary one — the range [`flash`] spans.
const FLASH_PEAK: f32 = 2.2;
const FLASH_FLOOR: f32 = 0.7;

/// The bolt's core: white pushed toward blue — hot, so the blue hull
/// around it does not go dull.
pub const CORE: Color = Color::srgb(0.6, 0.85, 1.0);
/// The hull around the core: electric blue.
pub const HULL: Color = Color::srgb(0.08, 0.4, 1.0);
/// The aura's cyan-blue.
pub const AURA: Color = Color::srgb(0.12, 0.72, 1.0);
/// The emissive push of the core and the hull.
pub const CORE_GLOW: f32 = 2.4;
/// The emissive push of the hull.
pub const HULL_GLOW: f32 = 2.2;
/// How much thicker the hull is than the core it wraps.
pub const HULL_FACTOR: f32 = 3.2;

/// How far the aura reaches OUT from the rail, away from the neck.
pub const AURA_OUT: f32 = 1.4;
/// How far it reaches IN over the neck — a lip, kept short and faint
/// so the outer lane's gems stay legible (the rail stands ~0.4 world
/// units outside the outer gem's collar).
pub const AURA_IN: f32 = 0.14;
/// How bright the lip's inner edge is, as a share of the rail's.
const AURA_LIP: f32 = 0.3;
/// Slabs the aura is cut into along z, so the surge can run along it.
pub const AURA_SLABS: usize = 8;
/// The ladder of aura brightness levels (material handles), level 0
/// being off.
pub const AURA_LEVELS: usize = 12;
/// The aura's emissive at its brightest level.
const AURA_GLOW: f32 = 6.0;
/// The aura's height at the rail (just above the neck's bed).
const AURA_Y: f32 = 0.012;
/// The deck outside the neck lies this far BELOW the rail (the neck
/// is a platform on the riser); the aura drops onto it over
/// `AURA_DROP` of its width and runs flat from there.
pub const AURA_DECK_DROP: f32 = 0.3;
const AURA_DROP: f32 = 0.15;
/// The aura's width while the arc is out: not zero, so the slab is
/// still drawn (dark) and its pipeline stays warm.
const AURA_DORMANT: f32 = 0.001;
/// How the aura throbs on its own, slowly, under the flicker.
const AURA_THROB_HZ: f32 = 1.6;

/// The surge's speed along the rail toward the horizon, world units
/// per second, and its wavelength.
pub const SURGE_SPEED: f32 = 12.0;
/// The surge's wavelength along the rail.
pub const SURGE_WAVELENGTH: f32 = 7.5;
/// Under reduced flashing the surge crawls and barely lifts.
const SURGE_CALM_SPEED: f32 = 0.5;
const SURGE_CALM_AMP: f32 = 0.33;
/// How much a passing surge lifts a bolt point's height, its
/// thickness, and a fork's length.
const SURGE_HEIGHT: f32 = 0.9;
const SURGE_THICK: f32 = 0.4;
const SURGE_FORK: f32 = 0.7;
/// How much of the aura's brightness rides on the surge.
const SURGE_AURA: f32 = 0.35;

/// One segment of a bolt.
#[derive(Component)]
pub struct BoltSegment {
    /// Owning player.
    pub player: usize,
    /// −1 left rail, +1 right.
    pub side: f32,
    /// Which bolt of the rail's set.
    pub bolt: usize,
    /// Position along the chain.
    pub segment: usize,
}

/// One fork off a bolt.
#[derive(Component)]
pub struct BoltFork {
    /// Owning player.
    pub player: usize,
    /// −1 left rail, +1 right.
    pub side: f32,
    /// Position in the fork pool.
    pub index: usize,
}

/// One slab of a rail's aura.
#[derive(Component)]
pub struct AuraSlab {
    /// Owning player.
    pub player: usize,
    /// −1 left rail, +1 right.
    pub side: f32,
    /// Position along the rail.
    pub slab: usize,
    /// The level whose material it wears right now.
    pub level: usize,
}

/// The aura's ladder of materials, level 0 to `AURA_LEVELS - 1`,
/// made once at spawn and swapped per frame — never written.
#[derive(Resource)]
pub struct AuraMaterials(pub Vec<Handle<StandardMaterial>>);

/// Which crackle step `now` falls in at `hz`. Pure — tested.
#[must_use]
pub fn step(now: f32, hz: f32) -> u32 {
    (now.max(0.0) * hz).floor() as u32
}

/// A seed for a rail of a player.
fn rail_seed(player: usize, side: f32, bolt: usize) -> usize {
    player * 7919 + if side < 0.0 { 11 } else { 13 } + bolt * 977
}

/// The lateral/vertical offset of bolt point `index` at `step`:
/// `(dx, dy)`. Deterministic in its inputs, so the whole bolt is a
/// function of time and not of history. Pure — tested.
#[must_use]
pub fn point(seed: usize, index: usize, step: u32) -> (f32, f32) {
    let a = super::fx::hash01(seed + index * 131 + step as usize * 7);
    let b = super::fx::hash01(seed + index * 197 + step as usize * 3 + 1);
    let dx = (a - 0.5) * 2.0 * JITTER_X;
    let dy = HEIGHT.0 + (HEIGHT.1 - HEIGHT.0) * b;
    (dx, dy)
}

/// The z of bolt point `index`.
#[must_use]
pub fn point_z(index: usize) -> f32 {
    SPAN.0 + (SPAN.1 - SPAN.0) * index as f32 / SEGMENTS as f32
}

/// Whether segment `index` is dropped at `step` — the gap that keeps
/// a bolt from reading as a wire. Never under reduced flashing.
/// Pure — tested.
#[must_use]
pub fn gapped(seed: usize, index: usize, step: u32, calm: bool) -> bool {
    !calm && super::fx::hash01(seed + index * 311 + step as usize * 17 + 5) < GAP_SHARE
}

/// The thickness factor at `step`: an uneven crackle, with the odd
/// bright flash. Steady under reduced flashing. Pure — tested.
#[must_use]
pub fn flash(seed: usize, step: u32, calm: bool) -> f32 {
    if calm {
        return 1.0;
    }
    let roll = super::fx::hash01(seed + step as usize * 23 + 2);
    if roll < FLASH_SHARE {
        FLASH_PEAK
    } else {
        FLASH_FLOOR + 0.8 * super::fx::hash01(seed + step as usize * 29 + 9)
    }
}

/// The surge at `z` and `now`: a wave, 0..1, running along the rail
/// toward the horizon (−z, where the notes come from) at
/// [`SURGE_SPEED`]. What is at `z` now is at `z − speed·dt` a `dt`
/// later. Under reduced flashing it crawls at half speed and rises
/// to a third. Pure — tested.
#[must_use]
pub fn surge(z: f32, now: f32, calm: bool) -> f32 {
    let (speed, amp) = if calm {
        (SURGE_SPEED * SURGE_CALM_SPEED, SURGE_CALM_AMP)
    } else {
        (SURGE_SPEED, 1.0)
    };
    // A feature of `z + speed·t` moves toward −z as t grows.
    let phase = ((z + speed * now) / SURGE_WAVELENGTH).rem_euclid(1.0);
    // A crest with a sharp peak and a long tail: near the crest it
    // is 1, a quarter wavelength away it is nearly gone.
    let crest = 1.0 - (phase - 0.5).abs() * 2.0;
    amp * crest * crest * crest
}

/// The aura's envelope at `step` and `now`, 0..1: a slow throb under
/// a flicker that follows the bolt's own [`flash`] — on a flash step
/// the aura flares with the bolt. Steady under reduced flashing.
/// Pure — tested.
#[must_use]
pub fn aura_envelope(seed: usize, step: u32, now: f32, calm: bool) -> f32 {
    if calm {
        return 0.85;
    }
    let throb = 0.5 + 0.5 * (now * AURA_THROB_HZ * std::f32::consts::TAU).sin();
    let flicker = (flash(seed, step, false) - FLASH_FLOOR) / (FLASH_PEAK - FLASH_FLOOR);
    (0.45 + 0.2 * throb + 0.35 * flicker).clamp(0.0, 1.0)
}

/// The aura's material level for a brightness: `brightness` is the
/// envelope times how far the arc has grown in, times the surge lift
/// and the effects intensity — 0 is off, the top level full glow.
/// Pure — tested.
#[must_use]
pub fn aura_level(envelope: f32, grown: f32, lift: f32, intensity: f32) -> usize {
    let brightness = envelope * grown * lift * intensity.clamp(0.0, 1.0);
    let top = (AURA_LEVELS - 1) as f32;
    ((brightness.clamp(0.0, 1.0) * top).round() as usize).min(AURA_LEVELS - 1)
}

/// Place a unit-length segment mesh between `a` and `b`: returns
/// `(midpoint, rotation, length)`. Pure — tested.
#[must_use]
pub fn segment_pose(a: Vec3, b: Vec3) -> (Vec3, Quat, f32) {
    let delta = b - a;
    let length = delta.length();
    if length < 1e-6 {
        return (a, Quat::IDENTITY, 0.0);
    }
    (
        a + delta * 0.5,
        Quat::from_rotation_arc(Vec3::X, delta / length),
        length,
    )
}

/// A fork's anchor segment and direction at `step`: `(segment
/// index, direction, length)`. Pure — tested.
#[must_use]
pub fn fork_shape(seed: usize, index: usize, step: u32, side: f32) -> (usize, Vec3, f32) {
    let a = super::fx::hash01(seed + index * 419 + step as usize * 11);
    let b = super::fx::hash01(seed + index * 523 + step as usize * 13 + 3);
    let c = super::fx::hash01(seed + index * 619 + step as usize * 19 + 7);
    let anchor = ((a * SEGMENTS as f32) as usize).min(SEGMENTS - 1);
    // Off the rail and up, with a little run along z.
    let dir = Vec3::new(side * (0.4 + 0.6 * b), 0.5 + 0.8 * c, (b - 0.5) * 0.8).normalize();
    (anchor, dir, 0.25 + 0.35 * c)
}

/// The z range of aura slab `slab`.
#[must_use]
pub fn slab_span(slab: usize) -> (f32, f32) {
    let length = (SPAN.1 - SPAN.0) / AURA_SLABS as f32;
    let start = SPAN.0 + length * slab as f32;
    (start, start + length)
}

/// The aura's brightness across its width at `x` (0 = the rail,
/// negative = over the neck), 0..1: full at the rail, a faint lip
/// over the neck, falling to nothing at the outer edge. Baked into
/// the gradient texture once; pure — tested.
#[must_use]
pub fn aura_alpha(x: f32) -> f32 {
    // (x, alpha) knots, linear between them.
    let knots: [(f32, f32); 7] = [
        (-AURA_IN, AURA_LIP),
        (0.0, 1.0),
        (AURA_DROP, 0.85),
        (AURA_OUT * 0.42, 0.5),
        (AURA_OUT * 0.7, 0.16),
        (AURA_OUT * 0.88, 0.03),
        (AURA_OUT, 0.0),
    ];
    if x <= knots[0].0 {
        return knots[0].1;
    }
    for pair in knots.windows(2) {
        let ((x0, a0), (x1, a1)) = (pair[0], pair[1]);
        if x <= x1 {
            return a0 + (a1 - a0) * ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        }
    }
    0.0
}

/// The gradient texture the aura wears: [`aura_alpha`] across u,
/// white, one row. Texture rather than vertex colour so the slab
/// draws through the SAME pipeline as the rig's beams (unlit,
/// additive, textured; position + normal + uv) — a vertex layout of
/// its own would compile a pipeline of its own on its first frame,
/// and that compile is a stall in the middle of play (seen as a
/// one-second clock teleport by the autopilot, twice).
#[must_use]
pub fn aura_gradient() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{Image, ImageSampler};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const WIDTH: usize = 256;
    let mut data = Vec::with_capacity(WIDTH * 4);
    for texel in 0..WIDTH {
        let u = (texel as f32 + 0.5) / WIDTH as f32;
        let alpha = aura_alpha(-AURA_IN + u * (AURA_IN + AURA_OUT));
        data.extend_from_slice(&[255, 255, 255, (alpha.clamp(0.0, 1.0) * 255.0) as u8]);
    }
    let mut image = Image::new(
        Extent3d {
            width: WIDTH as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// One aura slab's mesh: a strip from `AURA_IN` inside the rail
/// (x < 0) to `AURA_OUT` outside it (x > 0). From the rail it drops
/// onto the deck and runs out along it — a glow that hugs the
/// platform's edge, not a sheet in the air. Its u runs across the
/// width, so the gradient texture does the fade. Built once at spawn.
#[must_use]
pub fn slab_mesh(slab: usize) -> Mesh {
    let (z0, z1) = slab_span(slab);
    let deck = -AURA_DECK_DROP;
    // Columns across the strip: (x, y).
    let columns: [(f32, f32); 7] = [
        (-AURA_IN, 0.0),
        (0.0, 0.0),
        (AURA_DROP, deck),
        (AURA_OUT * 0.42, deck),
        (AURA_OUT * 0.7, deck),
        (AURA_OUT * 0.88, deck),
        (AURA_OUT, deck),
    ];
    let mut positions = Vec::with_capacity(columns.len() * 2);
    let mut uvs = Vec::with_capacity(columns.len() * 2);
    for (x, y) in columns {
        let u = (x + AURA_IN) / (AURA_IN + AURA_OUT);
        positions.push([x, y, z0]);
        positions.push([x, y, z1]);
        uvs.push([u, 0.5]);
        uvs.push([u, 0.5]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let mut indices = Vec::with_capacity((columns.len() - 1) * 6);
    for column in 0..(columns.len() as u32 - 1) {
        let a = column * 2;
        indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
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

/// An additive, strongly emissive material in `tone`, never written
/// after this. Shared with the star-power strike (`strike.rs`), which
/// wears the same core and hull.
pub fn bolt_material(
    materials: &mut Assets<StandardMaterial>,
    tone: Color,
    glow: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: tone.with_alpha(0.85),
        emissive: tone.to_linear() * glow,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

/// Spawn the arc pools for every rail. Instrument neck only.
pub fn spawn_arcs(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !super::stage3d::active(&settings) {
        return;
    }
    let layer = RenderLayers::layer(STAGE_LAYER);
    let segment = meshes.add(Cuboid::new(1.0, 0.035, 0.035));
    let fork = meshes.add(Cuboid::new(1.0, 0.022, 0.022));
    // The hull is a child of its core and inherits the core's pose
    // and scale, so it costs nothing per frame; its mesh is simply
    // thicker.
    let segment_hull = meshes.add(Cuboid::new(1.0, 0.035 * HULL_FACTOR, 0.035 * HULL_FACTOR));
    let fork_hull = meshes.add(Cuboid::new(1.0, 0.022 * HULL_FACTOR, 0.022 * HULL_FACTOR));
    let core = bolt_material(&mut materials, CORE, CORE_GLOW);
    let hull = bolt_material(&mut materials, HULL, HULL_GLOW);
    // The aura's ladder: level 0 is dark, the top is full glow. The
    // lit base colour stays faint so the stage lights do not paint
    // the slab; the glow is the emissive.
    // Unlit and textured like the rig's beams (same pipeline, see
    // `aura_gradient`): the glow is the base colour pushed past 1.0
    // in linear space, which the bloom pass turns into light; the
    // texture's alpha fades it (under additive blending the colour
    // is premultiplied by the alpha). Alpha itself stays 1 — a base
    // alpha that scaled with the level cut the glow by the same
    // factor twice over, and the first cut was invisible.
    let gradient = images.add(aura_gradient());
    let ladder: Vec<Handle<StandardMaterial>> = (0..AURA_LEVELS)
        .map(|level| {
            let share = level as f32 / (AURA_LEVELS - 1) as f32;
            let glow = AURA.to_linear() * (AURA_GLOW * share);
            materials.add(StandardMaterial {
                base_color: Color::LinearRgba(glow.with_alpha(1.0)),
                base_color_texture: Some(gradient.clone()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    let slab_meshes: Vec<Handle<Mesh>> = (0..AURA_SLABS)
        .map(|slab| meshes.add(slab_mesh(slab)))
        .collect();
    for index in &players {
        let player = index.0;
        for side in [-1.0f32, 1.0] {
            let x = rail_x(&layout, player, side);
            for bolt in 0..BOLTS_PER_RAIL {
                for seg in 0..SEGMENTS {
                    commands
                        .spawn((
                            GameplayScreen,
                            Stage3d,
                            BoltSegment {
                                player,
                                side,
                                bolt,
                                segment: seg,
                            },
                            super::stage3d::on_the_neck(),
                            Mesh3d(segment.clone()),
                            MeshMaterial3d(core.clone()),
                            Transform::from_xyz(x, 0.1, point_z(seg)).with_scale(Vec3::ZERO),
                            Visibility::Hidden,
                            layer.clone(),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                Stage3d,
                                super::stage3d::on_the_neck(),
                                Mesh3d(segment_hull.clone()),
                                MeshMaterial3d(hull.clone()),
                                Transform::IDENTITY,
                                layer.clone(),
                            ));
                        });
                }
            }
            for index in 0..FORKS_PER_RAIL {
                commands
                    .spawn((
                        GameplayScreen,
                        Stage3d,
                        BoltFork {
                            player,
                            side,
                            index,
                        },
                        super::stage3d::on_the_neck(),
                        Mesh3d(fork.clone()),
                        MeshMaterial3d(core.clone()),
                        Transform::from_xyz(x, 0.1, 0.0).with_scale(Vec3::ZERO),
                        Visibility::Hidden,
                        layer.clone(),
                    ))
                    .with_children(|parent| {
                        parent.spawn((
                            Stage3d,
                            super::stage3d::on_the_neck(),
                            Mesh3d(fork_hull.clone()),
                            MeshMaterial3d(hull.clone()),
                            Transform::IDENTITY,
                            layer.clone(),
                        ));
                    });
            }
            for (slab, mesh) in slab_meshes.iter().enumerate() {
                // The slab's x runs outward from 0; the side's sign is
                // in the scale, and so is how far the arc has grown.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    AuraSlab {
                        player,
                        side,
                        slab,
                        level: 0,
                    },
                    super::stage3d::on_the_neck(),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(ladder[0].clone()),
                    Transform::from_xyz(x, AURA_Y, 0.0).with_scale(Vec3::new(
                        side * AURA_DORMANT,
                        1.0,
                        1.0,
                    )),
                    // Never hidden: level 0 draws nothing, and a
                    // slab that is drawn from the first frame keeps
                    // whatever it needs of the renderer warm before
                    // the first Hype frame.
                    Visibility::Inherited,
                    layer.clone(),
                ));
            }
        }
    }
    commands.insert_resource(AuraMaterials(ladder));
}

/// Crackle the arcs while Hype runs, eased in and out like the edge
/// fire. Transforms, visibility and the aura's handle swaps only.
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn crackle_arcs(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut segments: Query<
        (&BoltSegment, &mut Transform, &mut Visibility),
        (Without<BoltFork>, Without<AuraSlab>),
    >,
    mut forks: Query<
        (&BoltFork, &mut Transform, &mut Visibility),
        (Without<BoltSegment>, Without<AuraSlab>),
    >,
    mut slabs: Query<
        (
            &mut AuraSlab,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (Without<BoltSegment>, Without<BoltFork>),
    >,
    ladder: Option<Res<AuraMaterials>>,
    mut blend: Local<Vec<f32>>,
) {
    let delta = time.delta_secs();
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
    let calm = settings.reduced_flashing;
    let now = time.elapsed_secs();
    let step = step(now, if calm { CALM_HZ } else { CRACKLE_HZ });
    let live_share = settings.fx_intensity.clamp(0.0, 1.0);

    for (seg, mut transform, mut visibility) in &mut segments {
        let grown = blend.get(seg.player).copied().unwrap_or(0.0);
        let seed = rail_seed(seg.player, seg.side, seg.bolt);
        let live = grown >= 0.02
            && !gapped(seed, seg.segment, step, calm)
            && (seg.segment as f32) < (SEGMENTS as f32) * live_share.max(0.3);
        let wanted = if live {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !live {
            continue;
        }
        let x = rail_x(&layout, seg.player, seg.side);
        let (ax, ay) = point(seed, seg.segment, step);
        let (bx, by) = point(seed, seg.segment + 1, step);
        let (za, zb) = (point_z(seg.segment), point_z(seg.segment + 1));
        let lift_a = 1.0 + SURGE_HEIGHT * surge(za, now, calm);
        let lift_b = 1.0 + SURGE_HEIGHT * surge(zb, now, calm);
        let a = Vec3::new(x + ax * grown, 0.015 + ay * lift_a * grown, za);
        let b = Vec3::new(x + bx * grown, 0.015 + by * lift_b * grown, zb);
        let (mid, rot, len) = segment_pose(a, b);
        let thick = flash(seed, step, calm)
            * (1.0 + SURGE_THICK * surge((za + zb) * 0.5, now, calm))
            * grown;
        transform.translation = mid;
        transform.rotation = rot;
        transform.scale = Vec3::new(len, thick, thick);
    }

    for (fork, mut transform, mut visibility) in &mut forks {
        let grown = blend.get(fork.player).copied().unwrap_or(0.0);
        let seed = rail_seed(fork.player, fork.side, 0);
        let live = grown >= 0.02 && (fork.index as f32) < (FORKS_PER_RAIL as f32) * live_share;
        let wanted = if live {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !live {
            continue;
        }
        let (anchor, dir, len) = fork_shape(seed, fork.index, step, fork.side);
        let x = rail_x(&layout, fork.player, fork.side);
        let (ax, ay) = point(seed, anchor, step);
        let z = point_z(anchor);
        let lift = 1.0 + SURGE_HEIGHT * surge(z, now, calm);
        let a = Vec3::new(x + ax * grown, 0.015 + ay * lift * grown, z);
        let b = a + dir * (len * (1.0 + SURGE_FORK * surge(z, now, calm)) * grown);
        let (mid, rot, len) = segment_pose(a, b);
        let thick = flash(seed + 1, step, calm) * grown;
        transform.translation = mid;
        transform.rotation = rot;
        transform.scale = Vec3::new(len, thick, thick);
    }

    let Some(ladder) = ladder else {
        return;
    };
    for (mut slab, mut transform, mut material) in &mut slabs {
        let grown = blend.get(slab.player).copied().unwrap_or(0.0);
        let seed = rail_seed(slab.player, slab.side, 0);
        let (z0, z1) = slab_span(slab.slab);
        let lift = 1.0 - SURGE_AURA + SURGE_AURA * surge((z0 + z1) * 0.5, now, calm);
        let level = if grown >= 0.02 {
            aura_level(
                aura_envelope(seed, step, now, calm),
                grown,
                lift,
                settings.fx_intensity,
            )
        } else {
            0
        };
        if level != slab.level {
            slab.level = level;
            if let Some(handle) = ladder.0.get(level) {
                material.0 = handle.clone();
            }
        }
        // The aura grows out from the rail as the arc comes in.
        transform.scale.x = slab.side * grown.max(AURA_DORMANT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bolt_jumps_between_steps_and_holds_within_one() {
        // Same step, same shape — the bolt does not shimmer between
        // frames inside a step. Next step, a different shape.
        assert_eq!(point(1, 5, 10), point(1, 5, 10));
        assert_ne!(point(1, 5, 10), point(1, 5, 11));
        // Bounded: never off the rail's neighbourhood, never below it.
        for i in 0..SEGMENTS {
            for s in 0..50 {
                let (dx, dy) = point(3, i, s);
                assert!(dx.abs() <= JITTER_X + 1e-6);
                assert!(dy >= HEIGHT.0 && dy <= HEIGHT.1);
            }
        }
    }

    #[test]
    fn steps_tick_at_the_crackle_rate_and_slower_when_calm() {
        assert_eq!(step(0.0, CRACKLE_HZ), 0);
        assert_eq!(step(1.0, CRACKLE_HZ), 24);
        assert_eq!(step(1.0, CALM_HZ), 2);
        assert_eq!(
            step(-5.0, CRACKLE_HZ),
            0,
            "before the clock starts is step 0"
        );
    }

    #[test]
    fn gaps_and_flashes_exist_when_crackling_and_never_when_calm() {
        let gaps = (0..SEGMENTS)
            .flat_map(|i| (0..100).map(move |s| (i, s)))
            .filter(|&(i, s)| gapped(7, i, s, false))
            .count();
        let total = SEGMENTS * 100;
        let share = gaps as f32 / total as f32;
        assert!(share > 0.05 && share < 0.2, "gap share {share}");
        assert!(
            (0..SEGMENTS).all(|i| !gapped(7, i, 3, true)),
            "calm: no gaps"
        );

        let flashes = (0..500).filter(|&s| flash(7, s, false) > 2.0).count();
        assert!(flashes > 15 && flashes < 90, "flashes {flashes} of 500");
        assert!(
            (0..500).all(|s| (flash(7, s, true) - 1.0).abs() < 1e-6),
            "calm: steady"
        );
        assert!((0..500).all(|s| flash(7, s, false) >= 0.7));
    }

    #[test]
    fn a_segment_spans_exactly_its_two_points() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(1.0, 0.0, 2.0);
        let (mid, rot, len) = segment_pose(a, b);
        assert!((len - 2.0).abs() < 1e-6);
        assert!((mid - Vec3::new(1.0, 0.0, 1.0)).length() < 1e-6);
        // The mesh's +X axis lands on the segment's direction.
        assert!((rot * Vec3::X - Vec3::Z).length() < 1e-5);
        assert_eq!(segment_pose(a, a).2, 0.0);
    }

    #[test]
    fn a_fork_leaves_the_rail_outward_and_up() {
        for side in [-1.0f32, 1.0] {
            for i in 0..FORKS_PER_RAIL {
                let (anchor, dir, len) = fork_shape(5, i, 4, side);
                assert!(anchor < SEGMENTS);
                assert!(dir.x * side > 0.0, "off the neck, not into it");
                assert!(dir.y > 0.0, "up");
                assert!(len > 0.2 && len < 0.7);
                assert!((dir.length() - 1.0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn the_chain_covers_the_rail_end_to_end() {
        assert!((point_z(0) - SPAN.0).abs() < 1e-6);
        assert!((point_z(SEGMENTS) - SPAN.1).abs() < 1e-6);
        assert!(point_z(10) < point_z(11));
    }

    /// The bolt is electric blue with a hot core, and the aura is
    /// cyan-blue — not the white-cyan the arc wore before.
    #[test]
    fn the_arc_is_electric_blue_with_a_hot_core() {
        let hull = HULL.to_srgba();
        assert!(
            hull.blue > 0.9 && hull.red < 0.3 && hull.green < 0.6,
            "the hull is blue, not white: {hull:?}"
        );
        let core = CORE.to_srgba();
        assert!(
            core.blue >= core.green && core.green >= core.red,
            "the core leans blue, not warm: {core:?}"
        );
        let luma = |c: bevy::color::Srgba| 0.299 * c.red + 0.587 * c.green + 0.114 * c.blue;
        assert!(
            luma(core) > luma(hull) + 0.25,
            "the core is hot, the hull is the colour"
        );
        let aura = AURA.to_srgba();
        assert!(
            aura.blue > 0.9 && aura.green > 0.55 && aura.red < 0.3,
            "the aura is cyan-blue: {aura:?}"
        );
    }

    /// The surge runs toward the horizon: what is at z now is at
    /// z − speed·dt a dt later. Bounded 0..1, and it does rise.
    #[test]
    fn the_surge_runs_toward_the_horizon() {
        for calm in [false, true] {
            let speed = if calm {
                SURGE_SPEED * SURGE_CALM_SPEED
            } else {
                SURGE_SPEED
            };
            for i in 0..40 {
                let z = -25.0 + i as f32 * 0.7;
                let now = 3.0 + i as f32 * 0.13;
                let dt = 0.25;
                let there = surge(z, now, calm);
                let later = surge(z - speed * dt, now + dt, calm);
                assert!(
                    (there - later).abs() < 1e-3,
                    "calm {calm}: {there} at z={z} moved to {later}"
                );
                assert!((0.0..=1.0).contains(&there));
            }
        }
        let peak = |calm: bool| {
            (0..200)
                .map(|i| surge(-25.0 + i as f32 * 0.14, 1.0, calm))
                .fold(0.0f32, f32::max)
        };
        assert!(peak(false) > 0.95, "the crest reaches full lift");
        assert!(
            peak(true) < 0.4 && peak(true) > 0.2,
            "calm: a third of the lift, still moving"
        );
        // The wave HAS a trough: not every z is lifted.
        let trough = (0..200)
            .map(|i| surge(-25.0 + i as f32 * 0.14, 1.0, false))
            .fold(1.0f32, f32::min);
        assert!(trough < 0.05);
    }

    /// The aura flickers with the bolt — brighter on the bolt's flash
    /// steps — throbs on its own, and holds still under reduced
    /// flashing.
    #[test]
    fn the_aura_pulses_with_the_bolt_and_holds_when_calm() {
        let seed = 11;
        let flash_steps: Vec<u32> = (0..500).filter(|&s| flash(seed, s, false) > 2.0).collect();
        let quiet_steps: Vec<u32> = (0..500).filter(|&s| flash(seed, s, false) < 1.0).collect();
        assert!(!flash_steps.is_empty() && !quiet_steps.is_empty());
        let now = 0.4;
        let flared = flash_steps
            .iter()
            .map(|&s| aura_envelope(seed, s, now, false))
            .fold(1.0f32, f32::min);
        let dim = quiet_steps
            .iter()
            .map(|&s| aura_envelope(seed, s, now, false))
            .fold(0.0f32, f32::max);
        assert!(
            flared > dim,
            "a flash step flares the aura: {flared} vs {dim}"
        );
        // It moves between steps (the flicker) and over time at one
        // step (the throb).
        let steps: Vec<f32> = (0..24)
            .map(|s| aura_envelope(seed, s, now, false))
            .collect();
        assert!(
            steps.iter().any(|v| (v - steps[0]).abs() > 0.05),
            "flickers"
        );
        let a = aura_envelope(seed, 3, 0.0, false);
        let b = aura_envelope(seed, 3, 0.25 / AURA_THROB_HZ, false);
        assert!((a - b).abs() > 0.1, "throbs: {a} vs {b}");
        assert!(steps.iter().all(|v| (0.0..=1.0).contains(v)));
        // Calm: one value, whatever the step or the time.
        let calm: Vec<f32> = (0..50)
            .map(|s| aura_envelope(seed, s, s as f32 * 0.37, true))
            .collect();
        assert!(calm.iter().all(|v| (v - calm[0]).abs() < 1e-6));
        assert!(calm[0] > 0.5, "calm is a steady glow, not off");
    }

    /// The level ladder: off at nothing, the top at everything, in
    /// between monotone — and the effects intensity scales it.
    #[test]
    fn the_aura_level_follows_brightness_and_intensity() {
        assert_eq!(aura_level(1.0, 1.0, 1.0, 1.0), AURA_LEVELS - 1);
        assert_eq!(aura_level(0.0, 1.0, 1.0, 1.0), 0);
        assert_eq!(aura_level(1.0, 0.0, 1.0, 1.0), 0, "not grown in: dark");
        assert_eq!(aura_level(1.0, 1.0, 1.0, 0.0), 0, "intensity off: dark");
        let half = aura_level(1.0, 1.0, 1.0, 0.5);
        assert!(half > 2 && half < AURA_LEVELS - 2, "half intensity: {half}");
        let mut last = 0;
        for i in 0..=20 {
            let level = aura_level(i as f32 / 20.0, 1.0, 1.0, 1.0);
            assert!(level >= last);
            last = level;
        }
        assert_eq!(aura_level(5.0, 5.0, 5.0, 5.0), AURA_LEVELS - 1, "clamped");
    }

    /// The slab mesh fades from the rail outward, reaches only a
    /// short faint lip over the neck, and the slabs tile the span.
    #[test]
    fn the_aura_fades_outward_and_barely_crosses_the_rail() {
        let mesh = slab_mesh(0);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("positions");
        let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(bevy::mesh::VertexAttributeValues::Float32x2(c)) => c.clone(),
            other => panic!("uvs as float2, got {other:?}"),
        };
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_none(),
            "no vertex colours: the slab shares the rig's vertex layout"
        );
        let inner = positions.iter().map(|p| p[0]).fold(0.0f32, f32::min);
        let outer = positions.iter().map(|p| p[0]).fold(0.0f32, f32::max);
        assert!(
            (inner + AURA_IN).abs() < 1e-6 && AURA_IN < 0.2,
            "a short lip"
        );
        assert!(
            (outer - AURA_OUT).abs() < 1e-6 && AURA_OUT > 0.6,
            "a broad glow"
        );
        // It hugs the platform: at the rail it lies on the neck, out
        // on the deck it lies on the deck, never in the air between.
        let y_at = |x: f32| {
            positions
                .iter()
                .find(|p| (p[0] - x).abs() < 1e-6)
                .map(|p| p[1])
                .expect("a column there")
        };
        assert!(y_at(0.0).abs() < 1e-6, "on the neck at the rail");
        assert!(
            (y_at(AURA_OUT) + AURA_DECK_DROP).abs() < 1e-6,
            "on the deck at its outer edge"
        );
        assert!(
            positions.windows(2).all(|w| w[1][1] <= w[0][1] + 1e-6),
            "never climbs back up"
        );
        // u runs across the width, 0 at the lip's edge and 1 at the
        // outer edge, so the gradient texture lands on the strip.
        let u_at = |x: f32| {
            positions
                .iter()
                .zip(&uvs)
                .find(|(p, _)| (p[0] - x).abs() < 1e-6)
                .map(|(_, uv)| uv[0])
                .expect("a column there")
        };
        assert!(u_at(-AURA_IN).abs() < 1e-6 && (u_at(AURA_OUT) - 1.0).abs() < 1e-6);
        assert!(u_at(0.0) > 0.0 && u_at(0.0) < u_at(AURA_DROP));
        // The brightness: full at the rail, nothing at the outer
        // edge, faint on the lip, falling monotonically outward.
        assert!((aura_alpha(0.0) - 1.0).abs() < 1e-6);
        assert!(aura_alpha(AURA_OUT).abs() < 1e-6 && aura_alpha(AURA_OUT + 1.0).abs() < 1e-6);
        assert!(aura_alpha(-AURA_IN) < 0.5 && aura_alpha(-AURA_IN) > 0.0);
        assert!(
            aura_alpha(-AURA_IN / 2.0) > aura_alpha(-AURA_IN),
            "rises to the rail"
        );
        let alphas: Vec<f32> = (0..=40)
            .map(|i| aura_alpha(AURA_OUT * i as f32 / 40.0))
            .collect();
        assert!(
            alphas.windows(2).all(|w| w[1] <= w[0]),
            "falls outward: {alphas:?}"
        );
        assert!(
            alphas[10] > 0.3 && alphas[10] < 0.9,
            "a broad shoulder, not a line"
        );
        // The baked texture carries the same profile.
        let image = aura_gradient();
        let texel = |u: f32| image.data.as_ref().expect("data")[(u * 255.0) as usize * 4 + 3];
        assert!(
            texel(AURA_IN / (AURA_IN + AURA_OUT)) > 240,
            "full at the rail"
        );
        assert_eq!(texel(0.999), 0, "nothing at the outer edge");
        assert!(texel(0.0) > 40 && texel(0.0) < 128, "a faint lip");
        // Slabs tile the rail's span with no gap and no overlap.
        assert!((slab_span(0).0 - SPAN.0).abs() < 1e-6);
        assert!((slab_span(AURA_SLABS - 1).1 - SPAN.1).abs() < 1e-6);
        for slab in 1..AURA_SLABS {
            assert!((slab_span(slab).0 - slab_span(slab - 1).1).abs() < 1e-6);
        }
    }
}
