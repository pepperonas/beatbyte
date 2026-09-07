//! The light show the room's own level drives: a **highlight** on
//! the stage lighting when the measured level crosses a threshold
//! that sets itself, and three white **light strips** — stage,
//! audience, PA — that now and then run a comet or glimmer.
//!
//! The level comes from [`super::monitors::Ears`], the same input
//! the monitors show; nothing here reads the chart, and without a
//! measurement (no input, refused, silent) the highlight simply
//! never fires while the strips go on glimmering: they are decor,
//! not a readout.
//!
//! **The threshold is dynamic**, the way the reference rig's
//! dB-Analyse sets its own (`disco-controller/auto_thr.py`, the duty
//! governor): the share of time the level sits above the threshold
//! is measured over a short window and the threshold is stepped —
//! up quickly, down at half the rate — toward a target share, one
//! step per interval, never below the noise floor plus a margin,
//! frozen while there is no music. The bit itself is live
//! (`level > threshold`, no hysteresis), as the reference deliberately
//! keeps it; the highlight fires on its rising edge.
//!
//! **The highlight** is an envelope on every stage `SpotLight`: a
//! punch to 1 on the edge, decaying over ~0.35 s, applied as a gain
//! on the light's own intensity (remembered the first time it is
//! seen — no fixture is retuned). Under `reduced_flashing` the punch
//! is a swell: a third as strong, twice as slow.
//!
//! **The strips** are pools of thin additive bars along a line,
//! driven by visibility and scale alone (the arc's pattern: one
//! material, no writes per frame): a comet is a bright head running
//! the strip with an exponential tail, a glimmer is a second of
//! random sparkle re-rolled 24 times a second (4 under reduced
//! flashing, and no comet is a flash to begin with). Every strip
//! fires on its own schedule, 9–18 s apart, from a hash of the strip
//! and the firing count — sporadic and deterministic. STAGE MOTION
//! off keeps every strip dark and writes nothing.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use beatbyte_audio::listen::LOUD_DB;

use super::GameplayScreen;
use super::arc::segment_pose;
use super::crowd::{BARRIER_X, CROWD_FLOOR_Y};
use super::fx::hash01;
use super::monitors::Ears;
use super::pa;
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use crate::config::Settings;
use crate::states::AppState;

// ---------------------------------------------------------------- threshold

/// The share of music time the level should sit above the threshold.
pub const DUTY_TARGET: f32 = 0.35;
/// No step while the measured share is this close to the target.
pub const DUTY_DEADBAND: f32 = 0.08;
/// dB per interval the threshold climbs when the level is over it
/// too often.
pub const STEP_UP_DB: f32 = 0.9;
/// dB per interval it falls — half as fast, the reference's choice:
/// coming back after a loud passage should be gentle.
pub const STEP_DOWN_DB: f32 = 0.45;
/// One step per this many seconds.
pub const STEP_INTERVAL_S: f32 = 0.5;
/// The duty window, seconds.
pub const DUTY_WINDOW_S: f32 = 2.0;
/// The threshold never sits closer than this to the noise floor.
pub const FLOOR_MARGIN_DB: f32 = 6.0;
/// The noise floor's climb rate, dB/s (it falls at once).
pub const FLOOR_RISE_DB_PER_S: f32 = 0.02;
/// Where the threshold starts, dBFS, before the music has taught it.
pub const THRESHOLD_START_DB: f32 = -30.0;

/// The self-setting threshold. Pure — tested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DutyThreshold {
    /// The threshold, dBFS.
    pub threshold: f32,
    /// The noise floor, dBFS.
    pub noise_floor: f32,
    /// The share of recent music time over the threshold (eased).
    pub duty: f32,
    last_step: f32,
    last_tick: Option<f32>,
}

impl Default for DutyThreshold {
    fn default() -> Self {
        DutyThreshold {
            threshold: THRESHOLD_START_DB,
            noise_floor: 0.0,
            duty: 0.0,
            last_step: -1e9,
            last_tick: None,
        }
    }
}

impl DutyThreshold {
    /// Feed the level at `now` seconds; returns whether the level is
    /// over the threshold RIGHT NOW (the live bit).
    pub fn tick(&mut self, now: f32, db: f32) -> bool {
        let dt = self
            .last_tick
            .map_or(0.0, |last| (now - last).clamp(0.0, 1.0));
        self.last_tick = Some(now);
        // The floor: falls at once, climbs slowly.
        if db < self.noise_floor {
            self.noise_floor = db;
        } else {
            self.noise_floor += FLOOR_RISE_DB_PER_S * dt;
        }
        let over = db > self.threshold;
        let music = db > LOUD_DB;
        if !music {
            // Silence teaches nothing: the governor freezes.
            return over;
        }
        // The duty as an ease over the window.
        let target = if over { 1.0 } else { 0.0 };
        self.duty += (target - self.duty) * (dt / DUTY_WINDOW_S).min(1.0);
        let error = self.duty - DUTY_TARGET;
        if error.abs() <= DUTY_DEADBAND || now - self.last_step < STEP_INTERVAL_S {
            return over;
        }
        self.last_step = now;
        let floor = self.noise_floor + FLOOR_MARGIN_DB;
        self.threshold = (self.threshold + step_db(error)).max(floor).min(0.0);
        over
    }
}

/// The step for a duty error (measured share − target): up by
/// [`STEP_UP_DB`] when over too often, down by [`STEP_DOWN_DB`] —
/// half of it — when under, scaled toward zero inside a second
/// deadband. Pure — tested.
#[must_use]
pub fn step_db(error: f32) -> f32 {
    let gain = ((error.abs() - DUTY_DEADBAND) / DUTY_DEADBAND).clamp(0.0, 1.0);
    if error > 0.0 {
        STEP_UP_DB * gain
    } else {
        -STEP_DOWN_DB * gain
    }
}

// ---------------------------------------------------------------- highlight

/// How much brighter a highlighted lamp gets at full punch.
pub const HIGHLIGHT_GAIN: f32 = 0.9;
/// The punch's decay, per second.
pub const PUNCH_DECAY_PER_S: f32 = 2.8;
/// Under reduced flashing: a swell instead of a flash.
pub const CALM_PUNCH: f32 = 0.35;
/// ... that also decays at a quarter of the rate, so the swell
/// lingers longer than the flash would.
pub const CALM_DECAY_PER_S: f32 = 0.7;

/// The highlight's state: the threshold and the envelope.
#[derive(Resource, Debug, Default)]
pub struct Highlight {
    /// The threshold governor.
    pub threshold: DutyThreshold,
    /// The live bit from the last tick.
    pub over: bool,
    /// The envelope, 0..1.
    pub punch: f32,
}

/// Advance the punch envelope by `dt`: a rising edge fires, the rest
/// decays. Pure — tested.
#[must_use]
pub fn advance_punch(punch: f32, rising: bool, reduced_flashing: bool, dt: f32) -> f32 {
    let (fire, decay) = if reduced_flashing {
        (CALM_PUNCH, CALM_DECAY_PER_S)
    } else {
        (1.0, PUNCH_DECAY_PER_S)
    };
    let punch = (punch - decay * dt).max(0.0);
    if rising { punch.max(fire) } else { punch }
}

/// The level the governor may learn from: the listener's, and only
/// while it is measuring. Pure — tested.
#[must_use]
pub fn heard_level(ears: &Ears) -> Option<f32> {
    ears.0
        .as_ref()
        .filter(|listener| listener.measuring())
        .map(beatbyte_audio::listen::Listener::db)
}

/// A stage lamp's own intensity, remembered the first time the
/// highlight touches it.
#[derive(Component, Debug, Clone, Copy)]
pub struct LampBase(pub f32);

/// Read the room's level, run the threshold, fire the highlight on
/// the rising edge and apply the envelope to every stage lamp.
pub fn drive_highlight(
    mut commands: Commands,
    settings: Res<Settings>,
    ears: Res<Ears>,
    time: Res<Time>,
    mut highlight: ResMut<Highlight>,
    mut lamps: Query<(Entity, &mut SpotLight, Option<&LampBase>), With<RenderLayers>>,
    mut last_reported: Local<f32>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    // Only a real measurement reaches the governor: before the
    // input is heard its level is the floor constant, and a governor
    // fed that once takes −100 dBFS for the room's noise floor and
    // never lets the margin bind (seen in the first live run).
    let (over, rising) = match heard_level(&ears) {
        Some(level) => {
            let over = highlight.threshold.tick(now, level);
            let rising = over && !highlight.over;
            if rising && now - *last_reported >= 1.0 {
                *last_reported = now;
                info!(
                    "highlight: level {level:.1} over threshold {:.1} (floor {:.1}, duty {:.2})",
                    highlight.threshold.threshold,
                    highlight.threshold.noise_floor,
                    highlight.threshold.duty
                );
            }
            (over, rising)
        }
        None => (false, false),
    };
    highlight.over = over;
    let punch = advance_punch(highlight.punch, rising, settings.reduced_flashing, dt);
    let settled = highlight.punch == 0.0 && punch == 0.0;
    highlight.punch = punch;
    if settled {
        return; // nothing to write: every lamp already sits at its base
    }
    let gain = 1.0 + HIGHLIGHT_GAIN * punch;
    for (entity, mut light, base) in &mut lamps {
        let base = match base {
            Some(base) => base.0,
            None => {
                commands.entity(entity).insert(LampBase(light.intensity));
                light.intensity
            }
        };
        light.intensity = base * gain;
    }
}

// ---------------------------------------------------------------- strips

/// Bars per strip.
pub const STRIP_BARS: usize = 40;
/// A bar's thickness at full brightness.
pub const STRIP_BAR: f32 = 0.045;
/// The comet's run, seconds.
pub const COMET_S: f32 = 1.4;
/// The comet's tail, as a share of the strip.
pub const COMET_TAIL: f32 = 0.30;
/// The glimmer's length, seconds.
pub const GLIMMER_S: f32 = 1.0;
/// How often the glimmer re-rolls.
pub const GLIMMER_HZ: f32 = 24.0;
/// ... under reduced flashing.
pub const CALM_GLIMMER_HZ: f32 = 4.0;
/// The share of bars a glimmer lights per step.
pub const GLIMMER_SHARE: f32 = 0.18;
/// The quiet between two effects on one strip, seconds: `MIN` plus
/// up to `SPREAD`.
pub const QUIET_MIN_S: f32 = 9.0;
/// The spread on top of the minimum quiet, seconds.
pub const QUIET_SPREAD_S: f32 = 9.0;
/// The strips' white: the warm white the sparks already use.
pub const STRIP_WHITE: Color = Color::srgb(1.0, 0.96, 0.86);

/// What a strip belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripPlace {
    /// Along the band riser's front edge.
    Stage,
    /// Along a barrier's top rail (one per side).
    Audience,
    /// Up the front of a PA stack (one per side).
    Pa,
}

/// The five strips: where each runs, from `a` to `b`. Pure — tested.
#[must_use]
pub fn strip_lines() -> [(StripPlace, Vec3, Vec3); 5] {
    // The band riser: front face at z −26, top at 1.3 (band.rs).
    let stage_y = 1.3 + 0.03;
    let stage_z = -26.0 + 0.03;
    // The barrier rails (crowd.rs): 22 long about z −22, top at
    // CROWD_FLOOR_Y + 1.14.
    let rail_y = CROWD_FLOOR_Y + 1.14 + 0.06;
    // The stacks: up the inner front corner of the sub, from the
    // deck to the head's top.
    let sub = pa::stack_layout(1.0)[0];
    let head = pa::stack_layout(1.0)[3];
    let pa_x = sub.centre.x - sub.size.x * 0.5 + 0.08;
    let pa_z = sub.centre.z + sub.size.z * 0.5 + 0.03;
    [
        (
            StripPlace::Stage,
            Vec3::new(-6.8, stage_y, stage_z),
            Vec3::new(6.8, stage_y, stage_z),
        ),
        (
            StripPlace::Audience,
            Vec3::new(-BARRIER_X, rail_y, -33.0),
            Vec3::new(-BARRIER_X, rail_y, -11.0),
        ),
        (
            StripPlace::Audience,
            Vec3::new(BARRIER_X, rail_y, -33.0),
            Vec3::new(BARRIER_X, rail_y, -11.0),
        ),
        (
            StripPlace::Pa,
            Vec3::new(-pa_x, sub.bottom(), pa_z),
            Vec3::new(-pa_x, head.top(), pa_z),
        ),
        (
            StripPlace::Pa,
            Vec3::new(pa_x, sub.bottom(), pa_z),
            Vec3::new(pa_x, head.top(), pa_z),
        ),
    ]
}

/// One effect on a strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// A head running from one end to the other.
    Comet {
        /// +1 runs a→b, −1 the other way.
        dir: f32,
    },
    /// A second of sparkle.
    Glimmer,
}

impl Effect {
    /// How long the effect runs.
    #[must_use]
    pub fn duration(self) -> f32 {
        match self {
            Effect::Comet { .. } => COMET_S,
            Effect::Glimmer => GLIMMER_S,
        }
    }
}

/// Which effect the `count`-th firing of strip `strip` is, and how
/// long the quiet before it lasts. Pure — tested.
#[must_use]
pub fn pick(strip: usize, count: u32) -> (Effect, f32) {
    let seed = strip * 7919 + count as usize * 104_729;
    let kind = hash01(seed);
    let dir = if hash01(seed + 1) < 0.5 { 1.0 } else { -1.0 };
    let quiet = QUIET_MIN_S + QUIET_SPREAD_S * hash01(seed + 2);
    let effect = if kind < 0.55 {
        Effect::Comet { dir }
    } else {
        Effect::Glimmer
    };
    (effect, quiet)
}

/// A comet's brightness at bar position `u` (0..1 along the strip)
/// when its head is at `head` running in `dir`: 1 at the head, an
/// exponential tail behind it, nothing ahead. Pure — tested.
#[must_use]
pub fn comet_brightness(u: f32, head: f32, dir: f32) -> f32 {
    let behind = (head - u) * dir;
    if behind < 0.0 {
        return 0.0;
    }
    (-behind / (COMET_TAIL * 0.35)).exp()
}

/// A glimmer's brightness for bar `bar` of strip `strip` at `step`:
/// most bars dark, a share lit at a random level. Pure — tested.
#[must_use]
pub fn glimmer_brightness(strip: usize, bar: usize, step: u32) -> f32 {
    let seed = strip * 311 + bar * 1009 + step as usize * 7;
    if hash01(seed) < GLIMMER_SHARE {
        0.4 + 0.6 * hash01(seed + 5)
    } else {
        0.0
    }
}

/// A strip's state.
#[derive(Component, Debug, Clone, Copy)]
pub struct Strip {
    /// Which strip, 0..5.
    pub index: usize,
    /// Where it belongs.
    pub place: StripPlace,
    /// When the next effect starts.
    pub next_at: f32,
    /// Effects fired so far.
    pub count: u32,
    /// The running effect and when it started.
    pub running: Option<(Effect, f32)>,
    /// The glimmer step drawn last, so a step redraws once.
    pub drawn_step: Option<u32>,
}

/// One bar of a strip.
#[derive(Component, Debug, Clone, Copy)]
pub struct StripBar {
    /// The strip.
    pub strip: usize,
    /// Position along it, 0..1.
    pub u: f32,
    /// The bar's centre.
    pub centre: Vec3,
    /// The bar's rotation along the strip.
    pub rotation: Quat,
    /// The bar's length.
    pub length: f32,
}

/// Spawn the five strips, every bar hidden.
pub fn spawn_strips(
    mut commands: Commands,
    settings: Res<Settings>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    let layer = RenderLayers::layer(STAGE_LAYER);
    let bar = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let white = materials.add(StandardMaterial {
        base_color: STRIP_WHITE.with_alpha(0.85),
        emissive: STRIP_WHITE.to_linear() * 6.0,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let now = time.elapsed_secs();
    for (index, (place, a, b)) in strip_lines().into_iter().enumerate() {
        let (_, quiet) = pick(index, 0);
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Strip {
                index,
                place,
                // The first effect comes sooner: the show should
                // start within the first half minute.
                next_at: now + quiet * 0.5,
                count: 0,
                running: None,
                drawn_step: None,
            },
        ));
        for k in 0..STRIP_BARS {
            let u0 = k as f32 / STRIP_BARS as f32;
            let u1 = (k + 1) as f32 / STRIP_BARS as f32;
            let (centre, rotation, length) = segment_pose(a.lerp(b, u0), a.lerp(b, u1));
            commands.spawn((
                GameplayScreen,
                Stage3d,
                NotShadowCaster,
                StripBar {
                    strip: index,
                    u: (u0 + u1) * 0.5,
                    centre,
                    rotation,
                    length: length * 0.92,
                },
                Mesh3d(bar.clone()),
                MeshMaterial3d(white.clone()),
                Transform::from_translation(centre)
                    .with_rotation(rotation)
                    .with_scale(Vec3::ZERO),
                Visibility::Hidden,
                layer.clone(),
            ));
        }
    }
}

/// Run the strips: start effects on schedule, draw the running one,
/// clear it when it ends. Visibility and scale only.
pub fn run_strips(
    settings: Res<Settings>,
    time: Res<Time>,
    mut strips: Query<&mut Strip>,
    mut bars: Query<(&StripBar, &mut Transform, &mut Visibility)>,
) {
    if !stage3d::active(&settings) || !settings.backdrop_motion {
        return;
    }
    let now = time.elapsed_secs();
    let glimmer_hz = if settings.reduced_flashing {
        CALM_GLIMMER_HZ
    } else {
        GLIMMER_HZ
    };
    for mut strip in &mut strips {
        // Start.
        if strip.running.is_none() && now >= strip.next_at {
            let (effect, _) = pick(strip.index, strip.count);
            info!("strip {} ({:?}): {effect:?}", strip.index, strip.place);
            strip.running = Some((effect, now));
            strip.drawn_step = None;
        }
        let Some((effect, started)) = strip.running else {
            continue;
        };
        let t = now - started;
        // End: every bar dark, the next quiet scheduled.
        if t >= effect.duration() {
            for (bar, mut transform, mut visibility) in &mut bars {
                if bar.strip == strip.index {
                    transform.scale = Vec3::ZERO;
                    *visibility = Visibility::Hidden;
                }
            }
            strip.count += 1;
            let (_, quiet) = pick(strip.index, strip.count);
            strip.next_at = now + quiet;
            strip.running = None;
            continue;
        }
        // Draw.
        let brightness_of: Box<dyn Fn(&StripBar) -> f32> = match effect {
            Effect::Comet { dir } => {
                let progress = t / COMET_S;
                // The head runs past both ends so the tail leaves too.
                let head = if dir > 0.0 {
                    -COMET_TAIL + progress * (1.0 + 2.0 * COMET_TAIL)
                } else {
                    1.0 + COMET_TAIL - progress * (1.0 + 2.0 * COMET_TAIL)
                };
                Box::new(move |bar: &StripBar| comet_brightness(bar.u, head, dir))
            }
            Effect::Glimmer => {
                let step = (t * glimmer_hz) as u32;
                if strip.drawn_step == Some(step) {
                    continue; // this step is on screen already
                }
                strip.drawn_step = Some(step);
                let index = strip.index;
                Box::new(move |bar: &StripBar| glimmer_brightness(index, bar.u_index(), step))
            }
        };
        for (bar, mut transform, mut visibility) in &mut bars {
            if bar.strip != strip.index {
                continue;
            }
            let brightness = brightness_of(bar);
            if brightness <= 0.02 {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
                continue;
            }
            let thick = STRIP_BAR * (0.35 + 0.65 * brightness);
            transform.scale = Vec3::new(bar.length, thick, thick);
            *visibility = Visibility::Inherited;
        }
    }
}

impl StripBar {
    /// The bar's index along its strip, from its position.
    #[must_use]
    pub fn u_index(&self) -> usize {
        ((self.u * STRIP_BARS as f32) as usize).min(STRIP_BARS - 1)
    }
}

/// Wire the light show into the app.
pub fn register(app: &mut App) {
    app.init_resource::<Highlight>()
        .add_systems(OnEnter(AppState::Gameplay), spawn_strips)
        .add_systems(
            Update,
            (drive_highlight, run_strips).run_if(
                in_state(crate::states::GamePhase::Playing)
                    .or_else(in_state(crate::states::GamePhase::Outro)),
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the governor over `seconds` of a level pattern: `loud` dB
    /// for `on` of every `period` seconds, `quiet` dB otherwise.
    fn run(loud: f32, quiet: f32, on: f32, period: f32, seconds: f32) -> DutyThreshold {
        let mut governor = DutyThreshold::default();
        let mut t = 0.0;
        while t < seconds {
            let db = if t % period < on { loud } else { quiet };
            governor.tick(t, db);
            t += 0.02;
        }
        governor
    }

    #[test]
    fn the_threshold_finds_the_level_that_gives_the_target_duty() {
        // Music that is loud 35 % of the time at −12 and −30 the
        // rest: the threshold lands between the two — over on the
        // loud share, under on the rest.
        let governor = run(-12.0, -30.0, 0.35, 1.0, 60.0);
        assert!(
            governor.threshold > -30.0 && governor.threshold < -12.0,
            "{}",
            governor.threshold
        );
        assert!(
            (governor.duty - DUTY_TARGET).abs() < 0.2,
            "{}",
            governor.duty
        );
    }

    #[test]
    fn a_louder_passage_pushes_the_threshold_up_and_a_softer_one_lets_it_down() {
        // The rule: a full step up is twice a full step down, and
        // inside the deadband nothing moves.
        assert_eq!(step_db(1.0), STEP_UP_DB);
        assert_eq!(step_db(-1.0), -STEP_DOWN_DB);
        assert_eq!(step_db(1.0) / -step_db(-1.0), 2.0);
        assert_eq!(step_db(DUTY_DEADBAND * 0.5), 0.0);
        assert!(step_db(DUTY_DEADBAND * 1.5) > 0.0 && step_db(DUTY_DEADBAND * 1.5) < STEP_UP_DB);
        // Always over the start threshold: it climbs.
        let up = run(-10.0, -10.0, 1.0, 1.0, 20.0);
        assert!(up.threshold > THRESHOLD_START_DB + 5.0, "{}", up.threshold);
        // Music that never reaches it (with a dip of silence every
        // second so the noise floor sits far below and does not
        // bind): it comes down — but less, over the same time.
        let down = run(-40.0, -95.0, 0.95, 1.0, 20.0);
        assert!(
            down.threshold < THRESHOLD_START_DB - 2.5,
            "{}",
            down.threshold
        );
        assert!(
            up.threshold - THRESHOLD_START_DB > THRESHOLD_START_DB - down.threshold,
            "up {} vs down {}",
            up.threshold,
            down.threshold
        );
    }

    #[test]
    fn silence_freezes_the_governor_and_the_floor_holds_the_threshold_off_the_noise() {
        let mut governor = DutyThreshold::default();
        let mut t = 0.0;
        while t < 30.0 {
            governor.tick(t, -70.0); // room noise, under LOUD_DB
            t += 0.02;
        }
        assert_eq!(
            governor.threshold, THRESHOLD_START_DB,
            "nothing learned from silence"
        );
        assert!(
            (governor.noise_floor + 70.0).abs() < 1.0,
            "the floor found the noise (and crept up its 0.02 dB/s): {}",
            governor.noise_floor
        );
        // A room whose noise sits at −50 (the floor is a minimum
        // that only creeps up, so this is a fresh governor) and whose
        // music, at −48, never reaches the threshold: the threshold
        // may come down, but it stops at the floor's margin (−44),
        // above the music — it never chases the level into the noise.
        let mut governor = DutyThreshold::default();
        let mut t = 0.0;
        while t < 400.0 {
            governor.tick(t, if t % 1.0 < 0.1 { -50.0 } else { -48.0 });
            t += 0.02;
        }
        let floor = governor.noise_floor + FLOOR_MARGIN_DB;
        // (The floor creeps 0.02 dB/s between two steps; a hair of
        // slack for that.)
        assert!(
            governor.threshold >= floor - 0.05,
            "{} under the floor {floor}",
            governor.threshold
        );
        assert!(
            governor.threshold > -47.0,
            "{}: held off the music by the floor, not sitting on it",
            governor.threshold
        );
    }

    #[test]
    fn the_bit_is_live_and_the_punch_fires_on_the_edge_only() {
        let mut governor = DutyThreshold::default();
        assert!(!governor.tick(0.0, -40.0));
        assert!(governor.tick(0.02, -20.0));
        assert!(
            !governor.tick(0.04, -40.0),
            "no hysteresis: it drops at once"
        );
        // The envelope: a rising edge fires, a held bit decays.
        let fired = advance_punch(0.0, true, false, 0.016);
        assert_eq!(fired, 1.0);
        let held = advance_punch(fired, false, false, 0.1);
        assert!(held < 1.0 && held > 0.6, "{held}");
        assert_eq!(
            advance_punch(0.05, false, false, 0.1),
            0.0,
            "and lands at zero"
        );
        // Reduced flashing: a swell, not a flash.
        let calm = advance_punch(0.0, true, true, 0.016);
        assert!((calm - CALM_PUNCH).abs() < 1e-6);
        assert!(
            advance_punch(calm, false, true, 0.1)
                > advance_punch(fired, false, false, 0.1) * CALM_PUNCH
        );
    }

    #[test]
    fn only_a_heard_input_teaches_the_governor() {
        use beatbyte_audio::listen::Listener;
        assert_eq!(heard_level(&Ears(None)), None, "no listener");
        assert_eq!(
            heard_level(&Ears(Some(Listener::stub(false, -30.0, None)))),
            None,
            "open but never heard: its level is the floor constant, not the room"
        );
        assert_eq!(
            heard_level(&Ears(Some(Listener::stub(true, -30.0, None)))),
            Some(-30.0)
        );
    }

    #[test]
    fn the_strips_run_where_the_things_they_light_are() {
        let lines = strip_lines();
        let places: Vec<StripPlace> = lines.iter().map(|(p, _, _)| *p).collect();
        assert_eq!(
            places,
            [
                StripPlace::Stage,
                StripPlace::Audience,
                StripPlace::Audience,
                StripPlace::Pa,
                StripPlace::Pa
            ]
        );
        for (place, a, b) in lines {
            assert!(a.distance(b) > 2.0, "{place:?} is a strip, not a dot");
            match place {
                StripPlace::Stage => assert!(a.y > 1.3 && (a.z + 26.0).abs() < 0.1),
                StripPlace::Audience => {
                    assert_eq!(a.x.abs(), BARRIER_X);
                    assert!(a.y > CROWD_FLOOR_Y + 1.14);
                }
                StripPlace::Pa => {
                    let sub = pa::stack_layout(a.x.signum())[0];
                    assert!(a.x.abs() < sub.centre.x.abs(), "on the inner corner");
                    assert!(b.y <= pa::CAMERA_Y - 0.2, "under the camera, like the head");
                }
            }
        }
    }

    #[test]
    fn a_comet_is_a_head_with_a_tail_behind_it_and_nothing_ahead() {
        assert_eq!(comet_brightness(0.5, 0.5, 1.0), 1.0);
        assert_eq!(comet_brightness(0.6, 0.5, 1.0), 0.0, "ahead is dark");
        assert!(comet_brightness(0.4, 0.5, 1.0) < 1.0);
        assert!(comet_brightness(0.4, 0.5, 1.0) > comet_brightness(0.3, 0.5, 1.0));
        // The other way round, the tail trails the other way.
        assert_eq!(comet_brightness(0.4, 0.5, -1.0), 0.0);
        assert!(comet_brightness(0.6, 0.5, -1.0) > 0.0);
    }

    #[test]
    fn a_glimmer_lights_a_share_and_changes_every_step() {
        let lit = |step: u32| -> Vec<usize> {
            (0..STRIP_BARS)
                .filter(|&bar| glimmer_brightness(0, bar, step) > 0.0)
                .collect()
        };
        let a = lit(1);
        let b = lit(2);
        assert!(!a.is_empty() && a.len() < STRIP_BARS / 2, "{}", a.len());
        assert_ne!(a, b, "a new roll every step");
        assert_eq!(lit(1), a, "deterministic");
    }

    #[test]
    fn the_schedule_is_sporadic_deterministic_and_mixes_both_effects() {
        let mut comets = 0;
        let mut glimmers = 0;
        for count in 0..40 {
            let (effect, quiet) = pick(0, count);
            assert!((QUIET_MIN_S..=QUIET_MIN_S + QUIET_SPREAD_S).contains(&quiet));
            match effect {
                Effect::Comet { dir } => {
                    assert!(dir == 1.0 || dir == -1.0);
                    comets += 1;
                }
                Effect::Glimmer => glimmers += 1,
            }
        }
        assert!(
            comets > 5 && glimmers > 5,
            "{comets} comets, {glimmers} glimmers"
        );
        assert_eq!(pick(3, 7), pick(3, 7));
        assert_ne!(pick(3, 7).1, pick(4, 7).1, "strips do not fire in step");
    }
}
