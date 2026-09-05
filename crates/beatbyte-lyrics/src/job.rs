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

use crate::align::{Anchoring, LyricsError, Options, Progress, Stats, align_with};
use crate::emissions::MODEL;
use crate::gate::{GateConfig, GateReport, gate};
use crate::transcript::Transcript;

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
    let started = std::time::Instant::now();
    let mut outcome = align_with(
        &audio,
        &audio_sha256,
        &transcript,
        &text_source,
        &runtime,
        &model,
        &Options {
            // The game's lyrics almost always carry line stamps, and
            // the measurement says that is what keeps an alignment
            // from sliding through an instrumental.
            anchoring: Some(Anchoring::default()),
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
    let gate_report = gated.then(|| {
        gate(
            &mut outcome.alignment,
            &transcript,
            audio.duration_s(),
            &GateConfig::default(),
        )
    });
    let out = out.unwrap_or_else(|| default_output(audio_path));
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
