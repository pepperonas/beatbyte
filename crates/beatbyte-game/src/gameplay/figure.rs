//! Procedural people for the stage: one builder for the crowd and the
//! band.
//!
//! A figure is a small joint tree of primitives — pelvis, torso, head,
//! two arms with elbows, two legs with knees — built once per gameplay
//! entry at **unit height** and placed by its root, whose uniform
//! scale IS the person's height (1.55–1.92 in the crowd). Joints carry
//! rotation and translation only; a scaled, rotated joint would shear
//! its children. Every mesh is shared across figures (per build and
//! part), so Bevy's automatic instancing keeps a crowd of sixty at a
//! few dozen draw batches.
//!
//! No character is anyone's: no likeness, no costume, no logo — a
//! silhouette with proportions, a hair shape and a shirt tone, all from
//! a hash (CLAUDE.md asset rule).
//!
//! The pose model is a handful of joint angles ([`Pose`]) that the
//! crowd's dance moves and the band's playing produce as pure
//! functions; [`pose_transform`] turns them into a joint's local
//! transform, and the forward kinematics in [`joint_transform`] let the
//! tests prove the feet stay on the ground.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use core::f32::consts::PI;

use super::fx::hash01;
use super::stage3d::STAGE_LAYER;
use crate::theme::Theme;

/// A side of the body, seen from the figure (left = +X).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The figure's left (+X).
    Left,
    /// The figure's right (−X).
    Right,
}

impl Side {
    /// +1 for left, −1 for right: the sign of the side's x.
    #[must_use]
    pub fn sign(self) -> f32 {
        match self {
            Side::Left => 1.0,
            Side::Right => -1.0,
        }
    }

    /// Index into a `[T; 2]` (left first).
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// The joints of a figure. `Arm`/`Leg` are the one-piece limbs of the
/// simple (far-row) build; a full figure has elbows and knees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Joint {
    /// The pelvis: bounce and squash live here.
    Hips,
    /// The torso above the pelvis: lean, twist, sway.
    Spine,
    /// The head at the neck base: nod, tilt.
    Head,
    /// Shoulder joint.
    UpperArm(Side),
    /// Elbow joint (carries the hand).
    Forearm(Side),
    /// Hip joint.
    Thigh(Side),
    /// Knee joint (carries the foot).
    Shin(Side),
    /// A one-piece arm (simple build).
    Arm(Side),
    /// A one-piece leg (simple build).
    Leg(Side),
}

/// Hair silhouettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hair {
    /// A close cap of hair.
    Short,
    /// None.
    Bare,
    /// A cap with a brim.
    Cap,
    /// Short hair with a tail.
    Ponytail,
    /// Hair to the shoulders.
    Long,
    /// A knit hat.
    Beanie,
}

impl Hair {
    /// Every variant, in order.
    pub const ALL: [Hair; 6] = [
        Hair::Short,
        Hair::Bare,
        Hair::Cap,
        Hair::Ponytail,
        Hair::Long,
        Hair::Beanie,
    ];

    fn index(self) -> usize {
        match self {
            Hair::Short => 0,
            Hair::Bare => 1,
            Hair::Cap => 2,
            Hair::Ponytail => 3,
            Hair::Long => 4,
            Hair::Beanie => 5,
        }
    }
}

/// Body width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Build {
    /// Narrow.
    Slim,
    /// Average.
    Medium,
    /// Wide.
    Broad,
}

impl Build {
    /// Every variant, in order.
    pub const ALL: [Build; 3] = [Build::Slim, Build::Medium, Build::Broad];

    /// The width factor applied to everything sideways.
    #[must_use]
    pub fn factor(self) -> f32 {
        match self {
            Build::Slim => 0.88,
            Build::Medium => 1.0,
            Build::Broad => 1.14,
        }
    }

    fn index(self) -> usize {
        match self {
            Build::Slim => 0,
            Build::Medium => 1,
            Build::Broad => 2,
        }
    }
}

/// How much of a figure is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Detail {
    /// Elbows, knees, hands, feet — the front rows and the band.
    Full,
    /// One-piece limbs — the far row, where a knee is a pixel.
    Simple,
}

/// Standing or seated (the drummer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stance {
    /// On both feet.
    Standing,
    /// On a stool: hips low, thighs forward, shins down.
    Seated,
}

/// Everything that makes one figure look like itself. Palette fields
/// index the shared materials in [`FigureAssets`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FigureSpec {
    /// The person's height (the root's uniform scale).
    pub height: f32,
    /// Body width.
    pub build: Build,
    /// Hair silhouette.
    pub hair: Hair,
    /// Full or simple limbs.
    pub detail: Detail,
    /// Shirt tone (0..[`TOPS`]).
    pub top: u8,
    /// Trouser tone (0..[`BOTTOMS`]).
    pub bottom: u8,
    /// Skin tone (0..[`SKINS`]).
    pub skin: u8,
    /// Hair tone (0..[`HAIRS`]).
    pub hair_tone: u8,
    /// Sleeves on the upper arm (false = skin).
    pub sleeves: bool,
}

/// Number of shirt tones.
pub const TOPS: u8 = 8;
/// Number of trouser tones.
pub const BOTTOMS: u8 = 4;
/// Number of skin tones.
pub const SKINS: u8 = 4;
/// Number of hair tones.
pub const HAIRS: u8 = 4;

/// The shortest person in a crowd.
pub const MIN_HEIGHT: f32 = 1.55;
/// The tallest person in a crowd.
pub const MAX_HEIGHT: f32 = 1.92;

impl FigureSpec {
    /// A person from a seed: same seed, same person. Heights spread
    /// over [`MIN_HEIGHT`]..[`MAX_HEIGHT`]; shirts are mostly dark
    /// (a crowd is a silhouette mass) with a few pale ones to catch
    /// the light.
    #[must_use]
    pub fn from_hash(seed: usize, detail: Detail) -> FigureSpec {
        let h = |salt: usize| hash01(seed.wrapping_mul(131).wrapping_add(salt));
        let build = match h(6) {
            r if r < 0.30 => Build::Slim,
            r if r < 0.75 => Build::Medium,
            _ => Build::Broad,
        };
        let hair = match h(7) {
            r if r < 0.40 => Hair::Short,
            r if r < 0.52 => Hair::Bare,
            r if r < 0.70 => Hair::Cap,
            r if r < 0.85 => Hair::Ponytail,
            r if r < 0.95 => Hair::Long,
            _ => Hair::Beanie,
        };
        let top = match h(8) {
            r if r < 0.55 => (r / 0.55 * 5.0) as u8,
            r if r < 0.85 => 5 + ((r - 0.55) / 0.30 * 2.0) as u8,
            _ => 7,
        };
        FigureSpec {
            height: MIN_HEIGHT + (MAX_HEIGHT - MIN_HEIGHT) * h(5),
            build,
            hair,
            detail,
            top: top.min(TOPS - 1),
            bottom: ((h(9) * f32::from(BOTTOMS)) as u8).min(BOTTOMS - 1),
            skin: ((h(10) * f32::from(SKINS)) as u8).min(SKINS - 1),
            hair_tone: ((h(11) * f32::from(HAIRS)) as u8).min(HAIRS - 1),
            sleeves: true,
        }
    }
}

/// The root entity of a figure.
#[derive(Component, Debug, Clone, Copy)]
pub struct FigureRoot {
    /// What this person looks like.
    pub spec: FigureSpec,
}

/// A joint entity: its rest transform and whether it bends (a simple
/// figure's hips do not drop with a squash — its legs cannot fold).
#[derive(Component, Debug, Clone, Copy)]
pub struct FigureJoint {
    /// The figure's root.
    pub owner: Entity,
    /// Which joint.
    pub joint: Joint,
    /// The joint's local transform at rest.
    pub rest: Transform,
    /// Whether the legs fold under a squash.
    pub bendable: bool,
}

/// The entities a caller may hang things on (a guitar from the spine,
/// sticks from the hands).
#[derive(Debug, Clone, Copy)]
pub struct FigureHandles {
    /// The root (position, yaw, height).
    pub root: Entity,
    /// The torso joint.
    pub spine: Entity,
    /// The head joint.
    pub head: Entity,
    /// The entities that carry the hands (left, right).
    pub hands: [Entity; 2],
}

// ---- proportions (unit height) -------------------------------------------

/// Height of the hips pivot when standing.
pub const HIPS_Y: f32 = 0.530;
/// Height of the hips pivot when seated.
pub const SEATED_HIPS_Y: f32 = 0.300;
/// Spine joint above the hips.
const SPINE_UP: f32 = 0.090;
/// Head joint (neck base) above the spine joint.
const HEAD_UP: f32 = 0.245;
/// Shoulder joints above the spine joint.
const SHOULDER_UP: f32 = 0.195;
/// Half the shoulder span, before the build factor.
const SHOULDER_HALF: f32 = 0.115;
/// Hip joints below the hips pivot.
const HIP_DOWN: f32 = 0.020;
/// Half the hip span, before the build factor.
const HIP_HALF: f32 = 0.085;
/// Upper arm length.
pub const UPPER_ARM: f32 = 0.170;
/// Forearm length.
pub const FOREARM: f32 = 0.150;
/// Thigh length.
pub const THIGH: f32 = 0.245;
/// Shin length, knee to sole (ankle plus foot height).
pub const SHIN: f32 = 0.265;
/// The head's top above the neck base.
pub const HEAD_TOP: f32 = 0.163;
/// Where the hand sits below the elbow.
const HAND_DOWN: f32 = 0.185;

/// The parent joint, or `None` for the hips (child of the root).
#[must_use]
pub fn parent_of(joint: Joint) -> Option<Joint> {
    match joint {
        Joint::Hips => None,
        Joint::Spine | Joint::Thigh(_) | Joint::Leg(_) => Some(Joint::Hips),
        Joint::Head | Joint::UpperArm(_) | Joint::Arm(_) => Some(Joint::Spine),
        Joint::Forearm(side) => Some(Joint::UpperArm(side)),
        Joint::Shin(side) => Some(Joint::Thigh(side)),
    }
}

/// A joint's local transform at rest, relative to its parent. Pure —
/// tested through the forward kinematics.
#[must_use]
pub fn joint_rest(joint: Joint, build: Build, stance: Stance) -> Transform {
    let b = build.factor();
    match joint {
        Joint::Hips => Transform::from_xyz(
            0.0,
            match stance {
                Stance::Standing => HIPS_Y,
                Stance::Seated => SEATED_HIPS_Y,
            },
            0.0,
        ),
        Joint::Spine => Transform::from_xyz(0.0, SPINE_UP, 0.0),
        Joint::Head => Transform::from_xyz(0.0, HEAD_UP, 0.01),
        Joint::UpperArm(side) | Joint::Arm(side) => {
            Transform::from_xyz(side.sign() * SHOULDER_HALF * b, SHOULDER_UP, 0.0)
                .with_rotation(Quat::from_rotation_z(side.sign() * 0.12))
        }
        Joint::Forearm(_) => {
            Transform::from_xyz(0.0, -UPPER_ARM, 0.0).with_rotation(Quat::from_rotation_x(-0.15))
        }
        Joint::Thigh(side) | Joint::Leg(side) => {
            let seated = matches!(stance, Stance::Seated);
            Transform::from_xyz(side.sign() * HIP_HALF * b, -HIP_DOWN, 0.0).with_rotation(
                if seated && matches!(joint, Joint::Thigh(_)) {
                    Quat::from_rotation_x(-1.50)
                } else {
                    Quat::IDENTITY
                },
            )
        }
        Joint::Shin(_) => Transform::from_xyz(0.0, -THIGH, 0.0).with_rotation(match stance {
            Stance::Standing => Quat::IDENTITY,
            Stance::Seated => Quat::from_rotation_x(1.50),
        }),
    }
}

// ---- pose ----------------------------------------------------------------

/// One arm's angles, in radians.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArmPose {
    /// Forward/up from hanging (0 = hanging, π/2 = forward, ~π = up).
    pub raise: f32,
    /// Outward from the body.
    pub spread: f32,
    /// Elbow bend (0 = straight).
    pub elbow: f32,
}

/// A whole body's pose: joint angles in radians, offsets in units of
/// the figure's height. `Default` is the rest pose.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    /// Hips raised off the ground (a jump).
    pub lift: f32,
    /// Knee bend: the hips drop, the feet stay planted.
    pub squash: f32,
    /// Pelvis roll about the forward axis.
    pub hips_roll: f32,
    /// Torso forward lean.
    pub lean: f32,
    /// Torso twist about the vertical.
    pub twist: f32,
    /// Torso sideways lean.
    pub side: f32,
    /// Head nod (chin forward).
    pub head_nod: f32,
    /// Head tilt sideways.
    pub head_tilt: f32,
    /// Arms, left then right.
    pub arms: [ArmPose; 2],
}

/// Joint limits, in radians and height units.
pub mod limits {
    /// Knee bend.
    pub const SQUASH: f32 = 0.35;
    /// Jump height.
    pub const LIFT: f32 = 0.22;
    /// Pelvis roll.
    pub const HIPS_ROLL: f32 = 0.105;
    /// Torso lean (back, forward).
    pub const LEAN: (f32, f32) = (-0.14, 0.32);
    /// Torso twist.
    pub const TWIST: f32 = 0.35;
    /// Torso side lean.
    pub const SIDE: f32 = 0.21;
    /// Head nod (back, forward).
    pub const HEAD_NOD: (f32, f32) = (-0.26, 0.61);
    /// Head tilt.
    pub const HEAD_TILT: f32 = 0.21;
    /// Arm raise (back, up).
    pub const RAISE: (f32, f32) = (-0.35, 3.05);
    /// Arm spread (in, out).
    pub const SPREAD: (f32, f32) = (-0.26, 0.79);
    /// Elbow bend.
    pub const ELBOW: f32 = 2.36;
}

impl Pose {
    /// Linear blend toward `other` by `t` (0 = self, 1 = other).
    #[must_use]
    pub fn lerp(&self, other: &Pose, t: f32) -> Pose {
        let t = t.clamp(0.0, 1.0);
        let l = |a: f32, b: f32| a + (b - a) * t;
        let arm = |a: ArmPose, b: ArmPose| ArmPose {
            raise: l(a.raise, b.raise),
            spread: l(a.spread, b.spread),
            elbow: l(a.elbow, b.elbow),
        };
        Pose {
            lift: l(self.lift, other.lift),
            squash: l(self.squash, other.squash),
            hips_roll: l(self.hips_roll, other.hips_roll),
            lean: l(self.lean, other.lean),
            twist: l(self.twist, other.twist),
            side: l(self.side, other.side),
            head_nod: l(self.head_nod, other.head_nod),
            head_tilt: l(self.head_tilt, other.head_tilt),
            arms: [
                arm(self.arms[0], other.arms[0]),
                arm(self.arms[1], other.arms[1]),
            ],
        }
    }

    /// Whether every angle and offset sits inside [`limits`].
    #[must_use]
    pub fn within_limits(&self) -> bool {
        let inside = |v: f32, lo: f32, hi: f32| v.is_finite() && v >= lo - 1e-6 && v <= hi + 1e-6;
        inside(self.squash, 0.0, limits::SQUASH)
            && inside(self.lift, 0.0, limits::LIFT)
            && inside(self.hips_roll, -limits::HIPS_ROLL, limits::HIPS_ROLL)
            && inside(self.lean, limits::LEAN.0, limits::LEAN.1)
            && inside(self.twist, -limits::TWIST, limits::TWIST)
            && inside(self.side, -limits::SIDE, limits::SIDE)
            && inside(self.head_nod, limits::HEAD_NOD.0, limits::HEAD_NOD.1)
            && inside(self.head_tilt, -limits::HEAD_TILT, limits::HEAD_TILT)
            && self.arms.iter().all(|arm| {
                inside(arm.raise, limits::RAISE.0, limits::RAISE.1)
                    && inside(arm.spread, limits::SPREAD.0, limits::SPREAD.1)
                    && inside(arm.elbow, 0.0, limits::ELBOW)
            })
    }

    /// The thigh's forward angle for a squash.
    #[must_use]
    pub fn thigh_angle(&self) -> f32 {
        0.9 * self.squash
    }

    /// The shin's angle relative to the thigh for a squash: back by
    /// the thigh's forward angle plus its own, chosen so the foot
    /// stays exactly under the hip (the shin is a little longer than
    /// the thigh, so the fold is not symmetric).
    #[must_use]
    pub fn shin_angle(&self) -> f32 {
        let thigh = self.thigh_angle();
        -(thigh + shin_back_angle(thigh))
    }
}

/// The shin's backward angle that puts the sole under the hip when
/// the thigh is `thigh` forward: `SHIN · sin(s) = THIGH · sin(t)`.
fn shin_back_angle(thigh: f32) -> f32 {
    ((THIGH / SHIN) * thigh.sin()).clamp(-1.0, 1.0).asin()
}

/// How far the hips drop for a squash, with the feet planted: exact
/// kinematics of the fold, not a guess. Pure — tested (the sole stays
/// at zero, under the hip).
#[must_use]
pub fn squash_drop(squash: f32) -> f32 {
    let thigh = 0.9 * squash;
    let shin = shin_back_angle(thigh);
    THIGH * (1.0 - thigh.cos()) + SHIN * (1.0 - shin.cos())
}

/// A joint's local transform under a pose: the rest transform with
/// the pose's rotation applied in the joint's own frame, plus the
/// hips' vertical offset. Joints the pose does not drive return their
/// rest. Pure.
#[must_use]
pub fn pose_transform(pose: &Pose, joint: Joint, rest: &Transform, bendable: bool) -> Transform {
    let (offset, delta) = match joint {
        Joint::Hips => {
            let drop = if bendable {
                squash_drop(pose.squash)
            } else {
                0.0
            };
            (
                Vec3::new(0.0, pose.lift - drop, 0.0),
                Quat::from_rotation_z(pose.hips_roll),
            )
        }
        Joint::Spine => (
            Vec3::ZERO,
            Quat::from_rotation_x(pose.lean)
                * Quat::from_rotation_y(pose.twist)
                * Quat::from_rotation_z(pose.side),
        ),
        Joint::Head => (
            Vec3::ZERO,
            Quat::from_rotation_x(pose.head_nod) * Quat::from_rotation_z(pose.head_tilt),
        ),
        Joint::UpperArm(side) | Joint::Arm(side) => {
            let arm = pose.arms[side.index()];
            (
                Vec3::ZERO,
                Quat::from_rotation_x(-arm.raise) * Quat::from_rotation_z(side.sign() * arm.spread),
            )
        }
        Joint::Forearm(side) => (
            Vec3::ZERO,
            Quat::from_rotation_x(-pose.arms[side.index()].elbow),
        ),
        Joint::Thigh(_) => (
            Vec3::ZERO,
            if bendable {
                Quat::from_rotation_x(-pose.thigh_angle())
            } else {
                Quat::IDENTITY
            },
        ),
        Joint::Shin(_) => (
            Vec3::ZERO,
            if bendable {
                Quat::from_rotation_x(-pose.shin_angle())
            } else {
                Quat::IDENTITY
            },
        ),
        Joint::Leg(_) => (Vec3::ZERO, Quat::IDENTITY),
    };
    Transform {
        translation: rest.translation + offset,
        rotation: rest.rotation * delta,
        scale: Vec3::ONE,
    }
}

/// Forward kinematics: a joint's transform in the figure's own frame
/// (unit height, root at the origin) under a pose. Pure — the tests'
/// ruler.
#[must_use]
pub fn joint_transform(pose: &Pose, build: Build, stance: Stance, joint: Joint) -> Transform {
    let bendable = !matches!(joint, Joint::Arm(_) | Joint::Leg(_));
    let local = pose_transform(pose, joint, &joint_rest(joint, build, stance), bendable);
    match parent_of(joint) {
        Some(parent) => joint_transform(pose, build, stance, parent).mul_transform(local),
        None => local,
    }
}

/// Where a foot's sole is, in the figure's frame.
#[must_use]
pub fn sole_position(
    pose: &Pose,
    build: Build,
    stance: Stance,
    side: Side,
    detail: Detail,
) -> Vec3 {
    match detail {
        Detail::Full => joint_transform(pose, build, stance, Joint::Shin(side))
            .transform_point(Vec3::new(0.0, -SHIN, 0.0)),
        Detail::Simple => joint_transform(pose, build, stance, Joint::Leg(side))
            .transform_point(Vec3::new(0.0, -(THIGH + SHIN), 0.0)),
    }
}

/// Where the top of the head is, in the figure's frame.
#[must_use]
pub fn head_top(pose: &Pose, build: Build, stance: Stance) -> Vec3 {
    joint_transform(pose, build, stance, Joint::Head).transform_point(Vec3::new(0.0, HEAD_TOP, 0.0))
}

/// Where a hand is, in the figure's frame.
#[must_use]
pub fn hand_position(
    pose: &Pose,
    build: Build,
    stance: Stance,
    side: Side,
    detail: Detail,
) -> Vec3 {
    match detail {
        Detail::Full => joint_transform(pose, build, stance, Joint::Forearm(side))
            .transform_point(Vec3::new(0.0, -HAND_DOWN, 0.005)),
        Detail::Simple => joint_transform(pose, build, stance, Joint::Arm(side))
            .transform_point(Vec3::new(0.0, -(UPPER_ARM + FOREARM + 0.04), 0.0)),
    }
}

// ---- assets --------------------------------------------------------------

/// Body parts with a mesh of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Pelvis,
    Torso,
    Head,
    UpperArm,
    Forearm,
    Thigh,
    Shin,
    SimpleArm,
    SimpleLeg,
}

const PARTS: [Part; 9] = [
    Part::Pelvis,
    Part::Torso,
    Part::Head,
    Part::UpperArm,
    Part::Forearm,
    Part::Thigh,
    Part::Shin,
    Part::SimpleArm,
    Part::SimpleLeg,
];

/// The shared meshes and materials every figure is built from.
#[derive(Resource, Clone)]
pub struct FigureAssets {
    /// `[build][part]`.
    parts: Vec<Vec<Handle<Mesh>>>,
    /// Per hair variant (`None` for bare).
    hair: Vec<Option<Handle<Mesh>>>,
    /// Shirt tones.
    pub tops: Vec<Handle<StandardMaterial>>,
    /// Trouser tones.
    pub bottoms: Vec<Handle<StandardMaterial>>,
    /// Skin tones.
    pub skins: Vec<Handle<StandardMaterial>>,
    /// Hair tones.
    pub hairs: Vec<Handle<StandardMaterial>>,
}

/// Merge extra pieces into a base mesh. A merge only fails when the
/// attribute sets differ, which the primitives here never do; if it
/// ever does, the piece is dropped and the log says which.
fn assemble(mut base: Mesh, pieces: &[Mesh]) -> Mesh {
    for piece in pieces {
        if let Err(error) = base.merge(piece) {
            warn!("figure: a body part could not be merged ({error:?})");
        }
    }
    base
}

fn frustum(top: f32, bottom: f32, height: f32, resolution: u32) -> Mesh {
    ConicalFrustum {
        radius_top: top,
        radius_bottom: bottom,
        height,
    }
    .mesh()
    .resolution(resolution)
    .build()
}

fn capsule(radius: f32, length: f32, latitudes: u32, longitudes: u32) -> Mesh {
    Capsule3d::new(radius, length)
        .mesh()
        .latitudes(latitudes)
        .longitudes(longitudes)
        .build()
}

fn sphere(radius: f32, sectors: u32, stacks: u32) -> Mesh {
    Sphere::new(radius).mesh().uv(sectors, stacks)
}

/// A part's mesh at its joint's origin, limbs hanging along −Y.
fn part_mesh(part: Part, build: Build) -> Mesh {
    let b = build.factor();
    match part {
        Part::Pelvis => {
            frustum(0.080 * b, 0.088 * b, 0.16, 12).scaled_by(Vec3::new(1.0, 1.0, 0.72))
        }
        Part::Torso => assemble(
            frustum(0.098 * b, 0.078 * b, 0.14, 12).translated_by(Vec3::new(0.0, 0.03, 0.0)),
            &[capsule(0.105 * b, 0.05, 8, 12)
                .scaled_by(Vec3::new(1.0, 0.95, 0.70))
                .translated_by(Vec3::new(0.0, 0.12, 0.0))],
        ),
        Part::Head => assemble(
            sphere(0.068, 14, 10)
                .scaled_by(Vec3::new(1.0, 1.15, 1.06))
                .translated_by(Vec3::new(0.0, 0.085, 0.005)),
            &[Cylinder::new(0.025, 0.06)
                .mesh()
                .resolution(10)
                .build()
                .translated_by(Vec3::new(0.0, 0.02, 0.0))],
        ),
        Part::UpperArm => assemble(
            capsule(0.030 * b, UPPER_ARM, 6, 10).translated_by(Vec3::new(
                0.0,
                -UPPER_ARM * 0.5,
                0.0,
            )),
            &[sphere(0.036 * b, 8, 6)],
        ),
        Part::Forearm => assemble(
            frustum(0.026 * b, 0.020 * b, FOREARM, 8).translated_by(Vec3::new(
                0.0,
                -FOREARM * 0.5,
                0.0,
            )),
            &[sphere(0.036, 8, 6)
                .scaled_by(Vec3::new(0.75, 1.15, 0.45))
                .translated_by(Vec3::new(0.0, -HAND_DOWN, 0.005))],
        ),
        Part::Thigh => frustum(0.056 * b, 0.043 * b, THIGH, 10).translated_by(Vec3::new(
            0.0,
            -THIGH * 0.5,
            0.0,
        )),
        Part::Shin => assemble(
            frustum(0.040 * b, 0.028 * b, 0.22, 8).translated_by(Vec3::new(0.0, -0.11, 0.0)),
            &[
                Mesh::from(Cuboid::new(0.07, 0.045, 0.145)).translated_by(Vec3::new(
                    0.0,
                    -(SHIN - 0.0225),
                    0.04,
                )),
            ],
        ),
        Part::SimpleArm => assemble(
            capsule(0.028 * b, UPPER_ARM + FOREARM, 6, 8).translated_by(Vec3::new(
                0.0,
                -(UPPER_ARM + FOREARM) * 0.5,
                0.0,
            )),
            &[sphere(0.034, 6, 5).translated_by(Vec3::new(
                0.0,
                -(UPPER_ARM + FOREARM + 0.04),
                0.0,
            ))],
        ),
        Part::SimpleLeg => assemble(
            frustum(0.055 * b, 0.030 * b, THIGH + SHIN - 0.025, 8).translated_by(Vec3::new(
                0.0,
                -(THIGH + SHIN - 0.025) * 0.5,
                0.0,
            )),
            &[
                Mesh::from(Cuboid::new(0.07, 0.045, 0.145)).translated_by(Vec3::new(
                    0.0,
                    -(THIGH + SHIN - 0.0225),
                    0.04,
                )),
            ],
        ),
    }
}

/// A hair silhouette at the head joint, or `None` for bare.
fn hair_mesh(hair: Hair) -> Option<Mesh> {
    let cap = || {
        sphere(0.071, 12, 8)
            .scaled_by(Vec3::new(1.02, 0.92, 1.02))
            .translated_by(Vec3::new(0.0, 0.099, -0.012))
    };
    Some(match hair {
        Hair::Bare => return None,
        Hair::Short => cap(),
        Hair::Cap => assemble(
            sphere(0.072, 12, 8)
                .scaled_by(Vec3::new(1.0, 0.62, 1.0))
                .translated_by(Vec3::new(0.0, 0.115, 0.0)),
            &[Mesh::from(Cuboid::new(0.09, 0.012, 0.07))
                .translated_by(Vec3::new(0.0, 0.10, 0.075))],
        ),
        Hair::Ponytail => assemble(
            cap(),
            &[capsule(0.022, 0.13, 6, 8)
                .rotated_by(Quat::from_rotation_x(-0.6))
                .translated_by(Vec3::new(0.0, 0.06, -0.08))],
        ),
        Hair::Long => assemble(
            cap(),
            &[
                capsule(0.03, 0.20, 6, 8).translated_by(Vec3::new(0.05, 0.0, -0.05)),
                capsule(0.03, 0.20, 6, 8).translated_by(Vec3::new(-0.05, 0.0, -0.05)),
            ],
        ),
        Hair::Beanie => sphere(0.074, 12, 8)
            .scaled_by(Vec3::new(1.0, 0.85, 1.0))
            .translated_by(Vec3::new(0.0, 0.11, -0.005)),
    })
}

fn matte(color: Color, roughness: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        ..default()
    }
}

impl FigureAssets {
    /// Build every shared mesh and material once. Shirt tones lean on
    /// the theme's background so a crowd belongs to its venue; skin,
    /// hair and trousers are the same in every theme.
    #[must_use]
    pub fn build(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        theme: Theme,
    ) -> FigureAssets {
        let parts = Build::ALL
            .iter()
            .map(|&build| {
                PARTS
                    .iter()
                    .map(|&part| meshes.add(part_mesh(part, build)))
                    .collect()
            })
            .collect();
        let hair = Hair::ALL
            .iter()
            .map(|&hair| hair_mesh(hair).map(|mesh| meshes.add(mesh)))
            .collect();
        let dark = |amount: f32| theme.background.mix(&Color::BLACK, amount);
        let tops = vec![
            dark(0.62),
            dark(0.56).mix(&Color::srgb(0.40, 0.20, 0.10), 0.15),
            dark(0.50).mix(&Color::srgb(0.10, 0.15, 0.35), 0.20),
            dark(0.60),
            dark(0.54).mix(&theme.accent, 0.08),
            Color::srgb(0.16, 0.19, 0.27),
            Color::srgb(0.26, 0.10, 0.12),
            Color::srgb(0.62, 0.60, 0.58),
        ];
        let bottoms = [
            Color::srgb(0.04, 0.04, 0.045),
            Color::srgb(0.10, 0.12, 0.18),
            Color::srgb(0.12, 0.12, 0.12),
            Color::srgb(0.14, 0.10, 0.07),
        ];
        let skins = [
            Color::srgb(0.86, 0.68, 0.56),
            Color::srgb(0.72, 0.52, 0.40),
            Color::srgb(0.55, 0.38, 0.28),
            Color::srgb(0.36, 0.25, 0.18),
        ];
        let hairs = [
            Color::srgb(0.06, 0.05, 0.05),
            Color::srgb(0.22, 0.13, 0.08),
            Color::srgb(0.40, 0.16, 0.08),
            Color::srgb(0.50, 0.38, 0.20),
        ];
        FigureAssets {
            parts,
            hair,
            tops: tops
                .into_iter()
                .map(|c| materials.add(matte(c, 0.9)))
                .collect(),
            bottoms: bottoms
                .into_iter()
                .map(|c| materials.add(matte(c, 0.85)))
                .collect(),
            skins: skins
                .into_iter()
                .map(|c| materials.add(matte(c, 0.7)))
                .collect(),
            hairs: hairs
                .into_iter()
                .map(|c| materials.add(matte(c, 0.8)))
                .collect(),
        }
    }

    fn part(&self, part: Part, build: Build) -> Handle<Mesh> {
        self.parts[build.index()][PARTS.iter().position(|&p| p == part).unwrap_or(0)].clone()
    }

    fn top(&self, spec: &FigureSpec) -> Handle<StandardMaterial> {
        self.tops[usize::from(spec.top) % self.tops.len()].clone()
    }

    fn bottom(&self, spec: &FigureSpec) -> Handle<StandardMaterial> {
        self.bottoms[usize::from(spec.bottom) % self.bottoms.len()].clone()
    }

    fn skin(&self, spec: &FigureSpec) -> Handle<StandardMaterial> {
        self.skins[usize::from(spec.skin) % self.skins.len()].clone()
    }

    fn hair_material(&self, spec: &FigureSpec) -> Handle<StandardMaterial> {
        match spec.hair {
            Hair::Cap | Hair::Beanie => self.bottom(spec),
            _ => self.hairs[usize::from(spec.hair_tone) % self.hairs.len()].clone(),
        }
    }
}

// ---- spawning ------------------------------------------------------------

/// Spawn one figure under `root` (position, yaw, and scale = height;
/// the caller sets the scale from the spec). `extra` goes on the root
/// (the screen marker, the stage marker, the caller's own component);
/// `joint_marker` goes on every joint so a caller's animation query
/// can be disjoint from another's. Every entity carries the stage
/// render layer; only the root carries the caller's bundle, and
/// despawning the root takes the whole person.
pub fn spawn_figure<M: Bundle + Clone>(
    commands: &mut Commands,
    assets: &FigureAssets,
    spec: &FigureSpec,
    stance: Stance,
    root: Transform,
    extra: impl Bundle,
    joint_marker: M,
) -> FigureHandles {
    let layer = RenderLayers::layer(STAGE_LAYER);
    let root_id = commands
        .spawn((
            FigureRoot { spec: *spec },
            root,
            Visibility::default(),
            layer.clone(),
            extra,
        ))
        .id();
    let bendable = matches!(spec.detail, Detail::Full);
    let build = spec.build;
    let spawn_joint = |commands: &mut Commands,
                       parent: Entity,
                       joint: Joint,
                       mesh: Handle<Mesh>,
                       material: Handle<StandardMaterial>|
     -> Entity {
        let rest = joint_rest(joint, build, stance);
        commands
            .spawn((
                FigureJoint {
                    owner: root_id,
                    joint,
                    rest,
                    bendable,
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                rest,
                Visibility::default(),
                layer.clone(),
                ChildOf(parent),
                joint_marker.clone(),
            ))
            .id()
    };

    let hips = spawn_joint(
        commands,
        root_id,
        Joint::Hips,
        assets.part(Part::Pelvis, build),
        assets.bottom(spec),
    );
    let spine = spawn_joint(
        commands,
        hips,
        Joint::Spine,
        assets.part(Part::Torso, build),
        assets.top(spec),
    );
    let head = spawn_joint(
        commands,
        spine,
        Joint::Head,
        assets.part(Part::Head, build),
        assets.skin(spec),
    );
    if let Some(hair) = assets.hair[spec.hair.index()].clone() {
        commands.spawn((
            Mesh3d(hair),
            MeshMaterial3d(assets.hair_material(spec)),
            Transform::IDENTITY,
            Visibility::default(),
            layer.clone(),
            ChildOf(head),
        ));
    }
    let sleeve = if spec.sleeves {
        assets.top(spec)
    } else {
        assets.skin(spec)
    };
    let mut hands = [root_id; 2];
    for side in [Side::Left, Side::Right] {
        match spec.detail {
            Detail::Full => {
                let upper = spawn_joint(
                    commands,
                    spine,
                    Joint::UpperArm(side),
                    assets.part(Part::UpperArm, build),
                    sleeve.clone(),
                );
                hands[side.index()] = spawn_joint(
                    commands,
                    upper,
                    Joint::Forearm(side),
                    assets.part(Part::Forearm, build),
                    assets.skin(spec),
                );
                let thigh = spawn_joint(
                    commands,
                    hips,
                    Joint::Thigh(side),
                    assets.part(Part::Thigh, build),
                    assets.bottom(spec),
                );
                spawn_joint(
                    commands,
                    thigh,
                    Joint::Shin(side),
                    assets.part(Part::Shin, build),
                    assets.bottom(spec),
                );
            }
            Detail::Simple => {
                hands[side.index()] = spawn_joint(
                    commands,
                    spine,
                    Joint::Arm(side),
                    assets.part(Part::SimpleArm, build),
                    sleeve.clone(),
                );
                spawn_joint(
                    commands,
                    hips,
                    Joint::Leg(side),
                    assets.part(Part::SimpleLeg, build),
                    assets.bottom(spec),
                );
            }
        }
    }
    FigureHandles {
        root: root_id,
        spine,
        head,
        hands,
    }
}

/// A root transform for a figure standing at `position`, turned by
/// `yaw` (0 = facing +Z), scaled to the spec's height.
#[must_use]
pub fn stand_at(position: Vec3, yaw: f32, spec: &FigureSpec) -> Transform {
    Transform::from_translation(position)
        .with_rotation(Quat::from_rotation_y(yaw))
        .with_scale(Vec3::splat(spec.height))
}

/// A figure faces +Z at yaw 0, which is toward the camera (the
/// camera sits at +Z looking down −Z).
pub const FACE_CAMERA: f32 = 0.0;

/// Facing the stage (−Z, toward the band) is a half turn.
pub const FACE_STAGE: f32 = PI;

/// Bake the shared figure assets once per gameplay entry, before the
/// crowd and the band spawn. Runs on the stage's own chain; the theme
/// is the one the song chose.
pub fn setup_figure_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<crate::theme::ActiveTheme>,
) {
    commands.insert_resource(FigureAssets::build(&mut meshes, &mut materials, theme.0));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_PI_2;

    #[test]
    fn a_figure_spec_is_the_same_for_the_same_seed_and_varied_across_seeds() {
        assert_eq!(
            FigureSpec::from_hash(42, Detail::Full),
            FigureSpec::from_hash(42, Detail::Full)
        );
        let specs: Vec<FigureSpec> = (0..64)
            .map(|s| FigureSpec::from_hash(s, Detail::Full))
            .collect();
        let builds: std::collections::HashSet<_> = specs.iter().map(|s| s.build).collect();
        let hairs: std::collections::HashSet<_> = specs.iter().map(|s| s.hair).collect();
        let tops: std::collections::HashSet<_> = specs.iter().map(|s| s.top).collect();
        assert!(builds.len() == 3, "all three builds appear: {builds:?}");
        assert!(hairs.len() >= 5, "hair varies: {hairs:?}");
        assert!(tops.len() >= 6, "shirts vary: {tops:?}");
        assert!(
            specs.iter().any(|s| s.top == TOPS - 1),
            "a pale shirt appears"
        );
    }

    #[test]
    fn every_figure_stands_between_the_shortest_and_the_tallest() {
        for seed in 0..500 {
            let spec = FigureSpec::from_hash(seed, Detail::Simple);
            assert!(
                spec.height >= MIN_HEIGHT && spec.height <= MAX_HEIGHT,
                "{spec:?}"
            );
            assert!(
                spec.top < TOPS
                    && spec.bottom < BOTTOMS
                    && spec.skin < SKINS
                    && spec.hair_tone < HAIRS
            );
        }
    }

    #[test]
    fn the_rest_pose_puts_the_soles_on_the_ground_and_the_head_at_unit_height() {
        let rest = Pose::default();
        for build in Build::ALL {
            for detail in [Detail::Full, Detail::Simple] {
                for side in [Side::Left, Side::Right] {
                    let sole = sole_position(&rest, build, Stance::Standing, side, detail);
                    assert!(
                        sole.y.abs() < 0.005,
                        "{build:?} {detail:?} {side:?} sole {sole}"
                    );
                }
            }
            let top = head_top(&rest, build, Stance::Standing);
            assert!((0.98..=1.05).contains(&top.y), "{build:?} head top {top}");
            let left = joint_transform(&rest, build, Stance::Standing, Joint::UpperArm(Side::Left))
                .translation;
            let right =
                joint_transform(&rest, build, Stance::Standing, Joint::UpperArm(Side::Right))
                    .translation;
            let span = left.x - right.x;
            assert!(
                (0.20..=0.27).contains(&span),
                "{build:?} shoulder span {span}"
            );
        }
    }

    #[test]
    fn a_squash_keeps_the_feet_planted_and_drops_the_hips() {
        let mut last_hips = f32::INFINITY;
        for squash in [0.0, 0.1, 0.25, 0.35] {
            let pose = Pose {
                squash,
                ..Pose::default()
            };
            for side in [Side::Left, Side::Right] {
                let sole =
                    sole_position(&pose, Build::Medium, Stance::Standing, side, Detail::Full);
                assert!(sole.y.abs() < 1e-3, "squash {squash}: sole {sole}");
                assert!(
                    sole.z.abs() < 1e-3,
                    "squash {squash}: the foot stays under the hip: {sole}"
                );
            }
            let hips = joint_transform(&pose, Build::Medium, Stance::Standing, Joint::Hips)
                .translation
                .y;
            assert!(
                hips < last_hips,
                "hips drop monotonically: {hips} after {last_hips}"
            );
            last_hips = hips;
        }
        assert!(HIPS_Y - last_hips > 0.02, "a full squash is visible");
    }

    #[test]
    fn a_raised_arm_goes_forward_and_a_spread_arm_goes_outward() {
        let rest = Pose::default();
        let at_rest = hand_position(
            &rest,
            Build::Medium,
            Stance::Standing,
            Side::Left,
            Detail::Full,
        );
        let mut raised = Pose::default();
        raised.arms[0].raise = FRAC_PI_2;
        let forward = hand_position(
            &raised,
            Build::Medium,
            Stance::Standing,
            Side::Left,
            Detail::Full,
        );
        assert!(
            forward.z > at_rest.z + 0.2,
            "raised hand is forward: {forward} vs {at_rest}"
        );
        assert!(forward.y > at_rest.y + 0.2, "and higher");
        let mut up = Pose::default();
        up.arms[1].raise = 2.9;
        let overhead = hand_position(
            &up,
            Build::Medium,
            Stance::Standing,
            Side::Right,
            Detail::Full,
        );
        assert!(
            overhead.y > 1.0,
            "an arm at 2.9 rad is overhead: {overhead}"
        );
        let mut spread = Pose::default();
        spread.arms[0].spread = 0.5;
        spread.arms[1].spread = 0.5;
        let left = hand_position(
            &spread,
            Build::Medium,
            Stance::Standing,
            Side::Left,
            Detail::Full,
        );
        let right = hand_position(
            &spread,
            Build::Medium,
            Stance::Standing,
            Side::Right,
            Detail::Full,
        );
        assert!(
            left.x > at_rest.x + 0.05,
            "left hand spreads left (+x): {left}"
        );
        assert!(
            right.x < -at_rest.x - 0.05,
            "right hand spreads right (-x): {right}"
        );
        // The simple build follows the same conventions.
        let simple = hand_position(
            &raised,
            Build::Medium,
            Stance::Standing,
            Side::Left,
            Detail::Simple,
        );
        assert!(simple.z > 0.2 && simple.y > 0.5, "{simple}");
    }

    #[test]
    fn a_seated_stance_lowers_the_hips_and_keeps_the_soles_down() {
        let rest = Pose::default();
        let seated = joint_transform(&rest, Build::Medium, Stance::Seated, Joint::Hips)
            .translation
            .y;
        let standing = joint_transform(&rest, Build::Medium, Stance::Standing, Joint::Hips)
            .translation
            .y;
        assert!(
            seated < standing - 0.2,
            "seated {seated} vs standing {standing}"
        );
        for side in [Side::Left, Side::Right] {
            let sole = sole_position(&rest, Build::Medium, Stance::Seated, side, Detail::Full);
            assert!(sole.y.abs() < 0.02, "seated sole {sole}");
            assert!(
                sole.z > 0.15,
                "seated feet are forward of the stool: {sole}"
            );
        }
    }

    #[test]
    fn a_pose_lerp_lands_on_its_ends_and_limits_hold_the_rest() {
        let a = Pose::default();
        let mut b = Pose {
            squash: 0.3,
            ..Pose::default()
        };
        b.arms[0].raise = 2.0;
        assert_eq!(a.lerp(&b, 0.0), a);
        assert_eq!(a.lerp(&b, 1.0), b);
        let mid = a.lerp(&b, 0.5);
        assert!((mid.squash - 0.15).abs() < 1e-6 && (mid.arms[0].raise - 1.0).abs() < 1e-6);
        assert!(a.within_limits() && b.within_limits());
        b.squash = 0.5;
        assert!(!b.within_limits(), "a squash past the knee limit is out");
    }
}
