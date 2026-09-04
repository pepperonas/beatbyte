//! Sparks that live in the room (roadmap H3, the 3D half of the hit
//! burst).
//!
//! The flat view throws 2D sprites at the strike line. On the 3D
//! stage that is the wrong space: the receptors stand on a neck in
//! perspective, lit by the rig and bloomed by the camera, and a
//! sprite pinned to screen coordinates belongs to neither. These
//! sparks are world-space — they leave the receptor that was struck,
//! arc through the stage light, and the bloom pass catches them the
//! way it catches the gems.
//!
//! Three decisions worth naming:
//!
//! - **They shrink instead of fading.** Alpha would drag every spark
//!   into the transparency sort; scale to nothing costs nothing and
//!   reads the same at this size.
//! - **Five materials, not one per spark.** A spark takes its lane's
//!   emissive material and a white one for the Perfect accent, all
//!   built once — a material per particle would be an allocation per
//!   note.
//! - **Round style only**, exactly like the hit flame. The 8-bit neck
//!   keeps its own vocabulary; this is not the place to change it.

use beatbyte_core::{Judgment, Lane, SessionEvent};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::stage3d::{NeckStyle, STAGE_LAYER, Stage3d, lane_x, neck_style};
use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, SessionFeedback};
use crate::config::Settings;

/// How many sparks a hit throws, before the intensity scale.
#[must_use]
pub fn spark_count(judgment: Judgment) -> usize {
    match judgment {
        Judgment::Perfect => 14,
        Judgment::Great => 9,
        Judgment::Good => 5,
        Judgment::Miss => 0,
    }
}

/// Never more than this many alive at once. Four players hitting
/// chords is the worst case, and it stays well inside the frame
/// budget the 3D stage already measures against.
pub const MAX_LIVE: usize = 260;

/// How long a spark lives, at its shortest and longest.
const TTL_MIN: f32 = 0.28;
const TTL_SPAN: f32 = 0.34;

/// Gravity, in world units per second squared. The neck is about a
/// unit wide per lane, so this is a gentle arc rather than a fall.
const GRAVITY: f32 = 6.5;

/// A spark's launch velocity, from a deterministic hash.
///
/// Upward-biased and fanned across the neck, with a forward lean
/// toward the camera so the arc is read in perspective rather
/// than sliding along the board. Pure — tested.
#[must_use]
pub fn launch(seed: usize, speed: f32) -> Vec3 {
    let a = super::fx::hash01(seed.wrapping_mul(31).wrapping_add(7));
    let b = super::fx::hash01(seed.wrapping_mul(57).wrapping_add(13));
    let c = super::fx::hash01(seed.wrapping_mul(97).wrapping_add(29));
    // Sideways fan, symmetric about the lane.
    let sideways = (a - 0.5) * 1.6;
    // Always upward, never into the neck.
    let up = 0.55 + 0.95 * b;
    // Toward the camera (+z is toward the player on this stage).
    let forward = 0.15 + 0.75 * c;
    Vec3::new(sideways, up, forward) * speed
}

/// Where a spark is, and how big, `age` seconds after its launch.
///
/// Returns `None` once it is spent, so the caller despawns rather
/// than drawing a zero-sized mesh forever. Pure — tested.
#[must_use]
pub fn advance(origin: Vec3, velocity: Vec3, age: f32, ttl: f32, size: f32) -> Option<(Vec3, f32)> {
    if age >= ttl {
        return None;
    }
    let position = origin + velocity * age - Vec3::Y * (0.5 * GRAVITY * age * age);
    // Shrink along the whole life, so the last frame is already
    // nothing and nothing pops out of existence.
    let left = 1.0 - age / ttl;
    Some((position, size * left * left))
}

/// One live spark.
#[derive(Component)]
pub struct Spark3d {
    /// Where it left the receptor.
    pub origin: Vec3,
    /// Its launch velocity.
    pub velocity: Vec3,
    /// Seconds since launch.
    pub age: f32,
    /// Its lifetime.
    pub ttl: f32,
    /// Its size at birth.
    pub size: f32,
}

/// The shared geometry and the one material per lane, built once.
#[derive(Resource)]
pub struct SparkAssets {
    mesh: Handle<Mesh>,
    lane: Vec<Handle<StandardMaterial>>,
    accent: Handle<StandardMaterial>,
}

/// Build them when the stage comes up.
pub fn setup_spark_assets(
    mut commands: Commands,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) {
        return;
    }
    // A coarse ball: at two centimetres on screen nobody counts the
    // facets, and a cheap mesh is the whole point of a particle.
    // A coarse ball: at two centimetres on screen nobody counts the
    // facets, and a cheap mesh is the whole point of a particle.
    let mesh = meshes.add(Sphere::new(1.0).mesh().uv(6, 4));
    let emissive = |color: Color| StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * 6.0,
        unlit: true,
        ..default()
    };
    let lane = Lane::ALL
        .iter()
        .map(|lane| materials.add(emissive(theme.0.lane_color(*lane))))
        .collect();
    let accent = materials.add(emissive(Color::srgb(1.0, 0.96, 0.86)));
    commands.insert_resource(SparkAssets { mesh, lane, accent });
}

/// Whether this configuration throws sparks at all.
///
/// The 3D stage, the round neck, and the particles setting — the
/// middle one is the 8-bit neck's protection: it keeps its own
/// vocabulary, and a spray of round embers is not part of it. Pure —
/// tested, because "the 8-bit mode stays untouched" is a promise and
/// not a detail.
#[must_use]
pub fn throws_sparks(settings: &Settings) -> bool {
    super::stage3d::active(settings)
        && neck_style(settings) == NeckStyle::Instrument
        && settings.particles
}

/// Throw sparks from the receptor that was struck.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn spawn_sparks(
    mut commands: Commands,
    settings: Res<Settings>,
    assets: Option<Res<SparkAssets>>,
    layout: Res<HighwayLayout>,
    mut feedback: MessageReader<SessionFeedback>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    live: Query<(), With<Spark3d>>,
) {
    let (Some(assets), true) = (assets, throws_sparks(&settings)) else {
        // Not ours this frame — but the messages still have to be
        // consumed, or this reader falls behind by a whole song.
        feedback.clear();
        return;
    };
    let mut alive = live.iter().count();
    let intensity = settings.fx_intensity.clamp(0.0, 1.0);
    for message in feedback.read() {
        let SessionEvent::NoteHit {
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
        let count = scaled(spark_count(judgment), intensity);
        let speed = match judgment {
            Judgment::Perfect => 2.3,
            _ => 1.8,
        };
        for lane in event.lanes.iter() {
            let x = lane_x(&layout, message.player_index, lane);
            // Just above the receptor's face, so a spark leaves the
            // ring rather than the board under it.
            let origin = Vec3::new(x, 0.09, 0.0);
            for i in 0..count {
                if alive >= MAX_LIVE {
                    return;
                }
                alive += 1;
                let seed = event_index
                    .wrapping_mul(101)
                    .wrapping_add(lane.index() * 17)
                    .wrapping_add(i);
                let velocity = launch(seed, speed);
                let size = 0.026 + 0.030 * super::fx::hash01(seed.wrapping_add(3));
                let ttl = TTL_MIN + TTL_SPAN * super::fx::hash01(seed.wrapping_add(5));
                // Every fourth spark of a Perfect is white: the same
                // "electric" accent the flat view gives it.
                let white = judgment == Judgment::Perfect && i % 4 == 0;
                let material = if white {
                    assets.accent.clone()
                } else {
                    assets
                        .lane
                        .get(lane.index())
                        .cloned()
                        .unwrap_or_else(|| assets.accent.clone())
                };
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Spark3d {
                        origin,
                        velocity,
                        age: 0.0,
                        ttl,
                        size,
                    },
                    Mesh3d(assets.mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(origin).with_scale(Vec3::splat(size)),
                    RenderLayers::layer(STAGE_LAYER),
                ));
            }
        }
    }
}

/// How many sparks survive the intensity setting. At zero there are
/// none; a hit never throws less than one while the setting is above
/// zero, or a low setting would silently mean "off".
#[must_use]
pub fn scaled(count: usize, intensity: f32) -> usize {
    if intensity <= 0.0 || count == 0 {
        return 0;
    }
    ((count as f32 * intensity).round() as usize).max(1)
}

/// Fly them, shrink them, and let them go.
pub fn drive_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark3d, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += dt;
        match advance(
            spark.origin,
            spark.velocity,
            spark.age,
            spark.ttl,
            spark.size,
        ) {
            Some((position, size)) => {
                transform.translation = position;
                transform.scale = Vec3::splat(size);
            }
            None => commands.entity(entity).despawn(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_better_hit_throws_more_sparks() {
        assert!(spark_count(Judgment::Perfect) > spark_count(Judgment::Great));
        assert!(spark_count(Judgment::Great) > spark_count(Judgment::Good));
        assert_eq!(spark_count(Judgment::Miss), 0, "a miss throws nothing");
    }

    #[test]
    fn every_spark_leaves_upward_and_toward_the_room() {
        for seed in 0..64 {
            let v = launch(seed, 2.0);
            assert!(v.y > 0.0, "seed {seed} went into the neck");
            assert!(v.z > 0.0, "seed {seed} went away from the camera");
            assert!(v.x.abs() <= 2.0 * 0.8 + 1e-6, "seed {seed} fanned too wide");
        }
        // Deterministic: the same seed is the same spark, so a replay
        // of the same chart throws the same sparks.
        assert_eq!(launch(9, 2.0), launch(9, 2.0));
        assert_ne!(launch(9, 2.0), launch(10, 2.0));
    }

    #[test]
    fn a_spark_arcs_shrinks_and_then_is_gone() {
        let origin = Vec3::new(0.5, 0.09, 0.0);
        let velocity = Vec3::new(0.0, 2.0, 1.0);
        let (early, size_early) = advance(origin, velocity, 0.05, 0.5, 0.02).expect("alive");
        let (later, size_later) = advance(origin, velocity, 0.25, 0.5, 0.02).expect("alive");
        assert!(early.y > origin.y, "it rises");
        assert!(later.z > early.z, "and keeps coming toward the room");
        assert!(size_later < size_early, "shrinking all the way");
        // Gravity bends it back down: at the end of a long life the
        // spark is below where a straight line would have put it.
        let (late, _) = advance(origin, velocity, 0.9, 1.0, 0.02).expect("alive");
        assert!(late.y < origin.y + velocity.y * 0.9, "gravity pulls");
        // Spent.
        assert_eq!(advance(origin, velocity, 0.5, 0.5, 0.02), None);
        assert_eq!(advance(origin, velocity, 9.0, 0.5, 0.02), None);
    }

    #[test]
    fn only_the_round_neck_on_the_3d_stage_throws_them() {
        let base = Settings {
            stage_3d: true,
            round_gems: true,
            particles: true,
            ..Settings::default()
        };
        assert!(throws_sparks(&base));
        assert!(
            !throws_sparks(&Settings {
                round_gems: false,
                ..base.clone()
            }),
            "the 8-bit neck keeps its own vocabulary"
        );
        assert!(
            !throws_sparks(&Settings {
                stage_3d: false,
                ..base.clone()
            }),
            "the flat view has its own sprites"
        );
        assert!(
            !throws_sparks(&Settings {
                particles: false,
                ..base.clone()
            }),
            "particles off is off"
        );
    }

    #[test]
    fn the_intensity_setting_thins_them_out_and_zero_means_none() {
        assert_eq!(scaled(14, 1.0), 14);
        assert_eq!(scaled(14, 0.5), 7);
        assert_eq!(scaled(14, 0.0), 0, "off is off");
        assert_eq!(scaled(0, 1.0), 0);
        assert_eq!(
            scaled(2, 0.05),
            1,
            "a low setting still shows something, or it would read as broken"
        );
    }
}
