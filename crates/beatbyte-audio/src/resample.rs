//! Sample-rate conversion for the learned models, which want 16 kHz.
//!
//! A windowed-sinc interpolator: for every output sample, a Hann-
//! windowed sinc centred on the fractional input position, with the
//! cutoff below the lower Nyquist so downsampling does not alias.
//! Pure and deterministic — the same input gives the same output on
//! every run, which the alignment cache relies on. Not the fastest
//! way to do this, and it does not need to be: a four-minute song is
//! ~12 M multiply-adds per output sample-tap, well under a second.

use core::f64::consts::PI;

/// Taps on each side of the centre.
const HALF_TAPS: i64 = 32;

/// Resample `input` from `from_hz` to `to_hz`. Same rate: a copy.
/// A degenerate rate (0) returns an empty buffer rather than panicking.
#[must_use]
pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == 0 || to_hz == 0 || input.is_empty() {
        return Vec::new();
    }
    if from_hz == to_hz {
        return input.to_vec();
    }
    let ratio = f64::from(from_hz) / f64::from(to_hz);
    let n_out = (input.len() as f64 / ratio).round() as usize;
    // Cutoff in cycles per INPUT sample: 90 % of the lower Nyquist.
    let cutoff = 0.9 * f64::from(from_hz.min(to_hz)) / 2.0 / f64::from(from_hz);
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let centre = i as f64 * ratio;
        let anchor = centre.floor() as i64;
        let mut acc = 0.0f64;
        let mut norm = 0.0f64;
        for k in (anchor - HALF_TAPS)..=(anchor + HALF_TAPS) {
            if k < 0 || k as usize >= input.len() {
                continue;
            }
            let t = k as f64 - centre;
            let sinc = if t.abs() < 1e-9 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * t).sin() / (PI * t)
            };
            let window = 0.5 + 0.5 * (PI * t / (HALF_TAPS as f64 + 1.0)).cos();
            let weight = sinc * window;
            acc += weight * f64::from(input[k as usize]);
            norm += weight;
        }
        // Normalising by the window's own sum keeps unity gain at DC
        // for every fractional phase and at the edges.
        out.push(if norm.abs() > 1e-12 {
            (acc / norm) as f32
        } else {
            0.0
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resample;

    fn sine(hz: f64, rate: u32, seconds: f64) -> Vec<f32> {
        (0..(f64::from(rate) * seconds) as usize)
            .map(|i| (2.0 * core::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    /// Amplitude of `hz` in `x` at `rate`, by correlation.
    fn amplitude(x: &[f32], hz: f64, rate: u32) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let phase = 2.0 * core::f64::consts::PI * hz * i as f64 / f64::from(rate);
            re += f64::from(v) * phase.cos();
            im += f64::from(v) * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn the_length_follows_the_ratio_exactly() {
        assert_eq!(resample(&[0.0; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&[0.0; 44_100], 44_100, 16_000).len(), 16_000);
        assert_eq!(resample(&[0.0; 16_000], 16_000, 48_000).len(), 48_000);
        assert_eq!(resample(&[1.0, 2.0], 16_000, 16_000), vec![1.0, 2.0]);
        assert!(resample(&[1.0], 0, 16_000).is_empty());
    }

    #[test]
    fn a_tone_below_the_new_nyquist_survives_with_its_amplitude() {
        for from in [44_100u32, 48_000] {
            let x = sine(1_000.0, from, 1.0);
            let y = resample(&x, from, 16_000);
            let a = amplitude(&y[800..15_200], 1_000.0, 16_000);
            assert!(
                (a - 1.0).abs() < 0.02,
                "{from} Hz → 16 kHz: 1 kHz came out at {a}"
            );
        }
    }

    #[test]
    fn a_tone_above_the_new_nyquist_is_removed_not_folded() {
        // 12 kHz at 48 kHz would alias to 4 kHz at 16 kHz without a
        // low-pass. It must vanish instead.
        let x = sine(12_000.0, 48_000, 1.0);
        let y = resample(&x, 48_000, 16_000);
        let leak = amplitude(&y[800..15_200], 4_000.0, 16_000);
        assert!(leak < 0.01, "aliased energy at 4 kHz: {leak}");
        // Away from the first and last taps, where the truncated
        // window rejects little (the song's first and last 2 ms —
        // nothing a model window ever starts on), the tone is gone.
        let peak = y[64..y.len() - 64]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.05, "residual peak {peak}");
    }

    #[test]
    fn dc_passes_at_unity_and_the_result_is_deterministic() {
        let x = vec![0.5f32; 4_800];
        let y = resample(&x, 48_000, 16_000);
        for (i, v) in y.iter().enumerate().skip(40).take(y.len() - 80) {
            assert!((v - 0.5).abs() < 1e-3, "sample {i}: {v}");
        }
        assert_eq!(resample(&x, 48_000, 16_000), y);
    }
}
