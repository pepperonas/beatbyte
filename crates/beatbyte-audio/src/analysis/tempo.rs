//! Tempo estimation and beat-grid fitting.
//!
//! BPM comes from the autocorrelation of the onset-strength (flux)
//! envelope, weighted by a log-normal prior around 120 BPM to
//! disambiguate tempo octaves, with parabolic interpolation for
//! sub-BPM resolution. The beat grid is then phase-fitted so beats
//! land on actual onsets.

use beatbyte_core::music::Onset;
use serde::{Deserialize, Serialize};

/// Configuration for tempo estimation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoConfig {
    /// Lowest tempo considered.
    pub min_bpm: f64,
    /// Highest tempo considered.
    pub max_bpm: f64,
    /// Center of the log-normal tempo prior.
    pub prior_center_bpm: f64,
    /// Width (sigma) of the prior in octaves.
    pub prior_width_octaves: f64,
}

impl Default for TempoConfig {
    fn default() -> Self {
        TempoConfig {
            min_bpm: 60.0,
            max_bpm: 200.0,
            prior_center_bpm: 120.0,
            prior_width_octaves: 0.9,
        }
    }
}

/// A tempo estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoEstimate {
    /// Estimated tempo in BPM.
    pub bpm: f64,
    /// Normalized autocorrelation at the chosen period, 0.0–1.0.
    pub confidence: f64,
    /// The plausible other octave (half/double), if it also scored.
    pub alt_bpm: Option<f64>,
}

/// Estimate the tempo from the flux envelope. Returns `None` when the
/// envelope is too short or flat to carry tempo information.
#[must_use]
pub fn estimate_tempo(flux: &[f32], hop_s: f64, config: &TempoConfig) -> Option<TempoEstimate> {
    if hop_s <= 0.0 || flux.len() < 8 {
        return None;
    }
    let lag_min = ((60.0 / config.max_bpm) / hop_s).round() as usize;
    let lag_max = ((60.0 / config.min_bpm) / hop_s).ceil() as usize;
    if lag_min < 1 || lag_max + 8 >= flux.len() {
        return None;
    }

    // Mean-subtracted autocorrelation, normalized by lag 0.
    let mean = flux.iter().copied().sum::<f32>() / flux.len() as f32;
    let centered: Vec<f64> = flux.iter().map(|&v| f64::from(v - mean)).collect();
    let energy: f64 = centered.iter().map(|v| v * v).sum();
    if energy < 1e-12 {
        return None;
    }

    let ac = |lag: usize| -> f64 {
        let mut sum = 0.0;
        for t in 0..centered.len() - lag {
            sum += centered[t] * centered[t + lag];
        }
        (sum / energy).max(0.0)
    };

    let prior = |bpm: f64| -> f64 {
        let octaves = (bpm / config.prior_center_bpm).log2();
        (-0.5 * (octaves / config.prior_width_octaves).powi(2)).exp()
    };

    let mut best_lag = lag_min;
    let mut best_score = f64::NEG_INFINITY;
    let mut correlations = vec![0.0f64; lag_max + 2];
    for (lag, slot) in correlations
        .iter_mut()
        .enumerate()
        .take(lag_max + 1)
        .skip(lag_min)
    {
        let correlation = ac(lag);
        *slot = correlation;
        let bpm = 60.0 / (lag as f64 * hop_s);
        let score = correlation * prior(bpm);
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    if best_score <= 0.0 {
        return None;
    }

    // Parabolic interpolation around the winning lag for sub-frame
    // period resolution.
    let refined_lag = if best_lag > lag_min && best_lag < lag_max {
        let left = correlations[best_lag - 1];
        let center = correlations[best_lag];
        let right = correlations[best_lag + 1];
        let denom = left - 2.0 * center + right;
        if denom.abs() > 1e-12 {
            let delta = (0.5 * (left - right) / denom).clamp(-0.5, 0.5);
            best_lag as f64 + delta
        } else {
            best_lag as f64
        }
    } else {
        best_lag as f64
    };

    let bpm = 60.0 / (refined_lag * hop_s);
    let confidence = correlations[best_lag].clamp(0.0, 1.0);

    // Report the other octave when it also correlates substantially.
    let alt_bpm = [best_lag * 2, best_lag / 2]
        .into_iter()
        .filter(|&lag| lag >= 1 && lag <= lag_max && lag >= lag_min)
        .filter(|&lag| correlations[lag] > correlations[best_lag] * 0.5)
        .map(|lag| 60.0 / (lag as f64 * hop_s))
        .next();

    Some(TempoEstimate {
        bpm,
        confidence,
        alt_bpm,
    })
}

/// Fit a beat grid of the given BPM to the onsets: choose the phase
/// that maximizes onset support, then lay beats across the duration.
#[must_use]
pub fn fit_beat_grid(onsets: &[Onset], bpm: f64, duration_s: f64) -> (f64, Vec<f64>) {
    let period = 60.0 / bpm.max(f64::EPSILON);
    if duration_s <= 0.0 || !period.is_finite() || period <= 0.0 {
        return (0.0, Vec::new());
    }

    // Score candidate phases by Gaussian-weighted onset proximity.
    const CANDIDATES: usize = 64;
    const SIGMA_S: f64 = 0.03;
    let score_phase = |phase: f64| -> f64 {
        onsets
            .iter()
            .map(|onset| {
                let position = (onset.time_s - phase) / period;
                let distance = (position - position.round()) * period;
                f64::from(onset.strength) * (-0.5 * (distance / SIGMA_S).powi(2)).exp()
            })
            .sum()
    };

    let mut best_phase = 0.0;
    let mut best_score = f64::NEG_INFINITY;
    for i in 0..CANDIDATES {
        let phase = period * i as f64 / CANDIDATES as f64;
        let score = score_phase(phase);
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
    }

    // Refine: move the phase by the weighted mean residual of nearby
    // onsets (one Gauss–Newton step).
    let mut weight_sum = 0.0;
    let mut residual_sum = 0.0;
    for onset in onsets {
        let position = (onset.time_s - best_phase) / period;
        let residual = (position - position.round()) * period;
        if residual.abs() < SIGMA_S * 3.0 {
            let weight = f64::from(onset.strength);
            weight_sum += weight;
            residual_sum += residual * weight;
        }
    }
    if weight_sum > 0.0 {
        best_phase += residual_sum / weight_sum;
        best_phase = best_phase.rem_euclid(period);
    }

    let mut beats = Vec::new();
    let mut t = best_phase;
    while t < duration_s {
        beats.push(t);
        t += period;
    }
    (best_phase, beats)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::analysis::onset::{OnsetConfig, analyze_onsets};
    use crate::synth;

    fn tempo_of_click_track(bpm: f64) -> TempoEstimate {
        let period = 60.0 / bpm;
        let times: Vec<f64> = (0..((20.0 / period) as usize))
            .map(|i| 0.2 + i as f64 * period)
            .collect();
        let audio = synth::click_track(&times, 21.0, 22_050);
        let flux = analyze_onsets(&audio, &OnsetConfig::default());
        estimate_tempo(&flux.flux, flux.hop_s, &TempoConfig::default())
            .expect("click track must yield a tempo")
    }

    #[test]
    fn detects_100_bpm() {
        let estimate = tempo_of_click_track(100.0);
        assert!(
            (estimate.bpm - 100.0).abs() < 2.0,
            "expected ~100 BPM, got {}",
            estimate.bpm
        );
        assert!(
            estimate.confidence > 0.3,
            "confidence {}",
            estimate.confidence
        );
    }

    #[test]
    fn detects_132_bpm() {
        let estimate = tempo_of_click_track(132.0);
        assert!(
            (estimate.bpm - 132.0).abs() < 2.5,
            "expected ~132 BPM, got {}",
            estimate.bpm
        );
    }

    #[test]
    fn fast_tempi_resolve_to_a_valid_octave() {
        // 174 BPM material may legitimately be heard at 87; either
        // octave is acceptable, silence about it is not.
        let estimate = tempo_of_click_track(174.0);
        let ok = (estimate.bpm - 174.0).abs() < 3.0 || (estimate.bpm - 87.0).abs() < 2.0;
        assert!(ok, "got {} BPM", estimate.bpm);
        if (estimate.bpm - 87.0).abs() < 2.0 {
            let alt = estimate
                .alt_bpm
                .expect("half-time pick must report the double");
            assert!((alt - 174.0).abs() < 4.0, "alt {alt}");
        }
    }

    #[test]
    fn silence_has_no_tempo() {
        assert!(estimate_tempo(&[0.0; 2000], 0.0116, &TempoConfig::default()).is_none());
        assert!(estimate_tempo(&[], 0.0116, &TempoConfig::default()).is_none());
    }

    #[test]
    fn beat_grid_locks_onto_click_phase() {
        let bpm = 120.0;
        let truth: Vec<f64> = (0..40).map(|i| 0.30 + i as f64 * 0.5).collect();
        let audio = synth::click_track(&truth, 21.0, 22_050);
        let flux = analyze_onsets(&audio, &OnsetConfig::default());
        let (offset, beats) = fit_beat_grid(&flux.onsets, bpm, 21.0);

        assert!(!beats.is_empty());
        // The grid phase must land on the click phase (0.30 mod 0.5).
        let phase_error = (offset - 0.30).abs().min((offset - 0.30 + 0.5).abs());
        assert!(
            phase_error < 0.02,
            "grid offset {offset} should align with clicks at 0.30 + n·0.5"
        );
        // Every true click should have a beat within 20 ms.
        for click in &truth {
            let nearest = beats
                .iter()
                .map(|b| (b - click).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 0.02, "click {click} has no nearby beat");
        }
    }

    #[test]
    fn beat_grid_handles_empty_input() {
        let (offset, beats) = fit_beat_grid(&[], 120.0, 10.0);
        assert_eq!(offset, 0.0);
        assert_eq!(beats.len(), 20);
    }
}
