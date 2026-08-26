//! Onset detection via spectral flux.
//!
//! STFT → log-compressed magnitude spectra → half-wave-rectified flux →
//! adaptive median threshold → local-maximum peak picking. A classic,
//! well-understood pipeline implemented from scratch; every stage is a
//! pure function over sample buffers.

use beatbyte_core::music::Onset;
use realfft::RealFftPlanner;

use crate::decode::AudioData;

/// Configuration for onset detection.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Reject onsets weaker than this fraction of the LOCAL peak.
    /// Release ramps and reverb tails are 20–40x weaker than the
    /// attacks around them, which is what makes a relative floor work
    /// where an absolute one cannot.
    pub min_local_strength: f32,
    /// Half-width of the window used to judge local loudness.
    pub local_window_s: f64,
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
            min_local_strength: 0.12,
            local_window_s: 2.0,
        }
    }
}

/// Frames of lag for the SuperFlux difference.
///
/// One frame, not two. The ±1-bin maximum filter is what defends
/// against frequency drift; the temporal lag only helps with tremolo,
/// and it costs timing: a two-frame lag holds the flux elevated after
/// the attack, so peak picking landed 17 ms late on a click track
/// whose true positions are known exactly.
const SUPERFLUX_LAG: usize = 1;

/// Number of frequency bands analysed separately.
const BANDS: usize = 3;

/// Band edges in Hz: bass/body, the register guitars and voices live
/// in, and the bright transient region (cymbals, pick noise).
const BAND_EDGES_HZ: [f64; BANDS + 1] = [0.0, 220.0, 2_000.0, 20_000.0];

/// Frame-level analysis products shared by onset and tempo stages.
#[derive(Debug, Clone, PartialEq)]
pub struct FluxAnalysis {
    /// Spectral flux per frame, normalized to 0.0–1.0.
    pub flux: Vec<f32>,
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
    // History deep enough for the SuperFlux lag.
    let mut history: Vec<Vec<f32>> = vec![vec![0.0f32; bins]; SUPERFLUX_LAG + 1];
    let mut band_flux: Vec<Vec<f32>> = (0..BANDS).map(|_| Vec::with_capacity(frames)).collect();
    let mut brightness = Vec::with_capacity(frames);
    // Band edges in Hz → bin indices, so the split follows the music
    // rather than the FFT size.
    let bin_hz = rate / config.window as f64;
    let edges: Vec<usize> = BAND_EDGES_HZ
        .iter()
        .map(|hz| ((hz / bin_hz).round() as usize).min(bins))
        .collect();

    for frame in 0..frames {
        let start = frame * config.hop;
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples[start + i] * hann[i];
        }
        // realfft only fails on wrong buffer lengths, which are fixed here.
        if fft.process(&mut input, &mut spectrum).is_err() {
            break;
        }

        let mut current = vec![0.0f32; bins];
        let mut centroid_num = 0.0f32;
        let mut centroid_den = 0.0f32;
        for (k, value) in spectrum.iter().enumerate() {
            let magnitude = value.norm();
            current[k] = (1.0 + 50.0 * magnitude).ln();
            centroid_num += k as f32 * magnitude;
            centroid_den += magnitude;
        }

        // SuperFlux: difference against a frame SUPERFLUX_LAG back,
        // maximum-filtered across ±1 bin. A tone that drifts slowly in
        // frequency (vibrato, a bent string, the beating of a harmonic
        // stack) then produces no flux at all, while a real attack —
        // energy appearing where there was none — still does.
        let reference = &history[frame % history.len()];
        let mut bands = [0.0f32; BANDS];
        for band in 0..BANDS {
            let (from, to) = (edges[band], edges[band + 1]);
            let mut sum = 0.0f32;
            for (k, value) in current.iter().enumerate().take(to).skip(from) {
                let low = k.saturating_sub(1);
                let high = (k + 2).min(bins);
                let filtered = reference[low..high].iter().copied().fold(0.0f32, f32::max);
                sum += (value - filtered).max(0.0);
            }
            bands[band] = if frame <= SUPERFLUX_LAG { 0.0 } else { sum };
        }
        for (band, value) in bands.iter().enumerate() {
            band_flux[band].push(*value);
        }
        let slot = frame % history.len();
        history[slot] = current;

        brightness.push(if centroid_den > 1e-9 {
            (centroid_num / centroid_den) / bins as f32
        } else {
            0.0
        });
    }

    // Each band is normalized on its own before they are summed, so a
    // quiet pick attack in the mid band is not buried by a loud kick
    // in the low band. Summing raw magnitudes is how drums came to
    // dominate every detection.
    let mut flux = vec![0.0f32; brightness.len()];
    for band in &mut band_flux {
        normalize(band);
        for (total, value) in flux.iter_mut().zip(band.iter()) {
            *total += *value;
        }
    }
    normalize(&mut flux);
    let local = local_scale(&flux, hop_s, config.local_window_s);
    let onsets = pick_onsets(&flux, &local, &brightness, hop_s, frame_offset_s, config);

    FluxAnalysis {
        flux,
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

/// The loudest flux in the neighbourhood of each frame.
///
/// Onset strength used to be normalized by the loudest onset in the
/// WHOLE song, which meant a quiet intro produced onsets with a
/// strength near zero — and the chart generator, which selects notes
/// by strength, then skipped quiet passages wholesale. Judging each
/// onset against its own surroundings is both what a listener does
/// and what makes a relative rejection floor meaningful.
fn local_scale(flux: &[f32], hop_s: f64, window_s: f64) -> Vec<f32> {
    let half = if hop_s > 0.0 {
        ((window_s / hop_s).round() as usize).max(1)
    } else {
        1
    };
    (0..flux.len())
        .map(|t| {
            let from = t.saturating_sub(half);
            let to = (t + half + 1).min(flux.len());
            flux[from..to].iter().copied().fold(1e-6f32, f32::max)
        })
        .collect()
}

/// Adaptive-threshold local-maximum peak picking.
fn pick_onsets(
    flux: &[f32],
    local: &[f32],
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

        // Strength relative to the local peak, which is also what
        // rejects release ramps: they sit 20–40x below the attacks
        // they follow.
        let strength = (flux[t] / local.get(t).copied().unwrap_or(1.0)).clamp(0.0, 1.0);
        if strength < config.min_local_strength {
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
            strength,
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
