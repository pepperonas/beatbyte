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
