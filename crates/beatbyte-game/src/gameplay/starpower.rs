//! The impact when a star-power phrase lands whole.
//!
//! A phrase every note of which was hit credits the Hype meter, and
//! until now the only sign of it was the meter itself moving in the
//! corner. This is the moment made visible: the neck takes the energy
//! in, the ceiling flashes, the screen catches it — **one impulse**,
//! read by three systems, gone inside half a second.
//!
//! # Where the trigger comes from
//!
//! [`beatbyte_core::SessionEvent::PhraseCompleted`], which already
//! existed and is already exactly right: the session emits it when a
//! phrase's last note is hit AND the phrase was never broken, in the
//! same breath as `complete_phrase()` credits the meter. A single
//! star note does not produce it, a phrase with a miss in it does
//! not, and it cannot fire twice for one phrase. Nothing here knows
//! any of those rules — it reads the event and that is the whole
//! point: **no star-power logic lives in a visual system.**
//!
//! # One impulse, three consumers
//!
//! This module owns the clock and the shapes; the three systems that
//! already own their pixels read them.
//!
//! | What | Read by | Scope |
//! |---|---|---|
//! | [`highway_lift`] | `stage3d::tint_stage_for_hype` | per player — each neck is their own |
//! | [`lamp_burst`] | `lightshow::drive_highlight` | the stage, which everyone shares |
//! | [`screen_shape`] | `fx`'s screen flash | the screen, which everyone shares — **flat view only** |
//!
//! On the 3D stage the screen flash is gone (0.18.50): a lightning
//! strike on the last fret of the phrase marks the moment instead
//! (`strike.rs`, its own clock, armed off the same bus). Without the
//! 3D stage there are no neck surfaces, no ceiling and no bolt, so
//! the flat view reads the lift on its lane guide strips instead
//! (`notes::star_lift_guides`) and keeps the screen flash. The
//! impulse itself is the same clock either way.
//!
//! Nothing here writes a material, a light or a sprite. That matters
//! for more than tidiness: the neck's wash and the ceiling's strobe
//! each have exactly one writer today, and a second one would fight
//! it for the same handle every frame.
//!
//! # Accessibility
//!
//! Under `REDUCED FLASHING` there is **no screen flash at all** —
//! the promise this game already makes for its full-screen flashes is
//! none, not a dimmer one, and a new effect does not get to soften an
//! existing promise — and the ceiling **swells once** instead of
//! pulsing twice, which is what the light show's own strobe already
//! does in that mode. The neck's glow is untouched and carries the
//! feedback, so the moment is still unmistakable.
//!
//! `FX INTENSITY` scales the flash and the lift; at zero the player
//! asked for no effects and gets none. The Hype meter's own step in
//! the HUD is independent of all of this and always says what
//! happened.
//!
//! There are no graphics presets in this game to simplify against —
//! the equivalent is this handful of individual switches, and the
//! flat view is the simplified stage. On every setting the screen
//! flash and the HUD's own step remain, so the moment is never
//! silent.
//!
//! # Sound
//!
//! None, deliberately. Nothing in the game plays a sound when a
//! phrase is banked: `sfx` reacts to a miss, a stray strum and
//! `HypeActivated` — which is the player SPENDING the power, a
//! different moment with a sound of its own already. So there was
//! nothing to synchronise with and no new audio system was built
//! for this.
//!
//! A short charge sound on completion would fit well, and the place
//! for it is `sfx::react_to_feedback` beside the existing arm: one
//! more `SessionEvent` match arm and one more synthesized clip, no
//! new machinery. It would want the rate-limiting the miss sound
//! already has, because a dense chart can bank two phrases inside a
//! second. Left for its own decision rather than smuggled in with a
//! visual change.

use bevy::prelude::*;

use beatbyte_core::SessionEvent;

use super::SessionFeedback;
use crate::states::AppState;

/// How long the whole impulse lasts.
///
/// The longest of the three shapes, and the one the neck uses: past
/// this everything is back to normal by construction rather than by
/// a timer that has to be remembered.
pub const IMPULSE_S: f32 = 0.45;

/// How fast the neck takes the energy in.
const HIGHWAY_ATTACK_S: f32 = 0.04;

/// How long the screen flash lasts — well inside the neck's glow, so
/// the picture is readable again long before the stage has settled.
pub const SCREEN_S: f32 = 0.26;

/// The flash's rise. Shorter than a frame at 30 fps on purpose: a
/// camera flash has no rise you can see, and one you can see reads
/// as a fade-in.
const SCREEN_ATTACK_S: f32 = 0.03;

/// When each ceiling pulse starts.
const PULSES: [f32; 2] = [0.0, 0.09];

/// How long one ceiling pulse lasts.
const PULSE_S: f32 = 0.05;

/// The single swell that replaces the pulses under reduced flashing.
const CALM_S: f32 = 0.35;

/// …and how hard it is, against a full pulse.
const CALM_PEAK: f32 = 0.45;

/// The age an impulse that is not running carries.
///
/// A number rather than an `Option`: every shape below is zero past
/// [`IMPULSE_S`] anyway, so "not running" needs no second state to
/// get out of step with — and a timer that cannot hang is better
/// than one that is remembered to be cleared.
const IDLE: f32 = f32::MAX;

/// How hard the neck glows `age` seconds into the impulse, 0..1.
///
/// A hard rise and a square fall: energy taken in, not a lamp warming
/// up. The square is the house idiom for a spark — they die, they do
/// not switch — and it keeps the second half of the tail faint, which
/// is what stops the neck reading as overexposed while notes are
/// still coming down it. Pure — tested.
#[must_use]
pub fn highway_lift(age: f32) -> f32 {
    if !(0.0..IMPULSE_S).contains(&age) {
        return 0.0;
    }
    if age < HIGHWAY_ATTACK_S {
        // Smoothstep in, so the first frame is not a step.
        let t = age / HIGHWAY_ATTACK_S;
        return t * t * (3.0 - 2.0 * t);
    }
    let fall = (IMPULSE_S - age) / (IMPULSE_S - HIGHWAY_ATTACK_S);
    fall * fall
}

/// How hard the ceiling is hit `age` seconds in, 0..1.
///
/// Two xenon pulses with a dark gap between them — the gap is what
/// makes it a strobe rather than a glow — or, under reduced flashing,
/// one soft swell of under half the strength that never goes dark in
/// between. Pure — tested.
#[must_use]
pub fn lamp_burst(age: f32, reduced_flashing: bool) -> f32 {
    if age < 0.0 {
        return 0.0;
    }
    if reduced_flashing {
        if age >= CALM_S {
            return 0.0;
        }
        let across = age / CALM_S;
        return CALM_PEAK * (1.0 - across) * (1.0 - across);
    }
    PULSES
        .iter()
        .map(|start| pulse(age - start))
        .fold(0.0, f32::max)
}

/// One xenon pulse: instant rise, a plateau, a fast fall, nothing
/// after — the same shape the light show's own strobe uses, because
/// it is the same kind of lamp.
fn pulse(since: f32) -> f32 {
    if !(0.0..PULSE_S).contains(&since) {
        return 0.0;
    }
    let across = since / PULSE_S;
    (1.0 - across * across * across * across).max(0.0)
}

/// The screen flash's shape `age` seconds in, 0..1 — the peak itself
/// belongs to the flash profile. Pure — tested.
#[must_use]
pub fn screen_shape(age: f32) -> f32 {
    if !(0.0..SCREEN_S).contains(&age) {
        return 0.0;
    }
    if age < SCREEN_ATTACK_S {
        return age / SCREEN_ATTACK_S;
    }
    let fall = (SCREEN_S - age) / (SCREEN_S - SCREEN_ATTACK_S);
    fall * fall
}

/// The impulse's clock: how long ago each player landed a phrase, and
/// how long ago anybody did.
///
/// Per player AND stage-wide because the game is built that way: a
/// neck belongs to one player, the ceiling and the screen belong to
/// the room. In a duet, one player's phrase lights their own neck and
/// the whole venue — which is what actually happens in a venue.
#[derive(Resource, Debug)]
pub struct StarPower {
    /// Seconds since each player's last completed phrase.
    players: Vec<f32>,
    /// Seconds since the most recent completion by anyone.
    stage: f32,
}

impl Default for StarPower {
    fn default() -> Self {
        StarPower {
            players: Vec::new(),
            stage: IDLE,
        }
    }
}

impl StarPower {
    /// Start the impulse for one player — and for the room.
    pub fn fire(&mut self, player: usize) {
        if self.players.len() <= player {
            self.players.resize(player + 1, IDLE);
        }
        self.players[player] = 0.0;
        self.stage = 0.0;
    }

    /// Age everything by `dt`, saturating rather than growing without
    /// bound.
    pub fn advance(&mut self, dt: f32) {
        for age in &mut self.players {
            if *age < IMPULSE_S {
                *age = (*age + dt).min(IDLE);
            }
        }
        if self.stage < IMPULSE_S {
            self.stage = (self.stage + dt).min(IDLE);
        }
    }

    /// Back to nothing — every age idle, every shape zero.
    pub fn clear(&mut self) {
        self.players.clear();
        self.stage = IDLE;
    }

    /// How hard this player's neck glows right now, 0..1.
    #[must_use]
    pub fn neck(&self, player: usize) -> f32 {
        highway_lift(self.players.get(player).copied().unwrap_or(IDLE))
    }

    /// How hard the ceiling is hit right now, 0..1.
    #[must_use]
    pub fn ceiling(&self, reduced_flashing: bool) -> f32 {
        lamp_burst(self.stage, reduced_flashing)
    }

    /// Whether anything at all is running — what lets the systems
    /// that read this cost nothing while it is not.
    #[must_use]
    pub fn running(&self) -> bool {
        self.stage < IMPULSE_S || self.players.iter().any(|age| *age < IMPULSE_S)
    }
}

/// Arm the impulse when a phrase lands whole.
///
/// Reads the same feedback bus every other consumer reads, after the
/// drain that publishes it, and asks the session nothing.
pub fn arm(mut star: ResMut<StarPower>, mut feedback: MessageReader<SessionFeedback>) {
    for message in feedback.read() {
        if matches!(message.event, SessionEvent::PhraseCompleted { .. }) {
            star.fire(message.player_index);
        }
    }
}

/// Advance the clock.
///
/// In `Gameplay` rather than in `Playing`: pausing mid-impulse would
/// otherwise freeze a white screen over the pause menu until the song
/// resumed. Half a second finishing over the pause screen is what a
/// camera flash does anyway.
pub fn advance(mut star: ResMut<StarPower>, time: Res<Time>) {
    if star.running() {
        star.advance(time.delta_secs());
    }
}

/// Leave nothing behind when gameplay ends, however it ends — a song
/// finishing, a restart, quitting from the pause screen.
pub fn reset(mut star: ResMut<StarPower>) {
    star.clear();
}

/// What one impulse actually did, folded together while it ran.
///
/// The three components live in three systems, and their pixels are
/// the only place they meet — which on a machine whose screen has
/// locked is no place at all: every capture comes back black and a
/// whole session is spent without eyes (this repository has the scar
/// twice over). So the impulse reports itself. The peaks are the
/// values that were really applied, after the intensity setting and
/// the accessibility rules; the tail is what was left behind once it
/// was over, and a tail that is not zero is the stage not returning.
#[derive(Debug, Default)]
pub struct Peaks {
    running: bool,
    at: f64,
    neck: f32,
    ceiling: f32,
    screen: f32,
    frames: u32,
}

impl Peaks {
    /// Fold this frame in; yields a line exactly once, on the frame
    /// the impulse ends. Pure — tested.
    pub fn frame(
        &mut self,
        running: bool,
        now: f64,
        neck: f32,
        ceiling: f32,
        screen: f32,
    ) -> Option<String> {
        if running {
            if !self.running {
                *self = Peaks {
                    running: true,
                    at: now,
                    ..Peaks::default()
                };
            }
            self.neck = self.neck.max(neck);
            self.ceiling = self.ceiling.max(ceiling);
            self.screen = self.screen.max(screen);
            self.frames += 1;
            return None;
        }
        if !self.running {
            return None;
        }
        self.running = false;
        Some(format!(
            "star impulse: fired at song {:.2}s, peaks neck {:.2} ceiling {:.2} \
             screen {:.2} over {} frames, tail neck {neck:.3} ceiling {ceiling:.3} \
             screen {screen:.3}",
            self.at, self.neck, self.ceiling, self.screen, self.frames
        ))
    }
}

/// Say what the impulse did, once it is over.
///
/// Reads the values the three consumers read, so the line is evidence
/// about them and not about this module's intentions.
pub fn report(
    star: Res<StarPower>,
    flash: Res<super::fx::ScreenFlash>,
    settings: Res<crate::config::Settings>,
    game_clock: Res<crate::audio_sys::GameClock>,
    time: Res<Time>,
    players: Query<&super::PlayerIndex>,
    mut peaks: Local<Peaks>,
) {
    let now = game_clock.song_time(&time).unwrap_or(0.0);
    let intensity = settings.fx_intensity.clamp(0.0, 1.0);
    let neck = players
        .iter()
        .map(|index| star.neck(index.0) * intensity)
        .fold(0.0f32, f32::max);
    let ceiling = star.ceiling(settings.reduced_flashing) * intensity;
    if let Some(line) = peaks.frame(star.running(), now, neck, ceiling, flash.alpha()) {
        info!("{line}");
    }
}

/// Wire the impulse into the app.
pub fn register(app: &mut App) {
    app.init_resource::<StarPower>()
        .add_systems(
            Update,
            (arm.after(super::drain_feedback), advance, report)
                .chain()
                .run_if(in_state(AppState::Gameplay)),
        )
        .add_systems(OnExit(AppState::Gameplay), reset)
        .add_systems(OnEnter(AppState::Gameplay), reset);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_arrives_once_when_the_impulse_is_over() {
        let mut peaks = Peaks::default();
        assert_eq!(
            peaks.frame(false, 10.0, 0.0, 0.0, 0.0),
            None,
            "idle is silent"
        );
        assert_eq!(
            peaks.frame(true, 18.84, 0.3, 0.0, 0.1),
            None,
            "not while it runs"
        );
        assert_eq!(peaks.frame(true, 18.87, 1.0, 0.8, 0.28), None);
        assert_eq!(peaks.frame(true, 19.00, 0.4, 0.0, 0.10), None);
        let line = peaks
            .frame(false, 19.30, 0.0, 0.0, 0.0)
            .expect("one line, on the frame it ends");
        assert!(line.contains("song 18.84s"), "the moment it fired: {line}");
        assert!(
            line.contains("neck 1.00"),
            "the peak, not the last value: {line}"
        );
        assert!(line.contains("ceiling 0.80"), "{line}");
        assert!(line.contains("screen 0.28"), "{line}");
        assert!(line.contains("over 3 frames"), "{line}");
        assert_eq!(
            peaks.frame(false, 19.31, 0.0, 0.0, 0.0),
            None,
            "and never a second time for the same impulse"
        );
    }

    #[test]
    fn a_second_impulse_reports_its_own_peaks_not_the_first_ones() {
        let mut peaks = Peaks::default();
        peaks.frame(true, 1.0, 1.0, 1.0, 1.0);
        peaks.frame(false, 2.0, 0.0, 0.0, 0.0).expect("first line");
        peaks.frame(true, 30.0, 0.2, 0.1, 0.05);
        let line = peaks
            .frame(false, 31.0, 0.0, 0.0, 0.0)
            .expect("second line");
        assert!(
            line.contains("neck 0.20") && line.contains("song 30.00s"),
            "a stale peak would quietly claim the second impulse was as \
             bright as the first: {line}"
        );
    }

    #[test]
    fn the_report_carries_the_tail_so_a_stage_that_stays_lit_says_so() {
        let mut peaks = Peaks::default();
        peaks.frame(true, 5.0, 1.0, 1.0, 0.3);
        let line = peaks
            .frame(false, 5.5, 0.0, 0.25, 0.0)
            .expect("the closing line");
        assert!(
            line.contains("tail neck 0.000 ceiling 0.250"),
            "the values left behind are the whole of \"does it return\": {line}"
        );
    }

    #[test]
    fn the_neck_takes_the_energy_in_and_gives_it_back() {
        assert_eq!(highway_lift(-0.1), 0.0, "nothing before it starts");
        assert_eq!(highway_lift(0.0), 0.0, "and nothing at the very instant");
        let peak = highway_lift(HIGHWAY_ATTACK_S);
        assert!(peak > 0.99, "the rise reaches full: {peak}");
        assert!(
            highway_lift(0.02) > 0.3,
            "and it is most of the way there inside twenty milliseconds"
        );
        // Monotone down from the peak, and gone at the end.
        let mut last = peak;
        for step in 1..=40u16 {
            let age = HIGHWAY_ATTACK_S + (IMPULSE_S - HIGHWAY_ATTACK_S) * f32::from(step) / 40.0;
            let now = highway_lift(age);
            assert!(now <= last + 1e-6, "the tail rises again at {age}");
            last = now;
        }
        assert_eq!(
            highway_lift(IMPULSE_S),
            0.0,
            "and it is over when it is over"
        );
        assert_eq!(highway_lift(9.0), 0.0);
        // The square tail: the second half is faint, so notes stay
        // readable while the glow is still technically running.
        assert!(
            highway_lift(IMPULSE_S * 0.6) < 0.35,
            "the tail must not sit bright over the notes: {}",
            highway_lift(IMPULSE_S * 0.6)
        );
    }

    #[test]
    fn the_ceiling_flashes_twice_with_a_dark_gap_between() {
        // Two pulses is what makes it a strobe; without the dark gap
        // it is one long glow, which is a different thing entirely.
        assert!(
            lamp_burst(0.01, false) > 0.9,
            "the first pulse is immediate"
        );
        let gap = lamp_burst(0.07, false);
        assert_eq!(gap, 0.0, "and there is real darkness between them: {gap}");
        assert!(lamp_burst(0.10, false) > 0.9, "then the second");
        assert_eq!(lamp_burst(0.20, false), 0.0, "and then nothing");
        assert_eq!(lamp_burst(-1.0, false), 0.0);
    }

    #[test]
    fn reduced_flashing_swells_once_instead_of_strobing() {
        // The accessibility promise is not "dimmer strobing", it is
        // no strobing: one rise, one fall, never dark in between.
        let calm: Vec<f32> = (0..=30u16)
            .map(|step| lamp_burst(CALM_S * f32::from(step) / 30.0, true))
            .collect();
        assert!(calm[0] > 0.0);
        for pair in calm.windows(2) {
            assert!(
                pair[1] <= pair[0] + 1e-6,
                "the swell must not pulse: {calm:?}"
            );
        }
        assert_eq!(lamp_burst(CALM_S, true), 0.0);
        assert!(
            calm.iter().copied().fold(0.0, f32::max) < lamp_burst(0.01, false),
            "and it is gentler than a pulse"
        );
        // No dark frame anywhere inside it — that is the whole point.
        assert!(
            calm[..calm.len() - 1].iter().all(|value| *value > 0.0),
            "a gap in the swell is a strobe: {calm:?}"
        );
    }

    #[test]
    fn the_screen_flash_is_shorter_than_the_glow_behind_it() {
        const { assert!(SCREEN_S < IMPULSE_S) };
        assert_eq!(screen_shape(0.0), 0.0);
        assert!(
            screen_shape(SCREEN_ATTACK_S) > 0.99,
            "a camera flash rises at once"
        );
        assert!(screen_shape(0.15) < 0.5, "and is mostly gone by then");
        assert_eq!(screen_shape(SCREEN_S), 0.0);
        assert_eq!(screen_shape(5.0), 0.0);
    }

    #[test]
    fn a_phrase_lights_its_own_neck_and_the_whole_room() {
        let mut star = StarPower::default();
        assert!(!star.running(), "nothing runs before a phrase lands");
        assert_eq!(star.neck(0), 0.0);
        assert_eq!(star.ceiling(false), 0.0);

        star.fire(1);
        star.advance(HIGHWAY_ATTACK_S);
        assert!(star.neck(1) > 0.99, "the player's own neck");
        assert_eq!(
            star.neck(0),
            0.0,
            "and only theirs — the other player did not earn it"
        );
        assert!(star.ceiling(false) > 0.0, "but the room is everyone's");
        assert!(star.running());
    }

    #[test]
    fn the_impulse_ends_by_itself_and_can_never_hang() {
        let mut star = StarPower::default();
        star.fire(0);
        // Age it well past the end in small steps, the way frames do.
        for _ in 0..200 {
            star.advance(0.016);
        }
        assert_eq!(star.neck(0), 0.0);
        assert_eq!(star.ceiling(false), 0.0);
        assert_eq!(star.ceiling(true), 0.0);
        assert!(
            !star.running(),
            "an impulse nobody stopped must stop itself"
        );
        // …and a single enormous frame cannot leave it mid-flash.
        star.fire(0);
        star.advance(60.0);
        assert!(!star.running(), "a stall must not freeze the effect on");
    }

    #[test]
    fn a_second_phrase_fires_again_and_does_not_stack() {
        let mut star = StarPower::default();
        star.fire(0);
        star.advance(0.30);
        let tail = star.neck(0);
        assert!(tail > 0.0 && tail < 1.0, "mid-tail: {tail}");
        star.fire(0);
        assert_eq!(star.neck(0), 0.0, "the new impulse starts at its own zero");
        star.advance(HIGHWAY_ATTACK_S);
        assert!(
            star.neck(0) > 0.99,
            "and reaches the same peak, not a higher one — brightness \
             must never accumulate across phrases"
        );
    }

    #[test]
    fn clearing_leaves_nothing_running() {
        let mut star = StarPower::default();
        star.fire(0);
        star.fire(3);
        star.advance(0.01);
        assert!(star.running());
        star.clear();
        assert!(!star.running(), "gameplay ended: nothing may be left armed");
        assert_eq!(star.neck(0), 0.0);
        assert_eq!(star.neck(3), 0.0);
        assert_eq!(star.ceiling(false), 0.0);
    }

    #[test]
    fn the_three_shapes_are_one_impulse() {
        // They must peak together and end in the documented order:
        // screen first, then the ceiling, then the neck. Three
        // effects that merely happen nearby do not read as one.
        assert!(
            screen_shape(0.03) > 0.9 && highway_lift(0.04) > 0.9 && lamp_burst(0.01, false) > 0.9
        );
        let last_ceiling = PULSES[1] + PULSE_S;
        assert!(
            last_ceiling < SCREEN_S,
            "the ceiling is done before the screen: {last_ceiling}"
        );
        const { assert!(SCREEN_S < IMPULSE_S, "the screen clears before the neck") };
    }

    /// The whole path in a real (headless) app, and — the part that
    /// matters — driven by the REAL judgment engine rather than by a
    /// hand-written event. What arms this effect is a property of
    /// `TrackSession`, and a test that writes `PhraseCompleted`
    /// itself proves only that the plumbing works.
    mod wired {
        use super::super::*;
        use crate::gameplay::{PlayerIndex, SessionFeedback};
        use beatbyte_core::{
            Difficulty, Lane, LaneSet, NoteEvent, NoteKind, Phrase, ScoreConfig, TempoMap,
            TimingWindows, Track, TrackSession,
        };

        fn tap(time_s: f64, lane: Lane) -> NoteEvent {
            NoteEvent {
                time_s,
                lanes: LaneSet::single(lane),
                sustain_s: 0.0,
                kind: NoteKind::Strum,
            }
        }

        /// Two notes inside one phrase, and a third outside it.
        fn a_session() -> TrackSession {
            let track = Track::new(
                Difficulty::Medium,
                TempoMap::constant(120.0, 0.0),
                vec![
                    tap(1.0, Lane::One),
                    tap(1.5, Lane::Two),
                    tap(3.0, Lane::One),
                ],
                vec![Phrase {
                    start_s: 0.5,
                    end_s: 2.0,
                }],
            )
            .expect("a track with one phrase");
            let mut session =
                TrackSession::new(track, TimingWindows::default(), ScoreConfig::default());
            session.set_tap_mode(true);
            session
        }

        fn app() -> App {
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin)
                .init_resource::<Time>()
                // What `report` reads to say what the impulse did.
                .init_resource::<crate::config::Settings>()
                .init_resource::<crate::audio_sys::GameClock>()
                .init_resource::<crate::gameplay::fx::ScreenFlash>()
                .init_state::<AppState>()
                .add_message::<SessionFeedback>();
            register(&mut app);
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Gameplay);
            app.update();
            app
        }

        /// Play the session and publish whatever it produced, the way
        /// the drain does.
        fn play(app: &mut App, session: &mut TrackSession, time_s: f64, lane: Lane) {
            let mut events = Vec::new();
            session.handle(
                beatbyte_core::GameInput {
                    time_s,
                    kind: beatbyte_core::InputKind::FretDown(lane),
                },
                &mut events,
            );
            session.advance(time_s, &mut events);
            let player = app.world_mut().spawn(PlayerIndex(0)).id();
            for event in events {
                app.world_mut().write_message(SessionFeedback {
                    player,
                    player_index: 0,
                    event,
                });
            }
            app.update();
        }

        fn lit(app: &App) -> bool {
            app.world().resource::<StarPower>().running()
        }

        /// Move the world on by `ms`.
        ///
        /// A headless `Time` does not advance by itself, and the
        /// impulse rises from ZERO — at the instant it is armed its
        /// value is genuinely nothing, which is what a rise means.
        /// (The first version of these tests asserted brightness on
        /// the arming frame and failed; the expectation was wrong,
        /// not the shape.)
        fn tick(app: &mut App, ms: u64) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(ms));
            app.update();
        }

        #[test]
        fn only_a_whole_phrase_lights_the_stage() {
            let mut app = app();
            let mut session = a_session();

            // The first star note of the phrase: the meter has not
            // moved and nothing may fire.
            play(&mut app, &mut session, 1.0, Lane::One);
            assert!(
                !lit(&app),
                "a single star note is not a phrase — the effect must \
                 not fire on the way through one"
            );

            // The last one completes it.
            play(&mut app, &mut session, 1.5, Lane::Two);
            assert!(lit(&app), "the phrase landed whole");
            tick(&mut app, 40);
            assert!(
                app.world().resource::<StarPower>().neck(0) > 0.9,
                "and the neck is at full lift a frame or two later"
            );
        }

        #[test]
        fn a_phrase_with_a_miss_in_it_lights_nothing() {
            let mut app = app();
            let mut session = a_session();
            play(&mut app, &mut session, 1.0, Lane::One);
            // Let the second note's window pass unplayed.
            let mut events = Vec::new();
            session.advance(3.0, &mut events);
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, beatbyte_core::SessionEvent::PhraseBroken { .. })),
                "the phrase really was broken: {events:?}"
            );
            let player = app.world_mut().spawn(PlayerIndex(0)).id();
            for event in events {
                app.world_mut().write_message(SessionFeedback {
                    player,
                    player_index: 0,
                    event,
                });
            }
            app.update();
            assert!(
                !lit(&app),
                "a phrase that was broken credits no power and must \
                 light nothing"
            );
        }

        #[test]
        fn the_effect_fires_once_per_phrase_and_again_for_the_next() {
            let mut app = app();
            let mut session = a_session();
            play(&mut app, &mut session, 1.0, Lane::One);
            play(&mut app, &mut session, 1.5, Lane::Two);
            tick(&mut app, 40);
            assert!(app.world().resource::<StarPower>().neck(0) > 0.9);

            // Hitting the note AFTER the phrase must not re-fire it:
            // one phrase, one impulse.
            for _ in 0..40 {
                tick(&mut app, 16);
            }
            assert!(!lit(&app), "it ended on its own");
            play(&mut app, &mut session, 3.0, Lane::One);
            assert!(
                !lit(&app),
                "a note outside a phrase completes nothing and lights \
                 nothing"
            );

            // A second phrase does fire again.
            app.world_mut().resource_mut::<StarPower>().fire(0);
            assert!(lit(&app));
        }

        #[test]
        fn nothing_survives_the_end_of_a_song() {
            let mut app = app();
            app.world_mut().resource_mut::<StarPower>().fire(0);
            assert!(lit(&app));
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::SongSelect);
            app.update();
            assert!(
                !lit(&app),
                "leaving gameplay mid-impulse must leave nothing armed \
                 — a song end, a restart and a quit are all this path"
            );
            assert_eq!(app.world().resource::<StarPower>().neck(0), 0.0);
            assert_eq!(app.world().resource::<StarPower>().ceiling(false), 0.0);
        }

        #[test]
        fn a_pause_mid_impulse_does_not_freeze_the_flash_on_the_screen() {
            let mut app = app();
            app.add_sub_state::<crate::states::GamePhase>();
            app.update();
            app.world_mut().resource_mut::<StarPower>().fire(0);
            app.world_mut()
                .resource_mut::<NextState<crate::states::GamePhase>>()
                .set(crate::states::GamePhase::Paused);
            app.update();
            for _ in 0..40u16 {
                tick(&mut app, 16);
            }
            assert!(
                !lit(&app),
                "the impulse ages in Gameplay, not in Playing: pausing \
                 inside one would otherwise hold a white screen over the \
                 pause menu until the song resumed"
            );
        }

        #[test]
        fn entering_gameplay_starts_clean() {
            // The resource outlives the screen, so the reset on the way
            // OUT is only half the promise: a run that begins with
            // something armed would open on the last run's flash.
            let mut app = app();
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::SongSelect);
            app.update();
            app.world_mut().resource_mut::<StarPower>().fire(0);
            assert!(lit(&app), "armed while no song is running");
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Gameplay);
            app.update();
            assert!(
                !lit(&app),
                "entering gameplay must clear whatever was left armed"
            );
        }

        #[test]
        fn one_players_phrase_lights_their_own_neck_and_the_shared_room() {
            // Through the bus, because the player the impulse belongs
            // to is carried BY the message: a router that ignored it
            // would light the wrong neck and no direct call would tell.
            let mut app = app();
            let player = app.world_mut().spawn(PlayerIndex(1)).id();
            app.world_mut().write_message(SessionFeedback {
                player,
                player_index: 1,
                event: beatbyte_core::SessionEvent::PhraseCompleted { phrase_index: 0 },
            });
            app.update();
            tick(&mut app, 40);
            let star = app.world().resource::<StarPower>();
            assert!(star.neck(1) > 0.9, "the player who earned it");
            assert_eq!(
                star.neck(0),
                0.0,
                "and not the one who did not — a neck belongs to a player"
            );
            assert!(
                star.ceiling(false) > 0.0,
                "the ceiling is one ceiling: the room reacts to whoever \
                 earned it"
            );
        }

        #[test]
        fn judgment_is_untouched_by_any_of_this() {
            // The effect reads the bus and writes nothing back. The
            // session's own verdict on the same input must be exactly
            // what it was.
            let mut app = app();
            let mut session = a_session();
            play(&mut app, &mut session, 1.0, Lane::One);
            play(&mut app, &mut session, 1.5, Lane::Two);
            let counts = session.performance().counts();
            assert_eq!(counts.perfect, 2);
            assert_eq!(counts.miss, 0);
            assert!(
                (session.performance().hype_meter() - 0.25).abs() < 1e-9,
                "and the meter was credited exactly once"
            );
        }
    }
}
