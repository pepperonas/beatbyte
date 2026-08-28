//! Deterministic signal synthesis.
//!
//! Used by the analysis test-suite (known ground truth in, tolerances
//! asserted out) and for generating original demo material — BeatByte
//! ships no copyrighted audio, so anything bundled is synthesized.

use crate::decode::AudioData;

/// A percussive click: a short decaying noise-free burst with a sharp
/// attack, placed at each of the given times.
#[must_use]
pub fn click_track(times_s: &[f64], duration_s: f64, sample_rate: u32) -> AudioData {
    click_track_with_gain(times_s, duration_s, sample_rate, 0.9)
}

/// [`click_track`] with an explicit gain per click.
#[must_use]
pub fn click_track_with_gain(
    times_s: &[f64],
    duration_s: f64,
    sample_rate: u32,
    gain: f32,
) -> AudioData {
    let rate = f64::from(sample_rate);
    let mut samples = vec![0.0f32; (duration_s * rate) as usize];
    for &time in times_s {
        add_burst(&mut samples, sample_rate, time, 1_000.0, 0.03, gain);
        // A touch of low body makes it kick-like rather than a pure tick.
        add_burst(&mut samples, sample_rate, time, 120.0, 0.05, gain * 0.8);
    }
    AudioData::from_mono(samples, sample_rate)
}

/// A single decaying tone burst at `time_s` inside a silent buffer.
#[must_use]
pub fn tone_burst(
    time_s: f64,
    frequency_hz: f64,
    burst_s: f64,
    duration_s: f64,
    sample_rate: u32,
) -> AudioData {
    let rate = f64::from(sample_rate);
    let mut samples = vec![0.0f32; (duration_s * rate) as usize];
    add_burst(
        &mut samples,
        sample_rate,
        time_s,
        frequency_hz,
        burst_s,
        0.9,
    );
    AudioData::from_mono(samples, sample_rate)
}

/// Add a decaying sine burst into an existing buffer.
pub fn add_burst(
    samples: &mut [f32],
    sample_rate: u32,
    time_s: f64,
    frequency_hz: f64,
    burst_s: f64,
    gain: f32,
) {
    let rate = f64::from(sample_rate);
    let start = (time_s * rate) as usize;
    let length = (burst_s * rate) as usize;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / rate;
        // Sharp attack, exponential decay…
        let envelope = (-t / (burst_s * 0.3)).exp() as f32;
        // …and a smooth release ramp over the final 15% — a hard
        // truncation is itself an audible click that onset detectors
        // (correctly!) report as an event.
        let remaining = (length - i) as f32 / (0.15 * length as f32);
        let release = remaining.min(1.0);
        let phase = 2.0 * core::f64::consts::PI * frequency_hz * t;
        *slot += gain * envelope * release * phase.sin() as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_track_is_silent_between_clicks() {
        let audio = click_track(&[1.0], 2.0, 22_050);
        let samples = audio.samples();
        // Well before the click: silence.
        assert_eq!(samples[1000], 0.0);
        // At the click: energy.
        let at = (1.001 * 22_050.0) as usize;
        assert!(samples[at].abs() > 0.01);
    }

    #[test]
    fn bursts_never_write_out_of_bounds() {
        let mut samples = vec![0.0f32; 100];
        add_burst(&mut samples, 22_050, 0.004, 440.0, 1.0, 1.0);
        // Burst extends past the buffer end — must simply stop.
        assert!(samples.iter().all(|s| s.is_finite()));
    }
}

/// The share of a voice's length spent ramping to silence.
const RELEASE_FRACTION: f64 = 0.18;

/// The recipe for one error sound: a pick landing on damped strings.
///
/// Both of BeatByte's error sounds are built from this one voice so
/// they read as siblings — the same instrument making the same kind of
/// unwanted noise — while still being told apart. A missed note and a
/// stray strum are different mistakes: one is a note that never
/// sounded, the other is a noise you made that should not exist.
///
/// The voice is two layers. A resonant band-pass on white noise gives
/// the *clank* of a pick on muted strings; a pulse oscillator under it
/// gives the sound just enough pitch to have a character. Both share
/// one envelope, so the result is a single event rather than a chord.
#[derive(Debug, Clone, Copy)]
pub struct ErrorVoice {
    /// Total length in seconds. These are interruptions, not notes:
    /// they have to survive being fired several times a second.
    pub length_s: f64,
    /// Band-pass centre for the string noise, in hertz. This is the
    /// single biggest lever on how bright the sound reads.
    pub clank_hz: f64,
    /// Band-pass resonance. Higher is more pitched, less airy.
    pub clank_q: f64,
    /// Level of the noise layer.
    pub clank_gain: f32,
    /// Pitched partials sounding under the clank, in hertz. More than
    /// one makes an interval — a dissonant one reads as "wrong".
    pub tones: &'static [f64],
    /// Pitch multiplier reached at the end of the sound. `1.0` holds
    /// pitch; below `1.0` the tone sags, which is what a note failing
    /// to sound does.
    pub bend: f64,
    /// Pulse width of the tone layer. `0.5` is a square; narrower is
    /// thinner and buzzier.
    pub duty: f64,
    /// Level of the tone layer.
    pub tone_gain: f32,
    /// Exponential decay constant in seconds.
    pub decay_s: f64,
    /// Peak the finished sound is normalised to.
    ///
    /// The layer gains set the *balance* between clank and tone; this
    /// sets the loudness. They have to be separate, because a
    /// band-pass on white noise passes less energy the lower and
    /// narrower its band is — a dark voice is quiet for reasons that
    /// have nothing to do with how loud it should be.
    pub peak: f32,
    /// Seed for the noise layer. Fixed per voice, so a given voice is
    /// bit-identical on every run and every machine.
    pub seed: u64,
}

impl ErrorVoice {
    /// Render the voice into mono audio at `sample_rate`.
    ///
    /// Deterministic: the noise comes from a seeded xorshift, so the
    /// same voice renders the same samples every time.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn render(&self, sample_rate: u32) -> AudioData {
        let rate = f64::from(sample_rate);
        let length = (self.length_s * rate) as usize;
        let mut samples = vec![0.0f32; length.max(1)];

        // Chamberlin state-variable filter. Stable while the centre
        // stays well under a quarter of the sample rate, which every
        // sensible clank does.
        let f = 2.0 * (core::f64::consts::PI * self.clank_hz / rate).sin();
        let q = 1.0 / self.clank_q.max(0.5);
        let (mut low, mut band) = (0.0f64, 0.0f64);

        let mut noise = self.seed | 1;
        let mut phases = vec![0.0f64; self.tones.len()];

        for (i, slot) in samples.iter_mut().enumerate() {
            let t = i as f64 / rate;
            let progress = t / self.length_s;

            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            let white = ((noise >> 40) as f64) / 8_388_608.0 - 1.0;

            let high = white - low - q * band;
            band += f * high;
            low += f * band;
            let clank = band as f32 * self.clank_gain;

            // The bend is applied as a continuous glide rather than a
            // jump, so the tone sags the way a string does.
            let slide = self.bend.powf(progress);
            let mut voiced = 0.0f32;
            for (phase, &tone_hz) in phases.iter_mut().zip(self.tones) {
                *phase += tone_hz * slide / rate;
                // Zero-mean pulse. The naive +1/-1 form carries a DC
                // offset of `1 - 2*duty`, which at the narrow duties
                // that make a buzz is most of the signal: measured on
                // the tritone voice, the two strongest components of
                // the finished sound were 16 Hz and 32 Hz - inaudible
                // energy eating the headroom the audible part needs.
                let pulse = if phase.fract() < self.duty {
                    1.0 - self.duty
                } else {
                    -self.duty
                };
                voiced += pulse as f32;
            }
            if !self.tones.is_empty() {
                voiced *= self.tone_gain / self.tones.len() as f32;
            }

            // Instant-but-not-clicking attack, exponential decay,
            // and a short ramp to true silence at the end. Without
            // that ramp the buffer stops while the sound is still
            // audible, and the step to zero is a click with energy
            // right across the spectrum — measured: it dragged every
            // voice's brightness up to a near-identical 4.5 kHz,
            // erasing the very difference the two voices exist for.
            let attack = 1.0 - (-t / 0.0012).exp();
            let release = ((1.0 - progress) / RELEASE_FRACTION).clamp(0.0, 1.0);
            let envelope = (attack * (-t / self.decay_s).exp() * release) as f32;

            *slot = (clank + voiced) * envelope;
        }

        let loudest = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        if loudest > f32::EPSILON {
            let scale = self.peak / loudest;
            for slot in &mut samples {
                *slot = (*slot * scale).clamp(-1.0, 1.0);
            }
        }

        AudioData::from_mono(samples, sample_rate)
    }
}

// The two facts that make these two voices *two* voices, asserted at
// compile time because they are properties of the constants
// themselves: the miss sags in pitch and the stray strum does not, and
// neither outlasts the 120 ms gate that stops a bad passage becoming a
// drone. A runtime test could not fail without the build failing first.
const _: () = assert!(MISS_VOICE.bend < 0.8, "a missed note should sag");
const _: () = assert!(OVERSTRUM_VOICE.bend > 0.9, "a stray strum holds pitch");
const _: () = assert!(MISS_VOICE.length_s <= 0.12, "too long to repeat");
const _: () = assert!(OVERSTRUM_VOICE.length_s <= 0.12, "too long to repeat");

/// A missed note: the sound of a note that never sounded.
///
/// Dark and falling. The clank sits low, and the tone under it sags a
/// fifth over the length of the sound, which is what deflation sounds
/// like. Slightly longer than [`OVERSTRUM_VOICE`] because absence is a
/// duller event than intrusion.
pub const MISS_VOICE: ErrorVoice = ErrorVoice {
    length_s: 0.085,
    clank_hz: 230.0,
    clank_q: 2.2,
    clank_gain: 0.42,
    tones: &[196.00], // G3
    bend: 0.62,
    duty: 0.5,
    tone_gain: 0.30,
    decay_s: 0.028,
    peak: 0.34,
    seed: 0x9E37_79B9_7F4A_7C15,
};

/// A stray strum: the sound of a noise you made that should not exist.
///
/// Brighter, tighter and deliberately dissonant — the tone layer is a
/// tritone (B♭3 against E4), the interval that reads as "wrong" without
/// having to be loud, played through a thin pulse so it buzzes.
pub const OVERSTRUM_VOICE: ErrorVoice = ErrorVoice {
    length_s: 0.062,
    clank_hz: 1_150.0,
    clank_q: 1.4,
    clank_gain: 0.34,
    tones: &[233.08, 329.63], // B♭3 + E4
    bend: 0.94,
    duty: 0.18,
    tone_gain: 0.26,
    decay_s: 0.020,
    peak: 0.34,
    seed: 0x2545_F491_4F6C_DD1D,
};

#[cfg(test)]
mod error_voice_tests {
    use super::{ErrorVoice, MISS_VOICE, OVERSTRUM_VOICE};

    const RATE: u32 = 44_100;

    /// The share of spectral magnitude below `cutoff`, and the
    /// strongest partial. A short noisy sound has no meaningful
    /// amplitude-weighted centroid — the broadband floor swamps it —
    /// so brightness is measured as a share instead.
    fn spectrum(voice: &ErrorVoice, cutoff: f64) -> (f64, f64) {
        let audio = voice.render(RATE);
        let samples = audio.samples();
        let n = samples.len();
        let rate = f64::from(RATE);
        let (mut best, mut best_hz) = (0.0, 0.0);
        let (mut low, mut total) = (0.0, 0.0);
        // Every fourth bin: enough resolution for a share, and it
        // keeps this naive transform inside a test's patience.
        for k in (1..n / 2).step_by(4) {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &s) in samples.iter().enumerate() {
                let angle = -2.0 * core::f64::consts::PI * (k * i) as f64 / n as f64;
                re += f64::from(s) * angle.cos();
                im += f64::from(s) * angle.sin();
            }
            let magnitude = re.hypot(im);
            let hz = k as f64 * rate / n as f64;
            if magnitude > best {
                best = magnitude;
                best_hz = hz;
            }
            if hz < cutoff {
                low += magnitude;
            }
            total += magnitude;
        }
        (best_hz, low / total)
    }

    #[test]
    fn a_voice_renders_the_length_it_asks_for() {
        let audio = MISS_VOICE.render(RATE);
        let expected = (MISS_VOICE.length_s * f64::from(RATE)) as usize;
        assert_eq!(audio.samples().len(), expected);
        assert_eq!(audio.sample_rate(), RATE);
    }

    #[test]
    fn a_voice_is_normalised_to_its_peak() {
        for voice in [MISS_VOICE, OVERSTRUM_VOICE] {
            let audio = voice.render(RATE);
            let peak = audio.samples().iter().fold(0.0f32, |a, s| a.max(s.abs()));
            assert!(
                (peak - voice.peak).abs() < 0.01,
                "peak {peak} should be {}",
                voice.peak
            );
        }
    }

    #[test]
    fn the_two_error_sounds_are_equally_loud() {
        // They mark mistakes of equal weight. If one were louder it
        // would read as the worse mistake, which is not true.
        let loudest = |voice: ErrorVoice| {
            voice
                .render(RATE)
                .samples()
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()))
        };
        let difference = (loudest(MISS_VOICE) - loudest(OVERSTRUM_VOICE)).abs();
        assert!(difference < 0.01, "loudness differs by {difference}");
    }

    #[test]
    fn a_voice_ends_in_silence() {
        // Without the release ramp the buffer stops while the sound is
        // still audible, and that step to zero is a click.
        for voice in [MISS_VOICE, OVERSTRUM_VOICE] {
            let audio = voice.render(RATE);
            let last = *audio.samples().last().expect("non-empty");
            assert!(last.abs() < 0.01, "voice ends at {last}, not silence");
        }
    }

    #[test]
    fn a_voice_carries_no_dc_offset() {
        // A narrow pulse is mostly DC unless it is centred, and DC is
        // inaudible energy stealing headroom from the audible part.
        for voice in [MISS_VOICE, OVERSTRUM_VOICE] {
            let audio = voice.render(RATE);
            let samples = audio.samples();
            let mean = samples.iter().map(|s| f64::from(*s)).sum::<f64>() / samples.len() as f64;
            assert!(mean.abs() < 0.02, "mean sample is {mean}, not centred");
        }
    }

    #[test]
    fn the_missed_note_is_darker_than_the_stray_strum() {
        // This is the whole point of there being two: one has to read
        // as dull and swallowed, the other as bright and wrong.
        let (_, miss_low) = spectrum(&MISS_VOICE, 1_000.0);
        let (_, over_low) = spectrum(&OVERSTRUM_VOICE, 1_000.0);
        assert!(
            miss_low > over_low + 0.10,
            "miss puts {:.0}% below 1 kHz, overstrum {:.0}% - too close to tell apart",
            miss_low * 100.0,
            over_low * 100.0
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(
            MISS_VOICE.render(RATE).samples(),
            MISS_VOICE.render(RATE).samples()
        );
    }

    #[test]
    fn every_sample_stays_in_range() {
        for voice in [MISS_VOICE, OVERSTRUM_VOICE] {
            assert!(
                voice.render(RATE).samples().iter().all(|s| s.abs() <= 1.0),
                "a sample left [-1, 1]"
            );
        }
    }
}
