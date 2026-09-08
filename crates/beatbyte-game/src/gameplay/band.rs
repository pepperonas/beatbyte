//! The band on stage.
//!
//! The genre's stage has a band on it — a guitarist, a bassist, a
//! drummer on a riser, a singer at a stand — and the venue here had
//! everything but. Since 2026-09-07 the four are people from the same
//! [`figure`](super::figure) builder as the crowd (elbows, hands, hair,
//! a shirt), on a raised platform between the neck's far end and the
//! back wall: they stand where the eye lands past the vanishing point
//! and can never occlude a note (the G23 rule — nothing on stage sits
//! inside the bed).
//!
//! They play. Every movement is a pure function of song beats (and
//! whether Hype runs) turned into a [`Pose`] per member, so the band
//! is deterministic, costs a few dozen transforms per frame, and keeps
//! time with the music rather than the frame rate — the same
//! discipline as the crowd and the LED wall.
//!
//! No character is anyone's: no likeness, no costume, no logo. Four
//! figures and their instruments, lit by the stage's key and rimmed by
//! its backline.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use core::f32::consts::FRAC_PI_2;

use super::figure::{
    ArmPose, Build, Detail, FigureAssets, FigureJoint, FigureSpec, Hair, Pose, Stance,
    pose_transform, spawn_figure,
};
use super::stage3d::{STAGE_LAYER, Stage3d};
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
/// A band member's height before the stage scale.
pub const BAND_HEIGHT: f32 = 1.78;
/// The drummer's own riser, at the back of the band's.
const DRUM_RISER_H: f32 = 0.5;

/// Who a figure is. Drives both the look and the playing.
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

impl Role {
    /// The four, in spawn order.
    pub const ALL: [Role; 4] = [Role::Singer, Role::Guitarist, Role::Bassist, Role::Drummer];
}

/// A band member's root.
#[derive(Component, Debug, Clone, Copy)]
pub struct BandMember {
    /// Who.
    pub role: Role,
    /// Radians of offset into the beat, so four figures do not pump
    /// as one block.
    pub phase: f32,
}

/// Marks a band figure's joints (disjoint from the crowd's).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct BandJoint;

/// Marker for everything the band spawned.
#[derive(Component)]
pub struct Band;

/// How far the singer's free arm is up (eased toward Hype).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct BandMood {
    /// 0 = down, 1 = up.
    pub arm_up: f32,
}

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

/// The drummer sits; everyone else stands. Pure — tested.
#[must_use]
pub fn stance(role: Role) -> Stance {
    match role {
        Role::Drummer => Stance::Seated,
        _ => Stance::Standing,
    }
}

/// What each member looks like: a fixed look per role (the band is
/// the same band every night), lit tones so the key reads on them.
#[must_use]
pub fn role_spec(role: Role) -> FigureSpec {
    let base = FigureSpec {
        height: BAND_HEIGHT,
        build: Build::Medium,
        hair: Hair::Short,
        detail: Detail::Full,
        top: 0,
        bottom: 0,
        skin: 1,
        hair_tone: 0,
        sleeves: true,
    };
    match role {
        Role::Singer => FigureSpec {
            build: Build::Slim,
            hair: Hair::Long,
            top: 7,
            bottom: 1,
            skin: 0,
            hair_tone: 3,
            ..base
        },
        Role::Guitarist => FigureSpec {
            hair: Hair::Short,
            top: 4,
            bottom: 0,
            skin: 2,
            hair_tone: 0,
            ..base
        },
        Role::Bassist => FigureSpec {
            build: Build::Broad,
            hair: Hair::Beanie,
            top: 5,
            bottom: 2,
            skin: 1,
            hair_tone: 1,
            ..base
        },
        Role::Drummer => FigureSpec {
            hair: Hair::Cap,
            top: 3,
            bottom: 3,
            skin: 3,
            hair_tone: 2,
            sleeves: false,
            ..base
        },
    }
}

/// The root scale of a band member (its height on stage).
#[must_use]
pub fn member_scale() -> f32 {
    BAND_HEIGHT * FIGURE_SCALE
}

/// A member's pose at song position `beats`: the playing, from the
/// pure per-role functions, as joint angles. `arm_up` is the
/// singer's free arm (0 down, 1 up), eased outside. Pure — tested:
/// every pose stays inside the joint limits at every beat.
#[must_use]
pub fn role_pose(role: Role, beats: f32, phase: f32, hype: bool, arm_up: f32) -> Pose {
    let lift = bob(beats, phase, hype) / member_scale();
    let head_nod = nod(beats, phase);
    match role {
        Role::Singer => Pose {
            lift: lift * 0.6,
            side: sway(beats),
            head_nod,
            arms: [
                // On the stand: forward and down to the mic.
                ArmPose {
                    raise: 0.9,
                    spread: -0.2,
                    elbow: 0.9,
                },
                // Free: up under Hype — the singer calls the moment.
                ArmPose {
                    raise: 0.15 + 2.75 * arm_up.clamp(0.0, 1.0),
                    spread: 0.15,
                    elbow: 0.4,
                },
            ],
            ..Pose::default()
        },
        Role::Guitarist | Role::Bassist => {
            let s = strum(beats, role, hype);
            Pose {
                lift,
                head_nod,
                lean: 0.06,
                arms: [
                    // Fretting hand out along the neck (the figure's
                    // left, where the neck points).
                    ArmPose {
                        raise: 0.7,
                        spread: 0.35,
                        elbow: 1.4,
                    },
                    // Strumming arm: lower and straighter on the
                    // down-stroke (`strum` is negative there).
                    ArmPose {
                        raise: 0.75 + 0.35 * s,
                        spread: -0.1,
                        elbow: 1.7 + 0.9 * s,
                    },
                ],
                ..Pose::default()
            }
        }
        Role::Drummer => Pose {
            lift: lift * 0.3,
            head_nod,
            lean: 0.15,
            arms: [
                ArmPose {
                    raise: 0.35 + 0.8 * drum(beats, 1, hype),
                    spread: 0.25,
                    elbow: 1.2,
                },
                ArmPose {
                    raise: 0.35 + 0.8 * drum(beats, 0, hype),
                    spread: 0.25,
                    elbow: 1.2,
                },
            ],
            ..Pose::default()
        },
    }
}

/// A box translated into place, for the instruments.
fn block(size: Vec3, at: Vec3) -> Mesh {
    Mesh::from(Cuboid::from_size(size)).translated_by(at)
}

fn merged(mut base: Mesh, pieces: Vec<Mesh>) -> Mesh {
    for piece in &pieces {
        if let Err(error) = base.merge(piece) {
            warn!("band: an instrument piece could not be merged ({error:?})");
        }
    }
    base
}

/// A guitar (or bass) in the figure's frame, hanging from the spine:
/// a waisted body from two discs and a bridge block, a neck out to
/// the figure's left, a tilted headstock. Sizes in units of the
/// figure's height. Pure geometry — no logo, no maker's shape.
fn guitar_mesh(bass: bool) -> Mesh {
    let (lower, upper, neck_len) = if bass {
        (0.11, 0.09, 0.38)
    } else {
        (0.10, 0.085, 0.32)
    };
    let disc = |r: f32, x: f32| {
        Mesh::from(Cylinder::new(r, 0.02))
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2))
            .translated_by(Vec3::new(x, 0.0, 0.0))
    };
    merged(
        disc(lower, -0.07),
        vec![
            disc(upper, 0.13),
            block(Vec3::new(0.18, 0.15, 0.02), Vec3::new(0.03, 0.0, 0.0)),
            block(
                Vec3::new(neck_len, 0.026, 0.015),
                Vec3::new(0.13 + neck_len * 0.5, 0.01, 0.0),
            ),
            block(
                Vec3::new(0.07, 0.035, 0.012),
                Vec3::new(0.13 + neck_len + 0.03, 0.02, 0.0),
            ),
        ],
    )
}

/// The chrome bits of a guitar: two pickups and a bridge.
fn guitar_chrome() -> Mesh {
    merged(
        block(Vec3::new(0.02, 0.06, 0.012), Vec3::new(0.02, 0.0, 0.012)),
        vec![
            block(Vec3::new(0.02, 0.06, 0.012), Vec3::new(0.07, 0.0, 0.012)),
            block(Vec3::new(0.02, 0.05, 0.015), Vec3::new(-0.06, 0.0, 0.012)),
        ],
    )
}

/// The strap over the left shoulder to the right hip.
fn strap_mesh() -> Mesh {
    let from = Vec3::new(0.10, 0.19, 0.03);
    let to = Vec3::new(-0.08, -0.12, 0.09);
    let dir = to - from;
    Mesh::from(Capsule3d::new(0.008, dir.length()))
        .rotated_by(Quat::from_rotation_arc(Vec3::Y, dir.normalize()))
        .translated_by((from + to) * 0.5)
}

/// Spawn the band. Instrument neck only — the 8-bit stage is left
/// exactly as it was.
#[allow(clippy::too_many_lines)] // one figure after another; splitting it would scatter the layout
pub fn spawn_band(
    mut commands: Commands,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    assets: Res<FigureAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) {
        return;
    }
    let stage = theme.0;
    let dark = stage.background;
    let layer = RenderLayers::layer(STAGE_LAYER);
    commands.insert_resource(BandMood::default());

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

    let guitar = meshes.add(guitar_mesh(false));
    let bass = meshes.add(guitar_mesh(true));
    let guitar_bits = meshes.add(guitar_chrome());
    let strap = meshes.add(strap_mesh());
    let stick = meshes.add(block(
        Vec3::new(0.012, 0.012, 0.23),
        Vec3::new(0.0, -0.185, 0.13),
    ));
    let mic_stand = meshes.add(Cylinder::new(0.025, 2.3));
    let mic = meshes.add(Sphere::new(0.08).mesh().uv(8, 6));
    let stool_seat = meshes.add(Cylinder::new(0.22, 0.05));
    let stool_post = meshes.add(Cylinder::new(0.03, 0.55));
    let drum_shell = meshes.add(Cylinder::new(0.32, 0.28));
    let snare = meshes.add(Cylinder::new(0.28, 0.15));
    let rim = |r: f32| Torus {
        minor_radius: 0.015,
        major_radius: r,
    };
    let tom_rim = meshes.add(rim(0.32));
    let snare_rim = meshes.add(rim(0.28));
    let kick = meshes.add(Cylinder::new(0.48, 0.5));
    let cymbal = meshes.add(Cylinder::new(0.42, 0.02));
    let hihat = meshes.add(Cylinder::new(0.28, 0.015));
    let cymbal_stand = meshes.add(Cylinder::new(0.02, 1.2));
    let hihat_stand = meshes.add(Cylinder::new(0.02, 0.9));

    for (index, role) in Role::ALL.into_iter().enumerate() {
        let (x, z, facing) = stand(role);
        let floor = if role == Role::Drummer {
            RISER_TOP + DRUM_RISER_H
        } else {
            RISER_TOP
        };
        let phase = index as f32 * 0.9;
        let spec = role_spec(role);
        let root = Transform::from_xyz(x, floor, z)
            .with_rotation(Quat::from_rotation_y(facing))
            .with_scale(Vec3::splat(member_scale()));
        let handles = spawn_figure(
            &mut commands,
            &assets,
            &spec,
            stance(role),
            root,
            (GameplayScreen, Stage3d, Band, BandMember { role, phase }),
            BandJoint,
        );

        match role {
            Role::Guitarist | Role::Bassist => {
                // The instrument hangs across the body from the
                // spine, neck out to the figure's left, tilted up.
                let tilt = if role == Role::Bassist { 0.2 } else { 0.35 };
                let hang = Transform::from_xyz(-0.02, -0.08, 0.13)
                    .with_rotation(Quat::from_rotation_z(tilt));
                commands.spawn((
                    Mesh3d(if role == Role::Bassist {
                        bass.clone()
                    } else {
                        guitar.clone()
                    }),
                    MeshMaterial3d(instrument_material.clone()),
                    hang,
                    layer.clone(),
                    ChildOf(handles.spine),
                ));
                commands.spawn((
                    Mesh3d(guitar_bits.clone()),
                    MeshMaterial3d(chrome.clone()),
                    hang,
                    layer.clone(),
                    ChildOf(handles.spine),
                ));
                commands.spawn((
                    Mesh3d(strap.clone()),
                    MeshMaterial3d(assets.bottoms[0].clone()),
                    Transform::IDENTITY,
                    layer.clone(),
                    ChildOf(handles.spine),
                ));
            }
            Role::Singer => {
                // The stand, planted in front; the mic at mouth height.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(mic_stand.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(x - 0.15, floor + 1.15, z + 0.7),
                    layer.clone(),
                ));
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(mic.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(x - 0.15, floor + 2.36, z + 0.62),
                    layer.clone(),
                ));
            }
            Role::Drummer => {
                // Sticks in both hands, pointing forward and down.
                for hand in handles.hands {
                    commands.spawn((
                        Mesh3d(stick.clone()),
                        MeshMaterial3d(chrome.clone()),
                        Transform::from_rotation(Quat::from_rotation_x(-0.25)),
                        layer.clone(),
                        ChildOf(hand),
                    ));
                }
                // The stool under the seated figure.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(stool_seat.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(x, floor + 0.57, z),
                    layer.clone(),
                ));
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Band,
                    Mesh3d(stool_post.clone()),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_xyz(x, floor + 0.28, z),
                    layer.clone(),
                ));

                // The drum kit stands in front of the drummer on the
                // riser — static geometry, not part of the figure.
                let kit_z = z + 1.4;
                let kit_y = floor;
                let kit_scale = Vec3::splat(FIGURE_SCALE);
                let mut place = |mesh: Handle<Mesh>,
                                 material: Handle<StandardMaterial>,
                                 at: Vec3,
                                 rotation: Quat| {
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        Band,
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(at)
                            .with_rotation(rotation)
                            .with_scale(kit_scale),
                        layer.clone(),
                    ));
                };
                place(
                    kick.clone(),
                    instrument_material.clone(),
                    Vec3::new(x, kit_y + 0.48 * FIGURE_SCALE, kit_z + 0.3),
                    Quat::from_rotation_x(FRAC_PI_2),
                );
                for (dx, dz, dy) in [(-0.75, -0.1, 0.75), (0.75, -0.1, 0.75), (-0.35, 0.55, 0.62)] {
                    let at =
                        Vec3::new(x + dx * FIGURE_SCALE, kit_y + dy * FIGURE_SCALE, kit_z + dz);
                    place(
                        drum_shell.clone(),
                        instrument_material.clone(),
                        at,
                        Quat::IDENTITY,
                    );
                    place(
                        tom_rim.clone(),
                        chrome.clone(),
                        at + Vec3::new(0.0, 0.14 * FIGURE_SCALE, 0.0),
                        Quat::IDENTITY,
                    );
                }
                // The snare between the knees.
                let snare_at = Vec3::new(
                    x + 0.35 * FIGURE_SCALE,
                    kit_y + 0.55 * FIGURE_SCALE,
                    kit_z - 0.35,
                );
                place(
                    snare.clone(),
                    instrument_material.clone(),
                    snare_at,
                    Quat::IDENTITY,
                );
                place(
                    snare_rim.clone(),
                    chrome.clone(),
                    snare_at + Vec3::new(0.0, 0.075 * FIGURE_SCALE, 0.0),
                    Quat::IDENTITY,
                );
                for (dx, dz, tilt) in [(-1.35, 0.2, 0.25), (1.3, 0.1, -0.2)] {
                    place(
                        cymbal_stand.clone(),
                        chrome.clone(),
                        Vec3::new(
                            x + dx * FIGURE_SCALE,
                            kit_y + 0.6 * FIGURE_SCALE,
                            kit_z + dz,
                        ),
                        Quat::IDENTITY,
                    );
                    place(
                        cymbal.clone(),
                        chrome.clone(),
                        Vec3::new(
                            x + dx * FIGURE_SCALE,
                            kit_y + 1.22 * FIGURE_SCALE,
                            kit_z + dz,
                        ),
                        Quat::from_rotation_z(tilt),
                    );
                }
                // The hi-hat pair on its own stand, to the left.
                let hh = Vec3::new(x - 1.15 * FIGURE_SCALE, kit_y, kit_z - 0.2);
                place(
                    hihat_stand.clone(),
                    chrome.clone(),
                    hh + Vec3::new(0.0, 0.45 * FIGURE_SCALE, 0.0),
                    Quat::IDENTITY,
                );
                for dy in [0.90, 0.94] {
                    place(
                        hihat.clone(),
                        chrome.clone(),
                        hh + Vec3::new(0.0, dy * FIGURE_SCALE, 0.0),
                        Quat::from_rotation_z(0.06),
                    );
                }
            }
        }
    }
}

/// Move the band with the song: one pose per member from the pure
/// per-role functions, written to the joints as transforms only.
/// Gated on STAGE MOTION like every ambient movement.
pub fn animate_band(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    mood: Option<ResMut<BandMood>>,
    members: Query<(Entity, &BandMember)>,
    mut joints: Query<(&FigureJoint, &mut Transform), With<BandJoint>>,
) {
    if !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(player), Some(mut mood)) =
        (game_clock.song_time(&time), players.iter().next(), mood)
    else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now) as f32;
    let hype = player.session.performance().hype_active();
    let target = if hype { 1.0 } else { 0.0 };
    let step = (time.delta_secs() * 4.0).min(1.0);
    mood.arm_up += (target - mood.arm_up) * step;

    let poses: Vec<(Entity, Pose)> = members
        .iter()
        .map(|(entity, member)| {
            (
                entity,
                role_pose(member.role, beats, member.phase, hype, mood.arm_up),
            )
        })
        .collect();
    for (joint, mut transform) in &mut joints {
        let Some((_, pose)) = poses.iter().find(|(owner, _)| *owner == joint.owner) else {
            continue;
        };
        let next = pose_transform(pose, joint.joint, &joint.rest, joint.bendable);
        if *transform != next {
            *transform = next;
        }
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
        for role in Role::ALL {
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
        let spots: Vec<(i32, i32)> = Role::ALL
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

    #[test]
    fn the_drummer_sits_and_the_others_stand() {
        assert_eq!(stance(Role::Drummer), Stance::Seated);
        for role in [Role::Singer, Role::Guitarist, Role::Bassist] {
            assert_eq!(stance(role), Stance::Standing);
        }
        assert!(
            !role_spec(Role::Drummer).sleeves,
            "the drummer plays sleeveless"
        );
        assert!(
            role_spec(Role::Singer).top == 7,
            "the singer wears the pale shirt"
        );
    }

    #[test]
    fn every_role_pose_stays_inside_the_limits_and_plays() {
        for role in Role::ALL {
            let mut moved = 0;
            let mut last = role_pose(role, 0.0, 0.0, false, 0.0);
            for step in 1..200 {
                let beats = step as f32 * 0.07;
                for (hype, arm_up) in [(false, 0.0), (true, 1.0)] {
                    let pose = role_pose(role, beats, 0.9, hype, arm_up);
                    assert!(
                        pose.within_limits(),
                        "{role:?} at {beats} hype {hype}: {pose:?}"
                    );
                }
                let pose = role_pose(role, beats, 0.9, false, 0.0);
                if pose != last {
                    moved += 1;
                }
                last = pose;
            }
            assert!(moved > 100, "{role:?} plays: {moved} changes");
        }
        // The singer's free arm goes up under Hype.
        let down = role_pose(Role::Singer, 1.0, 0.0, false, 0.0).arms[1].raise;
        let up = role_pose(Role::Singer, 1.0, 0.0, true, 1.0).arms[1].raise;
        assert!(up > down + 2.0);
    }
}
