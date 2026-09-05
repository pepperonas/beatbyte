//! The pipeline: audio and transcript in, an [`Alignment`] out.
//!
//! Decode → 16 kHz → emissions in windows → one Viterbi over the
//! whole song → spans back onto words and lines → provenance. Words
//! the model has no letters for are timed between their neighbours
//! and marked; nothing is dropped, so the karaoke text stays the
//! text the player gave.

use std::sync::atomic::{AtomicBool, Ordering};

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::resample::resample;
use beatbyte_ml::{Loaded, MlError, Runtime};
use thiserror::Error;

use crate::ctc::{AlignError, Emissions, TokenSpan, force_align_in_windows};
use crate::emissions::{FRAME_S, SAMPLE_RATE, compute_with};
use crate::transcript::{BLANK, Transcript, WORD_BOUNDARY};
use crate::words::{AlignedLine, AlignedWord, Alignment, SCHEMA, Source};

/// How the source's own line stamps constrain the alignment.
///
/// Measured on JamendoLyrics (`docs/lyrics/evaluation.md`), the
/// aligner's failure is a slide through a long instrumental: one in
/// four songs is lost that way, and the songs that are sung through
/// land at 0.28 s median error. A stamp per line — which lrclib gives
/// the game for nearly every song — bounds where each line's words
/// may sit, so a slide cannot travel past the next line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchoring {
    /// How far outside its own line a word may still land. Wide
    /// enough to absorb an ordinary master difference, far narrower
    /// than the slides being prevented.
    pub tolerance_s: f64,
    /// Below this share of stamped lines the stamps are not a grid
    /// and anchoring is skipped.
    pub min_stamped_share: f64,
}

impl Default for Anchoring {
    fn default() -> Anchoring {
        Anchoring {
            tolerance_s: 4.0,
            min_stamped_share: 0.5,
        }
    }
}

/// What an alignment run may do beyond the plain forced alignment.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Options {
    /// Constrain the second pass to the source's line stamps.
    /// `None` = the plain alignment, exactly as before.
    pub anchoring: Option<Anchoring>,
}

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

impl LyricsError {
    /// Whether this is the caller's own cancel, not a failure.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, LyricsError::Model(MlError::Cancelled { .. }))
    }
}

/// Where a running alignment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Bringing the audio to the model's rate.
    Resampling,
    /// Running the model, window by window (`done` of `total`).
    Emissions,
    /// The Viterbi over the whole song.
    Aligning,
}

/// A progress report: the stage and, for the windowed stage, how far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The stage.
    pub stage: Stage,
    /// Windows done (emissions only; 0 otherwise).
    pub done: usize,
    /// Windows total (emissions only; 0 otherwise).
    pub total: usize,
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
    /// Whether the source's line stamps constrained the result.
    pub anchored: bool,
}

/// Whether a source's line stamps can be believed enough to anchor
/// to at all — a structural question, answered before any alignment:
/// there must be enough of them, they must rise, and they must fit
/// inside this audio.
///
/// ⚠️ Deliberately NOT the gate's verdict. The gate judges by how
/// well an unanchored pass agreed with the stamps, and the songs that
/// need anchors most are exactly the ones where that pass derailed —
/// gating anchors on agreement would withhold them from the only
/// songs they could save. Pure — tested.
#[must_use]
pub fn stamps_are_usable(transcript: &Transcript, audio_len_s: f64, config: &Anchoring) -> bool {
    let stamps: Vec<f64> = transcript
        .lines
        .iter()
        .filter_map(|line| line.source_start_s)
        .collect();
    if transcript.lines.is_empty() {
        return false;
    }
    let share = stamps.len() as f64 / transcript.lines.len() as f64;
    if share < config.min_stamped_share || stamps.len() < 2 {
        return false;
    }
    let rising = stamps.windows(2).all(|w| w[1] >= w[0]);
    let last = stamps.last().copied().unwrap_or(0.0);
    // A stamp past the end of the file is a different edit, not a
    // late line (measured: one library song stamps to 272 s in 248 s
    // of audio).
    rising && last <= audio_len_s
}

/// The frame window each token may occupy, from the source's line
/// stamps shifted by `shift_s`. A line's tokens may sit between its
/// own stamp and the next one, plus the tolerance at both ends; a
/// line without a stamp inherits the room between its stamped
/// neighbours. `None` when the stamps cannot carry it. Pure — tested.
#[must_use]
pub fn token_windows(
    transcript: &Transcript,
    shift_s: f64,
    config: &Anchoring,
    frames: usize,
) -> Option<Vec<(usize, usize)>> {
    let count = transcript.lines.len();
    if count == 0 || frames == 0 {
        return None;
    }
    // Every line's anchor: its own stamp, or the nearest one before
    // it (a line with no stamp of its own must not be freer than the
    // line it follows).
    let mut starts: Vec<Option<f64>> = transcript
        .lines
        .iter()
        .map(|line| line.source_start_s.map(|s| s + shift_s))
        .collect();
    let mut carry: Option<f64> = None;
    for start in &mut starts {
        match *start {
            Some(value) => carry = Some(value),
            None => *start = carry,
        }
    }
    let mut carry: Option<f64> = None;
    let mut ends: Vec<Option<f64>> = vec![None; count];
    for index in (0..count).rev() {
        // A line's room ends where the NEXT stamped line begins.
        ends[index] = carry;
        if let Some(stamp) = transcript.lines[index].source_start_s {
            carry = Some(stamp + shift_s);
        }
    }
    let to_frame = |seconds: f64| -> usize {
        let frame = (seconds / FRAME_S).floor();
        frame.clamp(0.0, frames as f64) as usize
    };
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for (index, line) in transcript.lines.iter().enumerate() {
        let from = starts[index].map_or(0.0, |s| s - config.tolerance_s);
        let to = ends[index].map_or(f64::INFINITY, |e| e + config.tolerance_s);
        let mut window = (
            to_frame(from),
            if to.is_finite() { to_frame(to) } else { frames },
        );
        // A window has to hold its own line: one frame per token,
        // plus one between two equal ones. Widened forward, since a
        // line that starts late is likelier than one that started
        // before its stamp.
        let tokens: usize = line
            .words
            .iter()
            .map(|word| word.tokens.len() + usize::from(!word.tokens.is_empty()))
            .sum();
        if window.1.saturating_sub(window.0) < tokens {
            window.1 = (window.0 + tokens).min(frames);
            if window.1.saturating_sub(window.0) < tokens {
                window.0 = window.1.saturating_sub(tokens);
            }
        }
        for word in &line.words {
            if word.tokens.is_empty() {
                continue;
            }
            if !windows.is_empty() {
                windows.push(window); // the boundary before this word
            }
            windows.extend(std::iter::repeat_n(window, word.tokens.len()));
        }
    }
    (windows.len() == transcript.tokens().len()).then_some(windows)
}

/// The constant the source's stamps are off by, from a first pass:
/// the median of the line deltas that agree with each other. `0.0`
/// when there is no agreement — a derailed pass says nothing about
/// the shift, and the stamps are then taken as they are. Pure —
/// tested.
#[must_use]
pub fn shift_from(lines: &[AlignedLine], transcript: &Transcript) -> f64 {
    let pairs: Vec<(f64, f64)> = lines
        .iter()
        .zip(&transcript.lines)
        .filter(|(line, _)| line.words.iter().any(|w| !w.estimated))
        .filter_map(|(line, source)| source.source_start_s.map(|s| (s, line.start - s)))
        .collect();
    let judged =
        crate::gate::verdict_of(&pairs, f64::INFINITY, &crate::gate::GateConfig::default());
    match judged.verdict {
        crate::gate::Verdict::SameMaster | crate::gate::Verdict::ShiftedMaster { .. } => {
            judged.median.unwrap_or(0.0)
        }
        _ => 0.0,
    }
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
    align_with(
        audio,
        audio_sha256,
        transcript,
        text_source,
        runtime,
        model,
        &Options::default(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

/// The cancel error for this model — the flag is the caller's, the
/// error names what was being run.
fn cancelled(model: &Loaded) -> LyricsError {
    MlError::Cancelled {
        id: model.id.to_owned(),
    }
    .into()
}

/// [`align`] with progress reports and a cancel flag (checked between
/// stages and between model windows; the Viterbi itself runs to the
/// end — seconds).
#[allow(clippy::too_many_arguments)] // the pipeline's inputs, not an API to grow
pub fn align_with(
    audio: &AudioData,
    audio_sha256: &str,
    transcript: &Transcript,
    text_source: &str,
    runtime: &Runtime,
    model: &Loaded,
    options: &Options,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<AlignOutcome, LyricsError> {
    let tokens = transcript.tokens();
    if tokens.is_empty() {
        return Err(LyricsError::NoWords);
    }
    let report = |stage, done, total| Progress { stage, done, total };
    progress(report(Stage::Resampling, 0, 0));
    let samples = resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE);
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled(model));
    }
    let emissions = compute_with(
        runtime,
        model,
        &samples,
        &mut |done, total| progress(report(Stage::Emissions, done, total)),
        cancel,
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled(model));
    }
    progress(report(Stage::Aligning, 0, 0));
    let (lines, anchored) = align_emissions(
        &emissions,
        &tokens,
        transcript,
        audio.duration_s(),
        options.anchoring.as_ref(),
    )?;
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
    Ok(AlignOutcome {
        alignment,
        stats,
        anchored,
    })
}

/// The alignment itself, over emissions that are already computed:
/// the plain pass, and — when the source's stamps can carry it — a
/// second pass constrained to them. Returns the lines and whether the
/// stamps constrained them.
///
/// The two passes cost one extra Viterbi over the same emissions,
/// which is seconds against the model's minutes; the first pass earns
/// its keep by measuring how far the source's master is off.
///
/// **An anchored pass that turns out impossible is never fatal**: the
/// plain result stands, and the caller is told nothing was anchored.
pub fn align_emissions(
    emissions: &Emissions,
    tokens: &[u8],
    transcript: &Transcript,
    audio_len_s: f64,
    anchoring: Option<&Anchoring>,
) -> Result<(Vec<AlignedLine>, bool), LyricsError> {
    let spans = force_align_in_windows(emissions, tokens, BLANK, &[])?;
    let lines = place(transcript, &spans);
    let Some(config) = anchoring else {
        return Ok((lines, false));
    };
    if !stamps_are_usable(transcript, audio_len_s, config) {
        return Ok((lines, false));
    }
    let shift = shift_from(&lines, transcript);
    let Some(windows) = token_windows(transcript, shift, config, emissions.frames) else {
        return Ok((lines, false));
    };
    match force_align_in_windows(emissions, tokens, BLANK, &windows) {
        Ok(spans) => Ok((place(transcript, &spans), true)),
        Err(_) => Ok((lines, false)),
    }
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
mod anchor_tests {
    use super::*;
    use crate::transcript::Transcript;

    fn stamped(text: &str) -> Transcript {
        Transcript::parse(text)
    }

    #[test]
    fn stamps_are_judged_structurally_not_by_how_well_a_pass_agreed() {
        let config = Anchoring::default();
        let good = stamped("[00:10.00]one two\n[00:20.00]three four\n[00:30.00]five six");
        assert!(stamps_are_usable(&good, 200.0, &config));
        // Past the end of the audio: a different edit, not late lines.
        assert!(!stamps_are_usable(&good, 25.0, &config));
        // Going backwards is not a grid.
        let jumbled = stamped("[00:30.00]one\n[00:10.00]two\n[00:20.00]three");
        assert!(!stamps_are_usable(&jumbled, 200.0, &config));
        // Too few stamped lines to be a grid at all.
        let sparse = stamped("[00:10.00]one\nplain\nplain\nplain\nplain");
        assert!(!stamps_are_usable(&sparse, 200.0, &config));
        // No stamps, no anchoring.
        assert!(!stamps_are_usable(&stamped("one\ntwo"), 200.0, &config));
    }

    #[test]
    fn a_lines_words_may_only_sit_between_its_own_stamp_and_the_next() {
        let transcript = stamped("[00:10.00]ab cd\n[00:20.00]ef gh");
        let config = Anchoring {
            tolerance_s: 1.0,
            ..Anchoring::default()
        };
        let frames = (60.0 / FRAME_S) as usize;
        let windows = token_windows(&transcript, 0.0, &config, frames).expect("windows");
        assert_eq!(windows.len(), transcript.tokens().len());
        let frame = |seconds: f64| (seconds / FRAME_S) as usize;
        // Line 1: from 9 s (10 − tolerance) to 21 s (the next stamp
        // + tolerance).
        assert_eq!(windows[0], (frame(9.0), frame(21.0)));
        // Line 2 starts at 19 s and runs to the end of the audio.
        let last = windows.last().copied().expect("a window");
        assert_eq!(last, (frame(19.0), frames));
        // A shift moves the whole grid.
        let shifted = token_windows(&transcript, 2.0, &config, frames).expect("windows");
        assert_eq!(shifted[0], (frame(11.0), frame(23.0)));
    }

    #[test]
    fn a_window_too_small_for_its_line_is_widened_rather_than_left_impossible() {
        // Two stamps 0.05 s apart with a whole line between them:
        // the tokens cannot fit, so the window has to grow or the
        // alignment would be impossible for a reason the source
        // caused, not the audio.
        let transcript = stamped("[00:10.00]abcdefghij klmnopqrst\n[00:10.05]xy");
        let config = Anchoring {
            tolerance_s: 0.0,
            ..Anchoring::default()
        };
        let frames = (60.0 / FRAME_S) as usize;
        let windows = token_windows(&transcript, 0.0, &config, frames).expect("windows");
        let first = transcript.lines[0]
            .words
            .iter()
            .map(|w| w.tokens.len() + 1)
            .sum::<usize>();
        assert!(
            windows[0].1 - windows[0].0 >= first,
            "the line's own tokens must fit: {:?} for {first}",
            windows[0]
        );
    }

    #[test]
    fn the_shift_comes_from_agreement_and_is_zero_without_it() {
        let transcript = stamped("[00:10.00]ab\n[00:20.00]cd\n[00:30.00]ef\n[00:40.00]gh");
        let line = |start: f64| AlignedLine {
            start,
            end: start + 0.5,
            text: "x".to_owned(),
            words: vec![AlignedWord {
                text: "x".to_owned(),
                start,
                end: start + 0.5,
                conf: 0.5,
                estimated: false,
                chars: vec![[start, start + 0.5]],
            }],
        };
        // Every line 2 s late and agreeing: that is the shift.
        let agreeing: Vec<AlignedLine> = [12.0, 22.0, 32.0, 42.0].into_iter().map(line).collect();
        assert!((shift_from(&agreeing, &transcript) - 2.0).abs() < 1e-9);
        // A derailed pass agrees on nothing and must not invent a
        // shift - the stamps are then taken as they are.
        let derailed: Vec<AlignedLine> = [1.0, 90.0, 15.0, 200.0].into_iter().map(line).collect();
        assert!(shift_from(&derailed, &transcript).abs() < 1e-9);
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
