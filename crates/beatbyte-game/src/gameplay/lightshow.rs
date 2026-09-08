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
use super::PlayerSession;
use super::arc::segment_pose;
use super::crowd::{BARRIER_X, CROWD_FLOOR_Y};
use super::fx::hash01;
use super::monitors::Ears;
use super::pa;
use super::rig;
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use crate::audio_sys::GameClock;
use crate::config::{FlashSync, Settings};
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

/// How long one flash lasts.
pub const FLASH_S: f32 = 0.09;
/// The gap between two flashes of the same burst: far enough apart
/// to read as two hits rather than a flicker.
pub const FLASH_GAP_S: f32 = 0.16;
/// The fewest flashes in a burst.
pub const BURST_MIN: u32 = 1;
/// The most.
pub const BURST_MAX: u32 = 3;
/// The shortest rest between two bursts.
pub const REST_MIN_S: f32 = 2.0;
/// The longest.
pub const REST_MAX_S: f32 = 3.0;
/// Beats between two flashes of one burst, on the song's clock.
///
/// A beat is the natural gap: at 160 BPM it is longer than the wall
/// clock's [`FLASH_GAP_S`], so the ceiling gets calmer rather than
/// busier, and every flash lands where the drummer does.
pub const BEAT_GAP: f32 = 1.0;
/// Beats to a bar while the flashes count them.
///
/// The tempo map carries the beats, not the downbeats, so bars are
/// counted in fours from the chart's first tracked beat — the
/// convention the grid itself uses when no stage knew the downbeats.
/// It decides where a burst STARTS; whether a flash is ON the beat
/// does not depend on it.
pub const FLASH_BEATS_PER_BAR: f32 = 4.0;
/// The shortest rest between two bursts, in beats, before it is
/// rounded up to the next bar line.
pub const REST_MIN_BEATS: f32 = 3.0;
/// The spread on top of it, in beats.
pub const REST_SPAN_BEATS: f32 = 3.0;
/// Lamps hit per flash. A single lamp reads as a twinkle; a pair
/// reads as a hit.
pub const STROBE_AT_ONCE: usize = 2;
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

/// The flash's shape `since` seconds into it, whichever lamps are
/// firing: a hard rise, a plateau, a fast fall, nothing after. The
/// venue's wash dips by this too, so the ceiling flashes against a
/// darker room. Pure — tested.
#[must_use]
pub fn flash_shape(since: f32) -> f32 {
    if !(0.0..FLASH_S).contains(&since) {
        return 0.0;
    }
    // A xenon tube: instant rise, a plateau, a fast fall. An
    // exponential spent most of its lit share nearly dark, which
    // averages to a tint rather than a flash.
    let across = since / FLASH_S;
    (1.0 - across * across * across * across).max(0.0)
}

/// Which two lamps the `flash`-th flash lights: the next pair of the
/// shuffled order, so every lamp is hit once per pass through the
/// rig and no two passes run the same order — across bursts, not
/// within one. Pure — tested.
#[must_use]
pub fn flash_lamps(flash: u32, lamps: usize) -> [usize; STROBE_AT_ONCE] {
    let slots = (lamps.max(1)).div_ceil(STROBE_AT_ONCE) as u32;
    let order = strobe_order(flash / slots, lamps);
    let slot = (flash % slots) as usize;
    core::array::from_fn(|k| {
        let index = slot * STROBE_AT_ONCE + k;
        order[index.min(lamps.saturating_sub(1))] as usize
    })
}

/// How hard lamp `lamp` is hit `since` seconds into flash number
/// `flash`, 0..1. Pure — tested.
#[must_use]
pub fn strobe_hit(lamp: usize, flash: u32, lamps: usize, since: f32) -> f32 {
    if lamps == 0 || lamp >= lamps {
        return 0.0;
    }
    if flash_lamps(flash, lamps).contains(&lamp) {
        flash_shape(since)
    } else {
        0.0
    }
}

/// The clock a burst is scheduled on, and what it snaps to.
///
/// Both plans do the same thing — a burst of flashes, then a rest —
/// and differ only in the unit their positions are measured in and
/// in whether they round. [`Plan::SECONDS`] runs on the wall clock
/// and rounds to nothing; [`Plan::BEATS`] runs on the song's beats,
/// puts every flash on a whole beat and every burst on a bar line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Between two flashes of one burst, in this plan's unit.
    pub gap: f32,
    /// The shortest rest after a burst.
    pub rest_min: f32,
    /// The spread on top of it.
    pub rest_span: f32,
    /// What a flash inside a burst snaps to; zero snaps to nothing.
    pub beat: f32,
    /// What the first flash of a burst snaps to; zero snaps to nothing.
    pub bar: f32,
}

impl Plan {
    /// The wall clock: the pacing the ceiling has always run.
    pub const SECONDS: Plan = Plan {
        gap: FLASH_GAP_S,
        rest_min: REST_MIN_S,
        rest_span: REST_MAX_S - REST_MIN_S,
        beat: 0.0,
        bar: 0.0,
    };
    /// The song's own clock.
    pub const BEATS: Plan = Plan {
        gap: BEAT_GAP,
        rest_min: REST_MIN_BEATS,
        rest_span: REST_SPAN_BEATS,
        beat: 1.0,
        bar: FLASH_BEATS_PER_BAR,
    };

    /// The longest rest this plan can roll — and so the smallest
    /// backwards step that cannot be a clock correcting itself.
    #[must_use]
    pub fn longest_rest(&self) -> f32 {
        self.rest_min + self.rest_span
    }

    /// How long the rest after burst `number` lasts, in this plan's
    /// unit. Pure — tested.
    #[must_use]
    pub fn rest_after(&self, number: u32) -> f32 {
        self.rest_span
            .mul_add(hash01(number as usize * 3391 + 29), self.rest_min)
    }

    /// Where a flash may begin, given the position the schedule
    /// asked for: the next bar line when a burst is `starting`, the
    /// nearest whole beat inside one, and `pos` itself on a plan that
    /// snaps to nothing.
    ///
    /// A burst START rounds UP so the rest is never cut below
    /// [`Plan::rest_min`]; a flash inside a burst rounds to the
    /// NEAREST beat, because rounding up there would skip the beat
    /// it was aiming at. Pure — tested.
    #[must_use]
    pub fn snap(&self, pos: f32, starting: bool) -> f32 {
        let step = if starting { self.bar } else { self.beat };
        if step <= 0.0 {
            return pos;
        }
        let steps = pos / step;
        if starting {
            steps.ceil() * step
        } else {
            steps.round() * step
        }
    }
}

/// The plan a setting asks for. Pure — tested.
#[must_use]
pub fn flash_plan(sync: FlashSync) -> Plan {
    match sync {
        FlashSync::Level => Plan::SECONDS,
        FlashSync::Beat => Plan::BEATS,
    }
}

/// The strobe's own pacing: bursts of a few flashes with a rest of
/// a couple of seconds between them, both rolled from a hash of the
/// burst's number.
///
/// The first cut ran a flat twelve flashes a second for as long as
/// the level stayed over the threshold, which reads as one continuous
/// flicker — reported as too hectic and too wild. A burst and a rest
/// is how a lighting desk plays a strobe: a hit, maybe two or three,
/// then the room breathes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burst {
    /// Which burst this is, the seed for its roll.
    pub number: u32,
    /// Flashes left in it, this one included.
    pub left: u32,
    /// When the next flash begins.
    pub next_at: f32,
    /// The flash on screen, counted from the rig's start — the index
    /// the lamp order is read at. `u32::MAX` before the first, so the
    /// first flash is number zero.
    pub flash: u32,
    /// When the flash on screen began.
    pub lit_at: f32,
    /// When the schedule was last advanced, so a rest that runs out
    /// while the ceiling is dark does not fire the moment the room
    /// gets loud again.
    pub last_tick: Option<f32>,
}

impl Default for Burst {
    fn default() -> Self {
        Burst {
            number: 0,
            left: 0,
            next_at: f32::MIN,
            flash: u32::MAX,
            lit_at: f32::MIN,
            last_tick: None,
        }
    }
}

/// How many flashes burst `number` carries: [`BURST_MIN`]..=[`BURST_MAX`].
/// Pure — tested.
#[must_use]
pub fn burst_length(number: u32) -> u32 {
    let span = BURST_MAX - BURST_MIN + 1;
    BURST_MIN + ((hash01(number as usize * 7717 + 13) * span as f32) as u32).min(span - 1)
}

impl Burst {
    /// Advance the schedule to `pos` — the position on the `plan`'s
    /// own clock, seconds or beats — and say how hard the ceiling is
    /// flashing: the flash that is on screen and how far into it we
    /// are, in SECONDS, because a flash's own life is wall time
    /// whatever schedules it. Pure — tested.
    pub fn tick(&mut self, now: f32, pos: f32, plan: Plan) -> (u32, f32) {
        match self.last_tick {
            // The position went a long way backwards: another song,
            // or a switch of clock. Without this the schedule would
            // wait for the old position to come round again and the
            // ceiling would stay dark for the rest of the song.
            //
            // It has to be a LONG way. The song clock corrects itself
            // against the audio device (a snap of 30 ms, a 10 % slew)
            // and steps back a fraction of a beat when the count-in
            // hands over to the music. Restarting on those measured
            // as a fresh burst two beats after the last one — six
            // flashes in six beats at the top of the song, which is
            // the flicker the pacing exists to prevent. A step back
            // smaller than the longest rest is a clock correcting
            // itself: the schedule simply carries on, and the flash
            // it was waiting for arrives that fraction later.
            Some(last) if pos < last - plan.longest_rest() => {
                self.next_at = plan.snap(pos, true);
            }
            // The schedule freezes while the ceiling is dark. The
            // threshold bit flickers with the music, so a rest left to
            // run through the quiet stretches would be over every time
            // the room came back and the pacing would follow the music
            // instead of the schedule (measured: 17 % of armed frames
            // lit where the schedule asks for 6).
            Some(last) if pos - last > plan.gap => {
                self.next_at = plan.snap(self.next_at + (pos - last), self.left == 0);
            }
            None => self.next_at = plan.snap(pos, true),
            Some(_) => {}
        }
        self.last_tick = Some(pos);
        if pos >= self.next_at {
            if self.left == 0 {
                self.left = burst_length(self.number);
            }
            self.lit_at = now;
            self.flash = self.flash.wrapping_add(1);
            self.left -= 1;
            self.next_at = if self.left == 0 {
                let rest = plan.rest_after(self.number);
                self.number = self.number.wrapping_add(1);
                plan.snap(pos + rest, true)
            } else {
                plan.snap(pos + plan.gap, false)
            };
        }
        (self.flash, now - self.lit_at)
    }
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

/// Whether the strobe is armed this instant, before the hold: on
/// the room's level, the live threshold bit; on the song's beat,
/// simply that a song is running — a rhythm needs no permission from
/// a microphone. `BEATBYTE_LIGHTSHOW` arms it either way. Pure —
/// tested (the decision, not only its branches).
#[must_use]
pub fn armed_now(sync: FlashSync, over: bool, playing: bool, forced: bool) -> bool {
    forced
        || match sync {
            FlashSync::Level => over,
            FlashSync::Beat => playing,
        }
}

/// Whether the song crossed a bar line between two frames — the
/// swell's rising edge on the song's clock, as the threshold's
/// rising edge is on the room's.
///
/// Which bar a position sits in rises with the position, so asking
/// for a HIGHER bar than last frame already excludes a clock that
/// went backwards; an explicit direction guard was written here and
/// removed again when no mutation of it could be made to fail.
/// Pure — tested.
#[must_use]
pub fn crossed_bar(before: f32, now: f32) -> bool {
    (now / FLASH_BEATS_PER_BAR).floor() > (before / FLASH_BEATS_PER_BAR).floor()
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
    /// The burst schedule: how the flashes are paced.
    pub burst: Burst,
    /// Where the song stood last frame, in beats — the swell's
    /// rising edge on the song's clock needs both sides of a bar
    /// line. `None` while no song is running.
    pub last_beat: Option<f32>,
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
    game_clock: Res<GameClock>,
    players: Query<&PlayerSession>,
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
    // Where the song stands, in beats off its tracked grid: the other
    // clock the flashes may run on, and the only one that needs no
    // microphone.
    let beats = game_clock.song_time(&time).and_then(|song| {
        players
            .iter()
            .next()
            .map(|player| player.session.track().tempo.beats_at(song) as f32)
    });
    let sync = settings.flash_sync;
    // The swell's rising edge is the threshold's on the room's clock
    // and a bar line on the song's.
    let rising = match sync {
        FlashSync::Level => rising,
        FlashSync::Beat => match (highlight.last_beat, beats) {
            (Some(before), Some(at)) => crossed_bar(before, at),
            _ => false,
        },
    };
    highlight.last_beat = beats;
    highlight.punch = advance_punch(highlight.punch, rising, settings.reduced_flashing, dt);
    let armed = armed_now(
        sync,
        over,
        beats.is_some(),
        *show.get_or_insert_with(forced),
    );
    highlight.strobe_until = strobe_armed_until(armed, now, highlight.strobe_until);
    // The chase runs on its own clock and the arming only gates it.
    // Anchoring it to the arming instead restarted the cycle on
    // every edge — and the threshold bit flickers with the music, so
    // the same first pair fired over and over for a few milliseconds
    // each time (seen live: not one white frame in six).
    let plan = flash_plan(sync);
    let position = match sync {
        FlashSync::Level => Some(now),
        FlashSync::Beat => beats,
    };
    let strobe = position
        .filter(|_| strobing(now, highlight.strobe_until, settings.reduced_flashing))
        .map(|pos| highlight.burst.tick(now, pos, plan));
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
    let shape = strobe.map_or(0.0, |(_, since)| flash_shape(since));
    let dip = STROBE_DIP.mul_add(-shape, 1.0);
    // Every firing lamp of a step shares one shape, so the ten hits
    // are computed once and read by the lights AND their beams.
    let hits_by_lamp: [f32; rig::RIG_LAMPS] = core::array::from_fn(|lamp| {
        strobe.map_or(0.0, |(flash, since)| {
            strobe_hit(lamp, flash, rig::RIG_LAMPS, since)
        })
    });
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
        // Only a second that actually flashed says anything: on the
        // song's clock the strobe is armed for the whole song, and a
        // line a second either way would be noise rather than a
        // diagnostic.
        if tally.1 > 0 {
            info!(
                "strobe: {} of {} frames had a hit, {ceiling} lamps, \
                 {beams_lit}/{beams_seen} beams white, now {hits:?}, \
                 {sync:?} at {:.2}",
                tally.1,
                tally.0,
                // The schedule's own position: seconds on the wall
                // clock, beats on the song's. Printing the beats
                // either way would read NaN through half the game.
                position.unwrap_or(f32::NAN)
            );
        }
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
    fn every_lamp_is_hit_once_a_pass_and_the_pairs_never_repeat_a_lamp() {
        let lamps = rig::RIG_LAMPS;
        let slots = lamps.div_ceil(STROBE_AT_ONCE) as u32;
        let mut seen = vec![0u32; lamps];
        for flash in 0..slots {
            let pair = flash_lamps(flash, lamps);
            assert_ne!(pair[0], pair[1], "a flash lights two different lamps");
            for lamp in pair {
                seen[lamp] += 1;
            }
        }
        assert!(
            seen.iter().all(|count| *count == 1),
            "one pass covers every lamp exactly once: {seen:?}"
        );
        // The next pass is a different order.
        let first: Vec<_> = (0..slots).map(|f| flash_lamps(f, lamps)).collect();
        let second: Vec<_> = (slots..2 * slots).map(|f| flash_lamps(f, lamps)).collect();
        assert_ne!(first, second, "two passes must not run the same order");
        assert_eq!(
            flash_lamps(3, lamps),
            flash_lamps(3, lamps),
            "deterministic"
        );
    }

    #[test]
    fn a_flash_is_a_hit_with_a_hard_edge_and_then_nothing() {
        let lamps = rig::RIG_LAMPS;
        let lamp = flash_lamps(0, lamps)[0];
        assert!((strobe_hit(lamp, 0, lamps, 0.0) - 1.0).abs() < 1e-6);
        assert!(
            strobe_hit(lamp, 0, lamps, FLASH_S * 0.5) > 0.9,
            "it holds through its length"
        );
        let late = strobe_hit(lamp, 0, lamps, FLASH_S * 0.99);
        assert!(late > 0.0 && late < 0.1, "and then falls away fast: {late}");
        assert_eq!(strobe_hit(lamp, 0, lamps, FLASH_S), 0.0, "gone at its end");
        assert_eq!(strobe_hit(lamp, 0, lamps, -1.0), 0.0);
        // A lamp that is not in this flash's pair stays dark.
        let dark = (0..lamps)
            .find(|l| !flash_lamps(0, lamps).contains(l))
            .expect("a lamp between hits");
        assert_eq!(strobe_hit(dark, 0, lamps, 0.0), 0.0);
    }

    #[test]
    fn the_ceiling_flashes_in_bursts_with_the_room_breathing_between() {
        // Reported: a flat twelve flashes a second reads as one
        // continuous flicker. A burst of one to three, then a rest of
        // two to three seconds, both rolled per burst.
        let mut burst = Burst::default();
        let mut lit_at: Vec<f32> = Vec::new();
        let mut t = 0.0f32;
        let mut last_flash = 0;
        while t < 30.0 {
            let (flash, since) = burst.tick(t, t, Plan::SECONDS);
            if flash != last_flash {
                last_flash = flash;
                lit_at.push(t);
            }
            assert!(since >= 0.0);
            t += 1.0 / 240.0;
        }
        assert!(
            (8..=40).contains(&lit_at.len()),
            "half a minute is a handful of bursts, not a flicker: {}",
            lit_at.len()
        );
        // The gaps come in two kinds: inside a burst, and between.
        let gaps: Vec<f32> = lit_at.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let inside: Vec<f32> = gaps.iter().copied().filter(|g| *g < 1.0).collect();
        let between: Vec<f32> = gaps.iter().copied().filter(|g| *g >= 1.0).collect();
        assert!(!inside.is_empty(), "bursts of more than one flash exist");
        assert!(!between.is_empty(), "and the room rests between them");
        assert!(
            inside.iter().all(|g| (*g - FLASH_GAP_S).abs() < 0.02),
            "inside a burst the flashes are one gap apart: {inside:?}"
        );
        assert!(
            between
                .iter()
                .all(|g| (REST_MIN_S - 0.05..=REST_MAX_S + FLASH_GAP_S).contains(g)),
            "and a rest is two to three seconds: {between:?}"
        );
        // Randomised, not a metronome: the rests differ.
        let first = between[0];
        assert!(
            between.iter().any(|g| (g - first).abs() > 0.05),
            "the rests must vary: {between:?}"
        );
    }

    #[test]
    fn a_burst_is_one_to_three_flashes_and_its_rest_is_rolled_too() {
        let lengths: Vec<u32> = (0..60).map(burst_length).collect();
        assert!(lengths.iter().all(|n| (BURST_MIN..=BURST_MAX).contains(n)));
        assert!(lengths.contains(&BURST_MIN) && lengths.contains(&BURST_MAX));
        let rests: Vec<f32> = (0..60).map(|n| Plan::SECONDS.rest_after(n)).collect();
        assert!(rests.iter().all(|r| (REST_MIN_S..=REST_MAX_S).contains(r)));
        let spread = rests.iter().copied().fold(f32::MIN, f32::max)
            - rests.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            spread > 0.5,
            "the rests spread across their range: {spread}"
        );
        assert_eq!(burst_length(7), burst_length(7), "deterministic");
    }

    #[test]
    fn the_songs_clock_puts_every_flash_on_a_beat_and_every_burst_on_a_bar() {
        // The whole point of the setting: the ceiling stops flashing
        // at the room's loudness and starts flashing with the band.
        let mut burst = Burst::default();
        let mut lit: Vec<f32> = Vec::new();
        let mut starts: Vec<f32> = Vec::new();
        let mut last_flash = u32::MAX;
        let mut opening = true;
        // 120 BPM: two beats a second, sampled at 240 Hz.
        for step in 0..(60 * 240) {
            let t = step as f32 / 240.0;
            let beats = t * 2.0;
            let (flash, _) = burst.tick(t, beats, Plan::BEATS);
            if flash != last_flash {
                if opening || beats - lit.last().copied().unwrap_or(beats) > BEAT_GAP * 1.5 {
                    starts.push(beats);
                }
                opening = false;
                last_flash = flash;
                lit.push(beats);
            }
        }
        assert!(lit.len() > 8, "the ceiling flashes at all: {}", lit.len());
        // One frame at this rate is 0.0084 beats, and a crossing is
        // caught on the frame after it: everything else is a flash
        // that missed its beat.
        for beat in &lit {
            let off = (beat - beat.round()).abs();
            assert!(off < 0.02, "a flash off the beat by {off} at {beat}");
        }
        for start in &starts {
            let bar = start / FLASH_BEATS_PER_BAR;
            assert!(
                (bar - bar.round()).abs() < 0.01,
                "a burst starting off the bar at beat {start}"
            );
        }
        let gaps: Vec<f32> = lit.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let inside: Vec<f32> = gaps.iter().copied().filter(|g| *g < 2.0).collect();
        let between: Vec<f32> = gaps.iter().copied().filter(|g| *g >= 2.0).collect();
        assert!(!inside.is_empty() && !between.is_empty());
        assert!(
            inside.iter().all(|g| (*g - BEAT_GAP).abs() < 0.02),
            "inside a burst the flashes are a beat apart: {inside:?}"
        );
        assert!(
            between
                .iter()
                .all(|g| (REST_MIN_BEATS..=2.0 * FLASH_BEATS_PER_BAR + 0.1).contains(g)),
            "a rest is one or two bars: {between:?}"
        );
        let first = between[0];
        assert!(
            between.iter().any(|g| (g - first).abs() > 0.5),
            "and the rests are rolled, not a metronome: {between:?}"
        );
    }

    #[test]
    fn a_burst_starts_on_the_next_bar_and_a_flash_inside_it_on_the_nearest_beat() {
        // Rounding UP at a burst start keeps the rest at least as
        // long as it was rolled; rounding to the NEAREST inside a
        // burst keeps the flash on the beat it was aiming at, which
        // rounding up would skip.
        let plan = Plan::BEATS;
        assert!((plan.snap(5.02, true) - 8.0).abs() < 1e-5);
        assert!(
            (plan.snap(8.0, true) - 8.0).abs() < 1e-5,
            "already on a bar"
        );
        assert!(
            (plan.snap(4.02, false) - 4.0).abs() < 1e-5,
            "the beat it aims at"
        );
        assert!((plan.snap(3.98, false) - 4.0).abs() < 1e-5);
        // The wall clock rounds to nothing at all.
        for pos in [0.0, 1.7, 12.34] {
            assert!((Plan::SECONDS.snap(pos, true) - pos).abs() < 1e-6);
            assert!((Plan::SECONDS.snap(pos, false) - pos).abs() < 1e-6);
        }
    }

    #[test]
    fn the_setting_chooses_the_clock() {
        // The decision itself, not only the two plans it picks
        // between: a test of each branch alone stays green while the
        // code always answers the same one.
        assert_eq!(flash_plan(FlashSync::Level), Plan::SECONDS);
        assert_eq!(flash_plan(FlashSync::Beat), Plan::BEATS);
        assert_ne!(Plan::SECONDS, Plan::BEATS);
        assert_eq!(Plan::SECONDS.beat, 0.0, "the wall clock knows no beats");
        const {
            assert!(Plan::BEATS.beat > 0.0 && Plan::BEATS.bar > 0.0);
        }
    }

    #[test]
    fn the_beat_needs_no_microphone_and_the_level_needs_the_threshold() {
        // On the room's level nothing fires until the level is over;
        // on the song's beat it is enough that a song is running.
        assert!(!armed_now(FlashSync::Level, false, true, false));
        assert!(armed_now(FlashSync::Level, true, false, false));
        assert!(!armed_now(FlashSync::Beat, true, false, false));
        assert!(armed_now(FlashSync::Beat, false, true, false));
        // And the harness switch arms either clock.
        for sync in [FlashSync::Level, FlashSync::Beat] {
            assert!(armed_now(sync, false, false, true));
        }
    }

    #[test]
    fn a_bar_line_is_crossed_once_a_bar_and_not_by_going_back() {
        assert!(crossed_bar(3.9, 4.1), "the bar line between them");
        assert!(!crossed_bar(4.1, 4.9), "a beat inside the bar is not one");
        assert!(!crossed_bar(1.9, 2.1));
        assert!(
            !crossed_bar(4.1, 3.9),
            "and running back over one is not crossing it"
        );
        assert!(!crossed_bar(4.0, 4.0));
        let mut crossings = 0;
        let mut before = 0.0f32;
        for step in 1..=(16 * 100) {
            let now = step as f32 / 100.0;
            if crossed_bar(before, now) {
                crossings += 1;
            }
            before = now;
        }
        assert_eq!(crossings, 4, "sixteen beats are four bars");
    }

    #[test]
    fn a_clock_correcting_itself_backwards_is_not_a_new_song() {
        // Measured live before this rule existed: the song clock
        // steps back a fraction of a beat when the count-in hands
        // over to the music (it anchors to the audio device, which
        // snaps at 30 ms), the schedule read that as a fresh
        // timeline, and the top of the song flashed on six beats
        // running — the flicker the burst pacing exists to prevent.
        let mut burst = Burst::default();
        let mut pos = 0.0f32;
        // Run into a rest with room to spare.
        loop {
            burst.tick(pos, pos, Plan::BEATS);
            if burst.left == 0 && burst.next_at > pos + 1.0 {
                break;
            }
            pos += 0.05;
            assert!(pos < 100.0, "a rest must come");
        }
        let scheduled = burst.next_at;
        let before = burst.flash;
        // The clock corrects itself by a fraction of a beat.
        let (after, _) = burst.tick(pos, pos - 0.2, Plan::BEATS);
        assert_eq!(after, before, "no flash: the clock only nudged");
        assert!(
            (burst.next_at - scheduled).abs() < 1e-6,
            "and the schedule it was already keeping is untouched: \
             {scheduled} became {}",
            burst.next_at
        );
    }

    #[test]
    fn a_new_song_puts_the_schedule_back_rather_than_leaving_the_ceiling_dark() {
        // The resource outlives a song. Without this the schedule
        // would sit at a position the new song reaches minutes later
        // — or never — and the ceiling would stay dark for all of it.
        let mut burst = Burst::default();
        let mut last = u32::MAX;
        let mut flashes = 0;
        for step in 0..(240 * 120) {
            let t = step as f32 / 240.0;
            let (flash, _) = burst.tick(t, t * 2.0, Plan::BEATS);
            if flash != last {
                last = flash;
                flashes += 1;
            }
        }
        assert!(flashes > 10, "a long song flashes: {flashes}");
        // The next song starts at beat zero again.
        let mut fresh = 0;
        for step in 0..(240 * 20) {
            let t = 120.0 + step as f32 / 240.0;
            let (flash, _) = burst.tick(t, step as f32 / 240.0 * 2.0, Plan::BEATS);
            if flash != last {
                last = flash;
                fresh += 1;
            }
        }
        assert!(fresh > 2, "and so does the next one: {fresh}");
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
        // Every firing lamp of a flash shares one shape, and that
        // shape is what the rest of the room dips by: the strobe
        // reads by contrast.
        let lamps = rig::RIG_LAMPS;
        for k in 0..30 {
            let since = k as f32 * 0.005;
            let shape = flash_shape(since);
            let lit: Vec<f32> = (0..lamps)
                .map(|lamp| strobe_hit(lamp, 0, lamps, since))
                .filter(|hit| *hit > 0.0)
                .collect();
            assert!(
                lit.iter().all(|hit| (hit - shape).abs() < 1e-6),
                "one shape for every lamp of the flash: {lit:?} vs {shape}"
            );
            if lit.is_empty() {
                assert_eq!(shape, 0.0, "and nothing to dip by between flashes");
            }
        }
        assert!((flash_shape(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(flash_shape(-1.0), 0.0);
        // The dip is a real darkening at the peak, and nothing
        // between flashes.
        assert!(
            STROBE_DIP.mul_add(-flash_shape(0.0), 1.0) < 0.6,
            "the room gives way"
        );
        let gap = STROBE_DIP.mul_add(-flash_shape(FLASH_S + 0.01), 1.0);
        assert!((gap - 1.0).abs() < 1e-6, "and stands up again between them");
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
        let firing = flash_lamps(0, lamps)[0];
        let waiting = (0..lamps)
            .find(|lamp| !flash_lamps(0, lamps).contains(lamp))
            .expect("a lamp between hits");
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
        app.init_resource::<GameClock>();
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
    fn the_ceiling_flashes_on_the_song_alone_with_no_microphone_in_the_room() {
        // The setting's whole promise: on the song's clock the show
        // runs with no input device at all — where the room's level
        // would leave the ceiling dark for ever. And it runs on the
        // SONG's positions: the first frame stands three beats in,
        // between bar lines, and must NOT flash; the second stands on
        // the bar and must. A schedule reading the wall clock instead
        // would fire on the first.
        use beatbyte_core::{
            Difficulty, Lane, LaneSet, NoteEvent, NoteKind, ScoreConfig, TempoMap, TimingWindows,
            Track, TrackSession,
        };
        use bevy::ecs::system::RunSystemOnce;

        let lamps = rig::RIG_LAMPS;
        let firing = flash_lamps(0, lamps)[0];
        let tone = Color::srgb(1.0, 0.2, 0.1);
        // 120 BPM: beat 3 at 1.5 s (between bars), beat 4 at 2.0 s.
        let run = |sync: FlashSync, calm: bool, song: &[f64]| {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default()));
            app.init_asset::<StandardMaterial>();
            app.insert_resource(Settings {
                stage_3d: true,
                flash_sync: sync,
                reduced_flashing: calm,
                ..Settings::default()
            });
            app.init_resource::<Highlight>();
            // No input device: nothing is measured, ever.
            app.insert_resource(Ears(None));
            app.init_resource::<GameClock>();
            let track = Track::new(
                Difficulty::Medium,
                TempoMap::constant(120.0, 0.0),
                vec![NoteEvent {
                    time_s: 1.0,
                    lanes: LaneSet::single(Lane::One),
                    kind: NoteKind::Strum,
                    sustain_s: 0.0,
                }],
                Vec::new(),
            )
            .expect("a valid track");
            app.world_mut().spawn(PlayerSession {
                session: TrackSession::new(track, TimingWindows::default(), ScoreConfig::default()),
                frame_events: Vec::new(),
                spawn_cursor: 0,
            });
            let lamp = app
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
            let mut seen = Vec::new();
            for song_s in song {
                // The wall clock stands still across these frames —
                // only the song moves, which is the point.
                app.world_mut()
                    .resource_mut::<GameClock>()
                    .clock
                    .start(0.0, *song_s);
                app.world_mut()
                    .run_system_once(drive_highlight)
                    .expect("the system runs");
                let light = app.world().get::<SpotLight>(lamp).expect("a lamp");
                seen.push((light.color.to_srgba(), light.intensity));
            }
            seen
        };

        let white = |(color, bright): &(Srgba, f32)| {
            color.red > 0.95
                && color.green > 0.95
                && color.blue > 0.95
                && *bright > STROBE_FLASH * 0.9
        };
        let beat = run(FlashSync::Beat, false, &[1.5, 2.0]);
        assert!(
            !white(&beat[0]),
            "three beats in is between bar lines: {:?}",
            beat[0]
        );
        assert!(
            white(&beat[1]),
            "and the bar line flares the lamp white: {:?}",
            beat[1]
        );
        // The same room on the other clock: nothing is heard, so
        // nothing fires at all. That contrast IS the setting.
        let level = run(FlashSync::Level, false, &[1.5, 2.0]);
        for seen in &level {
            assert!(
                (seen.0.red - 1.0).abs() < 1e-3
                    && seen.0.blue < 0.2
                    && seen.1 < STROBE_FLASH * 0.01,
                "on the room's level an unheard room leaves the lamp alone: {seen:?}"
            );
        }
        // REDUCED FLASHING takes the strobe away and leaves the
        // swell — and on the song's clock the swell has a musical
        // edge to rise on, so that setting is no longer a room with
        // nothing in it at all.
        let swell = run(FlashSync::Beat, true, &[1.5, 2.0]);
        for seen in &swell {
            assert!(
                !white(seen),
                "reduced flashing never strobes, on any clock: {seen:?}"
            );
        }
        assert!(
            swell[1].1 > swell[0].1 * 1.1,
            "the bar line swells the lamp: {} then {}",
            swell[0].1,
            swell[1].1
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
