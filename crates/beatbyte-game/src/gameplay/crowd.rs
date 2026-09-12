//! The crowd: people who dance to the song.
//!
//! Fifty-six figures from the [`figure`] builder stand
//! in three staggered rows either side of the neck, behind the
//! barriers and in front of the band's riser — never inside the bed
//! (the G23 rule: nothing on stage may sit over a note), never inside
//! the riser (the old crowd's back seats stood buried in it). Each
//! person has a hash-chosen look and a small programme of dance moves
//! that changes on phrase boundaries; every move is a pure function of
//! the song's beat, the bar and an energy term, so the crowd is on the
//! beat the player plays to and stills to nothing when the song is
//! quiet — and, under STAGE MOTION off, writes not a single
//! transform.
//!
//! The vocabulary is the genre's (a crowd that pumps its arms and
//! jumps on the downbeat), drawn in our own hands: no likeness, no
//! costume, no logo.

use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use core::f32::consts::{PI, TAU};

use beatbyte_core::NoteEvent;

use super::figure::{
    self, ArmPose, Detail, FigureAssets, FigureJoint, FigureRoot, FigureSpec, Pose, Stance,
    pose_transform, spawn_figure, stand_at,
};
use super::fx::hash01;
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use super::{GameplayScreen, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;

/// People per side of the neck.
pub const CROWD_PER_SIDE: usize = 28;
/// Seats per row, front row first.
pub const ROWS: [usize; 3] = [10, 10, 8];
/// The nearest a person stands to the camera (behind the stacks at
/// z −7 and the barrier's near end).
pub const CROWD_NEAR_Z: f32 = -11.5;
/// The farthest — in front of the band riser's face at −26.
pub const CROWD_FAR_Z: f32 = -25.4;
/// The front row's distance from the neck's centre line.
pub const CROWD_INNER_X: f32 = 3.35;
/// Row spacing across.
pub const ROW_PITCH_X: f32 = 0.95;
/// The deck the crowd stands on (the highway riser's top).
pub const CROWD_FLOOR_Y: f32 = -0.30;
/// Where the barriers stand (their centre x).
pub const BARRIER_X: f32 = 2.9;

/// A dance move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Move {
    /// Knees dip on the beat.
    Bounce,
    /// Hips and shoulders sway over two beats.
    Sway,
    /// One fist pumps on the beat.
    Pump,
    /// Hands meet on the beat.
    Clap,
    /// The head bangs on the beat.
    Headbang,
    /// A jump on the downbeat, a bounce between.
    Jump,
    /// Both arms up — the hype pose, never in a programme.
    ArmsUp,
}

impl Move {
    /// The moves a programme draws from, with their weights.
    pub const REPERTOIRE: [(Move, f32); 6] = [
        (Move::Bounce, 0.35),
        (Move::Sway, 0.20),
        (Move::Pump, 0.15),
        (Move::Clap, 0.10),
        (Move::Headbang, 0.10),
        (Move::Jump, 0.10),
    ];

    /// Pick a move from the repertoire by a 0..1 draw.
    #[must_use]
    pub fn draw(r: f32) -> Move {
        let mut acc = 0.0;
        for (mv, weight) in Move::REPERTOIRE {
            acc += weight;
            if r < acc {
                return mv;
            }
        }
        Move::Bounce
    }
}

/// One person's dancing: which moves, how long each is held (in
/// phrases of four bars), and their own timing and vigour.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Dancer {
    /// The person's index in the crowd.
    pub index: u16,
    /// The programme, cycled.
    pub program: [Move; 4],
    /// Phrases each move is held for (2..=4).
    pub hold: u8,
    /// Phrase offset so the crowd does not switch as one.
    pub offset: u8,
    /// Small reaction offset around the beat, radians.
    pub phase: f32,
    /// Personal amplitude, 0.8..1.2.
    pub vigour: f32,
}

/// Marks a crowd figure's joints (keeps the crowd's and the band's
/// transform queries disjoint).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CrowdJoint;

/// The song's bar lines (downbeat times), for the moves that count
/// in bars.
#[derive(Resource, Debug, Clone, Default)]
pub struct CrowdBeat {
    /// Downbeat times, ascending.
    pub bars: Vec<f64>,
}

/// The crowd's shared mood: how far every arm is up (eased toward
/// Hype being active).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CrowdMood {
    /// 0 = the programme, 1 = everyone's arms up.
    pub arms_up: f32,
    /// Smoothed movement energy, so quiet/loud boundaries do not snap.
    pub energy: f32,
}

/// A stable seed from the song's title (the theme's own fold), so a
/// song always gets the same crowd and another song another one.
#[must_use]
pub fn song_seed(title: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash as usize
}

/// Where one person stands and who they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// −1 left of the neck, +1 right.
    pub side: f32,
    /// Row, front first.
    pub row: usize,
    /// Feet position.
    pub position: Vec3,
    /// Facing, radians (0 = toward the camera, π = toward the band).
    pub yaw: f32,
    /// The look.
    pub spec: FigureSpec,
    /// The dancing.
    pub dancer: Dancer,
}

/// The slot for crowd member `index` (0..2·[`CROWD_PER_SIDE`]) of the
/// song with `seed`. Pure — tested: every slot is outside the bed,
/// off the band riser, and the same on every run.
#[must_use]
pub fn slot(index: usize, seed: usize) -> Slot {
    let side = if index.is_multiple_of(2) { -1.0 } else { 1.0 };
    let k = index / 2;
    let (row, seat) = if k < ROWS[0] {
        (0, k)
    } else if k < ROWS[0] + ROWS[1] {
        (1, k - ROWS[0])
    } else {
        (2, k - ROWS[0] - ROWS[1])
    };
    let person = seed
        .wrapping_mul(7_919)
        .wrapping_add(index.wrapping_mul(97));
    let j = |salt: usize| hash01(person.wrapping_add(salt)) - 0.5;
    let h = |salt: usize| hash01(person.wrapping_add(salt));
    let per_row = ROWS[row] as f32;
    let pitch = (CROWD_NEAR_Z - CROWD_FAR_Z) / per_row;
    let stagger = [0.0, 0.5, 0.25][row];
    let z = (CROWD_NEAR_Z - (seat as f32 + 0.5 + stagger) * pitch + 0.30 * j(1))
        .clamp(CROWD_FAR_Z, CROWD_NEAR_Z - 0.5);
    let x = side * (CROWD_INNER_X + row as f32 * ROW_PITCH_X + 0.15 * j(2));
    let yaw = match h(4) {
        r if r < 0.18 => figure::FACE_STAGE + j(3).signum() * 1.05,
        r if r < 0.26 => figure::FACE_CAMERA + 0.6 * j(3),
        _ => figure::FACE_STAGE + 0.7 * j(3),
    };
    let detail = if row == 2 {
        Detail::Simple
    } else {
        Detail::Full
    };
    let spec = FigureSpec::from_hash(person, detail);
    let program = [
        Move::draw(h(20)),
        Move::draw(h(21)),
        Move::draw(h(22)),
        Move::draw(h(23)),
    ];
    Slot {
        side,
        row,
        position: Vec3::new(x, CROWD_FLOOR_Y, z),
        yaw,
        spec,
        dancer: Dancer {
            index: index as u16,
            program,
            hold: 2 + (h(24) * 3.0) as u8,
            offset: (h(25) * 3.0) as u8,
            // People react in loose clusters around the same beat. A full
            // random cycle made half the room rise while the other half fell,
            // which looked like procedural noise rather than a shared groove.
            phase: row as f32 * 0.12 + (seat % 3) as f32 * 0.06 + 0.40 * j(26),
            vigour: 0.8 + 0.4 * h(27),
        },
    }
}

// ---- the moves -----------------------------------------------------------

/// Half-cosine pulse peaking on the beat (1 on the beat, 0 between).
fn pulse(beats: f32, phase: f32) -> f32 {
    0.5 + 0.5 * (beats * TAU + phase).cos()
}

/// Fast up, slow down, once per beat (0..1).
fn strike(beats: f32, phase: f32) -> f32 {
    let t = (beats + phase / TAU).rem_euclid(1.0);
    if t < 0.2 {
        t / 0.2
    } else {
        1.0 - (t - 0.2) / 0.8
    }
}

fn arms(left: ArmPose, right: ArmPose) -> [ArmPose; 2] {
    [left, right]
}

/// A knee bounce at amplitude `a` (0..1 of the full move).
fn bounce(beats: f32, e: f32, phase: f32, a: f32) -> Pose {
    let p = pulse(beats, phase);
    Pose {
        squash: 0.16 * e * a * p,
        head_nod: 0.105 * e * a * p,
        arms: arms(
            ArmPose {
                elbow: 0.26 * e,
                ..ArmPose::default()
            },
            ArmPose {
                elbow: 0.26 * e,
                ..ArmPose::default()
            },
        ),
        ..Pose::default()
    }
}

/// The pose of one move at song position `beats`, bar phase `bar`
/// (0..1 from the downbeat), energy `e` (0..1) and a personal
/// `phase`. Every move is exactly the rest pose at `e = 0`, periodic
/// in four beats, and inside the joint limits at any energy. Pure —
/// tested.
#[must_use]
pub fn move_pose(mv: Move, beats: f32, bar: f32, e: f32, phase: f32) -> Pose {
    let e = e.clamp(0.0, 1.0);
    let pose = match mv {
        Move::Bounce => bounce(beats, e, phase, 1.0),
        Move::Sway => {
            let s = (beats * PI + phase).sin();
            Pose {
                hips_roll: 0.087 * e * s,
                side: -0.14 * e * s,
                head_tilt: 0.105 * e * s,
                squash: 0.05 * e,
                arms: arms(
                    ArmPose {
                        spread: 0.21 * e,
                        ..ArmPose::default()
                    },
                    ArmPose {
                        spread: 0.21 * e,
                        ..ArmPose::default()
                    },
                ),
                ..Pose::default()
            }
        }
        Move::Pump => {
            let mut pose = bounce(beats, e, phase, 0.4);
            pose.lean = -0.087 * e;
            pose.arms[1] = ArmPose {
                raise: e * (1.92 + 0.96 * strike(beats, phase)),
                spread: 0.14 * e,
                elbow: 1.40 * e,
            };
            pose
        }
        Move::Clap => {
            let spread = e * (0.44 - 0.52 * strike(beats, phase));
            Pose {
                head_nod: 0.087 * e,
                squash: 0.04 * e * pulse(beats, phase),
                arms: arms(
                    ArmPose {
                        raise: 1.22 * e,
                        spread,
                        elbow: 1.75 * e,
                    },
                    ArmPose {
                        raise: 1.22 * e,
                        spread,
                        elbow: 1.75 * e,
                    },
                ),
                ..Pose::default()
            }
        }
        Move::Headbang => {
            let h = (beats * TAU + phase).sin().max(0.0);
            Pose {
                head_nod: 0.61 * e * h,
                lean: 0.21 * e * h,
                squash: 0.08 * e * h,
                arms: arms(
                    ArmPose {
                        elbow: 0.35 * e,
                        ..ArmPose::default()
                    },
                    ArmPose {
                        elbow: 0.35 * e,
                        ..ArmPose::default()
                    },
                ),
                ..Pose::default()
            }
        }
        Move::Jump => {
            let bar = bar.rem_euclid(1.0);
            if bar >= 0.90 {
                // Anticipation: the knees load before the downbeat.
                Pose {
                    squash: 0.30 * e * (bar - 0.90) / 0.10,
                    ..Pose::default()
                }
            } else if bar < 0.35 {
                // Flight: a parabola over a third of the bar.
                let t = bar / 0.35;
                let up = ArmPose {
                    raise: 1.66 * e,
                    spread: 0.17 * e,
                    elbow: 0.35 * e,
                };
                Pose {
                    lift: 0.22 * e * 4.0 * t * (1.0 - t),
                    arms: arms(up, up),
                    ..Pose::default()
                }
            } else if bar < 0.55 {
                // Landing: the knees take it.
                Pose {
                    squash: 0.25 * e * (1.0 - (bar - 0.35) / 0.20),
                    ..Pose::default()
                }
            } else {
                bounce(beats, e, phase, 0.5)
            }
        }
        Move::ArmsUp => {
            let mut pose = bounce(beats, e, phase, 0.6);
            let spread = e * (0.26 + 0.17 * (beats * PI * 0.5 + phase).sin());
            let up = ArmPose {
                raise: 2.88 * e,
                spread,
                elbow: 0.35 * e,
            };
            pose.arms = arms(up, up);
            pose
        }
    };
    organic_pose(pose, beats, e, phase)
}

/// Layer a quiet weight shift and left/right asymmetry under every authored
/// move. All waves repeat over four beats and vanish at zero energy.
fn organic_pose(mut pose: Pose, beats: f32, e: f32, phase: f32) -> Pose {
    let weight = (beats * PI * 0.5 + phase * 0.37).sin();
    let counter = (beats * PI + phase + 0.8).sin();
    pose.hips_roll += 0.010 * e * weight;
    pose.twist += 0.025 * e * counter;
    pose.side += 0.012 * e * weight;
    pose.head_tilt += 0.020 * e * weight;
    pose.arms[0].elbow += 0.035 * e * (0.5 + 0.5 * counter);
    pose.arms[1].elbow += 0.035 * e * (0.5 - 0.5 * counter);
    pose
}

/// Which programme slot plays in `phrase` (four bars each).
#[must_use]
pub fn program_slot(dancer: &Dancer, phrase: usize) -> usize {
    ((phrase + usize::from(dancer.offset)) / usize::from(dancer.hold.max(1))) % 4
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One dancer's pose: the programme's current move, cross-faded
/// from the previous one over the first beat of a new phrase. Pure.
#[must_use]
pub fn dancer_pose(dancer: &Dancer, beats: f32, bar_index: usize, bar: f32, e: f32) -> Pose {
    let phrase = bar_index / 4;
    let current = dancer.program[program_slot(dancer, phrase)];
    let previous = dancer.program[program_slot(dancer, phrase.saturating_sub(1))];
    let now = move_pose(current, beats, bar, e, dancer.phase);
    if previous == current {
        return now;
    }
    let bars_into_phrase = (bar_index % 4) as f32 + bar.rem_euclid(1.0);
    let t = smoothstep(bars_into_phrase * 4.0);
    move_pose(previous, beats, bar, e, dancer.phase).lerp(&now, t)
}

// ---- energy and bars -----------------------------------------------------

/// How hard the crowd goes: everything under Hype, a floor plus the
/// streak otherwise, and less than half of that in a quiet stretch.
/// Pure — tested.
#[must_use]
pub fn crowd_energy(hype: bool, streak: u32, calm: bool) -> f32 {
    let base = if hype {
        1.0
    } else {
        0.55 + 0.30 * (streak as f32 / 24.0).min(1.0)
    };
    if calm { base * 0.45 } else { base }
}

/// Frame-rate-independent attack/release for the shared crowd energy.
#[must_use]
pub fn smooth_energy(current: f32, target: f32, dt: f32) -> f32 {
    let current = current.clamp(0.0, 1.0);
    let target = target.clamp(0.0, 1.0);
    let rate = if target > current { 4.5 } else { 1.8 };
    target + (current - target) * (-rate * dt.clamp(0.0, 0.25)).exp()
}

/// How far from a note counts as quiet.
pub const CALM_GAP_S: f64 = 2.0;

/// Whether no note lies within [`CALM_GAP_S`] of `now` (events are
/// time-ordered; a binary search keeps this O(log n) per frame).
#[must_use]
pub fn is_calm(events: &[NoteEvent], now: f64) -> bool {
    let next = events.partition_point(|event| event.time_s < now);
    let after = events
        .get(next)
        .map_or(f64::INFINITY, |event| event.time_s - now);
    let before = next
        .checked_sub(1)
        .and_then(|i| events.get(i))
        .map_or(f64::INFINITY, |event| {
            now - event.end_time_s().max(event.time_s)
        });
    after > CALM_GAP_S && before > CALM_GAP_S
}

/// The bar `now` falls in and how far through it (0..1), from the
/// downbeat times; before the first and after the last the spacing
/// of the nearest bars is extrapolated. Needs at least two bars.
/// Pure — tested.
#[must_use]
pub fn bar_position(bars: &[f64], now: f64) -> (usize, f32) {
    if bars.len() < 2 {
        return (0, 0.0);
    }
    let last = bars.len() - 1;
    let i = bars.partition_point(|&b| b <= now);
    let (index, start, length) = if i == 0 {
        let length = bars[1] - bars[0];
        let behind = ((bars[0] - now) / length).ceil().max(1.0);
        (0usize, bars[0] - behind * length, length)
    } else if i > last {
        let length = bars[last] - bars[last - 1];
        let ahead = ((now - bars[last]) / length).floor().max(0.0);
        (last + ahead as usize, bars[last] + ahead * length, length)
    } else {
        (i - 1, bars[i - 1], bars[i] - bars[i - 1])
    };
    let phase = if length > 0.0 {
        ((now - start) / length).clamp(0.0, 0.999_99) as f32
    } else {
        0.0
    };
    (index, phase)
}

// ---- systems -------------------------------------------------------------

/// Spawn the barriers and the crowd. Runs after the figure assets are
/// baked; the crowd is the venue's, so it exists in both note styles.
pub fn spawn_crowd(
    mut commands: Commands,
    settings: Res<Settings>,
    song: Res<crate::boot::LoadedSong>,
    assets: Res<FigureAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    let layer = bevy::camera::visibility::RenderLayers::layer(STAGE_LAYER);
    // Barriers, seated on the deck: matte black with a metal top rail.
    let barrier = meshes.add(Cuboid::new(0.5, 1.1, 22.0));
    let rail = meshes.add(Cuboid::new(0.08, 0.08, 22.0));
    let barrier_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.035),
        perceptual_roughness: 1.0,
        ..default()
    });
    let rail_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.36),
        perceptual_roughness: 0.4,
        metallic: 0.8,
        ..default()
    });
    for side in [-1.0f32, 1.0] {
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(barrier.clone()),
            MeshMaterial3d(barrier_material.clone()),
            Transform::from_xyz(side * BARRIER_X, CROWD_FLOOR_Y + 0.55, -22.0),
            layer.clone(),
        ));
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(rail.clone()),
            MeshMaterial3d(rail_material.clone()),
            Transform::from_xyz(side * BARRIER_X, CROWD_FLOOR_Y + 1.14, -22.0),
            layer.clone(),
        ));
    }

    let bars: Vec<f64> = song
        .chart
        .beat_marks()
        .into_iter()
        .filter(|&(_, downbeat)| downbeat)
        .map(|(time, _)| time)
        .collect();
    commands.insert_resource(CrowdBeat { bars });
    commands.insert_resource(CrowdMood::default());

    let seed = song_seed(&song.chart.song.title);
    for index in 0..2 * CROWD_PER_SIDE {
        let slot = slot(index, seed);
        spawn_figure(
            &mut commands,
            &assets,
            &slot.spec,
            Stance::Standing,
            stand_at(slot.position, slot.yaw, &slot.spec),
            (GameplayScreen, Stage3d, slot.dancer),
            CrowdJoint,
        );
    }
}

/// Whether the crowd moves at all: the stage is up and STAGE MOTION
/// is on. Under STAGE MOTION off not a single transform is written,
/// so the crowd stands bit-identical from frame to frame. Pure —
/// tested.
#[must_use]
pub fn crowd_moves(settings: &Settings) -> bool {
    stage3d::active(settings) && settings.backdrop_motion
}

/// Dance. One pose per person from the song's beat, the bar and the
/// crowd's energy, written to the joints as transforms only.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn animate_crowd(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    players: Query<&PlayerSession>,
    beat: Option<Res<CrowdBeat>>,
    mood: Option<ResMut<CrowdMood>>,
    dancers: Query<(Entity, &Dancer), With<FigureRoot>>,
    mut joints: Query<(&FigureJoint, &mut Transform), With<CrowdJoint>>,
) {
    if !crowd_moves(&settings) {
        return;
    }
    let (Some(now), Some(player), Some(mut mood)) =
        (game_clock.song_time(&time), players.iter().next(), mood)
    else {
        return;
    };
    let track = player.session.track();
    let beats = track.tempo.beats_at(now) as f32;
    let performance = player.session.performance();
    let hype = performance.hype_active();
    let target_energy = crowd_energy(hype, performance.streak(), is_calm(track.events(), now));
    mood.energy = smooth_energy(mood.energy, target_energy, time.delta_secs());
    let energy = mood.energy;
    let (bar_index, bar) = match beat.as_deref() {
        Some(beat) if beat.bars.len() >= 2 => bar_position(&beat.bars, now),
        _ => {
            let bars = beats / 4.0;
            (bars.floor().max(0.0) as usize, bars.rem_euclid(1.0))
        }
    };
    let target = if hype { 1.0 } else { 0.0 };
    let step = (time.delta_secs() * 4.0).min(1.0);
    mood.arms_up += (target - mood.arms_up) * step;

    let mut poses: EntityHashMap<Pose> = EntityHashMap::default();
    for (entity, dancer) in &dancers {
        let e = (energy * dancer.vigour).clamp(0.0, 1.0);
        let base = dancer_pose(dancer, beats, bar_index, bar, e);
        let pose = if mood.arms_up > 1e-3 {
            base.lerp(
                &move_pose(Move::ArmsUp, beats, bar, 1.0, dancer.phase),
                mood.arms_up,
            )
        } else {
            base
        };
        poses.insert(entity, pose);
    }
    for (joint, mut transform) in &mut joints {
        let Some(pose) = poses.get(&joint.owner) else {
            continue;
        };
        let next = pose_transform(pose, joint.joint, &joint.rest, joint.bendable);
        if *transform != next {
            *transform = next;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::figure::{Build, Joint, Side, hand_position, joint_transform};
    use super::*;
    use beatbyte_core::{Lane, LaneSet};

    const MOVES: [Move; 7] = [
        Move::Bounce,
        Move::Sway,
        Move::Pump,
        Move::Clap,
        Move::Headbang,
        Move::Jump,
        Move::ArmsUp,
    ];

    #[test]
    fn every_move_is_still_at_zero_energy() {
        for mv in MOVES {
            for i in 0..40 {
                let beats = i as f32 * 0.37;
                let bar = (i as f32 * 0.113).rem_euclid(1.0);
                assert_eq!(
                    move_pose(mv, beats, bar, 0.0, 1.3),
                    Pose::default(),
                    "{mv:?} at beats {beats}"
                );
            }
        }
    }

    #[test]
    fn every_move_repeats_over_four_beats() {
        for mv in MOVES {
            for i in 0..30 {
                let beats = i as f32 * 0.29;
                let bar = (beats / 4.0).rem_euclid(1.0);
                let a = move_pose(mv, beats, bar, 0.8, 0.7);
                let b = move_pose(mv, beats + 4.0, bar, 0.8, 0.7);
                assert_eq!(a.lerp(&b, 0.0), a);
                let same = a.lerp(&b, 1.0);
                assert!(
                    (same.squash - a.squash).abs() < 1e-4
                        && (same.arms[0].raise - a.arms[0].raise).abs() < 1e-4
                        && (same.head_nod - a.head_nod).abs() < 1e-4
                        && (same.lift - a.lift).abs() < 1e-4,
                    "{mv:?} differs four beats later: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn every_move_respects_the_joint_limits() {
        for mv in MOVES {
            for i in 0..400 {
                let beats = i as f32 * 0.0731;
                let bar = (i as f32 * 0.0173).rem_euclid(1.0);
                for e in [0.5, 1.0] {
                    let pose = move_pose(mv, beats, bar, e, i as f32 * 0.1);
                    assert!(
                        pose.within_limits(),
                        "{mv:?} at {beats}/{bar}/{e}: {pose:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn hype_puts_every_arm_up() {
        for i in 0..20 {
            let pose = move_pose(Move::ArmsUp, i as f32 * 0.3, 0.0, 1.0, 0.0);
            assert!(
                pose.arms[0].raise >= 2.6 && pose.arms[1].raise >= 2.6,
                "{pose:?}"
            );
            let hand = hand_position(
                &pose,
                Build::Medium,
                Stance::Standing,
                Side::Left,
                Detail::Full,
            );
            let head =
                joint_transform(&pose, Build::Medium, Stance::Standing, Joint::Head).translation;
            assert!(
                hand.y > head.y,
                "the hand is above the head: {hand} vs {head}"
            );
        }
    }

    #[test]
    fn a_calm_section_moves_less_than_a_loud_one() {
        let dancer = slot(3, 7).dancer;
        let loud = crowd_energy(false, 30, false);
        let calm = crowd_energy(false, 30, true);
        assert!(calm < loud * 0.5);
        let mut louder = 0;
        for i in 0..40 {
            let beats = i as f32 * 0.27;
            let a = dancer_pose(&dancer, beats, 1, (beats / 4.0).rem_euclid(1.0), loud);
            let b = dancer_pose(&dancer, beats, 1, (beats / 4.0).rem_euclid(1.0), calm);
            let size = |p: &Pose| {
                p.squash + p.arms[0].raise + p.arms[1].raise + p.head_nod.abs() + p.side.abs()
            };
            if size(&a) > size(&b) + 1e-6 {
                louder += 1;
            }
            assert!(size(&a) >= size(&b) - 1e-6);
        }
        assert!(
            louder > 20,
            "the loud pose is bigger most of the time: {louder}/40"
        );
    }

    #[test]
    fn the_jump_leaves_the_ground_on_the_downbeat_and_lands_within_the_bar() {
        let at = |bar: f32| move_pose(Move::Jump, bar * 4.0, bar, 1.0, 0.0);
        assert!(at(0.15).lift > 0.1, "in the air just after the downbeat");
        assert!(at(0.95).squash > 0.1, "loaded before the downbeat");
        for bar in [0.55, 0.6, 0.7, 0.85] {
            assert!(at(bar).lift.abs() < 1e-6, "on the ground at {bar}");
        }
        for i in 0..100 {
            assert!(at(i as f32 / 100.0).lift >= 0.0, "never below the ground");
        }
    }

    #[test]
    fn no_dancer_stands_in_the_bed_or_on_the_band_riser() {
        let seed = song_seed("Some Song");
        let mut spots = std::collections::HashSet::new();
        let mut per_side = [0usize; 2];
        for index in 0..2 * CROWD_PER_SIDE {
            let s = slot(index, seed);
            assert!(
                s.position.x.abs() >= 3.2 && s.position.x.abs() <= 5.6,
                "{index}: x {}",
                s.position.x
            );
            assert!(
                s.position.z > -25.5 && s.position.z <= -12.0,
                "{index}: z {}",
                s.position.z
            );
            assert_eq!(s.position.y, CROWD_FLOOR_Y);
            per_side[usize::from(s.side > 0.0)] += 1;
            let key = (
                (s.position.x * 10.0).round() as i32,
                (s.position.z * 10.0).round() as i32,
            );
            assert!(spots.insert(key), "{index} shares a spot");
        }
        assert_eq!(per_side, [CROWD_PER_SIDE, CROWD_PER_SIDE]);
    }

    #[test]
    fn no_dancer_reaches_into_the_bed() {
        // The widest reach over every move, then scaled by the tallest
        // person and taken off the innermost stance.
        let mut reach: f32 = 0.0;
        for mv in MOVES {
            for i in 0..80 {
                let beats = i as f32 * 0.11;
                let pose = move_pose(mv, beats, (beats / 4.0).rem_euclid(1.0), 1.0, 0.0);
                for side in [Side::Left, Side::Right] {
                    let hand =
                        hand_position(&pose, Build::Broad, Stance::Standing, side, Detail::Full);
                    reach = reach.max(hand.x.abs()).max(hand.z.abs());
                }
            }
        }
        let innermost = CROWD_INNER_X - 0.15 * 0.5;
        let rails = 1.66;
        assert!(
            innermost - reach * figure::MAX_HEIGHT > rails + 0.3,
            "reach {reach} from {innermost} clears the rails at {rails}"
        );
    }

    #[test]
    fn the_crowd_is_the_same_crowd_every_run_and_another_for_another_song() {
        let a = song_seed("Alpha");
        let b = song_seed("Beta");
        assert_eq!(slot(5, a), slot(5, a));
        let differ = (0..2 * CROWD_PER_SIDE)
            .filter(|&i| slot(i, a).spec != slot(i, b).spec)
            .count();
        assert!(
            differ > CROWD_PER_SIDE,
            "another song, another crowd: {differ} differ"
        );
    }

    #[test]
    fn reactions_cluster_around_the_beat_without_becoming_identical() {
        let phases: Vec<f32> = (0..2 * CROWD_PER_SIDE)
            .map(|index| slot(index, 17).dancer.phase)
            .collect();
        assert!(phases.iter().all(|phase| phase.abs() < 0.7));
        let span = phases.iter().copied().fold(f32::MIN, f32::max)
            - phases.iter().copied().fold(f32::MAX, f32::min);
        assert!(span > 0.1, "people still carry individual reaction lag");
    }

    #[test]
    fn crowd_energy_arrives_quickly_and_releases_slowly() {
        let attack = smooth_energy(0.2, 0.9, 0.1);
        let release = smooth_energy(0.9, 0.2, 0.1);
        assert!(attack > 0.2 && attack < 0.9);
        assert!(release > 0.2 && release < 0.9);
        assert!(
            attack - 0.2 > 0.9 - release,
            "the room reacts faster than it settles"
        );
        assert!((smooth_energy(0.5, 0.5, 0.2) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_dancer_holds_a_move_for_its_hold_and_then_changes() {
        let dancer = Dancer {
            index: 0,
            program: [Move::Bounce, Move::Sway, Move::Pump, Move::Clap],
            hold: 2,
            offset: 0,
            phase: 0.0,
            vigour: 1.0,
        };
        assert_eq!(program_slot(&dancer, 0), 0);
        assert_eq!(program_slot(&dancer, 1), 0);
        assert_eq!(program_slot(&dancer, 2), 1);
        assert_eq!(program_slot(&dancer, 7), 3);
        assert_eq!(program_slot(&dancer, 8), 0);
        // Mid-phrase the current move plays; at the boundary the
        // previous one still shows and fades within a beat.
        let sway = dancer_pose(&dancer, 8.0 * 4.0 + 2.5, 9, 0.625, 1.0);
        assert!(
            sway.side.abs() > 0.01 || sway.hips_roll.abs() > 0.01,
            "sway mid-phrase {sway:?}"
        );
        let boundary = dancer_pose(&dancer, 16.0, 16, 0.0, 1.0);
        let pump = move_pose(Move::Pump, 16.0, 0.0, 1.0, 0.0);
        assert!(
            boundary.arms[1].raise < pump.arms[1].raise * 0.5,
            "the pump fades in: {boundary:?}"
        );
        let settled = dancer_pose(&dancer, 17.5, 16, 0.375, 1.0);
        assert!(
            (settled.arms[1].raise - move_pose(Move::Pump, 17.5, 0.375, 1.0, 0.0).arms[1].raise)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn bar_position_reads_the_downbeats() {
        let bars = [0.0, 2.0, 4.0];
        assert_eq!(bar_position(&bars, 3.0), (1, 0.5));
        assert_eq!(bar_position(&bars, 0.0), (0, 0.0));
        let (i, p) = bar_position(&bars, -1.0);
        assert!(
            i == 0 && (p - 0.5).abs() < 1e-5,
            "before the first: {i} {p}"
        );
        let (i, p) = bar_position(&bars, 7.0);
        assert!(i == 3 && (p - 0.5).abs() < 1e-5, "after the last: {i} {p}");
        assert_eq!(bar_position(&[1.0], 5.0), (0, 0.0));
    }

    #[test]
    fn crowd_energy_rises_with_streak_and_saturates_under_hype() {
        assert!(crowd_energy(false, 0, false) < crowd_energy(false, 12, false));
        assert!(crowd_energy(false, 12, false) < crowd_energy(false, 24, false));
        assert_eq!(
            crowd_energy(false, 24, false),
            crowd_energy(false, 240, false)
        );
        assert_eq!(crowd_energy(true, 0, false), 1.0);
        assert!(crowd_energy(true, 0, true) < 0.5);
    }

    #[test]
    fn a_calm_stretch_is_two_seconds_without_a_note() {
        let note = |t: f64| NoteEvent::tap(t, LaneSet::single(Lane::One));
        let events = [note(1.0), note(10.0), note(10.5)];
        assert!(!is_calm(&events, 1.5));
        assert!(!is_calm(&events, 8.5));
        assert!(is_calm(&events, 5.0));
        assert!(is_calm(&events, 20.0));
        assert!(is_calm(&[], 0.0));
    }

    #[test]
    fn stage_motion_off_stills_the_crowd() {
        let on = Settings::default();
        assert!(crowd_moves(&on), "the default settings dance");
        let off = Settings {
            backdrop_motion: false,
            ..Settings::default()
        };
        assert!(!crowd_moves(&off), "STAGE MOTION off writes nothing");
        let no_stage = Settings {
            stage_3d: false,
            ..Settings::default()
        };
        assert!(!crowd_moves(&no_stage));
    }

    #[test]
    fn a_spawned_figure_carries_the_stage_layer_on_every_mesh_and_a_joint_on_every_moving_part() {
        use bevy::camera::visibility::RenderLayers;
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(crate::theme::ActiveTheme::default());
        app.world_mut()
            .run_system_once(figure::setup_figure_assets)
            .unwrap();
        let seed = song_seed("Test");
        let handles = app
            .world_mut()
            .run_system_once(move |mut commands: Commands, assets: Res<FigureAssets>| {
                let s = slot(0, seed);
                spawn_figure(
                    &mut commands,
                    &assets,
                    &s.spec,
                    Stance::Standing,
                    stand_at(s.position, s.yaw, &s.spec),
                    (GameplayScreen, Stage3d, s.dancer),
                    CrowdJoint,
                )
            })
            .unwrap();
        let world = app.world_mut();
        let mut meshes_seen = 0;
        let mut joints_seen = 0;
        let mut query = world.query::<(
            Entity,
            Option<&Mesh3d>,
            Option<&FigureJoint>,
            Option<&RenderLayers>,
        )>();
        for (entity, mesh, joint, layers) in query.iter(world) {
            if mesh.is_some() {
                meshes_seen += 1;
                assert_eq!(
                    layers,
                    Some(&RenderLayers::layer(STAGE_LAYER)),
                    "{entity:?} is off the stage layer"
                );
            }
            if let Some(joint) = joint {
                joints_seen += 1;
                assert_eq!(joint.owner, handles.root);
                assert!(
                    world.get::<CrowdJoint>(entity).is_some(),
                    "{entity:?} lacks the crowd marker"
                );
                assert_eq!(
                    world.get::<Transform>(entity),
                    Some(&joint.rest),
                    "spawned at rest"
                );
            }
        }
        let expected_joints = match slot(0, seed).spec.detail {
            Detail::Full => 11,
            Detail::Simple => 7,
        };
        assert_eq!(joints_seen, expected_joints, "every moving part is a joint");
        assert!(
            meshes_seen >= expected_joints,
            "every joint carries a mesh: {meshes_seen}"
        );
        assert!(world.get::<Visibility>(handles.root).is_some());
        assert!(world.get::<FigureRoot>(handles.root).is_some());
        assert!(world.get::<Dancer>(handles.root).is_some());
    }
}
