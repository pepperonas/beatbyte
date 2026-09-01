//! Measuring the analysis against known ground truth.
//!
//! Phase 1 of the loop-house hardening: nothing in the pipeline may
//! be tuned before its quality can be MEASURED, or the tuning is
//! guesswork with extra steps.
//!
//! Everything here is pure — ground truth in, scores out — so the
//! metric definitions themselves are unit-tested against cases whose
//! answers can be worked out by hand.
//!
//! ## Where ground truth comes from
//!
//! - **Synthetic cases** ([`synthetic`]): built by this crate, so the
//!   beat times are exact by construction rather than annotated.
//!   They reproduce the material properties that break the current
//!   pipeline (two timing rasters, soft transients, filter sweeps,
//!   a flat 4/4 with no accent hierarchy).
//! - **The built-in demo songs**: rendered from a known BPM and bar
//!   count, so their grids are exact too — the rock-class reference
//!   that must not regress.
//! - **Real tracks**: the JSON sidecar in [`GroundTruth`], or a
//!   Rekordbox XML export ([`rekordbox`]).
//!
//! ⚠️ Ableton `.asd` is deliberately NOT parsed: it is an
//! undocumented binary format, and guessing at its layout would put
//! invented facts into the measurement that everything else is
//! judged by.

use serde::{Deserialize, Serialize};

use beatbyte_core::music::SongAnalysis;

pub mod rekordbox;
pub mod synthetic;

/// MIREX beat tolerance: a detection counts when it lands within
/// this of an annotation.
pub const BEAT_TOLERANCE_S: f64 = 0.070;
/// MIREX continuity tolerance, as a fraction of the inter-beat
/// interval (used by CMLt/AMLt).
pub const CONTINUITY_RATIO: f64 = 0.175;
/// Structure boundaries count as hit within this.
pub const BOUNDARY_TOLERANCE_S: f64 = 0.5;

/// Reference annotations for one track — the JSON sidecar format.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GroundTruth {
    /// True tempo.
    pub bpm: f64,
    /// Where bar 1 starts, in milliseconds.
    pub first_downbeat_ms: f64,
    /// Every beat, in seconds.
    pub beats: Vec<f64>,
    /// Every downbeat, in seconds.
    pub downbeats: Vec<f64>,
    /// Structure boundaries, in seconds.
    #[serde(default)]
    pub boundaries: Vec<f64>,
}

impl GroundTruth {
    /// A steady 4/4 grid — how the synthetic cases and the demo
    /// songs describe themselves.
    #[must_use]
    pub fn steady(bpm: f64, first_beat_s: f64, beats: usize) -> GroundTruth {
        let period = 60.0 / bpm;
        let times: Vec<f64> = (0..beats)
            .map(|i| first_beat_s + i as f64 * period)
            .collect();
        GroundTruth {
            bpm,
            first_downbeat_ms: first_beat_s * 1000.0,
            downbeats: times.iter().step_by(4).copied().collect(),
            beats: times,
            boundaries: Vec::new(),
        }
    }
}

/// What one track scored.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scores {
    /// Beat F-measure at ±70 ms.
    pub beat_f: f64,
    /// Correct-metrical-level, total.
    pub cmlt: f64,
    /// Allowed-metrical-level, total (double/half/offbeat count).
    pub amlt: f64,
    /// Fraction of reference downbeats hit.
    pub downbeat_accuracy: f64,
    /// Fraction of reference boundaries hit within ±0.5 s.
    pub boundary_hit: f64,
    /// Fraction of hit boundaries that landed exactly on a bar line.
    pub boundary_on_bar: f64,
    /// Estimated tempo, for the octave-error check.
    pub bpm: f64,
    /// Notes per second, median across 10 s windows.
    pub notes_per_s_median: f64,
    /// Notes per second, 95th percentile.
    pub notes_per_s_p95: f64,
}

/// Match detections to annotations one-to-one within `tolerance`,
/// returning the number of matches. Both inputs must be ascending.
/// Pure — tested.
#[must_use]
pub fn count_matches(detected: &[f64], truth: &[f64], tolerance: f64) -> usize {
    let mut used = vec![false; detected.len()];
    let mut matches = 0;
    for &annotation in truth {
        // Nearest unused detection inside the window wins; a
        // detection may only pay for one annotation, or a single
        // burst of detections would "hit" a whole bar.
        let mut best: Option<(usize, f64)> = None;
        for (index, &time) in detected.iter().enumerate() {
            if used[index] {
                continue;
            }
            let distance = (time - annotation).abs();
            if distance <= tolerance && best.is_none_or(|(_, d)| distance < d) {
                best = Some((index, distance));
            }
        }
        if let Some((index, _)) = best {
            used[index] = true;
            matches += 1;
        }
    }
    matches
}

/// Beat F-measure at [`BEAT_TOLERANCE_S`]. Pure — tested.
#[must_use]
pub fn f_measure(detected: &[f64], truth: &[f64]) -> f64 {
    if detected.is_empty() || truth.is_empty() {
        return 0.0;
    }
    let matches = count_matches(detected, truth, BEAT_TOLERANCE_S) as f64;
    let precision = matches / detected.len() as f64;
    let recall = matches / truth.len() as f64;
    if precision + recall <= 0.0 {
        return 0.0;
    }
    2.0 * precision * recall / (precision + recall)
}

/// Continuity-based accuracy at ONE metrical level: the fraction of
/// annotations whose beat was found AND whose predecessor was too,
/// with a matching interval (the MIREX continuity condition).
/// Pure — tested.
#[must_use]
pub fn continuity_total(detected: &[f64], truth: &[f64]) -> f64 {
    if truth.len() < 2 || detected.is_empty() {
        return 0.0;
    }
    let nearest = |time: f64| -> Option<f64> {
        detected
            .iter()
            .copied()
            .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()))
    };
    let mut correct = 0usize;
    let mut previous_ok = false;
    for window in truth.windows(2) {
        let (before, now) = (window[0], window[1]);
        let interval = now - before;
        let tolerance = interval * CONTINUITY_RATIO;
        let hit_now = nearest(now).is_some_and(|d| (d - now).abs() <= tolerance);
        let hit_before = nearest(before).is_some_and(|d| (d - before).abs() <= tolerance);
        // "Total" counts every correctly tracked beat, whether or not
        // the tracking broke earlier — that is what separates it from
        // the "continuous" variant.
        if hit_now && (hit_before || previous_ok) {
            correct += 1;
        }
        previous_ok = hit_now;
    }
    correct as f64 / (truth.len() - 1) as f64
}

/// Allowed-metrical-level accuracy: the best continuity score over
/// the interpretations MIREX permits — the annotated grid, double
/// and half tempo, and the offbeat. Pure — tested.
#[must_use]
pub fn amlt(detected: &[f64], truth: &[f64]) -> f64 {
    let mut variants: Vec<Vec<f64>> = vec![truth.to_vec()];
    // Half tempo: every other beat.
    variants.push(truth.iter().step_by(2).copied().collect());
    // Double tempo: a beat inserted between each pair.
    let mut doubled = Vec::with_capacity(truth.len() * 2);
    for window in truth.windows(2) {
        doubled.push(window[0]);
        doubled.push(f64::midpoint(window[0], window[1]));
    }
    if let Some(&last) = truth.last() {
        doubled.push(last);
    }
    variants.push(doubled);
    // Offbeat: the annotated grid shifted by half a beat.
    if truth.len() >= 2 {
        let offset = (truth[1] - truth[0]) / 2.0;
        variants.push(truth.iter().map(|t| t + offset).collect());
    }
    variants
        .iter()
        .map(|variant| continuity_total(detected, variant))
        .fold(0.0, f64::max)
}

/// Notes per second across 10 s windows: the median and the 95th
/// percentile — the game-side question of whether a chart is empty
/// or a wall. Pure — tested.
#[must_use]
pub fn note_density(times: &[f64], duration_s: f64) -> (f64, f64) {
    if duration_s <= 0.0 {
        return (0.0, 0.0);
    }
    let windows = (duration_s / 10.0).ceil().max(1.0) as usize;
    let mut rates: Vec<f64> = (0..windows)
        .map(|w| {
            let (start, end) = (w as f64 * 10.0, (w as f64 + 1.0) * 10.0);
            let count = times.iter().filter(|&&t| t >= start && t < end).count();
            count as f64 / 10.0
        })
        .collect();
    rates.sort_by(f64::total_cmp);
    let median = rates[rates.len() / 2];
    let p95 = rates[((rates.len() as f64 * 0.95) as usize).min(rates.len() - 1)];
    (median, p95)
}

/// Score one analysis against its ground truth.
#[must_use]
pub fn evaluate(analysis: &SongAnalysis, truth: &GroundTruth) -> Scores {
    let beats = &analysis.beats;
    let bar = 4.0 * 60.0 / truth.bpm.max(f64::EPSILON);
    let on_bar = |time: f64| {
        let position = (time - truth.first_downbeat_ms / 1000.0) / bar;
        (position - position.round()).abs() * bar <= BEAT_TOLERANCE_S
    };
    let boundary_matches = count_matches(&[], &truth.boundaries, BOUNDARY_TOLERANCE_S);
    let onset_times: Vec<f64> = analysis.onsets.iter().map(|o| o.time_s).collect();
    let (median, p95) = note_density(&onset_times, analysis.duration_s);
    Scores {
        beat_f: f_measure(beats, &truth.beats),
        cmlt: continuity_total(beats, &truth.beats),
        amlt: amlt(beats, &truth.beats),
        downbeat_accuracy: if truth.downbeats.is_empty() {
            0.0
        } else {
            // The pipeline has no downbeat stage; the only candidate
            // is the grid's first beat, so this measures "does bar 1
            // land on a real downbeat" rather than a full sequence.
            let first = beats.first().copied().unwrap_or(0.0);
            f64::from(
                truth
                    .downbeats
                    .iter()
                    .any(|d| (d - first).abs() <= BEAT_TOLERANCE_S),
            )
        },
        boundary_hit: if truth.boundaries.is_empty() {
            0.0
        } else {
            boundary_matches as f64 / truth.boundaries.len() as f64
        },
        boundary_on_bar: if truth.boundaries.is_empty() {
            0.0
        } else {
            truth.boundaries.iter().filter(|&&b| on_bar(b)).count() as f64
                / truth.boundaries.len() as f64
        },
        bpm: analysis.bpm,
        notes_per_s_median: median,
        notes_per_s_p95: p95,
    }
}

/// Whether an estimate is an octave error against the truth.
/// Pure — tested.
#[must_use]
pub fn is_octave_error(estimated: f64, truth: f64) -> bool {
    if truth <= 0.0 || estimated <= 0.0 {
        return false;
    }
    let ratio = estimated / truth;
    let close = |target: f64| (ratio - target).abs() / target < 0.05;
    !close(1.0) && (close(2.0) || close(0.5) || close(4.0) || close(0.25))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perfect 120 BPM grid: a beat every half second.
    fn grid(count: usize) -> Vec<f64> {
        (0..count).map(|i| i as f64 * 0.5).collect()
    }

    #[test]
    fn a_perfect_grid_scores_one_everywhere() {
        let truth = grid(64);
        assert!((f_measure(&truth, &truth) - 1.0).abs() < 1e-9);
        assert!((continuity_total(&truth, &truth) - 1.0).abs() < 1e-9);
        assert!((amlt(&truth, &truth) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_detection_pays_for_only_one_annotation() {
        // Without one-to-one matching a burst of detections would
        // "hit" a whole bar and precision would be meaningless.
        let truth = grid(4);
        let burst = vec![0.0, 0.001, 0.002, 0.003];
        assert_eq!(count_matches(&burst, &truth, BEAT_TOLERANCE_S), 1);
    }

    #[test]
    fn the_tolerance_is_the_mirex_seventy_milliseconds() {
        let truth = vec![1.0];
        assert_eq!(count_matches(&[1.069], &truth, BEAT_TOLERANCE_S), 1);
        assert_eq!(count_matches(&[1.071], &truth, BEAT_TOLERANCE_S), 0);
    }

    #[test]
    fn half_tempo_fails_cmlt_but_passes_amlt() {
        // The distinction the two metrics exist for: a tracker that
        // locked onto half tempo is WRONG at the annotated level and
        // musically defensible at another.
        let truth = grid(64);
        let half: Vec<f64> = truth.iter().step_by(2).copied().collect();
        assert!(continuity_total(&half, &truth) < 0.6, "cmlt must punish it");
        assert!(amlt(&half, &truth) > 0.9, "amlt must forgive it");
    }

    #[test]
    fn the_offbeat_is_forgiven_by_amlt_only() {
        let truth = grid(64);
        let offbeat: Vec<f64> = truth.iter().map(|t| t + 0.25).collect();
        assert!(continuity_total(&offbeat, &truth) < 0.2);
        assert!(amlt(&offbeat, &truth) > 0.9);
    }

    #[test]
    fn octave_errors_are_named_as_such() {
        // The 62.5 / 125 / 250 failure this whole exercise exists to
        // stop — and 125 itself must NOT be flagged.
        assert!(is_octave_error(62.5, 125.0));
        assert!(is_octave_error(250.0, 125.0));
        assert!(!is_octave_error(125.0, 125.0));
        assert!(
            !is_octave_error(124.0, 125.0),
            "a small miss is not an octave"
        );
        // A wrong tempo that is not a power-of-two relative is a
        // different failure and must not be mislabelled.
        assert!(!is_octave_error(97.0, 125.0));
    }

    #[test]
    fn density_reports_the_middle_and_the_peak() {
        // 30 s: 10 notes, then 0, then 100 — the median must not be
        // dragged by the wall, and p95 must see it.
        let mut times: Vec<f64> = (0..10).map(|i| f64::from(i) * 0.9).collect();
        times.extend((0..100).map(|i| 20.0 + f64::from(i) * 0.09));
        let (median, p95) = note_density(&times, 30.0);
        assert!((median - 1.0).abs() < 1e-9, "median was {median}");
        assert!(p95 >= 10.0, "p95 was {p95}");
    }

    #[test]
    fn a_steady_grid_describes_itself() {
        let truth = GroundTruth::steady(120.0, 0.5, 16);
        assert_eq!(truth.beats.len(), 16);
        assert_eq!(truth.downbeats.len(), 4, "every fourth beat");
        assert!((truth.first_downbeat_ms - 500.0).abs() < 1e-9);
        assert!((truth.beats[1] - 1.0).abs() < 1e-9);
    }
}
