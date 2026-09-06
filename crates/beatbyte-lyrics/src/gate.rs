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
use crate::evidence::Evidence;
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
    /// Letters per second the model must produce in its own greedy
    /// reading before its alignment may outvote the source's stamps.
    ///
    /// **1.0, and the number is measured.** On the 79-song corpus,
    /// where every word's true onset is known, the raw aligner's
    /// median error is 15.46 s below this floor and 0.78 s above it —
    /// a factor of twenty. See `docs/lyrics/evaluation.md`.
    ///
    /// It is a floor, not a predictor: a quiet song at 0.65 aligned
    /// to within a second, and a legible one at 1.74 was 18 s out.
    /// What the floor says is that BELOW it an alignment carries no
    /// information worth setting against a human's stamps.
    pub min_letters_per_s: f32,
    /// A word that starts this long after the word before it, inside
    /// its own line, was not placed but parked: a weak final word
    /// (a held "sein") that the model cannot hear is put by the
    /// Viterbi where the next letters are — the onset of the NEXT
    /// line, thirteen seconds late in the case that found this. Lines
    /// do not pause that long between two words; the word is marked
    /// estimated and retimed to follow its predecessor.
    pub max_gap_s: f64,
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
    /// Stamps that stop before this share of the audio — and leave
    /// more than [`GateConfig::min_tail_s`] unstamped — describe a
    /// SHORTER edit. Measured on the library: a 4-minute original's
    /// stamps were handed to an 8:37 remix, and falling back to them
    /// would have crammed every line into the first 45 % of the song
    /// and left the rest silent.
    pub min_span_share: f64,
    /// How much unstamped tail it takes for the rule above to mean
    /// anything; below this it is an ordinary instrumental outro.
    pub min_tail_s: f64,
    /// A delta that changes by more than this across the compared
    /// span — and is explained by a straight line, not by noise — is
    /// a different edit. Set above a master's ordinary tempo
    /// tolerance: one song in the library drifted 0.7 s over 220 s
    /// with 98 % of its lines agreeing, which is the same
    /// performance, not another edit (and calling it one kept a
    /// worse alignment beside the song, because the verdict ranks
    /// below a confirmed one).
    pub edit_drift_s: f64,
}

impl Default for GateConfig {
    fn default() -> GateConfig {
        GateConfig {
            word_conf_floor: 0.0,
            min_letters_per_s: 1.0,
            max_gap_s: 2.0,
            max_word_s: 5.0,
            sprint_slack_frames: 0,
            line_fallback_share: 0.30,
            master_shift_s: 1.5,
            consensus_share: 0.5,
            line_outlier_s: 1.5,
            drift_residual_s: 0.75,
            edit_slack_s: 2.0,
            min_span_share: 0.5,
            min_tail_s: 60.0,
            edit_drift_s: 2.5,
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
    /// The source's stamps belong to another edit of this
    /// performance and were mapped onto this recording by
    /// `scale · t + offset` before anchoring; lines the map put
    /// beyond the sound were dropped as unsung. Aligned times are
    /// kept; the mapped stamps serve as the fallback.
    Stretched {
        /// The tempo ratio, this recording over the source's.
        scale: f64,
        /// Seconds added after scaling (the map's offset plus what
        /// the aligned lines still sat off the mapped stamps by).
        offset_s: f64,
    },
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
    /// Lines dropped as unsung: the source's text this recording
    /// does not sing (see [`Verdict::Stretched`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lines_unsung: usize,
    /// Letters per second in the model's own greedy reading of this
    /// audio, when it was measured — the fact that says whether the
    /// alignment is evidence at all. `None` for a gate run without it
    /// (older files, and the tests that do not care).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letters_per_s: Option<f32>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Apply the gate to an alignment in place. `audio_duration_s` is the
/// length of the file the alignment was computed on. With `warp`,
/// the source's stamps are judged AS MAPPED onto this recording, the
/// verdict says so, and the lines the map put beyond the sound are
/// dropped from the alignment.
pub fn gate(
    alignment: &mut Alignment,
    transcript: &Transcript,
    audio_duration_s: f64,
    evidence: Option<Evidence>,
    warp: Option<&crate::align::WarpResult>,
    config: &GateConfig,
) -> GateReport {
    // 1 + 2: the verdict, from the deltas against the source stamps.
    // A line without aligned letters (`♪`, a number) has a guessed
    // start, not an aligned one — it says nothing about the source.
    let stamps_source = warp.map_or(transcript, |w| &w.retimed);
    let stamps: Vec<Option<f64>> = stamps_source
        .lines
        .iter()
        .map(|l| l.source_start_s)
        .collect();
    // 0: the lines this recording does not sing carry no evidence
    // and get none; they are dropped once every index-based step
    // below is done.
    if let Some(warp) = warp {
        for &index in &warp.unsung {
            if let Some(line) = alignment.lines.get_mut(index) {
                for word in &mut line.words {
                    word.estimated = true;
                    word.conf = 0.0;
                    word.chars.clear();
                }
            }
        }
    }
    let pairs: Vec<(f64, f64)> = alignment
        .lines
        .iter()
        .zip(&stamps)
        .filter(|(line, _)| line.words.iter().any(|w| !w.estimated))
        .filter_map(|(line, stamp)| stamp.map(|s| (s, line.start - s)))
        .collect();
    let judged = verdict_of(&pairs, audio_duration_s, config);
    let (mut verdict, median, mad) = (judged.verdict, judged.median, judged.mad);
    if let Some(map) = warp.and_then(|w| w.warp) {
        verdict = match verdict {
            Verdict::SameMaster => Verdict::Stretched {
                scale: map.scale,
                offset_s: map.offset_s,
            },
            Verdict::ShiftedMaster { offset_s } => Verdict::Stretched {
                scale: map.scale,
                offset_s: map.offset_s + offset_s,
            },
            other => other,
        };
    }

    // 2b: an alignment is only evidence if the model heard the song.
    //
    // Forced alignment always returns a path. On a mix the model
    // cannot read, the words land wherever their windows are, the
    // deltas reproduce the shift those windows were centred on, and
    // the consensus measures the window rather than the audio. That
    // is how Mexico came back "shifted master, 85 % agreement,
    // −3.34 s" from a song whose greedy reading is one letter in six
    // seconds — and ran three seconds early on screen. Below the
    // floor the alignment cannot outvote a human's stamps, so the
    // verdict is the honest one: failed, fall back to the source.
    //
    // Two consequences, and they are different. The verdict may not
    // CLAIM anything the audio does not support — but only where the
    // source's stamps are a usable answer: `DifferentEdit` was
    // decided from the stamps against the file (they run past its end
    // or stop far short of it), and falling back to stamps already
    // known to belong to another recording would be worse than the
    // arbitrary-but-in-range times we have. And whatever the verdict,
    // the WORD times of a song the model could not read are not
    // knowledge, so every word is marked estimated and the song sings
    // by the line.
    let illegible = evidence.is_some_and(|e| !e.is_legible(config.min_letters_per_s));
    if illegible
        && matches!(
            verdict,
            Verdict::SameMaster | Verdict::ShiftedMaster { .. } | Verdict::Stretched { .. }
        )
    {
        verdict = Verdict::Failed;
    }

    // 3a: words.
    let mut words_estimated = 0usize;
    for line in &mut alignment.lines {
        // Where the last word that was PLACED ended — a sprinted or
        // endless word still started where it was placed, so it
        // anchors the gap rule; a parked one does not.
        let mut anchor_end: Option<f64> = None;
        let mut parked = false;
        for word in &mut line.words {
            let had_letters = !word.estimated;
            let parked_here = had_letters && word_is_parked(word, anchor_end, config);
            if had_letters && (illegible || parked_here || word_is_suspect(word, config)) {
                word.estimated = true;
                word.conf = 0.0;
                word.chars.clear();
                words_estimated += 1;
            }
            if had_letters && !parked_here {
                anchor_end = Some(word.end);
            }
            parked |= parked_here;
        }
        // A parked word had stretched the line to wherever it was
        // put; the line ends where its placed words end, and the
        // parked ones are retimed after them below.
        if parked && let Some(end) = anchor_end {
            line.end = end.max(line.start);
        }
    }

    // 3b: lines.
    let mut lines_fallen_back = 0usize;
    let source_is_reference = matches!(
        verdict,
        Verdict::SameMaster
            | Verdict::ShiftedMaster { .. }
            | Verdict::Stretched { .. }
            | Verdict::Failed
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
    // The unsung lines, last: everything above worked by index.
    let mut lines_unsung = 0;
    if let Some(warp) = warp {
        let mut unsung: Vec<usize> = warp.unsung.clone();
        unsung.sort_unstable();
        for index in unsung.into_iter().rev() {
            if index < alignment.lines.len() {
                alignment.lines.remove(index);
                lines_unsung += 1;
            }
        }
    }

    let report = GateReport {
        verdict,
        lines_compared: pairs.len(),
        consensus: judged.consensus,
        median_delta_s: median,
        mad_s: mad,
        words_estimated,
        lines_fallen_back: lines_fallen_back.saturating_sub(lines_unsung),
        lines_unsung,
        letters_per_s: evidence.map(|e| e.letters_per_s),
    };
    alignment.gate = Some(report.clone());
    report
}

/// A verdict's standing when two alignments of the same song are
/// compared: a failed one is worth least, one that could not be
/// judged against the source (no stamps, or stamps from another
/// edit) sits in the middle, and one the source confirmed ranks
/// highest. Pure — tested.
#[must_use]
pub fn verdict_rank(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::Failed => 0,
        Verdict::NoReference | Verdict::DifferentEdit => 1,
        Verdict::SameMaster | Verdict::ShiftedMaster { .. } | Verdict::Stretched { .. } => 2,
    }
}

/// Whether a new alignment should replace the one already beside the
/// audio, from the two gate reports alone: the verdict's standing
/// first, then how much of the song the model heard (letters per
/// second in its own greedy reading — the measure the gate itself
/// trusts). A tie keeps the old file: nothing is rewritten for
/// nothing. A report without a legibility figure counts as having
/// heard nothing, so a measured result always outranks an unmeasured
/// one of the same standing. Pure — tested.
#[must_use]
pub fn prefer(new: &GateReport, old: &GateReport) -> bool {
    let (rank_new, rank_old) = (verdict_rank(new.verdict), verdict_rank(old.verdict));
    if rank_new != rank_old {
        return rank_new > rank_old;
    }
    new.letters_per_s.unwrap_or(0.0) > old.letters_per_s.unwrap_or(0.0)
}

/// A word parked long after the word before it (see
/// [`GateConfig::max_gap_s`]). `anchor_end` is where the last placed
/// word of the same line ended, if any. Pure — tested.
#[must_use]
pub fn word_is_parked(word: &AlignedWord, anchor_end: Option<f64>, config: &GateConfig) -> bool {
    anchor_end.is_some_and(|end| word.start - end > config.max_gap_s)
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
    // ...and the other way round: a stamp grid that gives up less
    // than half way through a much longer file is a shorter edit,
    // not a song with a long outro.
    if audio_duration_s.is_finite()
        && last_stamp < audio_duration_s * config.min_span_share
        && audio_duration_s - last_stamp > config.min_tail_s
    {
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
        // No agreement AND the text ends a full minute before the
        // sound does: the words are another edit's — the single's
        // sheet on a seven-minute extended mix — and its stamps would
        // be wrong from the first line. The aligned times stand, as
        // for any other edit. With the text spanning the sound, the
        // same disagreement is a failed alignment and the stamps are
        // the better answer.
        if audio_duration_s.is_finite() && audio_duration_s - last_stamp > config.min_tail_s {
            return judged(
                Verdict::DifferentEdit,
                Some(median),
                Some(mad),
                Some(consensus),
            );
        }
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
        // Stamps that stop less than half way through a much longer
        // file are a SHORTER edit - measured on the library: a
        // 4-minute original's stamps handed to an 8:37 remix. Falling
        // back to them would leave the last 4:41 unsung.
        let short: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 25.0, 0.1)).collect();
        assert_eq!(
            verdict_of(&short, 517.0, &c).verdict,
            Verdict::DifferentEdit
        );
        // An ordinary instrumental outro is not that: the same
        // stamps under a song only a little longer.
        assert_eq!(verdict_of(&short, 260.0, &c).verdict, Verdict::SameMaster);
        // Nor is a short song with a short tail - the absolute rule
        // keeps the share rule from firing on small numbers.
        let tiny: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 2.0, 0.1)).collect();
        assert_eq!(verdict_of(&tiny, 50.0, &c).verdict, Verdict::SameMaster);
        // And a genuinely long outro is not a shorter edit either:
        // last word at 6:40 of an 8:20 track, 100 s of instrumental
        // after it. The share has to stay generous enough for that.
        let outro: Vec<(f64, f64)> = (0..20).map(|i| (i as f64 * 20.0 + 20.0, 0.1)).collect();
        assert_eq!(verdict_of(&outro, 500.0, &c).verdict, Verdict::SameMaster);
        // Consistent, small: same master.
        let same: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 10.0, 0.1)).collect();
        assert_eq!(verdict_of(&same, 100.0, &c).verdict, Verdict::SameMaster);
        // Consistent, large: shifted master, offset reported.
        let shifted: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 10.0, -2.4)).collect();
        assert!(matches!(
            verdict_of(&shifted, 100.0, &c).verdict,
            Verdict::ShiftedMaster { offset_s } if (offset_s + 2.4).abs() < 1e-9
        ));
        // Inconsistent: failed.
        let noisy: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 10.0, if i % 2 == 0 { -8.0 } else { 6.0 }))
            .collect();
        assert_eq!(verdict_of(&noisy, 100.0, &c).verdict, Verdict::Failed);
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
            verdict_of(&growing, 200.0, &c).verdict,
            Verdict::DifferentEdit
        );
        // A small tilt inside a consistent band is NOT a drift: 0.3 s
        // across the span stays the same master.
        let tilted: Vec<(f64, f64)> = (0..10)
            .map(|i| (i as f64 * 20.0, 0.1 + 0.0017 * i as f64 * 20.0))
            .collect();
        assert_eq!(verdict_of(&tilted, 200.0, &c).verdict, Verdict::SameMaster);
        // Nor is a second across the span on a shifted master (the
        // library's For You: 0.7 s over 220 s, 98 % agreeing).
        let breathing: Vec<(f64, f64)> = (0..12)
            .map(|i| (i as f64 * 20.0, 6.5 + 0.005 * i as f64 * 20.0))
            .collect();
        assert!(matches!(
            verdict_of(&breathing, 260.0, &c).verdict,
            Verdict::ShiftedMaster { .. }
        ));
        // A majority that agrees carries the song even when a third
        // of the lines wander (the stacked-chorus pop case): same
        // master, and the wanderers are the per-line business.
        let mostly: Vec<(f64, f64)> = (0..12)
            .map(|i| (i as f64 * 15.0, if i % 3 == 2 { -5.0 } else { -1.0 }))
            .collect();
        let j = verdict_of(&mostly, 180.0, &c);
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
        assert_eq!(verdict_of(&lost, 240.0, &c).verdict, Verdict::Failed);
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
        let report = gate(&mut a, &t, 40.0, None, None, &cfg());
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
        let report = gate(&mut a, &t, 200.0, None, None, &cfg());
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
        gate(&mut a, &t, 200.0, None, None, &cfg());
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
        let report = gate(&mut a, &t, 40.0, None, None, &cfg());
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

    /// A song the model could not read may not outvote the source.
    ///
    /// This is Mexico: a loud German mix under an English model,
    /// whose greedy reading is one letter in six seconds. The deltas
    /// still looked like a tidy shifted master, because the anchored
    /// pass is centred on the shift it is meant to test and the words
    /// simply land in their windows. The lyrics ran three seconds
    /// early on screen; the source's own stamps were right.
    #[test]
    fn an_alignment_the_model_could_not_hear_does_not_claim_a_shift() {
        use crate::evidence::Evidence;
        let make = || {
            alignment(vec![
                line(
                    6.7,
                    vec![word("ab", 6.7, 7.2, 0.01, 2), word("cd", 7.3, 7.8, 0.01, 2)],
                ),
                line(
                    16.7,
                    vec![
                        word("ef", 16.7, 17.2, 0.01, 2),
                        word("gh", 17.3, 17.8, 0.01, 2),
                    ],
                ),
                line(26.7, vec![word("ij", 26.7, 27.2, 0.01, 2)]),
            ])
        };
        let t = Transcript::parse("[00:10.00]ab cd\n[00:20.00]ef gh\n[00:30.00]ij");
        let heard = Evidence {
            voiced_share: 0.07,
            letters_per_s: 2.4,
        };
        let deaf = Evidence {
            voiced_share: 0.01,
            letters_per_s: 0.18,
        };

        // Heard: a consistent −3.3 s IS a shifted master, and the
        // aligned times stand.
        let mut a = make();
        let report = gate(&mut a, &t, 40.0, Some(heard), None, &cfg());
        assert!(
            matches!(report.verdict, Verdict::ShiftedMaster { offset_s } if (offset_s + 3.3).abs() < 1e-6),
            "{:?}",
            report.verdict
        );
        assert!((a.lines[0].start - 6.7).abs() < 1e-9, "aligned times kept");

        // Not heard: the same numbers prove nothing. The verdict is
        // failed, and every line goes back to the human's stamps.
        let mut a = make();
        let report = gate(&mut a, &t, 40.0, Some(deaf), None, &cfg());
        assert_eq!(report.verdict, Verdict::Failed);
        assert!(
            (a.lines[0].start - 10.0).abs() < 1e-9,
            "the source's stamp, not the alignment: {}",
            a.lines[0].start
        );
        assert_eq!(report.letters_per_s, Some(0.18), "the file says why");

        // And a song with no stamps at all cannot "fall back" to
        // them, so a deaf model leaves that verdict alone — but its
        // words are still not knowledge.
        let mut a = make();
        let bare = Transcript::parse("ab cd\nef gh\nij");
        let report = gate(&mut a, &bare, 40.0, Some(deaf), None, &cfg());
        assert_eq!(report.verdict, Verdict::NoReference);
        assert!(
            a.lines.iter().all(|l| l.words.iter().all(|w| w.estimated)),
            "a deaf model knows no word times, whatever the verdict"
        );
    }

    /// Stamps that belong to another recording are not a fallback,
    /// and a deaf model does not change that.
    ///
    /// `DifferentEdit` is decided from the stamps against the FILE —
    /// here they run past its end — so the acoustics have no say in
    /// it. Falling back to them because the model heard nothing would
    /// replace arbitrary-but-in-range times with times known to be
    /// from a different recording, which is worse.
    #[test]
    fn a_deaf_model_does_not_send_a_song_back_to_another_edits_stamps() {
        use crate::evidence::Evidence;
        let mut a = alignment(vec![
            line(10.0, vec![word("ab", 10.0, 10.5, 0.01, 2)]),
            line(20.0, vec![word("cd", 20.0, 20.5, 0.01, 2)]),
            line(30.0, vec![word("ef", 30.0, 30.5, 0.01, 2)]),
        ]);
        // The last stamp lies past the end of a 40 s file.
        let t = Transcript::parse("[00:10.00]ab\n[00:20.00]cd\n[01:30.00]ef");
        let deaf = Evidence {
            voiced_share: 0.01,
            letters_per_s: 0.1,
        };
        let report = gate(&mut a, &t, 40.0, Some(deaf), None, &cfg());
        assert_eq!(report.verdict, Verdict::DifferentEdit);
        assert!(
            a.lines[0].start < 40.0 && a.lines[2].start < 40.0,
            "lines stay inside the file, not at 90 s"
        );
        assert!(
            a.lines.iter().all(|l| l.words.iter().all(|w| w.estimated)),
            "and it still sings by the line"
        );
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
        let report = gate(&mut a, &t, 50.0, None, None, &cfg());
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
        let report = gate(&mut a, &t, 200.0, None, None, &cfg());
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
        assert_eq!(
            gate(&mut off, &t, 100.0, None, None, &cfg()).words_estimated,
            0
        );
        let mut on = make();
        let strict = GateConfig {
            word_conf_floor: 0.5,
            ..cfg()
        };
        assert_eq!(
            gate(&mut on, &t, 100.0, None, None, &strict).words_estimated,
            1
        );
        assert!(on.lines[0].words[0].estimated && !on.lines[0].words[1].estimated);
    }

    fn report(verdict: Verdict, letters_per_s: Option<f32>) -> GateReport {
        GateReport {
            verdict,
            lines_compared: 0,
            consensus: None,
            median_delta_s: None,
            mad_s: None,
            words_estimated: 0,
            lines_fallen_back: 0,
            lines_unsung: 0,
            letters_per_s,
        }
    }

    #[test]
    fn a_confirmed_verdict_outranks_an_unjudged_one_which_outranks_a_failure() {
        assert!(verdict_rank(Verdict::SameMaster) > verdict_rank(Verdict::DifferentEdit));
        assert_eq!(
            verdict_rank(Verdict::ShiftedMaster { offset_s: 3.0 }),
            verdict_rank(Verdict::SameMaster)
        );
        assert_eq!(
            verdict_rank(Verdict::NoReference),
            verdict_rank(Verdict::DifferentEdit)
        );
        assert!(verdict_rank(Verdict::DifferentEdit) > verdict_rank(Verdict::Failed));
        // Mapped stamps are as good as a shifted master's.
        assert_eq!(
            verdict_rank(Verdict::Stretched {
                scale: 1.04,
                offset_s: -10.8
            }),
            verdict_rank(Verdict::SameMaster)
        );
    }

    #[test]
    fn under_a_warp_the_verdict_is_stretched_and_the_unsung_lines_are_dropped() {
        use crate::align::{Warp, WarpResult};
        use crate::transcript::Transcript;
        // Four lines stamped for another edit: the recording sings
        // three of them at 1.04·t − 10.8 and ends before the fourth.
        let transcript =
            Transcript::parse("[00:20.00]ab cd\n[00:30.00]ef gh\n[00:40.00]ij kl\n[03:00.00]mn op");
        let warp = Warp {
            scale: 1.04,
            offset_s: -10.8,
        };
        let (retimed, unsung) = warp.retime(&transcript, 40.0);
        assert_eq!(unsung, vec![3]);
        let word = |text: &str, start: f64| AlignedWord {
            text: text.to_owned(),
            start,
            end: start + 0.4,
            conf: 0.5,
            estimated: false,
            chars: vec![[start, start + 0.4]],
        };
        let line = |start: f64, a: &str, b: &str| AlignedLine {
            start,
            end: start + 1.0,
            text: format!("{a} {b}"),
            words: vec![word(a, start), word(b, start + 0.5)],
        };
        let mut alignment = Alignment {
            schema: crate::words::SCHEMA.to_owned(),
            audio_sha256: String::new(),
            pipeline_version: crate::PIPELINE_VERSION,
            language: "en".to_owned(),
            source: crate::words::Source {
                text: String::new(),
                separator: "none".to_owned(),
                aligner: String::new(),
            },
            offset_ms: 0,
            gate: None,
            lines: vec![
                line(warp.apply(20.0) + 0.1, "ab", "cd"),
                line(warp.apply(30.0) - 0.1, "ef", "gh"),
                line(warp.apply(40.0), "ij", "kl"),
                // Placed at the end: junk.
                line(39.0, "mn", "op"),
            ],
        };
        let result = WarpResult {
            warp: Some(warp),
            retimed,
            unsung,
        };
        // Judged against the SOUND's end, not the container's: the
        // fourth line is unsung because the recording stops at 40 s.
        let report = gate(
            &mut alignment,
            &transcript,
            40.0,
            None,
            Some(&result),
            &GateConfig::default(),
        );
        assert!(
            matches!(report.verdict, Verdict::Stretched { scale, .. } if (scale - 1.04).abs() < 1e-9),
            "{:?}",
            report.verdict
        );
        assert_eq!(report.lines_unsung, 1);
        assert_eq!(alignment.lines.len(), 3, "the unsung line is gone");
        assert_eq!(alignment.lines[2].text, "ij kl");
        assert_eq!(report.lines_fallen_back, 0);
        // And the file says so.
        let json = alignment.to_json().expect("json");
        assert!(!json.contains("mn op"));
    }

    #[test]
    fn a_better_verdict_replaces_regardless_of_legibility() {
        // The stem alignment heard less but the source confirmed it;
        // the mix alignment heard more and failed.
        let new = report(Verdict::SameMaster, Some(0.9));
        let old = report(Verdict::Failed, Some(2.5));
        assert!(prefer(&new, &old));
        assert!(!prefer(&old, &new));
    }

    #[test]
    fn at_equal_standing_the_one_the_model_heard_more_of_wins_and_a_tie_keeps_the_old() {
        let heard_more = report(Verdict::SameMaster, Some(3.1));
        let heard_less = report(Verdict::ShiftedMaster { offset_s: 1.0 }, Some(1.2));
        assert!(prefer(&heard_more, &heard_less));
        assert!(!prefer(&heard_less, &heard_more));
        // Exactly equal: nothing is rewritten for nothing.
        assert!(!prefer(&heard_less, &heard_less.clone()));
        // Unmeasured counts as nothing heard: a measured result of
        // the same standing replaces it, never the other way round.
        let unmeasured = report(Verdict::SameMaster, None);
        assert!(prefer(&heard_less, &unmeasured));
        assert!(!prefer(&unmeasured, &heard_less));
    }

    #[test]
    fn a_word_parked_long_after_its_line_is_retimed_to_follow_its_predecessor() {
        use crate::transcript::Transcript;
        // "Wieder Weltmeister sein": the held "sein" was put at the
        // next line's onset, fourteen seconds after "Weltmeister".
        let transcript =
            Transcript::parse("Wieder Weltmeister Weltmeister sein\nSiegesgewiss fahren wir");
        let word = |text: &str, start: f64, end: f64| AlignedWord {
            text: text.to_owned(),
            start,
            end,
            conf: 0.4,
            estimated: false,
            chars: vec![[start, end]],
        };
        let mut alignment = Alignment {
            schema: crate::words::SCHEMA.to_owned(),
            audio_sha256: String::new(),
            pipeline_version: crate::PIPELINE_VERSION,
            language: "en".to_owned(),
            source: crate::words::Source {
                text: String::new(),
                separator: "none".to_owned(),
                aligner: String::new(),
            },
            offset_ms: 0,
            gate: None,
            lines: vec![
                AlignedLine {
                    start: 55.58,
                    end: 73.22,
                    text: "Wieder Weltmeister Weltmeister sein".to_owned(),
                    words: vec![
                        word("Wieder", 55.58, 55.86),
                        word("Weltmeister,", 55.92, 57.24),
                        word("Weltmeister", 57.34, 59.16),
                        word("sein", 73.04, 73.22),
                    ],
                },
                AlignedLine {
                    start: 73.24,
                    end: 77.56,
                    text: "Siegesgewiss fahren wir".to_owned(),
                    words: vec![
                        word("Siegesgewiss", 73.24, 74.5),
                        word("fahren", 74.6, 75.9),
                        word("wir", 76.0, 77.56),
                    ],
                },
            ],
        };
        let report = gate(
            &mut alignment,
            &transcript,
            300.0,
            None,
            None,
            &GateConfig::default(),
        );
        let sein = &alignment.lines[0].words[3];
        assert!(sein.estimated, "the parked word is not vouched for");
        assert_eq!(report.words_estimated, 1);
        // Retimed to follow "Weltmeister", not left at the next line.
        assert!(
            (sein.start - 59.16).abs() < 1e-9,
            "starts at {}",
            sein.start
        );
        assert!(sein.end < 62.0, "ends at {}", sein.end);
        assert!(
            alignment.lines[0].end < 62.0,
            "the line ends with its last word"
        );
        // The line that pauses under the limit keeps every word.
        assert!(alignment.lines[1].words.iter().all(|w| !w.estimated));
        // A pause under the limit, inside a line, is not parking.
        let ok = word("later", 59.16 + GateConfig::default().max_gap_s - 0.1, 62.0);
        assert!(!word_is_parked(&ok, Some(59.16), &GateConfig::default()));
        assert!(word_is_parked(
            &word("late", 62.0, 62.3),
            Some(59.16),
            &GateConfig::default()
        ));
        // The first word of a line has nothing to be parked after.
        assert!(!word_is_parked(
            &word("late", 62.0, 62.3),
            None,
            &GateConfig::default()
        ));
    }

    #[test]
    fn disagreeing_stamps_on_a_text_that_ends_long_before_the_sound_are_another_edit() {
        // The single's 54 lines on a seven-minute extended mix: the
        // deltas scatter, the last stamp sits at 228 s in 427 s.
        let scattered: Vec<(f64, f64)> = (0..20)
            .map(|i| (20.0 + 11.0 * i as f64, ((i * 37) % 60) as f64 - 30.0))
            .collect();
        let judged = verdict_of(&scattered, 427.0, &GateConfig::default());
        assert_eq!(judged.verdict, Verdict::DifferentEdit, "{judged:?}");
        assert!(judged.consensus.is_some_and(|c| c <= 0.5));
        // The same disagreement on a text that spans the sound is a
        // failed alignment, and the stamps are the fallback.
        let judged = verdict_of(&scattered, 240.0, &GateConfig::default());
        assert_eq!(judged.verdict, Verdict::Failed);
    }
}
