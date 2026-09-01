//! Onset detection via spectral flux.
//!
//! STFT → log-compressed magnitude spectra → half-wave-rectified flux →
//! adaptive median threshold → local-maximum peak picking. A classic,
//! well-understood pipeline implemented from scratch; every stage is a
//! pure function over sample buffers.

use beatbyte_core::music::Onset;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::decode::AudioData;

/// Configuration for onset detection.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OnsetConfig {
    /// STFT window size in samples (power of two).
    pub window: usize,
    /// Hop between frames in samples.
    pub hop: usize,
    /// Frames on each side of the adaptive-threshold median window.
    pub median_halfwidth: usize,
    /// Multiplier on the local median to form the threshold.
    pub threshold_scale: f32,
    /// Absolute floor added to the threshold (rejects silence noise).
    pub threshold_floor: f32,
    /// Minimum spacing between onsets in seconds.
    pub min_gap_s: f64,
    /// Lowest frequency counted into the kick channel, in Hz.
    pub low_band_from_hz: f32,
    /// Highest frequency counted into the kick channel, in Hz.
    pub low_band_to_hz: f32,
}

impl Default for OnsetConfig {
    fn default() -> Self {
        // Tuned for ~22 kHz analysis rate: 46 ms windows resolve the
        // spectrum, 11.6 ms hops resolve timing.
        OnsetConfig {
            window: 1024,
            hop: 256,
            median_halfwidth: 8,
            threshold_scale: 1.3,
            threshold_floor: 0.02,
            min_gap_s: 0.05,
            // A kick's fundamental and its first harmonic. Deliberately
            // narrow: the point of this channel is that an offbeat open
            // hat at 6 kHz cannot reach it, which is what breaks the
            // offbeat tie on four-to-the-floor material.
            low_band_from_hz: 30.0,
            low_band_to_hz: 130.0,
        }
    }
}

/// Frame-level analysis products shared by onset and tempo stages.
#[derive(Debug, Clone, PartialEq)]
pub struct FluxAnalysis {
    /// Spectral flux per frame, normalized to 0.0–1.0.
    pub flux: Vec<f32>,
    /// Spectral flux restricted to the kick band, normalised the same
    /// way. The broadband curve above is dominated by whatever is
    /// loudest and busiest, which on loop house is the hi-hat layer —
    /// half of which sits deliberately off the beat. This channel
    /// hears the kick and little else.
    pub flux_low: Vec<f32>,
    /// Spectral centroid per frame, 0.0–1.0 (0 = bassy, 1 = bright).
    pub brightness: Vec<f32>,
    /// Seconds between frames.
    pub hop_s: f64,
    /// Seconds from a frame index to the musical time it represents
    /// (compensates the window's look-ahead).
    pub frame_offset_s: f64,
    /// The detected onsets, ascending.
    pub onsets: Vec<Onset>,
}

/// Run onset detection over decoded audio.
#[must_use]
pub fn analyze_onsets(audio: &AudioData, config: &OnsetConfig) -> FluxAnalysis {
    let samples = audio.samples();
    let rate = f64::from(audio.sample_rate());
    let hop_s = config.hop as f64 / rate;
    // A transient is detected by the frame whose *fresh* hop of samples
    // contains it; that hop starts `window − hop` into the frame.
    let frame_offset_s = (config.window.saturating_sub(config.hop)) as f64 / rate;

    if samples.len() < config.window + config.hop {
        return FluxAnalysis {
            flux: Vec::new(),
            flux_low: Vec::new(),
            brightness: Vec::new(),
            hop_s,
            frame_offset_s,
            onsets: Vec::new(),
        };
    }

    let frames = (samples.len() - config.window) / config.hop + 1;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(config.window);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let hann: Vec<f32> = (0..config.window)
        .map(|i| {
            let x = i as f32 / config.window as f32;
            0.5 - 0.5 * (2.0 * core::f32::consts::PI * x).cos()
        })
        .collect();

    let bins = spectrum.len();
    let mut previous = vec![0.0f32; bins];
    let mut compressed = vec![0.0f32; bins];
    let mut flux = Vec::with_capacity(frames);
    let mut flux_low = Vec::with_capacity(frames);
    let mut brightness = Vec::with_capacity(frames);

    // Bin indices for the kick band at THIS rate, so halving the
    // analysis rate cannot silently move the band.
    let bin_hz = rate as f32 / config.window as f32;
    let low_from = ((config.low_band_from_hz / bin_hz).floor() as usize).max(1);
    let low_to = ((config.low_band_to_hz / bin_hz).ceil() as usize).min(bins);

    for frame in 0..frames {
        let start = frame * config.hop;
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples[start + i] * hann[i];
        }
        // realfft only fails on wrong buffer lengths, which are fixed here.
        if fft.process(&mut input, &mut spectrum).is_err() {
            break;
        }

        let mut frame_flux = 0.0f32;
        let mut frame_low = 0.0f32;
        let mut centroid_num = 0.0f32;
        let mut centroid_den = 0.0f32;
        for (k, value) in spectrum.iter().enumerate() {
            let magnitude = value.norm();
            let comp = (1.0 + 50.0 * magnitude).ln();
            let rise = (comp - previous[k]).max(0.0);
            frame_flux += rise;
            if k >= low_from && k < low_to {
                frame_low += rise;
            }
            compressed[k] = comp;
            centroid_num += k as f32 * magnitude;
            centroid_den += magnitude;
        }
        core::mem::swap(&mut previous, &mut compressed);

        flux.push(if frame == 0 { 0.0 } else { frame_flux });
        flux_low.push(if frame == 0 { 0.0 } else { frame_low });
        brightness.push(if centroid_den > 1e-9 {
            (centroid_num / centroid_den) / bins as f32
        } else {
            0.0
        });
    }

    normalize(&mut flux);
    normalize(&mut flux_low);
    let onsets = pick_onsets(&flux, &brightness, hop_s, frame_offset_s, config);

    FluxAnalysis {
        flux,
        flux_low,
        brightness,
        hop_s,
        frame_offset_s,
        onsets,
    }
}

/// Scale a signal so its maximum is 1.0 (no-op on silence).
fn normalize(signal: &mut [f32]) {
    let max = signal.iter().copied().fold(0.0f32, f32::max);
    if max > 1e-9 {
        for value in signal.iter_mut() {
            *value /= max;
        }
    }
}

/// Adaptive-threshold local-maximum peak picking.
fn pick_onsets(
    flux: &[f32],
    brightness: &[f32],
    hop_s: f64,
    frame_offset_s: f64,
    config: &OnsetConfig,
) -> Vec<Onset> {
    let mut onsets: Vec<Onset> = Vec::new();
    let mut median_buf = Vec::with_capacity(config.median_halfwidth * 2 + 1);

    for t in 1..flux.len().saturating_sub(1) {
        // Local maximum first (cheap reject).
        if flux[t] < flux[t - 1] || flux[t] <= flux[t + 1] {
            continue;
        }
        // Adaptive threshold from the local median.
        let lo = t.saturating_sub(config.median_halfwidth);
        let hi = (t + config.median_halfwidth + 1).min(flux.len());
        median_buf.clear();
        median_buf.extend_from_slice(&flux[lo..hi]);
        median_buf.sort_by(f32::total_cmp);
        let median = median_buf[median_buf.len() / 2];
        let threshold = config.threshold_scale * median + config.threshold_floor;
        if flux[t] <= threshold {
            continue;
        }

        let time_s = t as f64 * hop_s + frame_offset_s;
        if let Some(last) = onsets.last()
            && time_s - last.time_s < config.min_gap_s
        {
            continue;
        }
        onsets.push(Onset {
            time_s,
            strength: flux[t],
            brightness: brightness.get(t).copied().unwrap_or(0.0),
        });
    }
    onsets
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::synth as testsignal;

    #[test]
    fn silence_yields_no_onsets() {
        let audio = AudioData::from_mono(vec![0.0; 44_100], 22_050);
        let result = analyze_onsets(&audio, &OnsetConfig::default());
        assert!(result.onsets.is_empty());
    }

    #[test]
    fn short_input_is_handled_gracefully() {
        let audio = AudioData::from_mono(vec![0.1; 100], 22_050);
        let result = analyze_onsets(&audio, &OnsetConfig::default());
        assert!(result.onsets.is_empty());
        assert!(result.flux.is_empty());
    }

    #[test]
    fn click_track_onsets_are_found_at_the_right_times() {
        // Clicks every 0.5 s (120 BPM) over 8 s at 22.05 kHz.
        let rate = 22_050;
        let truth: Vec<f64> = (0..16).map(|i| 0.25 + i as f64 * 0.5).collect();
        let audio = testsignal::click_track(&truth, 8.5, rate);
        let result = analyze_onsets(&audio, &OnsetConfig::default());

        assert_eq!(
            result.onsets.len(),
            truth.len(),
            "expected one onset per click, got {:?}",
            result.onsets.iter().map(|o| o.time_s).collect::<Vec<_>>()
        );
        for (onset, expected) in result.onsets.iter().zip(&truth) {
            assert!(
                (onset.time_s - expected).abs() < 0.015,
                "onset at {} expected near {expected}",
                onset.time_s
            );
        }
    }

    #[test]
    fn onset_strengths_reflect_click_loudness() {
        let rate = 22_050;
        // A loud click and a quiet click.
        let mut audio = testsignal::click_track(&[0.5], 2.0, rate);
        let quiet = testsignal::click_track_with_gain(&[1.5], 2.0, rate, 0.25);
        let mixed: Vec<f32> = audio
            .samples()
            .iter()
            .zip(quiet.samples())
            .map(|(a, b)| a + b)
            .collect();
        audio = AudioData::from_mono(mixed, rate);

        let result = analyze_onsets(&audio, &OnsetConfig::default());
        assert_eq!(result.onsets.len(), 2, "{:?}", result.onsets);
        assert!(
            result.onsets[0].strength > result.onsets[1].strength,
            "louder click should be stronger"
        );
    }

    #[test]
    fn brightness_separates_bass_from_treble_clicks() {
        let rate = 22_050;
        let bass = testsignal::tone_burst(0.5, 80.0, 0.1, 3.0, rate);
        let treble = testsignal::tone_burst(1.5, 5_000.0, 0.1, 3.0, rate);
        let mixed: Vec<f32> = bass
            .samples()
            .iter()
            .zip(treble.samples())
            .map(|(a, b)| a + b)
            .collect();
        let audio = AudioData::from_mono(mixed, rate);

        let result = analyze_onsets(&audio, &OnsetConfig::default());
        // A decaying burst may produce faint echo detections; the
        // generator's strength floor drops those. Judge the strong ones.
        let strong: Vec<_> = result.onsets.iter().filter(|o| o.strength >= 0.1).collect();
        assert_eq!(strong.len(), 2, "{:?}", result.onsets);
        assert!((strong[0].time_s - 0.5).abs() < 0.02, "{:?}", strong[0]);
        assert!((strong[1].time_s - 1.5).abs() < 0.02, "{:?}", strong[1]);
        assert!(
            strong[1].brightness > strong[0].brightness + 0.1,
            "5 kHz burst must be brighter than 80 Hz burst: {:?}",
            result.onsets
        );
    }
}
