//! Thin Star-Power lightning along the rails while Hype runs.
//!
//! Rails and the phrase-completion strike share the same core/hull meshes,
//! materials, jagged chain and capped crackle. Each rail repeats the strike's
//! leader, impact and fade, with a short dark gap and staggered timing. There
//! are no overlapping bolts, branch pools or broad deck aura to thicken it.
//!
//! Pools are created once; animation writes only transforms and visibility.
//! Reduced flashing keeps a steady thin chain with slow shape changes.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::stage3d::{STAGE_LAYER, Stage3d, rail_x};
use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession};
use crate::config::Settings;

/// Segments in a rail's longer chain.
pub const SEGMENTS: usize = 40;
/// Shape changes per second, shared with the descending strike.
pub const CRACKLE_HZ: f32 = 24.0;
/// Slow shape changes under reduced flashing.
pub const CALM_HZ: f32 = 2.0;
/// Basis used by the deterministic chain-point hash.
pub const JITTER_X: f32 = 0.22;
/// Hot blue-white core, shared by both effects.
pub const CORE: Color = Color::srgb(0.6, 0.85, 1.0);
/// Electric-blue hull, shared by both effects.
pub const HULL: Color = Color::srgb(0.08, 0.4, 1.0);
/// Core emission.
pub const CORE_GLOW: f32 = 2.4;
/// Hull emission.
pub const HULL_GLOW: f32 = 2.2;

const HEIGHT: (f32, f32) = (0.04, 0.34);
const THICKNESS: f32 = 0.028;
const HULL_FACTOR: f32 = 2.4;
const CRACKLE_CAP: f32 = 1.3;
pub(super) const JITTER: f32 = 0.55;
const RAIL_Y: f32 = 0.4;
const SPAN: (f32, f32) = (-25.0, 2.0);
const REST_S: f32 = 0.16;

/// One segment of a player's rail lightning.
#[derive(Component)]
pub struct BoltSegment {
    /// Owning player.
    pub player: usize,
    /// −1 left rail, +1 right rail.
    pub side: f32,
    /// Position in the chain.
    pub segment: usize,
}

/// Which deterministic crackle step the clock is in.
#[must_use]
pub fn step(now: f32, hz: f32) -> u32 {
    (now.max(0.0) * hz).floor() as u32
}

/// Deterministic lateral/vertical hash, shared with the descending strike.
#[must_use]
pub fn point(seed: usize, index: usize, step: u32) -> (f32, f32) {
    let a = super::fx::hash01(seed + index * 131 + step as usize * 7);
    let b = super::fx::hash01(seed + index * 197 + step as usize * 3 + 1);
    (
        (a - 0.5) * 2.0 * JITTER_X,
        HEIGHT.0 + (HEIGHT.1 - HEIGHT.0) * b,
    )
}

/// Crackle brightness; the stroke renderer caps it for a thin hull.
#[must_use]
pub fn flash(seed: usize, step: u32, calm: bool) -> f32 {
    if calm {
        return 1.0;
    }
    let roll = super::fx::hash01(seed + step as usize * 23 + 2);
    if roll < 0.08 {
        2.2
    } else {
        0.7 + 0.8 * super::fx::hash01(seed + step as usize * 29 + 9)
    }
}

/// Identical physical stroke geometry for rails and the descending strike.
pub(super) fn bolt_meshes(meshes: &mut Assets<Mesh>) -> (Handle<Mesh>, Handle<Mesh>) {
    (
        meshes.add(Cuboid::new(1.0, THICKNESS, THICKNESS)),
        meshes.add(Cuboid::new(
            1.0,
            THICKNESS * HULL_FACTOR,
            THICKNESS * HULL_FACTOR,
        )),
    )
}

/// Shared thickness animation, including intensity and the crackle cap.
pub(super) fn stroke(glow: f32, seed: usize, step: u32, calm: bool, intensity: f32) -> f32 {
    glow * flash(seed, step, calm).min(CRACKLE_CAP) * intensity.clamp(0.0, 1.0)
}

/// A jagged chain with pinned endpoints, independent of its length/orientation.
pub(super) fn chain_point(
    origin: Vec3,
    target: Vec3,
    index: usize,
    segments: usize,
    seed: usize,
    step: u32,
) -> Vec3 {
    let along = index as f32 / segments as f32;
    let base = origin.lerp(target, along);
    let bump = 4.0 * along * (1.0 - along);
    let (dx, dy) = point(seed, index, step);
    let sideways = dx / JITTER_X * JITTER;
    let across = (dy - 0.19) / 0.15 * JITTER * 0.6;
    let direction = (target - origin).normalize_or_zero();
    let perpendicular = Vec3::new(0.0, direction.z, -direction.y).normalize_or_zero();
    base + Vec3::X * (sideways * bump) + perpendicular * (across * bump)
}

/// Place a unit-length segment between two chain points.
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

/// Shared, immutable additive material for both lightning effects.
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

fn rail_seed(player: usize, side: f32) -> usize {
    player * 7919 + if side < 0.0 { 11 } else { 13 }
}

fn rail_cycle(now: f32, seed: usize) -> (u32, f32) {
    let period = super::strike::STRIKE_S + REST_S;
    let clock = now.max(0.0) + super::fx::hash01(seed + 123) * period;
    ((clock / period).floor() as u32, clock.rem_euclid(period))
}

fn rail_phase(now: f32, seed: usize, calm: bool) -> super::strike::Phase {
    if calm {
        super::strike::Phase {
            reach: 1.0,
            glow: 1.0,
            impact: 0.0,
        }
    } else {
        super::strike::strike_phase(rail_cycle(now, seed).1, false)
    }
}

/// Spawn one thin lightning chain per rail, without aura or extra branches.
pub fn spawn_arcs(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) {
        return;
    }
    let layer = RenderLayers::layer(STAGE_LAYER);
    let (segment, hull_mesh) = bolt_meshes(&mut meshes);
    let core = bolt_material(&mut materials, CORE, CORE_GLOW);
    let hull = bolt_material(&mut materials, HULL, HULL_GLOW);
    for index in &players {
        for side in [-1.0, 1.0] {
            let x = rail_x(&layout, index.0, side);
            for seg in 0..SEGMENTS {
                commands
                    .spawn((
                        GameplayScreen,
                        Stage3d,
                        BoltSegment {
                            player: index.0,
                            side,
                            segment: seg,
                        },
                        super::stage3d::on_the_neck(),
                        Mesh3d(segment.clone()),
                        MeshMaterial3d(core.clone()),
                        Transform::from_xyz(x, RAIL_Y, SPAN.0).with_scale(Vec3::ZERO),
                        Visibility::Hidden,
                        layer.clone(),
                    ))
                    .with_children(|parent| {
                        parent.spawn((
                            Stage3d,
                            super::stage3d::on_the_neck(),
                            Mesh3d(hull_mesh.clone()),
                            MeshMaterial3d(hull.clone()),
                            Transform::IDENTITY,
                            layer.clone(),
                        ));
                    });
            }
        }
    }
}

/// Repeat the reference strike animation on active players' rails.
pub fn crackle_arcs(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut segments: Query<(&BoltSegment, &mut Transform, &mut Visibility)>,
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
    let intensity = settings.fx_intensity.clamp(0.0, 1.0);
    for (seg, mut transform, mut visibility) in &mut segments {
        let grown = blend.get(seg.player).copied().unwrap_or(0.0);
        let seed = rail_seed(seg.player, seg.side);
        let phase = rail_phase(now, seed, calm);
        let live = grown >= 0.02
            && intensity > 0.0
            && phase.glow > 0.0
            && (seg.segment as f32) < phase.reach * SEGMENTS as f32;
        *visibility = if live {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !live {
            continue;
        }
        let seed = if calm {
            seed
        } else {
            seed + rail_cycle(now, seed).0 as usize * 97
        };
        let x = rail_x(&layout, seg.player, seg.side);
        let origin = Vec3::new(x, RAIL_Y, SPAN.0);
        let target = Vec3::new(x, RAIL_Y, SPAN.1);
        let a = chain_point(origin, target, seg.segment, SEGMENTS, seed, step);
        let b = chain_point(origin, target, seg.segment + 1, SEGMENTS, seed, step);
        let (mid, rot, len) = segment_pose(a, b);
        let thick = stroke(phase.glow, seed, step, calm, intensity) * grown;
        transform.translation = mid;
        transform.rotation = rot;
        transform.scale = Vec3::new(len, thick, thick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_renderers_spawn_the_same_thin_core_hull_and_materials() {
        use beatbyte_core::{
            Difficulty, Lane, LaneSet, NoteEvent, ScoreConfig, TempoMap, TimingWindows, Track,
            TrackSession,
        };
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .insert_resource(Settings {
                stage_3d: true,
                ..default()
            })
            .insert_resource(HighwayLayout::for_players(1));
        let track = Track::new(
            Difficulty::Medium,
            TempoMap::constant(120.0, 0.0),
            vec![NoteEvent::tap(1.0, LaneSet::single(Lane::Three))],
            vec![],
        )
        .expect("valid track");
        app.world_mut().spawn((
            PlayerIndex(0),
            PlayerSession {
                session: TrackSession::new(track, TimingWindows::default(), ScoreConfig::default()),
                frame_events: Vec::new(),
                spawn_cursor: 0,
            },
        ));
        app.world_mut()
            .run_system_once(spawn_arcs)
            .expect("spawn rails");
        app.world_mut()
            .run_system_once(super::super::strike::spawn_strikes)
            .expect("spawn reference strike");
        let rail = app
            .world_mut()
            .query_filtered::<(
                Entity,
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
                &Children,
            ), With<BoltSegment>>()
            .iter(app.world())
            .next()
            .map(|(entity, mesh, material, children)| {
                (entity, mesh.0.clone(), material.0.clone(), children[0])
            })
            .expect("rail segment");
        let reference = app.world_mut().query_filtered::<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &Children), With<super::super::strike::StrikeSegment>>()
            .iter(app.world()).next().map(|(mesh, material, children)|
                (mesh.0.clone(), material.0.clone(), children[0])).expect("strike segment");
        let meshes = app.world().resource::<Assets<Mesh>>();
        let positions = |handle: &Handle<Mesh>| {
            meshes
                .get(handle)
                .expect("mesh")
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .expect("positions")
                .as_float3()
                .expect("float positions")
        };
        assert_eq!(
            positions(&rail.1),
            positions(&reference.0),
            "same core cross-section"
        );
        let rail_hull = app.world().get::<Mesh3d>(rail.3).expect("rail hull");
        let strike_hull = app.world().get::<Mesh3d>(reference.2).expect("strike hull");
        assert_eq!(
            positions(&rail_hull.0),
            positions(&strike_hull.0),
            "same thin blue hull"
        );
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let a = materials.get(&rail.2).expect("rail material");
        let b = materials.get(&reference.1).expect("strike material");
        assert_eq!(a.base_color, b.base_color);
        assert_eq!(a.emissive, b.emissive);
        assert_eq!(
            app.world_mut()
                .query::<&BoltSegment>()
                .iter(app.world())
                .count(),
            2 * SEGMENTS,
            "only one chain per rail, without overlaid duplicate bolts"
        );
    }

    #[test]
    fn rails_repeat_the_reference_leader_impact_and_fade_with_a_dark_gap() {
        let seed = 11;
        let period = super::super::strike::STRIKE_S + REST_S;
        let start = period - super::super::fx::hash01(seed + 123) * period;
        for age in [0.01, 0.025, 0.06, 0.08, 0.12, 0.3, 0.4] {
            let rail = rail_phase(start + age, seed, false);
            let reference = super::super::strike::strike_phase(age, false);
            assert!((rail.reach - reference.reach).abs() < 0.00001);
            assert!((rail.glow - reference.glow).abs() < 0.00001);
        }
        assert_eq!(rail_phase(start + 0.4, seed, false).glow, 0.0);
        assert!(rail_phase(start + period + 0.06, seed, false).glow > 1.0);
        assert_ne!(
            rail_cycle(1.0, rail_seed(0, -1.0)).1,
            rail_cycle(1.0, rail_seed(0, 1.0)).1
        );
    }

    #[test]
    fn calm_rails_have_no_flash_or_dark_gap_and_zero_intensity_is_off() {
        for i in 0..100 {
            let phase = rail_phase(i as f32 * 0.01, 11, true);
            assert_eq!(phase.reach, 1.0);
            assert_eq!(phase.glow, 1.0);
            assert_eq!(stroke(phase.glow, 11, i, true, 1.0), 1.0);
            assert_eq!(stroke(phase.glow, 11, i, true, 0.0), 0.0);
            assert!(stroke(phase.glow, 11, i, false, 1.0) <= 1.3);
        }
    }

    #[test]
    fn shared_chain_is_pinned_and_changes_only_at_crackle_steps() {
        let origin = Vec3::new(2.0, RAIL_Y, SPAN.0);
        let target = Vec3::new(2.0, RAIL_Y, SPAN.1);
        assert_eq!(chain_point(origin, target, 0, SEGMENTS, 11, 2), origin);
        assert_eq!(
            chain_point(origin, target, SEGMENTS, SEGMENTS, 11, 2),
            target
        );
        let middle = chain_point(origin, target, 20, SEGMENTS, 11, 2);
        assert_eq!(middle, chain_point(origin, target, 20, SEGMENTS, 11, 2));
        assert_ne!(middle, chain_point(origin, target, 20, SEGMENTS, 11, 3));
        assert_eq!(step(1.0, CRACKLE_HZ), 24);
        assert_eq!(step(1.0, CALM_HZ), 2);
    }

    #[test]
    fn a_segment_spans_exactly_its_two_points() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(1.0, 0.0, 2.0);
        let (mid, rot, len) = segment_pose(a, b);
        assert!((len - 2.0).abs() < 1e-6);
        assert!((mid - Vec3::new(1.0, 0.0, 1.0)).length() < 1e-6);
        assert!((rot * Vec3::X - Vec3::Z).length() < 1e-5);
        assert_eq!(segment_pose(a, a).2, 0.0);
    }
}
