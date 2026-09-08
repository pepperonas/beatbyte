//! The light rig: six moving heads on the front truss and four rim
//! fixtures on the backline.
//!
//! Each fixture is a PIVOT hanging from its truss — a housing with a
//! bright lens, a pair of additive cone mantles wearing the beam
//! gradient (the visible shaft in the haze), and, since the realism
//! pass of 2026-09-07, a real `SpotLight` on the same pivot: the
//! shaft is what the eye sees in the air, the light is what it does
//! to the deck, the crowd and the band. Both hang from one pivot and
//! wear one cone, so they can never drift apart — the old fake floor
//! pools that slid under the shafts by a matching formula are gone,
//! and with them a whole class of "the pool and the shaft disagree"
//! bugs.
//!
//! Everything additive here is marked `NotShadowCaster`: in this Bevy
//! a blended mesh casts a solid shadow, and a shaft that shadowed the
//! floor would be a black wedge.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use core::f32::consts::FRAC_PI_2;

use super::GameplayScreen;
use super::stage3d::{STAGE_LAYER, Stage3d, complementary};
use crate::theme::Theme;

/// The front truss's height.
pub const RIG_Y: f32 = 8.9;
/// The front truss's depth.
pub const RIG_Z: f32 = -13.0;
/// The backline truss's height.
pub const BACKLINE_Y: f32 = 12.5;
/// The backline fixtures' depth (just in front of the rear wall).
pub const BACKLINE_Z: f32 = -38.2;
/// The wide mantle's base radius, in fixture units.
pub const MANTLE_RADIUS: f32 = 1.05;
/// The wide mantle's length.
pub const MANTLE_LENGTH: f32 = 7.9;
/// A moving head's inner (full-intensity) cone half-angle.
/// Fixtures on the rear (backline) truss.
pub const RIMS: usize = 4;
/// Moving heads on the front truss.
pub const HEADS: usize = 6;
/// Every real lamp hanging from the two trusses — the ceiling the
/// strobe plays on. Each carries [`RigLamp`] with its own index, so
/// a chase can order them without depending on query order.
pub const RIG_LAMPS: usize = RIMS + HEADS;

/// A moving head's inner (full-intensity) cone half-angle.
pub const HEAD_INNER_ANGLE: f32 = 0.07;
/// How much wider the light is than its mantle, so the pool has a
/// soft edge.
pub const HEAD_SOFT_EDGE: f32 = 0.03;

/// A fixture that swings from its hanger.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpotBeam {
    /// The resting angle.
    pub base: f32,
    /// Phase offset, so the six do not swing as one.
    pub phase: f32,
    /// Swing speed, radians per second of the sine's argument.
    pub speed: f32,
}

/// Marks a fixture's visible mantle.
#[derive(Component, Debug, Clone, Copy)]
pub struct Mantle;

/// The one angle a fixture swings to at `now`: a ±0.30 sine around
/// its base. Both the shaft and the light hang from the pivot this
/// rotates, so nothing can take another angle. Pure — tested.
#[must_use]
pub fn beam_angle(now: f32, base: f32, phase: f32, speed: f32) -> f32 {
    let swing = (now * speed + phase).sin();
    swing.mul_add(0.30, base)
}

/// Sweep the fixtures. Gated on STAGE MOTION at spawn: a pivot only
/// carries [`SpotBeam`] when motion is on, so with it off the rig —
/// shafts and lights alike — stands still.
pub fn sweep_beams(time: Res<Time>, mut beams: Query<(&SpotBeam, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (beam, mut transform) in &mut beams {
        transform.rotation =
            Quat::from_rotation_z(beam_angle(now, beam.base, beam.phase, beam.speed));
    }
}

/// The half-angle of a cone `length` long with a base of `radius`.
#[must_use]
pub fn cone_of_mantle(radius: f32, length: f32) -> f32 {
    (radius / length).atan()
}

/// One moving head's place and character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovingHead {
    /// Hanger x on the truss.
    pub x: f32,
    /// Which of the two tones (0 = accent, 1 = paler).
    pub tone: usize,
    /// Resting swing angle.
    pub base: f32,
    /// Swing phase.
    pub phase: f32,
    /// Swing speed.
    pub speed: f32,
}

impl MovingHead {
    /// The light's outer cone half-angle: the mantle's, plus a soft
    /// edge. Pure — tested against the mantle.
    #[must_use]
    pub fn outer_angle() -> f32 {
        cone_of_mantle(MANTLE_RADIUS, MANTLE_LENGTH) + HEAD_SOFT_EDGE
    }
}

/// The six moving heads: three a side, none closer to the centre
/// than the speaker stacks, so the neck keeps a clear corridor.
#[must_use]
pub fn fixture(index: usize) -> MovingHead {
    let side = if index.is_multiple_of(2) { -1.0 } else { 1.0 };
    let x = side * (3.4 + 1.7 * (index / 2) as f32);
    MovingHead {
        x,
        tone: index % 2,
        base: x * 0.05,
        phase: index as f32 * 1.1,
        speed: 0.35 + 0.05 * index as f32,
    }
}

/// The four backline rim fixtures' hanger x.
#[must_use]
pub fn rim_x(index: usize) -> f32 {
    ((index as f32 + 0.5) / 4.0 - 0.5) * 22.0
}

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
#[must_use]
pub fn beam_cone_mesh(segments: usize) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let ring = beam_ring(segments);
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

fn additive(
    materials: &mut Assets<StandardMaterial>,
    tone: Color,
    alpha: f32,
    texture: Handle<Image>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: tone.with_alpha(alpha),
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

fn lamp(
    materials: &mut Assets<StandardMaterial>,
    tone: Color,
    glow: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: tone,
        emissive: tone.to_linear() * glow,
        unlit: true,
        ..default()
    })
}

/// A part of a fixture that wears the beam's colour — a mantle or a
/// lens. A strobe hit swaps it to white and back.
///
/// It has to, because the coloured additive beam is what the eye
/// actually sees at a fixture: measured over six frames of a running
/// strobe, the saturation of the near head cones did not move at all
/// (0.71–0.73) while their lights flashed white underneath, and the
/// far rims — whose mantle is fainter and small on screen — swung
/// 0.27–0.48. Flashing the light alone reads on a small distant
/// fixture and not on a big near one, which is exactly what was
/// reported.
#[derive(Component)]
pub struct RigBeam {
    /// The lamp this part belongs to.
    pub lamp: usize,
    /// The material it wears at rest.
    pub base: Handle<StandardMaterial>,
    /// The material it wears under a flash.
    pub flash: Handle<StandardMaterial>,
    /// Whether it is currently flashing, so the swap happens once
    /// per change instead of once per frame.
    pub lit: bool,
}

/// One lamp of the ceiling rig, numbered 0..[`RIG_LAMPS`]: the rims
/// first, then the heads. The number is the lamp's identity for
/// anything that wants to address them in an order of its own.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RigLamp(pub usize);

/// A real light hanging down the pivot's −Y, the way the mantle
/// does (a spot's forward is −Z, so it is turned to point down).
fn hung_light(
    index: usize,
    color: Color,
    intensity: f32,
    range: f32,
    inner: f32,
    outer: f32,
) -> impl Bundle {
    (
        RigLamp(index),
        SpotLight {
            color,
            intensity,
            range,
            inner_angle: inner,
            outer_angle: outer,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, -0.55, 0.0).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
        Visibility::default(),
        RenderLayers::layer(STAGE_LAYER),
    )
}

/// Spawn the rig. `beam_gradient` and `soft_dot` are the shared
/// shaft and halo textures; `dark` the venue's base tone; `motion`
/// whether the heads may swing (STAGE MOTION).
#[allow(clippy::too_many_arguments)] // one call site, every piece named
pub fn spawn_rig(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    beam_gradient: Handle<Image>,
    soft_dot: Handle<Image>,
    theme: Theme,
    dark: Color,
    motion: bool,
) {
    let layer = RenderLayers::layer(STAGE_LAYER);
    // What a fixture wears while the strobe hits it: a white beam
    // and a white bulb, shared by every fixture. Swapping the handle
    // costs nothing per frame and never touches a live material.
    let flash_beam = additive(materials, Color::WHITE, 0.55, beam_gradient.clone());
    let flash_lens = lamp(materials, Color::WHITE, 9.0);
    let mantle = meshes.add(beam_cone_mesh(28));
    let housing = meshes.add(Cuboid::new(0.34, 0.5, 0.34));
    let lens = meshes.add(Sphere::new(0.13).mesh().uv(10, 8));
    let halo_quad = meshes.add(Rectangle::new(1.5, 1.5));
    let housing_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.5),
        perceptual_roughness: 0.4,
        metallic: 0.6,
        ..default()
    });
    // Every second fixture runs a paler, whiter tone — a rig of six
    // identical colours reads as a texture, two tones read as lamps.
    let tones = [theme.accent, theme.accent.mix(&Color::WHITE, 0.45)];
    let beam_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|&tone| additive(materials, tone, 0.16, beam_gradient.clone()))
        .collect();
    let lens_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|&tone| lamp(materials, tone, 6.0))
        .collect();
    let halo_materials: Vec<Handle<StandardMaterial>> = tones
        .iter()
        .map(|&tone| additive(materials, tone, 0.55, soft_dot.clone()))
        .collect();

    // The backline (P4): four fixtures high on the rear truss firing
    // short, wide cones toward the camera in the accent's
    // complementary tone — the warm/cold opposition a one-colour rig
    // never has — and now real rim light on the band's and the
    // crowd's edges.
    let rim_tone = complementary(theme.accent);
    let rim_material = additive(materials, rim_tone, 0.10, beam_gradient.clone());
    let rim_lens_material = lamp(materials, rim_tone, 5.0);
    for i in 0..RIMS {
        let pivot = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                // Tipped toward the audience: the shaft leans out of
                // the wall plane instead of hanging straight down.
                Transform::from_xyz(rim_x(i), BACKLINE_Y, BACKLINE_Z)
                    .with_rotation(Quat::from_rotation_x(-0.55)),
                Visibility::default(),
                layer.clone(),
            ))
            .id();
        commands.spawn((
            Mesh3d(housing.clone()),
            MeshMaterial3d(housing_material.clone()),
            Transform::from_xyz(0.0, -0.2, 0.0),
            layer.clone(),
            ChildOf(pivot),
        ));
        commands.spawn((
            NotShadowCaster,
            RigBeam {
                lamp: i,
                base: rim_lens_material.clone(),
                flash: flash_lens.clone(),
                lit: false,
            },
            Mesh3d(lens.clone()),
            MeshMaterial3d(rim_lens_material.clone()),
            Transform::from_xyz(0.0, -0.45, 0.0),
            layer.clone(),
            ChildOf(pivot),
        ));
        commands.spawn((
            Mantle,
            NotShadowCaster,
            RigBeam {
                lamp: i,
                base: rim_material.clone(),
                flash: flash_beam.clone(),
                lit: false,
            },
            Mesh3d(mantle.clone()),
            MeshMaterial3d(rim_material.clone()),
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(1.6, 9.0, 1.6)),
            layer.clone(),
            ChildOf(pivot),
        ));
        commands.spawn((
            hung_light(i, rim_tone, 3_000_000.0, 30.0, 0.30, 0.45),
            ChildOf(pivot),
        ));
    }

    for index in 0..HEADS {
        let head = fixture(index);
        let pivot = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Transform::from_xyz(head.x, RIG_Y, RIG_Z)
                    .with_rotation(Quat::from_rotation_z(head.base)),
                Visibility::default(),
                layer.clone(),
            ))
            .id();
        if motion {
            commands.entity(pivot).insert(SpotBeam {
                base: head.base,
                phase: head.phase,
                speed: head.speed,
            });
        }
        commands.spawn((
            Mesh3d(housing.clone()),
            MeshMaterial3d(housing_material.clone()),
            Transform::from_xyz(0.0, -0.25, 0.0),
            layer.clone(),
            ChildOf(pivot),
        ));
        // A soft halo around the lens: a lamp blooms in air.
        commands.spawn((
            NotShadowCaster,
            Mesh3d(halo_quad.clone()),
            MeshMaterial3d(halo_materials[head.tone].clone()),
            Transform::from_xyz(0.0, -0.52, 0.05),
            layer.clone(),
            ChildOf(pivot),
        ));
        commands.spawn((
            NotShadowCaster,
            RigBeam {
                lamp: RIMS + index,
                base: lens_materials[head.tone].clone(),
                flash: flash_lens.clone(),
                lit: false,
            },
            Mesh3d(lens.clone()),
            MeshMaterial3d(lens_materials[head.tone].clone()),
            Transform::from_xyz(0.0, -0.52, 0.0),
            layer.clone(),
            ChildOf(pivot),
        ));
        // The hot core and the soft sheath: same mantle, same
        // gradient, different girth — their addition is what fakes
        // the volumetric falloff across the shaft.
        for scale in [
            Vec3::new(0.42, 7.6, 0.42),
            Vec3::new(MANTLE_RADIUS, MANTLE_LENGTH, MANTLE_RADIUS),
        ] {
            commands.spawn((
                Mantle,
                NotShadowCaster,
                RigBeam {
                    lamp: RIMS + index,
                    base: beam_materials[head.tone].clone(),
                    flash: flash_beam.clone(),
                    lit: false,
                },
                Mesh3d(mantle.clone()),
                MeshMaterial3d(beam_materials[head.tone].clone()),
                Transform::from_xyz(0.0, -0.55, 0.0).with_scale(scale),
                layer.clone(),
                ChildOf(pivot),
            ));
        }
        commands.spawn((
            hung_light(
                RIMS + index,
                tones[head.tone],
                900_000.0,
                24.0,
                HEAD_INNER_ANGLE,
                MovingHead::outer_angle(),
            ),
            ChildOf(pivot),
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_swings_within_its_bounds() {
        // The shaft and the light both take the pivot's angle from
        // ONE function; this pins that it actually swings (a constant
        // would keep them "in sync" while freezing the rig) and that
        // the swing stays inside ±0.30 of its base: a shaft past that
        // would rake across the fretboard.
        let a = beam_angle(0.0, 0.1, 0.0, 0.5);
        let b = beam_angle(3.0, 0.1, 0.0, 0.5);
        assert!((a - b).abs() > 0.05, "the rig must swing: {a} vs {b}");
        for step in 0..60 {
            let angle = beam_angle(step as f32 * 0.37, 0.1, 1.1, 0.45);
            assert!((angle - 0.1).abs() <= 0.30 + 1e-6);
        }
    }

    #[test]
    fn the_lamp_and_its_mantle_wear_one_cone() {
        let mantle = cone_of_mantle(MANTLE_RADIUS, MANTLE_LENGTH);
        let outer = MovingHead::outer_angle();
        assert!(
            (outer - mantle).abs() < 0.05,
            "outer {outer} vs mantle {mantle}"
        );
        assert!(HEAD_INNER_ANGLE < outer && outer < FRAC_PI_2);
        assert!(
            (mantle - 0.132).abs() < 0.01,
            "the mantle's own cone: {mantle}"
        );
    }

    #[test]
    fn the_heads_keep_the_necks_corridor_and_alternate_tones() {
        for index in 0..6 {
            let head = fixture(index);
            assert!(head.x.abs() >= 3.4, "{head:?}");
            assert_eq!(head.tone, index % 2);
            assert!(head.speed > 0.0);
        }
        assert!(rim_x(0) < rim_x(3));
    }

    #[test]
    fn every_lamp_hangs_from_the_pivot_that_swings_its_mantle() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.world_mut()
            .run_system_once(
                |mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>| {
                    spawn_rig(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        Handle::default(),
                        Handle::default(),
                        crate::theme::ActiveTheme::default().0,
                        Color::BLACK,
                        true,
                    );
                },
            )
            .unwrap();
        let world = app.world_mut();
        let mut heads = 0;
        let mut rims = 0;
        let mut lamps = world.query::<(Entity, &SpotLight, &ChildOf)>();
        let lamp_parents: Vec<(Entity, f32)> = lamps
            .iter(world)
            .map(|(_, light, parent)| (parent.parent(), light.outer_angle))
            .collect();
        for (pivot, outer) in lamp_parents {
            let swings = world.get::<SpotBeam>(pivot).is_some();
            let children = world.get::<Children>(pivot).expect("a pivot has children");
            let mantles = children
                .iter()
                .filter(|&child| world.get::<Mantle>(child).is_some())
                .count();
            assert!(mantles >= 1, "every lamp's pivot carries a mantle");
            if swings {
                heads += 1;
                assert!((outer - MovingHead::outer_angle()).abs() < 1e-6);
            } else {
                rims += 1;
            }
        }
        assert_eq!((heads, rims), (6, 4));
        // Every additive piece is a ghost to the shadow pass.
        let mut ghosts = world.query::<(&Mantle, Option<&NotShadowCaster>)>();
        for (_, mark) in ghosts.iter(world) {
            assert!(mark.is_some(), "a mantle casts no shadow");
        }
    }
}
