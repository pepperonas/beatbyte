//! The PA: two full stacks flanking the near end of the neck.
//!
//! The genre's convention — a sub, two tops and an amp head, stacked
//! beside the stage — drawn in our own hands: tolex boxes with a
//! recessed grille behind a raised frame, metal corner caps and
//! handles, rubber feet, a bass port, and a head with knobs and one
//! power LED. No badge, no name, no logo. Every surface comes from
//! [`crate::surfaces`]; every mesh is shared between the two sides.
//!
//! The stacks stand ON the deck (the old cabinets floated 0.27 above
//! it — invisible until the first cast shadow), and their driver cones
//! stroke out on the beat, because a PA that stands dead still gives a
//! stage away. Nothing here touches judgment; nothing writes a
//! material per frame.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::prelude::*;
use core::f32::consts::FRAC_PI_2;

use super::stage3d::{self, STAGE_LAYER, Stage3d, led_pulse};
use super::{GameplayScreen, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::surfaces::{StageSurfaces, tangent_mesh};
use crate::theme::Theme;

/// The top of the deck the stacks stand on (the highway riser).
pub const DECK_TOP: f32 = -0.30;
/// Where each stack stands.
pub const STACK_X: f32 = 4.4;
/// How far down the stage the stacks stand.
pub const STACK_Z: f32 = -7.0;
/// Height of the rubber feet under the sub.
pub const FOOT: f32 = 0.04;
/// Gap left by the stacking cleats between cabinets.
pub const CLEAT: f32 = 0.02;
/// The camera's height: a stack may not loom above it (G23).
pub const CAMERA_Y: f32 = 3.1;

/// What a cabinet is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabinetKind {
    /// One big driver, a slot port.
    Sub,
    /// A woofer and a tweeter.
    Top,
    /// The amp head: panel, knobs, a power LED.
    Head,
}

/// One cabinet's place and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CabinetSpec {
    /// What it is.
    pub kind: CabinetKind,
    /// Its centre.
    pub centre: Vec3,
    /// Width, height, depth.
    pub size: Vec3,
}

impl CabinetSpec {
    /// The cabinet's lowest point.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.centre.y - self.size.y * 0.5
    }

    /// The cabinet's highest point.
    #[must_use]
    pub fn top(&self) -> f32 {
        self.centre.y + self.size.y * 0.5
    }
}

/// The four cabinets of one stack, bottom up, seated on the deck.
/// Pure — tested.
#[must_use]
pub fn stack_layout(side: f32) -> [CabinetSpec; 4] {
    let sizes = [
        (CabinetKind::Sub, Vec3::new(1.36, 0.96, 1.18)),
        (CabinetKind::Top, Vec3::new(1.30, 0.84, 1.02)),
        (CabinetKind::Top, Vec3::new(1.30, 0.84, 1.02)),
        (CabinetKind::Head, Vec3::new(1.24, 0.40, 0.86)),
    ];
    let mut bottom = DECK_TOP + FOOT;
    let mut out = [CabinetSpec {
        kind: CabinetKind::Sub,
        centre: Vec3::ZERO,
        size: Vec3::ONE,
    }; 4];
    for (slot, (kind, size)) in out.iter_mut().zip(sizes) {
        *slot = CabinetSpec {
            kind,
            centre: Vec3::new(side * STACK_X, bottom + size.y * 0.5, STACK_Z),
            size,
        };
        bottom += size.y + CLEAT;
    }
    out
}

/// A driver cone that strokes out on the beat.
#[derive(Component, Debug, Clone, Copy)]
pub struct DriverCone {
    /// Beat phase, so the two stacks and the drivers breathe apart.
    pub phase: f32,
    /// The cone's resting z.
    pub rest_z: f32,
    /// How far it strokes out.
    pub reach: f32,
}

/// The cone's excursion on the beat, 0..1: out on the beat, back
/// between. Pure — tested.
#[must_use]
pub fn cone_stroke(beats: f32, phase: f32) -> f32 {
    ((led_pulse(beats, phase) - 1.0) / 0.16).clamp(0.0, 1.0)
}

/// Stroke the driver cones with the song's beat. Transform only,
/// gated on STAGE MOTION like every ambient movement.
pub fn pump_drivers(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mut cones: Query<(&DriverCone, &mut Transform)>,
) {
    if !stage3d::active(&settings) || !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player)) = (game_clock.song_time(&time), players.iter().next()) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    for (cone, mut transform) in &mut cones {
        let z = cone
            .reach
            .mul_add(cone_stroke(beats, cone.phase), cone.rest_z);
        if (transform.translation.z - z).abs() > 1e-6 {
            transform.translation.z = z;
        }
    }
}

/// The shared meshes of the PA, built once.
struct StackMeshes {
    body: [Handle<Mesh>; 3],
    baffle: [Handle<Mesh>; 2],
    cloth: [Handle<Mesh>; 2],
    frame: [Handle<Mesh>; 2],
    caps: [Handle<Mesh>; 3],
    rubber: [Handle<Mesh>; 2],
    ports: [Handle<Mesh>; 2],
    sub_driver: Handle<Mesh>,
    woofer: Handle<Mesh>,
    tweeter: Handle<Mesh>,
    panel: Handle<Mesh>,
    knobs: Handle<Mesh>,
    led: Handle<Mesh>,
    vent: Handle<Mesh>,
    handle: Handle<Mesh>,
}

/// A box translated into place.
fn block(size: Vec3, at: Vec3) -> Mesh {
    Mesh::from(Cuboid::from_size(size)).translated_by(at)
}

/// Merge pieces into one mesh (one material, one draw).
fn merged(mut base: Mesh, pieces: Vec<Mesh>) -> Mesh {
    for piece in &pieces {
        if let Err(error) = base.merge(piece) {
            warn!("pa: a piece could not be merged ({error:?})");
        }
    }
    base
}

/// The eight corner caps of a box of `size`.
fn corner_caps(size: Vec3) -> Mesh {
    let cap = 0.10;
    let mut pieces = Vec::new();
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                pieces.push(block(
                    Vec3::splat(cap),
                    Vec3::new(
                        sx * (size.x * 0.5 - cap * 0.3),
                        sy * (size.y * 0.5 - cap * 0.3),
                        sz * (size.z * 0.5 - cap * 0.3),
                    ),
                ));
            }
        }
    }
    let first = pieces.remove(0);
    merged(first, pieces)
}

/// The raised frame around a cabinet's front: two rails and two
/// stiles, proud of the baffle so the cloth sits recessed behind it.
fn front_frame(size: Vec3) -> Mesh {
    let z = size.z * 0.5 + 0.025;
    let w = size.x - 0.04;
    let h = size.y - 0.04;
    merged(
        block(Vec3::new(w, 0.07, 0.07), Vec3::new(0.0, h * 0.5 - 0.035, z)),
        vec![
            block(
                Vec3::new(w, 0.07, 0.07),
                Vec3::new(0.0, -(h * 0.5 - 0.035), z),
            ),
            block(
                Vec3::new(0.07, h - 0.14, 0.07),
                Vec3::new(w * 0.5 - 0.035, 0.0, z),
            ),
            block(
                Vec3::new(0.07, h - 0.14, 0.07),
                Vec3::new(-(w * 0.5 - 0.035), 0.0, z),
            ),
        ],
    )
}

/// Corner caps plus the two side handle bars.
fn metal_hardware(size: Vec3) -> Mesh {
    merged(
        corner_caps(size),
        vec![
            block(
                Vec3::new(0.03, 0.03, 0.22),
                Vec3::new(size.x * 0.5 + 0.015, 0.05, 0.0),
            ),
            block(
                Vec3::new(0.03, 0.03, 0.22),
                Vec3::new(-(size.x * 0.5 + 0.015), 0.05, 0.0),
            ),
        ],
    )
}

/// The handle wells and the four feet.
fn rubber_hardware(size: Vec3) -> Mesh {
    let mut feet = Vec::new();
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            feet.push(block(
                Vec3::new(0.12, FOOT, 0.12),
                Vec3::new(
                    sx * (size.x * 0.5 - 0.12),
                    -(size.y * 0.5 + FOOT * 0.5),
                    sz * (size.z * 0.5 - 0.12),
                ),
            ));
        }
    }
    feet.push(block(
        Vec3::new(0.02, 0.13, 0.28),
        Vec3::new(-(size.x * 0.5 + 0.004), 0.05, 0.0),
    ));
    merged(
        block(
            Vec3::new(0.02, 0.13, 0.28),
            Vec3::new(size.x * 0.5 + 0.004, 0.05, 0.0),
        ),
        feet,
    )
}

/// The bass ports of a cabinet kind, on the baffle.
fn ports(kind: CabinetKind, size: Vec3) -> Mesh {
    let z = size.z * 0.5 + 0.003;
    match kind {
        CabinetKind::Sub => block(Vec3::new(0.60, 0.05, 0.02), Vec3::new(0.0, 0.36, z)),
        _ => merged(
            block(Vec3::new(0.10, 0.05, 0.02), Vec3::new(0.45, -0.30, z)),
            vec![block(
                Vec3::new(0.10, 0.05, 0.02),
                Vec3::new(-0.45, -0.30, z),
            )],
        ),
    }
}

/// The head's eight knobs in a row.
fn knobs() -> Mesh {
    let knob = |i: usize| {
        Mesh::from(Cylinder::new(0.035, 0.03))
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2))
            .translated_by(Vec3::new(-0.42 + i as f32 * 0.12, -0.02, 0.0))
    };
    let first = knob(0);
    merged(first, (1..8).map(knob).collect())
}

impl StackMeshes {
    fn build(meshes: &mut Assets<Mesh>, layout: &[CabinetSpec; 4]) -> StackMeshes {
        let sub = layout[0].size;
        let top = layout[1].size;
        let head = layout[3].size;
        let front = |size: Vec3| Rectangle::new(size.x - 0.16, size.y - 0.16);
        let disc = |radius: f32| tangent_mesh(Mesh::from(Circle::new(radius)));
        StackMeshes {
            body: [sub, top, head]
                .map(|size| meshes.add(tangent_mesh(Mesh::from(Cuboid::from_size(size))))),
            baffle: [sub, top].map(|size| meshes.add(Mesh::from(front(size)))),
            cloth: [sub, top].map(|size| meshes.add(tangent_mesh(Mesh::from(front(size))))),
            frame: [sub, top].map(|size| meshes.add(tangent_mesh(front_frame(size)))),
            caps: [
                meshes.add(metal_hardware(sub)),
                meshes.add(metal_hardware(top)),
                meshes.add(merged(
                    corner_caps(head),
                    vec![block(
                        Vec3::new(0.30, 0.03, 0.05),
                        Vec3::new(0.0, head.y * 0.5 + 0.015, 0.0),
                    )],
                )),
            ],
            rubber: [sub, top].map(|size| meshes.add(rubber_hardware(size))),
            ports: [
                meshes.add(ports(CabinetKind::Sub, sub)),
                meshes.add(ports(CabinetKind::Top, top)),
            ],
            sub_driver: meshes.add(disc(0.33)),
            woofer: meshes.add(disc(0.25)),
            tweeter: meshes.add(disc(0.09)),
            panel: meshes.add(Mesh::from(Rectangle::new(head.x - 0.14, head.y - 0.12))),
            knobs: meshes.add(knobs()),
            led: meshes.add(Sphere::new(0.014).mesh().uv(8, 6)),
            vent: meshes.add(Mesh::from(Cuboid::new(1.0, 0.05, 0.01))),
            handle: meshes.add(Mesh::from(Cuboid::new(0.30, 0.03, 0.05))),
        }
    }
}

/// The PA's materials, from the surface tiles and the theme.
struct StackMaterials {
    tolex: Handle<StandardMaterial>,
    baffle: Handle<StandardMaterial>,
    cloth: Handle<StandardMaterial>,
    driver: Handle<StandardMaterial>,
    metal: Handle<StandardMaterial>,
    dark_metal: Handle<StandardMaterial>,
    rubber: Handle<StandardMaterial>,
    hole: Handle<StandardMaterial>,
    knob: Handle<StandardMaterial>,
    led: Handle<StandardMaterial>,
}

impl StackMaterials {
    fn build(
        materials: &mut Assets<StandardMaterial>,
        surfaces: &StageSurfaces,
        theme: Theme,
    ) -> StackMaterials {
        let tile = |x: f32, y: f32| Affine2::from_scale(Vec2::new(x, y));
        StackMaterials {
            tolex: materials.add(StandardMaterial {
                // Near-black: a lit tolex box that reads grey is felt,
                // not vinyl (the first pass was 0.13 and went pale under
                // the blue lamp).
                base_color: Color::srgb(0.06, 0.06, 0.065).mix(&theme.background, 0.12),
                base_color_texture: Some(surfaces.tolex_color.clone()),
                normal_map_texture: Some(surfaces.tolex_normal.clone()),
                metallic_roughness_texture: Some(surfaces.tolex_rough.clone()),
                perceptual_roughness: 1.0,
                metallic: 0.0,
                uv_transform: tile(3.0, 3.0),
                ..default()
            }),
            baffle: materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.05, 0.05),
                perceptual_roughness: 0.85,
                ..default()
            }),
            cloth: materials.add(StandardMaterial {
                base_color: Color::srgba(0.05, 0.05, 0.055, 0.62),
                alpha_mode: AlphaMode::Blend,
                normal_map_texture: Some(surfaces.cloth_normal.clone()),
                perceptual_roughness: 0.65,
                reflectance: 0.3,
                uv_transform: tile(4.0, 3.0),
                ..default()
            }),
            driver: materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(surfaces.driver_color.clone()),
                normal_map_texture: Some(surfaces.driver_normal.clone()),
                perceptual_roughness: 0.6,
                ..default()
            }),
            // Black protectors, not chrome: the stacks stand right
            // under the moving heads, and any specular cap bloomed into
            // a fairy light in the first two frames. Real corner
            // protectors are matte black anyway.
            metal: materials.add(StandardMaterial {
                base_color: Color::srgb(0.11, 0.11, 0.115),
                metallic: 0.15,
                perceptual_roughness: 0.8,
                ..default()
            }),
            dark_metal: materials.add(StandardMaterial {
                base_color: Color::srgb(0.30, 0.30, 0.31),
                metallic: 0.85,
                perceptual_roughness: 1.0,
                metallic_roughness_texture: Some(surfaces.metal_rough.clone()),
                uv_transform: tile(3.0, 1.0),
                ..default()
            }),
            rubber: materials.add(StandardMaterial {
                base_color: Color::srgb(0.02, 0.02, 0.02),
                perceptual_roughness: 0.95,
                ..default()
            }),
            hole: materials.add(StandardMaterial {
                base_color: Color::BLACK,
                unlit: true,
                ..default()
            }),
            knob: materials.add(StandardMaterial {
                base_color: Color::srgb(0.18, 0.18, 0.19),
                metallic: 0.9,
                perceptual_roughness: 0.25,
                ..default()
            }),
            led: materials.add(StandardMaterial {
                base_color: theme.accent,
                emissive: theme.accent.to_linear() * 8.0,
                unlit: true,
                ..default()
            }),
        }
    }
}

/// Spawn both stacks. Called from the venue's setup with the theme
/// it chose; every entity is a venue piece on the stage layer.
pub fn spawn_stacks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    surfaces: &StageSurfaces,
    theme: Theme,
) {
    let layout = stack_layout(1.0);
    let parts = StackMeshes::build(meshes, &layout);
    let looks = StackMaterials::build(materials, surfaces, theme);
    let layer = RenderLayers::layer(STAGE_LAYER);
    for side in [-1.0f32, 1.0] {
        for (level, cabinet) in stack_layout(side).iter().enumerate() {
            let size = cabinet.size;
            let front_z = cabinet.centre.z + size.z * 0.5;
            let mut piece = |mesh: Handle<Mesh>,
                             material: Handle<StandardMaterial>,
                             at: Vec3,
                             extra: Option<DriverCone>| {
                let mut entity = commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_translation(at),
                    layer.clone(),
                ));
                if let Some(cone) = extra {
                    entity.insert(cone);
                }
            };
            let kind = match cabinet.kind {
                CabinetKind::Sub => 0,
                CabinetKind::Top => 1,
                CabinetKind::Head => 2,
            };
            piece(
                parts.body[kind].clone(),
                looks.tolex.clone(),
                cabinet.centre,
                None,
            );
            piece(
                parts.caps[kind].clone(),
                looks.metal.clone(),
                cabinet.centre,
                None,
            );
            match cabinet.kind {
                CabinetKind::Sub | CabinetKind::Top => {
                    let k = usize::from(cabinet.kind == CabinetKind::Top);
                    piece(
                        parts.baffle[k].clone(),
                        looks.baffle.clone(),
                        cabinet.centre + Vec3::new(0.0, 0.0, size.z * 0.5 + 0.002),
                        None,
                    );
                    piece(
                        parts.frame[k].clone(),
                        looks.tolex.clone(),
                        cabinet.centre,
                        None,
                    );
                    piece(
                        parts.rubber[k].clone(),
                        looks.rubber.clone(),
                        cabinet.centre,
                        None,
                    );
                    piece(
                        parts.ports[k].clone(),
                        looks.hole.clone(),
                        cabinet.centre,
                        None,
                    );
                    let phase = side.mul_add(0.6, level as f32 * 0.8);
                    let driver_z = front_z + 0.012;
                    if cabinet.kind == CabinetKind::Sub {
                        piece(
                            parts.sub_driver.clone(),
                            looks.driver.clone(),
                            Vec3::new(cabinet.centre.x, cabinet.centre.y - 0.03, driver_z),
                            Some(DriverCone {
                                phase,
                                rest_z: driver_z,
                                reach: 0.028,
                            }),
                        );
                    } else {
                        piece(
                            parts.woofer.clone(),
                            looks.driver.clone(),
                            Vec3::new(cabinet.centre.x, cabinet.centre.y - 0.10, driver_z),
                            Some(DriverCone {
                                phase,
                                rest_z: driver_z,
                                reach: 0.016,
                            }),
                        );
                        piece(
                            parts.tweeter.clone(),
                            looks.driver.clone(),
                            Vec3::new(cabinet.centre.x, cabinet.centre.y + 0.26, driver_z),
                            None,
                        );
                    }
                    // The cloth last and marked: a blended mesh casts a
                    // solid shadow in this Bevy, and a grille that
                    // shadowed the cones would hide them.
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        NotShadowCaster,
                        Mesh3d(parts.cloth[k].clone()),
                        MeshMaterial3d(looks.cloth.clone()),
                        Transform::from_translation(
                            cabinet.centre + Vec3::new(0.0, 0.0, size.z * 0.5 + 0.045),
                        ),
                        layer.clone(),
                    ));
                }
                CabinetKind::Head => {
                    let z = front_z;
                    piece(
                        parts.panel.clone(),
                        looks.dark_metal.clone(),
                        Vec3::new(cabinet.centre.x, cabinet.centre.y, z + 0.002),
                        None,
                    );
                    piece(
                        parts.knobs.clone(),
                        looks.knob.clone(),
                        Vec3::new(cabinet.centre.x, cabinet.centre.y, z + 0.017),
                        None,
                    );
                    piece(
                        parts.led.clone(),
                        looks.led.clone(),
                        Vec3::new(cabinet.centre.x + 0.50, cabinet.centre.y - 0.02, z + 0.01),
                        None,
                    );
                    piece(
                        parts.vent.clone(),
                        looks.hole.clone(),
                        Vec3::new(cabinet.centre.x, cabinet.centre.y + 0.13, z + 0.003),
                        None,
                    );
                    piece(
                        parts.handle.clone(),
                        looks.metal.clone(),
                        Vec3::new(cabinet.centre.x, cabinet.top() + 0.015, cabinet.centre.z),
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_stands_on_the_deck() {
        let stack = stack_layout(1.0);
        assert!(
            (stack[0].bottom() - (DECK_TOP + FOOT)).abs() < 1e-6,
            "{}",
            stack[0].bottom()
        );
        assert_eq!(stack[0].kind, CabinetKind::Sub);
        assert!(stack.iter().all(|c| (c.centre.x - STACK_X).abs() < 1e-6));
        assert!(
            stack_layout(-1.0)
                .iter()
                .all(|c| (c.centre.x + STACK_X).abs() < 1e-6)
        );
    }

    #[test]
    fn cabinets_stack_without_gap_or_overlap_beyond_the_cleat() {
        let stack = stack_layout(1.0);
        for pair in stack.windows(2) {
            assert!(
                (pair[1].bottom() - (pair[0].top() + CLEAT)).abs() < 1e-6,
                "{:?} on {:?}",
                pair[1],
                pair[0]
            );
        }
        assert_eq!(stack[3].kind, CabinetKind::Head);
    }

    #[test]
    fn the_head_stays_below_the_camera() {
        let stack = stack_layout(1.0);
        assert!(stack[3].top() < CAMERA_Y - 0.2, "top {}", stack[3].top());
    }

    #[test]
    fn the_cone_strokes_out_on_the_beat_and_rests_between() {
        assert!(
            (cone_stroke(0.5, 0.0) - 1.0).abs() < 1e-6,
            "out on the beat"
        );
        assert!(cone_stroke(0.0, 0.0).abs() < 1e-6, "at rest between");
        for i in 0..40 {
            let s = cone_stroke(i as f32 * 0.123, 0.4);
            assert!((0.0..=1.0).contains(&s));
        }
    }
}
