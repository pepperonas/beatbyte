//! The Star-Power arc: lightning along the rails while Hype runs.
//!
//! The genre's edge during its power state is electric — jagged
//! white-cyan bolts crackling down both sides of the neck, forking,
//! jumping to new shapes many times a second, never the same twice.
//! (The first idea here was a sheet of fire; it read as an ice fence
//! and the user asked for the bolt instead.)
//!
//! Built from what the project already renders: each rail carries a
//! fixed pool of thin additive **segments** chained along z. Every
//! crackle step (24 Hz) each segment's endpoints are re-rolled from a
//! hash of `(rail, segment, step)` — so the bolt jumps like lightning
//! does — and a few segments drop out (a bolt is not a continuous
//! wire). **Forks** are a second, shorter pool, each re-anchored to a
//! random segment per step and thrown off at an angle. Brightness
//! flashes by thickness, not by material: one shared material, zero
//! material writes, zero allocation per frame.
//!
//! `reduced_flashing` turns the crackle into a slow wander (2 Hz),
//! with no gaps and no flashes — a steady arc instead of a strobe.
//! `fx_intensity` scales how many segments and forks are live.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::stage3d::{NeckStyle, STAGE_LAYER, Stage3d, neck_style, rail_x};
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
const JITTER_X: f32 = 0.22;
/// Height range of a bolt point above the rail.
const HEIGHT: (f32, f32) = (0.04, 0.34);
/// Share of segments dropped per step: the gaps in a bolt.
const GAP_SHARE: f32 = 0.12;
/// Share of steps that flash bright.
const FLASH_SHARE: f32 = 0.08;
/// The arc's colour: white-cyan, the genre's power hue, not the
/// house Hype purple (the same call the edge fire made).
const ARC: Color = Color::srgb(0.7, 0.95, 1.0);

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
        2.2
    } else {
        0.7 + 0.8 * super::fx::hash01(seed + step as usize * 29 + 9)
    }
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

/// Spawn the arc pools for every rail. Instrument neck only.
pub fn spawn_arcs(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) || neck_style(&settings) != NeckStyle::Instrument {
        return;
    }
    let layer = RenderLayers::layer(STAGE_LAYER);
    let segment = meshes.add(Cuboid::new(1.0, 0.035, 0.035));
    let fork = meshes.add(Cuboid::new(1.0, 0.022, 0.022));
    // One material for everything: additive, strongly emissive so it
    // reads next to the gems, never written after this.
    let material = materials.add(StandardMaterial {
        base_color: ARC.with_alpha(0.85),
        emissive: ARC.to_linear() * 7.0,
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for index in &players {
        let player = index.0;
        for side in [-1.0f32, 1.0] {
            let x = rail_x(&layout, player, side);
            for bolt in 0..BOLTS_PER_RAIL {
                for seg in 0..SEGMENTS {
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        BoltSegment {
                            player,
                            side,
                            bolt,
                            segment: seg,
                        },
                        Mesh3d(segment.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(x, 0.1, point_z(seg)).with_scale(Vec3::ZERO),
                        Visibility::Hidden,
                        layer.clone(),
                    ));
                }
            }
            for index in 0..FORKS_PER_RAIL {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    BoltFork {
                        player,
                        side,
                        index,
                    },
                    Mesh3d(fork.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(x, 0.1, 0.0).with_scale(Vec3::ZERO),
                    Visibility::Hidden,
                    layer.clone(),
                ));
            }
        }
    }
}

/// Crackle the arcs while Hype runs, eased in and out like the edge
/// fire. Transforms and visibility only.
#[allow(clippy::type_complexity)]
pub fn crackle_arcs(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut segments: Query<(&BoltSegment, &mut Transform, &mut Visibility), Without<BoltFork>>,
    mut forks: Query<(&BoltFork, &mut Transform, &mut Visibility), Without<BoltSegment>>,
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
        let a = Vec3::new(x + ax * grown, 0.015 + ay * grown, point_z(seg.segment));
        let b = Vec3::new(x + bx * grown, 0.015 + by * grown, point_z(seg.segment + 1));
        let (mid, rot, len) = segment_pose(a, b);
        let thick = flash(seed, step, calm) * grown;
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
        let a = Vec3::new(x + ax * grown, 0.015 + ay * grown, point_z(anchor));
        let b = a + dir * (len * grown);
        let (mid, rot, len) = segment_pose(a, b);
        let thick = flash(seed + 1, step, calm) * grown;
        transform.translation = mid;
        transform.rotation = rot;
        transform.scale = Vec3::new(len, thick, thick);
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
}
