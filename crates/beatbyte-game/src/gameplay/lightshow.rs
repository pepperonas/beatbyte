//! The light show the room's own level drives: a **strobe** on the
//! ceiling rig while the measured level sits over a threshold that
//! sets itself, and three white **light strips** — stage, audience,
//! PA — that now and then run a comet or a spray of sparks.
//!
//! The level comes from [`super::monitors::Ears`], the same input
//! the monitors show; nothing here reads the chart, and without a
//! measurement (no input, refused, silent) the strobe never fires
//! while the strips go on sparkling: they are decor, not a readout.
//!
//! **The threshold is dynamic**, the way the reference rig's
//! dB-Analyse sets its own (`disco-controller/auto_thr.py`, the duty
//! governor): the share of time the level sits above the threshold
//! is measured over a short window and the threshold is stepped —
//! up quickly, down at half the rate — toward a target share, one
//! step per interval, never below the noise floor plus a margin,
//! frozen while there is no music. The bit itself is live
//! (`level > threshold`, no hysteresis), as the reference deliberately
//! keeps it.
//!
//! **The strobe** owns the ten lamps hanging from the two trusses
//! ([`super::rig::RigLamp`]) while the level stays over: a pair of
//! them flares WHITE twelve times a second, and which pair is a
//! shuffle of all ten, so every lamp is hit once per cycle and no
//! two cycles run the same order. A lamp's own colour and resting
//! intensity are remembered the first time it is touched and it goes
//! back to them the frame the level drops. Each hit rises hard and
//! falls away inside its step, leaving the dark gap that makes it a
//! strobe rather than a chase. Under `reduced_flashing` there is no
//! strobe at all — the rising edge swells every lamp instead, a
//! third as strong and slow to fade.
//!
//! **The strips** are pools of thin additive bars along a line,
//! driven by visibility and scale alone (the arc's pattern: one
//! material, no writes per frame). A **comet** is a bright head that
//! enters fast, eases out and drags an exponential tail. A
//! **sparkle** is a spray of two-to-five-bar clusters that flare and
//! then **die by dimming** — quadratically over a third of a second,
//! the reference rig's own recipe, whose lesson is that *sparks die,
//! they do not switch*. The first sparkle here rolled every bar
//! independently at 24 Hz: no cluster, no decay, a bar lit for one
//! frame and gone. That is white noise, and it looked like it.
//! Every strip fires on its own schedule, 9–18 s apart, from a hash
//! of the strip and the firing count — sporadic and deterministic.
//! STAGE MOTION off keeps every strip dark and writes nothing.

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
use super::rig;
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

/// Strobe steps per second while the level is over the threshold.
pub const STROBE_HZ: f32 = 12.0;
/// Lamps hit per step. A single lamp reads as a twinkle; a pair
/// reads as a hit.
pub const STROBE_AT_ONCE: usize = 2;
/// The share of a step a hit lasts. The rest is the dark gap that
/// makes this a strobe rather than a chase.
pub const STROBE_DUTY: f32 = 0.55;
/// What a hit adds to a lamp's intensity, in the engine's own units
/// rather than as a factor on the lamp's resting brightness.
///
/// A flash is a property of the flash, not of the fixture: the rims
/// rest at 3 000 000 and the moving heads at 900 000, so a factor
/// made the heads flash a third as hard as the rims — and the heads
/// are the near, large cones, where it was reported as not reading.
pub const STROBE_FLASH: f32 = 6_000_000.0;
/// The strobe's colour, whatever colour the lamp usually wears.
pub const STROBE_WHITE: Color = Color::srgb(1.0, 1.0, 1.0);
/// How far the venue's colour wash drops under a flash. A strobe
/// reads by contrast: the ceiling's narrow spots are small against
/// the two big coloured washes on the deck, and the first cut —
/// which flashed the spots and left the wash alone — put white on
/// the stage that could not be seen in a single photographed frame.
pub const STROBE_DIP: f32 = 0.6;
/// How long the strobe stays armed after the last sample over the
/// threshold. The bit is live and flickers with the music (no
/// hysteresis, deliberately); without a hold the strobe stutters in
/// and out inside one loud passage and reads as a fault.
pub const STROBE_HOLD_S: f32 = 0.15;

/// The order the ceiling lamps fire in for one cycle: a shuffle of
/// `0..lamps`, so every lamp is hit exactly once per cycle and no
/// two cycles run the same order. Fisher–Yates over the project's
/// hash — deterministic, allocation-free. Pure — tested.
#[must_use]
pub fn strobe_order(cycle: u32, lamps: usize) -> [u8; rig::RIG_LAMPS] {
    let mut order = [0u8; rig::RIG_LAMPS];
    let lamps = lamps.min(rig::RIG_LAMPS);
    for (index, slot) in order.iter_mut().enumerate().take(lamps) {
        *slot = index as u8;
    }
    for index in (1..lamps).rev() {
        let roll = hash01(cycle as usize * 9151 + index * 3221);
        let other = ((roll * (index + 1) as f32) as usize).min(index);
        order.swap(index, other);
    }
    order
}

/// The flash's shape at `t`, whichever lamps are firing: a hard
/// rise on the step, a plateau, a fast fall, then the dark gap. The
/// venue's wash dips by this too, so the ceiling flashes against a
/// darker room. Pure — tested.
#[must_use]
pub fn strobe_shape(t: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let step = (t * STROBE_HZ).floor();
    let within = t.mul_add(STROBE_HZ, -step);
    if within > STROBE_DUTY {
        return 0.0;
    }
    // A xenon tube: instant rise, a plateau, a fast fall. An
    // exponential from the first cut spent most of its lit share
    // nearly dark, which averages to a tint rather than a flash.
    let across = within / STROBE_DUTY;
    (1.0 - across * across * across * across).max(0.0)
}

/// How hard lamp `lamp` is hit `t` seconds into the strobe, 0..1:
/// the flash's shape on its own step, dark for the rest of the
/// cycle. Pure — tested.
#[must_use]
pub fn strobe_hit(lamp: usize, lamps: usize, t: f32) -> f32 {
    if lamps == 0 || lamp >= lamps || t < 0.0 {
        return 0.0;
    }
    let step = (t * STROBE_HZ).floor();
    let within = t.mul_add(STROBE_HZ, -step);
    if within > STROBE_DUTY {
        return 0.0;
    }
    let slots = lamps.div_ceil(STROBE_AT_ONCE);
    let cycle = (step as u32) / slots as u32;
    let slot = (step as usize) % slots;
    let order = strobe_order(cycle, lamps);
    let hit = (0..STROBE_AT_ONCE)
        .map(|k| slot * STROBE_AT_ONCE + k)
        .filter(|index| *index < lamps)
        .any(|index| order[index] as usize == lamp);
    if hit { strobe_shape(t) } else { 0.0 }
}

/// Whether `BEATBYTE_LIGHTSHOW` is set: the ceiling strobes
/// whatever the level is doing and the strips fire back to back, so
/// the show can be LOOKED at without waiting for a loud passage to
/// coincide with a screenshot. It changes nothing else — judgment,
/// the threshold and the effects themselves are untouched.
#[must_use]
pub fn forced() -> bool {
    std::env::var_os("BEATBYTE_LIGHTSHOW").is_some()
}

/// Until when the strobe is armed: every sample over the threshold
/// pushes the arming out by [`STROBE_HOLD_S`]. Pure — tested.
#[must_use]
pub fn strobe_armed_until(over: bool, now: f32, armed_until: f32) -> f32 {
    if over {
        now + STROBE_HOLD_S
    } else {
        armed_until
    }
}

/// Whether the ceiling strobes at `now`: still inside the arming,
/// and never under reduced flashing — the whole point of that
/// setting. Pure — tested (the decision, not only its branches).
#[must_use]
pub fn strobing(now: f32, armed_until: f32, reduced_flashing: bool) -> bool {
    now < armed_until && !reduced_flashing
}

/// The highlight's state: the threshold, the envelope and the
/// strobe's own clock.
#[derive(Resource, Debug, Default)]
pub struct Highlight {
    /// The threshold governor.
    pub threshold: DutyThreshold,
    /// The live bit from the last tick.
    pub over: bool,
    /// The envelope, 0..1.
    pub punch: f32,
    /// Until when the strobe is armed.
    pub strobe_until: f32,
    /// Whether anything was written to the lamps last frame, so the
    /// idle case costs nothing but still restores once.
    pub was_active: bool,
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
    let punch = decay.mul_add(-dt, punch).max(0.0);
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

/// The venue's colour wash: the two big coloured lights on the deck
/// and the fill on the crowd. Not ceiling fixtures — they never
/// strobe — but they dip under a flash so the strobe has something
/// to be brighter than.
#[derive(Component, Debug, Clone, Copy)]
pub struct VenueWash;

/// A stage lamp's own colour and intensity, remembered the first
/// time the light show touches it — no fixture is ever retuned.
#[derive(Component, Debug, Clone, Copy)]
pub struct LampBase {
    /// The intensity the rig gave it.
    pub intensity: f32,
    /// The colour the rig gave it.
    pub color: Color,
}

/// Read the room's level, run the threshold, and put it on the
/// lamps: the ceiling strobes white while the level is over, every
/// lamp swells on the rising edge, and both end at the lamp's own
/// colour and intensity.
#[allow(clippy::too_many_arguments)] // a Bevy system: every parameter is a world handle
pub fn drive_highlight(
    mut commands: Commands,
    settings: Res<Settings>,
    ears: Res<Ears>,
    time: Res<Time>,
    mut highlight: ResMut<Highlight>,
    mut lamps: Query<(
        Entity,
        &mut SpotLight,
        Option<&LampBase>,
        Option<&rig::RigLamp>,
    )>,
    mut wash: Query<(Entity, &mut PointLight, Option<&LampBase>), With<VenueWash>>,
    mut beams: Query<(&mut rig::RigBeam, &mut MeshMaterial3d<StandardMaterial>)>,
    mut last_reported: Local<f32>,
    mut show: Local<Option<bool>>,
    mut tally: Local<(u32, u32)>,
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
    highlight.punch = advance_punch(highlight.punch, rising, settings.reduced_flashing, dt);
    let armed = over || *show.get_or_insert_with(forced);
    highlight.strobe_until = strobe_armed_until(armed, now, highlight.strobe_until);
    // The chase runs on the wall clock and the threshold only gates
    // it. Anchoring it to the arming instead restarted the cycle on
    // every edge — and the bit flickers with the music, so the same
    // first pair fired over and over for a few milliseconds each
    // time (seen live: not one white frame in six).
    let strobe = strobing(now, highlight.strobe_until, settings.reduced_flashing).then_some(now);
    // Nothing to say and nothing said last frame: every lamp already
    // sits at its own colour and intensity.
    let active = highlight.punch > 0.0 || strobe.is_some();
    if !active && !highlight.was_active {
        return;
    }
    highlight.was_active = active;
    let gain = HIGHLIGHT_GAIN.mul_add(highlight.punch, 1.0);
    // Everything that is not being hit right now gives way to the
    // flash, so the ceiling has something to be brighter than.
    let shape = strobe.map_or(0.0, strobe_shape);
    let dip = STROBE_DIP.mul_add(-shape, 1.0);
    // Every firing lamp of a step shares one shape, so the ten hits
    // are computed once and read by the lights AND their beams.
    let hits_by_lamp: [f32; rig::RIG_LAMPS] =
        core::array::from_fn(|lamp| strobe.map_or(0.0, |t| strobe_hit(lamp, rig::RIG_LAMPS, t)));
    let mut ceiling = 0usize;
    let mut hits: Vec<usize> = Vec::new();
    tally.0 += 1;
    for (entity, mut light, base, rig_lamp) in &mut lamps {
        let base = match base {
            Some(base) => *base,
            None => {
                let base = LampBase {
                    intensity: light.intensity,
                    color: light.color,
                };
                commands.entity(entity).insert(base);
                base
            }
        };
        let hit = rig_lamp.map_or(0.0, |lamp| {
            ceiling += 1;
            hits_by_lamp[lamp.0]
        });
        if hit > 0.0
            && let Some(lamp) = rig_lamp
        {
            hits.push(lamp.0);
        }
        if hit > 0.0 && hits.len() == 1 {
            tally.1 += 1;
        }
        if hit > 0.0 {
            // White for the whole flash: a xenon tube does not tint.
            // The brightness carries the shape.
            light.color = STROBE_WHITE;
            light.intensity = base.intensity.mul_add(gain, STROBE_FLASH * hit);
        } else {
            light.color = base.color;
            light.intensity = base.intensity * gain * dip;
        }
    }
    // The beam a fixture wears goes white with it. Handle swaps, and
    // only when the state changes.
    let mut beams_seen = 0usize;
    let mut beams_lit = 0usize;
    for (mut beam, mut material) in &mut beams {
        let lit = hits_by_lamp.get(beam.lamp).is_some_and(|hit| *hit > 0.0);
        beams_seen += 1;
        if lit {
            beams_lit += 1;
        }
        if lit != beam.lit {
            beam.lit = lit;
            material.0 = if lit {
                beam.flash.clone()
            } else {
                beam.base.clone()
            };
        }
    }
    for (entity, mut light, base) in &mut wash {
        let base = match base {
            Some(base) => *base,
            None => {
                let base = LampBase {
                    intensity: light.intensity,
                    color: light.color,
                };
                commands.entity(entity).insert(base);
                base
            }
        };
        light.intensity = base.intensity * dip;
    }
    // A line a second while the ceiling is live: a locked screen
    // renders black and a photographed frame can miss a 46 ms hit,
    // but a log line cannot.
    // Counted over the second, never sampled: the first probe read
    // the instant it logged, and a 12 Hz strobe divides a second
    // evenly, so it aliased onto one phase and reported the same
    // answer for ever.
    if strobe.is_some() && now - *last_reported >= 1.0 {
        *last_reported = now;
        info!(
            "strobe: {} of {} frames had a hit, {ceiling} lamps, \
             {beams_lit}/{beams_seen} beams white, now {hits:?}",
            tally.1, tally.0
        );
        *tally = (0, 0);
    }
}

// ---------------------------------------------------------------- strips

/// Bars per strip. Finer than the first cut's forty: the comet's
/// gradient and a spark cluster both live on this resolution.
pub const STRIP_BARS: usize = 64;
/// Strips in the venue — see [`strip_lines`].
pub const STRIPS: usize = 5;
/// A bar's thickness at full brightness.
pub const STRIP_BAR: f32 = 0.045;
/// The comet's run, seconds.
pub const COMET_S: f32 = 1.4;
/// The comet's tail, as a share of the strip.
pub const COMET_TAIL: f32 = 0.30;
/// The whisker of glow ahead of the comet's head, as a share of the
/// strip: a comet has a bow, not a wall.
pub const COMET_BOW: f32 = 0.012;
/// How long a sparkle keeps making sparks, seconds.
pub const SPARKLE_SPAWN_S: f32 = 1.0;
/// A spark's life: it dies by dimming, never by switching off. The
/// reference rig's own figure.
pub const SPARK_DECAY_S: f32 = 0.28;
/// A new cluster this often, seconds.
pub const SPARK_EVERY_S: f32 = 0.045;
/// ... this much rarer under reduced flashing.
pub const CALM_SPARK_FACTOR: f32 = 4.0;
/// The narrowest spark cluster, in bars.
pub const SPARK_MIN_BARS: usize = 2;
/// The widest.
pub const SPARK_MAX_BARS: usize = 5;
/// The quiet between two effects on one strip, seconds: `MIN` plus
/// up to `SPREAD`.
pub const QUIET_MIN_S: f32 = 9.0;
/// The spread on top of the minimum quiet, seconds.
pub const QUIET_SPREAD_S: f32 = 9.0;
/// How often a firing is a comet rather than a sparkle. The comet
/// is the stronger of the two by eye, so it leads.
pub const COMET_SHARE: f32 = 0.6;
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
pub fn strip_lines() -> [(StripPlace, Vec3, Vec3); STRIPS] {
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
    /// A spray of sparks that flare and die.
    Sparkle,
}

impl Effect {
    /// How long the effect runs. A sparkle outlives its last spark's
    /// birth by that spark's life, so it ends by going out rather
    /// than by being cut off.
    #[must_use]
    pub fn duration(self) -> f32 {
        match self {
            Effect::Comet { .. } => COMET_S,
            Effect::Sparkle => SPARKLE_SPAWN_S + SPARK_DECAY_S,
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
    let quiet = QUIET_SPREAD_S.mul_add(hash01(seed + 2), QUIET_MIN_S);
    let effect = if kind < COMET_SHARE {
        Effect::Comet { dir }
    } else {
        Effect::Sparkle
    };
    (effect, quiet)
}

/// Where a comet's head sits at `progress` (0..1 through its run):
/// it enters fast and eases out, the way the reference rig's meteor
/// flies — a constant crawl reads as a moving dot, an easing one as
/// a thrown spark. It starts a tail's length before the strip and
/// ends a tail's length past it, so it arrives and leaves whole.
/// Pure — tested.
#[must_use]
pub fn comet_head(progress: f32, dir: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let eased = (1.0 - progress).mul_add(-(1.0 - progress), 1.0);
    let span = 2.0f32.mul_add(COMET_TAIL, 1.0);
    if dir > 0.0 {
        eased.mul_add(span, -COMET_TAIL)
    } else {
        eased.mul_add(-span, 1.0 + COMET_TAIL)
    }
}

/// A comet's brightness at bar position `u` (0..1 along the strip)
/// when its head is at `head` running in `dir`: full at the head, an
/// exponential tail behind it, a whisker of bow glow in front. Pure
/// — tested.
#[must_use]
pub fn comet_brightness(u: f32, head: f32, dir: f32) -> f32 {
    let behind = (head - u) * dir;
    if behind < 0.0 {
        return (behind / COMET_BOW).exp().min(1.0);
    }
    (-behind / (COMET_TAIL * 0.35)).exp()
}

/// How bright bar `bar` of strip `strip` is `t` seconds into a
/// sparkle, with a cluster born every `every` seconds.
///
/// A spark is a CLUSTER of two to five bars, each bar with its own
/// peak, that flares and then **dies by dimming** — quadratically
/// over [`SPARK_DECAY_S`], the reference rig's own recipe, whose
/// lesson is that sparks die, they do not switch. The first cut
/// rolled every bar independently at 24 Hz: no cluster, no decay, a
/// bar lit for a single frame. That is white noise. Pure — tested.
#[must_use]
pub fn sparkle_brightness(strip: usize, bar: usize, t: f32, every: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let every = every.max(1e-3);
    let newest = (t / every).floor() as i64;
    let oldest = (newest - (SPARK_DECAY_S / every).ceil() as i64).max(0);
    let mut light = 0.0f32;
    for generation in oldest..=newest {
        let age = (generation as f32).mul_add(-every, t);
        if age < 0.0 || age > SPARK_DECAY_S {
            continue;
        }
        let seed = strip * 7919 + generation as usize * 65_537;
        let span = (SPARK_MAX_BARS - SPARK_MIN_BARS + 1) as f32;
        let width = (SPARK_MIN_BARS + (hash01(seed + 1) * span) as usize).min(SPARK_MAX_BARS);
        let start = (hash01(seed) * (STRIP_BARS - width) as f32) as usize;
        if bar < start || bar >= start + width {
            continue;
        }
        // Every bar of a cluster has its own peak, so the cluster is
        // a spray of sparks and not a lit block.
        let peak = 0.4f32.mul_add(hash01(seed + 17 + bar * 131), 0.6);
        let left = 1.0 - age / SPARK_DECAY_S;
        light = light.max(peak * left * left);
    }
    light
}

/// A strip's state.
#[derive(Component, Debug, Clone, Copy)]
pub struct Strip {
    /// Which strip, 0..[`STRIPS`].
    pub index: usize,
    /// Where it belongs.
    pub place: StripPlace,
    /// When the next effect starts.
    pub next_at: f32,
    /// Effects fired so far.
    pub count: u32,
    /// The running effect and when it started.
    pub running: Option<(Effect, f32)>,
}

/// One bar of a strip.
#[derive(Component, Debug, Clone, Copy)]
pub struct StripBar {
    /// The strip.
    pub strip: usize,
    /// Position along it, 0..1.
    pub u: f32,
    /// The bar's index along its strip.
    pub index: usize,
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
                next_at: quiet.mul_add(0.5, now),
                count: 0,
                running: None,
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
                    index: k,
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

/// How bright a bar is under a running effect. Pure — tested through
/// its two halves.
#[must_use]
fn bar_brightness(effect: Effect, strip: usize, bar: &StripBar, t: f32, every: f32) -> f32 {
    match effect {
        Effect::Comet { dir } => comet_brightness(bar.u, comet_head(t / COMET_S, dir), dir),
        Effect::Sparkle => sparkle_brightness(strip, bar.index, t, every),
    }
}

/// Run the strips: start effects on schedule, draw the running one,
/// clear it when it ends. Visibility and scale only, and a dark bar
/// of an idle strip is not written at all.
pub fn run_strips(
    settings: Res<Settings>,
    time: Res<Time>,
    mut strips: Query<&mut Strip>,
    mut bars: Query<(&StripBar, &mut Transform, &mut Visibility)>,
    mut show: Local<Option<bool>>,
) {
    if !stage3d::active(&settings) || !settings.backdrop_motion {
        return;
    }
    let now = time.elapsed_secs();
    let every = if settings.reduced_flashing {
        SPARK_EVERY_S * CALM_SPARK_FACTOR
    } else {
        SPARK_EVERY_S
    };
    // What each strip is doing this frame, so the bars are walked
    // once instead of once per strip.
    let mut running: [Option<(Effect, f32)>; STRIPS] = [None; STRIPS];
    for mut strip in &mut strips {
        if strip.running.is_none() && now >= strip.next_at {
            let (effect, _) = pick(strip.index, strip.count);
            info!("strip {} ({:?}): {effect:?}", strip.index, strip.place);
            strip.running = Some((effect, now));
        }
        let Some((effect, started)) = strip.running else {
            continue;
        };
        let t = now - started;
        if t >= effect.duration() {
            strip.count += 1;
            let (_, quiet) = pick(strip.index, strip.count);
            strip.next_at = if *show.get_or_insert_with(forced) {
                now
            } else {
                now + quiet
            };
            strip.running = None;
            continue;
        }
        if let Some(slot) = running.get_mut(strip.index) {
            *slot = Some((effect, t));
        }
    }
    for (bar, mut transform, mut visibility) in &mut bars {
        let brightness = running
            .get(bar.strip)
            .copied()
            .flatten()
            .map_or(0.0, |(effect, t)| {
                bar_brightness(effect, bar.strip, bar, t, every)
            });
        if brightness <= 0.02 {
            // Untouched while dark: no transform write, no change
            // detection, nothing for the renderer to reconsider.
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
                transform.scale = Vec3::ZERO;
            }
            continue;
        }
        let thick = STRIP_BAR * 0.65f32.mul_add(brightness, 0.35);
        transform.scale = Vec3::new(bar.length, thick, thick);
        *visibility = Visibility::Inherited;
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
    fn the_strobe_order_is_a_shuffle_that_hits_every_lamp_once() {
        let lamps = rig::RIG_LAMPS;
        let all: Vec<u8> = (0..lamps as u8).collect();
        for cycle in 0..12 {
            let order = strobe_order(cycle, lamps);
            let mut seen = order[..lamps].to_vec();
            seen.sort_unstable();
            assert_eq!(seen, all, "cycle {cycle} is not a permutation");
        }
        assert_eq!(
            strobe_order(3, lamps),
            strobe_order(3, lamps),
            "deterministic"
        );
        assert!(
            (0..12).any(|c| strobe_order(c, lamps) != strobe_order(c + 1, lamps)),
            "the order has to change from cycle to cycle"
        );
        assert!(
            (0..12).any(|c| strobe_order(c, lamps)[0] != 0),
            "and it is not the lamps in their own order every time"
        );
    }

    #[test]
    fn every_lamp_is_hit_once_a_cycle_with_a_dark_gap_between_hits() {
        let lamps = rig::RIG_LAMPS;
        let slots = lamps.div_ceil(STROBE_AT_ONCE);
        let cycle_s = slots as f32 / STROBE_HZ;
        let samples = 400;
        let mut hits = vec![0u32; lamps];
        let mut dark = 0;
        for k in 0..samples {
            let t = k as f32 / samples as f32 * cycle_s;
            let lit: Vec<usize> = (0..lamps)
                .filter(|l| strobe_hit(*l, lamps, t) > 0.0)
                .collect();
            if lit.is_empty() {
                dark += 1;
            }
            assert!(lit.len() <= STROBE_AT_ONCE, "a pair at a time, not a wash");
            for lamp in lit {
                hits[lamp] += 1;
            }
        }
        assert!(
            hits.iter().all(|count| *count > 0),
            "every lamp is hit once a cycle: {hits:?}"
        );
        assert!(
            dark > samples / 5,
            "a real gap between hits: {dark}/{samples}"
        );
        // A hit rises hard and falls away inside its own step.
        let lamp = strobe_order(0, lamps)[0] as usize;
        assert!((strobe_hit(lamp, lamps, 0.0) - 1.0).abs() < 1e-6);
        // A plateau, not a slope: still near full half way through
        // its lit share, gone by the end of it.
        let half = strobe_hit(lamp, lamps, STROBE_DUTY * 0.5 / STROBE_HZ);
        assert!(half > 0.9, "the flash holds through its share: {half}");
        let late = strobe_hit(lamp, lamps, STROBE_DUTY * 0.99 / STROBE_HZ);
        assert!(late > 0.0 && late < 0.1, "and then falls away fast: {late}");
        assert_eq!(
            strobe_hit(lamp, lamps, 0.99 / STROBE_HZ),
            0.0,
            "dark before the next step"
        );
        assert_eq!(strobe_hit(lamp, lamps, -1.0), 0.0);
    }

    #[test]
    fn a_flash_is_as_hard_from_a_weak_fixture_as_from_a_strong_one() {
        // The rims rest at 3 000 000 and the heads at 900 000. A
        // factor on the resting brightness made the heads flash a
        // third as hard as the rims — and the heads are the near,
        // large cones, which is where it was reported as not
        // reading. The flash is absolute; the rest is the fixture.
        let flash = |rest: f32| rest.mul_add(1.0, STROBE_FLASH);
        let weak = flash(900_000.0);
        let strong = flash(3_000_000.0);
        assert!(
            strong / weak < 2.0,
            "a weak fixture must flash within a factor of two of a \
             strong one: {weak} vs {strong}"
        );
        assert!(weak / 900_000.0 > 5.0, "and it must be a flash: {weak}");
    }

    #[test]
    fn the_whole_room_gives_way_to_a_flash() {
        // Every firing lamp shares one shape, and that shape is what
        // the rest of the room dips by: the strobe reads by contrast.
        let lamps = rig::RIG_LAMPS;
        for k in 0..40 {
            let t = k as f32 * 0.01;
            let shape = strobe_shape(t);
            let lit: Vec<f32> = (0..lamps)
                .map(|lamp| strobe_hit(lamp, lamps, t))
                .filter(|hit| *hit > 0.0)
                .collect();
            assert!(
                lit.iter().all(|hit| (hit - shape).abs() < 1e-6),
                "one shape for every lamp of the step: {lit:?} vs {shape}"
            );
            if lit.is_empty() {
                assert_eq!(shape, 0.0, "and nothing to dip by in the gap");
            }
        }
        assert!((strobe_shape(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(strobe_shape(-1.0), 0.0);
        // The dip is a real darkening, and it lifts again.
        // The dip is a real darkening at the peak, and nothing in
        // the gap.
        assert!(
            STROBE_DIP.mul_add(-strobe_shape(0.0), 1.0) < 0.6,
            "the room gives way"
        );
        let gap = STROBE_DIP.mul_add(-strobe_shape(0.9 / STROBE_HZ), 1.0);
        assert!(
            (gap - 1.0).abs() < 1e-6,
            "and stands up again between flashes"
        );
    }

    #[test]
    fn reduced_flashing_takes_the_strobe_away_and_leaves_the_swell() {
        assert!(strobing(0.0, 1.0, false));
        assert!(
            !strobing(0.0, 1.0, true),
            "no strobe under reduced flashing"
        );
        assert!(
            !strobing(2.0, 1.0, false),
            "and none once the arming runs out"
        );
        // The arming holds through the gaps in a live bit.
        let armed = strobe_armed_until(true, 10.0, 0.0);
        assert!((armed - (10.0 + STROBE_HOLD_S)).abs() < 1e-6);
        assert!(
            strobing(10.0 + STROBE_HOLD_S * 0.5, armed, false),
            "a sample under the threshold does not end the burst"
        );
        assert!(!strobing(10.0 + STROBE_HOLD_S * 1.5, armed, false));
        assert_eq!(
            strobe_armed_until(false, 10.1, armed),
            armed,
            "and a quiet sample never pushes it out"
        );
        assert!(
            advance_punch(0.0, true, true, 0.016) > 0.0,
            "the swell is what remains"
        );
    }

    /// The commission, wired: while the level is over the threshold
    /// a ceiling lamp flares WHITE and brighter than it stands, its
    /// neighbours in the same instant do not, and everything is back
    /// at its own colour the moment the level drops.
    #[test]
    fn a_ceiling_lamp_flares_white_and_gives_its_colour_back() {
        use bevy::ecs::system::RunSystemOnce;
        let lamps = rig::RIG_LAMPS;
        let order = strobe_order(0, lamps);
        let firing = order[0] as usize;
        let waiting = order[lamps - 1] as usize;
        let tone = Color::srgb(1.0, 0.2, 0.1);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<StandardMaterial>();
        app.insert_resource(Settings {
            stage_3d: true,
            reduced_flashing: false,
            ..Settings::default()
        });
        app.init_resource::<Highlight>();
        // A loud room: over the governor's opening threshold.
        app.insert_resource(Ears(Some(beatbyte_audio::listen::Listener::stub(
            true, -20.0, None,
        ))));
        let base_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let flash_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        for lamp in [firing, waiting] {
            app.world_mut().spawn((
                rig::RigBeam {
                    lamp,
                    base: base_material.clone(),
                    flash: flash_material.clone(),
                    lit: false,
                },
                MeshMaterial3d(base_material.clone()),
            ));
        }
        let lit = app
            .world_mut()
            .spawn((
                rig::RigLamp(firing),
                SpotLight {
                    color: tone,
                    intensity: 1000.0,
                    ..default()
                },
            ))
            .id();
        let dark = app
            .world_mut()
            .spawn((
                rig::RigLamp(waiting),
                SpotLight {
                    color: tone,
                    intensity: 1000.0,
                    ..default()
                },
            ))
            .id();
        // Not a rig lamp: the band's key light never strobes.
        let key = app
            .world_mut()
            .spawn(SpotLight {
                color: tone,
                intensity: 1000.0,
                ..default()
            })
            .id();
        app.world_mut()
            .run_system_once(drive_highlight)
            .expect("the system runs");
        let look = |world: &mut World, entity: Entity| {
            let light = world.get::<SpotLight>(entity).expect("a lamp");
            (light.color.to_srgba(), light.intensity)
        };
        let (white, bright) = look(app.world_mut(), lit);
        assert!(
            white.red > 0.95 && white.green > 0.95 && white.blue > 0.95,
            "the hit lamp flares white: {white:?}"
        );
        assert!(
            bright > STROBE_FLASH * 0.9,
            "and flashes in absolute terms, not as a factor on a dim \
             fixture: {bright}"
        );
        let (kept, held_back) = look(app.world_mut(), dark);
        assert!(
            kept.green < 0.5,
            "a lamp between hits keeps its colour: {kept:?}"
        );
        assert!(
            held_back < bright * 0.5,
            "and gives way to the flash instead of matching it: {held_back}"
        );
        // The fixture's own beam goes white with it, and the one
        // between hits keeps its colour: the coloured additive cone
        // is what the eye sees at a fixture, and flashing the light
        // alone was invisible on the near, large ones.
        let beam_of = |world: &mut World, lamp: usize| -> Handle<StandardMaterial> {
            world
                .query::<(&rig::RigBeam, &MeshMaterial3d<StandardMaterial>)>()
                .iter(world)
                .find(|(beam, _)| beam.lamp == lamp)
                .map(|(_, material)| material.0.clone())
                .expect("a beam")
        };
        assert_eq!(beam_of(app.world_mut(), firing), flash_material);
        assert_eq!(beam_of(app.world_mut(), waiting), base_material);
        let (key_color, _) = look(app.world_mut(), key);
        assert!(
            key_color.green < 0.5,
            "the key light is not part of the ceiling"
        );
        // The level drops and the arming runs out: one restoring
        // pass, then nothing is written.
        app.insert_resource(Ears(Some(beatbyte_audio::listen::Listener::stub(
            true, -95.0, None,
        ))));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(STROBE_HOLD_S * 2.0));
        app.world_mut().resource_mut::<Highlight>().punch = 0.0;
        app.world_mut()
            .run_system_once(drive_highlight)
            .expect("the system runs");
        assert_eq!(
            beam_of(app.world_mut(), firing),
            base_material,
            "and the beam comes back too"
        );
        for lamp in [lit, dark, key] {
            let (color, intensity) = look(app.world_mut(), lamp);
            assert!(
                (color.red - 1.0).abs() < 1e-3 && color.green < 0.3,
                "back to its own colour: {color:?}"
            );
            assert!((intensity - 1000.0).abs() < 1e-3, "and its own intensity");
        }
        assert!(!app.world().resource::<Highlight>().was_active);
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
    fn a_comet_is_a_head_with_a_tail_behind_it_and_a_whisker_in_front() {
        assert!((comet_brightness(0.5, 0.5, 1.0) - 1.0).abs() < 1e-6);
        // Behind the head: an exponential tail.
        assert!(comet_brightness(0.4, 0.5, 1.0) < 1.0);
        assert!(comet_brightness(0.4, 0.5, 1.0) > comet_brightness(0.3, 0.5, 1.0));
        // In front: a whisker that dies within a bar or two — a bow,
        // not the wall the first cut had.
        let close = comet_brightness(0.505, 0.5, 1.0);
        assert!(close > 0.1 && close < 1.0, "a bow of glow: {close}");
        assert!(
            comet_brightness(0.6, 0.5, 1.0) < 0.001,
            "and nothing further ahead"
        );
        // The other way round, everything mirrors.
        assert!(comet_brightness(0.4, 0.5, -1.0) < 0.001);
        assert!(comet_brightness(0.6, 0.5, -1.0) > 0.0);
    }

    #[test]
    fn the_comet_enters_fast_and_leaves_the_strip_whole() {
        let span = 2.0f32.mul_add(COMET_TAIL, 1.0);
        assert!(
            (comet_head(0.0, 1.0) + COMET_TAIL).abs() < 1e-6,
            "a tail before"
        );
        assert!(
            (comet_head(1.0, 1.0) - (1.0 + COMET_TAIL)).abs() < 1e-5,
            "a tail past"
        );
        let mut last = comet_head(0.0, 1.0);
        for k in 1..=50 {
            let now = comet_head(k as f32 / 50.0, 1.0);
            assert!(now > last, "the head only ever moves forward");
            last = now;
        }
        let half = (comet_head(0.5, 1.0) + COMET_TAIL) / span;
        assert!(half > 0.7, "a constant crawl would sit at 0.5: {half}");
        // Mirrored, and clamped outside its run.
        assert!((comet_head(0.0, -1.0) - (1.0 + COMET_TAIL)).abs() < 1e-6);
        assert!(comet_head(1.0, -1.0) < 0.0);
        assert_eq!(comet_head(-1.0, 1.0), comet_head(0.0, 1.0));
        assert_eq!(comet_head(2.0, 1.0), comet_head(1.0, 1.0));
    }

    #[test]
    fn a_spark_is_a_cluster_that_dies_by_dimming() {
        // One generation alone, so a single spark can be watched.
        let alone = 10.0;
        let lit: Vec<usize> = (0..STRIP_BARS)
            .filter(|bar| sparkle_brightness(0, *bar, 0.0, alone) > 0.0)
            .collect();
        assert!(
            (SPARK_MIN_BARS..=SPARK_MAX_BARS).contains(&lit.len()),
            "a cluster of two to five bars, not a lone dot: {lit:?}"
        );
        assert!(
            lit.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "and its bars are neighbours: {lit:?}"
        );
        let bar = lit[0];
        let born = sparkle_brightness(0, bar, 0.0, alone);
        let half = sparkle_brightness(0, bar, SPARK_DECAY_S * 0.5, alone);
        let late = sparkle_brightness(0, bar, SPARK_DECAY_S * 0.9, alone);
        assert!(
            born > half && half > late && late > 0.0,
            "it dims, it does not switch"
        );
        assert!(
            (half / born - 0.25).abs() < 0.02,
            "quadratically: a quarter as bright half way through: {}",
            half / born
        );
        assert_eq!(
            sparkle_brightness(0, bar, SPARK_DECAY_S + 0.01, alone),
            0.0,
            "and then it is gone"
        );
        // Bars of one cluster do not share a brightness: it is a
        // spray of sparks, not a lit block.
        if lit.len() > 1 {
            let peaks: Vec<f32> = lit
                .iter()
                .map(|b| sparkle_brightness(0, *b, 0.0, alone))
                .collect();
            assert!(
                peaks
                    .windows(2)
                    .any(|pair| (pair[0] - pair[1]).abs() > 0.01),
                "every bar its own peak: {peaks:?}"
            );
        }
    }

    #[test]
    fn a_running_sparkle_stays_sparse_and_keeps_making_sparks() {
        let lit = |t: f32| -> Vec<usize> {
            (0..STRIP_BARS)
                .filter(|bar| sparkle_brightness(1, *bar, t, SPARK_EVERY_S) > 0.02)
                .collect()
        };
        let mut ever = false;
        for k in 0..50 {
            let t = 0.02f32.mul_add(k as f32, 0.05);
            let bars = lit(t);
            ever |= !bars.is_empty();
            assert!(
                bars.len() < STRIP_BARS / 2,
                "a sparkle is sparse, not a wash: {} of {STRIP_BARS} at {t}",
                bars.len()
            );
        }
        assert!(ever, "something sparkles");
        assert_ne!(lit(0.10), lit(0.60), "new sparks keep being born");
        assert_eq!(lit(0.35), lit(0.35), "deterministic");
        // The effect outlives its last spark's birth, so it ends by
        // going out rather than by being cut off.
        assert!(Effect::Sparkle.duration() > SPARKLE_SPAWN_S);
        assert!((Effect::Sparkle.duration() - (SPARKLE_SPAWN_S + SPARK_DECAY_S)).abs() < 1e-6);
    }

    #[test]
    fn the_schedule_is_sporadic_deterministic_and_mixes_both_effects() {
        let mut comets = 0;
        let mut sparkles = 0;
        for count in 0..40 {
            let (effect, quiet) = pick(0, count);
            assert!((QUIET_MIN_S..=QUIET_MIN_S + QUIET_SPREAD_S).contains(&quiet));
            match effect {
                Effect::Comet { dir } => {
                    assert!(dir == 1.0 || dir == -1.0);
                    comets += 1;
                }
                Effect::Sparkle => sparkles += 1,
            }
        }
        assert!(
            comets > 5 && sparkles > 5,
            "{comets} comets, {sparkles} sparkles"
        );
        assert!(comets > sparkles, "the comet leads");
        assert_eq!(pick(3, 7), pick(3, 7));
        assert_ne!(pick(3, 7).1, pick(4, 7).1, "strips do not fire in step");
    }
}
