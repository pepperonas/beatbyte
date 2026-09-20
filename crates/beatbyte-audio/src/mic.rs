//! The microphone path's pure half: decimation, hop scheduling and
//! putting a captured frame on the song's timeline.
//!
//! Everything here is samples in, values out — no device, no thread,
//! no clock. [`crate::listen`] owns the stream and drives this; a test
//! drives it with a synthetic tone and gets the same answers.
//!
//! ## Why the audio callback does none of it
//!
//! The device callback runs on a realtime thread with a hard
//! deadline, and the only thing it does is convert and copy. Pitch
//! detection, resampling and every allocation happen on the worker
//! that already exists for the stage meters — measured at 1.6 % of a
//! core per hop, which is why one worker serves both rather than a
//! second thread serving one.
//!
//! ## Why it decimates instead of resampling
//!
//! A device runs at 44.1 or 48 kHz; the detector wants about 16. A
//! windowed-sinc resampler called once per block has an edge at every
//! block boundary, and the detector's whole job is to measure
//! periodicity across those boundaries. An **integer decimator** with
//! a rolling filter history has no boundaries at all: it is one
//! continuous filter over the stream, and a test can prove the
//! stream's pitch survives it.

use crate::pitch::{PitchConfig, PitchDetector, PitchFrame};

/// The rate the detector is aimed at. Every fundamental a voice
/// produces and several of its harmonics fit under 8 kHz, and this
/// is the rate the offline analysis runs at too.
pub const TARGET_RATE: u32 = 16_000;

/// Seconds between pitch estimates.
pub const HOP_S: f32 = 0.016;

/// Seconds of signal each estimate sees.
pub const WINDOW_S: f32 = 0.064;

/// A streaming integer decimator: a low-pass, then every `factor`th
/// sample.
///
/// The filter history rolls across pushes, so a stream decimated in
/// one call and in a hundred gives the same samples. That property is
/// the reason this exists rather than a per-block resample, and it
/// has a test.
pub struct Decimator {
    factor: usize,
    taps: Vec<f32>,
    history: Vec<f32>,
    /// Where the next write goes in the ring.
    cursor: usize,
    /// Samples until the next output.
    countdown: usize,
}

impl Decimator {
    /// A decimator that keeps one sample in `factor`.
    ///
    /// `factor` 1 is a pass-through, which is what a device already
    /// at the target rate gets — no filter, no cost, no rounding.
    #[must_use]
    pub fn new(factor: usize) -> Decimator {
        let factor = factor.max(1);
        let taps = if factor == 1 {
            vec![1.0]
        } else {
            low_pass(factor)
        };
        let length = taps.len();
        Decimator {
            factor,
            taps,
            history: vec![0.0; length],
            cursor: 0,
            countdown: 1,
        }
    }

    /// The factor it was built with.
    #[must_use]
    pub fn factor(&self) -> usize {
        self.factor
    }

    /// Push samples; append the decimated ones to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.factor == 1 {
            out.extend_from_slice(input);
            return;
        }
        let length = self.taps.len();
        for &sample in input {
            self.history[self.cursor] = sample;
            self.cursor = (self.cursor + 1) % length;
            self.countdown -= 1;
            if self.countdown > 0 {
                continue;
            }
            self.countdown = self.factor;
            // The newest sample is at `cursor - 1`; walk back through
            // the ring so tap 0 multiplies the newest.
            let mut sum = 0.0f32;
            let mut index = self.cursor;
            for tap in &self.taps {
                index = if index == 0 { length - 1 } else { index - 1 };
                sum += tap * self.history[index];
            }
            out.push(sum);
        }
    }
}

/// A Hann-windowed sinc low-pass with its cutoff at the decimated
/// Nyquist, normalised to unity at DC.
fn low_pass(factor: usize) -> Vec<f32> {
    // Enough taps to be a real filter and few enough to be cheap:
    // eight per output sample.
    let length = factor * 8 + 1;
    let half = (length / 2) as isize;
    let cutoff = 0.5 / factor as f32;
    let mut taps = Vec::with_capacity(length);
    let mut sum = 0.0f32;
    for k in 0..length {
        let n = k as isize - half;
        #[expect(clippy::cast_precision_loss, reason = "a tap index, hundreds at most")]
        let x = n as f32;
        let sinc = if n == 0 {
            2.0 * cutoff
        } else {
            (core::f32::consts::TAU * cutoff * x).sin() / (core::f32::consts::PI * x)
        };
        #[expect(clippy::cast_precision_loss, reason = "a tap count, hundreds at most")]
        let window = 0.5 - 0.5 * (core::f32::consts::TAU * k as f32 / (length - 1) as f32).cos();
        let tap = sinc * window;
        sum += tap;
        taps.push(tap);
    }
    for tap in &mut taps {
        *tap /= sum;
    }
    taps
}

/// The decimation factor that lands `device_rate` nearest
/// [`TARGET_RATE`] without going under it.
///
/// Under it would throw away fundamentals the detector is asked for,
/// so the factor is the largest that keeps the decimated rate at or
/// above the target: 48 kHz → 3 (16 kHz), 44.1 kHz → 2 (22.05 kHz),
/// 16 kHz → 1. Pure — tested.
#[must_use]
pub fn decimation_for(device_rate: u32) -> usize {
    if device_rate <= TARGET_RATE {
        return 1;
    }
    (device_rate / TARGET_RATE).max(1) as usize
}

/// One estimate, stamped on the capture stream's own clock.
///
/// `capture_s` counts **samples**, not wall time: it is exact, it
/// cannot drift, and it is the only honest answer to "when was this
/// sung" before anything knows about a song.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapturedFrame {
    /// Seconds from the first captured sample to the CENTRE of the
    /// window this estimate came from. The centre, because that is
    /// where the estimate applies — the start would put every note
    /// half a window early.
    pub capture_s: f64,
    /// What the detector said.
    pub pitch: PitchFrame,
}

/// Turns a stream of device samples into pitch estimates.
///
/// Owns its decimator, its ring and its detector, so the hot path
/// allocates nothing after the first block: the frames it produces go
/// into a buffer the caller owns.
pub struct PitchStream {
    decimator: Decimator,
    rate: u32,
    detector: PitchDetector,
    window: usize,
    hop: usize,
    /// The decimated samples a window may still need.
    ring: Vec<f32>,
    /// The absolute index, in decimated samples, of `ring[0]`.
    ring_start: u64,
    /// The absolute index one past the next window's last sample.
    next_end: u64,
    /// Scratch the decimator writes into.
    scratch: Vec<f32>,
}

impl PitchStream {
    /// A stream for a device running at `device_rate`.
    #[must_use]
    pub fn new(device_rate: u32, config: PitchConfig) -> PitchStream {
        let factor = decimation_for(device_rate);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a sample rate divided by a small factor"
        )]
        let rate = (device_rate / factor as u32).max(1);
        let detector = PitchDetector::new(PitchConfig {
            sample_rate: rate,
            ..config
        });
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "positive sample counts from positive seconds"
        )]
        let window = ((WINDOW_S * rate as f32) as usize).max(detector.min_window());
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "positive sample counts from positive seconds"
        )]
        let hop = ((HOP_S * rate as f32) as usize).max(1);
        PitchStream {
            decimator: Decimator::new(factor),
            rate,
            detector,
            window,
            hop,
            ring: Vec::with_capacity(window * 2),
            ring_start: 0,
            next_end: window as u64,
            scratch: Vec::with_capacity(4096),
        }
    }

    /// The rate the detector actually runs at.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The delay between a sound happening and the frame that
    /// measures it being produced, in seconds.
    ///
    /// Half the window: an estimate is stamped at its window's centre
    /// but cannot exist until the window is full. This is the part of
    /// the microphone's latency that is **known** rather than
    /// measured, and calibration subtracts it before measuring the
    /// rest.
    #[must_use]
    pub fn known_latency_s(&self) -> f64 {
        self.window as f64 / 2.0 / f64::from(self.rate)
    }

    /// Push a block of device samples; append whatever estimates
    /// became possible to `out`.
    ///
    /// ⚠️ Positions are tracked in **absolute decimated samples**,
    /// not per push. The first version added the whole block to a
    /// counter before emitting, so every frame a single push produced
    /// carried the same stamp — a whole block of estimates landing on
    /// one instant, which a hop-spacing test caught at "gap 0".
    pub fn push(&mut self, input: &[f32], out: &mut Vec<CapturedFrame>) {
        self.scratch.clear();
        let mut scratch = std::mem::take(&mut self.scratch);
        self.decimator.push(input, &mut scratch);
        self.ring.extend_from_slice(&scratch);
        self.scratch = scratch;

        let window = self.window as u64;
        let hop = self.hop as u64;
        let available = self.ring_start + self.ring.len() as u64;
        while available >= self.next_end {
            let end = (self.next_end - self.ring_start) as usize;
            let start = end - self.window;
            let pitch = self.detector.detect(&self.ring[start..end]);
            #[expect(
                clippy::cast_precision_loss,
                reason = "a sample index; f64 holds a song's worth exactly"
            )]
            let centre = (self.next_end as f64) - window as f64 / 2.0;
            out.push(CapturedFrame {
                capture_s: centre / f64::from(self.rate),
                pitch,
            });
            self.next_end += hop;
        }
        // Keep only what the next window can still need.
        let needed_from = self.next_end.saturating_sub(window);
        if needed_from > self.ring_start {
            let drop = (needed_from - self.ring_start) as usize;
            let drop = drop.min(self.ring.len());
            self.ring.drain(..drop);
            self.ring_start += drop as u64;
        }
    }
}

/// Where the capture clock and the song clock were seen to agree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptureAnchor {
    /// A capture-stream instant.
    pub capture_s: f64,
    /// The song time at that instant.
    pub song_time_s: f64,
}

/// A step larger than this is a new correspondence, not a drift.
pub const ANCHOR_SNAP_S: f64 = 0.050;

/// How much of a small disagreement is taken out per reconciliation.
pub const ANCHOR_SLEW: f64 = 0.10;

impl CaptureAnchor {
    /// Keep the two clocks together as they drift apart.
    ///
    /// The microphone and the speakers are two crystals. Over a four
    /// minute song a hundred parts per million is twenty-four
    /// milliseconds — small, and squarely inside the tolerances this
    /// game judges on, so it is corrected rather than ignored.
    ///
    /// The shape is the one [`crate::clock::SongClock`] already uses
    /// against the audio device, and for the same reason: a large
    /// disagreement is a NEW correspondence and is taken in one step
    /// (a song started, a device reopened), while a small one is
    /// drift and is eased out, because snapping the anchor every
    /// frame would make every note's judged time jitter. Returns
    /// whether it snapped. Pure — tested.
    pub fn reconcile(&mut self, capture_now_s: f64, song_now_s: f64) -> bool {
        let implied = self.song_time_s + (capture_now_s - self.capture_s);
        let error = song_now_s - implied;
        if error.abs() > ANCHOR_SNAP_S {
            self.capture_s = capture_now_s;
            self.song_time_s = song_now_s;
            return true;
        }
        self.song_time_s += error * ANCHOR_SLEW;
        false
    }
}

/// Put a captured frame on the song's timeline.
///
/// Three things are subtracted, and each is a different kind of
/// number: the anchor is where the two clocks were seen together,
/// `known_latency_s` is arithmetic the detector can state exactly
/// (half its window), and `mic_offset_ms` is the rest — the output
/// path, the air, the input path — which can only be **measured**,
/// per device, by the calibration screen.
///
/// ⚠️ It is not the controller's `latency_offset_ms`. That value
/// contains human reaction time, which a microphone does not have.
/// Pure — tested.
#[must_use]
pub fn to_song_time(
    frame_capture_s: f64,
    anchor: CaptureAnchor,
    known_latency_s: f64,
    mic_offset_ms: f32,
) -> f64 {
    let since_anchor = frame_capture_s - anchor.capture_s;
    anchor.song_time_s + since_anchor - known_latency_s - f64::from(mic_offset_ms) / 1000.0
}

/// The longest round trip worth looking for, in seconds. Bluetooth
/// can be a third of a second; beyond half a second something else
/// is wrong and a number found out there would be noise.
pub const MAX_ROUND_TRIP_S: f64 = 0.5;

/// How much better the best match must be than the runner-up to be
/// believed.
pub const CALIBRATION_MARGIN: f32 = 1.5;

/// Measure the round trip: how long after the game plays a sound the
/// microphone hears it.
///
/// This is the number the calibration screen exists to produce, and
/// it is the one part of a microphone's latency that cannot be
/// derived — it is the output buffer, the speaker, the air, the
/// input buffer and the driver, and only the room can say.
///
/// ## Why it is not a player tapping along
///
/// The controller's offset is measured by asking someone to tap on
/// the beat, so it contains their reaction time. A microphone has no
/// reaction time, and reusing that number would make every singer
/// late by however long the player's hands take. So the game plays a
/// known signal and looks for it coming back.
///
/// The signal must be one that correlates sharply with itself and
/// with nothing else in a room — a short swept chirp, which is what
/// [`chirp`] builds. Returns `None` when the best match is not
/// clearly better than the next best: a room that returned nothing
/// useful must say so rather than hand back its loudest accident.
/// Pure — tested.
#[must_use]
pub fn round_trip_offset(played: &[f32], captured: &[f32], sample_rate: u32) -> Option<f64> {
    let rate = f64::from(sample_rate.max(1));
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "a positive lag in samples from positive seconds"
    )]
    let max_lag = (MAX_ROUND_TRIP_S * rate) as usize;
    if played.is_empty() || captured.len() <= played.len() {
        return None;
    }
    let max_lag = max_lag.min(captured.len() - played.len());
    // The played signal's own energy, once: the correlation is
    // normalised by BOTH sides.
    let played_energy: f64 = played.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    if played_energy <= 0.0 {
        return None;
    }
    let score_at = |lag: usize| -> f32 {
        let mut sum = 0.0f64;
        let mut energy = 0.0f64;
        for (i, &sample) in played.iter().enumerate() {
            let heard = f64::from(captured[lag + i]);
            sum += f64::from(sample) * heard;
            energy += heard * heard;
        }
        // ⚠️ Normalised by BOTH sides — the NCC, bounded in [-1, 1].
        // Dividing by the captured energy alone looks equivalent and
        // is not: as a window approaches silence its energy goes to
        // zero faster than its correlation does, so a sliver of room
        // noise scores arbitrarily high and wins. The first version
        // did exactly that and could not find a chirp it had placed
        // at zero delay.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a correlation in [-1, 1]; f32 is the precision compared"
        )]
        let score = if energy > 0.0 {
            (sum / (played_energy * energy).sqrt()) as f32
        } else {
            0.0
        };
        score
    };

    let mut best = (0usize, f32::NEG_INFINITY);
    for lag in 0..=max_lag {
        let score = score_at(lag);
        if score > best.1 {
            best = (lag, score);
        }
    }
    if best.1 <= 0.0 {
        return None;
    }
    // ⚠️ The runner-up must come from OUTSIDE the peak's own
    // neighbourhood. A lag one sample either side of the answer
    // scores almost exactly as well — of course it does, it is the
    // same match — so comparing against it rejects every correct
    // measurement there is. The first version did, and could not
    // confirm a chirp sitting at zero delay. The exclusion is the
    // chirp's own width, which is how far its match can plausibly
    // smear.
    let guard = played.len() / 2;
    let mut runner_up = f32::NEG_INFINITY;
    for lag in 0..=max_lag {
        if lag.abs_diff(best.0) <= guard {
            continue;
        }
        runner_up = runner_up.max(score_at(lag));
    }
    // Nothing outside the peak at all: the signal is shorter than the
    // search, which is a clean match rather than an ambiguous one.
    if runner_up.is_finite() && best.1 < runner_up * CALIBRATION_MARGIN {
        return None;
    }
    Some(best.0 as f64 / rate)
}

/// A short swept chirp: the signal the calibration plays.
///
/// A sweep rather than a tone or a click. A tone correlates with
/// itself at every period, so the answer would be ambiguous by whole
/// periods; a click is broadband but tiny, so a room swallows it.
/// A sweep is long enough to be heard and correlates sharply only
/// with itself. Pure — tested.
#[must_use]
pub fn chirp(sample_rate: u32, seconds: f32, from_hz: f32, to_hz: f32) -> Vec<f32> {
    let rate = sample_rate.max(1) as f32;
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "a positive sample count from positive seconds"
    )]
    let n = (seconds.max(0.0) * rate) as usize;
    let mut out = Vec::with_capacity(n);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / n.max(1) as f32;
        let hz = from_hz + (to_hz - from_hz) * t;
        // A raised-cosine envelope: a chirp that starts and stops
        // abruptly is a click at each end, and the click correlates
        // better than the sweep does.
        let window = 0.5 - 0.5 * (core::f32::consts::TAU * t).cos();
        out.push(0.7 * window * phase.sin());
        phase += core::f32::consts::TAU * hz / rate;
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tone(hz: f32, n: usize, rate: u32) -> Vec<f32> {
        (0..n)
            .map(|i| 0.4 * (core::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn a_device_rate_picks_a_factor_that_never_goes_under_the_target() {
        assert_eq!(decimation_for(48_000), 3, "48 kHz lands exactly on 16");
        assert_eq!(decimation_for(44_100), 2, "22.05 kHz, not 14.7");
        assert_eq!(decimation_for(96_000), 6);
        assert_eq!(decimation_for(16_000), 1, "already there: no filter at all");
        assert_eq!(decimation_for(8_000), 1, "below the target, left alone");
        assert_eq!(decimation_for(0), 1, "a nonsense rate is not a panic");
        for rate in [22_050u32, 32_000, 44_100, 48_000, 88_200, 96_000, 192_000] {
            let decimated = rate / decimation_for(rate) as u32;
            assert!(
                decimated >= TARGET_RATE,
                "{rate} Hz decimated to {decimated}, under the target"
            );
        }
    }

    #[test]
    fn decimating_in_one_call_and_in_many_gives_the_same_stream() {
        // The property the whole design rests on: no block boundary
        // exists, so the detector sees one continuous signal however
        // the device happens to hand it over.
        let input = tone(220.0, 6000, 48_000);
        let mut whole = Vec::new();
        Decimator::new(3).push(&input, &mut whole);

        let mut pieces = Vec::new();
        let mut decimator = Decimator::new(3);
        // Deliberately awkward block sizes, none a multiple of 3.
        let mut at = 0;
        for size in [7usize, 128, 1, 499, 1000, 64].iter().cycle() {
            if at >= input.len() {
                break;
            }
            let end = (at + size).min(input.len());
            decimator.push(&input[at..end], &mut pieces);
            at = end;
        }
        assert_eq!(whole.len(), pieces.len());
        for (a, b) in whole.iter().zip(&pieces) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn a_factor_of_one_is_a_pass_through() {
        let input = tone(440.0, 500, 16_000);
        let mut out = Vec::new();
        let mut decimator = Decimator::new(1);
        decimator.push(&input, &mut out);
        assert_eq!(decimator.factor(), 1);
        assert_eq!(out, input, "a device already at the target is untouched");
    }

    #[test]
    fn decimation_keeps_the_pitch_it_was_given() {
        // The point of the filter: 48 kHz in, 16 kHz out, same note.
        let mut stream = PitchStream::new(48_000, PitchConfig::default());
        assert_eq!(stream.rate(), 16_000);
        let mut frames = Vec::new();
        for hz in [110.0f32, 220.0, 440.0] {
            frames.clear();
            let mut fresh = PitchStream::new(48_000, PitchConfig::default());
            fresh.push(&tone(hz, 48_000 / 2, 48_000), &mut frames);
            let voiced: Vec<f32> = frames.iter().filter_map(|f| f.pitch.hz).collect();
            assert!(!voiced.is_empty(), "{hz} Hz produced no estimate");
            let middle = voiced[voiced.len() / 2];
            let cents = 1200.0 * (middle / hz).log2();
            assert!(
                cents.abs() < 10.0,
                "{hz} Hz read as {middle} ({cents:.1} cents)"
            );
        }
        stream.push(&tone(440.0, 1024, 48_000), &mut frames);
    }

    #[test]
    fn the_aliasing_a_naive_decimation_would_cause_is_filtered_away() {
        // 15 560 Hz at 48 kHz folds to exactly 440 Hz when every third
        // sample is taken without a filter — a confident A4 that
        // nobody sang. The filter's whole job is that it does not.
        //
        // ⚠️ The first version of this test used 7 kHz and failed:
        // 7 kHz is UNDER the filter's cutoff and under the decimated
        // Nyquist, so it never aliases at all. What the detector
        // found there was its seventh subharmonic at 1 kHz — correct
        // behaviour for a tone above the vocal range, and nothing to
        // do with decimation. A test has to reproduce the mechanism
        // it claims to be about.
        let input = tone(15_560.0, 48_000 / 2, 48_000);

        // The gegenprobe FIRST: taking every third sample by hand,
        // with no filter, must produce the phantom A4 — otherwise
        // this test could pass because the signal is harmless rather
        // than because the filter works.
        let naive: Vec<f32> = input.iter().step_by(3).copied().collect();
        let mut detector = PitchDetector::new(PitchConfig {
            sample_rate: 16_000,
            ..PitchConfig::default()
        });
        let phantom = detector.detect(&naive[..2048]);
        let phantom_hz = phantom
            .hz
            .expect("an unfiltered fold should read as a confident pitch");
        assert!(
            (phantom_hz - 440.0).abs() < 20.0,
            "the gegenprobe did not fold to A4 ({phantom_hz} Hz); this test proves nothing"
        );

        // And now the real path.
        let mut stream = PitchStream::new(48_000, PitchConfig::default());
        let mut frames = Vec::new();
        stream.push(&input, &mut frames);
        let fooled = frames
            .iter()
            .filter(|f| f.pitch.hz.is_some_and(|hz| (hz - 440.0).abs() < 30.0))
            .count();
        assert_eq!(
            fooled,
            0,
            "{fooled} of {} frames heard the aliased A4 through the filter",
            frames.len()
        );
    }

    #[test]
    fn frames_arrive_a_hop_apart_stamped_at_their_window_centres() {
        let mut stream = PitchStream::new(48_000, PitchConfig::default());
        let mut frames = Vec::new();
        // One second of audio, handed over in awkward blocks.
        let input = tone(220.0, 48_000, 48_000);
        for block in input.chunks(731) {
            stream.push(block, &mut frames);
        }
        assert!(
            frames.len() > 50,
            "only {} frames in a second",
            frames.len()
        );
        // A hop apart, to the sample.
        for pair in frames.windows(2) {
            let gap = pair[1].capture_s - pair[0].capture_s;
            assert!((gap - f64::from(HOP_S)).abs() < 1e-6, "gap {gap}");
        }
        // The first frame's centre is half a window in, not zero.
        let first = frames[0].capture_s;
        assert!(
            (first - stream.known_latency_s()).abs() < 1e-6,
            "first frame stamped at {first}, half a window is {}",
            stream.known_latency_s()
        );
        // And the stamps track real time: the last frame is within a
        // window of the second that was pushed.
        let last = frames.last().unwrap().capture_s;
        assert!((1.0 - last) < f64::from(WINDOW_S), "last frame at {last}");
    }

    #[test]
    fn the_known_latency_is_half_a_window_and_nothing_else() {
        let stream = PitchStream::new(48_000, PitchConfig::default());
        // 64 ms window at 16 kHz → 32 ms of unavoidable delay.
        assert!(
            (stream.known_latency_s() - 0.032).abs() < 0.005,
            "{}",
            stream.known_latency_s()
        );
    }

    #[test]
    fn a_frame_lands_on_the_song_where_the_offsets_put_it() {
        let anchor = CaptureAnchor {
            capture_s: 10.0,
            song_time_s: 30.0,
        };
        // Same instant as the anchor, no latency, no offset.
        assert!((to_song_time(10.0, anchor, 0.0, 0.0) - 30.0).abs() < 1e-9);
        // A second later is a second later.
        assert!((to_song_time(11.0, anchor, 0.0, 0.0) - 31.0).abs() < 1e-9);
        // The detector's own delay moves the frame EARLIER: the sound
        // happened before the estimate existed.
        assert!((to_song_time(11.0, anchor, 0.032, 0.0) - 30.968).abs() < 1e-9);
        // And so does the measured device offset, in milliseconds.
        assert!((to_song_time(11.0, anchor, 0.0, 120.0) - 30.88).abs() < 1e-9);
        // A negative offset (an input ahead of the output) pushes it
        // later rather than being clamped away.
        assert!((to_song_time(11.0, anchor, 0.0, -20.0) - 31.02).abs() < 1e-9);
    }

    #[test]
    fn a_big_disagreement_snaps_and_a_small_one_is_eased_out() {
        // A new song, or a reopened device: one step.
        let mut anchor = CaptureAnchor {
            capture_s: 10.0,
            song_time_s: 30.0,
        };
        assert!(
            anchor.reconcile(11.0, 45.0),
            "a fifteen-second jump is not drift"
        );
        assert_eq!(anchor.capture_s, 11.0);
        assert_eq!(anchor.song_time_s, 45.0);

        // Two crystals drifting: eased, never snapped, and it
        // converges rather than oscillating.
        let mut anchor = CaptureAnchor {
            capture_s: 0.0,
            song_time_s: 0.0,
        };
        let mut error = f64::MAX;
        for step in 1..=200 {
            let capture_now = f64::from(step) * 0.1;
            // The song clock running 100 ppm fast.
            let song_now = capture_now * 1.0001;
            assert!(
                !anchor.reconcile(capture_now, song_now),
                "drift must not snap"
            );
            let implied = anchor.song_time_s + (capture_now - anchor.capture_s);
            error = (song_now - implied).abs();
        }
        assert!(error < 0.001, "the anchor never caught up: {error} s out");
    }

    #[test]
    fn reconciling_does_not_move_a_clock_that_already_agrees() {
        let mut anchor = CaptureAnchor {
            capture_s: 5.0,
            song_time_s: 20.0,
        };
        let before = anchor;
        assert!(!anchor.reconcile(6.0, 21.0));
        assert_eq!(anchor, before, "an exact agreement was corrected anyway");
    }

    #[test]
    fn the_round_trip_is_found_where_it_was_put() {
        let rate = 48_000u32;
        let played = chirp(rate, 0.08, 300.0, 4_000.0);
        for delay_ms in [0.0f64, 12.0, 45.0, 210.0] {
            let delay = (delay_ms / 1000.0 * f64::from(rate)) as usize;
            let mut captured = vec![0.0f32; delay];
            // The room: quieter, and with noise on top.
            let mut state = 0x1234_5678_9ABC_DEF0u64;
            captured.extend(played.iter().map(|s| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                let noise = ((state >> 40) as f32 / 16_777_216.0 - 0.5) * 0.02;
                s * 0.3 + noise
            }));
            captured.extend(std::iter::repeat_n(0.0, 4_000));
            let found = round_trip_offset(&played, &captured, rate)
                .unwrap_or_else(|| panic!("{delay_ms} ms was not found at all"));
            let error_ms = (found * 1000.0 - delay_ms).abs();
            assert!(
                error_ms < 1.0,
                "{delay_ms} ms read as {:.1} ms",
                found * 1000.0
            );
        }
    }

    #[test]
    fn a_room_that_heard_nothing_says_so_rather_than_guessing() {
        // The failure that matters: a wrong offset silently makes
        // every note late for the rest of the game.
        let rate = 48_000u32;
        let played = chirp(rate, 0.08, 300.0, 4_000.0);
        let mut state = 0xDEAD_BEEF_CAFE_1234u64;
        let noise: Vec<f32> = (0..rate as usize)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                (state >> 40) as f32 / 16_777_216.0 - 0.5
            })
            .collect();
        assert_eq!(
            round_trip_offset(&played, &noise, rate),
            None,
            "noise was taken for the chirp"
        );
        assert_eq!(
            round_trip_offset(&played, &vec![0.0; rate as usize], rate),
            None,
            "silence produced an offset"
        );
        // And nonsense inputs are not a panic.
        assert_eq!(round_trip_offset(&[], &noise, rate), None);
        assert_eq!(round_trip_offset(&played, &played[..10], rate), None);
    }

    #[test]
    fn the_chirp_is_a_sweep_that_starts_and_ends_quietly() {
        let c = chirp(48_000, 0.08, 300.0, 4_000.0);
        assert_eq!(c.len(), 3_840);
        // The envelope: a chirp that starts abruptly is a click, and
        // a click correlates better than the sweep it is attached to.
        assert!(c[0].abs() < 0.01, "it starts at {}", c[0]);
        assert!(c[c.len() - 1].abs() < 0.01);
        let peak = c.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.5 && peak <= 0.7, "peak {peak}");
        // It really sweeps: the second half crosses zero far more
        // often than the first.
        let crossings = |half: &[f32]| half.windows(2).filter(|p| p[0] * p[1] < 0.0).count();
        let half = c.len() / 2;
        // 300 Hz to 4 kHz: the halves average about 1.1 and 3.1 kHz,
        // so the second crosses zero roughly two and a half times as
        // often. Asking for three was my arithmetic, not the signal's.
        assert!(
            crossings(&c[half..]) > crossings(&c[..half]) * 2,
            "{} against {}",
            crossings(&c[half..]),
            crossings(&c[..half])
        );
        // A zero-length request is empty, not a panic.
        assert!(chirp(48_000, 0.0, 300.0, 4_000.0).is_empty());
    }

    #[test]
    fn a_silent_stream_produces_frames_that_say_they_are_silent() {
        // Not no frames: the HUD needs to know the microphone is
        // alive and hearing nothing, which is a different thing from
        // a microphone that has stopped.
        let mut stream = PitchStream::new(48_000, PitchConfig::default());
        let mut frames = Vec::new();
        stream.push(&vec![0.0f32; 48_000 / 2], &mut frames);
        assert!(!frames.is_empty(), "silence produced no frames at all");
        assert!(frames.iter().all(|f| !f.pitch.voiced));
        assert!(frames.iter().all(|f| f.pitch.hz.is_none()));
    }

    #[test]
    fn the_ring_does_not_grow_with_the_song() {
        // An hour of singing must not be an hour of samples in memory.
        let mut stream = PitchStream::new(48_000, PitchConfig::default());
        let mut frames = Vec::new();
        let block = tone(220.0, 4800, 48_000);
        for _ in 0..200 {
            frames.clear();
            stream.push(&block, &mut frames);
        }
        assert!(
            stream.ring.len() <= stream.window + stream.hop,
            "the ring holds {} samples",
            stream.ring.len()
        );
    }
}
