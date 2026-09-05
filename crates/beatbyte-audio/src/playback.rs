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
    /// The OUTGOING song during a DJ crossfade: it keeps playing on
    /// its own player while its volume ramps to silence. `None`
    /// outside a transition.
    tail: Option<TailFade>,
    /// The volume the game asked for; during a fade both players are
    /// driven from it through the crossfade gains.
    base_volume: f32,
    /// Global silence, held HERE rather than folded into
    /// `base_volume` by each caller. It used to be the caller's job
    /// (`set_volume(v * muted_factor)`), and the browser's song
    /// preview forgot it — one missing multiplication and a muted
    /// run played out loud until the key was toggled twice. A gate
    /// the player owns cannot be forgotten by a new call site.
    muted: bool,
    /// Playback speed factor (practice mode). The device position
    /// rodio reports lives in the OUTPUT timeline (it advances at
    /// wall-clock pace whatever the factor), while the game thinks
    /// in SOURCE seconds — the chart's timeline. The base pair below
    /// anchors the affine map between the two; it is re-based on
    /// every play, seek and speed change, because after a mid-song
    /// speed change the naive `output × factor` is off by a constant.
    speed: f64,
    /// Source position at the last re-base.
    src_base_s: f64,
    /// Output (reported) position at the last re-base.
    out_base_s: f64,
}

/// The affine output→source map: source seconds at `outer_now`,
/// given the base pair and the factor in effect since it. Pure —
/// this arithmetic is the whole correctness of practice-speed
/// timing, so it is testable without an audio device.
#[must_use]
pub fn map_source_position(src_base_s: f64, out_base_s: f64, speed: f64, outer_now_s: f64) -> f64 {
    (outer_now_s - out_base_s).mul_add(speed, src_base_s)
}

/// The outgoing side of a crossfade.
struct TailFade {
    player: Player,
    started: std::time::Instant,
    fade_s: f32,
}

/// The gain actually handed to an output player: the volume the game
/// asked for, scaled by a crossfade side's gain, and silenced while
/// muted. Mute WINS over any volume — that is the whole point of
/// keeping it here. Pure — tested.
#[must_use]
pub fn output_gain(base_volume: f32, muted: bool, fade_gain: f32) -> f32 {
    if muted {
        0.0
    } else {
        base_volume.max(0.0) * fade_gain
    }
}

/// Equal-power crossfade gains at `elapsed` seconds into a fade of
/// `fade_s`: `(outgoing, incoming)`. The pair sums in POWER, not in
/// amplitude — a linear fade dips audibly in the middle, which is
/// exactly the "hard stop" feel the transition exists to avoid.
/// Pure — tested.
#[must_use]
pub fn crossfade_gains(elapsed_s: f32, fade_s: f32) -> (f32, f32) {
    let t = (elapsed_s / fade_s.max(1e-3)).clamp(0.0, 1.0);
    let angle = t * core::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
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
            tail: None,
            base_volume: 1.0,
            muted: false,
            speed: 1.0,
            src_base_s: 0.0,
            out_base_s: 0.0,
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
        self.replace_player(|fresh| fresh.append(decoder));
        Ok(duration)
    }

    /// Start playing an in-memory buffer, replacing anything current.
    pub fn play_buffer(&mut self, audio: &AudioData) {
        let channels = core::num::NonZero::<u16>::MIN; // mono
        let Some(rate) = core::num::NonZero::new(audio.sample_rate()) else {
            return;
        };
        let source = rodio::buffer::SamplesBuffer::new(channels, rate, audio.samples().to_vec());
        self.replace_player(|fresh| fresh.append(source));
    }

    /// A new song gets a NEW output player; the old one is stopped
    /// and dropped, along with any crossfade tail.
    ///
    /// Not a tidiness measure. rodio's `stop()` only raises a flag:
    /// its position is zeroed later, on the audio thread, when the
    /// stopped source is next pulled — so for a few milliseconds
    /// after `stop()`+`append()` the SAME player still reports the
    /// OLD song's position. Measured: 185.630 s right after loading
    /// a new file, the browser preview's place inside another track.
    /// Publish that once and the game anchors its clock three minutes
    /// into a song that just began. A fresh player's position is
    /// zero from its first sample.
    fn replace_player(&mut self, load: impl FnOnce(&Player)) {
        if let Some(tail) = self.tail.take() {
            tail.player.stop();
        }
        let fresh = Player::connect_new(self._device.mixer());
        load(&fresh);
        fresh.set_volume(output_gain(self.base_volume, self.muted, 1.0));
        if self.speed != 1.0 {
            #[allow(clippy::cast_possible_truncation)]
            fresh.set_speed(self.speed as f32);
        }
        fresh.play();
        let old = core::mem::replace(&mut self.player, fresh);
        old.stop();
        self.src_base_s = 0.0;
        self.out_base_s = 0.0;
    }

    /// Change the playback speed (practice mode; pitch moves with
    /// it). Re-bases the position map at the current moment so
    /// source time stays continuous across the change. Degenerate
    /// factors are refused.
    pub fn set_speed(&mut self, speed: f64) {
        if !(speed.is_finite() && speed > 0.0) {
            return;
        }
        let outer = self.player.get_pos().as_secs_f64();
        self.src_base_s = map_source_position(self.src_base_s, self.out_base_s, self.speed, outer);
        self.out_base_s = outer;
        self.speed = speed;
        #[allow(clippy::cast_possible_truncation)]
        self.player.set_speed(speed as f32);
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

    /// The playback position in SOURCE seconds — the chart's
    /// timeline, at any speed (coarse; smooth it through
    /// [`crate::clock::SongClock`]).
    #[must_use]
    pub fn position_s(&self) -> f64 {
        map_source_position(
            self.src_base_s,
            self.out_base_s,
            self.speed,
            self.player.get_pos().as_secs_f64(),
        )
    }

    /// Seek to a SOURCE position in seconds. The player's own seek
    /// speaks the output timeline (its speed stage multiplies the
    /// target back up), so the target is divided by the factor and
    /// the position map re-based on the landing point.
    pub fn seek_s(&mut self, position_s: f64) -> Result<(), PlaybackError> {
        let source_s = position_s.max(0.0);
        let outer_target = source_s / self.speed;
        self.player
            .try_seek(Duration::from_secs_f64(outer_target))
            .map_err(|e| PlaybackError::Seek(e.to_string()))?;
        self.src_base_s = source_s;
        self.out_base_s = outer_target;
        Ok(())
    }

    /// Set the music volume (0.0–1.0, values above 1.0 amplify).
    /// Silence stays silent: while muted this only remembers the
    /// level to return to.
    pub fn set_volume(&mut self, volume: f32) {
        self.base_volume = volume.max(0.0);
        self.apply_gain();
    }

    /// Silence (or unsilence) the output without disturbing the
    /// volume the game asked for.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.apply_gain();
    }

    /// Push the current gain to the live player. Skipped during a
    /// fade, where `tick_fade` reapplies both sides every 2 ms tick
    /// anyway.
    fn apply_gain(&self) {
        if self.tail.is_none() {
            self.player
                .set_volume(output_gain(self.base_volume, self.muted, 1.0));
        }
    }

    /// Begin a DJ crossfade into a file: the current song keeps
    /// playing as the outgoing tail while the new one starts at
    /// silence; [`MusicPlayer::tick_fade`] ramps both. Position and
    /// speed reporting swap to the NEW song immediately.
    pub fn crossfade_to_file(&mut self, path: &Path, fade_s: f32) -> Result<(), PlaybackError> {
        let display = path.display().to_string();
        let file = File::open(path).map_err(|source| PlaybackError::Open {
            path: display.clone(),
            source,
        })?;
        let decoder = Decoder::try_from(file).map_err(|source| PlaybackError::Decode {
            path: display,
            source,
        })?;
        self.begin_crossfade(fade_s, |incoming| incoming.append(decoder));
        Ok(())
    }

    /// [`MusicPlayer::crossfade_to_file`] for an in-memory song.
    pub fn crossfade_to_buffer(&mut self, audio: &AudioData, fade_s: f32) {
        let channels = core::num::NonZero::<u16>::MIN; // mono
        let Some(rate) = core::num::NonZero::new(audio.sample_rate()) else {
            return;
        };
        let source = rodio::buffer::SamplesBuffer::new(channels, rate, audio.samples().to_vec());
        self.begin_crossfade(fade_s, |incoming| incoming.append(source));
    }

    /// The shared half of both crossfades: a fresh player becomes
    /// the current one, the old one becomes the fading tail.
    fn begin_crossfade(&mut self, fade_s: f32, load: impl FnOnce(&Player)) {
        // A fade already running: the old tail has had its moment.
        if let Some(tail) = self.tail.take() {
            tail.player.stop();
        }
        let incoming = Player::connect_new(self._device.mixer());
        load(&incoming);
        incoming.set_volume(0.0);
        if self.speed != 1.0 {
            incoming.set_speed(self.speed as f32);
        }
        incoming.play();
        let outgoing = core::mem::replace(&mut self.player, incoming);
        self.tail = Some(TailFade {
            player: outgoing,
            started: std::time::Instant::now(),
            fade_s,
        });
        // The map re-bases on the NEW song's zero.
        self.src_base_s = 0.0;
        self.out_base_s = 0.0;
    }

    /// Advance a running crossfade; call once per thread tick. Does
    /// nothing outside a fade.
    pub fn tick_fade(&mut self) {
        let Some(tail) = &self.tail else {
            return;
        };
        let elapsed = tail.started.elapsed().as_secs_f32();
        let (out_gain, in_gain) = crossfade_gains(elapsed, tail.fade_s);
        if elapsed >= tail.fade_s {
            if let Some(done) = self.tail.take() {
                done.player.stop();
            }
            self.apply_gain();
            return;
        }
        tail.player
            .set_volume(output_gain(self.base_volume, self.muted, out_gain));
        self.player
            .set_volume(output_gain(self.base_volume, self.muted, in_gain));
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
    CrossfadeFile(std::path::PathBuf, f32),
    CrossfadeBuffer(AudioData, f32),
    Pause,
    Resume,
    Stop,
    SeekS(f64),
    Volume(f32),
    Mute(bool),
    Speed(f64),
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

impl MusicShared {
    /// Announce that a new song has begun: position first, generation
    /// last, and the generation with `Release`.
    ///
    /// The order is not cosmetic. A consumer watches the generation to
    /// notice a song change, then reads the position to anchor its
    /// clock. Bumping the generation FIRST leaves a window in which
    /// that consumer sees the new song carrying the previous song's
    /// position - and if the previous song ended at 248 s, the new
    /// one's clock is anchored at 248 s. Every note is then already in
    /// the past: the highway stays empty and the song counts as
    /// finished the instant it begins.
    ///
    /// `Release` pairs with the `Acquire` on the generation load, so
    /// seeing the new generation guarantees seeing the reset position.
    /// Two relaxed stores would permit the same stale pairing even in
    /// the right order.
    fn begin_song(&self) {
        self.position_us.store(0, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }
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

    /// DJ-crossfade into a file over `fade_s` seconds: the current
    /// song fades out underneath while the new one fades in and
    /// becomes the reported song (generation bumps, so the game
    /// clock re-anchors to it).
    pub fn crossfade_file(&self, path: std::path::PathBuf, fade_s: f32) {
        let _ = self
            .commands
            .send(MusicCommand::CrossfadeFile(path, fade_s));
    }

    /// [`MusicHandle::crossfade_file`] for an in-memory song.
    pub fn crossfade_buffer(&self, audio: AudioData, fade_s: f32) {
        let _ = self
            .commands
            .send(MusicCommand::CrossfadeBuffer(audio, fade_s));
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

    /// Set music volume (0.0–1.0). While muted this only chooses the
    /// level playback returns to — see [`MusicHandle::set_muted`].
    pub fn set_volume(&self, volume: f32) {
        let _ = self.commands.send(MusicCommand::Volume(volume));
    }

    /// Silence or unsilence all music. Independent of the volume, so
    /// nothing that starts a song can undo it by accident.
    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(MusicCommand::Mute(muted));
    }

    /// Change the playback speed (practice mode; 1.0 = normal, pitch
    /// moves with it). Applied on the music thread; the reported
    /// position stays in source seconds at any factor.
    pub fn set_speed(&self, speed: f64) {
        let _ = self.commands.send(MusicCommand::Speed(speed));
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
        // Acquire, paired with the Release in `MusicShared::begin_song`:
        // seeing a new generation must guarantee seeing its reset
        // position, or the caller anchors the new song's clock to
        // where the previous one stopped.
        self.shared.generation.load(Ordering::Acquire)
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
        if let Some(player) = player.as_mut() {
            player.tick_fade();
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
                shared.begin_song();
            }
        }
        MusicCommand::PlayBuffer(audio) => {
            player.play_buffer(&audio);
            shared.begin_song();
        }
        MusicCommand::CrossfadeFile(path, fade_s) => {
            if player.crossfade_to_file(&path, fade_s).is_ok() {
                // A new song begins the moment the fade starts: the
                // clock re-anchors to the INCOMING side while the
                // outgoing tail plays itself out underneath.
                shared.begin_song();
            }
        }
        MusicCommand::CrossfadeBuffer(audio, fade_s) => {
            player.crossfade_to_buffer(&audio, fade_s);
            shared.begin_song();
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
        MusicCommand::Mute(muted) => player.set_muted(muted),
        MusicCommand::Speed(speed) => player.set_speed(speed),
        MusicCommand::Shutdown => return true,
    }
    false
}

#[cfg(test)]
mod gain_tests {
    use super::output_gain;

    #[test]
    fn mute_beats_every_volume_a_caller_can_ask_for() {
        // The bug this gate exists for: a call site set the raw
        // volume and the muted run played out loud. No argument may
        // lift the silence.
        for volume in [0.0, 0.3, 0.8, 1.0, 4.0] {
            for fade in [0.0, 0.5, 1.0] {
                assert!(
                    output_gain(volume, true, fade).abs() < f32::EPSILON,
                    "volume {volume} at fade {fade} escaped the mute"
                );
            }
        }
    }

    #[test]
    fn unmuted_passes_the_volume_through_the_fade() {
        assert!((output_gain(0.8, false, 1.0) - 0.8).abs() < 1e-6);
        assert!((output_gain(0.8, false, 0.5) - 0.4).abs() < 1e-6);
        // A negative volume is a caller mistake, not an inverted
        // phase: clamp rather than amplify.
        assert!(output_gain(-1.0, false, 1.0).abs() < f32::EPSILON);
    }
}

#[cfg(test)]
mod crossfade_tests {
    use super::crossfade_gains;

    #[test]
    fn the_fade_is_equal_power_and_never_dips() {
        // Ends exact: full outgoing at 0, full incoming at the end.
        let (out0, in0) = crossfade_gains(0.0, 4.0);
        assert!((out0 - 1.0).abs() < 1e-6 && in0.abs() < 1e-6);
        let (out1, in1) = crossfade_gains(4.0, 4.0);
        assert!(out1.abs() < 1e-6 && (in1 - 1.0).abs() < 1e-6);
        // Equal POWER throughout: out^2 + in^2 == 1. A linear fade
        // fails this exactly in the middle - the audible dip.
        for step in 0..=20 {
            let (out, inn) = crossfade_gains(step as f32 * 0.2, 4.0);
            let power = out.mul_add(out, inn * inn);
            assert!((power - 1.0).abs() < 1e-5, "power {power} at step {step}");
        }
        // Beyond the fade: clamped, no wrap-around of the cosine.
        let (out_late, in_late) = crossfade_gains(99.0, 4.0);
        assert!(out_late.abs() < 1e-6 && (in_late - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod mapping_tests {
    use super::map_source_position;

    const EPS: f64 = 1e-9;

    #[test]
    fn at_full_speed_source_equals_output() {
        assert!((map_source_position(0.0, 0.0, 1.0, 12.34) - 12.34).abs() < EPS);
    }

    #[test]
    fn a_mid_song_speed_change_stays_continuous() {
        // 10 s at full speed, then half speed: rodio's own position
        // keeps wall-clock pace, so the naive `output × factor`
        // would be off by a constant after the change — the re-base
        // is the fix, and this walks it exactly.
        let (mut src_base, mut out_base, mut speed) = (0.0f64, 0.0f64, 1.0f64);
        let outer_at_change = 10.0;
        let source_at_change = map_source_position(src_base, out_base, speed, outer_at_change);
        assert!((source_at_change - 10.0).abs() < EPS);
        // Re-base (what set_speed does), then run 4 output seconds
        // at half speed: source advances 2.
        src_base = source_at_change;
        out_base = outer_at_change;
        speed = 0.5;
        let source = map_source_position(src_base, out_base, speed, 14.0);
        assert!((source - 12.0).abs() < EPS, "got {source}");
        // The naive map without re-basing claims 7.0 — a 5-second
        // teleport the clock would snap to.
        assert!((map_source_position(0.0, 0.0, speed, 14.0) - 7.0).abs() < EPS);
    }

    #[test]
    fn a_seek_rebases_in_both_timelines() {
        // Seek to source 30 s at speed 1.25: the player's outer
        // timeline lands at 24 s; from there, 2 outer seconds are
        // 2.5 source seconds.
        let speed = 1.25f64;
        let source_target = 30.0;
        let outer_target = source_target / speed;
        let source = map_source_position(source_target, outer_target, speed, outer_target + 2.0);
        assert!((source - 32.5).abs() < EPS);
    }
}

#[cfg(test)]
mod new_song_tests {
    use super::MusicShared;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    /// The two calls, spelled in pieces so this file's own test code
    /// does not match the patterns it searches for. An earlier version
    /// counted itself and reported two call sites where there was one.
    const BUMP: &str = concat!("generation", ".fetch_add(");
    const RESET: &str = concat!("position_us", ".store(0");

    /// The module's source ABOVE this test module, with comments
    /// stripped.
    ///
    /// Both cuts are load-bearing. The prose in this file quotes the
    /// code it asserts about, and the tests below use the very calls
    /// they are counting - an earlier version searched the whole file
    /// and stayed green while the real code was mutated, because it
    /// kept finding itself.
    fn code() -> String {
        let source = include_str!("playback.rs");
        let shipped = source.split("#[cfg(test)]").next().unwrap_or(source);
        shipped
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_new_song_starts_from_zero() {
        let shared = MusicShared {
            position_us: AtomicU64::new(248_000_000),
            active: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            generation: AtomicU64::new(7),
            healthy: AtomicBool::new(true),
        };
        shared.begin_song();
        assert_eq!(shared.generation.load(Ordering::Acquire), 8);
        assert_eq!(
            shared.position_us.load(Ordering::Relaxed),
            0,
            "the previous song's position survived into the new one"
        );
    }

    #[test]
    fn the_position_is_cleared_before_the_generation_moves() {
        // The defect: the generation was bumped first, so a reader
        // could see the new song carrying the previous song's
        // position and anchor its clock to the end of a track that
        // had not started - an empty highway, and a song judged
        // finished the instant it began.
        //
        // Checked against the SOURCE, because a single-threaded test
        // cannot observe the order of two stores: by the time it
        // reads, both have happened. An earlier version of this test
        // asserted the values and passed happily with the order
        // reversed, which is how it came to be written this way.
        let code = code();
        let reset = code.find(RESET).expect("the position is cleared somewhere");
        let bump = code.find(BUMP).expect("the generation moves somewhere");
        assert!(
            reset < bump,
            "the generation is advanced before the position is cleared"
        );
    }

    #[test]
    fn only_one_place_advances_the_generation() {
        // The ordering above is worth nothing if a second code path
        // writes both atomics by hand - which is exactly how the
        // defect arrived, with two call sites each doing it.
        let bumps = code().matches(BUMP).count();
        assert_eq!(
            bumps, 1,
            "the generation is advanced in {bumps} places; it belongs only in `begin_song`"
        );
    }

    #[test]
    fn the_generation_is_read_with_acquire() {
        // Release without Acquire pairs with nothing, and the stale
        // pairing remains permitted however the stores are ordered.
        assert!(
            code().contains(concat!("generation", ".load(Ordering::Acquire)")),
            "the generation is loaded without Acquire"
        );
    }
}
