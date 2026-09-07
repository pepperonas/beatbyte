//! Listening to the room: the machine's own audio input, read for
//! two numbers — the level in dBFS and the tempo in BPM — and
//! nothing else.
//!
//! This exists for the two monitors on the PA stacks (the left one
//! shows the tempo, the right one the level). Both are **measured**,
//! at the laptop, from whatever the input device hears; neither is
//! taken from the chart. Three rules:
//!
//! - **Nothing leaves.** Samples are reduced to two floats on the
//!   thread that receives them and are never written, kept or sent.
//!   The handle exposes the two floats and a state, that is all.
//! - **Absence is a state, not a zero.** No input device, a device
//!   that refuses to open, a stream that dies: [`Listener::state`]
//!   says so, and the monitors do not exist. A device that opens but
//!   never delivers a non-zero sample — which is what a denied
//!   microphone permission looks like on macOS — is reported as not
//!   yet heard; nothing is shown for it either.
//! - **The device thread owns the device.** A `cpal::Stream` is not
//!   `Send`, so, like the music player, the stream lives on a thread
//!   of its own that also runs the analysis; the handle shares
//!   atomics with it and stops it on drop.
//!
//! The estimator is the one the reference rig runs (`disco-controller`
//! `BpmAnalyzer`, itself a port of `bpbel` / `inspector-rust`): band
//! energy 30–150 Hz per block, an onset when the energy beats its
//! 3-second moving average by a margin AND rises above the recent
//! peak, the tempo as the median inter-onset interval folded into
//! 60–200 BPM, shown as a 4-second mean. [`LiveAnalyzer`] is pure and
//! pinned on synthesized input; the device code is exercised by the
//! game, as the player is.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use realfft::RealFftPlanner;

/// Samples per analysis block: ~21 ms at 48 kHz, the same hop the
/// reference rig runs.
pub const BLOCK: usize = 1024;
/// The rolling FFT window the bass band is read from: long, so the
/// 30–150 Hz band spans real bins instead of sharing one.
pub const WINDOW: usize = 4096;
/// The band the onset detector listens to, in Hz.
pub const BAND_HZ: (f32, f32) = (30.0, 150.0);
/// The quietest level reported, in dBFS: digital silence and a
/// denied device both sit here.
pub const DB_FLOOR: f32 = -100.0;
/// Below this level no onset is counted: room noise never makes a
/// beat, so a quiet room shows no tempo instead of a made-up one.
pub const LOUD_DB: f32 = -50.0;

/// Onset threshold over the moving average.
const ONSET_RATIO: f32 = 1.4;
/// The moving-average baseline window, seconds.
const AVG_WINDOW_S: f64 = 3.0;
/// Minimum gap between onsets (⇒ at most 200 BPM).
const ONSET_REFRACTORY_S: f64 = 0.30;
/// Sliding window of onsets the median is taken over.
const IOI_WINDOW_S: f64 = 6.0;
/// Onsets needed before a tempo is claimed.
const MIN_ONSETS: usize = 4;
/// Without a valid estimate for this long the display returns to
/// "no tempo".
const STALE_RESET_S: f64 = 4.0;
/// The tempo range the raw estimate is folded into.
pub const BPM_RANGE: (f32, f32) = (60.0, 200.0);
/// Snap a raw estimate back to the locked octave within this.
pub const OCTAVE_SNAP: f32 = 8.0;
/// The rolling mean the displayed tempo is smoothed over.
const DISPLAY_WINDOW_S: f64 = 4.0;
/// SuperFlux-style rise test: the current block must beat the
/// peak of `SF_WIN` blocks lagged `SF_LAG` back by this factor.
const SF_LAG: usize = 4;
const SF_WIN: usize = 4;
const SF_MARGIN: f32 = 1.04;
/// Level smoothing per block, so the readout does not flicker at
/// the block rate (a ~100 ms ease).
const DB_EASE: f32 = 0.35;

/// What the listener is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ListenState {
    /// The device thread is still opening the input.
    Opening = 0,
    /// The input is open and delivering blocks.
    Listening = 1,
    /// No input device, a device that would not open, or a stream
    /// that died. Final.
    Unavailable = 2,
}

impl ListenState {
    fn from_u8(value: u8) -> ListenState {
        match value {
            1 => ListenState::Listening,
            2 => ListenState::Unavailable,
            _ => ListenState::Opening,
        }
    }
}

/// The numbers the device thread shares with the game.
struct Shared {
    state: AtomicU8,
    /// A non-zero sample has arrived at least once.
    heard: AtomicBool,
    /// f32 bits: the eased level in dBFS.
    db: AtomicU32,
    /// f32 bits: the displayed tempo, 0 = none.
    bpm: AtomicU32,
    /// Blocks analysed so far (diagnostics).
    blocks: AtomicU64,
    stop: AtomicBool,
}

/// The handle the game holds: two floats and a state, and the
/// thread stops when it drops.
pub struct Listener {
    shared: Arc<Shared>,
}

impl Listener {
    /// Open the default input device on a thread of its own and
    /// start listening. Never fails: a missing or refusing device
    /// shows up as [`ListenState::Unavailable`].
    #[must_use]
    pub fn open() -> Listener {
        let shared = Arc::new(Shared {
            state: AtomicU8::new(ListenState::Opening as u8),
            heard: AtomicBool::new(false),
            db: AtomicU32::new(DB_FLOOR.to_bits()),
            bpm: AtomicU32::new(0f32.to_bits()),
            blocks: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("beatbyte-listen".into())
            .spawn(move || device::run(&worker));
        if spawned.is_err() {
            shared
                .state
                .store(ListenState::Unavailable as u8, Ordering::Release);
        }
        Listener { shared }
    }

    /// A handle without a device, for tests and harnesses: listening
    /// and, if `heard`, measuring `db` and `bpm`; not heard = an open
    /// device that never delivered a sample (a refused permission).
    #[must_use]
    pub fn stub(heard: bool, db: f32, bpm: Option<f32>) -> Listener {
        Listener {
            shared: Arc::new(Shared {
                state: AtomicU8::new(ListenState::Listening as u8),
                heard: AtomicBool::new(heard),
                db: AtomicU32::new(db.to_bits()),
                bpm: AtomicU32::new(bpm.unwrap_or(0.0).to_bits()),
                blocks: AtomicU64::new(0),
                stop: AtomicBool::new(false),
            }),
        }
    }

    /// What the listener is doing.
    #[must_use]
    pub fn state(&self) -> ListenState {
        ListenState::from_u8(self.shared.state.load(Ordering::Acquire))
    }

    /// Whether a non-zero sample has ever arrived. A device that is
    /// open but silent forever is a permission that was refused (or
    /// a muted input): nothing is measured, so nothing is shown.
    #[must_use]
    pub fn heard(&self) -> bool {
        self.shared.heard.load(Ordering::Acquire)
    }

    /// The monitors may exist: listening, and something was heard.
    #[must_use]
    pub fn measuring(&self) -> bool {
        self.state() == ListenState::Listening && self.heard()
    }

    /// The level in dBFS (`DB_FLOOR` ..= 0), eased over ~100 ms.
    #[must_use]
    pub fn db(&self) -> f32 {
        f32::from_bits(self.shared.db.load(Ordering::Relaxed))
    }

    /// The tempo in BPM, or `None` while no tempo is heard.
    #[must_use]
    pub fn bpm(&self) -> Option<f32> {
        let bpm = f32::from_bits(self.shared.bpm.load(Ordering::Relaxed));
        (bpm > 0.0).then_some(bpm)
    }

    /// Blocks analysed so far.
    #[must_use]
    pub fn blocks(&self) -> u64 {
        self.shared.blocks.load(Ordering::Relaxed)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

/// Level of one block in dBFS from its RMS, floored. Pure — tested.
#[must_use]
pub fn dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return DB_FLOOR;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let rms = mean_square.sqrt();
    if rms <= 0.0 {
        return DB_FLOOR;
    }
    (20.0 * rms.log10()).clamp(DB_FLOOR, 0.0)
}

/// Fold a raw tempo into [`BPM_RANGE`] by octaves. Pure — tested.
#[must_use]
pub fn fold_octaves(mut bpm: f32) -> f32 {
    if bpm <= 0.0 || !bpm.is_finite() {
        return 0.0;
    }
    while bpm < BPM_RANGE.0 {
        bpm *= 2.0;
    }
    while bpm > BPM_RANGE.1 {
        bpm /= 2.0;
    }
    bpm
}

/// Snap `raw` to the octave of a locked `display` tempo when it is
/// within [`OCTAVE_SNAP`] of half or double it: a few rogue
/// intervals must not flip 120 ↔ 240. Pure — tested.
#[must_use]
pub fn snap_octave(raw: f32, display: f32) -> f32 {
    if display <= 0.0 {
        return raw;
    }
    if (raw * 2.0 - display).abs() < OCTAVE_SNAP {
        raw * 2.0
    } else if (raw / 2.0 - display).abs() < OCTAVE_SNAP {
        raw / 2.0
    } else {
        raw
    }
}

/// The energy-onset + median-IOI tempo estimator, fed one band
/// energy per block. Pure; time is whatever the caller passes.
#[derive(Debug, Clone)]
pub struct BpmEstimator {
    onsets: std::collections::VecDeque<f64>,
    energy: std::collections::VecDeque<(f64, f32)>,
    recent: std::collections::VecDeque<f32>,
    raw_history: std::collections::VecDeque<(f64, f32)>,
    first_t: Option<f64>,
    last_onset: f64,
    last_valid: f64,
    /// The tempo on display, 0 = none.
    pub display_bpm: f32,
    /// How regular the intervals are, 0..1.
    pub confidence: f32,
}

impl Default for BpmEstimator {
    fn default() -> Self {
        BpmEstimator {
            onsets: std::collections::VecDeque::new(),
            energy: std::collections::VecDeque::new(),
            recent: std::collections::VecDeque::with_capacity(SF_LAG + SF_WIN + 1),
            raw_history: std::collections::VecDeque::new(),
            first_t: None,
            last_onset: -1e9,
            last_valid: -1e9,
            display_bpm: 0.0,
            confidence: 0.0,
        }
    }
}

impl BpmEstimator {
    /// Feed one block's band energy at `now` seconds. `allow` is the
    /// loudness gate: false still moves the baseline but counts no
    /// onset. Returns whether an onset fired.
    pub fn push(&mut self, energy: f32, now: f64, allow: bool) -> bool {
        let first = *self.first_t.get_or_insert(now);
        self.energy.push_back((now, energy));
        while self
            .energy
            .front()
            .is_some_and(|&(t, _)| now - t > AVG_WINDOW_S)
        {
            self.energy.pop_front();
        }
        // The SuperFlux reference: the peak of a lagged window,
        // before this attack's smear.
        let reference = if self.recent.len() >= SF_LAG + SF_WIN {
            self.recent
                .iter()
                .take(SF_WIN)
                .copied()
                .fold(0.0f32, f32::max)
        } else {
            0.0
        };
        self.recent.push_back(energy);
        while self.recent.len() > SF_LAG + SF_WIN {
            self.recent.pop_front();
        }
        // The baseline needs its window before anything counts.
        if now - first < AVG_WINDOW_S {
            return false;
        }
        let average = self.energy.iter().map(|&(_, e)| e).sum::<f32>() / self.energy.len() as f32;
        if average <= 1e-9 {
            return false;
        }
        let rising = energy > reference * SF_MARGIN;
        let fired = allow
            && energy > average * ONSET_RATIO
            && rising
            && now - self.last_onset >= ONSET_REFRACTORY_S;
        if fired {
            self.last_onset = now;
            self.onsets.push_back(now);
            self.trim_onsets(now);
        }
        fired
    }

    fn trim_onsets(&mut self, now: f64) {
        while self.onsets.front().is_some_and(|&t| now - t > IOI_WINDOW_S) {
            self.onsets.pop_front();
        }
    }

    /// Recompute the displayed tempo and confidence. Cheap; call per
    /// block.
    pub fn estimate(&mut self, now: f64) {
        self.trim_onsets(now);
        if self.onsets.len() < MIN_ONSETS {
            if self.display_bpm > 0.0 && now - self.last_valid > STALE_RESET_S {
                self.display_bpm = 0.0;
                self.raw_history.clear();
                self.confidence = 0.0;
            }
            return;
        }
        let onsets: Vec<f64> = self.onsets.iter().copied().collect();
        let intervals: Vec<f64> = onsets.windows(2).map(|w| w[1] - w[0]).collect();
        let mut ordered = intervals.clone();
        ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = ordered[ordered.len() / 2];
        if median <= 0.0 {
            return;
        }
        let raw = snap_octave(fold_octaves((60.0 / median) as f32), self.display_bpm);
        if self
            .raw_history
            .back()
            .is_none_or(|&(_, last)| (last - raw).abs() > 0.01)
        {
            self.raw_history.push_back((now, raw));
        }
        while self
            .raw_history
            .front()
            .is_some_and(|&(t, _)| now - t > DISPLAY_WINDOW_S)
        {
            self.raw_history.pop_front();
        }
        if !self.raw_history.is_empty() {
            self.display_bpm = self.raw_history.iter().map(|&(_, b)| b).sum::<f32>()
                / self.raw_history.len() as f32;
        }
        self.last_valid = now;
        let variance =
            intervals.iter().map(|i| (i - median).powi(2)).sum::<f64>() / intervals.len() as f64;
        self.confidence = (1.0 - (variance.sqrt() / median) as f32).clamp(0.0, 1.0);
    }
}

/// The whole per-block analysis: level and tempo from mono samples.
/// Pure — pinned on synthesized input.
pub struct LiveAnalyzer {
    rate: f32,
    window: Vec<f32>,
    hann: Vec<f32>,
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    fft_in: Vec<f32>,
    fft_out: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    band: (usize, usize),
    samples_seen: u64,
    /// The eased level in dBFS.
    pub db: f32,
    /// The tempo estimator.
    pub bpm: BpmEstimator,
}

impl LiveAnalyzer {
    /// An analyzer for input at `rate` Hz.
    #[must_use]
    pub fn new(rate: f32) -> LiveAnalyzer {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WINDOW);
        let hann: Vec<f32> = (0..WINDOW)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / WINDOW as f32).cos())
            .collect();
        let bin = |hz: f32| ((hz / rate.max(1.0)) * WINDOW as f32).round() as usize;
        let band = (bin(BAND_HZ.0).max(1), bin(BAND_HZ.1).min(WINDOW / 2));
        LiveAnalyzer {
            rate,
            window: vec![0.0; WINDOW],
            hann,
            fft_in: fft.make_input_vec(),
            fft_out: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            band,
            samples_seen: 0,
            db: DB_FLOOR,
            bpm: BpmEstimator::default(),
        }
    }

    /// Seconds of audio analysed so far.
    #[must_use]
    pub fn now(&self) -> f64 {
        self.samples_seen as f64 / f64::from(self.rate.max(1.0))
    }

    /// Analyse one block of mono samples (any length; [`BLOCK`] is
    /// the intended one). Returns whether an onset fired.
    pub fn push(&mut self, block: &[f32]) -> bool {
        if block.is_empty() {
            return false;
        }
        // Level: the block's own RMS, eased.
        let level = dbfs(block);
        self.db += (level - self.db) * DB_EASE;
        // The rolling window for the band energy.
        let n = block.len().min(WINDOW);
        self.window.rotate_left(n);
        let tail = WINDOW - n;
        self.window[tail..].copy_from_slice(&block[block.len() - n..]);
        for (slot, (sample, weight)) in self
            .fft_in
            .iter_mut()
            .zip(self.window.iter().zip(&self.hann))
        {
            *slot = sample * weight;
        }
        // realfft only fails on wrong buffer lengths, fixed here.
        let _ =
            self.fft
                .process_with_scratch(&mut self.fft_in, &mut self.fft_out, &mut self.scratch);
        let energy: f32 = self.fft_out[self.band.0..=self.band.1]
            .iter()
            .map(|c| c.norm_sqr())
            .sum::<f32>()
            / WINDOW as f32;
        self.samples_seen += block.len() as u64;
        let now = self.now();
        let fired = self.bpm.push(energy, now, self.db > LOUD_DB);
        self.bpm.estimate(now);
        fired
    }
}

/// The device side: opens the default input on the listener's
/// thread, feeds blocks to a [`LiveAnalyzer`], publishes the two
/// numbers. Not unit-tested (no input device on CI); exercised by
/// the game.
mod device {
    use super::{
        BLOCK, DB_FLOOR, ListenState, LiveAnalyzer, Ordering, Receiver, Shared, SyncSender,
        TrySendError, sync_channel,
    };
    use cpal::FromSample;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::time::Duration;

    /// Blocks that may wait for the analysis before the newest is
    /// dropped: a backlog of sound is not worth keeping.
    const QUEUE: usize = 64;

    fn fail(shared: &Shared) {
        shared
            .state
            .store(ListenState::Unavailable as u8, Ordering::Release);
    }

    /// Fold interleaved samples of any format to mono f32 and hand
    /// them to the analysis; a full queue drops the block.
    fn feed<T: cpal::Sample>(
        data: &[T],
        channels: usize,
        mono: &mut Vec<f32>,
        tx: &SyncSender<Vec<f32>>,
        heard: &std::sync::atomic::AtomicBool,
    ) where
        f32: cpal::FromSample<T>,
    {
        let channels = channels.max(1);
        for frame in data.chunks(channels) {
            let sum: f32 = frame.iter().map(|s| f32::from_sample_(*s)).sum();
            let sample: f32 = sum / frame.len() as f32;
            if sample != 0.0 && !heard.load(Ordering::Relaxed) {
                heard.store(true, Ordering::Release);
            }
            mono.push(sample);
            if mono.len() >= BLOCK {
                let block = std::mem::replace(mono, Vec::with_capacity(BLOCK));
                if let Err(TrySendError::Full(_)) = tx.try_send(block) {
                    // Dropped: the analysis is behind, the newest
                    // block is the one that matters and the next
                    // one will be it.
                }
            }
        }
    }

    pub(super) fn run(shared: &Shared) {
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            return fail(shared);
        };
        let Ok(config) = device.default_input_config() else {
            return fail(shared);
        };
        let channels = usize::from(config.channels());
        let rate = config.sample_rate() as f32;
        let (tx, rx): (SyncSender<Vec<f32>>, Receiver<Vec<f32>>) = sync_channel(QUEUE);
        let errored = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let on_error = {
            let errored = std::sync::Arc::clone(&errored);
            move |_err: cpal::StreamError| errored.store(true, Ordering::Release)
        };
        let heard = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        macro_rules! build {
            ($t:ty) => {{
                let tx = tx.clone();
                let heard = std::sync::Arc::clone(&heard);
                let mut mono: Vec<f32> = Vec::with_capacity(BLOCK);
                device.build_input_stream::<$t, _, _>(
                    &config.config(),
                    move |data: &[$t], _| feed(data, channels, &mut mono, &tx, &heard),
                    on_error,
                    None,
                )
            }};
        }
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::I32 => build!(i32),
            cpal::SampleFormat::I8 => build!(i8),
            cpal::SampleFormat::U8 => build!(u8),
            cpal::SampleFormat::F64 => build!(f64),
            _ => return fail(shared),
        };
        let Ok(stream) = stream else {
            return fail(shared);
        };
        if stream.play().is_err() {
            return fail(shared);
        }
        shared
            .state
            .store(ListenState::Listening as u8, Ordering::Release);
        let mut analyzer = LiveAnalyzer::new(rate);
        while !shared.stop.load(Ordering::Acquire) {
            if errored.load(Ordering::Acquire) {
                fail(shared);
                break;
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(block) => {
                    analyzer.push(&block);
                    if heard.load(Ordering::Acquire) {
                        shared.heard.store(true, Ordering::Release);
                    }
                    shared
                        .db
                        .store(analyzer.db.max(DB_FLOOR).to_bits(), Ordering::Relaxed);
                    shared
                        .bpm
                        .store(analyzer.bpm.display_bpm.to_bits(), Ordering::Relaxed);
                    shared.blocks.fetch_add(1, Ordering::Relaxed);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    fail(shared);
                    break;
                }
            }
        }
        drop(stream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kick pattern at `bpm`: a decaying 60 Hz burst on every beat
    /// over quiet noise, `seconds` long at `rate`.
    fn kicks(bpm: f32, seconds: f32, rate: f32) -> Vec<f32> {
        let n = (seconds * rate) as usize;
        let beat = 60.0 / bpm;
        let mut state = 0x9E37_79B9_u32;
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                let since = t % beat;
                let burst = if since < 0.12 {
                    0.8 * (-since * 30.0).exp() * (2.0 * std::f32::consts::PI * 60.0 * t).sin()
                } else {
                    0.0
                };
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let noise = (state as f32 / u32::MAX as f32 - 0.5) * 0.002;
                burst + noise
            })
            .collect()
    }

    fn run(samples: &[f32], rate: f32) -> LiveAnalyzer {
        let mut analyzer = LiveAnalyzer::new(rate);
        for block in samples.chunks(BLOCK) {
            analyzer.push(block);
        }
        analyzer
    }

    #[test]
    fn silence_is_the_floor_and_no_tempo() {
        let analyzer = run(&vec![0.0; 48_000 * 8], 48_000.0);
        assert!((analyzer.db - DB_FLOOR).abs() < 1e-3, "{}", analyzer.db);
        assert_eq!(analyzer.bpm.display_bpm, 0.0);
    }

    #[test]
    fn a_full_scale_sine_reads_minus_three_dbfs() {
        let rate = 48_000.0;
        let sine: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate).sin())
            .collect();
        assert!((dbfs(&sine) + 3.01).abs() < 0.05, "{}", dbfs(&sine));
        let analyzer = run(&sine, rate);
        assert!(
            (analyzer.db + 3.01).abs() < 0.1,
            "eased level {}",
            analyzer.db
        );
    }

    #[test]
    fn a_kick_pattern_reads_its_tempo() {
        for (bpm, rate) in [(120.0, 48_000.0), (96.0, 44_100.0), (140.0, 48_000.0)] {
            let analyzer = run(&kicks(bpm, 14.0, rate), rate);
            let shown = analyzer.bpm.display_bpm;
            assert!(
                (shown - bpm).abs() < 2.5,
                "{bpm} BPM at {rate} Hz read as {shown} (confidence {})",
                analyzer.bpm.confidence
            );
            assert!(analyzer.bpm.confidence > 0.8);
        }
    }

    #[test]
    fn room_noise_under_the_gate_makes_no_tempo() {
        // The same kick pattern, forty dB down: it is a rhythm, but
        // a quiet one — below LOUD_DB nothing is a beat.
        let rate = 48_000.0;
        let quiet: Vec<f32> = kicks(120.0, 14.0, rate).iter().map(|s| s * 0.003).collect();
        let analyzer = run(&quiet, rate);
        assert!(analyzer.db < LOUD_DB, "{}", analyzer.db);
        assert_eq!(analyzer.bpm.display_bpm, 0.0, "no tempo from a whisper");
    }

    #[test]
    fn the_tempo_goes_stale_when_the_music_stops() {
        let rate = 48_000.0;
        // Twelve seconds of kicks, then silence long enough for the
        // onset window (6 s) to empty AND the stale timer (4 s) to
        // run out — a stop is announced, not guessed at.
        let mut samples = kicks(120.0, 12.0, rate);
        samples.extend(vec![0.0; (rate * 12.0) as usize]);
        let mut analyzer = LiveAnalyzer::new(rate);
        let mut had_tempo = false;
        for block in samples.chunks(BLOCK) {
            analyzer.push(block);
            had_tempo |= analyzer.bpm.display_bpm > 0.0;
        }
        assert!(had_tempo);
        assert_eq!(
            analyzer.bpm.display_bpm, 0.0,
            "reset after the stale window"
        );
    }

    #[test]
    fn octaves_fold_and_snap() {
        assert_eq!(fold_octaves(30.0), 60.0);
        assert_eq!(fold_octaves(29.0), 116.0);
        assert_eq!(fold_octaves(480.0), 120.0);
        assert_eq!(fold_octaves(250.0), 125.0);
        assert_eq!(fold_octaves(0.0), 0.0);
        assert_eq!(snap_octave(62.0, 120.0), 124.0);
        assert_eq!(snap_octave(190.0, 96.0), 95.0);
        assert_eq!(snap_octave(130.0, 120.0), 130.0);
        assert_eq!(snap_octave(62.0, 0.0), 62.0);
    }

    /// Manual: `cargo test -p beatbyte-audio hear_the_room -- --ignored
    /// --nocapture` opens the real input for four seconds and prints
    /// what it makes of the room. Ignored: CI has no input device.
    #[test]
    #[ignore = "needs an input device; run by hand"]
    fn hear_the_room_for_four_seconds() {
        let listener = Listener::open();
        for tenth in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if tenth % 5 == 4 {
                println!(
                    "{:4.1} s  state={:?} heard={} blocks={} db={:6.1} bpm={:?}",
                    (tenth + 1) as f32 / 10.0,
                    listener.state(),
                    listener.heard(),
                    listener.blocks(),
                    listener.db(),
                    listener.bpm()
                );
            }
        }
    }

    #[test]
    fn a_fresh_listener_shows_nothing_until_it_is_heard() {
        // The state machine without a device: a handle whose thread
        // never reached the device is not measuring.
        let shared = Arc::new(Shared {
            state: AtomicU8::new(ListenState::Listening as u8),
            heard: AtomicBool::new(false),
            db: AtomicU32::new(DB_FLOOR.to_bits()),
            bpm: AtomicU32::new(0f32.to_bits()),
            blocks: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        });
        let listener = Listener {
            shared: Arc::clone(&shared),
        };
        assert!(!listener.measuring(), "open but silent = a refused device");
        assert_eq!(listener.bpm(), None);
        shared.heard.store(true, Ordering::Release);
        assert!(listener.measuring());
        shared
            .state
            .store(ListenState::Unavailable as u8, Ordering::Release);
        assert!(!listener.measuring(), "a dead stream ends the monitors");
        drop(listener);
        assert!(shared.stop.load(Ordering::Acquire), "drop stops the thread");
    }
}
