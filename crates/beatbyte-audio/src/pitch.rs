//! Monophonic pitch detection: the McLeod Pitch Method, written out.
//!
//! One estimator serves both ends of vocal play — the offline pass
//! over a separated stem and the live pass over a microphone — and
//! that is deliberate: a target built by one algorithm and judged by
//! another would disagree with itself at the edges of every note.
//!
//! ## Why it is written here and not pulled in
//!
//! The plan named the `pitch-detection` crate and listed what it
//! would have to pass first: an allocation audit, a synthetic-signal
//! corpus and a realtime CPU benchmark, against a dependency with a
//! low release cadence. MPM is a page of arithmetic; writing it out
//! satisfies the audit by construction — [`PitchDetector`] owns its
//! scratch and allocates nothing per hop — keeps the result bit-stable
//! across releases of somebody else's crate, and still leaves the
//! seam the plan asked for, since callers hold a [`PitchDetector`] and
//! could hold something else.
//!
//! ## The method
//!
//! McLeod and Wyvill's NSDF (*A Smarter Way to Find Pitch*, ICMC
//! 2005) normalises the autocorrelation by the energy in both windows
//! it compares:
//!
//! ```text
//!             2 · Σ x[j]·x[j+τ]
//!   n'(τ) = ───────────────────────
//!           Σ (x[j]² + x[j+τ]²)
//! ```
//!
//! which lands in `[-1, 1]` regardless of level, so one threshold
//! works for a whisper and a belt. The pitch is the first peak that
//! is *nearly* as tall as the tallest — not the tallest itself, which
//! is what keeps a strong second harmonic from reading an octave
//! high — and the peak is interpolated parabolically, because at
//! 16 kHz one lag is over a semitone up at the top of a soprano's
//! range.
//!
//! The NSDF value at that peak is the **clarity**, and it is the
//! honest confidence: a pure tone reads near 1, a vowel a little
//! lower, noise and two simultaneous voices much lower. Nothing here
//! guesses — an unvoiced frame returns no pitch rather than a number
//! nobody should trust.

/// How the detector is set up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchConfig {
    /// The rate the windows are sampled at.
    pub sample_rate: u32,
    /// The lowest pitch to look for, in Hz.
    pub min_hz: f32,
    /// The highest, in Hz.
    pub max_hz: f32,
    /// How tall a peak must be, as a fraction of the tallest, to be
    /// taken instead of it. McLeod's *k*: lower forgives more octave
    /// error downward, higher risks reading the first harmonic.
    pub peak_ratio: f32,
    /// The clarity below which a frame is not called voiced.
    pub clarity_threshold: f32,
    /// The level below which a frame is not called voiced, in dBFS.
    /// A quiet room still has a most-periodic lag; without this the
    /// detector reports the room's hum as singing.
    pub rms_gate_dbfs: f32,
}

impl Default for PitchConfig {
    /// The singing default: C2 to C6 covers a bass's low E to a
    /// soprano's high C with room either side, and the gate and
    /// threshold are the values the synthetic corpus in this module
    /// is measured at.
    fn default() -> PitchConfig {
        PitchConfig {
            sample_rate: 16_000,
            min_hz: 65.0,
            max_hz: 1_100.0,
            peak_ratio: 0.9,
            clarity_threshold: 0.6,
            rms_gate_dbfs: -45.0,
        }
    }
}

/// The floor reported for digital silence, in dBFS. A real `-inf`
/// would poison every average it reached.
pub const SILENCE_DBFS: f32 = -120.0;

/// The level at which a sample counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;

/// What one window of audio was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchFrame {
    /// The fundamental, in Hz — `None` when the frame is not voiced.
    pub hz: Option<f32>,
    /// The NSDF at the chosen peak, `0..=1`.
    pub clarity: f32,
    /// The window's level.
    pub rms_dbfs: f32,
    /// Whether the frame passed both gates and carries a pitch.
    pub voiced: bool,
    /// Whether any sample in the window hit the rails — the pitch may
    /// still be right, but the level is not, and a clipped signal
    /// grows harmonics that were never sung.
    pub clipped: bool,
}

impl PitchFrame {
    /// The silent frame.
    #[must_use]
    pub fn unvoiced(rms_dbfs: f32, clarity: f32, clipped: bool) -> PitchFrame {
        PitchFrame {
            hz: None,
            clarity,
            rms_dbfs,
            voiced: false,
            clipped,
        }
    }

    /// The pitch as fractional MIDI, when there is one.
    #[must_use]
    pub fn midi(&self) -> Option<f32> {
        self.hz.and_then(beatbyte_core::vocal::hz_to_midi)
    }
}

/// A reusable detector. Holds its scratch, so `detect` allocates
/// nothing: the realtime worker calls it every hop.
pub struct PitchDetector {
    config: PitchConfig,
    min_tau: usize,
    max_tau: usize,
    search_tau: usize,
    nsdf: Vec<f32>,
}

impl PitchDetector {
    /// A detector for `config`.
    ///
    /// The lag range is derived once from the frequency range; a
    /// configuration whose range is empty or inverted is clamped to
    /// something workable rather than panicking in an audio thread.
    #[must_use]
    pub fn new(config: PitchConfig) -> PitchDetector {
        let rate = config.sample_rate.max(1) as f32;
        let hi = config.max_hz.max(1.0);
        let lo = config.min_hz.max(1.0).min(hi);
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "both are positive lags derived from clamped positive rates"
        )]
        let min_tau = ((rate / hi).floor() as usize).max(2);
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_truncation,
            reason = "both are positive lags derived from clamped positive rates"
        )]
        let max_tau = ((rate / lo).ceil() as usize).max(min_tau + 2);
        // The hump around the lowest pitch's period runs out to
        // about 5/4 of it, and a hump only counts once it has closed.
        // Searching exactly to `max_tau` therefore loses the lowest
        // note in the range — a 220 Hz tone vanished from a
        // 180–500 Hz detector. Search wider; report inside.
        let search_tau = max_tau + max_tau / 4 + 2;
        PitchDetector {
            config,
            min_tau,
            max_tau,
            search_tau,
            nsdf: vec![0.0; search_tau + 1],
        }
    }

    /// The configuration it was built with.
    #[must_use]
    pub fn config(&self) -> PitchConfig {
        self.config
    }

    /// The shortest window that can carry the lowest pitch.
    ///
    /// The NSDF at lag τ compares `window - τ` samples; below about
    /// two periods there is not enough overlap for the normalisation
    /// to mean anything. Callers size their window from this.
    #[must_use]
    pub fn min_window(&self) -> usize {
        self.search_tau * 2
    }

    /// Estimate the pitch of one window.
    ///
    /// The window may be any length at least [`PitchDetector::min_window`];
    /// a shorter one returns an unvoiced frame rather than a number
    /// the overlap cannot support.
    pub fn detect(&mut self, window: &[f32]) -> PitchFrame {
        let (rms_dbfs, clipped) = level(window);
        if window.len() < self.min_window() || rms_dbfs < self.config.rms_gate_dbfs {
            return PitchFrame::unvoiced(rms_dbfs, 0.0, clipped);
        }
        self.fill_nsdf(window);
        let Some((tau, clarity)) = self.pick_peak() else {
            return PitchFrame::unvoiced(rms_dbfs, 0.0, clipped);
        };
        if clarity < self.config.clarity_threshold {
            return PitchFrame::unvoiced(rms_dbfs, clarity, clipped);
        }
        let hz = self.config.sample_rate as f32 / tau;
        PitchFrame {
            hz: Some(hz),
            clarity,
            rms_dbfs,
            voiced: true,
            clipped,
        }
    }

    /// n'(τ) from zero to the longest lag, into the preallocated
    /// scratch.
    ///
    /// It starts at **zero**, not at the shortest lag of interest,
    /// and that is not waste. `n'(0)` is 1 by definition and the
    /// curve descends from it; the peak picker has to see that
    /// descent to know where the first genuine hump begins. Starting
    /// at `min_tau` hid it whenever the range's short end fell inside
    /// the first hump — at 880 Hz it did, the hump was mistaken for
    /// the descent and skipped, and the detector reported 440.
    fn fill_nsdf(&mut self, window: &[f32]) {
        let n = window.len();
        for tau in 0..=self.search_tau {
            let overlap = n - tau;
            let mut correlation = 0.0f64;
            let mut energy = 0.0f64;
            for j in 0..overlap {
                let a = f64::from(window[j]);
                let b = f64::from(window[j + tau]);
                correlation += a * b;
                energy += a * a + b * b;
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "an NSDF value in [-1, 1]; f32 is the precision the caller uses"
            )]
            let value = if energy > 0.0 {
                (2.0 * correlation / energy) as f32
            } else {
                0.0
            };
            self.nsdf[tau] = value;
        }
    }

    /// The first key maximum, which is the period.
    fn pick_peak(&self) -> Option<(f32, f32)> {
        // Two passes over the humps rather than a list of crests:
        // this runs on the realtime worker every hop, and a Vec here
        // would be a heap allocation per pitch estimate.
        let tallest = self.walk_crests(None)?.1;
        if tallest <= 0.0 {
            return None;
        }
        // McLeod's key maximum: the EARLIEST crest that is nearly as
        // tall as the tallest. Taking the tallest outright is the
        // classic octave error — a voice with a strong second
        // harmonic correlates marginally better with itself half a
        // period along.
        let cutoff = tallest * self.config.peak_ratio.clamp(0.0, 1.0);
        let (index, value) = self.walk_crests(Some(cutoff))?;
        Some((self.interpolate(index), value.clamp(-1.0, 1.0)))
    }

    /// Walk the positive humps once.
    ///
    /// With `cutoff` it returns the FIRST crest reaching it; without,
    /// the tallest crest there is. Only a hump that closes inside the
    /// searched range counts — one still rising at the last lag has
    /// no proof it has peaked and no right-hand neighbour to
    /// interpolate against — and only a crest inside the configured
    /// pitch range is eligible.
    fn walk_crests(&self, cutoff: Option<f32>) -> Option<(usize, f32)> {
        // Step past the descent from n'(0) = 1: everything until the
        // curve first goes non-positive belongs to lag zero, not to a
        // period.
        let mut tau = 0;
        while tau <= self.search_tau && self.nsdf[tau] > 0.0 {
            tau += 1;
        }
        let mut best: Option<(usize, f32)> = None;
        while tau <= self.search_tau {
            if self.nsdf[tau] <= 0.0 {
                tau += 1;
                continue;
            }
            let mut crest = tau;
            while tau <= self.search_tau && self.nsdf[tau] > 0.0 {
                if self.nsdf[tau] > self.nsdf[crest] {
                    crest = tau;
                }
                tau += 1;
            }
            let closed = tau <= self.search_tau;
            if !closed || !(self.min_tau..=self.max_tau).contains(&crest) {
                continue;
            }
            let value = self.nsdf[crest];
            match cutoff {
                Some(bar) if value >= bar => return Some((crest, value)),
                Some(_) => {}
                None => {
                    if best.is_none_or(|(_, top)| value > top) {
                        best = Some((crest, value));
                    }
                }
            }
        }
        best
    }

    /// Parabolic vertex through the crest and its neighbours. At
    /// 16 kHz the lag grid is over a semitone wide at 1 kHz, so this
    /// is not a refinement — without it the top of the range is
    /// quantised past the tolerances the game judges on.
    fn interpolate(&self, index: usize) -> f32 {
        if index == 0 || index + 1 > self.search_tau {
            return index as f32;
        }
        let before = self.nsdf[index - 1];
        let here = self.nsdf[index];
        let after = self.nsdf[index + 1];
        // Curvature, negative at a maximum. The sign matters and is
        // easy to get backwards: with `before` the taller neighbour
        // the vertex lies EARLIER, so the shift must come out
        // negative. The first version here flipped it and every pitch
        // came back sharp — 11 cents at 110 Hz, 35 at 440.
        let curvature = before - 2.0 * here + after;
        if curvature.abs() < f32::EPSILON {
            return index as f32;
        }
        let shift = 0.5 * (before - after) / curvature;
        index as f32 + shift.clamp(-0.5, 0.5)
    }
}

/// A window's level in dBFS, and whether it hit the rails.
#[must_use]
pub fn level(window: &[f32]) -> (f32, bool) {
    if window.is_empty() {
        return (SILENCE_DBFS, false);
    }
    let mut sum = 0.0f64;
    let mut clipped = false;
    for &sample in window {
        sum += f64::from(sample) * f64::from(sample);
        clipped |= sample.abs() >= CLIP_LEVEL;
    }
    let rms = (sum / window.len() as f64).sqrt();
    if rms <= 0.0 {
        return (SILENCE_DBFS, clipped);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a level in dB; f32 is what the caller reports"
    )]
    let dbfs = (20.0 * rms.log10()) as f32;
    (dbfs.max(SILENCE_DBFS), clipped)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn detector() -> PitchDetector {
        PitchDetector::new(PitchConfig::default())
    }

    fn sine(hz: f32, n: usize, amplitude: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amplitude * (core::f32::consts::TAU * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    /// A vowel-ish tone: a fundamental with harmonics, the second one
    /// LOUDER than the first. This is the shape that makes a naive
    /// "tallest peak" detector read an octave high.
    fn voweled(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let w = core::f32::consts::TAU * hz * t;
                0.30 * w.sin() + 0.45 * (2.0 * w).sin() + 0.20 * (3.0 * w).sin()
            })
            .collect()
    }

    fn cents_off(got: f32, want: f32) -> f32 {
        1200.0 * (got / want).log2()
    }

    #[test]
    fn a_sine_reads_its_own_frequency_to_within_a_few_cents() {
        let mut d = detector();
        for hz in [82.41, 110.0, 220.0, 440.0, 523.25, 880.0] {
            let frame = d.detect(&sine(hz as f32, 2048, 0.4));
            let got = frame
                .hz
                .unwrap_or_else(|| panic!("{hz} Hz read as unvoiced"));
            let off = cents_off(got, hz as f32).abs();
            assert!(off < 5.0, "{hz} Hz read as {got} Hz ({off:.1} cents off)");
            assert!(frame.voiced);
            assert!(
                frame.clarity > 0.9,
                "a pure tone is clear: {}",
                frame.clarity
            );
        }
    }

    #[test]
    fn a_detuned_tone_is_reported_detuned_rather_than_snapped() {
        // The whole point of fractional pitch: 30 cents flat of A4 has
        // to come back as 30 cents flat, not as A4.
        let mut d = detector();
        let target = 440.0 * 2f32.powf(-30.0 / 1200.0);
        let got = d.detect(&sine(target, 2048, 0.4)).hz.unwrap();
        let off = cents_off(got, 440.0);
        assert!(
            (off + 30.0).abs() < 5.0,
            "expected about -30 cents, got {off:.1}"
        );
    }

    #[test]
    fn a_strong_second_harmonic_does_not_read_an_octave_high() {
        // The classic failure. McLeod's key-maximum rule exists for
        // exactly this signal, so it gets its own test.
        let mut d = detector();
        for hz in [110.0, 165.0, 220.0] {
            let got = d.detect(&voweled(hz, 2048)).hz.unwrap();
            let off = cents_off(got, hz).abs();
            assert!(
                off < 15.0,
                "{hz} Hz vowel read as {got} Hz ({off:.0} cents) — an octave would be 1200"
            );
        }
    }

    #[test]
    fn silence_and_a_quiet_room_are_not_singing() {
        let mut d = detector();
        let quiet = d.detect(&vec![0.0; 2048]);
        assert!(!quiet.voiced);
        assert_eq!(quiet.hz, None);
        assert_eq!(quiet.rms_dbfs, SILENCE_DBFS, "a floor, never -inf");

        // A tone well under the gate: periodic, and still not singing.
        let whisper = d.detect(&sine(220.0, 2048, 0.001));
        assert!(!whisper.voiced, "{} dBFS got through", whisper.rms_dbfs);
        assert_eq!(whisper.hz, None);
    }

    #[test]
    fn noise_is_not_given_a_pitch() {
        // Deterministic white-ish noise: a hashed LCG, so a failure
        // here is reproducible rather than a once-in-a-run surprise.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let noise: Vec<f32> = (0..4096)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ((state >> 33) as f32 / f32::from(u16::MAX) / 8.0) - 0.5
            })
            .collect();
        let mut d = detector();
        let frame = d.detect(&noise);
        assert!(
            !frame.voiced,
            "noise read as {:?} Hz at clarity {}",
            frame.hz, frame.clarity
        );
    }

    #[test]
    fn two_voices_at_once_are_refused_rather_than_averaged() {
        // The plan's requirement: ambiguous input must be rejected or
        // come back with low confidence. A fifth is the hard case —
        // it is consonant, so the sum is still fairly periodic.
        let mut d = detector();
        let a = sine(220.0, 4096, 0.35);
        let b = sine(330.0, 4096, 0.35);
        let both: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let frame = d.detect(&both);
        // Either unvoiced, or — if it does commit — at the common
        // fundamental, never at one of the two voices.
        if let Some(hz) = frame.hz {
            let off_low = cents_off(hz, 220.0).abs();
            let off_high = cents_off(hz, 330.0).abs();
            assert!(
                off_low > 100.0 && off_high > 100.0,
                "committed to one of two simultaneous voices: {hz} Hz"
            );
        }
    }

    #[test]
    fn vibrato_is_followed_rather_than_smoothed_away() {
        // 6 Hz, ±50 cents — an ordinary singer. Each window must read
        // near the instantaneous pitch, so the contour survives.
        let mut d = detector();
        let base = 440.0f32;
        let n = 2048;
        let mut seen_sharp = false;
        let mut seen_flat = false;
        for start_ms in [0u32, 40, 80, 120] {
            let window: Vec<f32> = (0..n)
                .map(|i| {
                    let t = (i as f32 / RATE as f32) + f32::from(start_ms as u16) / 1000.0;
                    // Integrate the frequency so the phase is continuous.
                    let dev = 50.0 / 1200.0;
                    let phase = core::f32::consts::TAU
                        * base
                        * (t + dev / (core::f32::consts::TAU * 6.0)
                            * (core::f32::consts::TAU * 6.0 * t).sin());
                    0.4 * phase.sin()
                })
                .collect();
            let hz = d.detect(&window).hz.unwrap();
            let off = cents_off(hz, base);
            assert!(off.abs() < 90.0, "{off} cents is not vibrato any more");
            seen_sharp |= off > 8.0;
            seen_flat |= off < -8.0;
        }
        assert!(
            seen_sharp && seen_flat,
            "the detector flattened the vibrato instead of following it"
        );
    }

    #[test]
    fn a_clipped_window_is_marked_even_when_its_pitch_is_right() {
        let mut d = detector();
        let hard: Vec<f32> = sine(220.0, 2048, 2.0)
            .iter()
            .map(|s| s.clamp(-1.0, 1.0))
            .collect();
        let frame = d.detect(&hard);
        assert!(frame.clipped, "a square-topped sine is clipped");
        // Still pitched — the level is what is wrong, not the note.
        assert!(cents_off(frame.hz.unwrap(), 220.0).abs() < 10.0);
        // And a clean one is not falsely marked.
        assert!(!d.detect(&sine(220.0, 2048, 0.4)).clipped);
    }

    #[test]
    fn a_window_too_short_for_the_lowest_pitch_says_so() {
        let mut d = detector();
        assert!(d.min_window() >= 2 * (RATE as usize / 65));
        let frame = d.detect(&sine(440.0, 64, 0.4));
        assert!(!frame.voiced, "64 samples cannot support a 65 Hz floor");
    }

    #[test]
    fn a_narrow_range_finds_what_is_in_it_and_nothing_outside() {
        // What the vocal-range setting does: look only where the
        // singer is, which is also what keeps a rumble out.
        let mut d = PitchDetector::new(PitchConfig {
            min_hz: 180.0,
            max_hz: 500.0,
            ..PitchConfig::default()
        });
        assert!(cents_off(d.detect(&sine(220.0, 2048, 0.4)).hz.unwrap(), 220.0).abs() < 5.0);
        // 80 Hz is below the range: whatever comes back, it is not 80.
        if let Some(hz) = d.detect(&sine(80.0, 2048, 0.4)).hz {
            assert!(hz > 150.0, "found {hz} Hz outside the configured range");
        }
    }

    #[test]
    fn an_impossible_configuration_is_clamped_rather_than_fatal() {
        // This runs in an audio worker; a panic there takes the stream
        // down. Inverted, zero and absurd ranges must all give a
        // detector that simply answers.
        for (min, max) in [(500.0, 100.0), (0.0, 0.0), (1.0, 100_000.0)] {
            let mut d = PitchDetector::new(PitchConfig {
                min_hz: min,
                max_hz: max,
                ..PitchConfig::default()
            });
            let _ = d.detect(&sine(220.0, 4096, 0.4));
        }
    }

    #[test]
    fn the_level_meter_reads_what_the_standard_says() {
        // A full-scale sine is -3.01 dBFS RMS.
        let (db, clipped) = level(&sine(1000.0, 4096, 1.0));
        assert!((db + 3.01).abs() < 0.1, "{db}");
        assert!(clipped, "amplitude 1.0 touches the rails");
        // Half amplitude is 6 dB down from that.
        let (half, _) = level(&sine(1000.0, 4096, 0.5));
        assert!((half - (db - 6.02)).abs() < 0.1, "{half} vs {db}");
        assert_eq!(level(&[]), (SILENCE_DBFS, false));
    }

    #[test]
    fn a_frame_converts_to_the_representation_the_game_scores_in() {
        let mut d = detector();
        let midi = d.detect(&sine(440.0, 2048, 0.4)).midi().unwrap();
        assert!((midi - 69.0).abs() < 0.05, "{midi}");
        assert_eq!(PitchFrame::unvoiced(-60.0, 0.1, false).midi(), None);
    }
}
