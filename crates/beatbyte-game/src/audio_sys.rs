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
}

impl GameClock {
    /// The current song time given Bevy's monotonic time, if running.
    #[must_use]
    pub fn song_time(&self, time: &Time) -> Option<f64> {
        self.clock.song_time(time.elapsed_secs_f64())
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

/// Reconcile the song clock against the device-reported position.
fn sync_clock(time: Res<Time>, music: Res<Music>, mut game_clock: ResMut<GameClock>) {
    let mono = time.elapsed_secs_f64();
    let generation = music.0.generation();
    if generation != game_clock.generation {
        // A new song started: re-anchor at its reported position.
        game_clock.generation = generation;
        game_clock.clock.start(mono, music.0.position_s());
        return;
    }
    if game_clock.clock.is_playing() && music.0.is_active() {
        game_clock.clock.reconcile(mono, music.0.position_s());
    }
}
