//! The bridge between the audio stack and the ECS: the music handle,
//! the song clock, and the per-frame synchronization system.

use beatbyte_audio::SongClock;
use beatbyte_audio::playback::MusicHandle;
use bevy::prelude::*;

/// The handle to the music thread.
#[derive(Resource)]
pub struct Music(pub MusicHandle);

/// The authoritative song clock, reconciled every frame while playing.
#[derive(Resource, Default)]
pub struct GameClock {
    /// The pure clock state machine.
    pub clock: SongClock,
    /// The music generation the clock is currently tracking.
    pub generation: u64,
    /// Whether the game has ASKED for music that the clock should
    /// follow — set the moment gameplay, the editor or the
    /// calibration screen starts a track, cleared when the clock has
    /// anchored to it.
    ///
    /// ⚠️ This exists because `crossfade_*` bumps the music
    /// generation, and the clock reads a bump as "a new song
    /// started". The song browser's PREVIEW bumps it too — and its
    /// bump can arrive a frame AFTER the player already left the
    /// browser, so no amount of "which screen are we on" catches it.
    /// Measured before the fix: a 63-second song "finished at 185.6s"
    /// ten milliseconds after starting, 185.6 being exactly where the
    /// preview was playing inside another track.
    pub expect_song: bool,
    /// How long the loaded song is, when one is loaded. A position
    /// past it cannot belong to this song, so it is never anchored
    /// to — the second half of the preview fix, and the half that
    /// closes the race: `expect_song` is anonymous, so a preview's
    /// LATE generation bump can consume the expectation the game
    /// just set. A position of 185.6 s in a 63-second song cannot.
    pub song_len_s: Option<f64>,
    /// Monotonic time until which reconciliation is suppressed. A
    /// seek command travels to the music thread asynchronously; for
    /// a few frames the device still reports the OLD position, and
    /// reconciling against it would snap the freshly-seeked clock
    /// right back (a seek storm, in the loop's case).
    pub hold_reconcile_until: f64,
    /// Whether the running timeline has been anchored to the song
    /// the game asked for. The clock RECONCILES only while this is
    /// true: between a timeline's start and its anchor, whatever the
    /// device reports belongs to something else — the browser
    /// preview that is still winding down, the previous song of an
    /// MC set — and reconciling against it teleports the count-in.
    ///
    /// Measured before this flag: a song started from the browser
    /// read 185.6 s one frame into its count-in, the preview's
    /// position inside a 248-second track — well within the length
    /// bound, so `song_len_s` could not catch it. The autopilot then
    /// played 371 notes in that frame; a human would have missed them.
    pub anchored: bool,
    /// The device position the clock was last reconciled against: the
    /// clock is corrected only when the device reports a NEW one (see
    /// [`fresh_position`]).
    pub last_position: Option<f64>,
    /// Steps in a row that disagreed with the clock by a snap's worth,
    /// signed by direction (see [`weigh_step`]).
    pub outliers: i8,
}

impl GameClock {
    /// Begin a new timeline at `at` song seconds. The clock runs free
    /// from here and follows no device position until the song the
    /// game asks for has been anchored.
    pub fn begin(&mut self, mono: f64, at: f64) {
        self.clock.start(mono, at);
        self.anchored = false;
    }

    /// The current song time given Bevy's monotonic time, if running.
    #[must_use]
    pub fn song_time(&self, time: &Time) -> Option<f64> {
        self.clock.song_time(time.elapsed_secs_f64())
    }

    /// The song time VISUALS draw at: [`GameClock::song_time`] plus
    /// the player's video offset. Judgment, autopilot and the score
    /// never read this — the offset shifts where notes are DRAWN,
    /// never when they count.
    #[must_use]
    pub fn visual_time(&self, time: &Time, settings: &crate::config::Settings) -> Option<f64> {
        self.song_time(time)
            .map(|now| now + settings.video_offset_s())
    }
}

/// Plugin: owns the music thread handle and keeps the clock honest.
pub struct AudioBridgePlugin;

impl Plugin for AudioBridgePlugin {
    fn build(&self, app: &mut App) {
        let handle = beatbyte_audio::playback::spawn_music_thread();
        if !handle.is_healthy() {
            warn!("no audio output available — running silently");
        }
        app.insert_resource(Music(handle))
            .init_resource::<GameClock>()
            .add_systems(PreUpdate, sync_clock);
    }
}

/// What the clock should do about the music this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMove {
    /// A new song: re-anchor the clock at the device's position.
    Anchor,
    /// Someone else's playback started (a browser preview): take note
    /// of the generation so it is not mistaken for a song later, and
    /// leave the clock alone.
    Absorb,
    /// Same song, still playing: correct the drift.
    Reconcile,
    /// Nothing to do.
    Nothing,
}

/// The decision, pure.
///
/// ⚠️ `expected` is the whole point: only a generation the GAME asked
/// for may move the clock. A browser preview bumps the generation
/// exactly like a song does, and gating on the screen does not work —
/// the bump can arrive a frame after the player left the browser.
#[must_use]
#[allow(clippy::too_many_arguments)] // every input is a fact about the frame; a struct would only rename them
pub fn clock_move(
    expected: bool,
    plausible: bool,
    music_generation: u64,
    clock_generation: u64,
    clock_playing: bool,
    music_active: bool,
    reconcile_held: bool,
    anchored: bool,
) -> ClockMove {
    if music_generation != clock_generation {
        return if expected && plausible {
            ClockMove::Anchor
        } else {
            ClockMove::Absorb
        };
    }
    // Reconciliation SNAPS on a large drift, so a device position
    // from someone else's playback teleports the clock just as surely
    // as an anchor would. Two guards, both needed: the length bound
    // rejects a position that cannot be this song's, and `anchored`
    // rejects EVERY position until the song the game asked for has
    // arrived — a preview's position inside a long song passes the
    // first and was teleporting the count-in.
    if clock_playing && music_active && !reconcile_held && plausible && anchored {
        ClockMove::Reconcile
    } else {
        ClockMove::Nothing
    }
}

/// The song position a device report stands for, or `None` when the
/// report is the one already seen. Pure.
///
/// The device does not report a line, it reports a STAIRCASE: the
/// position moves when the output asks for its next buffer, and stands
/// still in between — 45 ms a step on the M1's built-in output
/// (measured: the same position on consecutive frames, then +45 ms).
/// Reconciling against every frame's reading compared a running clock
/// with a standing number: the drift swung through a whole step, and
/// whenever the output's timing leaned a little it crossed the 30 ms
/// snap threshold — the notes jerked back and, one step later, forward
/// again, with no long frame anywhere (233 snaps in 130 s of one run).
///
/// So only a step is a measurement. At the instant of a step the line
/// through the staircase's tops reads exactly the reported position;
/// what the clock has always settled on is the staircase's MEAN, half
/// a step under that line, and players' calibration is set against it
/// — hence half the step comes off. The frame that notices a step
/// notices it half a frame late on average, which goes back on.
#[must_use]
pub fn fresh_position(last: Option<f64>, position: f64, frame_s: f64) -> Option<f64> {
    let last = last?;
    // Exact: the position is the same atomic read twice.
    #[allow(clippy::float_cmp)]
    if position == last {
        return None;
    }
    // A seek or a new song is no step of the staircase.
    let step = (position - last).clamp(0.0, 0.1);
    Some(position - step / 2.0 + frame_s.clamp(0.0, 0.05) / 2.0)
}

/// What one step of the device position does to the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepVerdict {
    /// Within a snap of the clock, or far enough off to be believed:
    /// hand it to the clock.
    Reconcile,
    /// A snap's worth off and not yet confirmed: leave the clock alone.
    Wait,
}

/// How many steps in a row must disagree the same way before the clock
/// snaps to them.
pub const CONFIRM_STEPS: i8 = 3;

/// A drift this large is no late buffer: a seek, a restart, a stall
/// the clock must follow at once.
pub const BELIEVED_AT_ONCE_S: f64 = 0.25;

/// Decide what a step that is `drift` seconds off the clock is worth,
/// keeping the count of disagreeing steps in `outliers`. Pure.
///
/// One step is one buffer request, and a busy machine serves requests
/// late and then in a hurry: the position reads 40 ms behind on one
/// step and 40 ms ahead on the next while the sound never faltered.
/// Snapping to each of them jerked the highway back and forth (58
/// snaps in 150 s of a loaded run, nearly all in pairs 45 ms apart).
/// So a step that is a snap's worth off is believed only when the
/// steps after it say the same — or when it is too far off to be a
/// late buffer at all.
#[must_use]
pub fn weigh_step(outliers: &mut i8, drift: f64) -> StepVerdict {
    if drift.abs() < beatbyte_audio::clock::SNAP_THRESHOLD_S {
        *outliers = 0;
        return StepVerdict::Reconcile;
    }
    if drift.abs() >= BELIEVED_AT_ONCE_S {
        *outliers = 0;
        return StepVerdict::Reconcile;
    }
    let sign: i8 = if drift > 0.0 { 1 } else { -1 };
    *outliers = if outliers.signum() == sign {
        *outliers + sign
    } else {
        sign
    };
    if outliers.abs() >= CONFIRM_STEPS {
        *outliers = 0;
        StepVerdict::Reconcile
    } else {
        StepVerdict::Wait
    }
}

/// Reconcile the song clock against the device-reported position.
fn sync_clock(
    time: Res<Time>,
    real: Res<Time<Real>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
) {
    let mono = time.elapsed_secs_f64();
    let generation = music.0.generation();
    let position = music.0.position_s();
    // A position past the end of the loaded song is not this song's.
    let plausible = game_clock
        .song_len_s
        .is_none_or(|len| position <= len + 5.0);
    match clock_move(
        game_clock.expect_song,
        plausible,
        generation,
        game_clock.generation,
        game_clock.clock.is_playing(),
        music.0.is_active(),
        mono < game_clock.hold_reconcile_until,
        game_clock.anchored,
    ) {
        ClockMove::Anchor => {
            game_clock.generation = generation;
            game_clock.expect_song = false;
            game_clock.anchored = true;
            game_clock.clock.start(mono, position);
            game_clock.last_position = Some(position);
            game_clock.outliers = 0;
        }
        // The browser's own music is not a timeline. Adopting the
        // generation here is what keeps the NEXT bump — the song the
        // player just started — readable as the new song it is.
        ClockMove::Absorb => {
            game_clock.generation = generation;
        }
        ClockMove::Reconcile => {
            let target = fresh_position(game_clock.last_position, position, time.delta_secs_f64());
            game_clock.last_position = Some(position);
            let Some(target) = target else {
                return;
            };
            let drift = game_clock
                .clock
                .song_time(mono)
                .map_or(0.0, |ours| target - ours);
            if weigh_step(&mut game_clock.outliers, drift) == StepVerdict::Wait {
                return;
            }
            let correction = game_clock.clock.reconcile(mono, target);
            // A correction this large is a visible jump of the
            // highway. Say so, with what the frame looked like: the
            // one line that tells a stalled frame (virtual time
            // clamped) from a stalled audio thread (the device
            // position lagging, then catching up).
            let snapped = correction.abs() >= beatbyte_audio::clock::SNAP_THRESHOLD_S;
            if snapped {
                crate::frame_marks::note(crate::frame_marks::CLOCK);
            }
            if correction.abs() > 0.1 {
                warn!(
                    "clock: snapped {correction:+.3} s at song {position:.3} (frame: wall {:.3} s, virtual {:.3} s)",
                    real.delta_secs_f64(),
                    time.delta_secs_f64()
                );
            } else if snapped {
                // Under the warning's size a snap is still a jerk of the
                // highway the eye sees, and it is no long frame: without
                // this line it leaves no trace at all.
                info!(
                    "clock: snapped {:+.1} ms at song {position:.3} (frame: wall {:.1} ms)",
                    correction * 1000.0,
                    real.delta_secs_f64() * 1000.0
                );
            }
        }
        ClockMove::Nothing => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A clock driven the way `sync_clock` drives it, against an output
    /// that reports its position in buffer-sized steps: `frames` frames
    /// of `frame_s`, a step of `step_s` every `step_s`, each step
    /// arriving `lean(k)` seconds off its time. Returns the snaps and
    /// the clock's mean lead over the staircase's mean line.
    fn run_staircase(
        every_frame: bool,
        frames: usize,
        frame_s: f64,
        step_s: f64,
        lean: impl Fn(usize) -> f64,
    ) -> (usize, f64) {
        let mut clock = beatbyte_audio::SongClock::default();
        clock.start(0.0, 0.0);
        let mut last = Some(0.0);
        let mut outliers = 0;
        let (mut snaps, mut lead, mut counted) = (0, 0.0, 0);
        for frame in 1..=frames {
            let mono = frame as f64 * frame_s;
            // The newest step whose (leaning) time has come.
            let mut k = (mono / step_s) as usize;
            if k > 0 && k as f64 * step_s + lean(k) > mono {
                k -= 1;
            }
            let position = k as f64 * step_s;
            let target = if every_frame {
                Some(position)
            } else {
                fresh_position(last, position, frame_s)
            };
            last = Some(position);
            if let Some(target) = target {
                let drift = target - clock.song_time(mono).unwrap();
                if !every_frame && weigh_step(&mut outliers, drift) == StepVerdict::Wait {
                    continue;
                }
                let correction = clock.reconcile(mono, target);
                if correction.abs() >= beatbyte_audio::clock::SNAP_THRESHOLD_S {
                    snaps += 1;
                }
            }
            if frame > frames / 2 {
                // The staircase's mean line: half a step under its tops.
                lead += clock.song_time(mono).unwrap() - (mono - step_s / 2.0);
                counted += 1;
            }
        }
        (snaps, lead / f64::from(counted))
    }

    /// The output's timing leaning by a few milliseconds, the way a
    /// real one does: a slow wobble plus a late buffer now and then.
    fn leaning(k: usize) -> f64 {
        let wobble = 0.006 * (k as f64 * 0.37).sin();
        if k % 53 == 17 { wobble + 0.012 } else { wobble }
    }

    /// The fault, and its fix, on the M1's 45 ms staircase at 120 Hz:
    /// read every frame the clock snaps again and again; read on steps
    /// it never does, and settles where it always has.
    #[test]
    fn a_stepped_device_position_no_longer_snaps_the_clock() {
        let frames = 120 * 120;
        let (old_snaps, old_lead) = run_staircase(true, frames, 1.0 / 120.0, 0.045, leaning);
        let (snaps, lead) = run_staircase(false, frames, 1.0 / 120.0, 0.045, leaning);
        assert!(old_snaps > 10, "the fault is in the fixture: {old_snaps}");
        assert_eq!(snaps, 0);
        assert!(
            (lead - old_lead).abs() < 0.006,
            "calibration moved: {lead} against {old_lead}"
        );
        assert!(lead.abs() < 0.006, "{lead}");
        // At 60 Hz, and on an output with small buffers, the same.
        for (frame_s, step_s) in [
            (1.0 / 60.0, 0.045),
            (1.0 / 120.0, 0.010),
            (1.0 / 60.0, 0.010),
        ] {
            let (snaps, lead) = run_staircase(false, 7200, frame_s, step_s, leaning);
            assert_eq!(snaps, 0, "{frame_s} {step_s}");
            assert!(lead.abs() < 0.008, "{frame_s} {step_s}: {lead}");
        }
    }

    /// A real stall is still a snap: the output stops for 200 ms, and
    /// the clock comes back to it in one move.
    #[test]
    fn a_stalled_output_still_snaps_once() {
        let (snaps, _) = run_staircase(false, 1200, 1.0 / 120.0, 0.045, |k| {
            if (100..104).contains(&k) { 0.2 } else { 0.0 }
        });
        assert!((1..=5).contains(&snaps), "{snaps}");
    }

    /// A loaded machine: every seventh buffer is asked for 40 ms late
    /// and the next one on time again. The sound never stopped, and
    /// the clock does not move for it.
    #[test]
    fn a_single_late_buffer_does_not_snap_the_clock() {
        let late = |k: usize| if k % 7 == 3 { 0.040 } else { 0.0 };
        let (snaps, lead) = run_staircase(false, 120 * 60, 1.0 / 120.0, 0.045, late);
        assert_eq!(snaps, 0);
        assert!(lead.abs() < 0.006, "{lead}");
    }

    #[test]
    fn a_step_far_off_the_clock_must_be_confirmed_or_be_huge() {
        let mut n = 0;
        // Small drift: always the clock's to slew.
        assert_eq!(weigh_step(&mut n, 0.012), StepVerdict::Reconcile);
        // One late buffer, then one early: neither is believed.
        assert_eq!(weigh_step(&mut n, -0.046), StepVerdict::Wait);
        assert_eq!(weigh_step(&mut n, 0.050), StepVerdict::Wait);
        assert_eq!(n, 1, "the other direction starts its own count");
        // Three the same way are.
        assert_eq!(weigh_step(&mut n, 0.050), StepVerdict::Wait);
        assert_eq!(weigh_step(&mut n, 0.050), StepVerdict::Reconcile);
        assert_eq!(n, 0);
        // A good step in between forgets the count.
        assert_eq!(weigh_step(&mut n, -0.05), StepVerdict::Wait);
        assert_eq!(weigh_step(&mut n, -0.05), StepVerdict::Wait);
        assert_eq!(weigh_step(&mut n, 0.0), StepVerdict::Reconcile);
        assert_eq!(weigh_step(&mut n, -0.05), StepVerdict::Wait);
        // A seek's worth is believed at once.
        assert_eq!(weigh_step(&mut n, 0.25), StepVerdict::Reconcile);
        assert_eq!(weigh_step(&mut n, -90.0), StepVerdict::Reconcile);
    }

    #[test]
    fn only_a_step_of_the_device_position_is_a_measurement() {
        assert_eq!(
            fresh_position(None, 1.0, 0.008),
            None,
            "nothing to step from"
        );
        assert_eq!(
            fresh_position(Some(1.0), 1.0, 0.008),
            None,
            "the same report"
        );
        let target = fresh_position(Some(1.0), 1.045, 0.008).unwrap();
        assert!((target - (1.045 - 0.0225 + 0.004)).abs() < 1e-9);
        // A seek: no half step of 90 seconds comes off.
        let seek = fresh_position(Some(1.0), 91.0, 0.008).unwrap();
        assert!((seek - 91.0).abs() < 0.06);
        let back = fresh_position(Some(91.0), 1.0, 0.008).unwrap();
        assert!((back - 1.0).abs() < 0.01);
    }

    #[test]
    fn only_a_generation_the_game_asked_for_moves_the_clock() {
        // Gameplay, the editor, calibration: the game started this
        // track, so the bump IS the new song.
        assert_eq!(
            clock_move(true, true, 7, 6, false, true, false, true),
            ClockMove::Anchor
        );
        // Nobody asked: a browser preview. Take note of the
        // generation so the NEXT bump still reads as new, and leave
        // the clock where it is.
        assert_eq!(
            clock_move(false, true, 7, 6, false, true, false, true),
            ClockMove::Absorb
        );
        // And the race the flag alone cannot win: the preview's bump
        // arrives AFTER the game set its expectation, carrying a
        // position from inside another track. The length says no.
        assert_eq!(
            clock_move(true, false, 7, 6, false, true, false, true),
            ClockMove::Absorb,
            "185 seconds into a 63-second song is not this song"
        );
    }

    #[test]
    fn the_same_generation_reconciles_only_while_it_may() {
        assert_eq!(
            clock_move(true, true, 6, 6, true, true, false, true),
            ClockMove::Reconcile
        );
        assert_eq!(
            clock_move(true, true, 6, 6, true, true, true, true),
            ClockMove::Nothing,
            "held after a seek"
        );
        assert_eq!(
            clock_move(true, true, 6, 6, false, true, false, true),
            ClockMove::Nothing,
            "a stopped clock has no drift"
        );
        assert_eq!(
            clock_move(true, true, 6, 6, true, false, false, true),
            ClockMove::Nothing,
            "silent music reports nothing worth following"
        );
        // THE defect: same generation, and the device is three
        // minutes inside a track this song is not. Reconciling would
        // snap the clock there and end the song at once.
        assert_eq!(
            clock_move(true, false, 6, 6, true, true, false, true),
            ClockMove::Nothing,
            "a position that cannot be this song's is not followed"
        );
    }

    #[test]
    fn a_fresh_timeline_follows_nothing_until_its_song_is_anchored() {
        // The count-in: the clock is running (from −2 s), the browser
        // preview is still winding down on the device (active, and
        // reporting 185.6 s — inside this 248-second song, so the
        // length bound says yes), and no song has been anchored yet.
        // This is the frame that teleported the clock.
        assert_eq!(
            clock_move(false, true, 6, 6, true, true, false, false),
            ClockMove::Nothing,
            "the count-in follows no device position"
        );
        // Once THIS song has been anchored, the same frame reconciles.
        assert_eq!(
            clock_move(false, true, 6, 6, true, true, false, true),
            ClockMove::Reconcile
        );
        // And anchoring itself does not need the flag — it is what
        // sets it.
        assert_eq!(
            clock_move(true, true, 7, 6, true, true, false, false),
            ClockMove::Anchor
        );
    }

    #[test]
    fn beginning_a_timeline_forgets_the_last_anchor() {
        let mut clock = GameClock {
            anchored: true,
            ..GameClock::default()
        };
        clock.begin(10.0, -2.0);
        assert!(!clock.anchored, "a new timeline starts unanchored");
        assert!(clock.clock.is_playing(), "and running");
    }
}
