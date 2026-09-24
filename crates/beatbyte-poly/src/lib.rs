//! # beatbyte-poly
//!
//! Which notes were **struck together** — from a local model. The
//! analyzer in `beatbyte-audio` and the monophonic pitch tracker hear
//! one line at a time, so every chord BeatByte ever charted was a
//! guess from loudness. This crate runs Spotify's *Basic Pitch*
//! (Bittner et al., ICASSP 2022 — code and weights Apache-2.0) as the
//! ONNX file its own repository publishes, through `beatbyte-ml`'s
//! pinned runtime: audio in, polyphonic notes out.
//!
//! - [`frames`] — the model's two-second windows, cut and stitched the
//!   way the reference does it, drift correction included, pure
//! - [`notes`] — its note and onset answers to notes, the reference's
//!   polyphonic tracker without the step that invents unstruck notes,
//!   pure
//!
//! Basic Pitch hears best one instrument at a time, so what is fed to
//! it matters more than anything here: the guitar-ish `other` stem of
//! a separation, not the mix, where bass, keys and voice would all
//! read as chord tones.
//!
//! No model file is in the repository. The ONNX file is a registry
//! entry in `beatbyte-ml`, fetched only on the user's action and
//! verified before it is loaded.

pub mod frames;
pub mod notes;

use std::sync::atomic::{AtomicBool, Ordering};

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::resample::resample;
use beatbyte_ml::{BASIC_PITCH, Input, Loaded, MlError, ModelStore, Runtime};

pub use frames::{FPS, PITCHES, SAMPLE_RATE};
pub use notes::{PolyNote, Posteriors};

/// The graph's input.
pub const INPUT: &str = "serving_default_input_2:0";
/// The graph's output holding the sounding likelihoods.
pub const NOTE_OUTPUT: &str = "StatefulPartitionedCall:1";
/// The graph's output holding the onset likelihoods.
pub const ONSET_OUTPUT: &str = "StatefulPartitionedCall:2";

/// What the model heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcription {
    /// The notes, by start time, then pitch.
    pub notes: Vec<PolyNote>,
    /// The model's registry id.
    pub model: &'static str,
    /// Its registered SHA-256 — record it in whatever the notes go
    /// into.
    pub sha256: &'static str,
}

/// Why a track could not be transcribed.
#[derive(Debug, thiserror::Error)]
pub enum PolyError {
    /// The runtime or the store refused.
    #[error(transparent)]
    Ml(#[from] MlError),
    /// The model answered in a shape this driver does not understand.
    #[error("`{model}` returned `{what}` with shape {shape:?}; expected [1, 172, 88]")]
    Shape {
        /// The model's registry id.
        model: &'static str,
        /// The output in question.
        what: &'static str,
        /// The shape it had.
        shape: Vec<usize>,
    },
    /// The model's output lacks a tensor the driver needs.
    #[error("`{model}` returned no `{what}` output")]
    Missing {
        /// The model's registry id.
        model: &'static str,
        /// The output in question.
        what: &'static str,
    },
}

/// Whether the model is installed and intact. Hashes the file, so not
/// per frame.
#[must_use]
pub fn installed(store: &ModelStore) -> bool {
    store.status(&BASIC_PITCH) == beatbyte_ml::Status::Installed
}

/// Transcribe a decoded track: resampled to [`SAMPLE_RATE`] mono and
/// the model loaded from the store (verified first).
///
/// # Errors
/// When the model is missing, broken, or answers in a shape this
/// driver does not know.
pub fn transcribe(
    runtime: &Runtime,
    store: &ModelStore,
    audio: &AudioData,
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<Transcription, PolyError> {
    let model = runtime.load(store, &BASIC_PITCH)?;
    let samples = resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE);
    transcribe_samples(runtime, &model, &samples, progress, cancel)
}

/// Transcribe mono samples already at [`SAMPLE_RATE`] with a loaded
/// model, reporting `(windows done, windows total)` and checking the
/// cancel flag before each window.
///
/// # Errors
/// As [`transcribe`].
pub fn transcribe_samples(
    runtime: &Runtime,
    model: &Loaded,
    samples: &[f32],
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<Transcription, PolyError> {
    let posteriors = posteriors(runtime, model, samples, progress, cancel)?;
    Ok(Transcription {
        notes: notes::notes(&posteriors),
        model: model.id,
        sha256: model.sha256,
    })
}

/// The model over the whole track, window by window, stitched.
///
/// # Errors
/// As [`transcribe`].
pub fn posteriors(
    runtime: &Runtime,
    model: &Loaded,
    samples: &[f32],
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<Posteriors, PolyError> {
    let windows = frames::windows(samples);
    let total = windows.len();
    let mut note_answers = Vec::with_capacity(total);
    let mut onset_answers = Vec::with_capacity(total);
    progress(0, total);
    for (done, window) in windows.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MlError::Cancelled {
                id: model.id.to_owned(),
            }
            .into());
        }
        let outputs = runtime.run(
            model,
            &[Input {
                name: INPUT,
                shape: vec![1, frames::WINDOW_SAMPLES, 1],
                data: window,
            }],
        )?;
        let take = |name: &'static str, what: &'static str| -> Result<Vec<f32>, PolyError> {
            let output = outputs
                .iter()
                .find(|o| o.name == name)
                .ok_or(PolyError::Missing {
                    model: model.id,
                    what,
                })?;
            if output.data.len() != frames::WINDOW_FRAMES * PITCHES {
                return Err(PolyError::Shape {
                    model: model.id,
                    what,
                    shape: output.shape.clone(),
                });
            }
            Ok(output.data.clone())
        };
        note_answers.push(take(NOTE_OUTPUT, "note")?);
        onset_answers.push(take(ONSET_OUTPUT, "onset")?);
        progress(done + 1, total);
    }
    let stitch = |answers: &[Vec<f32>], what: &'static str| {
        frames::stitch(answers, PITCHES, samples.len()).map_err(|_| PolyError::Shape {
            model: model.id,
            what,
            shape: Vec::new(),
        })
    };
    let note = stitch(&note_answers, "note")?;
    let onset = stitch(&onset_answers, "onset")?;
    Ok(Posteriors {
        frames: note.len() / PITCHES,
        note,
        onset,
    })
}
