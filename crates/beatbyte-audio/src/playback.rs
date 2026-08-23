//! Music playback: a thin, honest wrapper around rodio.
//!
//! One song at a time, streamed from disk (no full decode), exposing
//! exactly what the song clock needs: play/pause/seek/position. The
//! wrapper is deliberately free of gameplay logic — clock smoothing
//! lives in [`crate::clock`], judgment in `beatbyte-core`.
//!
//! This module talks to real audio hardware and is therefore *not*
//! unit-tested (CI runners have no output device); it is exercised by
//! the game and the smoke test instead.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use rodio::{Decoder, MixerDeviceSink, Player};
use thiserror::Error;

/// Errors from the playback layer.
#[derive(Debug, Error)]
pub enum PlaybackError {
    /// No audio output device could be opened.
    #[error("cannot open audio output: {0}")]
    Output(String),
    /// The song file could not be opened.
    #[error("cannot open `{path}`: {source}")]
    Open {
        /// The file involved.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The song file could not be decoded.
    #[error("cannot decode `{path}`: {source}")]
    Decode {
        /// The file involved.
        path: String,
        /// The underlying error.
        #[source]
        source: rodio::decoder::DecoderError,
    },
    /// Seeking failed (unsupported by the source or out of range).
    #[error("seek failed: {0}")]
    Seek(String),
}

/// Owns the audio output and plays one music track at a time.
pub struct MusicPlayer {
    /// Keeps the output device alive; dropping it stops all audio.
    _device: MixerDeviceSink,
    player: Player,
}

impl MusicPlayer {
    /// Open the default audio output.
    pub fn new() -> Result<MusicPlayer, PlaybackError> {
        let device = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| PlaybackError::Output(e.to_string()))?;
        let player = Player::connect_new(device.mixer());
        Ok(MusicPlayer {
            _device: device,
            player,
        })
    }

    /// Load and start playing a song, replacing anything current.
    /// Returns the total duration when the decoder knows it.
    pub fn play_file(&mut self, path: &Path) -> Result<Option<Duration>, PlaybackError> {
        let display = path.display().to_string();
        let file = File::open(path).map_err(|source| PlaybackError::Open {
            path: display.clone(),
            source,
        })?;
        let decoder = Decoder::try_from(file).map_err(|source| PlaybackError::Decode {
            path: display,
            source,
        })?;
        let duration = rodio::Source::total_duration(&decoder);

        self.player.stop();
        self.player.append(decoder);
        self.player.play();
        Ok(duration)
    }

    /// Pause playback.
    pub fn pause(&self) {
        self.player.pause();
    }

    /// Resume playback.
    pub fn resume(&self) {
        self.player.play();
    }

    /// Stop and clear the current song.
    pub fn stop(&self) {
        self.player.stop();
    }

    /// Whether playback is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// Whether nothing is queued (song finished or stopped).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.player.empty()
    }

    /// The playback position in seconds, as reported by the device
    /// (coarse; smooth it through [`crate::clock::SongClock`]).
    #[must_use]
    pub fn position_s(&self) -> f64 {
        self.player.get_pos().as_secs_f64()
    }

    /// Seek to a position in seconds.
    pub fn seek_s(&self, position_s: f64) -> Result<(), PlaybackError> {
        self.player
            .try_seek(Duration::from_secs_f64(position_s.max(0.0)))
            .map_err(|e| PlaybackError::Seek(e.to_string()))
    }

    /// Set the music volume (0.0–1.0, values above 1.0 amplify).
    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.max(0.0));
    }
}

impl core::fmt::Debug for MusicPlayer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MusicPlayer")
            .field("paused", &self.is_paused())
            .field("empty", &self.is_empty())
            .field("position_s", &self.position_s())
            .finish()
    }
}

// ---- threaded front-end ------------------------------------------------
//
// Audio output types are not `Send` on every platform, and game engines
// want `Send + Sync` resources. The music thread owns the
// [`MusicPlayer`]; the [`MusicHandle`] is a cheap, cloneable, `Send`
// front-end speaking over a channel, with the playback position
// published through an atomic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::decode::AudioData;

/// Commands understood by the music thread.
enum MusicCommand {
    PlayFile(std::path::PathBuf),
    PlayBuffer(AudioData),
    Pause,
    Resume,
    Stop,
    SeekS(f64),
    Volume(f32),
    Shutdown,
}

/// Shared state published by the music thread.
struct MusicShared {
    /// Playback position in microseconds.
    position_us: AtomicU64,
    /// Whether a song is loaded and not yet finished.
    active: AtomicBool,
    /// Whether playback is paused.
    paused: AtomicBool,
    /// Incremented on every successful song start.
    generation: AtomicU64,
    /// Whether the audio output could be opened at all.
    healthy: AtomicBool,
}

/// A `Send + Sync` handle to the music thread.
///
/// All methods are non-blocking; position/state reads are atomics.
#[derive(Clone)]
pub struct MusicHandle {
    commands: Sender<MusicCommand>,
    shared: Arc<MusicShared>,
}

impl MusicHandle {
    /// Play a song from disk (streamed).
    pub fn play_file(&self, path: std::path::PathBuf) {
        let _ = self.commands.send(MusicCommand::PlayFile(path));
    }

    /// Play an in-memory buffer (e.g. the synthesized demo song).
    pub fn play_buffer(&self, audio: AudioData) {
        let _ = self.commands.send(MusicCommand::PlayBuffer(audio));
    }

    /// Pause playback.
    pub fn pause(&self) {
        let _ = self.commands.send(MusicCommand::Pause);
    }

    /// Resume playback.
    pub fn resume(&self) {
        let _ = self.commands.send(MusicCommand::Resume);
    }

    /// Stop and unload the current song.
    pub fn stop(&self) {
        let _ = self.commands.send(MusicCommand::Stop);
    }

    /// Seek to a position in seconds (file sources only).
    pub fn seek_s(&self, position_s: f64) {
        let _ = self.commands.send(MusicCommand::SeekS(position_s));
    }

    /// Set music volume (0.0–1.0).
    pub fn set_volume(&self, volume: f32) {
        let _ = self.commands.send(MusicCommand::Volume(volume));
    }

    /// The device-reported playback position in seconds (coarse —
    /// smooth it through [`crate::clock::SongClock`]).
    #[must_use]
    pub fn position_s(&self) -> f64 {
        self.shared.position_us.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Whether a song is loaded and not finished.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::Relaxed)
    }

    /// Whether playback is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    /// Increments every time a new song starts (lets consumers detect
    /// song changes without extra bookkeeping).
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Relaxed)
    }

    /// Whether the audio output opened successfully. When `false`, the
    /// game keeps running silently instead of crashing.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.shared.healthy.load(Ordering::Relaxed)
    }

    /// Ask the music thread to exit. Remaining handles become no-ops.
    /// (The thread also exits when every handle is dropped.)
    pub fn shutdown(&self) {
        let _ = self.commands.send(MusicCommand::Shutdown);
    }
}

impl core::fmt::Debug for MusicHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MusicHandle")
            .field("position_s", &self.position_s())
            .field("active", &self.is_active())
            .field("paused", &self.is_paused())
            .field("healthy", &self.is_healthy())
            .finish()
    }
}

/// Spawn the music thread and return its handle.
///
/// If the audio output cannot be opened the handle still works (the
/// game runs silently) and [`MusicHandle::is_healthy`] reports `false`.
#[must_use]
pub fn spawn_music_thread() -> MusicHandle {
    let (tx, rx) = channel();
    let shared = Arc::new(MusicShared {
        position_us: AtomicU64::new(0),
        active: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        healthy: AtomicBool::new(true),
    });
    let thread_shared = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("beatbyte-music".into())
        .spawn(move || music_thread(&rx, &thread_shared))
        .map_or_else(|_| shared.healthy.store(false, Ordering::Relaxed), |_| ());
    MusicHandle {
        commands: tx,
        shared,
    }
}

fn music_thread(rx: &Receiver<MusicCommand>, shared: &MusicShared) {
    let mut player = match MusicPlayer::new() {
        Ok(player) => Some(player),
        Err(_) => {
            shared.healthy.store(false, Ordering::Relaxed);
            None
        }
    };

    loop {
        // Drain all pending commands, then publish state.
        loop {
            match rx.try_recv() {
                Ok(command) => {
                    if handle_command(command, &mut player, shared) {
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if let Some(player) = &player {
            let micros = (player.position_s() * 1_000_000.0) as u64;
            shared.position_us.store(micros, Ordering::Relaxed);
            shared.active.store(!player.is_empty(), Ordering::Relaxed);
            shared.paused.store(player.is_paused(), Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Apply one command; returns `true` on shutdown.
fn handle_command(
    command: MusicCommand,
    player: &mut Option<MusicPlayer>,
    shared: &MusicShared,
) -> bool {
    let Some(player) = player.as_mut() else {
        return matches!(command, MusicCommand::Shutdown);
    };
    match command {
        MusicCommand::PlayFile(path) => {
            if player.play_file(&path).is_ok() {
                shared.generation.fetch_add(1, Ordering::Relaxed);
                shared.position_us.store(0, Ordering::Relaxed);
            }
        }
        MusicCommand::PlayBuffer(audio) => {
            let channels = core::num::NonZero::<u16>::MIN; // mono
            if let Some(rate) = core::num::NonZero::new(audio.sample_rate()) {
                let source =
                    rodio::buffer::SamplesBuffer::new(channels, rate, audio.samples().to_vec());
                player.stop();
                player.player.append(source);
                player.player.play();
                shared.generation.fetch_add(1, Ordering::Relaxed);
                shared.position_us.store(0, Ordering::Relaxed);
            }
        }
        MusicCommand::Pause => player.pause(),
        MusicCommand::Resume => player.resume(),
        MusicCommand::Stop => {
            player.stop();
            shared.active.store(false, Ordering::Relaxed);
        }
        MusicCommand::SeekS(position_s) => {
            let _ = player.seek_s(position_s);
        }
        MusicCommand::Volume(volume) => player.set_volume(volume),
        MusicCommand::Shutdown => return true,
    }
    false
}
