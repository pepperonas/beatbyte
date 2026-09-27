//! The backline behind the band: amp stacks and a skirted drum riser.
//!
//! The band riser used to end in an empty dark wall — four figures
//! standing in front of nothing. A stage always carries its backline:
//! here two full stacks per side (two 4×12 cabinets and a head each)
//! behind the guitarist and the bassist, and a cloth skirt on the
//! drummer's step so it reads as a riser rather than a black block.
//!
//! Static geometry on the tolex and grille-cloth textures the PA
//! already uses: no light, no material written per frame. Sizes are
//! real ones (a 4×12 is 0.76 m square) times the band's stage scale,
//! so an amp stands as tall as the player in front of it.
//!
//! What this module leaves out, on purpose: piping, corner caps and
//! cables. At the band's distance a unit is ~22 pixels, and a detail
//! under a pixel does not add realism — it shimmers.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::GameplayScreen;
use super::band::{Band, DRUM_RISER_DEPTH, DRUM_RISER_H, DRUM_RISER_W, DRUM_RISER_Z};
use super::band::{FIGURE_SCALE, RISER_DEPTH, RISER_TOP, RISER_Z};
use super::stage3d::{STAGE_LAYER, Stage3d};
use crate::surfaces::StageSurfaces;

/// A 4×12 cabinet, width × height × depth, before the stage scale.
const CAB: Vec3 = Vec3::new(0.76, 0.76, 0.36);
/// The amp head on top.
const HEAD: Vec3 = Vec3::new(0.75, 0.27, 0.28);
/// How far each stack stands from the centre line.
const STACK_X: [f32; 2] = [4.3, 5.55];
/// How far in front of the riser's back edge a stack's back stands.
const BACK_GAP: f32 = 0.4;
/// The tolex border around a cabinet's grille, before the scale.
/// Wide enough to be two pixels at the band's distance.
const GRILLE_BORDER: f32 = 0.06;

/// What an amp piece is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpKind {
    /// A speaker cabinet: tolex box, cloth grille.
    Cabinet,
    /// The head: tolex box, a control panel, knobs, a lamp.
    Head,
}

/// One amp piece's place and size, in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmpBox {
    /// What it is.
    pub kind: AmpKind,
    /// Its centre.
    pub centre: Vec3,
    /// Width, height, depth.
    pub size: Vec3,
}

impl AmpBox {
    /// Its lowest point.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.centre.y - self.size.y * 0.5
    }

    /// Its highest point.
    #[must_use]
    pub fn top(&self) -> f32 {
        self.centre.y + self.size.y * 0.5
    }

    /// Its front face.
    #[must_use]
    pub fn front(&self) -> f32 {
        self.centre.z + self.size.z * 0.5
    }
}

/// Every amp piece on the riser: four stacks (two per side), each two
/// cabinets and a head, seated on the riser and on each other. Pure —
/// tested.
#[must_use]
pub fn backline_layout() -> Vec<AmpBox> {
    let cab = CAB * FIGURE_SCALE;
    let head = HEAD * FIGURE_SCALE;
    let back = RISER_Z - RISER_DEPTH * 0.5 + BACK_GAP;
    let mut out = Vec::new();
    for side in [-1.0f32, 1.0] {
        for x in STACK_X {
            let x = side * x;
            let mut bottom = RISER_TOP;
            for _ in 0..2 {
                out.push(AmpBox {
                    kind: AmpKind::Cabinet,
                    centre: Vec3::new(x, bottom + cab.y * 0.5, back + cab.z * 0.5),
                    size: cab,
                });
                bottom += cab.y;
            }
            // The head sits flush with the cabinets' front.
            out.push(AmpBox {
                kind: AmpKind::Head,
                centre: Vec3::new(x, bottom + head.y * 0.5, back + cab.z - head.z * 0.5),
                size: head,
            });
        }
    }
    out
}

/// The knobs on a head's panel, as one mesh centred on the panel.
fn knobs(width: f32) -> Mesh {
    let knob = |x: f32| {
        Mesh::from(Cylinder::new(0.022 * FIGURE_SCALE, 0.03 * FIGURE_SCALE))
            .rotated_by(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
            .translated_by(Vec3::new(x, 0.0, 0.015 * FIGURE_SCALE))
    };
    let span = width * 0.62;
    let mut mesh = knob(-span * 0.5);
    for i in 1..8 {
        let x = -span * 0.5 + span * i as f32 / 7.0;
        if let Err(error) = mesh.merge(&knob(x)) {
            warn!("backline: a knob could not be merged ({error:?})");
        }
    }
    mesh
}

/// Spawn the backline. Called from the band's spawn, so it exists
/// exactly when the band does.
#[allow(clippy::too_many_lines)] // one piece after another
pub fn spawn_backline(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    surfaces: &StageSurfaces,
    riser_material: Handle<StandardMaterial>,
) {
    let layer = RenderLayers::layer(STAGE_LAYER);
    let tile = |x: f32, y: f32| bevy::math::Affine2::from_scale(Vec2::new(x, y));
    let tolex = materials.add(StandardMaterial {
        base_color: Color::srgb(0.055, 0.055, 0.06),
        base_color_texture: Some(surfaces.tolex_color.clone()),
        normal_map_texture: Some(surfaces.tolex_normal.clone()),
        metallic_roughness_texture: Some(surfaces.tolex_rough.clone()),
        perceptual_roughness: 1.0,
        uv_transform: tile(2.0, 2.0),
        ..default()
    });
    // Salt-and-pepper grille cloth: lighter than the tolex, so a
    // cabinet reads as a framed grille rather than a black box.
    let cloth = materials.add(StandardMaterial {
        base_color: Color::srgb(0.26, 0.25, 0.235),
        normal_map_texture: Some(surfaces.cloth_normal.clone()),
        perceptual_roughness: 0.9,
        uv_transform: tile(6.0, 6.0),
        ..default()
    });
    let panel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.52, 0.46),
        metallic_roughness_texture: Some(surfaces.metal_rough.clone()),
        metallic: 1.0,
        perceptual_roughness: 1.0,
        ..default()
    });
    let knob = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.03),
        perceptual_roughness: 0.5,
        ..default()
    });
    // The power lamp glows; it lights nothing.
    let lamp = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.1, 0.08),
        emissive: LinearRgba::rgb(6.0, 0.4, 0.2),
        ..default()
    });

    let cab = CAB * FIGURE_SCALE;
    let head = HEAD * FIGURE_SCALE;
    let border = GRILLE_BORDER * FIGURE_SCALE;
    let cab_body = meshes.add(Cuboid::from_size(cab));
    let grille = meshes.add(Cuboid::new(
        cab.x - 2.0 * border,
        cab.y - 2.0 * border,
        0.01,
    ));
    let head_body = meshes.add(Cuboid::from_size(head));
    let head_panel = meshes.add(Cuboid::new(head.x - 2.0 * border, head.y * 0.42, 0.01));
    let head_knobs = meshes.add(knobs(head.x));
    let head_lamp = meshes.add(Sphere::new(0.02 * FIGURE_SCALE).mesh().uv(8, 6));

    let mut put = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3| {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Band,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            layer.clone(),
        ));
    };
    for piece in backline_layout() {
        let face = Vec3::new(piece.centre.x, piece.centre.y, piece.front() + 0.005);
        match piece.kind {
            AmpKind::Cabinet => {
                put(&cab_body, &tolex, piece.centre);
                put(&grille, &cloth, face);
            }
            AmpKind::Head => {
                put(&head_body, &tolex, piece.centre);
                put(&head_panel, &panel, face);
                put(&head_knobs, &knob, face);
                put(
                    &head_lamp,
                    &lamp,
                    face + Vec3::new(head.x * 0.42, 0.0, 0.01),
                );
            }
        }
    }

    // The drummer's step gets a cloth skirt on its front, like every
    // hired riser: a lighter, woven face above the dark platform.
    let skirt = meshes.add(Cuboid::new(DRUM_RISER_W, DRUM_RISER_H, 0.02));
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Band,
        Mesh3d(skirt),
        MeshMaterial3d(cloth_skirt(materials, surfaces, riser_material)),
        Transform::from_xyz(
            0.0,
            RISER_TOP + DRUM_RISER_H * 0.5,
            DRUM_RISER_Z + DRUM_RISER_DEPTH * 0.5 + 0.011,
        ),
        layer,
    ));
}

/// The skirt's material: the riser's own colour, lifted a little and
/// woven, so the step reads without a new colour on stage.
fn cloth_skirt(
    materials: &mut Assets<StandardMaterial>,
    surfaces: &StageSurfaces,
    riser: Handle<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let base = materials
        .get(&riser)
        .map_or(Color::srgb(0.05, 0.05, 0.05), |m| m.base_color);
    materials.add(StandardMaterial {
        base_color: base.mix(&Color::srgb(0.2, 0.2, 0.2), 0.35),
        normal_map_texture: Some(surfaces.cloth_normal.clone()),
        perceptual_roughness: 0.95,
        uv_transform: bevy::math::Affine2::from_scale(Vec2::new(24.0, 3.0)),
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::band::{Role, stand};

    #[test]
    fn every_amp_stands_on_the_riser_behind_the_band_and_clear_of_the_drums() {
        let layout = backline_layout();
        assert_eq!(layout.len(), 12, "four stacks of three");
        let back_edge = RISER_Z - RISER_DEPTH * 0.5;
        for amp in &layout {
            assert!(
                amp.centre.z - amp.size.z * 0.5 >= back_edge - 1e-4,
                "{amp:?} hangs off the riser's back"
            );
            assert!(
                amp.centre.x.abs() + amp.size.x * 0.5 < 7.0,
                "{amp:?} hangs off the riser's side (14 wide)"
            );
            assert!(
                amp.centre.x.abs() - amp.size.x * 0.5 > DRUM_RISER_W * 0.5,
                "{amp:?} stands on the drum riser"
            );
            for role in [Role::Guitarist, Role::Bassist, Role::Singer] {
                let (_, z, _) = stand(role);
                assert!(amp.front() < z - 1.0, "{amp:?} stands in front of {role:?}");
            }
        }
    }

    #[test]
    fn a_stack_is_seated_piece_on_piece_and_as_tall_as_a_player() {
        let layout = backline_layout();
        for stack in layout.chunks(3) {
            assert!((stack[0].bottom() - RISER_TOP).abs() < 1e-4, "on the riser");
            assert!(
                (stack[1].bottom() - stack[0].top()).abs() < 1e-4,
                "cab on cab"
            );
            assert!(
                (stack[2].bottom() - stack[1].top()).abs() < 1e-4,
                "head on cab"
            );
            assert_eq!(stack[2].kind, AmpKind::Head);
            assert!(
                (stack[2].front() - stack[1].front()).abs() < 1e-4,
                "the head is flush with the cabinets"
            );
            let height = stack[2].top() - RISER_TOP;
            let player = super::super::band::member_scale();
            assert!(
                (0.85..1.2).contains(&(height / player)),
                "a full stack stands about as tall as a player: {height} vs {player}"
            );
        }
        // No two pieces overlap.
        for (i, a) in layout.iter().enumerate() {
            for b in &layout[i + 1..] {
                let d = (a.centre - b.centre).abs();
                let reach = (a.size + b.size) * 0.5 - Vec3::splat(1e-4);
                assert!(
                    d.x >= reach.x || d.y >= reach.y || d.z >= reach.z,
                    "{a:?} overlaps {b:?}"
                );
            }
        }
    }
}
