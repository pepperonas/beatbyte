//! # beatbyte-meter
//!
//! Beats and **downbeats** from a local model. The spectral analyzer
//! in `beatbyte-audio` tracks beats; it has no notion of a bar, so
//! every chart counted bars in fours from its first beat. This crate
//! runs *Beat This!* (Foscarin, Schlüter & Widmer, ISMIR 2024 — code
//! and published weights MIT) as the ONNX pair the `beat-this-rs`
//! port converted, through `beatbyte-ml`'s pinned runtime: audio in,
//! beat and downbeat times out (plan
//! `docs/plans/ai-song-graph-upgrade.md`, Track C, C1).
//!
//! - [`chunks`] — the model was trained on 30 s pieces; a song is cut
//!   into overlapping 1500-frame chunks and stitched keep-first, pure
//! - [`peaks`] — logits to times: the reference "minimal" decoder
//!   (local maximum, threshold, adjacent peaks merged, downbeats
//!   snapped onto beats), pure
//! - [`merge`](mod@merge) — how a model's answer enters a [`SongAnalysis`]: the
//!   downbeats onto the analyzer's grid, or the model's whole grid
//!
//! No model file is in the repository; the two ONNX files are
//! registry entries in `beatbyte-ml`, fetched only on the user's
//! action and verified before they are loaded. The runtime is the
//! pinned pool, so the same audio gives the same bars on a machine.
//!
//! [`SongAnalysis`]: beatbyte_core::music::SongAnalysis

pub mod chunks;
pub mod merge;
pub mod peaks;

use std::sync::atomic::{AtomicBool, Ordering};

use beatbyte_audio::analysis::structure;
use beatbyte_audio::decode::AudioData;
use beatbyte_audio::resample::resample;
use beatbyte_ml::{BEAT_THIS, BEAT_THIS_MEL, BEAT_THIS_SMALL, Input, Loaded, MlError, ModelSpec};
use beatbyte_ml::{ModelStore, Runtime, Status};

pub use merge::{Outcome, Policy, apply, merge, same_level};

/// The sample rate the mel front end expects.
pub const SAMPLE_RATE: u32 = 22_050;
/// Mel frames per second (hop 441 samples at 22 050 Hz).
pub const FPS: f64 = 50.0;
/// Mel bands per frame.
pub const MEL_BANDS: usize = 128;

/// Which beat model to run. Both share the mel front end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// `beat-this-small` — 10 MB, the paper's "smaller model".
    Small,
    /// `beat-this` — 83 MB, the paper's main model.
    Full,
}

impl Size {
    /// The registry entry of the beat model.
    #[must_use]
    pub const fn spec(self) -> &'static ModelSpec {
        match self {
            Size::Small => &BEAT_THIS_SMALL,
            Size::Full => &BEAT_THIS,
        }
    }

    /// Every size, largest first — the order [`installed`] prefers.
    pub const ALL: [Size; 2] = [Size::Full, Size::Small];
}

/// What the model heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    /// Beat times in seconds, ascending.
    pub beats: Vec<f64>,
    /// Downbeat times in seconds, ascending, each one a beat.
    pub downbeats: Vec<f64>,
    /// The beat model's registry id.
    pub model: &'static str,
    /// The beat model's registered SHA-256 — record it in whatever
    /// the meter goes into.
    pub sha256: &'static str,
}

/// Why a track could not be metered.
#[derive(Debug, thiserror::Error)]
pub enum MeterError {
    /// The runtime or the store refused.
    #[error(transparent)]
    Ml(#[from] MlError),
    /// A model answered in a shape this driver does not understand.
    #[error("`{model}` returned `{what}` with shape {shape:?}; expected {expected}")]
    Shape {
        /// The model's registry id.
        model: &'static str,
        /// The output in question.
        what: &'static str,
        /// The shape it had.
        shape: Vec<usize>,
        /// What the driver needed.
        expected: &'static str,
    },
    /// A model's output lacks a tensor the driver needs.
    #[error("`{model}` returned no `{what}` output")]
    Missing {
        /// The model's registry id.
        model: &'static str,
        /// The output in question.
        what: &'static str,
    },
}

/// The largest beat model that is installed and intact — with the
/// mel front end it needs — or `None`. Hashes files, so not per frame.
#[must_use]
pub fn installed(store: &ModelStore) -> Option<Size> {
    if store.status(&BEAT_THIS_MEL) != Status::Installed {
        return None;
    }
    Size::ALL
        .into_iter()
        .find(|size| store.status(size.spec()) == Status::Installed)
}

/// Meter a decoded song: the audio is resampled to [`SAMPLE_RATE`]
/// and both models are loaded from the store (verified first).
pub fn track(
    runtime: &Runtime,
    store: &ModelStore,
    size: Size,
    audio: &AudioData,
) -> Result<Meter, MeterError> {
    let mel = runtime.load(store, &BEAT_THIS_MEL)?;
    let beat = runtime.load(store, size.spec())?;
    let samples = resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE);
    track_samples(runtime, &mel, &beat, &samples)
}

/// Meter mono samples already at [`SAMPLE_RATE`] with loaded models.
pub fn track_samples(
    runtime: &Runtime,
    mel: &Loaded,
    beat: &Loaded,
    samples: &[f32],
) -> Result<Meter, MeterError> {
    track_samples_with(
        runtime,
        mel,
        beat,
        samples,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
}

/// [`track_samples`] with a progress report `(chunks done, chunks
/// total)` and a cancel flag checked before each chunk — a song's
/// analysis runs off the frame thread and must be able to stop.
pub fn track_samples_with(
    runtime: &Runtime,
    mel: &Loaded,
    beat: &Loaded,
    samples: &[f32],
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<Meter, MeterError> {
    let spectrogram = mel_spectrogram(runtime, mel, samples)?;
    let (beat_logits, downbeat_logits) = predict(runtime, beat, &spectrogram, progress, cancel)?;
    let (beats, downbeats) = peaks::decode(&beat_logits, &downbeat_logits);
    Ok(Meter {
        beats,
        downbeats,
        model: beat.id,
        sha256: beat.sha256,
    })
}

/// A log-mel spectrogram, `frames × MEL_BANDS` row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrogram {
    /// Frames at [`FPS`].
    pub frames: usize,
    /// `frames * MEL_BANDS` values.
    pub data: Vec<f32>,
}

/// The front end: samples at [`SAMPLE_RATE`] → mel frames.
pub fn mel_spectrogram(
    runtime: &Runtime,
    mel: &Loaded,
    samples: &[f32],
) -> Result<Spectrogram, MeterError> {
    let outputs = runtime.run(
        mel,
        &[Input {
            name: "audio_pcm",
            shape: vec![1, samples.len()],
            data: samples.to_vec(),
        }],
    )?;
    let output = outputs
        .into_iter()
        .find(|o| o.name == "mel_spectrogram")
        .ok_or(MeterError::Missing {
            model: mel.id,
            what: "mel_spectrogram",
        })?;
    if output.shape.len() != 3 || output.shape[0] != 1 || output.shape[2] != MEL_BANDS {
        return Err(MeterError::Shape {
            model: mel.id,
            what: "mel_spectrogram",
            shape: output.shape,
            expected: "[1, frames, 128]",
        });
    }
    Ok(Spectrogram {
        frames: output.shape[1],
        data: output.data,
    })
}

/// The beat model over the whole spectrogram, chunked and stitched:
/// `(beat logits, downbeat logits)`, one value per frame.
pub fn predict(
    runtime: &Runtime,
    beat: &Loaded,
    spectrogram: &Spectrogram,
    progress: &mut dyn FnMut(usize, usize),
    cancel: &AtomicBool,
) -> Result<(Vec<f32>, Vec<f32>), MeterError> {
    let starts = chunks::starts(spectrogram.frames);
    let total = starts.len();
    let mut stitched = chunks::Stitched::new(spectrogram.frames);
    progress(0, total);
    for (done, &start) in starts.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MlError::Cancelled {
                id: beat.id.to_owned(),
            }
            .into());
        }
        let chunk = chunks::cut(&spectrogram.data, spectrogram.frames, MEL_BANDS, start);
        let outputs = runtime.run(
            beat,
            &[Input {
                name: "spectrogram",
                shape: vec![1, chunk.frames, MEL_BANDS],
                data: chunk.data,
            }],
        )?;
        let take = |what: &'static str, alt: &'static str| -> Result<Vec<f32>, MeterError> {
            let output = outputs
                .iter()
                .find(|o| o.name == what || o.name == alt)
                .ok_or(MeterError::Missing {
                    model: beat.id,
                    what,
                })?;
            if output.data.len() != chunk.frames {
                return Err(MeterError::Shape {
                    model: beat.id,
                    what,
                    shape: output.shape.clone(),
                    expected: "one logit per frame of the chunk",
                });
            }
            Ok(output.data.clone())
        };
        let beats = take("beat", "beat_logits")?;
        let downbeats = take("downbeat", "downbeat_logits")?;
        stitched.write(start, &beats, &downbeats);
        progress(done + 1, total);
    }
    Ok(stitched.finish())
}

/// The policy the chart pipeline applies — the corpus's choice
/// (`docs/audio-eval-baseline.md`, "Downbeats from a model"; ADR-0015):
/// the model's whole grid, because where the tracker has locked onto
/// the wrong metrical level no downbeat placed on its beats can be
/// right, and where it has not the two grids agree within a frame.
pub const DEFAULT_POLICY: Policy = Policy::Grid;

/// What [`refine`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The beat model's registry id.
    pub model: &'static str,
    /// Its registered SHA-256.
    pub sha256: &'static str,
    /// How it was meant to enter the analysis.
    pub policy: Policy,
    /// Whether it did — [`Outcome::Adopted`] — or why not.
    pub outcome: Outcome,
    /// Beats the model heard.
    pub beats: usize,
    /// Downbeats the analysis carries now.
    pub downbeats: usize,
    /// Repeated sections the analysis carries now.
    pub repeats: usize,
}

impl Applied {
    /// One line for a log: what the model heard and what became of it.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.outcome {
            Outcome::Adopted => format!(
                "`{}` heard {} beats; {} downbeats on the grid ({:?}); {} repeated sections",
                self.model, self.beats, self.downbeats, self.policy, self.repeats
            ),
            Outcome::Disagreement {
                tracker_bpm,
                model_bpm,
            } => format!(
                "`{}` hears {model_bpm:.1} BPM where the tracker hears {tracker_bpm:.1} \
                 (ratio {:.2}) — not a reading of the same grid; the chart keeps the \
                 tracker's grid and counts bars in fours",
                self.model,
                model_bpm / tracker_bpm.max(f64::EPSILON)
            ),
            Outcome::NoTempo => format!(
                "`{}` heard {} beats, too few for a tempo; the chart keeps the tracker's grid",
                self.model, self.beats
            ),
        }
    }
}

/// Run the largest installed model over the song and fold its answer
/// into the analysis under `policy` — when the model reads the same
/// metrical level as the tracker ([`apply`]). `Ok(None)` when no
/// model pair is installed — the analysis is then exactly the
/// analyzer's; an error only when an installed model failed to run.
pub fn refine(
    analysis: &mut beatbyte_core::music::SongAnalysis,
    audio: &AudioData,
    policy: Policy,
) -> Result<Option<Applied>, MeterError> {
    let Some(store) = ModelStore::default_location() else {
        return Ok(None);
    };
    let Some(size) = installed(&store) else {
        return Ok(None);
    };
    let runtime = Runtime::new();
    let meter = track(&runtime, &store, size, audio)?;
    let outcome = apply(analysis, &meter, policy);
    if outcome == Outcome::Adopted {
        // The repeats are beat indices into a grid that just changed:
        // find them again on the model's grid, with its bars.
        let prepared = if audio.sample_rate() >= 32_000 {
            audio.clone().downsample_half()
        } else {
            audio.clone()
        };
        analysis.repeats = structure::find_repeats(
            &prepared,
            &analysis.beats,
            &analysis.downbeats,
            &structure::StructureConfig::default(),
        );
    }
    Ok(Some(Applied {
        model: meter.model,
        sha256: meter.sha256,
        policy,
        outcome,
        beats: meter.beats.len(),
        downbeats: analysis.downbeats.len(),
        repeats: analysis.repeats.len(),
    }))
}
