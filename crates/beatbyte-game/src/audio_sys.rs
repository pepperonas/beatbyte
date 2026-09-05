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

/// Reconcile the song clock against the device-reported position.
fn sync_clock(time: Res<Time>, music: Res<Music>, mut game_clock: ResMut<GameClock>) {
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
        }
        // The browser's own music is not a timeline. Adopting the
        // generation here is what keeps the NEXT bump — the song the
        // player just started — readable as the new song it is.
        ClockMove::Absorb => {
            game_clock.generation = generation;
        }
        ClockMove::Reconcile => {
            // Returns the correction it applied; the caller has no
            // use for it.
            game_clock.clock.reconcile(mono, position);
        }
        ClockMove::Nothing => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
