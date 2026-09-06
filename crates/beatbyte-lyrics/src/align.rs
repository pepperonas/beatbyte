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
use crate::evidence::Evidence;
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
    /// How far outside its own line a word may still land when the
    /// source's offset is NOT known — wide enough to absorb an
    /// ordinary master difference, far narrower than the slides being
    /// prevented.
    ///
    /// ⚠️ Measured, and the measurement overturned the obvious
    /// answer. On a corpus whose stamps are on time, a tight window
    /// beats a wide one on every number (PCO@0.1 51.7 % at ±1 s
    /// against 46.7 % at ±4 s). Put the same stamps three seconds off
    /// and it reverses: 38.4 % against 45.4 %, and a song is lost —
    /// because a window that cannot hold the truth forces words
    /// somewhere they are not. So the width follows what is KNOWN:
    /// see [`Anchoring::known_shift_tolerance_s`].
    pub tolerance_s: f64,
    /// The window once the first pass has AGREED on the source's
    /// offset. The offset is then removed, so the stamps are as good
    /// as the corpus's own and the window can close in.
    pub known_shift_tolerance_s: f64,
    /// Below this share of stamped lines the stamps are not a grid
    /// and anchoring is skipped.
    pub min_stamped_share: f64,
}

impl Default for Anchoring {
    fn default() -> Anchoring {
        Anchoring {
            tolerance_s: 4.0,
            known_shift_tolerance_s: 1.0,
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
    /// The audio handed in to listen to is not the song on the
    /// song's timeline (see [`HEARD_LENGTH_TOLERANCE_S`]).
    #[error(
        "the stem is {heard_s:.3} s long where the song is {song_s:.3} s — not this song on \
         this timeline (a separator fed the container instead of the decoded song?)"
    )]
    HeardMismatch {
        /// The song's length, seconds.
        song_s: f64,
        /// The stem's length, seconds.
        heard_s: f64,
    },
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
    /// How many Viterbi passes the result cost: 1 when the source's
    /// stamps could not be used at all (or every anchored pass
    /// saturated its window and the plain pass stands), 2 when they
    /// constrained a second pass, one more when that pass sat at its
    /// windows' edge and a wider window was tried, one more when a
    /// pass then agreed on an offset the first one could not see and
    /// a tighter pass was worth it.
    pub passes: u8,
    /// How much of the song the model heard at all — measured on the
    /// same emissions, so it costs nothing. The gate needs it: an
    /// alignment on a mix the model cannot read is a path, not
    /// evidence. See [`crate::evidence`].
    pub evidence: Evidence,
    /// When the source's stamps turned out to belong to another edit
    /// of this performance and a linear map put them onto this
    /// recording: the map, the retimed transcript the anchoring ran
    /// against, and the lines the map put beyond the sound. The gate
    /// judges against the retimed stamps and drops the unsung lines.
    pub warp: Option<WarpResult>,
}

/// A linear map from a source's line stamps onto this recording —
/// the stamps were made on another EDIT of the same performance: a
/// longer intro, a tempo a few percent off, verses this cut does not
/// have. Measured on the case that found it (Böhse Onkelz, *Mexico*):
/// stamps from a 254 s studio version on a 168 s recording, aligned
/// ≈ 1.0396 · stamp − 10.83 s over the first twenty-four lines with a
/// mean residual of 0.37 s, the remaining fifteen lines never sung.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warp {
    /// The tempo ratio, this recording over the source's.
    pub scale: f64,
    /// Seconds added after scaling.
    pub offset_s: f64,
}

/// What retiming the source's stamps produced: a map, when the
/// stamps needed one; and the lines the sound does not hold, which
/// a plain constant shift can reveal as well (a text whose last
/// verses lie past where this recording ends).
#[derive(Debug, Clone, PartialEq)]
pub struct WarpResult {
    /// The map — `None` when the stamps sat a constant off the song
    /// and only the unsung lines were taken away.
    pub warp: Option<Warp>,
    /// The transcript with every stamp mapped (or left as it was);
    /// unsung lines carry no stamp.
    pub retimed: Transcript,
    /// Indices of the lines the map put beyond the sound (or before
    /// it): text this recording does not sing.
    pub unsung: Vec<usize>,
}

/// Fewest stamped, placed lines a warp may be fitted on.
pub const WARP_MIN_LINES: usize = 8;
/// The median residual, seconds, the better-heard half must stay
/// under for the fit to count. Up to [`WARP_TIGHT_RESIDUAL_S`] the
/// mapped stamps are as good as a source's own; above it the map is
/// coarse — a jump of a bar at one point and a slow drift after it
/// (France Gall, Fettes Brot in the library) — and the anchored
/// windows are kept at least twice the residual wide.
pub const WARP_MAX_RESIDUAL_S: f64 = 2.0;
/// A map whose residual is under this places lines to within the
/// tight window.
pub const WARP_TIGHT_RESIDUAL_S: f64 = 0.75;
/// Tempo ratios outside this are not the same performance.
pub const WARP_SCALE_RANGE: (f64, f64) = (0.9, 1.1);
/// Share of the in-sound lines the map must place within
/// [`WARP_EXPLAINED_S`] for it to count.
pub const WARP_MIN_SHARE: f64 = 0.6;
/// How far a line may sit from where the map puts it and still be
/// explained by it.
pub const WARP_EXPLAINED_S: f64 = 3.0;
/// A line the map puts closer than this to the sound's end — or
/// before its start — is not sung in this recording.
pub const UNSUNG_MARGIN_S: f64 = 0.5;

impl Warp {
    /// Where the map puts a source stamp.
    #[must_use]
    pub fn apply(self, stamp_s: f64) -> f64 {
        self.scale * stamp_s + self.offset_s
    }

    /// The transcript's stamps mapped onto this recording. A line the
    /// map puts beyond the sound (or before it) loses its stamp — it
    /// is not sung here — and is reported. Pure — tested.
    #[must_use]
    pub fn retime(self, transcript: &Transcript, sounding_end_s: f64) -> (Transcript, Vec<usize>) {
        let mut retimed = transcript.clone();
        let mut unsung = Vec::new();
        for (index, line) in retimed.lines.iter_mut().enumerate() {
            let Some(stamp) = line.source_start_s else {
                continue;
            };
            let mapped = self.apply(stamp);
            if mapped < 0.0 || mapped > sounding_end_s - UNSUNG_MARGIN_S {
                line.source_start_s = None;
                unsung.push(index);
            } else {
                line.source_start_s = Some(mapped);
            }
        }
        (retimed, unsung)
    }
}

/// The transcript with the lines a constant shift puts beyond the
/// sound (or before it) stripped of their stamp, and their indices:
/// a text with more verses than this recording sings, on a source
/// that otherwise agrees with the song. Pure — tested.
#[must_use]
pub fn trim_unsung(
    transcript: &Transcript,
    shift_s: f64,
    sounding_end_s: f64,
) -> (Transcript, Vec<usize>) {
    let mut trimmed = transcript.clone();
    let mut unsung = Vec::new();
    for (index, line) in trimmed.lines.iter_mut().enumerate() {
        let Some(stamp) = line.source_start_s else {
            continue;
        };
        let placed = stamp + shift_s;
        if placed < 0.0 || placed > sounding_end_s - UNSUNG_MARGIN_S {
            line.source_start_s = None;
            unsung.push(index);
        }
    }
    (trimmed, unsung)
}

/// Fit a [`Warp`] from a plain pass against the source's stamps:
/// least squares over the better-heard half of the stamped lines,
/// accepted only when it explains them — the residual small, the
/// tempo ratio plausible, and most of the lines that land inside
/// the sound sitting where the map puts them. `None` is the usual
/// answer: a derailed pass has no line to fit, and stamps that
/// merely sit on another master agree on a constant and never get
/// here. Pure — tested.
#[must_use]
pub fn fit_warp(
    lines: &[AlignedLine],
    transcript: &Transcript,
    sounding_end_s: f64,
) -> Option<(Warp, f64)> {
    let points: Vec<(f64, f64, f64)> = lines
        .iter()
        .zip(&transcript.lines)
        .filter(|(line, _)| line.words.iter().any(|w| !w.estimated))
        .filter_map(|(line, source)| {
            source.source_start_s.map(|stamp| {
                (
                    stamp,
                    line.start,
                    pass_confidence(std::slice::from_ref(line)),
                )
            })
        })
        .collect();
    if points.len() < WARP_MIN_LINES {
        return None;
    }
    let mut confs: Vec<f64> = points.iter().map(|p| p.2).collect();
    confs.sort_by(f64::total_cmp);
    let median_conf = confs[confs.len() / 2];
    let heard: Vec<(f64, f64)> = points
        .iter()
        .filter(|p| p.2 >= median_conf)
        .map(|p| (p.0, p.1))
        .collect();
    if heard.len() < WARP_MIN_LINES / 2 {
        return None;
    }
    // Theil–Sen, not least squares: the better-heard half still
    // carries lines the plain pass lost (on Mexico, four of twenty,
    // one of them 77 s out), and one such line drags a least-squares
    // slope from 1.04 to 1.00 and the residual past the limit. The
    // median of the pairwise slopes shrugs them off.
    let mut slopes: Vec<f64> = Vec::with_capacity(heard.len() * heard.len() / 2);
    for (i, a) in heard.iter().enumerate() {
        for b in &heard[i + 1..] {
            if (b.0 - a.0).abs() > 1e-9 {
                slopes.push((b.1 - a.1) / (b.0 - a.0));
            }
        }
    }
    if slopes.is_empty() {
        return None;
    }
    slopes.sort_by(f64::total_cmp);
    let scale = slopes[slopes.len() / 2];
    let mut offsets: Vec<f64> = heard.iter().map(|p| p.1 - scale * p.0).collect();
    offsets.sort_by(f64::total_cmp);
    let warp = Warp {
        scale,
        offset_s: offsets[offsets.len() / 2],
    };
    if warp.scale < WARP_SCALE_RANGE.0 || warp.scale > WARP_SCALE_RANGE.1 {
        return None;
    }
    let mut residuals: Vec<f64> = heard
        .iter()
        .map(|p| (p.1 - warp.apply(p.0)).abs())
        .collect();
    residuals.sort_by(f64::total_cmp);
    let residual_s = residuals[residuals.len() / 2];
    if residual_s > WARP_MAX_RESIDUAL_S {
        return None;
    }
    let in_sound: Vec<&(f64, f64, f64)> = points
        .iter()
        .filter(|p| {
            let mapped = warp.apply(p.0);
            mapped >= 0.0 && mapped <= sounding_end_s - UNSUNG_MARGIN_S
        })
        .collect();
    if in_sound.is_empty() {
        return None;
    }
    let explained = in_sound
        .iter()
        .filter(|p| (p.1 - warp.apply(p.0)).abs() <= WARP_EXPLAINED_S)
        .count();
    (explained as f64 / in_sound.len() as f64 >= WARP_MIN_SHARE).then_some((warp, residual_s))
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

/// How wide the anchored pass's windows may be, given what the first
/// pass agreed on: the tight window once the offset is known and
/// removed, the wide one while it is not. Pure — tested.
///
/// ⚠️ The whole point of the distinction. Measured on the corpus, a
/// ±1 s window beats ±4 s on every number when the stamps are on
/// time — and loses to it, badly, when they are three seconds off.
/// Choosing by agreement gets both.
#[must_use]
pub fn tolerance_for(agreed_shift_s: Option<f64>, config: &Anchoring) -> f64 {
    if agreed_shift_s.is_some() {
        config.known_shift_tolerance_s
    } else {
        config.tolerance_s
    }
}

/// The constant the source's stamps are off by, from a first pass:
/// the median of the line deltas that agree with each other.
///
/// `None` when there is no agreement — a derailed pass says nothing
/// about the shift, and the caller must then both take the stamps as
/// they are AND leave the window wide enough for an offset it cannot
/// see. Pure — tested.
#[must_use]
pub fn shift_from(lines: &[AlignedLine], transcript: &Transcript) -> Option<f64> {
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
            judged.median
        }
        _ => None,
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
        None,
        audio_sha256,
        transcript,
        &Provenance {
            text: text_source,
            separator: NO_SEPARATOR,
        },
        runtime,
        model,
        &Options::default(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

/// The `separator` a result records when the model listened to the
/// mix itself.
pub const NO_SEPARATOR: &str = "none";

/// How far, in seconds, a stem's length may differ from the song's
/// before it is refused as not being this song on this timeline.
///
/// A separator returns the length it was given; a stem that is
/// longer by a container's encoder priming (23 ms for FFmpeg's AAC,
/// 48 ms for Apple's, at 44.1 kHz) was decoded from the container by
/// a tool that did not skip it — and every word would then sit that
/// far early. The tolerance is set under the smaller priming so that
/// case is caught by construction, and above the padding a decoder
/// may keep or drop at the tail.
pub const HEARD_LENGTH_TOLERANCE_S: f64 = 0.015;

/// Where the inputs of an alignment came from, recorded verbatim in
/// the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance<'a> {
    /// The lyric text's origin (`lrclib`, `file:<name>`, …).
    pub text: &'a str,
    /// What produced the audio the model listened to
    /// ([`NO_SEPARATOR`] for the mix itself).
    pub separator: &'a str,
}

/// Refuse a stem that is not the song on the song's timeline. Pure —
/// tested.
pub fn check_heard_matches(song: &AudioData, heard: &AudioData) -> Result<(), LyricsError> {
    let difference_s = (song.duration_s() - heard.duration_s()).abs();
    if difference_s > HEARD_LENGTH_TOLERANCE_S {
        return Err(LyricsError::HeardMismatch {
            song_s: song.duration_s(),
            heard_s: heard.duration_s(),
        });
    }
    Ok(())
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
    heard: Option<&AudioData>,
    audio_sha256: &str,
    transcript: &Transcript,
    provenance: &Provenance,
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
    if let Some(heard) = heard {
        check_heard_matches(audio, heard)?;
    }
    let listened = heard.unwrap_or(audio);
    let report = |stage, done, total| Progress { stage, done, total };
    progress(report(Stage::Resampling, 0, 0));
    let samples = resample(listened.samples(), listened.sample_rate(), SAMPLE_RATE);
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
    let anchored = align_emissions(
        &emissions,
        &tokens,
        transcript,
        audio.duration_s(),
        // The SONG's sound, not the stem's: a stem is silent wherever
        // nobody sings, and that is not where the recording ends.
        audio.sounding_end_s(SOUNDING_FLOOR),
        options.anchoring.as_ref(),
    )?;
    let (lines, passes, warp) = (anchored.lines, anchored.passes, anchored.warp);
    let stats = stats(&lines, transcript, emissions.frames);
    let alignment = Alignment {
        schema: SCHEMA.to_owned(),
        audio_sha256: audio_sha256.to_owned(),
        pipeline_version: crate::PIPELINE_VERSION,
        language: "en".to_owned(),
        source: Source {
            text: provenance.text.to_owned(),
            separator: provenance.separator.to_owned(),
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
        passes,
        evidence: crate::evidence::measure(&emissions, usize::from(BLANK), FRAME_S),
        warp,
    })
}

/// Linear amplitude under which a sample is silence for
/// [`AudioData::sounding_end_s`] (−60 dBFS).
pub const SOUNDING_FLOOR: f32 = 0.001;

/// What the anchored alignment produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchored {
    /// The lines.
    pub lines: Vec<AlignedLine>,
    /// How many Viterbi passes they cost.
    pub passes: u8,
    /// The warp, when the source's stamps needed one.
    pub warp: Option<WarpResult>,
}

/// The alignment itself, over emissions that are already computed:
/// the plain pass, and — when the source's stamps can carry it — a
/// second constrained to them, and sometimes a third. Returns the
/// lines and how many passes they cost.
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
    sounding_end_s: f64,
    anchoring: Option<&Anchoring>,
) -> Result<Anchored, LyricsError> {
    let spans = force_align_in_windows(emissions, tokens, BLANK, &[])?;
    let lines = place(transcript, &spans);
    let plain = |lines| Anchored {
        lines,
        passes: 1,
        warp: None,
    };
    let Some(config) = anchoring else {
        return Ok(plain(lines));
    };
    let agreed = shift_from(&lines, transcript);
    // Stamps that sit a constant off the song may still run past
    // where it ends: a text with the album's last verses on a
    // recording that stops before them. Those lines are not sung
    // here; without their stamps the rest anchors as any agreeing
    // source does, and the gate drops them. (Four library songs
    // were "a different edit" for this alone, with their other lines
    // agreeing to within a tenth of a second.)
    if let Some(shift) = agreed {
        let (trimmed, unsung) = trim_unsung(transcript, shift, sounding_end_s);
        if !unsung.is_empty() && stamps_are_usable(&trimmed, audio_len_s, config) {
            let (anchored, passes) = anchor(emissions, tokens, &trimmed, lines, config, 0.0);
            return Ok(Anchored {
                lines: anchored,
                passes,
                warp: Some(WarpResult {
                    warp: None,
                    retimed: trimmed,
                    unsung,
                }),
            });
        }
    }
    // Stamps that agree with the plain pass on nothing constant may
    // still agree on a LINE: another edit of the same performance,
    // a few percent off in tempo and seconds off at the start, with
    // verses this recording never sings. Then the stamps are mapped
    // onto this recording first and the anchoring runs against the
    // mapped ones, which are as good as any other source's. Tried
    // BEFORE the raw stamps are judged usable: a text whose last
    // stamps run past the file (Easy Lover: 295 s of stamps on a
    // 287 s file, the song 4 % faster than its sheet) is exactly what
    // the map exists for, and the mapped stamps are what is judged.
    if agreed.is_none()
        && let Some((warp, residual_s)) = fit_warp(&lines, transcript, sounding_end_s)
    {
        let (retimed, unsung) = warp.retime(transcript, sounding_end_s);
        if stamps_are_usable(&retimed, audio_len_s, config) {
            // A coarse map leaves lines a second or two off where it
            // puts them; a window narrower than that would push them.
            let floor_s = if residual_s > WARP_TIGHT_RESIDUAL_S {
                2.0 * residual_s
            } else {
                0.0
            };
            let (anchored, passes) =
                anchor(emissions, tokens, &retimed, lines.clone(), config, floor_s);
            return Ok(Anchored {
                lines: anchored,
                passes,
                warp: Some(WarpResult {
                    warp: Some(warp),
                    retimed,
                    unsung,
                }),
            });
        }
    }
    if !stamps_are_usable(transcript, audio_len_s, config) {
        return Ok(plain(lines));
    }
    let (anchored, passes) = anchor(emissions, tokens, transcript, lines, config, 0.0);
    Ok(Anchored {
        lines: anchored,
        passes,
        warp: None,
    })
}

/// The anchored passes over a plain pass `lines`, against
/// `transcript`'s stamps: the shift the plain pass agreed on decides
/// the window, an anchored pass pushed against the evidence gets one
/// wider window, and a pass that then agrees on an offset gets a
/// tighter one. `floor_s` is the narrowest any window may be — for
/// stamps a coarse map placed, wider than the tight default. Returns
/// the lines and the passes they cost in total (the plain one
/// included).
fn anchor(
    emissions: &Emissions,
    tokens: &[u8],
    transcript: &Transcript,
    lines: Vec<AlignedLine>,
    config: &Anchoring,
    floor_s: f64,
) -> (Vec<AlignedLine>, u8) {
    // What the first pass agreed on decides BOTH the offset and how
    // tight the windows may be: a known offset is removed, so the
    // stamps become as good as ground truth and the window can close
    // in; an unknown one has to fit inside the window instead.
    let agreed = shift_from(&lines, transcript);
    let applied = agreed.unwrap_or(0.0);
    let tolerance = tolerance_for(agreed, config).max(floor_s);
    let plain_conf = pass_confidence(&lines);
    let Some(mut anchored) = anchored_pass(emissions, tokens, transcript, applied, tolerance)
    else {
        return (lines, 1);
    };
    let mut anchored_conf = pass_confidence(&anchored);
    let mut passes = 2;
    // A window that cannot hold the truth does not report that: it
    // places the words at its own edge, and on legible audio the
    // result then looks like evidence (Mexico: stamps ten seconds
    // late, a ±4 s window, "shifted master −3.65 s, 85 % agreement").
    // Two signs give it away — the pass sits at its windows' edge,
    // or its words' confidence collapses against the plain pass's
    // (Mexico: 0.005 against 0.032, the model heard the verse ten
    // seconds earlier and the window would not let it say so). Then
    // the window is opened wider, once; if the evidence still says
    // pushed, the stamps are not this recording's and the plain pass
    // stands for the gate to judge.
    if window_saturated(&anchored, transcript, applied, tolerance)
        || pushed_against_evidence(anchored_conf, plain_conf)
    {
        let wider = tolerance * WIDER_BY;
        let opened = anchored_pass(emissions, tokens, transcript, applied, wider)
            .map(|opened| (pass_confidence(&opened), opened));
        match opened {
            Some((conf, opened)) if !pushed_against_evidence(conf, plain_conf) => {
                anchored = opened;
                anchored_conf = conf;
                passes += 1;
            }
            _ => return (lines, 1),
        }
    }
    // A song whose FIRST pass derailed never agreed on an offset, so
    // it got the wide window — even though the anchored pass then
    // places it well. Ask that pass: it no longer derails, and if it
    // agrees, the offset is known after all and the window can close
    // in. Only for the songs that needed it, and only once — and a
    // tighter pass that is pushed against the pass it came from
    // would mean the offset was the window's, not the song's; then
    // the wider result stands.
    if agreed.is_none()
        && let Some(from_anchored) = shift_from(&anchored, transcript)
    {
        let tight = config.known_shift_tolerance_s.max(floor_s);
        if let Some(tighter) = anchored_pass(emissions, tokens, transcript, from_anchored, tight)
            && !window_saturated(&tighter, transcript, from_anchored, tight)
            && !pushed_against_evidence(pass_confidence(&tighter), anchored_conf)
        {
            return (tighter, passes + 1);
        }
    }
    (anchored, passes)
}

/// Below this share of the plain pass's confidence an anchored pass
/// counts as pushed against the evidence rather than placed by it.
/// Measured on the one known case (0.005 against 0.032 = 17 %); a
/// pass the window merely tidied keeps most of it.
pub const PUSHED_SHARE: f64 = 0.5;

/// The confidence an alignment carries as a whole: the geometric
/// mean of its aligned words' confidence (words without letters do
/// not count; no words, no confidence). The geometric mean, so a
/// pass that placed half its words well and lost the rest does not
/// score as if it had placed them all. Pure — tested.
#[must_use]
pub fn pass_confidence(lines: &[AlignedLine]) -> f64 {
    let confs: Vec<f64> = lines
        .iter()
        .flat_map(|line| line.words.iter())
        .filter(|word| !word.estimated && !word.chars.is_empty())
        .map(|word| f64::from(word.conf).max(1e-6))
        .collect();
    if confs.is_empty() {
        return 0.0;
    }
    (confs.iter().map(|c| c.ln()).sum::<f64>() / confs.len() as f64).exp()
}

/// Whether a constrained pass lost too much of the evidence an
/// unconstrained one had found (see [`PUSHED_SHARE`]). A plain pass
/// with no confidence at all pushes nothing. Pure — tested.
#[must_use]
pub fn pushed_against_evidence(constrained: f64, plain: f64) -> bool {
    plain > 0.0 && constrained < plain * PUSHED_SHARE
}

/// How much wider the anchored window is opened, once, when the
/// first anchored pass sat at its edge.
pub const WIDER_BY: f64 = 3.0;

/// The outer share of an anchored window that counts as its edge:
/// a pass whose median line lands there was pushed, not placed.
/// Measured on the one known case (−3.65 s in a ±4 s window = 91 %);
/// a stamp jitter of ±0.5 s in a ±4 s window stays well inside.
pub const EDGE_SHARE: f64 = 0.85;

/// Whether an anchored pass was pushed to its windows' edge rather
/// than placed inside them: the median of the lines' distance from
/// their (shifted) stamps lies in the outer [`EDGE_SHARE`] of the
/// window. Lines without a stamp or without aligned letters do not
/// vote; fewer than two votes say nothing. Pure — tested.
#[must_use]
pub fn window_saturated(
    lines: &[AlignedLine],
    transcript: &Transcript,
    applied_shift_s: f64,
    tolerance_s: f64,
) -> bool {
    let mut deltas: Vec<f64> = lines
        .iter()
        .zip(&transcript.lines)
        .filter(|(line, _)| line.words.iter().any(|w| !w.estimated))
        .filter_map(|(line, source)| {
            source
                .source_start_s
                .map(|s| line.start - (s + applied_shift_s))
        })
        .collect();
    if deltas.len() < 2 {
        return false;
    }
    deltas.sort_by(f64::total_cmp);
    let median = deltas[deltas.len() / 2];
    median.abs() >= tolerance_s * EDGE_SHARE
}

/// One anchored pass: windows from the source's stamps shifted by
/// `shift_s`, `tolerance_s` wide at both ends. `None` when the stamps
/// cannot carry windows or the constrained path does not exist — the
/// caller then keeps what it had.
fn anchored_pass(
    emissions: &Emissions,
    tokens: &[u8],
    transcript: &Transcript,
    shift_s: f64,
    tolerance_s: f64,
) -> Option<Vec<AlignedLine>> {
    let narrowed = Anchoring {
        tolerance_s,
        min_stamped_share: 0.0,
        known_shift_tolerance_s: tolerance_s,
    };
    let windows = token_windows(transcript, shift_s, &narrowed, emissions.frames)?;
    force_align_in_windows(emissions, tokens, BLANK, &windows)
        .ok()
        .map(|spans| place(transcript, &spans))
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

    /// Emissions where each `(token, frames)` in the plan is nearly
    /// certain over those frames — the same shape the CTC tests use.
    fn synthetic(vocab: usize, plan: &[(u8, usize)]) -> Emissions {
        let mut log_probs = Vec::new();
        let hot = 0.9f32.ln();
        let cold = ((1.0 - 0.9) / (vocab as f32 - 1.0)).ln();
        for &(token, frames) in plan {
            for _ in 0..frames {
                for v in 0..vocab {
                    log_probs.push(if v == usize::from(token) { hot } else { cold });
                }
            }
        }
        Emissions {
            frames: log_probs.len() / vocab,
            vocab,
            log_probs,
        }
    }

    #[test]
    fn a_derailed_first_pass_gets_a_second_opinion_before_the_window_closes() {
        // Two words the model is sure of, seconds apart, and stamps
        // that are RIGHT. The unanchored pass finds them, so it
        // agrees straight away: two passes, no third needed.
        let vocab = 32usize;
        let (a, b) = (
            crate::transcript::token_of('A').expect("A"),
            crate::transcript::token_of('B').expect("B"),
        );
        let frame = |seconds: f64| (seconds / FRAME_S) as usize;
        let plan = vec![
            (BLANK, frame(1.0)),
            (a, 10),
            (BLANK, frame(1.0)),
            (b, 10),
            (BLANK, frame(1.0)),
        ];
        let emissions = synthetic(vocab, &plan);
        let transcript = Transcript::parse("[00:01.00]a\n[00:02.20]b");
        let tokens = transcript.tokens();
        let config = Anchoring::default();
        let passes = align_emissions(&emissions, &tokens, &transcript, 5.0, 5.0, Some(&config))
            .expect("aligns")
            .passes;
        assert_eq!(passes, 2, "stamps that agree need no second opinion");
        // Without anchoring at all it is one pass, whatever the
        // stamps say.
        let passes = align_emissions(&emissions, &tokens, &transcript, 5.0, 5.0, None)
            .expect("aligns")
            .passes;
        assert_eq!(passes, 1);
        // Stamps this audio cannot carry (they run past its end) are
        // refused before any anchored pass.
        let passes = align_emissions(&emissions, &tokens, &transcript, 1.5, 1.5, Some(&config))
            .expect("aligns")
            .passes;
        assert_eq!(passes, 1, "stamps past the end anchor nothing");
    }

    #[test]
    fn the_window_closes_in_only_when_the_offset_is_known() {
        // The rule the measurement forced: a tight window is better
        // ONLY once the offset has been agreed and removed. Without
        // agreement the window has to be able to hold an offset
        // nobody has seen.
        let config = Anchoring::default();
        assert!(
            config.known_shift_tolerance_s < config.tolerance_s,
            "the known-offset window is the tighter one"
        );
        let transcript = stamped("[00:10.00]ab\n[00:20.00]cd\n[00:30.00]ef");
        let frames = (60.0 / FRAME_S) as usize;
        let frame = |seconds: f64| (seconds / FRAME_S) as usize;
        let tight = Anchoring {
            tolerance_s: config.known_shift_tolerance_s,
            ..config
        };
        // The choice itself: agreement buys the tight window, and
        // nothing else does.
        assert!(
            (tolerance_for(Some(2.0), &config) - config.known_shift_tolerance_s).abs() < 1e-9,
            "an agreed offset closes the window in"
        );
        assert!(
            (tolerance_for(Some(0.0), &config) - config.known_shift_tolerance_s).abs() < 1e-9,
            "an agreed offset of zero is still an agreement"
        );
        assert!(
            (tolerance_for(None, &config) - config.tolerance_s).abs() < 1e-9,
            "without agreement the window must hold an offset nobody saw"
        );
        let known = token_windows(&transcript, 0.0, &tight, frames).expect("windows");
        let unknown = token_windows(&transcript, 0.0, &config, frames).expect("windows");
        assert_eq!(known[0].0, frame(9.0), "±1 s around the first stamp");
        assert_eq!(unknown[0].0, frame(6.0), "±4 s when the offset is unknown");
        assert!(
            known[0].1 < unknown[0].1,
            "and the tight window ends sooner too"
        );
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
        assert!((shift_from(&agreeing, &transcript).expect("agreed") - 2.0).abs() < 1e-9);
        // A derailed pass agrees on nothing and must not invent a
        // shift. It must also not be MISTAKEN for an agreed shift of
        // zero: the window width hangs on the difference, and a tight
        // window around stamps that are secretly off is worse than no
        // window at all (measured: PCO@0.1 38.4 % against 45.4 %).
        let derailed: Vec<AlignedLine> = [1.0, 90.0, 15.0, 200.0].into_iter().map(line).collect();
        assert_eq!(shift_from(&derailed, &transcript), None);
    }

    #[test]
    fn a_pass_pushed_to_its_windows_edge_is_saturated_and_one_placed_inside_is_not() {
        let transcript = stamped(
            "[00:10.00]ab
[00:20.00]cd
[00:30.00]ef
[00:40.00]gh",
        );
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
        let at = |delta: f64| -> Vec<AlignedLine> {
            [10.0, 20.0, 30.0, 40.0]
                .into_iter()
                .map(|s| line(s + delta))
                .collect()
        };
        // The known case: every line −3.65 s from its stamp in a
        // ±4 s window is the window's edge, not the song.
        assert!(window_saturated(&at(-3.65), &transcript, 0.0, 4.0));
        // Three seconds off in the same window — the source's master
        // condition the anchor width was tuned on — is placed, not
        // pushed.
        assert!(!window_saturated(&at(-3.0), &transcript, 0.0, 4.0));
        assert!(!window_saturated(&at(0.4), &transcript, 0.0, 4.0));
        // The distance is measured from the SHIFTED stamps: lines at
        // stamp + 6 in a window centred on an applied shift of 6 are
        // inside it.
        assert!(!window_saturated(&at(6.0), &transcript, 6.0, 1.0));
        assert!(window_saturated(&at(6.9), &transcript, 6.0, 1.0));
        // One outlier does not saturate a placed pass — the median
        // votes — and fewer than two votes say nothing.
        let mut placed = at(0.2);
        placed[3].start = 43.9;
        assert!(!window_saturated(&placed, &transcript, 0.0, 4.0));
        assert!(!window_saturated(&at(-3.9)[..1], &transcript, 0.0, 4.0));
        // The wider retry can hold what the first window could not.
        const {
            assert!(WIDER_BY * 4.0 > 10.0);
        }
    }

    #[test]
    fn a_pass_whose_confidence_collapses_against_the_plain_one_was_pushed() {
        let word = |conf: f32, estimated: bool| AlignedWord {
            text: "x".to_owned(),
            start: 0.0,
            end: 0.5,
            conf,
            estimated,
            chars: if estimated { vec![] } else { vec![[0.0, 0.5]] },
        };
        let line = |words: Vec<AlignedWord>| AlignedLine {
            start: 0.0,
            end: 0.5,
            text: "x".to_owned(),
            words,
        };
        // The geometric mean: 0.25 and 1.0 give 0.5, an estimated
        // word does not vote, and no words is no confidence.
        let lines = vec![
            line(vec![word(0.25, false), word(0.9, true)]),
            line(vec![word(1.0, false)]),
        ];
        assert!((pass_confidence(&lines) - 0.5).abs() < 1e-9);
        assert_eq!(pass_confidence(&[]), 0.0);
        assert_eq!(pass_confidence(&[line(vec![word(0.9, true)])]), 0.0);
        // The measured case: the anchored pass at 0.005 against the
        // plain pass at 0.032 was pushed; a pass that kept most of it
        // was placed; and a plain pass with nothing to lose pushes
        // nothing.
        assert!(pushed_against_evidence(0.0053, 0.0315));
        assert!(!pushed_against_evidence(0.03, 0.0315));
        assert!(!pushed_against_evidence(0.0, 0.0));
    }

    /// A stamped transcript of `n` lines, ten seconds apart from 20 s.
    fn stamped_lines(n: usize) -> Transcript {
        let text: Vec<String> = (0..n)
            .map(|i| {
                let t = 20.0 + 10.0 * i as f64;
                format!("[{:02}:{:05.2}]ab cd", (t / 60.0) as u32, t % 60.0)
            })
            .collect();
        Transcript::parse(&text.join("\n"))
    }

    /// A placed line at `start` with confidence `conf`.
    fn placed(start: f64, conf: f32) -> AlignedLine {
        let word = |text: &str, s: f64| AlignedWord {
            text: text.to_owned(),
            start: s,
            end: s + 0.3,
            conf,
            estimated: false,
            chars: vec![[s, s + 0.3]],
        };
        AlignedLine {
            start,
            end: start + 0.8,
            text: "ab cd".to_owned(),
            words: vec![word("ab", start), word("cd", start + 0.5)],
        }
    }

    #[test]
    fn a_warp_is_fitted_through_the_outliers_and_tells_the_unsung_lines_apart() {
        // Thirty lines stamped for another edit. This recording
        // sings the first twenty at 1.04 · t − 10.8 (with up to
        // ±0.3 s of wobble), the plain pass lost four of them by
        // tens of seconds, and the last ten lie beyond the sound.
        let transcript = stamped_lines(30);
        let truth = Warp {
            scale: 1.04,
            offset_s: -10.8,
        };
        let mut lines: Vec<AlignedLine> = (0..30)
            .map(|i| {
                let stamp = 20.0 + 10.0 * i as f64;
                let wobble = 0.3 * ((i % 3) as f64 - 1.0);
                match i {
                    5 | 11 | 14 | 19 => placed(truth.apply(stamp) - 40.0, 0.2),
                    0..=19 => placed(truth.apply(stamp) + wobble, 0.2),
                    // Beyond the sound the pass put junk, quietly.
                    _ => placed(199.0 + i as f64 * 0.1, 0.01),
                }
            })
            .collect();
        let sounding_end = truth.apply(20.0 + 10.0 * 19.0) + 3.0;
        let (warp, residual) = fit_warp(&lines, &transcript, sounding_end).expect("a warp");
        assert!((warp.scale - 1.04).abs() < 0.01, "scale {}", warp.scale);
        assert!(
            residual <= WARP_TIGHT_RESIDUAL_S,
            "a wobble of ±0.3 s is a tight fit: {residual}"
        );
        assert!(
            (warp.offset_s + 10.8).abs() < 0.8,
            "offset {}",
            warp.offset_s
        );
        let (retimed, unsung) = warp.retime(&transcript, sounding_end);
        assert_eq!(unsung, (20..30).collect::<Vec<_>>());
        assert!(retimed.lines[3].source_start_s.is_some());
        assert!(retimed.lines[25].source_start_s.is_none());
        // Stamps that merely wobble around the truth are placed, not
        // warped away: the fit stays near identity.
        let on_time: Vec<AlignedLine> = (0..30)
            .map(|i| placed(20.0 + 10.0 * i as f64 + 0.2, 0.3))
            .collect();
        let (near, _) = fit_warp(&on_time, &transcript, 400.0).expect("fits");
        assert!((near.scale - 1.0).abs() < 1e-6 && (near.offset_s - 0.2).abs() < 1e-6);
        // A coarse map — a bar's jump after the third line and a slow
        // drift after it (France Gall) — is accepted with its
        // residual, which the caller turns into a wider window.
        let jumpy: Vec<AlignedLine> = (0..30)
            .map(|i| {
                let stamp = 20.0 + 10.0 * i as f64;
                // Five seconds more every ten lines, and a wobble.
                let jumps = 5.0 * f64::from(u8::try_from(i / 10).unwrap_or(0));
                let wobble = 0.5 * ((i % 3) as f64 - 1.0);
                placed(stamp + jumps + wobble + 0.01 * stamp, 0.3)
            })
            .collect();
        let (coarse, residual) = fit_warp(&jumpy, &transcript, 400.0).expect("a coarse fit");
        assert!(
            residual > WARP_TIGHT_RESIDUAL_S && residual <= WARP_MAX_RESIDUAL_S,
            "{residual}"
        );
        assert!((coarse.scale - 1.05).abs() < 0.02, "{}", coarse.scale);
        // A derailed pass fits nothing.
        for (i, line) in lines.iter_mut().enumerate() {
            line.start = (i as f64 * 37.0) % 250.0;
            for word in &mut line.words {
                word.start = line.start;
                word.end = line.start + 0.3;
            }
        }
        assert_eq!(fit_warp(&lines, &transcript, 400.0), None);
        // Too few lines fit nothing either.
        let few: Vec<AlignedLine> = (0..5)
            .map(|i| placed(20.0 + 10.0 * i as f64, 0.3))
            .collect();
        assert_eq!(fit_warp(&few, &stamped_lines(5), 400.0), None);
        // A tempo ratio outside the same performance is refused.
        let doubled: Vec<AlignedLine> = (0..30)
            .map(|i| placed(2.0 * (20.0 + 10.0 * i as f64), 0.3))
            .collect();
        assert_eq!(fit_warp(&doubled, &transcript, 1000.0), None);
    }

    #[test]
    fn under_a_constant_shift_the_lines_past_the_sound_lose_their_stamp() {
        // Ten lines at 20, 30 … 110 s, the song sitting 5 s late
        // against them, and the sound ending at 90 s: the stamps at
        // 90 s and beyond land past 95 − margin.
        let transcript = stamped_lines(10);
        let (trimmed, unsung) = trim_unsung(&transcript, 5.0, 90.0);
        assert_eq!(unsung, vec![7, 8, 9]);
        assert!(trimmed.lines[6].source_start_s.is_some());
        assert!(trimmed.lines[7].source_start_s.is_none());
        // The kept stamps are left AS THEY WERE — the gate still sees
        // the shift and calls it a shifted master.
        assert_eq!(
            trimmed.lines[0].source_start_s,
            transcript.lines[0].source_start_s
        );
        // A shift the other way puts early lines before the start.
        let (_, early) = trim_unsung(&transcript, -25.0, 500.0);
        assert_eq!(early, vec![0]);
        // No line past the sound: nothing to trim.
        assert!(trim_unsung(&transcript, 0.0, 500.0).1.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::tokens_of;

    #[test]
    fn a_stem_the_length_of_the_song_is_accepted_and_a_primed_one_is_not() {
        let song = AudioData::from_mono(vec![0.0; 44_100 * 10], 44_100);
        // The same length, and a tail padding of a few hundred samples
        // a decoder may keep or drop: this song, this timeline.
        assert!(check_heard_matches(&song, &song).is_ok());
        let padded = AudioData::from_mono(vec![0.0; 44_100 * 10 + 300], 44_100);
        assert!(check_heard_matches(&song, &padded).is_ok());
        // Longer by the smaller AAC priming (1024 samples = 23 ms):
        // decoded from the container by a tool that did not skip it,
        // so every word would sit 23 ms early. Refused by construction.
        let primed = AudioData::from_mono(vec![0.0; 44_100 * 10 + 1024], 44_100);
        let err = check_heard_matches(&song, &primed).expect_err("primed stem");
        assert!(matches!(err, LyricsError::HeardMismatch { .. }), "{err}");
        // A different song altogether.
        let other = AudioData::from_mono(vec![0.0; 44_100 * 12], 44_100);
        assert!(check_heard_matches(&song, &other).is_err());
        // The tolerance itself sits under that priming.
        const {
            assert!(HEARD_LENGTH_TOLERANCE_S < 1024.0 / 44_100.0);
        }
    }

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
