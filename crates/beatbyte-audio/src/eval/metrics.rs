//! Quantitative metrics for the evaluation scenes.
//!
//! Everything here is a measurement against ground truth that the
//! scene placed itself, so the numbers are facts rather than
//! impressions. Matching is greedy nearest-neighbour within a
//! tolerance and is deterministic: sorted inputs, no hashing, no ties
//! broken by chance.

use super::scenes::{Role, Scene, TruthNote};

/// Default matching tolerance. 50 ms is the standard MIR onset
/// tolerance and also roughly BeatByte's Good window (100 ms), so a
/// match here means "the player would have hit it".
pub const TOLERANCE_S: f64 = 0.05;

/// A detected event, whatever produced it (onsets, melody, chart).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detected {
    /// When the detector says the event starts.
    pub time_s: f64,
    /// When it says the event ends (equal to `time_s` when unknown).
    pub end_s: f64,
    /// Pitch if the detector produced one.
    pub midi: Option<f32>,
}

/// The outcome of matching detections against ground truth.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchReport {
    /// Detections matched to a chartable truth event.
    pub hits: usize,
    /// Detections that matched nothing chartable.
    pub false_positives: usize,
    /// Chartable truth events with no detection.
    pub misses: usize,
    /// Mean absolute timing error over matched pairs, seconds.
    pub timing_error_s: f64,
    /// Fraction of matched pairs whose pitch is within half a
    /// semitone. `None` when no detection carried a pitch.
    pub pitch_accuracy: Option<f64>,
    /// Mean absolute length error over matched pairs where the truth
    /// event is a real sustain (≥ 0.4 s). `None` if there are none.
    pub sustain_error_s: Option<f64>,
    /// False positives that line up with a distractor (drum, voice,
    /// bass) — i.e. the detector charted the wrong instrument.
    pub distractor_hits: usize,
}

impl MatchReport {
    /// Fraction of detections that were real.
    #[must_use]
    pub fn precision(&self) -> f64 {
        let total = self.hits + self.false_positives;
        if total == 0 {
            return 1.0;
        }
        self.hits as f64 / total as f64
    }

    /// Fraction of chartable events that were found.
    #[must_use]
    pub fn recall(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 1.0;
        }
        self.hits as f64 / total as f64
    }

    /// Harmonic mean of precision and recall.
    #[must_use]
    pub fn f1(&self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r <= 0.0 {
            return 0.0;
        }
        2.0 * p * r / (p + r)
    }
}

/// Match detections against a scene's chartable events.
///
/// Greedy by detection time: each detection takes the nearest
/// unclaimed truth event inside `tolerance_s`. Greedy is the standard
/// choice here and, on sorted inputs, is deterministic — an optimal
/// assignment would change scores by fractions of a percent and cost
/// a solver.
#[must_use]
pub fn match_events(scene: &Scene, detected: &[Detected], tolerance_s: f64) -> MatchReport {
    let truth = scene.chartable();
    let distractors = scene.distractors();
    let mut claimed = vec![false; truth.len()];

    let mut hits = 0usize;
    let mut false_positives = 0usize;
    let mut distractor_hits = 0usize;
    let mut timing_error = 0.0f64;
    let mut pitched_pairs = 0usize;
    let mut pitch_correct = 0usize;
    let mut sustain_pairs = 0usize;
    let mut sustain_error = 0.0f64;

    let mut ordered: Vec<Detected> = detected.to_vec();
    ordered.sort_by(|a, b| {
        a.time_s
            .partial_cmp(&b.time_s)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    for event in &ordered {
        let mut best: Option<(usize, f64)> = None;
        for (i, candidate) in truth.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            let distance = (candidate.time_s - event.time_s).abs();
            if distance <= tolerance_s && best.is_none_or(|(_, d)| distance < d) {
                best = Some((i, distance));
            }
        }
        match best {
            Some((index, distance)) => {
                claimed[index] = true;
                hits += 1;
                timing_error += distance;
                let matched = truth[index];
                if let (Some(detected_midi), Some(true_midi)) = (event.midi, matched.midi) {
                    pitched_pairs += 1;
                    if (detected_midi - true_midi).abs() <= 0.5 {
                        pitch_correct += 1;
                    }
                }
                if matched.len_s() >= 0.4 && event.end_s > event.time_s {
                    sustain_pairs += 1;
                    sustain_error += ((event.end_s - event.time_s) - matched.len_s()).abs();
                }
            }
            None => {
                false_positives += 1;
                if distractors
                    .iter()
                    .any(|d| (d.time_s - event.time_s).abs() <= tolerance_s)
                {
                    distractor_hits += 1;
                }
            }
        }
    }

    MatchReport {
        hits,
        false_positives,
        misses: claimed.iter().filter(|c| !**c).count(),
        timing_error_s: if hits == 0 {
            0.0
        } else {
            timing_error / hits as f64
        },
        pitch_accuracy: (pitched_pairs > 0).then(|| pitch_correct as f64 / pitched_pairs as f64),
        sustain_error_s: (sustain_pairs > 0).then(|| sustain_error / sustain_pairs as f64),
        distractor_hits,
    }
}

/// How well a detected tempo matches the scene, tolerant of the
/// half/double-time ambiguity being *reported* but not of it being
/// chosen: `octave_error` says the pipeline picked the wrong pulse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoReport {
    /// Detected tempo.
    pub bpm: f64,
    /// Absolute error against the scene's true tempo.
    pub error_bpm: f64,
    /// Whether the detection is a half/double-time version of truth.
    pub octave_error: bool,
}

/// Compare a detected tempo to the scene's true tempo.
#[must_use]
pub fn tempo_report(scene: &Scene, bpm: f64) -> TempoReport {
    let error = (bpm - scene.bpm).abs();
    let octave = [0.5, 2.0, 0.25, 4.0]
        .iter()
        .any(|factor| (bpm - scene.bpm * factor).abs() < scene.bpm * factor * 0.06);
    TempoReport {
        bpm,
        error_bpm: error,
        octave_error: octave && error > scene.bpm * 0.06,
    }
}

/// Fraction of ground-truth sustains (≥ 0.4 s) that were represented
/// as a single detected event rather than shredded into several.
///
/// This is the "one held tone must not become five notes" measurement:
/// it counts detections whose start falls strictly inside a truth
/// sustain, well after its attack.
#[must_use]
pub fn sustain_fragmentation(scene: &Scene, detected: &[Detected]) -> f64 {
    let holds: Vec<TruthNote> = scene
        .chartable()
        .into_iter()
        .filter(|note| note.len_s() >= 0.4)
        .collect();
    if holds.is_empty() {
        return 0.0;
    }
    let mut extra = 0usize;
    for event in detected {
        for hold in &holds {
            // Ignore the attack region: that detection IS the note.
            if event.time_s > hold.time_s + 0.12 && event.time_s < hold.end_s - 0.05 {
                extra += 1;
                break;
            }
        }
    }
    extra as f64 / holds.len() as f64
}

/// Share of detections that sit on a distractor and on nothing else —
/// the "drums/vocals hijacked the chart" number.
#[must_use]
pub fn contamination(scene: &Scene, detected: &[Detected], tolerance_s: f64) -> f64 {
    if detected.is_empty() {
        return 0.0;
    }
    let lead = scene.chartable();
    let distractors = scene.distractors();
    let wrong = detected
        .iter()
        .filter(|event| {
            let near_lead = lead
                .iter()
                .any(|n| (n.time_s - event.time_s).abs() <= tolerance_s);
            let near_distractor = distractors
                .iter()
                .filter(|n| n.role != Role::Bass)
                .any(|n| (n.time_s - event.time_s).abs() <= tolerance_s);
            !near_lead && near_distractor
        })
        .count();
    wrong as f64 / detected.len() as f64
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::eval::scenes;

    fn detection(time_s: f64, end_s: f64, midi: Option<f32>) -> Detected {
        Detected {
            time_s,
            end_s,
            midi,
        }
    }

    #[test]
    fn perfect_detection_scores_perfectly() {
        let scene = scenes::simple_melody();
        let detected: Vec<Detected> = scene
            .chartable()
            .iter()
            .map(|n| detection(n.time_s, n.end_s, n.midi))
            .collect();
        let report = match_events(&scene, &detected, TOLERANCE_S);
        assert_eq!(report.misses, 0);
        assert_eq!(report.false_positives, 0);
        assert!((report.f1() - 1.0).abs() < 1e-9);
        assert!(report.timing_error_s < 1e-9);
        assert_eq!(report.pitch_accuracy, Some(1.0));
    }

    #[test]
    fn a_missing_half_halves_recall() {
        let scene = scenes::simple_melody();
        let truth = scene.chartable();
        let detected: Vec<Detected> = truth
            .iter()
            .step_by(2)
            .map(|n| detection(n.time_s, n.end_s, n.midi))
            .collect();
        let report = match_events(&scene, &detected, TOLERANCE_S);
        assert!(
            (report.recall() - 0.5).abs() < 0.06,
            "recall {} should be about half",
            report.recall()
        );
        assert!((report.precision() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn detections_on_drums_are_counted_as_contamination() {
        let scene = scenes::drums_and_guitar();
        // Chart the KIT instead of the guitar. Only percussion: the
        // scene's bass deliberately doubles the guitar's downbeats,
        // so "detections on the bass" are largely detections on the
        // lead and prove nothing about contamination.
        let detected: Vec<Detected> = scene
            .distractors()
            .iter()
            .filter(|n| n.role == Role::Percussion)
            .map(|n| detection(n.time_s, n.time_s, None))
            .collect();
        let report = match_events(&scene, &detected, TOLERANCE_S);
        assert!(
            report.distractor_hits > 0,
            "drum-only detections must register as distractor hits"
        );
        assert!(
            contamination(&scene, &detected, TOLERANCE_S) > 0.3,
            "charting the kit should read as heavy contamination"
        );
    }

    #[test]
    fn shredding_a_sustain_is_measured() {
        let scene = scenes::sustains();
        let truth = scene.chartable();
        // One detection at the attack (fine) plus four inside the
        // hold (the failure mode this metric exists for).
        let mut detected = Vec::new();
        for note in &truth {
            for k in 0..5 {
                let t = note.time_s + f64::from(k) * 0.3;
                detected.push(detection(t, t, note.midi));
            }
        }
        let fragmentation = sustain_fragmentation(&scene, &detected);
        assert!(
            fragmentation >= 3.0,
            "five detections per hold should read as heavy fragmentation, got {fragmentation}"
        );
        // The honest version: one detection per hold, full length.
        let clean: Vec<Detected> = truth
            .iter()
            .map(|n| detection(n.time_s, n.end_s, n.midi))
            .collect();
        assert!(sustain_fragmentation(&scene, &clean).abs() < 1e-9);
    }

    #[test]
    fn tempo_octave_errors_are_named() {
        let scene = scenes::tempo_ambiguity(); // 150 BPM
        assert!(!tempo_report(&scene, 150.4).octave_error);
        assert!((tempo_report(&scene, 150.4).error_bpm - 0.4).abs() < 1e-9);
        let half = tempo_report(&scene, 75.0);
        assert!(half.octave_error, "75 BPM against 150 is an octave error");
    }

    #[test]
    fn sustain_length_error_is_measured_only_on_real_holds() {
        let scene = scenes::sustains();
        let truth = scene.chartable();
        // Report every hold as 0.3 s long: a large, obvious error.
        let detected: Vec<Detected> = truth
            .iter()
            .map(|n| detection(n.time_s, n.time_s + 0.3, n.midi))
            .collect();
        let report = match_events(&scene, &detected, TOLERANCE_S);
        let error = report.sustain_error_s.unwrap();
        assert!(error > 0.8, "truncating every hold must show up: {error}");
    }
}
