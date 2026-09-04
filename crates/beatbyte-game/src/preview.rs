//! Song previews in the browser: rest the cursor on a song and its
//! hook plays (optimization plan P4, the half that was never built).
//!
//! The chart has carried `preview_start_s` since the generator was
//! written — the loudest ten seconds of the song, its hook — and
//! nothing ever played it. A seventy-song library is a list of names
//! without it.
//!
//! Three rules, all of them about not being annoying:
//!
//! - **The cursor must REST.** Arrowing through twenty songs must not
//!   fire twenty previews, so a song has to be selected for
//!   [`REST_S`] before it sounds.
//! - **Moving on stops it.** Silence is the honest answer to "which
//!   song is selected?" while the answer is still changing.
//! - **It fades.** A hard cut into the middle of a track is a noise,
//!   not a preview; the music thread already knows how to crossfade.
//!
//! The decision is [`step`] — a pure function of the cursor, the
//! clock and the setting — so the whole policy is tested without a
//! sound card.

use bevy::prelude::*;

use crate::audio_sys::Music;
use crate::boot::{BuiltinSongs, SongAudio};
use crate::config::Settings;
use crate::library::{SongEntry, SongLibrary, SongSource};

/// How long the cursor must rest on a song before it sounds.
pub const REST_S: f32 = 0.55;

/// The crossfade in and out, in seconds.
pub const FADE_S: f32 = 0.35;

/// Where a preview starts when the chart carries no hook: a quarter
/// of the way in, which beats the silence most songs open with.
#[must_use]
pub fn fallback_start_s(duration_s: Option<f64>) -> f64 {
    duration_s.map_or(0.0, |d| (d * 0.25).max(0.0))
}

/// Where this entry's preview starts.
#[must_use]
pub fn start_of(entry: &SongEntry) -> f64 {
    entry
        .preview_start_s
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or_else(|| fallback_start_s(entry.duration_s))
}

/// What the browser is doing about previews right now.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct SongPreview {
    /// The library index the cursor sits on, and how long it has.
    on: Option<usize>,
    rest_s: f32,
    /// The library index currently sounding, if any.
    playing: Option<usize>,
}

/// What [`step`] asks the music thread to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// Leave the music alone.
    Nothing,
    /// Fade this library index in.
    Start(usize),
    /// Fade whatever is playing out.
    Stop,
}

impl SongPreview {
    /// The library index currently sounding.
    #[must_use]
    pub fn playing(&self) -> Option<usize> {
        self.playing
    }

    /// How long the cursor has rested on the current song.
    #[must_use]
    pub fn rest_s(&self) -> f32 {
        self.rest_s
    }

    /// Forget everything (leaving the browser, or the setting going
    /// off). Does not itself stop the music — the caller does that.
    pub fn clear(&mut self) {
        *self = SongPreview::default();
    }
}

/// One frame of the policy. `cursor` is the library index under the
/// cursor, `None` when the list is empty.
///
/// Pure — the resource is the only thing it touches.
pub fn step(state: &mut SongPreview, cursor: Option<usize>, dt: f32, enabled: bool) -> Move {
    if !enabled {
        let was_playing = state.playing.is_some();
        state.clear();
        return if was_playing {
            Move::Stop
        } else {
            Move::Nothing
        };
    }
    if state.on != cursor {
        // The selection moved: restart the clock, and hush whatever
        // the old selection was saying.
        state.on = cursor;
        state.rest_s = 0.0;
        if state.playing.is_some() {
            state.playing = None;
            return Move::Stop;
        }
        return Move::Nothing;
    }
    let Some(song) = cursor else {
        return Move::Nothing;
    };
    if state.playing == Some(song) {
        return Move::Nothing;
    }
    state.rest_s += dt;
    if state.rest_s >= REST_S {
        state.playing = Some(song);
        return Move::Start(song);
    }
    Move::Nothing
}

/// Drive the preview from the browser's cursor.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn drive_preview(
    time: Res<Time>,
    settings: Res<Settings>,
    library: Res<SongLibrary>,
    builtins: Res<BuiltinSongs>,
    view: Res<crate::song_select::BrowserView>,
    cursor: Res<crate::song_select::BrowserCursor>,
    music: Res<Music>,
    mut state: ResMut<SongPreview>,
) {
    let song = view.order.get(cursor.0).copied();
    match step(
        &mut state,
        song,
        time.delta_secs(),
        settings.song_preview && settings.music_volume > 0.0,
    ) {
        Move::Nothing => {}
        Move::Stop => music.0.stop(),
        Move::Start(index) => {
            let Some(entry) = library.entries.get(index) else {
                return;
            };
            let at = start_of(entry);
            match &entry.source {
                SongSource::File { audio_path, .. } => {
                    music.0.crossfade_file(audio_path.clone(), FADE_S);
                }
                SongSource::Builtin(builtin) => {
                    // The built-ins live decoded in memory; starting
                    // one for real clones the same buffer.
                    match builtins.0.get(*builtin).map(|song| &song.audio) {
                        Some(SongAudio::Memory(audio)) => {
                            music.0.crossfade_buffer(audio.clone(), FADE_S);
                        }
                        Some(SongAudio::File(path)) => {
                            music.0.crossfade_file(path.clone(), FADE_S);
                        }
                        None => return,
                    }
                }
            }
            music.0.set_volume(settings.music_volume);
            music.0.seek_s(at);
            info!(
                "preview: \"{}\" from {at:.1}s",
                crate::ui::font_safe(&entry.title)
            );
        }
    }
}

/// Silence on the way out: the browser's music must not follow the
/// player into the menu, and gameplay starts its own.
pub fn stop_preview(music: Res<Music>, mut state: ResMut<SongPreview>) {
    if state.playing.is_some() {
        music.0.stop();
    }
    state.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(preview: Option<f64>, duration: Option<f64>) -> SongEntry {
        SongEntry {
            title: "T".to_owned(),
            artist: "A".to_owned(),
            bpm: 120.0,
            duration_s: duration,
            difficulties: vec![],
            note_counts: vec![],
            genre: None,
            preview_start_s: preview,
            source: SongSource::Builtin(0),
            has_lyrics: false,
        }
    }

    #[test]
    fn a_resting_cursor_starts_the_preview_and_a_moving_one_never_does() {
        let mut state = SongPreview::default();
        // Arriving on a song starts the clock, nothing else — the
        // frame the selection changed is not a frame of rest.
        assert_eq!(step(&mut state, Some(3), 0.016, true), Move::Nothing);
        // Not yet.
        assert_eq!(step(&mut state, Some(3), 0.5, true), Move::Nothing);
        // Now.
        assert_eq!(step(&mut state, Some(3), 0.1, true), Move::Start(3));
        assert_eq!(state.playing(), Some(3));
        // And it does not start again while it plays.
        assert_eq!(step(&mut state, Some(3), 1.0, true), Move::Nothing);

        // Moving hushes it at once, and the next song waits its turn.
        assert_eq!(step(&mut state, Some(4), 0.016, true), Move::Stop);
        assert_eq!(state.playing(), None);
        assert_eq!(step(&mut state, Some(4), 0.2, true), Move::Nothing);
    }

    #[test]
    fn scrolling_fast_never_fires_a_preview() {
        // Twenty songs at a fifth of the rest time each: the cursor
        // never settles, so nothing ever sounds.
        let mut state = SongPreview::default();
        for song in 0..20 {
            let step = step(&mut state, Some(song), REST_S / 5.0, true);
            assert!(matches!(step, Move::Nothing), "song {song} sounded");
        }
        assert_eq!(state.playing(), None);
    }

    /// Arrive on a song and rest there until it sounds. The arrival
    /// frame only starts the clock — it is the frame the selection
    /// CHANGED, and a change is never a rest.
    fn settle(state: &mut SongPreview, song: usize) -> Move {
        step(state, Some(song), 0.016, true);
        step(state, Some(song), REST_S, true)
    }

    #[test]
    fn turning_it_off_stops_what_is_playing_exactly_once() {
        let mut state = SongPreview::default();
        assert_eq!(settle(&mut state, 1), Move::Start(1));
        assert_eq!(state.playing(), Some(1));
        assert_eq!(step(&mut state, Some(1), 0.016, false), Move::Stop);
        assert_eq!(
            step(&mut state, Some(1), 0.016, false),
            Move::Nothing,
            "nothing left to stop"
        );
        // And with it off, resting starts nothing.
        assert_eq!(step(&mut state, Some(1), 10.0, false), Move::Nothing);
    }

    #[test]
    fn an_empty_list_is_quiet() {
        let mut state = SongPreview::default();
        assert_eq!(step(&mut state, None, 10.0, true), Move::Nothing);
        assert_eq!(state.playing(), None);
        // And a list that empties under a playing preview hushes it.
        assert_eq!(settle(&mut state, 0), Move::Start(0));
        assert_eq!(step(&mut state, None, 0.016, true), Move::Stop);
    }

    #[test]
    fn the_hook_is_used_when_there_is_one_and_a_quarter_in_when_there_is_not() {
        assert!((start_of(&entry(Some(93.5), Some(200.0))) - 93.5).abs() < 1e-9);
        assert!((start_of(&entry(None, Some(200.0))) - 50.0).abs() < 1e-9);
        assert!((start_of(&entry(None, None)) - 0.0).abs() < 1e-9);
        // A chart with a nonsense hook falls back rather than seeking
        // to a place that cannot exist.
        assert!((start_of(&entry(Some(-4.0), Some(200.0))) - 50.0).abs() < 1e-9);
        assert!((start_of(&entry(Some(f64::NAN), Some(200.0))) - 50.0).abs() < 1e-9);
    }

    /// The system in a real world: the browser's own resources, the
    /// real music handle (which degrades silently without a device),
    /// and the state it leaves behind.
    ///
    /// This is the half a screenshot would have shown — and could
    /// not, the day it was written: the screen was locked, so every
    /// injected key went to the lock screen and every capture came
    /// back empty.
    ///
    /// ⚠️ What these tests CANNOT see: the calls into the music
    /// thread. `MusicHandle` posts commands down a channel and
    /// reports nothing back that a device-less test could read, so
    /// deleting `music.0.stop()` from [`stop_preview`] leaves them
    /// green (probed — it does). The state machine is pinned here;
    /// that the browser actually falls silent on the way out is an
    /// ear's job.
    mod wired {
        use super::*;
        use crate::library::SongLibrary;
        use crate::song_select::{BrowserCursor, BrowserView};

        fn app() -> App {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<SongPreview>()
                .init_resource::<BrowserCursor>()
                .init_resource::<BrowserView>()
                .insert_resource(BuiltinSongs(vec![]))
                .insert_resource(Music(beatbyte_audio::playback::spawn_music_thread()))
                .insert_resource(Settings {
                    song_preview: true,
                    music_volume: 0.8,
                    ..Settings::default()
                })
                .add_systems(Update, drive_preview);
            let entries = (0..3)
                .map(|i| SongEntry {
                    title: format!("Song {i}"),
                    artist: "A".to_owned(),
                    bpm: 120.0,
                    duration_s: Some(200.0),
                    difficulties: vec![],
                    note_counts: vec![],
                    genre: None,
                    preview_start_s: Some(40.0 + i as f64),
                    // A path that does not exist: the command reaches
                    // the music thread either way, and the point here
                    // is the wiring, not the sound.
                    source: SongSource::File {
                        chart_path: std::path::PathBuf::from("none.json"),
                        audio_path: std::path::PathBuf::from("none.m4a"),
                    },
                    has_lyrics: false,
                })
                .collect();
            app.insert_resource(SongLibrary { entries });
            app.world_mut().resource_mut::<BrowserView>().order = vec![0, 1, 2];
            app
        }

        fn frame(app: &mut App, dt: f32) {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(dt));
            app.update();
        }

        fn playing(app: &App) -> Option<usize> {
            app.world().resource::<SongPreview>().playing()
        }

        #[test]
        fn the_browser_cursor_drives_the_preview_through_the_real_system() {
            let mut app = app();
            frame(&mut app, 0.016);
            assert_eq!(
                playing(&app),
                None,
                "the arrival frame only starts the clock"
            );
            frame(&mut app, REST_S);
            assert_eq!(playing(&app), Some(0), "resting sounds the song");

            // Moving the cursor hushes it, and the next song waits.
            app.world_mut().resource_mut::<BrowserCursor>().0 = 2;
            frame(&mut app, 0.016);
            assert_eq!(playing(&app), None);
            frame(&mut app, REST_S);
            assert_eq!(
                playing(&app),
                Some(2),
                "the cursor's own song, not the first"
            );

            // Leaving takes the sound with it — through the real
            // exit system, run the way the state transition runs it.
            app.world_mut()
                .run_system_cached(stop_preview)
                .expect("the exit system runs");
            assert_eq!(playing(&app), None);
        }

        #[test]
        fn a_silent_setting_keeps_the_browser_quiet() {
            let mut app = app();
            app.world_mut().resource_mut::<Settings>().song_preview = false;
            for _ in 0..5 {
                frame(&mut app, REST_S);
            }
            assert_eq!(playing(&app), None);
            // And so does a music volume of zero: the preview is
            // music, and silent music is no music.
            app.world_mut().resource_mut::<Settings>().song_preview = true;
            app.world_mut().resource_mut::<Settings>().music_volume = 0.0;
            for _ in 0..5 {
                frame(&mut app, REST_S);
            }
            assert_eq!(playing(&app), None);
        }
    }
}
