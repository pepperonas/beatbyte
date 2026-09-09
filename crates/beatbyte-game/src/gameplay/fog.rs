//! The fog machines: two nozzles that let go a puff now and then.
//!
//! Asked for as "von Zeit zu Zeit soll auch Nebelmaschine etwas Nebel
//! sprühen". A hazer runs all night and gives the beams a body — that
//! is the static haze in [`super::stage3d`] and it is already there.
//! This is the other thing: a machine that fires, throws a slug of fog
//! out over the boards, and goes quiet again.
//!
//! # What it may not do
//!
//! **Never cover the neck.** The highway is a reading surface (see
//! [`super::stage3d::on_the_neck`]) and a cloud drifting over it would
//! be exactly the kind of decoration that costs a player notes. The
//! nozzles sit outboard of the deck's edge, upstage, and the puffs
//! drift further OUT — [`NOZZLE_X`] is checked against the neck's own
//! corridor by a test.
//!
//! # How it is built
//!
//! A fixed pool of billboard quads, additive and unlit, parked
//! invisible. A firing wakes a slice of them; each one rises, spreads,
//! drifts and fades on its own curve. No allocation per frame, no
//! material written per frame, and nothing at all when STAGE MOTION is
//! off — the same contract the crowd and the light show keep.
//!
//! The schedule is a **hash of the firing count**, like the strips':
//! deterministic, so two runs of the same song fog alike, and
//! irregular, so it never reads as a metronome.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::fx::hash01;
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use super::{GameplayScreen, pa};
use crate::audio_sys::GameClock;
use crate::config::Settings;

/// How many puffs one machine can have in the air at once.
pub const PUFFS_PER_NOZZLE: usize = 18;

/// The two nozzles.
pub const NOZZLES: usize = 2;

/// Every quad the pool holds.
pub const POOL: usize = NOZZLES * PUFFS_PER_NOZZLE;

/// Where a nozzle stands, outboard of the deck's edge.
///
/// Outboard on purpose: fog that starts over the boards ends up over
/// the neck, and the neck is a reading surface.
pub const NOZZLE_X: f32 = pa::STACK_X + 1.6;

/// How far upstage they sit — behind the PA, in front of the crowd.
pub const NOZZLE_Z: f32 = -12.0;

/// The nozzle's mouth, above the boards.
pub const NOZZLE_Y: f32 = pa::DECK_TOP + 0.35;

/// The shortest wait between two firings of one machine, seconds.
pub const QUIET_MIN_S: f32 = 26.0;

/// The spread on top of that.
///
/// With the minimum, a machine fires every 26–52 s and the two are
/// out of step, so the room gets fog roughly every quarter minute
/// without a rhythm anybody can count.
pub const QUIET_SPREAD_S: f32 = 26.0;

/// How long one puff lives.
pub const PUFF_LIFE_S: f32 = 7.0;

/// How long a firing keeps releasing, seconds.
pub const BURST_S: f32 = 1.6;

/// How fast a puff rises.
pub const RISE_PER_S: f32 = 0.42;

/// How fast it drifts outward, away from the neck.
pub const DRIFT_PER_S: f32 = 0.55;

/// A puff's size when it leaves the nozzle.
pub const BORN_SIZE: f32 = 1.7;

/// How much it swells over its life.
pub const SWELL: f32 = 5.2;

/// The thickest a puff ever draws.
///
/// Thin: the first cut at 0.16 read as a ball of smoke rather than as
/// fog, because a few dense round sprites are balls however soft
/// their edges. Fog is many faint overlapping ones.
pub const PEAK_ALPHA: f32 = 0.10;

/// How much wider than tall a puff may be drawn.
///
/// A disc sprite is radially symmetric, so spinning it changes
/// nothing; stretching it does. Each puff takes its own stretch, and
/// what was a row of balls becomes a bank of fog.
pub const STRETCH: f32 = 1.7;

/// One quad in the pool.
#[derive(Component, Debug, Clone, Copy)]
pub struct Puff {
    /// Which machine owns it.
    pub nozzle: usize,
    /// Its place in that machine's slice, which sets its delay,
    /// its size and where it drifts.
    pub slot: usize,
    /// When it was released; `None` while it is parked.
    pub born_s: Option<f64>,
}

/// The machines' shared clock.
#[derive(Resource, Debug, Clone, Copy)]
pub struct FogSchedule {
    /// When each machine fires next, in song seconds.
    pub next_at: [f64; NOZZLES],
    /// How many times each has fired, which seeds the next wait.
    pub fired: [u32; NOZZLES],
}

impl Default for FogSchedule {
    fn default() -> Self {
        FogSchedule {
            // Not at zero: the first bars of a song belong to the
            // song, and a machine that fires on the count-in reads as
            // a fault rather than as a cue.
            next_at: [12.0, 30.0],
            fired: [0; NOZZLES],
        }
    }
}

/// How long a machine waits after its `count`-th firing. Pure —
/// tested.
#[must_use]
pub fn quiet_after(nozzle: usize, count: u32) -> f32 {
    QUIET_SPREAD_S.mul_add(
        hash01(nozzle * 7919 + count as usize * 4243 + 17),
        QUIET_MIN_S,
    )
}

/// A puff's age in seconds, or `None` if it has not been released or
/// has already died. Pure — tested.
#[must_use]
pub fn age_of(puff: &Puff, now: f64) -> Option<f32> {
    let born = puff.born_s?;
    let age = (now - born) as f32;
    (0.0..PUFF_LIFE_S).contains(&age).then_some(age)
}

/// How thick a puff draws at `age`.
///
/// It blooms fast and thins slowly — a slug of fog leaves the machine
/// dense and dissolves. Never a hard edge at either end: it fades in
/// over its first tenth and is already at zero when it dies, so a puff
/// is never seen to appear or to blink out. Pure — tested.
#[must_use]
pub fn thickness(age: f32) -> f32 {
    let t = (age / PUFF_LIFE_S).clamp(0.0, 1.0);
    let bloom = (t / 0.12).clamp(0.0, 1.0);
    let fade = (1.0 - t).powf(1.6);
    PEAK_ALPHA * bloom * fade
}

/// Where a puff sits, relative to its nozzle, at `age`.
///
/// Out and up: the machines stand outboard of the deck and throw
/// their fog further out still, so nothing drifts over the neck.
/// Pure — tested.
#[must_use]
pub fn drift(side: f32, slot: usize, age: f32) -> Vec3 {
    let spread = hash01(slot * 6151 + 5) - 0.5;
    let lift = hash01(slot * 3163 + 11).mul_add(0.5, 0.75);
    Vec3::new(
        side.signum() * (DRIFT_PER_S * age + spread * 0.9),
        RISE_PER_S * lift * age,
        (hash01(slot * 2777 + 23) - 0.5).mul_add(2.4, -0.35 * age),
    )
}

/// How big a puff draws at `age`.
#[must_use]
pub fn size_at(slot: usize, age: f32) -> f32 {
    let t = (age / PUFF_LIFE_S).clamp(0.0, 1.0);
    let own = hash01(slot * 8161 + 31).mul_add(0.6, 0.7);
    own * SWELL.mul_add(t, BORN_SIZE)
}

/// The scale a puff draws at: wider than tall, by its own amount.
/// Pure — tested.
#[must_use]
pub fn scale_at(slot: usize, age: f32) -> Vec3 {
    let size = size_at(slot, age);
    let wide = hash01(slot * 4877 + 41).mul_add(STRETCH - 0.9, 0.9);
    Vec3::new(size * wide, size, 1.0)
}

/// When a puff in `slot` is released after its machine fires.
#[must_use]
pub fn release_delay(slot: usize) -> f32 {
    BURST_S * (slot as f32 / PUFFS_PER_NOZZLE as f32)
}

/// Everything the fog machines need.
pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogSchedule>()
            .add_systems(OnEnter(crate::AppState::Gameplay), spawn_fog)
            .add_systems(
                Update,
                drive_fog.run_if(in_state(crate::AppState::Gameplay)),
            );
    }
}

/// Park the pool: every quad exists from the start, invisible.
fn spawn_fog(
    mut commands: Commands,
    settings: Res<Settings>,
    mut schedule: ResMut<FogSchedule>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shapes: Res<crate::shapes::LaneShapes>,
    theme: Res<crate::theme::ActiveTheme>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    *schedule = FogSchedule::default();
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    // The fog wears the room's own tint, faintly: white fog under
    // coloured light is what the beams are for, and a puff that
    // arrives grey reads as smoke rather than as stage fog.
    //
    // One material EACH, because each puff fades on its own clock —
    // a shared handle would make the whole pool breathe as one.
    let tint = theme.0.background.mix(&Color::WHITE, 0.85);
    for nozzle in 0..NOZZLES {
        for slot in 0..PUFFS_PER_NOZZLE {
            let paint = materials.add(StandardMaterial {
                base_color: tint.with_alpha(0.0),
                base_color_texture: Some(shapes.round_body()),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            commands.spawn((
                GameplayScreen,
                Stage3d,
                NotShadowCaster,
                Puff {
                    nozzle,
                    slot,
                    born_s: None,
                },
                Mesh3d(quad.clone()),
                MeshMaterial3d(paint),
                Transform::from_xyz(0.0, -50.0, 0.0),
                Visibility::Hidden,
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }
}

/// Fire the machines and carry every live puff.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn drive_fog(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut schedule: ResMut<FogSchedule>,
    mut puffs: Query<(
        &mut Puff,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !stage3d::active(&settings) || !settings.backdrop_motion {
        return;
    }
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    // The song clock steps BACK at the count-in handover and again,
    // by a fraction, whenever it corrects itself against the device.
    // A schedule that treated either as a new song would fire on
    // every correction; one that treated a new song as a correction
    // would never fire again. The gap between firings is the measure.
    for nozzle in 0..NOZZLES {
        if now + f64::from(QUIET_MIN_S) < schedule.next_at[nozzle] {
            schedule.next_at[nozzle] = now + 4.0;
        }
        if now >= schedule.next_at[nozzle] {
            let count = schedule.fired[nozzle];
            schedule.fired[nozzle] = count.wrapping_add(1);
            schedule.next_at[nozzle] = now + f64::from(quiet_after(nozzle, count));
            for (mut puff, _, _, _) in &mut puffs {
                if puff.nozzle == nozzle {
                    puff.born_s = Some(now + f64::from(release_delay(puff.slot)));
                }
            }
        }
    }

    // The stage camera never moves, so the billboard turns toward a
    // constant rather than toward a queried transform — one fewer
    // query, and no chance of facing the menu camera by mistake.
    let eye = stage3d::CAMERA_POS;
    for (mut puff, mut transform, mut visibility, material) in &mut puffs {
        let Some(age) = age_of(&puff, now) else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            // A puff whose life ran out is parked, so the next firing
            // finds it free rather than skipping it.
            if puff
                .born_s
                .is_some_and(|born| (now - born) as f32 >= PUFF_LIFE_S)
            {
                puff.born_s = None;
            }
            continue;
        };
        let side = if puff.nozzle == 0 { -1.0 } else { 1.0 };
        let home = Vec3::new(side * NOZZLE_X, NOZZLE_Y, NOZZLE_Z);
        let at = home + drift(side, puff.slot, age);
        transform.translation = at;
        transform.scale = scale_at(puff.slot, age);
        // Billboarded, because a flat quad seen edge-on is a line.
        let away = (at - eye).with_y(0.0);
        if away.length_squared() > 1e-4 {
            transform.rotation = Quat::from_rotation_y(away.x.atan2(away.z));
        }
        if *visibility != Visibility::Visible {
            *visibility = Visibility::Visible;
        }
        if let Some(mut paint) = materials.get_mut(&material.0) {
            let alpha = thickness(age);
            if (paint.base_color.alpha() - alpha).abs() > 1e-4 {
                paint.base_color.set_alpha(alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nozzles_stand_clear_of_the_neck_and_blow_outward() {
        // The rule the whole module is built around: fog may never
        // drift over the highway.
        const { assert!(NOZZLE_X > stage3d::DECK_WIDTH / 2.0 - 1.0) } // clear of the boards
        for side in [-1.0f32, 1.0] {
            let early = drift(side, 3, 0.5);
            let late = drift(side, 3, PUFF_LIFE_S - 0.1);
            // Signed, in the nozzle's OWN direction. Comparing
            // magnitudes was not enough: a probe that sent the fog
            // inward still passed it, because a puff moving toward
            // the neck also grows |x| once it is past the nozzle.
            assert!(
                late.x * side > early.x * side,
                "the puff must travel outward, away from the neck: {} then {}",
                early.x,
                late.x
            );
            let home = side * NOZZLE_X;
            assert!(
                (home + late.x) * side > home * side,
                "a puff ended inboard of its own machine"
            );
            assert!(
                (home + late.x).abs() > stage3d::DECK_WIDTH / 2.0 - 1.0,
                "a puff drifted in over the boards"
            );
            assert!(late.y > early.y, "and rise");
        }
    }

    #[test]
    fn a_puff_is_never_seen_to_appear_or_to_blink_out() {
        assert!(thickness(0.0).abs() < 1e-6, "it fades in");
        assert!(thickness(PUFF_LIFE_S).abs() < 1e-6, "and out");
        assert!(
            thickness(PUFF_LIFE_S - 0.01) < 0.01,
            "with nothing left at the end"
        );
        // Dense early, thinning late: a slug of fog, not a cloud that
        // grows.
        let peak = (0..70)
            .map(|i| thickness(i as f32 * 0.1))
            .fold(0.0f32, f32::max);
        assert!(peak <= PEAK_ALPHA + 1e-6, "never thicker than its cap");
        assert!(thickness(1.2) > thickness(5.0), "it thins as it ages");
        // Thin enough to be atmosphere: a puff that hides the stage
        // behind it is not fog.
        const { assert!(PEAK_ALPHA < 0.25) } // a veil, not a curtain
    }

    #[test]
    fn a_puff_grows_and_then_is_parked() {
        assert!(size_at(2, 0.0) < size_at(2, PUFF_LIFE_S), "fog spreads");
        // Wider than tall, and not all alike: round sprites of one
        // size read as balls however soft their edges.
        let shapes: Vec<Vec3> = (0..PUFFS_PER_NOZZLE).map(|s| scale_at(s, 2.0)).collect();
        for shape in &shapes {
            assert!(shape.x > shape.y * 0.85, "a puff lies down: {shape:?}");
        }
        let widest = shapes.iter().fold(0.0f32, |a, s| a.max(s.x / s.y));
        let narrowest = shapes.iter().fold(f32::MAX, |a, s| a.min(s.x / s.y));
        assert!(
            widest - narrowest > 0.3,
            "every puff the same shape: {narrowest}..{widest}"
        );
        let mut puff = Puff {
            nozzle: 0,
            slot: 0,
            born_s: Some(100.0),
        };
        assert!(age_of(&puff, 100.5).is_some(), "alive just after release");
        assert!(
            age_of(&puff, 99.5).is_none(),
            "a puff released in the future is not on stage yet"
        );
        assert!(
            age_of(&puff, 100.0 + f64::from(PUFF_LIFE_S) + 0.1).is_none(),
            "and it dies"
        );
        puff.born_s = None;
        assert!(
            age_of(&puff, 100.0).is_none(),
            "a parked puff draws nothing"
        );
    }

    #[test]
    fn the_machines_fire_now_and_then_rather_than_on_a_count() {
        // The ask was "von Zeit zu Zeit", which is neither a metronome
        // nor a one-off.
        let waits: Vec<f32> = (0..24).map(|c| quiet_after(0, c)).collect();
        for wait in &waits {
            assert!(
                (QUIET_MIN_S..=QUIET_MIN_S + QUIET_SPREAD_S).contains(wait),
                "a wait of {wait} s is outside the range the room was tuned for"
            );
        }
        let shortest = waits.iter().copied().fold(f32::MAX, f32::min);
        let longest = waits.iter().copied().fold(0.0f32, f32::max);
        assert!(
            longest - shortest > QUIET_SPREAD_S * 0.4,
            "the waits barely differ: {shortest} to {longest} reads as a count"
        );
        // The two machines are out of step, or the room gets one big
        // cloud instead of fog drifting from both sides.
        let together = (0..12).filter(|c| (quiet_after(0, *c) - quiet_after(1, *c)).abs() < 0.5);
        assert!(together.count() < 4, "the nozzles fire in lockstep");
        // Deterministic: the same song fogs the same way twice.
        assert!((quiet_after(1, 7) - quiet_after(1, 7)).abs() < f32::EPSILON);
    }

    #[test]
    fn a_firing_lets_go_over_time_rather_than_all_at_once() {
        let mut delays: Vec<f32> = (0..PUFFS_PER_NOZZLE).map(release_delay).collect();
        assert!((delays[0]).abs() < 1e-6, "the first puff is immediate");
        let last = delays.pop().expect("a pool holds puffs");
        assert!(last > 0.5, "the burst has a length: {last}");
        assert!(last < BURST_S + 1e-6, "and ends when the burst does");
    }
}
