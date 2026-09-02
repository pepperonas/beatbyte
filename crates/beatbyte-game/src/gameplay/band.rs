//! The band on stage.
//!
//! The genre's stage has a band on it — a guitarist, a bassist, a
//! drummer on a riser, a singer at a stand — and the venue here had
//! everything but. These are **original figures built from
//! primitives**, in the same silhouette language as the crowd
//! (capsule, sphere, box), on a raised platform between the neck's
//! far end and the back wall: they stand where the eye lands past
//! the vanishing point and can never occlude a note (the G23 rule —
//! nothing on stage sits inside the bed).
//!
//! They play. Every movement is a pure function of song beats (and
//! whether Hype runs), so the band is deterministic, costs a few
//! transforms per frame, and keeps time with the music rather than
//! the frame rate — the same discipline as the crowd's bob and the
//! LED wall's pulse.
//!
//! No character is anyone's: no likeness, no costume, no logo. Four
//! dark figures and their instruments, lit by the stage.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::stage3d::{NeckStyle, STAGE_LAYER, Stage3d, neck_style};
use super::{GameplayScreen, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;

/// Where the band riser stands: its front edge meets the neck's far
/// end (−26), so the band is the thing the neck runs INTO — the
/// first placement, 7 units further back, put the figures 65 % into
/// the fog and they read as more crowd.
const RISER_Z: f32 = -30.0;
/// The riser's depth; its front face sits at `RISER_Z + DEPTH/2`.
const RISER_DEPTH: f32 = 8.0;
/// The riser's top surface. Raised, so the figures show above the
/// neck's vanishing end rather than behind it.
const RISER_TOP: f32 = 1.3;
/// The figures are drawn larger than life: at thirty units they
/// would otherwise be a few pixels tall.
const FIGURE_SCALE: f32 = 1.5;
/// The drummer's own riser, at the back of the band's.
const DRUM_RISER_H: f32 = 0.5;

/// Who a figure is. Drives both the pose and the animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Front centre, at the stand.
    Singer,
    /// Stage left, strumming.
    Guitarist,
    /// Stage right, strumming lower and slower.
    Bassist,
    /// Back centre on the drum riser, hitting on the beat.
    Drummer,
}

/// A part of a figure that moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The whole figure: bobs on the beat (and sways, for the singer).
    Body,
    /// The strumming or hitting arm; `0` = right, `1` = left.
    Arm(u8),
    /// The head: nods.
    Head,
}

/// A moving part of a band member, with its resting transform so the
/// motion is an offset and never a drift.
#[derive(Component)]
pub struct BandPart {
    /// Who this belongs to.
    pub role: Role,
    /// Which part.
    pub part: Part,
    /// Radians of offset into the beat, so four figures do not pump
    /// as one block.
    pub phase: f32,
    /// Where it rests.
    pub rest: Transform,
}

/// Marker for everything the band spawned.
#[derive(Component)]
pub struct Band;

/// How high a body bobs at this point in the beat, in world units.
///
/// Half a beat up, half a beat down, never below rest — a musician
/// bounces, they do not sink through the riser. Hype lifts the whole
/// band higher: the boost is visible on stage, not only on the HUD.
/// Pure — tested.
#[must_use]
pub fn bob(beats: f32, phase: f32, hype: bool) -> f32 {
    let swing = (beats * core::f32::consts::PI + phase).sin().max(0.0);
    let height = if hype { 0.34 } else { 0.18 };
    swing * height
}

/// The strumming arm's angle (radians about X, forward-back) for a
/// guitarist or bassist: one down-stroke per beat, snapping down
/// and easing back up — the shape a strum has. The bassist strums
/// every other beat and shallower. Pure — tested.
#[must_use]
pub fn strum(beats: f32, role: Role, hype: bool) -> f32 {
    let (per_beat, depth) = match role {
        Role::Bassist => (0.5, 0.45),
        _ => (1.0, 0.7),
    };
    let depth = if hype { depth * 1.3 } else { depth };
    // Position inside the stroke, 0 = just struck.
    let t = (beats * per_beat).rem_euclid(1.0);
    // Fast down (first 15 %), slow recovery: a saw with a soft return.
    let down = if t < 0.15 {
        t / 0.15
    } else {
        1.0 - (t - 0.15) / 0.85
    };
    -depth * down
}

/// A drummer's arm: the right hand keeps the beat, the left answers
/// on the off-beat. Returns the lift in radians (positive = raised,
/// so the hit is the drop to zero). Pure — tested.
#[must_use]
pub fn drum(beats: f32, hand: u8, hype: bool) -> f32 {
    let offset = if hand == 0 { 0.0 } else { 0.5 };
    let t = (beats + offset).rem_euclid(1.0);
    // Raised through most of the beat, dropping sharply into the hit
    // at the beat line.
    let lift = if t < 0.7 {
        t / 0.7
    } else {
        1.0 - (t - 0.7) / 0.3
    };
    let range = if hype { 0.9 } else { 0.65 };
    lift * range
}

/// The singer's sway (radians about Z): a slow lean, one full cycle
/// per two bars, so the front of the stage moves at a different
/// rate from the beat everyone else is on. Pure — tested.
#[must_use]
pub fn sway(beats: f32) -> f32 {
    (beats * core::f32::consts::PI / 4.0).sin() * 0.10
}

/// The nod (radians about X): a dip on every beat, small.
#[must_use]
pub fn nod(beats: f32, phase: f32) -> f32 {
    (beats * core::f32::consts::TAU + phase).sin().max(0.0) * 0.12
}

/// Where each member stands on the riser, and which way they face.
/// `(x, z, facing)` where facing is radians about Y — the crowd is
/// toward +Z, so 0 faces them. Pure — tested.
#[must_use]
pub fn stand(role: Role) -> (f32, f32, f32) {
    match role {
        Role::Singer => (0.0, RISER_Z + 2.2, 0.0),
        Role::Guitarist => (-3.6, RISER_Z + 0.4, 0.35),
        Role::Bassist => (3.6, RISER_Z + 0.4, -0.35),
        Role::Drummer => (0.0, RISER_Z - 2.4, 0.0),
    }
}

/// Spawn the band. Instrument neck only — the 8-bit stage is left
/// exactly as it was.
#[allow(clippy::too_many_lines)] // one figure after another; splitting it would scatter the layout
pub fn spawn_band(
    mut commands: Commands,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) || neck_style(&settings) != NeckStyle::Instrument {
        return;
    }
    let stage = theme.0;
    let dark = stage.background;
    let layer = RenderLayers::layer(STAGE_LAYER);

    // The riser: a dark platform with a lit front edge, like the one
    // the highway stands on.
    let riser = meshes.add(Cuboid::new(14.0, RISER_TOP + 1.5, RISER_DEPTH));
    let riser_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.72),
        perceptual_roughness: 0.75,
        ..default()
    });
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Band,
        Mesh3d(riser),
        MeshMaterial3d(riser_material.clone()),
        Transform::from_xyz(0.0, (RISER_TOP - 1.5) / 2.0, RISER_Z),
        layer.clone(),
    ));
    // The drummer's riser, one step higher at the back.
    let drum_riser = meshes.add(Cuboid::new(4.2, DRUM_RISER_H, 3.0));
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Band,
        Mesh3d(drum_riser),
        MeshMaterial3d(riser_material),
        Transform::from_xyz(0.0, RISER_TOP + DRUM_RISER_H / 2.0, RISER_Z - 2.6),
        layer.clone(),
    ));
    // A warm wash over the band, so the figures read as figures
    // against the dark and not as more crowd. Ranged: it must not
    // reach the neck.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Band,
        PointLight {
            color: stage.accent.mix(&Color::srgb(1.0, 0.85, 0.7), 0.6),
            intensity: 1_400_000.0,
            range: 12.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, RISER_TOP + 5.0, RISER_Z + 2.0),
        layer.clone(),
    ));

    // Bodies: the crowd's silhouette, a shade lighter, so a lit
    // band member is a person and a lit crowd member stays a mass.
    let body_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.45),
        perceptual_roughness: 0.85,
        ..default()
    });
    let instrument_material = materials.add(StandardMaterial {
        base_color: dark.mix(&Color::BLACK, 0.5).mix(&stage.accent, 0.18),
        perceptual_roughness: 0.45,
        metallic: 0.3,
        ..default()
    });
    let chrome = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.6, 0.55),
        perceptual_roughness: 0.28,
        metallic: 0.9,
        ..default()
    });

    let head = meshes.add(Sphere::new(0.27).mesh().uv(10, 8));
    let torso = meshes.add(Capsule3d::new(0.3, 0.8).mesh().latitudes(8).longitudes(12));
    let arm = meshes.add(Cuboid::new(0.13, 0.78, 0.13));
    let leg = meshes.add(Cuboid::new(0.16, 0.9, 0.16));
    let guitar_body = meshes.add(Cuboid::new(0.95, 0.6, 0.14));
    let guitar_neck = meshes.add(Cuboid::new(0.95, 0.1, 0.06));
    let stick = meshes.add(Cuboid::new(0.04, 0.5, 0.04));
    let mic_stand = meshes.add(Cylinder::new(0.025, 1.7));
    let mic = meshes.add(Sphere::new(0.07).mesh().uv(8, 6));
    let drum_shell = meshes.add(Cylinder::new(0.32, 0.28));
    let kick = meshes.add(Cylinder::new(0.48, 0.5));
    let cymbal = meshes.add(Cylinder::new(0.42, 0.02));
    let cymbal_stand = meshes.add(Cylinder::new(0.02, 1.2));

    for (index, role) in [Role::Singer, Role::Guitarist, Role::Bassist, Role::Drummer]
        .into_iter()
        .enumerate()
    {
        let (x, z, facing) = stand(role);
        let floor = if role == Role::Drummer {
            RISER_TOP + DRUM_RISER_H
        } else {
            RISER_TOP
        };
        let phase = index as f32 * 0.9;
        // The drummer sits; everyone else stands.
        let hip = if role == Role::Drummer { 0.55 } else { 0.9 };
        let rest = Transform::from_xyz(x, floor + hip * FIGURE_SCALE, z)
            .with_rotation(Quat::from_rotation_y(facing))
            .with_scale(Vec3::splat(FIGURE_SCALE));
        let root = commands
            .spawn((
                GameplayScreen,
                Stage3d,
                Band,
                BandPart {
                    role,
                    part: Part::Body,
                    phase,
                    rest,
                },
                rest,
                Visibility::default(),
                layer.clone(),
            ))
            .id();

        commands.entity(root).with_children(|body| {
            body.spawn((
                Mesh3d(torso.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, 0.2, 0.0),
                layer.clone(),
            ));
            let head_rest = Transform::from_xyz(0.0, 0.92, 0.0);
            body.spawn((
                BandPart {
                    role,
                    part: Part::Head,
                    phase,
                    rest: head_rest,
                },
                Mesh3d(head.clone()),
                MeshMaterial3d(body_material.clone()),
                head_rest,
                layer.clone(),
            ));
            if role != Role::Drummer {
                for side in [-1.0f32, 1.0] {
                    body.spawn((
                        Mesh3d(leg.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_xyz(side * 0.17, -0.6, 0.0),
                        layer.clone(),
                    ));
                }
            }
            match role {
                Role::Guitarist | Role::Bassist => {
                    // The instrument hangs across the body, neck out
                    // to the player's left.
                    let tilt = if role == Role::Bassist { 0.2 } else { 0.35 };
                    body.spawn((
                        Mesh3d(guitar_body.clone()),
                        MeshMaterial3d(instrument_material.clone()),
                        Transform::from_xyz(0.1, 0.0, 0.38)
                            .with_rotation(Quat::from_rotation_z(tilt)),
                        layer.clone(),
                    ));
                    body.spawn((
                        Mesh3d(guitar_neck.clone()),
                        MeshMaterial3d(instrument_material.clone()),
                        Transform::from_xyz(-0.85, 0.32, 0.38)
                            .with_rotation(Quat::from_rotation_z(tilt)),
                        layer.clone(),
                    ));
                    // Fretting arm, still; strumming arm, animated.
                    body.spawn((
                        Mesh3d(arm.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_xyz(-0.42, 0.35, 0.25)
                            .with_rotation(Quat::from_rotation_z(0.9)),
                        layer.clone(),
                    ));
                    let strum_rest = Transform::from_xyz(0.42, 0.55, 0.2)
                        .with_rotation(Quat::from_rotation_z(-0.3));
                    body.spawn((
                        BandPart {
                            role,
                            part: Part::Arm(0),
                            phase,
                            rest: strum_rest,
                        },
                        Mesh3d(arm.clone()),
                        MeshMaterial3d(body_material.clone()),
                        strum_rest,
                        layer.clone(),
                    ));
                }
                Role::Singer => {
                    // One hand on the stand, the other free.
                    body.spawn((
                        Mesh3d(arm.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform::from_xyz(-0.3, 0.45, 0.25)
                            .with_rotation(Quat::from_rotation_x(-0.9)),
                        layer.clone(),
                    ));
                    let free_rest = Transform::from_xyz(0.4, 0.3, 0.0)
                        .with_rotation(Quat::from_rotation_z(-0.2));
                    body.spawn((
                        BandPart {
                            role,
                            part: Part::Arm(0),
                            phase,
                            rest: free_rest,
                        },
                        Mesh3d(arm.clone()),
                        MeshMaterial3d(body_material.clone()),
                        free_rest,
                        layer.clone(),
                    ));
                    // The stand, planted in front.
                    body.spawn((
                        Mesh3d(mic_stand.clone()),
                        MeshMaterial3d(chrome.clone()),
                        Transform::from_xyz(-0.1, -0.05, 0.55),
                        layer.clone(),
                    ));
                    body.spawn((
                        Mesh3d(mic.clone()),
                        MeshMaterial3d(chrome.clone()),
                        Transform::from_xyz(-0.1, 0.82, 0.45),
                        layer.clone(),
                    ));
                }
                Role::Drummer => {
                    // Two arms with sticks, each animated on its own beat.
                    for hand in 0u8..2 {
                        let side = if hand == 0 { 1.0 } else { -1.0 };
                        let arm_rest = Transform::from_xyz(side * 0.38, 0.4, 0.3)
                            .with_rotation(Quat::from_rotation_x(-0.6));
                        body.spawn((
                            BandPart {
                                role,
                                part: Part::Arm(hand),
                                phase,
                                rest: arm_rest,
                            },
                            Mesh3d(arm.clone()),
                            MeshMaterial3d(body_material.clone()),
                            arm_rest,
                            layer.clone(),
                        ))
                        .with_child((
                            Mesh3d(stick.clone()),
                            MeshMaterial3d(chrome.clone()),
                            Transform::from_xyz(0.0, -0.55, 0.12)
                                .with_rotation(Quat::from_rotation_x(1.2)),
                            layer.clone(),
                        ));
                    }
                }
            }
        });

        // The drum kit stands in front of the drummer on the riser —
        // static geometry, not part of the animated figure.
        if role == Role::Drummer {
            let kit_z = z + 1.4;
            let kit_y = floor;
            let kit_scale = Vec3::splat(FIGURE_SCALE);
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Band,
                Mesh3d(kick.clone()),
                MeshMaterial3d(instrument_material.clone()),
                Transform::from_xyz(x, kit_y + 0.48 * FIGURE_SCALE, kit_z + 0.3)
                    .with_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                    .with_scale(kit_scale),
                layer.clone(),
            ));
            for (dx, dz, dy) in [(-0.75, -0.1, 0.75), (0.75, -0.1, 0.75), (-0.35, 0.55, 0.62)] {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(drum_shell.clone()),
                    MeshMaterial3d(instrument_material.clone()),
                    Transform::from_xyz(
                        x + dx * FIGURE_SCALE,
                        kit_y + dy * FIGURE_SCALE,
                        kit_z + dz,
                    )
                    .with_scale(kit_scale),
                    layer.clone(),
                ));
            }
            for (dx, dz, tilt) in [(-1.35, 0.2, 0.25), (1.3, 0.1, -0.2)] {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(cymbal_stand.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(
                        x + dx * FIGURE_SCALE,
                        kit_y + 0.6 * FIGURE_SCALE,
                        kit_z + dz,
                    )
                    .with_scale(kit_scale),
                    layer.clone(),
                ));
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(cymbal.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(
                        x + dx * FIGURE_SCALE,
                        kit_y + 1.22 * FIGURE_SCALE,
                        kit_z + dz,
                    )
                    .with_rotation(Quat::from_rotation_z(tilt))
                    .with_scale(kit_scale),
                    layer.clone(),
                ));
            }
        }
    }
}

/// Move the band with the song. Pure functions of beats and Hype,
/// applied as offsets to each part's resting transform.
pub fn animate_band(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mut parts: Query<(&BandPart, &mut Transform)>,
) {
    if !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player)) = (game_clock.song_time(&time), players.iter().next()) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    let hype = player.session.performance().hype_active();
    for (part, mut transform) in &mut parts {
        let rest = part.rest;
        *transform = match (part.role, part.part) {
            (Role::Singer, Part::Body) => {
                let mut t = rest;
                t.translation.y += bob(beats, part.phase, hype) * 0.6;
                t.rotation = rest.rotation * Quat::from_rotation_z(sway(beats));
                t
            }
            (_, Part::Body) => {
                let mut t = rest;
                t.translation.y += bob(beats, part.phase, hype);
                t
            }
            (_, Part::Head) => {
                let mut t = rest;
                t.rotation = rest.rotation * Quat::from_rotation_x(nod(beats, part.phase));
                t
            }
            (Role::Guitarist | Role::Bassist, Part::Arm(_)) => {
                let mut t = rest;
                t.rotation = rest.rotation * Quat::from_rotation_x(strum(beats, part.role, hype));
                t
            }
            (Role::Drummer, Part::Arm(hand)) => {
                let mut t = rest;
                t.rotation = rest.rotation * Quat::from_rotation_x(-drum(beats, hand, hype));
                t
            }
            (Role::Singer, Part::Arm(_)) => {
                // The free arm goes up under Hype and stays down
                // otherwise — the singer calls the moment.
                let mut t = rest;
                let lift = if hype { 2.4 } else { 0.0 };
                t.rotation = rest.rotation * Quat::from_rotation_z(-lift);
                t
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_bobs_up_never_down_and_higher_under_hype() {
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for step in 0..400 {
            let beats = step as f32 * 0.01;
            let calm = bob(beats, 0.0, false);
            lowest = lowest.min(calm);
            highest = highest.max(calm);
            assert!(
                bob(beats, 0.0, true) >= calm,
                "hype never lowers the bounce"
            );
        }
        assert!(lowest >= 0.0, "never below the riser");
        assert!(highest > 0.1, "and it actually moves");
    }

    #[test]
    fn a_strum_strikes_once_per_beat_and_the_bass_every_other() {
        // The stroke is at its deepest right at the beat line and
        // recovers through the beat.
        let at_beat = strum(1.15, Role::Guitarist, false);
        let mid = strum(1.6, Role::Guitarist, false);
        assert!(
            at_beat < mid,
            "deepest just after the beat, {at_beat} vs {mid}"
        );
        // The bass takes two beats per stroke: at 1.15 it is
        // recovering, not striking.
        let bass_at = strum(2.15, Role::Bassist, false);
        let bass_off = strum(3.15, Role::Bassist, false);
        assert!(bass_at < bass_off, "the bass strikes on even beats");
        assert!(
            strum(1.15, Role::Guitarist, true) < at_beat,
            "hype digs deeper"
        );
    }

    #[test]
    fn the_drummers_hands_alternate() {
        // Right hand lowest at the beat, left hand lowest half a beat
        // later — the hit is the drop to zero.
        assert!(drum(1.0, 0, false) < 0.05);
        assert!(drum(1.5, 1, false) < 0.05);
        // The left hand is at 5/7 of its lift when the right hits
        // (t = 0.5 of a 0.7 rise, scaled by the 0.65 range).
        assert!(
            drum(1.0, 1, false) > 0.4,
            "the left hand is up when the right hits"
        );
        assert!(
            drum(1.35, 0, false) > drum(1.0, 0, false),
            "raised between hits"
        );
    }

    #[test]
    fn the_band_stands_behind_the_neck_and_off_it() {
        // The neck ends at z = -26 and is ~1.6 wide either side of
        // centre near the strike line, narrowing away. Every member
        // stands further back than the neck's end — nothing can sit
        // on the board — and the flanks stand well outside its width.
        for role in [Role::Singer, Role::Guitarist, Role::Bassist, Role::Drummer] {
            let (x, z, _) = stand(role);
            assert!(z < -26.0, "{role:?} stands past the neck's end, z = {z}");
            assert!(
                z > RISER_Z - RISER_DEPTH / 2.0,
                "{role:?} stands ON the riser"
            );
            assert!(z > -40.0, "{role:?} stands in front of the back wall");
            if matches!(role, Role::Guitarist | Role::Bassist) {
                assert!(x.abs() > 2.5, "{role:?} flanks the centre");
            }
        }
        // The riser's front edge is exactly the neck's end: the neck
        // runs INTO the stage, and no part of the riser is in the bed.
        assert!((RISER_Z + RISER_DEPTH / 2.0 + 26.0).abs() < 1e-6);
        // And nobody shares a spot.
        let spots: Vec<(i32, i32)> = [Role::Singer, Role::Guitarist, Role::Bassist, Role::Drummer]
            .iter()
            .map(|r| {
                let (x, z, _) = stand(*r);
                ((x * 10.0) as i32, (z * 10.0) as i32)
            })
            .collect();
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_singers_sway_is_slow_and_small() {
        // One full cycle per eight beats (two bars), never past a
        // tenth of a radian either way.
        assert!(sway(0.0).abs() < 1e-6);
        assert!((sway(2.0) - 0.10).abs() < 1e-6, "peak lean at two beats");
        assert!(sway(6.0) < -0.09, "and the other way at six");
        assert!((sway(8.0)).abs() < 1e-5, "back to centre at eight");
    }
}
