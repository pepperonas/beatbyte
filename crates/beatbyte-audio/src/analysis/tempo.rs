//! Tempo estimation and beat-grid fitting.
//!
//! BPM comes from the autocorrelation of the onset-strength (flux)
//! envelope, weighted by a log-normal prior around 120 BPM to
//! disambiguate tempo octaves, with parabolic interpolation for
//! sub-BPM resolution. The beat grid is then phase-fitted so beats
//! land on actual onsets.

use beatbyte_core::music::Onset;

/// Configuration for tempo estimation.
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// The listener's preference for tempi near the centre of the
/// perceptual range. Used only to break ties between grids that
/// explain the onsets equally well.
fn perceptual_prior(bpm: f64, config: &TempoConfig) -> f64 {
    let octaves = (bpm / config.prior_center_bpm).log2();
    (-0.5 * (octaves / config.prior_width_octaves).powi(2)).exp()
}

/// Width of the support band inside which the octave is decided by
/// preference rather than by evidence.
///
/// It has to be wide enough to hold octave-related grids together:
/// weighting a beat hit above an eighth hit means a faster grid
/// scores systematically higher on identical music — by up to 1/0.8 —
/// so a tight band silently excluded the correct slower octave before
/// the prior ever got to vote.
const OCTAVE_BAND: f64 = 0.85;

/// Whether two periods are the same grid seen at a different octave.
///
/// Only this relationship may be settled by preference. Support is
/// mathematically unable to separate a grid from its double (the
/// faster grid contains the slower one), which is exactly why the
/// prior exists — but it must never choose between UNRELATED tempi.
/// Letting it do so put a voice-and-guitar scene at 138 BPM instead
/// of 110, purely because 138 sits closer to the perceptual centre.
fn is_octave_of(period: f64, reference: f64) -> bool {
    if period <= 0.0 || reference <= 0.0 {
        return false;
    }
    let octaves = (period / reference).log2();
    octaves.abs() <= 2.5 && (octaves - octaves.round()).abs() <= 0.045
}

/// How close an onset must sit to a grid point to count as "on it".
/// 45 ms is the outer edge of BeatByte's Great window: a listener
/// would still hear such an onset as being on the beat.
const GRID_TOLERANCE_S: f64 = 0.045;

/// Share of onset strength that a beat of `period_s` explains, once
/// its SUBDIVISIONS are taken into account.
///
/// Music does not put every note on a beat, so scoring a candidate
/// tempo by beat hits alone rewards whatever grid happens to catch
/// the most notes — measured on a sixteenth-note riff, that was a
/// dotted-eighth grid at 186 BPM instead of the real 140. What makes
/// a tempo right is that the notes land on its beats, eighths and
/// sixteenths, and the metric hierarchy is honoured by weighting a
/// beat hit above an eighth above a sixteenth.
#[must_use]
pub fn grid_support(onsets: &[Onset], period_s: f64) -> f64 {
    if period_s <= 0.0 || onsets.is_empty() {
        return 0.0;
    }
    let total: f64 = onsets.iter().map(|o| f64::from(o.strength)).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let (_, support) = best_phase_support(onsets, period_s);
    support / total
}

/// Weight of a hit by which level of the metric hierarchy it lands
/// on: beat, eighth, sixteenth.
fn subdivision_weight(sixteenth_index: i64) -> f64 {
    if sixteenth_index.rem_euclid(4) == 0 {
        1.0
    } else if sixteenth_index.rem_euclid(2) == 0 {
        0.9
    } else {
        0.8
    }
}

/// The best phase for a period and the (weighted) onset strength its
/// sixteenth-note grid explains.
fn best_phase_support(onsets: &[Onset], period_s: f64) -> (f64, f64) {
    const CANDIDATES: usize = 48;
    let step = period_s / 4.0;
    // The tolerance must shrink with the grid: at 200 BPM a sixteenth
    // is 75 ms, and a fixed 45 ms window would call more than half of
    // all random positions "on the grid".
    let tolerance = GRID_TOLERANCE_S.min(step * 0.3);
    let mut best = (0.0, f64::NEG_INFINITY);
    for i in 0..CANDIDATES {
        let phase = period_s * i as f64 / CANDIDATES as f64;
        let mut support = 0.0;
        for onset in onsets {
            let position = (onset.time_s - phase) / step;
            let nearest = position.round();
            if ((position - nearest) * step).abs() <= tolerance {
                support += f64::from(onset.strength) * subdivision_weight(nearest as i64);
            }
        }
        if support > best.1 {
            best = (phase, support);
        }
    }
    best
}

/// How plausible a candidate beat is as a *metre*, from the interval
/// the music actually repeats at.
///
/// Sparse material leaves the octave underdetermined in a way support
/// cannot touch: eight chords 1.33 s apart are explained equally well
/// by 90 BPM (a chord every 2 beats), 135 (every 3) and 180 (every
/// 4). Western music is overwhelmingly duple, so the reading where
/// the repeat lands on a power-of-two number of beats is the one to
/// believe — that is what tells 90 from 135 here.
fn meter_plausibility(onsets: &[Onset], period_s: f64) -> f64 {
    if onsets.len() < 3 || period_s <= 0.0 {
        return 1.0;
    }
    let mut gaps: Vec<f64> = onsets
        .windows(2)
        .map(|pair| pair[1].time_s - pair[0].time_s)
        .filter(|gap| *gap > 0.02)
        .collect();
    if gaps.is_empty() {
        return 1.0;
    }
    gaps.sort_by(f64::total_cmp);
    let median = gaps[gaps.len() / 2];
    let beats = median / period_s;
    if beats <= 0.0 {
        return 1.0;
    }
    // Distance to the nearest power of two, in octaves.
    let octaves = beats.log2();
    let error = (octaves - octaves.round()).abs();
    // 1.0 on a power of two, falling to ~0.35 at the worst case (a
    // 1.5x or 3x relationship). The range has to be wide: on sparse
    // material this term is often the only thing separating a duple
    // reading from a triple one, and the perceptual prior actively
    // pulls the wrong way there (it prefers whatever sits closest to
    // 120 BPM regardless of how the music divides).
    (-0.5 * (error / 0.42).powi(2)).exp().mul_add(0.65, 0.35)
}

/// Candidate periods taken straight from the spacing between onsets.
///
/// The autocorrelation needs a reasonably dense envelope; sparse
/// material defeats it entirely (a chord progression of eight stabs
/// produced no candidate at all, and the pipeline silently fell back
/// to 120 BPM while a 90 BPM grid explained every single onset).
/// Inter-onset intervals do not care about density: if the same gap
/// keeps recurring, that gap is musical.
fn ioi_periods(onsets: &[Onset], min_period: f64, max_period: f64) -> Vec<f64> {
    if onsets.len() < 3 {
        return Vec::new();
    }
    // First- and second-order gaps: a melody often skips a beat.
    let mut gaps: Vec<f64> = Vec::new();
    for order in 1..=2usize {
        for pair in onsets.windows(order + 1) {
            let gap = pair[order].time_s - pair[0].time_s;
            if gap > 0.02 {
                gaps.push(gap);
            }
        }
    }
    if gaps.is_empty() {
        return Vec::new();
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    // Cluster on a log scale (5% bins): musical relationships are
    // ratios, so equal-width linear bins would over-resolve the fast
    // end and smear the slow end.
    let mut clusters: Vec<(f64, usize)> = Vec::new();
    for gap in gaps {
        match clusters.last_mut() {
            Some((centre, count)) if gap / *centre < 1.05 => {
                *centre = (*centre * *count as f64 + gap) / (*count + 1) as f64;
                *count += 1;
            }
            _ => clusters.push((gap, 1)),
        }
    }
    clusters.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.total_cmp(&b.0)));
    clusters.truncate(4);

    let mut periods = Vec::new();
    for (gap, _) in clusters {
        // The gap may be one beat or several; try the small integer
        // divisions and keep whatever lands in range.
        for division in 1..=4u32 {
            let period = gap / f64::from(division);
            if period >= min_period && period <= max_period {
                periods.push(period);
            }
        }
    }
    periods
}

/// One tempo candidate with the evidence behind it — the answer to
/// "why did it pick that BPM?".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoCandidate {
    /// Candidate tempo.
    pub bpm: f64,
    /// Share of onset strength its sixteenth grid explains.
    pub support: f64,
    /// Perceptual preference for this tempo.
    pub prior: f64,
    /// Duple-metre plausibility of the repeat interval.
    pub meter: f64,
    /// Whether it survived the support band and could be chosen.
    pub in_band: bool,
}

/// Every candidate the estimator considered, best support first.
/// Exposed for debugging and for the CLI's analysis report: a tempo
/// decision that cannot be inspected cannot be trusted.
#[must_use]
pub fn tempo_candidates(
    flux: &[f32],
    hop_s: f64,
    onsets: &[Onset],
    config: &TempoConfig,
) -> Vec<TempoCandidate> {
    let mut periods = candidate_periods(flux, hop_s, onsets, config);
    periods.retain(|p| *p >= 60.0 / config.max_bpm && *p <= 60.0 / config.min_bpm);
    let mut scored: Vec<TempoCandidate> = periods
        .iter()
        .map(|period| TempoCandidate {
            bpm: 60.0 / period,
            support: grid_support(onsets, *period),
            prior: perceptual_prior(60.0 / period, config),
            meter: meter_plausibility(onsets, *period),
            in_band: false,
        })
        .collect();
    let best = scored.iter().map(|c| c.support).fold(0.0f64, f64::max);
    let best_grid = scored
        .iter()
        .max_by(|a, b| {
            a.support
                .total_cmp(&b.support)
                .then(b.bpm.total_cmp(&a.bpm))
        })
        .map_or(config.prior_center_bpm, |c| c.bpm);
    for candidate in &mut scored {
        candidate.in_band = candidate.support >= best * OCTAVE_BAND
            && is_octave_of(60.0 / candidate.bpm, 60.0 / best_grid);
    }
    scored.sort_by(|a, b| {
        b.support
            .total_cmp(&a.support)
            .then(a.bpm.total_cmp(&b.bpm))
    });
    scored
}

/// Estimate the tempo from the flux envelope and the onsets it
/// produced. Returns `None` when there is too little to go on.
///
/// The autocorrelation proposes candidates; the ONSETS decide between
/// them. A fixed preference for 120 BPM cannot disambiguate octaves —
/// it just picks one (measured: three of eight evaluation scenes came
/// out at the wrong tempo, one of them not even an octave away). The
/// tempo that wins here is the one whose beat grid actually explains
/// where the onsets are.
#[must_use]
pub fn estimate_tempo(
    flux: &[f32],
    hop_s: f64,
    onsets: &[Onset],
    config: &TempoConfig,
) -> Option<TempoEstimate> {
    if hop_s <= 0.0 || flux.len() < 8 {
        return None;
    }
    let lag_min = ((60.0 / config.max_bpm) / hop_s).round() as usize;
    let lag_max = ((60.0 / config.min_bpm) / hop_s).ceil() as usize;
    if lag_min < 1 || lag_max + 8 >= flux.len() {
        return None;
    }

    // Silence carries no tempo; the autocorrelation inside
    // `candidate_periods` makes the same check for its own reasons.
    let mean = flux.iter().copied().sum::<f32>() / flux.len() as f32;
    if flux.iter().map(|v| (v - mean).powi(2)).sum::<f32>() < 1e-12 && onsets.is_empty() {
        return None;
    }

    let mut periods = candidate_periods(flux, hop_s, onsets, config);
    periods.retain(|p| *p >= 60.0 / config.max_bpm && *p <= 60.0 / config.min_bpm);
    if periods.is_empty() {
        return None;
    }

    // Step 1: which grid explains the onsets? That is a measurement.
    let scored: Vec<(f64, f64)> = periods
        .iter()
        .map(|period| (*period, grid_support(onsets, *period)))
        .collect();
    let best_support = scored
        .iter()
        .map(|(_, support)| *support)
        .fold(0.0f64, f64::max);
    if best_support <= 0.0 {
        return None;
    }

    // Step 2: which OCTAVE of that grid? That is perception, not
    // measurement — a grid and its double explain the onsets equally
    // well by construction (the faster grid contains the slower one),
    // so no amount of onset evidence can separate them. The listener's
    // preferred tactus does: among the grids that explain the music
    // essentially as well as the best one, take the tempo closest to
    // the perceptual centre. This is what finally got all three
    // ambiguous scenes right at once (90 not 180, 96 not 192,
    // 150 not 75) — earlier attempts that tried to decide the octave
    // from the onsets alone fixed one scene and broke another.
    // The grid with the most support is the reference; only its own
    // octaves may then be re-ranked by preference.
    let best_grid = scored
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.total_cmp(&a.0)))
        .map_or(60.0 / config.prior_center_bpm, |(period, _)| *period);
    let mut best_period = best_grid;
    let mut best_prior = f64::NEG_INFINITY;
    for (period, support) in &scored {
        if *support < best_support * OCTAVE_BAND || !is_octave_of(*period, best_grid) {
            continue;
        }
        let value = perceptual_prior(60.0 / period, config) * meter_plausibility(onsets, *period);
        if value > best_prior {
            best_prior = value;
            best_period = *period;
        }
    }

    let bpm = 60.0 / best_period;
    // Confidence is how much of the music the chosen grid explains —
    // a far more useful number downstream than a raw autocorrelation
    // peak, which can be high on a grid that fits nothing.
    let confidence = grid_support(onsets, best_period).clamp(0.0, 1.0);

    // The honest alternative reading, when one exists in range.
    let alt_bpm = [bpm * 2.0, bpm / 2.0]
        .into_iter()
        .filter(|candidate| *candidate >= config.min_bpm && *candidate <= config.max_bpm)
        .find(|candidate| grid_support(onsets, 60.0 / candidate) >= best_support * 0.95);

    Some(TempoEstimate {
        bpm,
        confidence,
        alt_bpm,
    })
}

/// Every period worth testing: autocorrelation peaks (refined to
/// sub-frame resolution) plus the raw onset spacing. Both are only
/// proposals — the onsets decide between them.
fn candidate_periods(flux: &[f32], hop_s: f64, onsets: &[Onset], config: &TempoConfig) -> Vec<f64> {
    let mut periods = Vec::new();
    let lag_min = ((60.0 / config.max_bpm) / hop_s).round() as usize;
    let lag_max = ((60.0 / config.min_bpm) / hop_s).ceil() as usize;
    if hop_s > 0.0 && lag_min >= 1 && lag_max + 8 < flux.len() {
        let mean = flux.iter().copied().sum::<f32>() / flux.len() as f32;
        let centered: Vec<f64> = flux.iter().map(|&v| f64::from(v - mean)).collect();
        let energy: f64 = centered.iter().map(|v| v * v).sum();
        if energy >= 1e-12 {
            let mut correlations = vec![0.0f64; lag_max + 2];
            for lag in lag_min..=lag_max {
                let mut sum = 0.0;
                for t in 0..centered.len() - lag {
                    sum += centered[t] * centered[t + lag];
                }
                correlations[lag] = (sum / energy).max(0.0);
            }
            let mut peaks: Vec<(usize, f64)> = (lag_min + 1..lag_max)
                .filter(|&lag| {
                    correlations[lag] >= correlations[lag - 1]
                        && correlations[lag] > correlations[lag + 1]
                })
                .map(|lag| {
                    let bpm = 60.0 / (lag as f64 * hop_s);
                    (lag, correlations[lag] * perceptual_prior(bpm, config))
                })
                .filter(|(_, score)| *score > 0.0)
                .collect();
            peaks.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            peaks.truncate(8);
            periods.extend(
                peaks
                    .iter()
                    .map(|(lag, _)| refine_lag(&correlations, *lag, lag_min, lag_max) * hop_s),
            );
        }
    }
    periods.extend(ioi_periods(
        onsets,
        60.0 / config.max_bpm,
        60.0 / config.min_bpm,
    ));
    periods
}

/// Parabolic interpolation around an autocorrelation peak, for
/// sub-frame period resolution (lag quantization alone is worth
/// several BPM at fast tempi).
fn refine_lag(correlations: &[f64], lag: usize, lag_min: usize, lag_max: usize) -> f64 {
    if lag <= lag_min || lag >= lag_max || lag + 1 >= correlations.len() {
        return lag as f64;
    }
    let (left, centre, right) = (
        correlations[lag - 1],
        correlations[lag],
        correlations[lag + 1],
    );
    let denominator = left - 2.0 * centre + right;
    if denominator.abs() <= 1e-12 {
        return lag as f64;
    }
    lag as f64 + (0.5 * (left - right) / denominator).clamp(-0.5, 0.5)
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
        estimate_tempo(
            &flux.flux,
            flux.hop_s,
            &flux.onsets,
            &TempoConfig::default(),
        )
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
        assert!(estimate_tempo(&[0.0; 2000], 0.0116, &[], &TempoConfig::default()).is_none());
        assert!(estimate_tempo(&[], 0.0116, &[], &TempoConfig::default()).is_none());
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
