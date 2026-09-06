//! How a model's answer enters the analysis the chart is built from.
//!
//! Two policies, because the corpus decides between them
//! (`docs/audio-eval-baseline.md`): the analyzer's own tracked grid
//! with only the model's *downbeats* placed on it, or the model's
//! whole grid. Pure and tested.

use beatbyte_core::music::SongAnalysis;

use crate::Meter;
use crate::peaks::snap_to;

/// What the meter is allowed to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Keep the analyzer's beats; each model downbeat moves onto the
    /// nearest of them. The tempo estimate stays.
    Downbeats,
    /// The model's beats and downbeats replace the analyzer's grid;
    /// the tempo is re-read from the model's median beat interval.
    Grid,
}

/// Tempo ratio within which the model and the tracker read the SAME
/// grid. Two readings of one grid differ by under 2 % (the corpus's
/// eleven, the library's 41 adopted songs); the smallest
/// disagreement seen on the library is 6 % and most are a metrical
/// level apart — a factor 4/3, 3/2 or 2 (2.05, 1.97, 2.06, 1.48,
/// 0.75 on fast rock and a shuffle). The same number `redesign`
/// merges charts within, so an import and a rollover decide alike.
pub const LEVEL_TOLERANCE: f64 = 0.05;

/// Whether `model_bpm` and `tracker_bpm` read the same grid (within
/// [`LEVEL_TOLERANCE`]). A non-positive tempo agrees with nothing.
/// Pure — tested, and it is the decision [`apply`] hinges on.
#[must_use]
pub fn same_level(tracker_bpm: f64, model_bpm: f64) -> bool {
    if tracker_bpm <= 0.0 || model_bpm <= 0.0 {
        return false;
    }
    let ratio = model_bpm / tracker_bpm;
    (1.0 / (1.0 + LEVEL_TOLERANCE)..=1.0 + LEVEL_TOLERANCE).contains(&ratio)
}

/// What [`apply`] did with the meter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// The policy was applied.
    Adopted,
    /// The model and the tracker disagree on the tempo — on the
    /// library mostly by a metrical level; the analysis is untouched.
    /// Nothing of the model is taken, because a downbeat at the
    /// wrong level is a half-bar or a bar and a half, not a bar.
    Disagreement {
        /// The tracker's tempo.
        tracker_bpm: f64,
        /// The model's tempo (median beat interval).
        model_bpm: f64,
    },
    /// The model heard too little to read a tempo from.
    NoTempo,
}

/// The shipped rule: apply `policy` when the model reads the same
/// metrical level as the tracker, and leave the analysis alone when
/// it does not. Measured (`docs/audio-eval-baseline.md`): on the
/// corpus the two agree and the model wins; on the user's rock
/// library the model hears double time on four songs whose charts
/// the ear had approved at the tracker's level.
pub fn apply(analysis: &mut SongAnalysis, meter: &Meter, policy: Policy) -> Outcome {
    let Some(model_bpm) = grid_bpm(&meter.beats) else {
        return Outcome::NoTempo;
    };
    if !same_level(analysis.bpm, model_bpm) {
        return Outcome::Disagreement {
            tracker_bpm: analysis.bpm,
            model_bpm,
        };
    }
    merge(analysis, meter, policy);
    Outcome::Adopted
}

/// Apply a policy unconditionally. A meter without beats changes
/// nothing under [`Policy::Grid`]; one without downbeats leaves the
/// downbeats empty. The corpus example measures policies with this;
/// the pipeline goes through [`apply`].
pub fn merge(analysis: &mut SongAnalysis, meter: &Meter, policy: Policy) {
    match policy {
        Policy::Downbeats => {
            analysis.downbeats = snap_to(&analysis.beats, &meter.downbeats);
        }
        Policy::Grid => {
            if meter.beats.len() < 2 {
                return;
            }
            analysis.beats = meter.beats.clone();
            analysis.downbeats = snap_to(&meter.beats, &meter.downbeats);
            if let Some(bpm) = grid_bpm(&meter.beats) {
                analysis.bpm = bpm;
                analysis.alt_bpm = None;
            }
        }
    }
}

/// Tempo from the beat intervals: the mean of the intervals within
/// half of the median (a breakdown the model heard nothing in is a
/// gap, not a slow beat); `None` under two beats or with a
/// non-positive interval. The mean rather than the median because
/// the model's beats sit on 20 ms frames — a median interval is one
/// of 0.48 or 0.50 s, which reads 125 or 120 BPM for a track at
/// 122; averaged over a song's beats the quantisation cancels.
#[must_use]
pub fn grid_bpm(beats: &[f64]) -> Option<f64> {
    if beats.len() < 2 {
        return None;
    }
    let mut intervals: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_by(f64::total_cmp);
    let median = intervals[intervals.len() / 2];
    if median <= 0.0 {
        return None;
    }
    let steady: Vec<f64> = intervals
        .iter()
        .copied()
        .filter(|i| *i >= 0.5 * median && *i <= 1.5 * median)
        .collect();
    let mean = steady.iter().sum::<f64>() / steady.len() as f64;
    (mean > 0.0).then(|| 60.0 / mean)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn analysis() -> SongAnalysis {
        SongAnalysis {
            repeats: Vec::new(),
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: Some(60.0),
            beats: (0..8).map(|i| f64::from(i) * 0.5).collect(),
            downbeats: Vec::new(),
            onsets: Vec::new(),
            energy: Vec::new(),
            energy_hop_s: 0.05,
            duration_s: 4.0,
            melody: Vec::new(),
        }
    }

    fn meter() -> Meter {
        Meter {
            beats: (0..8).map(|i| 0.1 + f64::from(i) * 0.48).collect(),
            downbeats: vec![0.1, 2.02],
            model: "beat-this-small",
            sha256: "00",
        }
    }

    #[test]
    fn the_downbeat_policy_keeps_the_grid_and_places_the_bars_on_it() {
        let mut a = analysis();
        merge(&mut a, &meter(), Policy::Downbeats);
        assert_eq!(a.beats, analysis().beats, "the analyzer's beats stay");
        assert_eq!(a.downbeats, vec![0.0, 2.0], "each downbeat sits on a beat");
        assert_eq!(a.bpm, 120.0);
    }

    #[test]
    fn the_grid_policy_takes_the_models_beats_and_reads_the_tempo_from_them() {
        let mut a = analysis();
        merge(&mut a, &meter(), Policy::Grid);
        assert_eq!(a.beats, meter().beats);
        assert_eq!(a.downbeats, vec![0.1, 0.1 + 4.0 * 0.48]);
        assert!((a.bpm - 125.0).abs() < 1e-9, "60 / 0.48");
        assert_eq!(a.alt_bpm, None);
    }

    #[test]
    fn the_level_decision_is_a_ratio_and_the_rule_hinges_on_it() {
        // One grid, two readings (the corpus: within 2 %).
        assert!(same_level(120.0, 120.9));
        assert!(same_level(112.51, 112.0));
        assert!(same_level(123.03, 125.0));
        assert!(same_level(100.0, 104.0));
        assert!(same_level(100.0, 96.0));
        // The library's disagreements: a level apart, and the two
        // smallest (6 % and 11 %) that are no reading of one grid.
        assert!(!same_level(112.51, 230.77));
        assert!(!same_level(112.51, 166.67));
        assert!(!same_level(95.34, 187.5));
        assert!(!same_level(117.19, 88.24));
        assert!(!same_level(133.71, 148.72));
        assert!(!same_level(116.32, 123.33));
        assert!(!same_level(0.0, 120.0));
        assert!(!same_level(120.0, 0.0));

        // The rule: adopted at the same level, untouched a level apart.
        let mut a = analysis();
        assert_eq!(apply(&mut a, &meter(), Policy::Grid), Outcome::Adopted);
        assert_eq!(a.beats, meter().beats);

        let mut a = analysis();
        let doubled = Meter {
            beats: (0..16).map(|i| f64::from(i) * 0.25).collect(),
            downbeats: vec![0.0, 1.0, 2.0, 3.0],
            model: "beat-this",
            sha256: "00",
        };
        assert_eq!(
            apply(&mut a, &doubled, Policy::Grid),
            Outcome::Disagreement {
                tracker_bpm: 120.0,
                model_bpm: 240.0
            }
        );
        assert_eq!(a, analysis(), "nothing of the model is taken");
        let mut a = analysis();
        assert_eq!(
            apply(&mut a, &doubled, Policy::Downbeats),
            Outcome::Disagreement {
                tracker_bpm: 120.0,
                model_bpm: 240.0
            }
        );
        assert!(a.downbeats.is_empty(), "not even the downbeats");

        let mut a = analysis();
        let silent = Meter {
            beats: vec![1.0],
            downbeats: vec![],
            model: "beat-this",
            sha256: "00",
        };
        assert_eq!(apply(&mut a, &silent, Policy::Grid), Outcome::NoTempo);
    }

    #[test]
    fn a_meter_that_heard_no_beats_changes_no_grid() {
        let mut a = analysis();
        let silent = Meter {
            beats: vec![],
            downbeats: vec![],
            model: "x",
            sha256: "00",
        };
        merge(&mut a, &silent, Policy::Grid);
        assert_eq!(a, analysis());
        assert_eq!(grid_bpm(&[]), None);
        assert_eq!(grid_bpm(&[1.0]), None);
        assert_eq!(grid_bpm(&[1.0, 1.0]), None);
        assert_eq!(grid_bpm(&[0.0, 0.5, 1.0, 1.5]), Some(120.0));
        // Frame-quantised intervals (0.48 / 0.50 s alternating, twenty
        // of each) read 122.4 BPM on average, not the median's 120.
        let quantised: Vec<f64> = (0..41)
            .scan(0.0, |t, i| {
                *t += if i % 2 == 0 { 0.48 } else { 0.50 };
                Some(*t)
            })
            .collect();
        let bpm = grid_bpm(&quantised).expect("a tempo");
        assert!((bpm - 60.0 / 0.49).abs() < 1e-6, "{bpm}");
        // A gap where the model heard nothing does not slow the tempo.
        let with_gap = [0.0, 0.5, 1.0, 1.5, 9.0, 9.5, 10.0];
        assert_eq!(grid_bpm(&with_gap), Some(120.0));
    }
}
