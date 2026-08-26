//! Deterministic instrument synthesis for the evaluation scenes.
//!
//! These are not meant to sound convincing — they are meant to carry
//! the *analysis-relevant* properties of real instruments, so that a
//! metric measured here means something about real music:
//!
//! - a plucked string has a sharp attack, a rich harmonic series and
//!   a long decay (that is what makes it a sustain candidate);
//! - a kick is a fast downward pitch sweep with almost no harmonics
//!   (broadband transient, no stable pitch);
//! - a snare is noise plus a body tone (broadband, no stable pitch);
//! - a voice has vibrato and a slow attack (the thing that fools a
//!   naive pitch tracker into hundreds of tiny notes);
//! - a bass sits an octave or two below the lead and is *louder* in
//!   the spectrum than the lead almost everywhere.
//!
//! All noise comes from a fixed-seed generator, so every scene is
//! byte-identical on every run and on every machine.

use core::f64::consts::TAU;

/// A deterministic uniform noise source (SplitMix64 → f32 in −1..1).
/// Seeded per call site so scenes never depend on evaluation order.
pub struct Noise(u64);

impl Noise {
    /// Create a generator with an explicit seed.
    #[must_use]
    pub fn new(seed: u64) -> Noise {
        Noise(seed)
    }

    /// Next sample in −1.0..1.0.
    pub fn sample(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // Top 24 bits → [0,1) → [−1,1). 2^24 exactly: dividing by
        // anything else silently scales the noise (the first version
        // divided by 4096 and produced samples over 600).
        let unit = ((z >> 40) as f32) / 16_777_216.0;
        unit.mul_add(2.0, -1.0)
    }
}

/// Add a plucked, harmonically rich tone — the "guitar" of the suite.
///
/// `hold_s` is how long the note rings; the harmonic amplitudes fall
/// as 1/n and the higher partials decay faster than the fundamental,
/// which is what real strings do and what makes brightness drop over
/// a held note.
pub fn add_pluck(
    samples: &mut [f32],
    rate: u32,
    time_s: f64,
    freq_hz: f64,
    hold_s: f64,
    gain: f32,
) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (hold_s * sr) as usize;
    const ATTACK_S: f64 = 0.004;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let attack = (t / ATTACK_S).min(1.0);
        // Release over the final 8% so the note END is not itself a
        // transient the onset detector would (correctly) report.
        let release = (((length - i) as f64) / (0.08 * length as f64)).min(1.0);
        let mut value = 0.0f64;
        for harmonic in 1..=6u32 {
            let h = f64::from(harmonic);
            // Higher partials die faster: τ shrinks with harmonic.
            let decay = (-t / (hold_s * 0.9 / h)).exp();
            value += (TAU * h * freq_hz * t).sin() * decay / h;
        }
        *slot += gain * (attack * release * value * 0.55) as f32;
    }
}

/// Add a sustained bowed/organ-like tone: no decay, flat while held.
/// Used where "the tone is genuinely held" must be unambiguous.
pub fn add_sustained(
    samples: &mut [f32],
    rate: u32,
    time_s: f64,
    freq_hz: f64,
    hold_s: f64,
    gain: f32,
) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (hold_s * sr) as usize;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let attack = (t / 0.01).min(1.0);
        let release = (((length - i) as f64) / (0.02 * sr)).min(1.0);
        let mut value = 0.0f64;
        for harmonic in 1..=5u32 {
            let h = f64::from(harmonic);
            value += (TAU * h * freq_hz * t).sin() / h;
        }
        *slot += gain * (attack * release * value * 0.5) as f32;
    }
}

/// Add a voice-like tone: slow attack, 5 Hz vibrato of ±40 cents, and
/// a slight scoop into pitch. This is the signal that turns a naive
/// frame-wise pitch tracker into note confetti.
pub fn add_voice(
    samples: &mut [f32],
    rate: u32,
    time_s: f64,
    freq_hz: f64,
    hold_s: f64,
    gain: f32,
) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (hold_s * sr) as usize;
    let mut phase = 0.0f64;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let attack = (t / 0.06).min(1.0);
        let release = (((length - i) as f64) / (0.05 * sr)).min(1.0);
        // Scoop up into the target over the first 80 ms, then vibrato.
        let scoop = -0.6 * (-t / 0.03).exp();
        let vibrato = 0.04 * (TAU * 5.0 * t).sin() * (t / 0.15).min(1.0);
        let semitones = scoop + vibrato;
        let f = freq_hz * 2f64.powf(semitones / 12.0);
        phase += TAU * f / sr;
        let mut value = phase.sin();
        // A couple of formant-ish partials.
        value += 0.35 * (2.0 * phase).sin() + 0.2 * (3.0 * phase).sin();
        *slot += gain * (attack * release * value * 0.5) as f32;
    }
}

/// Add a kick drum: a fast downward pitch sweep, no stable pitch.
pub fn add_kick(samples: &mut [f32], rate: u32, time_s: f64, gain: f32) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (0.16 * sr) as usize;
    let mut phase = 0.0f64;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let f = 55.0 + 110.0 * (-t / 0.02).exp();
        phase += TAU * f / sr;
        let envelope = (-t / 0.05).exp();
        *slot += gain * (envelope * phase.sin()) as f32;
    }
}

/// Add a snare: noise plus a short body tone.
pub fn add_snare(samples: &mut [f32], rate: u32, time_s: f64, gain: f32, noise: &mut Noise) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (0.13 * sr) as usize;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let envelope = (-t / 0.035).exp() as f32;
        let body = (TAU * 190.0 * t).sin() as f32 * 0.4;
        *slot += gain * envelope * (noise.sample() * 0.8 + body);
    }
}

/// Add a hi-hat: very short, very bright noise.
pub fn add_hat(samples: &mut [f32], rate: u32, time_s: f64, gain: f32, noise: &mut Noise) {
    let sr = f64::from(rate);
    let start = (time_s * sr) as usize;
    let length = (0.045 * sr) as usize;
    let mut previous = 0.0f32;
    for i in 0..length {
        let Some(slot) = samples.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / sr;
        let envelope = (-t / 0.012).exp() as f32;
        // One-pole high-pass: hats live above everything else.
        let raw = noise.sample();
        let high = raw - previous;
        previous = raw;
        *slot += gain * envelope * high;
    }
}

/// MIDI note number → frequency in Hz.
#[must_use]
pub fn midi_hz(midi: f32) -> f64 {
    440.0 * 2f64.powf((f64::from(midi) - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a: Vec<f32> = (0..64).map(|_| Noise::new(7).sample()).collect();
        let mut source = Noise::new(7);
        let b: Vec<f32> = (0..64).map(|_| source.sample()).collect();
        assert_eq!(a[0], b[0], "same seed must start identically");
        let mut source = Noise::new(11);
        for _ in 0..10_000 {
            let v = source.sample();
            assert!((-1.0..=1.0).contains(&v), "noise out of range: {v}");
        }
    }

    #[test]
    fn noise_actually_varies() {
        let mut source = Noise::new(3);
        let values: Vec<f32> = (0..256).map(|_| source.sample()).collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let spread = values.iter().map(|v| (v - mean).abs()).sum::<f32>() / values.len() as f32;
        assert!(spread > 0.2, "noise is too flat: spread {spread}");
    }

    #[test]
    fn midi_maps_to_the_reference_pitch() {
        assert!((midi_hz(69.0) - 440.0).abs() < 1e-9);
        assert!((midi_hz(81.0) - 880.0).abs() < 1e-6, "an octave up");
        assert!((midi_hz(40.0) - 82.4069).abs() < 0.01, "guitar low E");
    }

    #[test]
    fn generators_never_write_out_of_bounds() {
        let mut samples = vec![0.0f32; 64];
        let mut noise = Noise::new(1);
        add_pluck(&mut samples, 22_050, 0.9, 440.0, 1.0, 0.5);
        add_sustained(&mut samples, 22_050, 0.9, 440.0, 1.0, 0.5);
        add_voice(&mut samples, 22_050, 0.9, 440.0, 1.0, 0.5);
        add_kick(&mut samples, 22_050, 0.9, 0.5);
        add_snare(&mut samples, 22_050, 0.9, 0.5, &mut noise);
        add_hat(&mut samples, 22_050, 0.9, 0.5, &mut noise);
        assert!(samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn a_pluck_decays_but_a_sustained_tone_does_not() {
        let rate = 22_050;
        let mut plucked = vec![0.0f32; rate as usize];
        let mut held = vec![0.0f32; rate as usize];
        add_pluck(&mut plucked, rate, 0.0, 220.0, 0.9, 1.0);
        add_sustained(&mut held, rate, 0.0, 220.0, 0.9, 1.0);
        let level = |buf: &[f32], at: f64| -> f32 {
            let start = (at * f64::from(rate)) as usize;
            buf[start..start + 512].iter().map(|s| s.abs()).sum::<f32>() / 512.0
        };
        assert!(
            level(&plucked, 0.7) < level(&plucked, 0.05) * 0.5,
            "a pluck must lose at least half its level"
        );
        assert!(
            level(&held, 0.7) > level(&held, 0.05) * 0.7,
            "a sustained tone must keep its level"
        );
    }
}
