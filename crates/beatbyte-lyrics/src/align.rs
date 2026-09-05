//! The pipeline: audio and transcript in, an [`Alignment`] out.
//!
//! Decode → 16 kHz → emissions in windows → one Viterbi over the
//! whole song → spans back onto words and lines → provenance. Words
//! the model has no letters for are timed between their neighbours
//! and marked; nothing is dropped, so the karaoke text stays the
//! text the player gave.

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::resample::resample;
use beatbyte_ml::{Loaded, MlError, Runtime};
use thiserror::Error;

use crate::ctc::{AlignError, TokenSpan, force_align};
use crate::emissions::{FRAME_S, SAMPLE_RATE, compute};
use crate::transcript::{BLANK, Transcript, WORD_BOUNDARY};
use crate::words::{AlignedLine, AlignedWord, Alignment, SCHEMA, Source};

/// Why an alignment was not produced.
#[derive(Debug, Error)]
pub enum LyricsError {
    /// The transcript has no word the model has letters for.
    #[error("the lyrics contain no alignable words")]
    NoWords,
    /// The model could not be loaded or run.
    #[error(transparent)]
    Model(#[from] MlError),
    /// The alignment itself failed.
    #[error(transparent)]
    Align(#[from] AlignError),
}

/// What the run found out about itself — for the CLI's report and
/// for the confidence gating of the next milestone.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// Words in the output.
    pub words: usize,
    /// Words the model placed no letters for.
    pub estimated: usize,
    /// Mean confidence over the aligned words.
    pub mean_conf: f32,
    /// Aligned words with confidence under [`UNCERTAIN_BELOW`].
    pub uncertain: usize,
    /// Emission frames the Viterbi ran over.
    pub frames: usize,
    /// Against the source's own line stamps, when it had them:
    /// `(lines compared, median delta, median absolute deviation)`,
    /// aligned minus source, in seconds. A consistent delta is a
    /// different master; an inconsistent one is a failed alignment.
    pub source_line_delta: Option<(usize, f64, f64)>,
}

/// A word below this confidence counts as uncertain in the stats.
pub const UNCERTAIN_BELOW: f32 = 0.5;

/// An alignment with its stats.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignOutcome {
    /// The result.
    pub alignment: Alignment,
    /// What the run found out about itself.
    pub stats: Stats,
}

/// Align `transcript` against `audio`. `audio_sha256` and
/// `text_source` are provenance, recorded verbatim.
pub fn align(
    audio: &AudioData,
    audio_sha256: &str,
    transcript: &Transcript,
    text_source: &str,
    runtime: &Runtime,
    model: &Loaded,
) -> Result<AlignOutcome, LyricsError> {
    let tokens = transcript.tokens();
    if tokens.is_empty() {
        return Err(LyricsError::NoWords);
    }
    let samples = resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE);
    let emissions = compute(runtime, model, &samples)?;
    let spans = force_align(&emissions, &tokens, BLANK)?;
    let lines = place(transcript, &spans);
    let stats = stats(&lines, transcript, emissions.frames);
    let alignment = Alignment {
        schema: SCHEMA.to_owned(),
        audio_sha256: audio_sha256.to_owned(),
        pipeline_version: crate::PIPELINE_VERSION,
        language: "en".to_owned(),
        source: Source {
            text: text_source.to_owned(),
            separator: "none".to_owned(),
            aligner: format!(
                "{}@sha256:{} {}",
                model.id,
                model.sha256,
                beatbyte_ml::FINGERPRINT
            ),
        },
        offset_ms: 0,
        gate: None,
        lines,
    };
    Ok(AlignOutcome { alignment, stats })
}

/// Hand the token spans back to the words they belong to, in order,
/// and time the letterless words between their neighbours. Pure —
/// tested with synthetic spans.
#[must_use]
pub fn place(transcript: &Transcript, spans: &[TokenSpan]) -> Vec<AlignedLine> {
    let seconds = |frame: usize| frame as f64 * FRAME_S;
    let mut cursor = 0usize;
    let mut first_word = true;
    let mut lines: Vec<AlignedLine> = Vec::with_capacity(transcript.lines.len());
    for line in &transcript.lines {
        let mut words = Vec::with_capacity(line.words.len());
        for word in &line.words {
            if word.tokens.is_empty() {
                words.push(AlignedWord {
                    text: word.text.clone(),
                    start: f64::NAN,
                    end: f64::NAN,
                    conf: 0.0,
                    estimated: true,
                    chars: Vec::new(),
                });
                continue;
            }
            if !first_word {
                // The boundary span between this word and the last.
                debug_assert_eq!(spans.get(cursor).map(|s| s.token), Some(WORD_BOUNDARY));
                cursor += 1;
            }
            first_word = false;
            let letters = &spans[cursor..cursor + word.tokens.len()];
            cursor += word.tokens.len();
            let chars: Vec<[f64; 2]> = letters
                .iter()
                .map(|s| [seconds(s.start), seconds(s.end)])
                .collect();
            let conf = geometric_mean(letters.iter().map(|s| s.score));
            words.push(AlignedWord {
                text: word.text.clone(),
                start: chars.first().map_or(0.0, |c| c[0]),
                end: chars.last().map_or(0.0, |c| c[1]),
                conf,
                estimated: false,
                chars,
            });
        }
        lines.push(AlignedLine {
            start: 0.0,
            end: 0.0,
            text: line.text.clone(),
            words,
        });
    }
    interpolate_estimated(&mut lines);
    for line in &mut lines {
        line.start = line.words.first().map_or(0.0, |w| w.start);
        line.end = line.words.last().map_or(0.0, |w| w.end);
    }
    lines
}

/// Estimated words take an even share of the gap between the aligned
/// words around them; at the very start or end they lean on the one
/// neighbour they have.
fn interpolate_estimated(lines: &mut [AlignedLine]) {
    // Flatten to (line, word) indices for neighbour search.
    let index: Vec<(usize, usize)> = lines
        .iter()
        .enumerate()
        .flat_map(|(l, line)| (0..line.words.len()).map(move |w| (l, w)))
        .collect();
    let time_of = |lines: &[AlignedLine], i: usize| -> (f64, f64) {
        let (l, w) = index[i];
        (lines[l].words[w].start, lines[l].words[w].end)
    };
    let mut i = 0usize;
    while i < index.len() {
        let (l, w) = index[i];
        if !lines[l].words[w].estimated {
            i += 1;
            continue;
        }
        // The run of estimated words starting here.
        let run_start = i;
        let mut run_end = i;
        while run_end < index.len() && {
            let (l2, w2) = index[run_end];
            lines[l2].words[w2].estimated
        } {
            run_end += 1;
        }
        let before = run_start.checked_sub(1).map(|j| time_of(lines, j).1);
        let after = (run_end < index.len()).then(|| time_of(lines, run_end).0);
        let (from, to) = match (before, after) {
            (Some(b), Some(a)) => (b, a.max(b)),
            (Some(b), None) => (b, b + 0.3 * (run_end - run_start) as f64),
            (None, Some(a)) => ((a - 0.3 * (run_end - run_start) as f64).max(0.0), a),
            (None, None) => (0.0, 0.0),
        };
        let count = (run_end - run_start) as f64;
        for (k, j) in (run_start..run_end).enumerate() {
            let (l2, w2) = index[j];
            let word = &mut lines[l2].words[w2];
            word.start = from + (to - from) * k as f64 / count;
            word.end = from + (to - from) * (k as f64 + 1.0) / count;
        }
        i = run_end;
    }
}

fn geometric_mean(scores: impl Iterator<Item = f32>) -> f32 {
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for s in scores {
        sum += f64::from(s.max(1e-6)).ln();
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        (sum / n as f64).exp() as f32
    }
}

fn stats(lines: &[AlignedLine], transcript: &Transcript, frames: usize) -> Stats {
    let words: Vec<&AlignedWord> = lines.iter().flat_map(|l| l.words.iter()).collect();
    let aligned: Vec<&AlignedWord> = words.iter().copied().filter(|w| !w.estimated).collect();
    let mean_conf = if aligned.is_empty() {
        0.0
    } else {
        aligned.iter().map(|w| w.conf).sum::<f32>() / aligned.len() as f32
    };
    let mut deltas: Vec<f64> = lines
        .iter()
        .zip(&transcript.lines)
        .filter_map(|(aligned, source)| source.source_start_s.map(|s| aligned.start - s))
        .collect();
    let source_line_delta = if deltas.is_empty() {
        None
    } else {
        deltas.sort_by(f64::total_cmp);
        let median = deltas[deltas.len() / 2];
        let mut deviations: Vec<f64> = deltas.iter().map(|d| (d - median).abs()).collect();
        deviations.sort_by(f64::total_cmp);
        Some((deltas.len(), median, deviations[deviations.len() / 2]))
    };
    Stats {
        words: words.len(),
        estimated: words.len() - aligned.len(),
        mean_conf,
        uncertain: aligned.iter().filter(|w| w.conf < UNCERTAIN_BELOW).count(),
        frames,
        source_line_delta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::tokens_of;

    fn span(token: u8, start: usize, end: usize, score: f32) -> TokenSpan {
        TokenSpan {
            token,
            start,
            end,
            score,
        }
    }

    #[test]
    fn spans_land_on_their_words_with_boundaries_skipped() {
        let t = Transcript::parse("ab 7 cd\nef");
        // Tokens: A B | C D | E F (the "7" contributes nothing).
        let spans = vec![
            span(7, 10, 12, 0.9),  // A
            span(24, 12, 15, 0.8), // B
            span(4, 15, 16, 0.5),  // |
            span(19, 20, 22, 0.7), // C
            span(14, 22, 25, 0.6), // D
            span(4, 25, 26, 0.5),  // |
            span(5, 40, 42, 0.95), // E
            span(20, 42, 44, 0.9), // F
        ];
        let lines = place(&t, &spans);
        assert_eq!(lines.len(), 2);
        let ab = &lines[0].words[0];
        assert!((ab.start - 0.20).abs() < 1e-9 && (ab.end - 0.30).abs() < 1e-9);
        assert_eq!(ab.chars, vec![[0.20, 0.24], [0.24, 0.30]]);
        assert!(!ab.estimated);
        // The 7 sits between "ab" (ends 0.30) and "cd" (starts 0.40).
        let seven = &lines[0].words[1];
        assert!(seven.estimated);
        assert!((seven.start - 0.30).abs() < 1e-9 && (seven.end - 0.40).abs() < 1e-9);
        assert!(seven.chars.is_empty());
        let cd = &lines[0].words[2];
        assert!((cd.start - 0.40).abs() < 1e-9);
        // Lines span their words.
        assert!((lines[0].start - 0.20).abs() < 1e-9 && (lines[0].end - 0.50).abs() < 1e-9);
        assert!((lines[1].start - 0.80).abs() < 1e-9 && (lines[1].end - 0.88).abs() < 1e-9);
        // Confidence is the geometric mean of the letters.
        assert!((ab.conf - (0.9f32 * 0.8).sqrt()).abs() < 1e-5);
        let _ = tokens_of;
    }

    #[test]
    fn estimated_words_at_the_edges_lean_on_their_one_neighbour() {
        let t = Transcript::parse("42 ab 99 100");
        let spans = vec![span(7, 50, 52, 0.9), span(24, 52, 55, 0.9)];
        let lines = place(&t, &spans);
        let w = &lines[0].words;
        assert!(w[0].estimated && w[0].end <= w[1].start + 1e-9 && w[0].start >= 0.0);
        assert!(w[2].estimated && w[3].estimated);
        assert!((w[2].start - 1.10).abs() < 1e-9, "starts where ab ends");
        assert!(w[2].end <= w[3].start + 1e-9 && w[3].end > w[3].start);
    }

    #[test]
    fn the_stats_compare_against_source_stamps_when_there_are_any() {
        let t = Transcript::parse("[00:01.00]ab\n[00:02.00]cd\nef");
        let spans = vec![
            span(7, 60, 62, 0.9),
            span(24, 62, 64, 0.9),
            span(4, 64, 65, 0.5),
            span(19, 110, 112, 0.3),
            span(14, 112, 114, 0.3),
            span(4, 114, 115, 0.5),
            span(5, 200, 202, 0.9),
            span(20, 202, 204, 0.9),
        ];
        let lines = place(&t, &spans);
        let s = stats(&lines, &t, 300);
        assert_eq!(s.words, 3);
        assert_eq!(s.estimated, 0);
        assert_eq!(s.uncertain, 1, "cd scored 0.3");
        // ab at 1.20 vs 1.00, cd at 2.20 vs 2.00: median +0.20, MAD 0.
        let (n, median, mad) = s.source_line_delta.expect("two stamped lines");
        assert_eq!(n, 2);
        assert!((median - 0.20).abs() < 1e-9 && mad.abs() < 1e-9);
    }
}
