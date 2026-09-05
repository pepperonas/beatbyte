//! Audio → the acoustic model's emissions, in windows, stitched.
//!
//! `wav2vec2-base-960h` wants 16 kHz mono, normalised to zero mean
//! and unit variance, and returns one 32-way log-probability row per
//! 20 ms. A whole song at once would be a 12 000-frame attention —
//! memory and time both quadratic — so the audio goes through in
//! **60-second windows with a 50-second hop**, and only the centre
//! 50 s of each window's frames are kept (the first window keeps its
//! start, the last its end): every frame comes from a window in which
//! it had 5 s of context on either side, and the seams fall on frame
//! boundaries so the stitched matrix is one continuous timeline. The
//! Viterbi then runs ONCE over the whole thing, so no word can drift
//! at a window edge.

use beatbyte_ml::{Input, Loaded, MlError, ModelSpec, Runtime};

use crate::ctc::Emissions;
use crate::transcript::VOCAB;

/// The acoustic model, as the registry knows it (`beatbyte-ml`).
pub const MODEL: ModelSpec = beatbyte_ml::WAV2VEC2_BASE_960H;

/// The sample rate the model expects.
pub const SAMPLE_RATE: u32 = 16_000;
/// Seconds per emission frame (the model's stride, 320 samples).
pub const FRAME_S: f64 = 0.02;
/// Samples per frame.
pub const FRAME_SAMPLES: usize = 320;
/// Window length in seconds.
pub const WINDOW_S: f64 = 60.0;
/// Window hop in seconds.
pub const HOP_S: f64 = 50.0;

/// One window of the plan: which samples go in, which of the
/// resulting frames are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First sample.
    pub start: usize,
    /// One past the last sample.
    pub end: usize,
    /// First frame (window-local) that is kept.
    pub keep_from: usize,
    /// One past the last kept frame (window-local); `usize::MAX` =
    /// to the end of whatever the model returns.
    pub keep_to: usize,
}

/// The windows for `total` samples at [`SAMPLE_RATE`]. Pure — tested:
/// every frame of the song is kept exactly once, in order.
#[must_use]
pub fn window_plan(total: usize) -> Vec<Window> {
    let window = (WINDOW_S * f64::from(SAMPLE_RATE)) as usize;
    let hop = (HOP_S * f64::from(SAMPLE_RATE)) as usize;
    let margin_frames = (window - hop) / 2 / FRAME_SAMPLES; // 5 s = 250 frames
    let hop_frames = hop / FRAME_SAMPLES; // 2500
    if total == 0 {
        return Vec::new();
    }
    let mut plan = Vec::new();
    let mut start = 0usize;
    loop {
        let end = (start + window).min(total);
        let first = start == 0;
        let last = end == total;
        plan.push(Window {
            start,
            end,
            keep_from: if first { 0 } else { margin_frames },
            keep_to: if last {
                usize::MAX
            } else {
                margin_frames + hop_frames
            },
        });
        if last {
            break;
        }
        start += hop;
    }
    plan
}

/// The model's frame count for `samples` input samples: a 400-sample
/// receptive field advancing by 320.
#[must_use]
pub fn frames_for(samples: usize) -> usize {
    if samples < 400 {
        0
    } else {
        (samples - 400) / FRAME_SAMPLES + 1
    }
}

/// Run the whole song (16 kHz mono) through the model and stitch the
/// windows into one emission matrix of log-probabilities.
pub fn compute(runtime: &Runtime, model: &Loaded, samples: &[f32]) -> Result<Emissions, MlError> {
    let vocab = VOCAB.len();
    let mut log_probs: Vec<f32> = Vec::new();
    for window in window_plan(samples.len()) {
        let slice = &samples[window.start..window.end];
        if frames_for(slice.len()) == 0 {
            continue;
        }
        let normalised = normalise(slice);
        let outputs = runtime.run(
            model,
            &[Input {
                name: "input_values",
                shape: vec![1, normalised.len()],
                data: normalised,
            }],
        )?;
        let Some(logits) = outputs.into_iter().next() else {
            return Err(MlError::Run {
                id: model.id.to_owned(),
                reason: "the model returned no output".to_owned(),
            });
        };
        if logits.shape.len() != 3 || logits.shape[2] != vocab {
            return Err(MlError::Run {
                id: model.id.to_owned(),
                reason: format!("unexpected logits shape {:?}", logits.shape),
            });
        }
        let frames = logits.shape[1];
        // The seams between windows assume the model's stride; a
        // model with another one would stitch a corrupt timeline.
        if frames != frames_for(slice.len()) {
            return Err(MlError::Run {
                id: model.id.to_owned(),
                reason: format!(
                    "{frames} frames for {} samples; expected {} (stride {FRAME_SAMPLES})",
                    slice.len(),
                    frames_for(slice.len())
                ),
            });
        }
        let keep_to = window.keep_to.min(frames);
        for f in window.keep_from.min(frames)..keep_to {
            let row = &logits.data[f * vocab..(f + 1) * vocab];
            log_probs.extend(log_softmax(row));
        }
    }
    let frames = log_probs.len() / vocab;
    Ok(Emissions {
        frames,
        vocab,
        log_probs,
    })
}

/// Zero mean, unit variance — what the model's feature extractor
/// does (`do_normalize: true`).
fn normalise(samples: &[f32]) -> Vec<f32> {
    let n = samples.len() as f64;
    let mean = samples.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let var = samples
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n;
    let scale = 1.0 / (var.sqrt() + 1e-7);
    samples
        .iter()
        .map(|&v| ((f64::from(v) - mean) * scale) as f32)
        .collect()
}

/// Log-softmax of one row, numerically safe.
fn log_softmax(row: &[f32]) -> Vec<f32> {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let log_sum = row.iter().map(|&v| (v - max).exp()).sum::<f32>().ln();
    row.iter().map(|&v| v - max - log_sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_keeps_every_frame_exactly_once_in_order() {
        for seconds in [3.0, 59.0, 60.0, 61.0, 125.0, 248.2, 600.0] {
            let total = (seconds * f64::from(SAMPLE_RATE)) as usize;
            let plan = window_plan(total);
            let mut global_frames = Vec::new();
            for w in &plan {
                let frames = frames_for(w.end - w.start);
                let window_first_frame = w.start / FRAME_SAMPLES;
                let to = w.keep_to.min(frames);
                for f in w.keep_from.min(frames)..to {
                    global_frames.push(window_first_frame + f);
                }
            }
            let expected = frames_for(total);
            assert_eq!(
                global_frames.len(),
                expected,
                "{seconds} s: {} frames kept, {expected} in the song",
                global_frames.len()
            );
            for (i, &f) in global_frames.iter().enumerate() {
                assert_eq!(f, i, "{seconds} s: frame {i} came out as {f}");
            }
        }
    }

    #[test]
    fn a_short_song_is_one_window_and_an_empty_one_none() {
        assert_eq!(window_plan(0), Vec::new());
        let plan = window_plan(16_000 * 30);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].keep_from, 0);
        assert_eq!(plan[0].keep_to, usize::MAX);
    }

    #[test]
    fn frames_follow_the_models_conv_stack() {
        // Measured on the real model: 12.0 s → 599 frames.
        assert_eq!(frames_for(192_000), 599);
        assert_eq!(frames_for(399), 0);
        assert_eq!(frames_for(400), 1);
        assert_eq!(frames_for(720), 2);
    }

    #[test]
    fn normalisation_and_log_softmax_do_what_they_say() {
        let x = normalise(&[1.0, 3.0, 5.0, 7.0]);
        let mean: f32 = x.iter().sum::<f32>() / 4.0;
        let var: f32 = x.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-6 && (var - 1.0).abs() < 1e-4);
        let lp = log_softmax(&[1.0, 2.0, 3.0]);
        let total: f32 = lp.iter().map(|v| v.exp()).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(lp[2] > lp[1] && lp[1] > lp[0]);
    }
}
