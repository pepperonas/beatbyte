//! Confidence gating and fallback (plan milestone L3): an alignment
//! must never ship a worse experience than the line-level lyrics the
//! player already had.
//!
//! Three questions, in order:
//!
//! 1. **Is the source's timeline this audio's?** Line stamps that run
//!    past the end of the file, or a delta that grows *cleanly* along
//!    the song (a straight line with little scatter — a stretched or
//!    otherwise re-timed edit), mean a *different edit*. Then the
//!    source is no reference at all and only the aligned times can be
//!    used. (Measured on the way here: lrclib's stamps for a
//!    248-second file ran to 272 s.) A delta that wanders without
//!    being a line is not this case — it is the aligner losing the
//!    vocal, and that is a failed alignment.
//! 2. **Does the alignment agree with the source, up to a constant?**
//!    When most lines sit within a tolerance of the median delta,
//!    there is a consensus: the aligned times are kept, the median
//!    is the master shift (a different master of the same edit when
//!    it is large), and the lines outside the tolerance fall back
//!    one by one. When fewer than half agree there is no consensus:
//!    the alignment failed, and every line falls back to the
//!    source's own stamps. Measured on four songs: a pop mix with
//!    stacked choruses kept 64 % consensus and lost only the stacks;
//!    a rock mix the model could not follow had 13 % and fell back
//!    whole — which was right, its stamps fit the file.
//! 3. **Which words and lines are not to be trusted?** A word the
//!    Viterbi sprinted through — one frame per letter, the minimum it
//!    is allowed — carries no acoustic evidence; a word held for
//!    longer than any sung word is a path that got stuck. Such words
//!    are kept, marked `estimated`, and timed between their trusted
//!    neighbours. A line with more than a share of them falls back to
//!    line level — on the source's stamp when the source is a
//!    reference, on its own aligned span otherwise. Per line, never
//!    per song: a chorus that aligned is not thrown away for a
//!    mumbled bridge.
//!
//! Every threshold is a field of [`GateConfig`] with a default that
//! is an assumption, not a measurement — the corpus harness (L5) is
//! where they get calibrated. The word-confidence floor defaults to
//! *off*: on a full mix the model's per-letter probabilities are low
//! even where the timing is right (0.01–0.16 on a correctly aligned
//! song), so a floor would discard good timings; the structural
//! signals above do not have that problem.

use serde::{Deserialize, Serialize};

use crate::emissions::FRAME_S;
use crate::transcript::Transcript;
use crate::words::{AlignedLine, AlignedWord, Alignment};

/// The thresholds. Defaults are assumptions to be calibrated (L5).
#[derive(Debug, Clone, PartialEq)]
pub struct GateConfig {
    /// A word with confidence below this is estimated. `0.0` = off.
    pub word_conf_floor: f32,
    /// A word longer than this many seconds is estimated.
    pub max_word_s: f64,
    /// A word within this many frames of the Viterbi's minimum
    /// duration (one frame per letter) is estimated: no evidence.
    pub sprint_slack_frames: usize,
    /// More than this share of a line's words estimated → the line
    /// falls back to line level.
    pub line_fallback_share: f32,
    /// |median delta| beyond this is a shifted master.
    pub master_shift_s: f64,
    /// No more than this share of the compared lines within
    /// `line_outlier_s` of the median delta is a failed alignment:
    /// a consensus is a majority, and half is not one.
    pub consensus_share: f32,
    /// A line whose delta is this far from the median falls back.
    pub line_outlier_s: f64,
    /// Scatter a clean drift may leave after its straight line is
    /// removed; more, and the drift is not clean.
    pub drift_residual_s: f64,
    /// Source stamps beyond the audio's length plus this are a
    /// different edit.
    pub edit_slack_s: f64,
    /// A delta that changes by more than this across the compared
    /// span — and is explained by a straight line, not by noise — is
    /// a different edit.
    pub edit_drift_s: f64,
}

impl Default for GateConfig {
    fn default() -> GateConfig {
        GateConfig {
            word_conf_floor: 0.0,
            max_word_s: 5.0,
            sprint_slack_frames: 0,
            line_fallback_share: 0.30,
            master_shift_s: 1.5,
            consensus_share: 0.5,
            line_outlier_s: 1.5,
            drift_residual_s: 0.75,
            edit_slack_s: 2.0,
            edit_drift_s: 1.0,
        }
    }
}

/// What the source's stamps are worth as a reference.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    /// The source carried no line stamps; only the aligned times
    /// exist.
    NoReference,
    /// Aligned and source agree: same edit, same master.
    SameMaster,
    /// Aligned and source agree up to a constant: the source was
    /// stamped against another master of this edit. Aligned times are
    /// kept; the shift is what a lyric offset would have been.
    ShiftedMaster {
        /// Aligned minus source, seconds.
        offset_s: f64,
    },
    /// The source's timeline is not this file's (stamps beyond the
    /// audio, or a delta that grows). Aligned times are kept; the
    /// source cannot serve as a fallback.
    DifferentEdit,
    /// Fewer than half the lines agree on a delta: the alignment
    /// failed. Every line falls back to the source's stamps.
    Failed,
}

/// What the gate did, for the report and the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateReport {
    /// The source's standing.
    #[serde(flatten)]
    pub verdict: Verdict,
    /// Lines that had a source stamp AND aligned letters to compare.
    pub lines_compared: usize,
    /// Of those, the share within tolerance of the median delta.
    pub consensus: Option<f32>,
    /// Median of aligned minus source over those lines, seconds.
    pub median_delta_s: Option<f64>,
    /// Median absolute deviation of the deltas, seconds.
    pub mad_s: Option<f64>,
    /// Words newly marked estimated by the word rules.
    pub words_estimated: usize,
    /// Lines that fell back to line level.
    pub lines_fallen_back: usize,
}

/// Apply the gate to an alignment in place. `audio_duration_s` is the
/// length of the file the alignment was computed on.
pub fn gate(
    alignment: &mut Alignment,
    transcript: &Transcript,
    audio_duration_s: f64,
    config: &GateConfig,
) -> GateReport {
    // 1 + 2: the verdict, from the deltas against the source stamps.
    // A line without aligned letters (`♪`, a number) has a guessed
    // start, not an aligned one — it says nothing about the source.
    let stamps: Vec<Option<f64>> = transcript.lines.iter().map(|l| l.source_start_s).collect();
    let pairs: Vec<(f64, f64)> = alignment
        .lines
        .iter()
        .zip(&stamps)
        .filter(|(line, _)| line.words.iter().any(|w| !w.estimated))
        .filter_map(|(line, stamp)| stamp.map(|s| (s, line.start - s)))
        .collect();
    let judged = verdict_of(&pairs, audio_duration_s, config);
    let (verdict, median, mad) = (judged.verdict, judged.median, judged.mad);

    // 3a: words.
    let mut words_estimated = 0usize;
    for line in &mut alignment.lines {
        for word in &mut line.words {
            if !word.estimated && word_is_suspect(word, config) {
                word.estimated = true;
                word.conf = 0.0;
                word.chars.clear();
                words_estimated += 1;
            }
        }
    }

    // 3b: lines.
    let mut lines_fallen_back = 0usize;
    let source_is_reference = matches!(
        verdict,
        Verdict::SameMaster | Verdict::ShiftedMaster { .. } | Verdict::Failed
    );
    // The shift the source's stamps need to land on this master. A
    // failed alignment's median says nothing: its stamps are used as
    // they are.
    let shift = if verdict == Verdict::Failed {
        0.0
    } else {
        median.unwrap_or(0.0)
    };
    let line_count = alignment.lines.len();
    for i in 0..line_count {
        let stamp = stamps.get(i).copied().flatten();
        let delta = stamp.map(|s| alignment.lines[i].start - s);
        let outlier =
            delta.is_some_and(|d| (d - shift).abs() > config.line_outlier_s) && source_is_reference;
        let share = {
            let line = &alignment.lines[i];
            if line.words.is_empty() {
                0.0
            } else {
                line.words.iter().filter(|w| w.estimated).count() as f32 / line.words.len() as f32
            }
        };
        let falls_back =
            verdict == Verdict::Failed || outlier || share > config.line_fallback_share;
        if !falls_back {
            continue;
        }
        lines_fallen_back += 1;
        // Where the line goes: the source's stamp (shifted onto this
        // master) when the source is a reference, else its own span.
        let next_stamp = stamps.get(i + 1).copied().flatten();
        let (start, end) = match (source_is_reference, stamp) {
            (true, Some(s)) => {
                let start = s + shift;
                let end = next_stamp
                    .map(|n| n + shift)
                    .unwrap_or(start + 3.0)
                    .max(start + 0.5);
                (start, end)
            }
            _ => (
                alignment.lines[i].start,
                alignment.lines[i].end.max(alignment.lines[i].start + 0.5),
            ),
        };
        let line = &mut alignment.lines[i];
        line.start = start;
        line.end = end;
        let n = line.words.len().max(1) as f64;
        for (k, word) in line.words.iter_mut().enumerate() {
            word.estimated = true;
            word.conf = 0.0;
            word.chars.clear();
            word.start = start + (end - start) * k as f64 / n;
            word.end = start + (end - start) * (k as f64 + 1.0) / n;
        }
    }
    // Re-time estimated words inside lines that kept their alignment.
    for line in &mut alignment.lines {
        retime_estimated_within(line);
    }

    let report = GateReport {
        verdict,
        lines_compared: pairs.len(),
        consensus: judged.consensus,
        median_delta_s: median,
        mad_s: mad,
        words_estimated,
        lines_fallen_back,
    };
    alignment.gate = Some(report.clone());
    report
}

/// A word the alignment does not vouch for.
fn word_is_suspect(word: &AlignedWord, config: &GateConfig) -> bool {
    let duration = word.end - word.start;
    if duration > config.max_word_s {
        return true;
    }
    let letters = word.chars.len();
    if letters > 0 {
        let minimum = (letters + config.sprint_slack_frames) as f64 * FRAME_S;
        if duration <= minimum + 1e-9 {
            return true;
        }
    }
    config.word_conf_floor > 0.0 && word.conf < config.word_conf_floor
}

/// What [`verdict_of`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Judged {
    /// The verdict.
    pub verdict: Verdict,
    /// Median of aligned minus source, when there were pairs.
    pub median: Option<f64>,
    /// Median absolute deviation of the deltas.
    pub mad: Option<f64>,
    /// Share of pairs within `line_outlier_s` of the median.
    pub consensus: Option<f32>,
}

/// The verdict from `(source stamp, aligned − source)` pairs. Pure —
/// tested.
#[must_use]
pub fn verdict_of(pairs: &[(f64, f64)], audio_duration_s: f64, config: &GateConfig) -> Judged {
    let judged = |verdict, median, mad, consensus| Judged {
        verdict,
        median,
        mad,
        consensus,
    };
    if pairs.is_empty() {
        return judged(Verdict::NoReference, None, None, None);
    }
    let last_stamp = pairs.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    if last_stamp > audio_duration_s + config.edit_slack_s {
        return judged(Verdict::DifferentEdit, None, None, None);
    }
    let mut deltas: Vec<f64> = pairs.iter().map(|p| p.1).collect();
    deltas.sort_by(f64::total_cmp);
    let median = deltas[deltas.len() / 2];
    let mut deviations: Vec<f64> = deltas.iter().map(|d| (d - median).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    let mad = deviations[deviations.len() / 2];
    let within = deltas
        .iter()
        .filter(|d| (*d - median).abs() <= config.line_outlier_s)
        .count();
    let consensus = within as f32 / deltas.len() as f32;
    // A delta that grows with the song: a least-squares line through
    // (source time, delta). It counts only when the line EXPLAINS the
    // deltas — the drift across the compared span is large and what
    // is left after removing it is small. Noise has a slope too; it
    // does not have a small residual.
    if pairs.len() >= 4 {
        let n = pairs.len() as f64;
        let mean_t = pairs.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_d = pairs.iter().map(|p| p.1).sum::<f64>() / n;
        let cov: f64 = pairs.iter().map(|p| (p.0 - mean_t) * (p.1 - mean_d)).sum();
        let var: f64 = pairs.iter().map(|p| (p.0 - mean_t).powi(2)).sum();
        if var > 0.0 {
            let slope = cov / var;
            let (min_t, max_t) = pairs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.0), hi.max(p.0))
            });
            let drift = slope * (max_t - min_t);
            let mut residuals: Vec<f64> = pairs
                .iter()
                .map(|p| (p.1 - (mean_d + slope * (p.0 - mean_t))).abs())
                .collect();
            residuals.sort_by(f64::total_cmp);
            let residual_mad = residuals[residuals.len() / 2];
            if drift.abs() > config.edit_drift_s
                && residual_mad <= config.drift_residual_s
                && drift.abs() > 4.0 * residual_mad
            {
                return judged(
                    Verdict::DifferentEdit,
                    Some(median),
                    Some(mad),
                    Some(consensus),
                );
            }
        }
    }
    if consensus <= config.consensus_share {
        return judged(Verdict::Failed, Some(median), Some(mad), Some(consensus));
    }
    if median.abs() > config.master_shift_s {
        return judged(
            Verdict::ShiftedMaster { offset_s: median },
            Some(median),
            Some(mad),
            Some(consensus),
        );
    }
    judged(
        Verdict::SameMaster,
        Some(median),
        Some(mad),
        Some(consensus),
    )
}

/// Seconds an estimated word gets when it has only one trusted
/// neighbour (the start or the end of a line) — the same allowance
/// the aligner gives letterless words. A trailing word must NOT take
/// the line's end: the Viterbi stretches the last word of a line
/// over the silence after it, and that stretch is the very thing
/// that got the word marked.
const LONE_ESTIMATED_S: f64 = 0.3;

/// Estimated words inside a line that kept its alignment take an even
/// share of the gap between the trusted words around them; at the
/// line's edges they get [`LONE_ESTIMATED_S`] each.
fn retime_estimated_within(line: &mut AlignedLine) {
    let n = line.words.len();
    let mut i = 0;
    while i < n {
        if !line.words[i].estimated {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < n && line.words[i].estimated {
            i += 1;
        }
        let run_end = i;
        let before = run_start.checked_sub(1).map(|j| line.words[j].end);
        let after = (run_end < n).then(|| line.words[run_end].start);
        let count = (run_end - run_start) as f64;
        let (from, to) = match (before, after) {
            (Some(b), Some(a)) => (b, a.max(b)),
            (Some(b), None) => (b, b + LONE_ESTIMATED_S * count),
            (None, Some(a)) => ((a - LONE_ESTIMATED_S * count).max(0.0), a),
            (None, None) => (line.start, line.end),
        };
        for (k, j) in (run_start..run_end).enumerate() {
            let word = &mut line.words[j];
            word.start = from + (to - from) * k as f64 / count;
            word.end = from + (to - from) * (k as f64 + 1.0) / count;
        }
    }
    if let (Some(first), Some(last)) = (line.words.first(), line.words.last()) {
        line.start = first.start;
        line.end = last.end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::words::{SCHEMA, Source};

    fn word(text: &str, start: f64, end: f64, conf: f32, letters: usize) -> AlignedWord {
        let step = (end - start) / letters.max(1) as f64;
        AlignedWord {
            text: text.to_owned(),
            start,
            end,
            conf,
            estimated: false,
            chars: (0..letters)
                .map(|k| [start + step * k as f64, start + step * (k + 1) as f64])
                .collect(),
        }
    }

    fn alignment(lines: Vec<AlignedLine>) -> Alignment {
        Alignment {
            schema: SCHEMA.to_owned(),
            audio_sha256: "00".repeat(32),
            pipeline_version: 1,
            language: "en".to_owned(),
            source: Source {
                text: "test".to_owned(),
                separator: "none".to_owned(),
                aligner: "test".to_owned(),
            },
            offset_ms: 0,
            gate: None,
            lines,
        }
    }

    fn line(start: f64, words: Vec<AlignedWord>) -> AlignedLine {
        let end = words.last().map_or(start, |w| w.end);
        AlignedLine {
            start,
            end,
            text: words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words,
        }
    }

    fn cfg() -> GateConfig {
        GateConfig::default()
    }

    #[test]
    fn the_verdict_reads_the_deltas() {
        let c = cfg();
        assert_eq!(verdict_of(&[], 200.0, &c).verdict, Verdict::NoReference);
        // Consistent, small: same master.
        let same: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 10.0, 0.1)).collect();
        assert_eq!(verdict_of(&same, 200.0, &c).verdict, Verdict::SameMaster);
        // Consistent, large: shifted master, offset reported.
        let shifted: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 10.0, -2.4)).collect();
        assert!(matches!(
            verdict_of(&shifted, 200.0, &c).verdict,
            Verdict::ShiftedMaster { offset_s } if (offset_s + 2.4).abs() < 1e-9
        ));
        // Inconsistent: failed.
        let noisy: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 10.0, if i % 2 == 0 { -8.0 } else { 6.0 }))
            .collect();
        assert_eq!(verdict_of(&noisy, 200.0, &c).verdict, Verdict::Failed);
        // Stamps past the end of the file: a different edit — the
        // Blondie case, 272 s of stamps in a 248-second file.
        let long: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 30.0, 0.0)).collect();
        assert_eq!(verdict_of(&long, 248.0, &c).verdict, Verdict::DifferentEdit);
        // A delta that grows along the song, wider than the noise
        // band: a different edit too, even inside the file's length.
        let growing: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 20.0, -0.4 * i as f64))
            .collect();
        assert_eq!(
            verdict_of(&growing, 250.0, &c).verdict,
            Verdict::DifferentEdit
        );
        // A small tilt inside a consistent band is NOT a drift: 0.3 s
        // across the span stays the same master.
        let tilted: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 20.0, 0.1 + 0.0017 * i as f64 * 20.0))
            .collect();
        assert_eq!(verdict_of(&tilted, 250.0, &c).verdict, Verdict::SameMaster);
        // A majority that agrees carries the song even when a third
        // of the lines wander (the stacked-chorus pop case): same
        // master, and the wanderers are the per-line business.
        let mostly: Vec<(f64, f64)> = (0..12)
            .map(|i| (i as f64 * 15.0, if i % 3 == 2 { -5.0 } else { -1.0 }))
            .collect();
        let j = verdict_of(&mostly, 200.0, &c);
        assert_eq!(j.verdict, Verdict::SameMaster);
        assert!((j.consensus.unwrap_or(0.0) - 8.0 / 12.0).abs() < 1e-6);
        // A delta that wanders WITHOUT being a straight line is the
        // aligner losing the vocal, not a different edit: failed.
        let lost: Vec<(f64, f64)> = (0..12)
            .map(|i| {
                let t = i as f64 * 20.0;
                (t, -5.0 - 0.15 * t + if i % 2 == 0 { 4.0 } else { -4.0 })
            })
            .collect();
        assert_eq!(verdict_of(&lost, 250.0, &c).verdict, Verdict::Failed);
    }

    #[test]
    fn a_line_without_letters_does_not_vote() {
        // Two aligned lines agree with the source; a `♪` line between
        // them has only an estimated word at a guessed time 20 s off.
        // It must not turn the verdict.
        let mut a = alignment(vec![
            line(10.0, vec![word("ab", 10.0, 10.5, 0.5, 2)]),
            line(
                40.0,
                vec![AlignedWord {
                    text: "♪".to_owned(),
                    start: 40.0,
                    end: 40.3,
                    conf: 0.0,
                    estimated: true,
                    chars: Vec::new(),
                }],
            ),
            line(30.0, vec![word("cd", 30.0, 30.5, 0.5, 2)]),
        ]);
        let t = Transcript::parse("[00:10.00]ab\n[00:20.00]♪\n[00:30.00]cd");
        let report = gate(&mut a, &t, 200.0, &cfg());
        assert_eq!(report.lines_compared, 2);
        assert_eq!(report.verdict, Verdict::SameMaster);
    }

    #[test]
    fn a_sprinted_word_and_an_endless_word_are_estimated_and_retimed() {
        // "ab" got exactly one frame per letter (40 ms): a sprint.
        // "loooong" spans 6 s. "cd" is fine.
        let mut a = alignment(vec![line(
            10.0,
            vec![
                word("cd", 10.0, 10.4, 0.5, 2),
                word("ab", 10.4, 10.44, 0.5, 2),
                word("ef", 11.0, 11.5, 0.5, 2),
                word("loooong", 11.5, 17.5, 0.5, 7),
                word("gh", 17.5, 18.0, 0.5, 2),
                word("ij", 18.0, 18.5, 0.5, 2),
                word("kl", 18.5, 19.0, 0.5, 2),
            ],
        )]);
        let t = Transcript::parse("cd ab ef loooong gh ij kl");
        let report = gate(&mut a, &t, 200.0, &cfg());
        assert_eq!(report.verdict, Verdict::NoReference);
        assert_eq!(report.words_estimated, 2);
        assert_eq!(
            report.lines_fallen_back, 0,
            "2 of 7 is under the 30 % share"
        );
        let w = &a.lines[0].words;
        assert!(w[1].estimated && w[1].chars.is_empty());
        assert!(
            (w[1].start - 10.4).abs() < 1e-9 && (w[1].end - 11.0).abs() < 1e-9,
            "between cd and ef: {:?}",
            w[1]
        );
        assert!(w[3].estimated);
        assert!((w[3].start - 11.5).abs() < 1e-9 && (w[3].end - 17.5).abs() < 1e-9);
        assert!([0, 2, 4, 5, 6].iter().all(|&i| !w[i].estimated));
        // A stretched LAST word is retimed to a short span after its
        // neighbour, not to the line's end - the stretch was the
        // symptom (seen live: "remember" held for ten seconds of
        // instrumental, and the line never dimmed).
        let mut a = alignment(vec![line(
            10.0,
            vec![
                word("ab", 10.0, 10.4, 0.5, 2),
                word("cd", 10.4, 10.8, 0.5, 2),
                word("ef", 10.8, 11.2, 0.5, 2),
                word("remember", 11.2, 21.2, 0.5, 8),
            ],
        )]);
        let t = Transcript::parse("ab cd ef remember");
        gate(&mut a, &t, 200.0, &cfg());
        let last = &a.lines[0].words[3];
        assert!(last.estimated);
        assert!((last.start - 11.2).abs() < 1e-9);
        assert!(last.end < 12.0, "bounded, not the stretch: {}", last.end);
        assert!(
            (a.lines[0].end - last.end).abs() < 1e-9,
            "the line ends with it"
        );
        assert!(a.gate.is_some(), "the file records what the gate did");
    }

    #[test]
    fn a_line_with_too_many_suspects_falls_back_to_the_source_stamp() {
        // Same master; line 2's words all sprinted → the line takes
        // the source stamp (shifted by the median) and spreads its
        // words evenly.
        let mut a = alignment(vec![
            line(
                10.1,
                vec![
                    word("ab", 10.1, 10.6, 0.5, 2),
                    word("cd", 10.7, 11.2, 0.5, 2),
                ],
            ),
            line(
                20.1,
                vec![
                    word("ef", 20.1, 20.14, 0.5, 2),
                    word("gh", 20.2, 20.24, 0.5, 2),
                ],
            ),
            line(30.1, vec![word("ij", 30.1, 30.6, 0.5, 2)]),
        ]);
        let t = Transcript::parse("[00:10.00]ab cd\n[00:20.00]ef gh\n[00:30.00]ij");
        let report = gate(&mut a, &t, 200.0, &cfg());
        assert_eq!(report.verdict, Verdict::SameMaster);
        assert_eq!(report.lines_fallen_back, 1);
        let l = &a.lines[1];
        assert!((l.start - 20.1).abs() < 1e-9, "source 20.0 + median 0.1");
        assert!((l.end - 30.1).abs() < 1e-9, "to the next stamp");
        assert!(l.words.iter().all(|w| w.estimated));
        assert!((l.words[0].start - 20.1).abs() < 1e-9 && (l.words[1].end - 30.1).abs() < 1e-9);
        // The other lines are untouched.
        assert!(!a.lines[0].words[0].estimated && !a.lines[2].words[0].estimated);
    }

    #[test]
    fn a_failed_alignment_falls_back_to_the_source_everywhere() {
        let mut a = alignment(vec![
            line(2.0, vec![word("ab", 2.0, 2.5, 0.5, 2)]),
            line(60.0, vec![word("cd", 60.0, 60.5, 0.5, 2)]),
            line(30.0, vec![word("ef", 30.0, 30.5, 0.5, 2)]),
            line(150.0, vec![word("gh", 150.0, 150.5, 0.5, 2)]),
        ]);
        let t = Transcript::parse("[00:10.00]ab\n[00:20.00]cd\n[00:30.00]ef\n[00:40.00]gh");
        let report = gate(&mut a, &t, 200.0, &cfg());
        assert_eq!(report.verdict, Verdict::Failed);
        assert_eq!(report.lines_fallen_back, 4);
        // The source's stamps as they are — the median of a failed
        // alignment says nothing — every line, every word estimated.
        for (i, l) in a.lines.iter().enumerate() {
            let stamp = 10.0 * (i as f64 + 1.0);
            assert!((l.start - stamp).abs() < 1e-9, "line {i}: {}", l.start);
            assert!(l.words.iter().all(|w| w.estimated));
        }
        assert!(
            (a.lines[3].end - 43.0).abs() < 1e-9,
            "the last line ends 3 s after its stamp"
        );
    }

    #[test]
    fn a_different_edit_keeps_the_aligned_times_and_uses_no_stamp() {
        // Stamps run to 300 s in a 200-second file; one line has all
        // sprinted words and must fall back on ITS OWN span, not on a
        // stamp from another edit.
        let mut a = alignment(vec![
            line(10.0, vec![word("ab", 10.0, 10.5, 0.5, 2)]),
            line(
                50.0,
                vec![
                    word("cd", 50.0, 50.04, 0.5, 2),
                    word("ef", 50.1, 50.14, 0.5, 2),
                ],
            ),
            line(90.0, vec![word("gh", 90.0, 90.5, 0.5, 2)]),
        ]);
        let t = Transcript::parse("[00:10.00]ab\n[02:30.00]cd ef\n[05:00.00]gh");
        let report = gate(&mut a, &t, 200.0, &cfg());
        assert_eq!(report.verdict, Verdict::DifferentEdit);
        assert_eq!(report.lines_fallen_back, 1);
        let l = &a.lines[1];
        assert!(
            (l.start - 50.0).abs() < 1e-9,
            "its own span, not 150 s: {}",
            l.start
        );
        assert!(l.words.iter().all(|w| w.estimated));
        assert!((a.lines[0].start - 10.0).abs() < 1e-9 && !a.lines[0].words[0].estimated);
    }

    #[test]
    fn the_confidence_floor_is_off_by_default_and_bites_when_set() {
        let make = || {
            alignment(vec![line(
                10.0,
                vec![
                    word("ab", 10.0, 10.5, 0.01, 2),
                    word("cd", 10.6, 11.0, 0.9, 2),
                    word("ef", 11.1, 11.5, 0.9, 2),
                    word("gh", 11.6, 12.0, 0.9, 2),
                ],
            )])
        };
        let t = Transcript::parse("ab cd ef gh");
        let mut off = make();
        assert_eq!(gate(&mut off, &t, 100.0, &cfg()).words_estimated, 0);
        let mut on = make();
        let strict = GateConfig {
            word_conf_floor: 0.5,
            ..cfg()
        };
        assert_eq!(gate(&mut on, &t, 100.0, &strict).words_estimated, 1);
        assert!(on.lines[0].words[0].estimated && !on.lines[0].words[1].estimated);
    }
}
