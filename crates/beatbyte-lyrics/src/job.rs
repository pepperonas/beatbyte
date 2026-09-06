//! One song, start to finish: lyrics file + audio file → gated
//! `words.json` beside the audio. The one path both `beatbyte-cli
//! align` and the game's "align this song" run, so a result from the
//! menu is exactly the result from the terminal.
//!
//! Everything slow happens here — loading the model, decoding, the
//! model windows — so the caller can run it off its main thread,
//! watch [`JobProgress`] and pull the cancel flag.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use beatbyte_ml::{MlError, ModelStore, Runtime};
use thiserror::Error;

use crate::align::{
    Anchoring, LyricsError, NO_SEPARATOR, Options, Progress, Provenance, Stats, align_with,
};
use crate::emissions::MODEL;
use crate::gate::{GateConfig, GateReport, gate, prefer};
use crate::transcript::Transcript;
use crate::words::Alignment;

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStage {
    /// Reading the lyrics and loading the model.
    Loading,
    /// Decoding the audio file.
    Decoding,
    /// The alignment pipeline (with its own stage inside).
    Aligning(crate::align::Stage),
    /// The confidence gate and the write.
    Finishing,
}

/// A progress report from a running job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobProgress {
    /// The stage.
    pub stage: JobStage,
    /// Model windows done (emissions stage only; 0 otherwise).
    pub done: usize,
    /// Model windows total (emissions stage only; 0 otherwise).
    pub total: usize,
}

impl JobProgress {
    /// A short line for a status row, e.g. `emissions 3/5`.
    #[must_use]
    pub fn label(&self) -> String {
        match self.stage {
            JobStage::Loading => "loading the model".to_owned(),
            JobStage::Decoding => "decoding".to_owned(),
            JobStage::Aligning(crate::align::Stage::Resampling) => "resampling".to_owned(),
            JobStage::Aligning(crate::align::Stage::Emissions) => {
                format!("listening {}/{}", self.done, self.total)
            }
            JobStage::Aligning(crate::align::Stage::Aligning) => "aligning".to_owned(),
            JobStage::Finishing => "checking".to_owned(),
        }
    }
}

/// Why a job did not produce a file.
#[derive(Debug, Error)]
pub enum JobError {
    /// The lyrics file could not be read.
    #[error("cannot read `{path}`: {reason}")]
    Lyrics {
        /// The file.
        path: PathBuf,
        /// The OS's reason.
        reason: String,
    },
    /// The lyrics hold no word the model has letters for.
    #[error("`{path}` holds no words the model has letters for")]
    NoWords {
        /// The file.
        path: PathBuf,
    },
    /// No config directory on this platform.
    #[error("no config directory on this platform; models cannot be stored")]
    NoStore,
    /// The model is not installed (download it first).
    #[error("model `{id}` is not installed")]
    NotInstalled {
        /// The registry id.
        id: String,
    },
    /// The model store or runtime failed.
    #[error(transparent)]
    Model(MlError),
    /// The audio could not be decoded.
    #[error("{0}")]
    Audio(String),
    /// The vocal stem could not be decoded.
    #[error("cannot decode the stem `{path}`: {reason}")]
    Vocals {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// The alignment failed.
    #[error(transparent)]
    Align(LyricsError),
    /// The job was cancelled; nothing was written.
    #[error("alignment cancelled")]
    Cancelled,
    /// The result could not be written.
    #[error("cannot write `{path}`: {reason}")]
    Write {
        /// The file.
        path: PathBuf,
        /// The OS's reason.
        reason: String,
    },
}

/// What a finished job produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Where the alignment was written.
    pub out: PathBuf,
    /// The aligner's stats.
    pub stats: Stats,
    /// The gate's report (`None` when the job ran raw).
    pub gate: Option<GateReport>,
    /// Wall-clock time of the alignment itself.
    pub took: Duration,
    /// How many Viterbi passes the alignment cost (see
    /// [`crate::align::AlignOutcome::passes`]).
    pub passes: u8,
    /// When the source's stamps were retimed: the map they needed, if
    /// any, and how many lines lay beyond the sound.
    pub warp: Option<(Option<crate::align::Warp>, usize)>,
    /// Where the song's sound ends, seconds — the length every rule
    /// judged against.
    pub sounding_end_s: f64,
    /// Whether the result was written. `false` only under
    /// [`JobOptions::keep_better`], when the alignment already beside
    /// the audio outranked the new one and was kept.
    pub written: bool,
}

/// What a job may do beyond aligning the song's own audio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobOptions {
    /// Where to write the alignment (default: beside the audio).
    pub out: Option<PathBuf>,
    /// Run the confidence gate (what the game wants); `false` writes
    /// the raw alignment.
    pub gated: bool,
    /// Listen to this file instead of the song — a vocal stem an
    /// external separator wrote from the song, on the song's own
    /// timeline. The song still supplies the hash, the length the
    /// stem is checked against, and the length the gate judges by.
    pub vocals: Option<PathBuf>,
    /// With `vocals`: what produced the stem, recorded as provenance.
    pub separator: String,
    /// Write only when the new alignment outranks the one already at
    /// `out` by [`prefer`]; otherwise keep that file and report
    /// `written: false`. A file that is not a gated alignment (raw,
    /// unreadable) never outranks anything.
    pub keep_better: bool,
    /// Run the plain forced alignment only, without confining the
    /// words to the source's line stamps — for looking at what the
    /// model heard where, when the stamps themselves are in question.
    pub no_anchors: bool,
}

/// Whether the alignment already at `existing` outranks `new` — the
/// keep-better decision, from the files alone. Pure over its inputs;
/// tested through [`prefer`] and the job.
#[must_use]
pub fn existing_outranks(existing: Option<&Alignment>, new: &GateReport) -> bool {
    existing
        .and_then(|alignment| alignment.gate.as_ref())
        .is_some_and(|old| !prefer(new, old))
}

/// Where a song's alignment goes: `<audio stem>.words.json` beside
/// the audio.
#[must_use]
pub fn default_output(audio: &Path) -> PathBuf {
    let stem = audio
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "song".to_owned());
    audio.with_file_name(format!("{stem}.words.json"))
}

/// Align `lyrics_path` against `audio_path` and write the result to
/// `out` (default: beside the audio). `gated` runs the confidence
/// gate (what the game wants); `false` writes the raw alignment (the
/// evaluation harness, and looking at what the aligner produced).
pub fn align_file(
    audio_path: &Path,
    lyrics_path: &Path,
    out: Option<PathBuf>,
    gated: bool,
    progress: &mut dyn FnMut(JobProgress),
    cancel: &AtomicBool,
) -> Result<Summary, JobError> {
    align_file_with(
        audio_path,
        lyrics_path,
        &JobOptions {
            out,
            gated,
            ..JobOptions::default()
        },
        progress,
        cancel,
    )
}

/// [`align_file`] with every option: a vocal stem to listen to, its
/// provenance, and the keep-better rule.
pub fn align_file_with(
    audio_path: &Path,
    lyrics_path: &Path,
    options: &JobOptions,
    progress: &mut dyn FnMut(JobProgress),
    cancel: &AtomicBool,
) -> Result<Summary, JobError> {
    let report = |stage| JobProgress {
        stage,
        done: 0,
        total: 0,
    };
    progress(report(JobStage::Loading));
    let lyrics = std::fs::read_to_string(lyrics_path).map_err(|error| JobError::Lyrics {
        path: lyrics_path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let transcript = Transcript::parse(&lyrics);
    if transcript.alignable_words() == 0 {
        return Err(JobError::NoWords {
            path: lyrics_path.to_path_buf(),
        });
    }
    let store = ModelStore::default_location().ok_or(JobError::NoStore)?;
    let runtime = Runtime::new();
    let model = match runtime.load(&store, &MODEL) {
        Ok(model) => model,
        Err(MlError::NotInstalled { id }) => return Err(JobError::NotInstalled { id }),
        Err(error) => return Err(JobError::Model(error)),
    };
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(JobError::Cancelled);
    }
    progress(report(JobStage::Decoding));
    let audio = beatbyte_audio::decode_file(audio_path)
        .map_err(|error| JobError::Audio(error.to_string()))?;
    let heard = match &options.vocals {
        Some(path) => {
            Some(
                beatbyte_audio::decode_file(path).map_err(|error| JobError::Vocals {
                    path: path.clone(),
                    reason: error.to_string(),
                })?,
            )
        }
        None => None,
    };
    let audio_sha256 = beatbyte_ml::hash::sha256_file(audio_path).map_err(|error| {
        JobError::Audio(format!("cannot hash `{}`: {error}", audio_path.display()))
    })?;
    let text_source = format!(
        "file:{}",
        lyrics_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let separator = if heard.is_some() {
        options.separator.as_str()
    } else {
        NO_SEPARATOR
    };
    let started = std::time::Instant::now();
    let mut outcome = align_with(
        &audio,
        heard.as_ref(),
        &audio_sha256,
        &transcript,
        &Provenance {
            text: &text_source,
            separator,
        },
        &runtime,
        &model,
        &Options {
            // The game's lyrics almost always carry line stamps, and
            // the measurement says that is what keeps an alignment
            // from sliding through an instrumental.
            anchoring: (!options.no_anchors).then(Anchoring::default),
        },
        &mut |p: Progress| {
            progress(JobProgress {
                stage: JobStage::Aligning(p.stage),
                done: p.done,
                total: p.total,
            });
        },
        cancel,
    )
    .map_err(|error| {
        if error.is_cancelled() {
            JobError::Cancelled
        } else {
            JobError::Align(error)
        }
    })?;
    let took = started.elapsed();
    progress(report(JobStage::Finishing));
    let sounding_end_s = audio.sounding_end_s(crate::align::SOUNDING_FLOOR);
    let warp_summary = outcome.warp.as_ref().map(|w| (w.warp, w.unsung.len()));
    let gate_report = options.gated.then(|| {
        gate(
            &mut outcome.alignment,
            &transcript,
            // The sound's end, not the container's: a stamp in a
            // tail of digital silence is past the song.
            sounding_end_s,
            Some(outcome.evidence),
            outcome.warp.as_ref(),
            &GateConfig::default(),
        )
    });
    let out = options
        .out
        .clone()
        .unwrap_or_else(|| default_output(audio_path));
    if options.keep_better
        && let Some(new) = &gate_report
    {
        let existing = std::fs::read_to_string(&out)
            .ok()
            .and_then(|json| Alignment::from_json(&json).ok());
        if existing_outranks(existing.as_ref(), new) {
            return Ok(Summary {
                out,
                stats: outcome.stats,
                gate: gate_report,
                took,
                passes: outcome.passes,
                warp: warp_summary,
                sounding_end_s,
                written: false,
            });
        }
    }
    let json = outcome
        .alignment
        .to_json()
        .map_err(|error| JobError::Write {
            path: out.clone(),
            reason: error.to_string(),
        })?;
    // Written whole and renamed: a reader never sees half a file.
    let part = out.with_extension("json.part");
    std::fs::write(&part, json)
        .and_then(|()| std::fs::rename(&part, &out))
        .map_err(|error| JobError::Write {
            path: out.clone(),
            reason: error.to_string(),
        })?;
    Ok(Summary {
        out,
        stats: outcome.stats,
        gate: gate_report,
        took,
        passes: outcome.passes,
        warp: warp_summary,
        sounding_end_s,
        written: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_lands_beside_the_audio_with_the_audio_stem() {
        assert_eq!(
            default_output(Path::new("/songs/x/Artist - Title.m4a")),
            PathBuf::from("/songs/x/Artist - Title.words.json")
        );
        // A `.tar.gz`-style name keeps everything before the LAST dot.
        assert_eq!(
            default_output(Path::new("a.b.ogg")),
            PathBuf::from("a.b.words.json")
        );
    }

    #[test]
    fn progress_labels_say_where_the_job_is() {
        let p = |stage, done, total| JobProgress { stage, done, total }.label();
        assert_eq!(p(JobStage::Loading, 0, 0), "loading the model");
        assert_eq!(
            p(JobStage::Aligning(crate::align::Stage::Emissions), 3, 5),
            "listening 3/5"
        );
        assert_eq!(p(JobStage::Finishing, 0, 0), "checking");
    }

    #[test]
    fn keep_better_keeps_a_file_only_when_it_is_a_gated_alignment_that_outranks() {
        use crate::gate::Verdict;
        let gated = |verdict, letters_per_s| GateReport {
            verdict,
            lines_compared: 0,
            consensus: None,
            median_delta_s: None,
            mad_s: None,
            words_estimated: 0,
            lines_fallen_back: 0,
            lines_unsung: 0,
            letters_per_s,
        };
        let file = |gate: Option<GateReport>| Alignment {
            schema: crate::words::SCHEMA.to_owned(),
            audio_sha256: String::new(),
            pipeline_version: crate::PIPELINE_VERSION,
            language: "en".to_owned(),
            source: crate::words::Source {
                text: String::new(),
                separator: NO_SEPARATOR.to_owned(),
                aligner: String::new(),
            },
            offset_ms: 0,
            gate,
            lines: vec![],
        };
        let new = gated(Verdict::SameMaster, Some(1.5));
        // No file, or a raw file: nothing outranks the new result.
        assert!(!existing_outranks(None, &new));
        assert!(!existing_outranks(Some(&file(None)), &new));
        // A failed alignment beside the audio does not outrank.
        assert!(!existing_outranks(
            Some(&file(Some(gated(Verdict::Failed, Some(9.0))))),
            &new
        ));
        // One of the same standing that heard more of the song does.
        assert!(existing_outranks(
            Some(&file(Some(gated(Verdict::SameMaster, Some(2.0))))),
            &new
        ));
    }

    #[test]
    fn a_lyrics_file_without_words_and_a_missing_one_are_named_errors() {
        let dir = std::env::temp_dir().join(format!("bb-lyrics-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let audio = dir.join("song.wav");
        let missing = dir.join("nope.lrc");
        let cancel = AtomicBool::new(false);
        let err = align_file(&audio, &missing, None, true, &mut |_| {}, &cancel)
            .expect_err("missing lyrics");
        assert!(matches!(err, JobError::Lyrics { .. }), "{err}");
        let numbers = dir.join("numbers.lrc");
        std::fs::write(&numbers, "[00:01.00]1999 42\n").expect("writes");
        let err =
            align_file(&audio, &numbers, None, true, &mut |_| {}, &cancel).expect_err("no words");
        assert!(matches!(err, JobError::NoWords { .. }), "{err}");
        // Nothing was written for either.
        assert!(!default_output(&audio).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
