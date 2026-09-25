//! The lightning strike when a star-power phrase lands whole.
//!
//! On the 3D stage the full-screen white flash is gone; in its place
//! a bolt comes down out of the dark above the player's own highway
//! and hits the last note of the phrase on the hit line — one arm per
//! fret when that note is a chord. Core near white, a broad blue
//! hull, jagged and re-rolled every crackle step, the same colour and
//! the same bolt vocabulary as the edge arc in [`super::arc`], whose
//! pure pieces (`point`, `segment_pose`, `flash`, the core + hull
//! materials) this module reuses rather than copies.
//!
//! # Where the trigger comes from
//!
//! [`beatbyte_core::SessionEvent::PhraseCompleted`], read off the
//! feedback bus exactly as [`super::starpower::arm`] reads it. The
//! event carries only the phrase's index; the fret is looked up from
//! the player's own track ([`strike_targets`]): the latest event
//! inside the phrase, all of its lanes. Nothing here knows what a
//! phrase is or when it counts — no star-power logic lives in a
//! visual system.
//!
//! # Time, not frames
//!
//! [`strike_phase`] is a function of the strike's age: the leader
//! reaches down from the origin to the fret over [`BUILD_S`], the
//! impact flares over [`IMPACT_S`] (the bolt at its thickest, a point
//! light on the fret), and everything fades over [`FADE_S`]. The whole
//! thing is over inside [`STRIKE_S`], before the neck's own glow from
//! the impulse has settled. A second phrase on the same fret restarts
//! the age; two players are two slots.
//!
//! # Pools
//!
//! Five bolts per player, one per lane, each a chain of
//! [`STRIKE_SEGMENTS`] core segments with a hull child, plus one
//! point light per player, all spawned once with the stage. Per
//! frame: transforms, visibility and a light's intensity. No
//! allocation, no material writes. Everything additive is
//! `NotShadowCaster` through `on_the_neck`.
//!
//! `reduced_flashing`: a steady bolt (no crackle) and a soft glow
//! on the fret, no impact flare. `fx_intensity` scales thickness and
//! light; at zero there is no strike.
//!
//! The flat view has no meshes and no lights; it keeps the screen
//! flash it always had (`fx::flash_on_star_power`).

use beatbyte_core::{Lane, LaneSet, NoteEvent, Phrase, SessionEvent};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::arc::{
    CALM_HZ, CORE, CORE_GLOW, CRACKLE_HZ, HULL, HULL_GLOW, bolt_material, flash, point,
    segment_pose, step,
};
use super::stage3d::{STAGE_LAYER, Stage3d, lane_x, rail_x};
use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, SessionFeedback};
use crate::config::Settings;
use crate::states::AppState;

/// Segments in one strike's chain, origin to fret.
pub const STRIKE_SEGMENTS: usize = 14;
/// The leader's descent: how long the chain takes to reach the fret.
pub const BUILD_S: f32 = 0.05;
/// The impact flare.
pub const IMPACT_S: f32 = 0.04;
/// The fade after it.
pub const FADE_S: f32 = 0.26;
/// The whole strike, start to nothing.
pub const STRIKE_S: f32 = BUILD_S + IMPACT_S + FADE_S;
/// Where the bolt comes from: this high above the neck…
const ORIGIN_Y: f32 = 3.2;
/// …and this far up the neck from the hit line (negative z is away
/// from the player), so the strike comes down diagonally.
const ORIGIN_Z: f32 = -3.6;
/// How far the origin may wander sideways from the fret it hits.
const ORIGIN_WANDER: f32 = 0.9;
/// The origin never comes closer than this to the player's own rail
/// — and so never stands over a neighbour's neck.
const ORIGIN_MARGIN: f32 = 0.15;
/// Where on the fret the bolt lands.
const FRET_Y: f32 = 0.05;
/// Sideways jitter of a chain point, world units, at the chain's
/// middle (the ends are pinned).
const JITTER: f32 = 0.55;
/// The core segment's thickness at glow 1.
const THICKNESS: f32 = 0.028;
/// The strike's hull, thinner than the edge arc's: a bolt this long
/// reads as a ribbon past that.
const STRIKE_HULL_FACTOR: f32 = 2.4;
/// The crackle factor is capped: the flare is the phase's job.
const CRACKLE_CAP: f32 = 1.3;
/// The fret light at full impact, in Bevy's point-light lumens.
const LIGHT_LUMENS: f32 = 900_000.0;
/// The age a slot that is not striking carries.
const IDLE: f32 = f32::MAX;

/// The strike's shape at `age` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phase {
    /// How far down the chain is drawn, 0 (nothing) to 1 (on the fret).
    pub reach: f32,
    /// The bolt's thickness factor.
    pub glow: f32,
    /// The fret light and the flare, 0..1.
    pub impact: f32,
}

/// Nothing at all.
const NONE: Phase = Phase {
    reach: 0.0,
    glow: 0.0,
    impact: 0.0,
};

/// What the strike is doing `age` seconds in. Past [`STRIKE_S`] (and
/// before zero) it is nothing. Under reduced flashing the impact is
/// a soft glow, never a flare. Pure — tested.
#[must_use]
pub fn strike_phase(age: f32, calm: bool) -> Phase {
    if !(0.0..STRIKE_S).contains(&age) {
        return NONE;
    }
    if age < BUILD_S {
        return Phase {
            reach: age / BUILD_S,
            glow: 0.7,
            impact: 0.0,
        };
    }
    let flare_peak = if calm { 1.0 } else { 1.4 };
    let light_peak = if calm { 0.35 } else { 1.0 };
    if age < BUILD_S + IMPACT_S {
        let t = (age - BUILD_S) / IMPACT_S;
        return Phase {
            reach: 1.0,
            glow: flare_peak,
            impact: light_peak * (1.0 - 0.3 * t),
        };
    }
    let t = (age - BUILD_S - IMPACT_S) / FADE_S;
    let fall = (1.0 - t) * (1.0 - t);
    Phase {
        reach: 1.0,
        glow: fall,
        impact: light_peak * 0.7 * fall,
    }
}

/// The lanes the strike hits for `phrase`: the latest event inside
/// it — a chord gives every one of its lanes. Empty when the phrase
/// holds no event (a strike needs a fret). Pure — tested.
#[must_use]
pub fn strike_targets(events: &[NoteEvent], phrase: &Phrase) -> LaneSet {
    events
        .iter()
        .filter(|event| phrase.contains(event.time_s))
        .max_by(|a, b| a.time_s.total_cmp(&b.time_s))
        .map_or(LaneSet::EMPTY, |event| event.lanes)
}

/// Where the bolt for `lane` comes from: high above this player's own
/// neck, wandering sideways by `wander` (−1..1) but never past their
/// rails — so in a duet it never stands over the other neck. Pure —
/// tested.
#[must_use]
pub fn strike_origin(layout: &HighwayLayout, player: usize, lane: Lane, wander: f32) -> Vec3 {
    let x = lane_x(layout, player, lane) + wander.clamp(-1.0, 1.0) * ORIGIN_WANDER;
    let left = rail_x(layout, player, -1.0) + ORIGIN_MARGIN;
    let right = rail_x(layout, player, 1.0) - ORIGIN_MARGIN;
    Vec3::new(x.clamp(left, right), ORIGIN_Y, ORIGIN_Z)
}

/// Where the bolt lands: the fret on the hit line.
#[must_use]
pub fn strike_target(layout: &HighwayLayout, player: usize, lane: Lane) -> Vec3 {
    Vec3::new(lane_x(layout, player, lane), FRET_Y, 0.0)
}

/// Chain point `index` (0 = origin, [`STRIKE_SEGMENTS`] = the fret)
/// at crackle `step`: the straight line, jagged sideways by the
/// arc's own [`point`] hash, the jitter widest in the middle and
/// exactly zero at both ends — the bolt ends ON the fret. Pure —
/// tested.
#[must_use]
pub fn chain_point(origin: Vec3, target: Vec3, index: usize, seed: usize, step: u32) -> Vec3 {
    let along = index as f32 / STRIKE_SEGMENTS as f32;
    let base = origin.lerp(target, along);
    // A bump that is 0 at both ends and 1 in the middle.
    let bump = 4.0 * along * (1.0 - along);
    let (dx, dy) = point(seed, index, step);
    // `point` gives dx in ±JITTER_X and dy in the arc's height range;
    // centre dy so it jitters both ways too.
    let sideways = dx / super::arc::JITTER_X * JITTER;
    let across = (dy - 0.19) / 0.15 * JITTER * 0.6;
    // Sideways in x; "across" perpendicular to the bolt in the y–z
    // plane it descends through.
    let direction = (target - origin).normalize_or_zero();
    let perpendicular = Vec3::new(0.0, direction.z, -direction.y).normalize_or_zero();
    base + Vec3::X * (sideways * bump) + perpendicular * (across * bump)
}

/// The strikes running right now: an age per player per lane.
#[derive(Resource, Debug, Default)]
pub struct Strikes {
    ages: Vec<[f32; 5]>,
    /// Per player: which strike this is (seeds the shape and the
    /// origin's wander).
    counts: Vec<u32>,
}

impl Strikes {
    /// Start a strike on every lane in `lanes` for `player`.
    pub fn fire(&mut self, player: usize, lanes: LaneSet) {
        if self.ages.len() <= player {
            self.ages.resize(player + 1, [IDLE; 5]);
            self.counts.resize(player + 1, 0);
        }
        for lane in lanes.iter() {
            self.ages[player][lane.index()] = 0.0;
        }
        self.counts[player] = self.counts[player].wrapping_add(1);
    }

    /// Age every running strike.
    pub fn advance(&mut self, dt: f32) {
        for ages in &mut self.ages {
            for age in ages {
                if *age < STRIKE_S {
                    *age = (*age + dt).min(IDLE);
                }
            }
        }
    }

    /// The age of `player`'s strike on `lane`, or idle.
    #[must_use]
    pub fn age(&self, player: usize, lane: Lane) -> f32 {
        self.ages
            .get(player)
            .map_or(IDLE, |ages| ages[lane.index()])
    }

    /// The strike count of `player` (a seed).
    #[must_use]
    pub fn count(&self, player: usize) -> u32 {
        self.counts.get(player).copied().unwrap_or(0)
    }

    /// Whether anything is running.
    #[must_use]
    pub fn running(&self) -> bool {
        self.ages
            .iter()
            .any(|ages| ages.iter().any(|age| *age < STRIKE_S))
    }

    /// Nothing running.
    pub fn clear(&mut self) {
        self.ages.clear();
        self.counts.clear();
    }
}

/// One segment of a strike's chain.
#[derive(Component)]
pub struct StrikeSegment {
    /// Owning player.
    pub player: usize,
    /// The lane it strikes.
    pub lane: Lane,
    /// Position in the chain.
    pub segment: usize,
}

/// The fret light of a player's strikes.
#[derive(Component)]
pub struct StrikeLight {
    /// Owning player.
    pub player: usize,
}

/// Arm strikes when a phrase lands whole: the fret comes from the
/// player's own track. Runs after the drain, like the impulse.
pub fn arm(
    mut strikes: ResMut<Strikes>,
    mut feedback: MessageReader<SessionFeedback>,
    players: Query<&PlayerSession>,
) {
    for message in feedback.read() {
        let SessionEvent::PhraseCompleted { phrase_index } = message.event else {
            continue;
        };
        let Ok(player) = players.get(message.player) else {
            continue;
        };
        let track = player.session.track();
        let Some(phrase) = track.phrases().get(phrase_index) else {
            continue;
        };
        let lanes = strike_targets(track.events(), phrase);
        if lanes != LaneSet::EMPTY {
            strikes.fire(message.player_index, lanes);
        }
    }
}

/// Advance the ages (in `Gameplay`, so a pause finishes the strike
/// rather than freezing a bolt over the menu).
pub fn advance(mut strikes: ResMut<Strikes>, time: Res<Time>) {
    if strikes.running() {
        strikes.advance(time.delta_secs());
    }
}

/// Nothing left behind when gameplay ends or begins.
pub fn reset(mut strikes: ResMut<Strikes>) {
    strikes.clear();
}

/// Spawn the strike pools. Instrument neck only.
pub fn spawn_strikes(
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
    let segment = meshes.add(Cuboid::new(1.0, THICKNESS, THICKNESS));
    let hull_mesh = meshes.add(Cuboid::new(
        1.0,
        THICKNESS * STRIKE_HULL_FACTOR,
        THICKNESS * STRIKE_HULL_FACTOR,
    ));
    let core = bolt_material(&mut materials, CORE, CORE_GLOW);
    let hull = bolt_material(&mut materials, HULL, HULL_GLOW);
    for index in &players {
        let player = index.0;
        for lane in Lane::ALL {
            let x = lane_x(&layout, player, lane);
            for seg in 0..STRIKE_SEGMENTS {
                commands
                    .spawn((
                        GameplayScreen,
                        Stage3d,
                        StrikeSegment {
                            player,
                            lane,
                            segment: seg,
                        },
                        super::stage3d::on_the_neck(),
                        Mesh3d(segment.clone()),
                        MeshMaterial3d(core.clone()),
                        Transform::from_xyz(x, 1.0, -1.0).with_scale(Vec3::ZERO),
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
        commands.spawn((
            GameplayScreen,
            Stage3d,
            StrikeLight { player },
            PointLight {
                color: CORE,
                intensity: 0.0,
                range: 2.4,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(lane_x(&layout, player, Lane::Three), 0.4, 0.0),
            layer.clone(),
        ));
    }
}

/// Draw the running strikes. Transforms, visibility, one light per
/// player.
#[allow(clippy::type_complexity)]
pub fn drive_strikes(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    strikes: Res<Strikes>,
    mut segments: Query<(&StrikeSegment, &mut Transform, &mut Visibility), Without<StrikeLight>>,
    mut lights: Query<(&StrikeLight, &mut Transform, &mut PointLight), Without<StrikeSegment>>,
) {
    let calm = settings.reduced_flashing;
    let intensity = settings.fx_intensity.clamp(0.0, 1.0);
    let now = time.elapsed_secs();
    let step = step(now, if calm { CALM_HZ } else { CRACKLE_HZ });

    for (seg, mut transform, mut visibility) in &mut segments {
        let age = strikes.age(seg.player, seg.lane);
        let phase = strike_phase(age, calm);
        let drawn = phase.reach * STRIKE_SEGMENTS as f32;
        let live = intensity > 0.0 && phase.glow > 0.0 && (seg.segment as f32) < drawn;
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
        let count = strikes.count(seg.player) as usize;
        let seed = seg.player * 8191 + seg.lane.index() * 613 + count * 97;
        let wander = super::fx::hash01(seed + 5) * 2.0 - 1.0;
        let origin = strike_origin(&layout, seg.player, seg.lane, wander);
        let target = strike_target(&layout, seg.player, seg.lane);
        let a = chain_point(origin, target, seg.segment, seed, step);
        let b = chain_point(origin, target, seg.segment + 1, seed, step);
        let (mid, rot, len) = segment_pose(a, b);
        let thick = phase.glow * flash(seed, step, calm).min(CRACKLE_CAP) * intensity;
        transform.translation = mid;
        transform.rotation = rot;
        transform.scale = Vec3::new(len, thick, thick);
    }

    for (light, mut transform, mut point_light) in &mut lights {
        // The brightest strike of this player lights its fret.
        let mut best = (0.0f32, Lane::Three);
        for lane in Lane::ALL {
            let impact = strike_phase(strikes.age(light.player, lane), calm).impact;
            if impact > best.0 {
                best = (impact, lane);
            }
        }
        let lumens = best.0 * intensity * LIGHT_LUMENS;
        if (point_light.intensity - lumens).abs() > 1.0 {
            point_light.intensity = lumens;
            transform.translation.x = lane_x(&layout, light.player, best.1);
        }
    }
}

/// Wire the strike into the app: the clock beside the impulse's, the
/// pools and the drawing with the stage.
pub fn register(app: &mut App) {
    app.init_resource::<Strikes>()
        .add_systems(
            Update,
            (arm.after(super::drain_feedback), advance)
                .chain()
                .run_if(in_state(AppState::Gameplay)),
        )
        .add_systems(OnExit(AppState::Gameplay), reset)
        .add_systems(OnEnter(AppState::Gameplay), reset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::NoteEvent;

    fn phrase(start_s: f64, end_s: f64) -> Phrase {
        Phrase { start_s, end_s }
    }

    /// The fret is the LAST event inside the phrase, all of its lanes.
    #[test]
    fn the_strike_hits_the_last_note_of_the_phrase_every_lane_of_a_chord() {
        let events = [
            NoteEvent::tap(0.5, LaneSet::single(Lane::One)),
            NoteEvent::tap(1.0, LaneSet::single(Lane::Two)),
            NoteEvent::tap(1.5, LaneSet::from_lanes([Lane::Three, Lane::Five])),
            NoteEvent::tap(2.5, LaneSet::single(Lane::Four)),
        ];
        // The phrase covers 1.0..2.0: the chord at 1.5 is its last.
        let chord = strike_targets(&events, &phrase(1.0, 2.0));
        assert_eq!(chord, LaneSet::from_lanes([Lane::Three, Lane::Five]));
        assert_eq!(
            strike_targets(&events, &phrase(0.0, 1.2)),
            LaneSet::single(Lane::Two),
            "not the first, the last"
        );
        assert_eq!(strike_targets(&events, &phrase(3.0, 4.0)), LaneSet::EMPTY);
        assert_eq!(strike_targets(&[], &phrase(0.0, 9.0)), LaneSet::EMPTY);
    }

    /// The origin stands over the player's own neck whatever the
    /// wander — in a duet never over the other's.
    #[test]
    fn the_bolt_comes_from_above_the_players_own_neck() {
        let layout = HighwayLayout::for_players(2);
        for player in 0..2 {
            let left = rail_x(&layout, player, -1.0);
            let right = rail_x(&layout, player, 1.0);
            for lane in Lane::ALL {
                for wander in [-1.0f32, -0.3, 0.0, 0.7, 1.0, 5.0] {
                    let origin = strike_origin(&layout, player, lane, wander);
                    assert!(
                        origin.x > left && origin.x < right,
                        "player {player} lane {lane:?} wander {wander}: x {} outside {left}..{right}",
                        origin.x
                    );
                    assert!(origin.y > 2.0, "from above");
                    assert!(origin.z < -1.0, "from up the neck");
                }
            }
            let other = 1 - player;
            let other_left = rail_x(&layout, other, -1.0);
            let other_right = rail_x(&layout, other, 1.0);
            let origin = strike_origin(&layout, player, Lane::Three, 0.0);
            assert!(origin.x < other_left || origin.x > other_right);
        }
        // The wander moves the origin: it is not one fixed point.
        let layout = HighwayLayout::for_players(1);
        assert!(
            (strike_origin(&layout, 0, Lane::Three, -1.0).x
                - strike_origin(&layout, 0, Lane::Three, 1.0).x)
                .abs()
                > 0.5
        );
    }

    /// The chain starts at the origin and ends ON the fret, whatever
    /// the step; in between it is jagged, and it changes between steps.
    #[test]
    fn the_chain_is_pinned_at_both_ends_and_jagged_between() {
        let origin = Vec3::new(1.0, 3.2, -3.6);
        let target = Vec3::new(0.4, 0.05, 0.0);
        for step in 0..30 {
            assert!((chain_point(origin, target, 0, 3, step) - origin).length() < 1e-6);
            assert!(
                (chain_point(origin, target, STRIKE_SEGMENTS, 3, step) - target).length() < 1e-6,
                "ends exactly on the fret"
            );
        }
        let straight = origin.lerp(target, 0.5);
        let mid = chain_point(origin, target, STRIKE_SEGMENTS / 2, 3, 4);
        assert!((mid - straight).length() > 0.05, "jagged");
        assert!(
            (mid - straight).length() < JITTER * 1.2,
            "but near the line"
        );
        assert_ne!(
            chain_point(origin, target, 5, 3, 4),
            chain_point(origin, target, 5, 3, 5),
            "re-rolled per step"
        );
        assert_eq!(
            chain_point(origin, target, 5, 3, 4),
            chain_point(origin, target, 5, 3, 4),
            "steady within a step"
        );
    }

    /// The phase: the leader reaches the fret by the end of the
    /// build, the impact flares after — not before — and everything
    /// is gone by the end, which is before the impulse's neck glow.
    #[test]
    fn the_strike_builds_flares_fades_and_ends_before_the_impulse() {
        assert_eq!(strike_phase(-0.1, false), NONE);
        assert_eq!(strike_phase(STRIKE_S, false), NONE);
        assert_eq!(strike_phase(9.0, false), NONE);
        const { assert!(STRIKE_S < super::super::starpower::IMPULSE_S) };
        let early = strike_phase(BUILD_S * 0.4, false);
        assert!(early.reach > 0.3 && early.reach < 0.5, "on its way down");
        assert!(early.impact == 0.0, "no impact before it lands");
        let landed = strike_phase(BUILD_S + 0.005, false);
        assert!((landed.reach - 1.0).abs() < 1e-6);
        assert!(landed.impact > 0.9 && landed.glow > 1.2, "the flare");
        let late = strike_phase(BUILD_S + IMPACT_S + FADE_S * 0.5, false);
        assert!(late.glow > 0.0 && late.glow < landed.glow);
        assert!(late.impact > 0.0 && late.impact < landed.impact);
        // Monotone fade.
        let mut last = f32::MAX;
        for i in 0..20 {
            let age = BUILD_S + IMPACT_S + FADE_S * i as f32 / 20.0;
            let glow = strike_phase(age, false).glow;
            assert!(glow <= last);
            last = glow;
        }
        // Calm: it still lands, but there is no flare — a soft glow.
        let calm = strike_phase(BUILD_S + 0.005, true);
        assert!((calm.reach - 1.0).abs() < 1e-6);
        assert!(calm.impact <= 0.4 && calm.impact > 0.0, "soft: {calm:?}");
        assert!(calm.glow <= 1.0, "no thickness spike");
    }

    /// Two players and two phrases in a row keep to themselves: a
    /// restart resets the age, the other player's slot is untouched,
    /// and a finished strike is idle again.
    #[test]
    fn strikes_keep_their_slots_apart() {
        let mut strikes = Strikes::default();
        assert!(!strikes.running());
        strikes.fire(1, LaneSet::from_lanes([Lane::One, Lane::Four]));
        assert_eq!(strikes.age(1, Lane::One), 0.0);
        assert_eq!(strikes.age(1, Lane::Two), IDLE);
        assert_eq!(strikes.age(0, Lane::One), IDLE, "the other player");
        strikes.advance(0.2);
        assert!((strikes.age(1, Lane::Four) - 0.2).abs() < 1e-6);
        strikes.fire(1, LaneSet::single(Lane::Four));
        assert_eq!(strikes.age(1, Lane::Four), 0.0, "restarted");
        assert!(
            (strikes.age(1, Lane::One) - 0.2).abs() < 1e-6,
            "the other lane runs on"
        );
        assert_eq!(strikes.count(1), 2);
        strikes.advance(STRIKE_S);
        assert!(!strikes.running());
        assert!(strikes.age(1, Lane::One) >= STRIKE_S, "over");
        assert_eq!(strike_phase(strikes.age(1, Lane::One), false), NONE);
        strikes.clear();
        assert_eq!(strikes.count(1), 0);
    }
}
