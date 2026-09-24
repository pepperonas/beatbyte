//! Cutting a song into the model's windows and stitching its answers
//! back into one timeline — Basic Pitch's own framing, written out.
//!
//! The model takes two seconds at a time (`AUDIO_N_SAMPLES` of
//! 22 050 Hz audio) and answers 172 frames of 256 samples each. The
//! windows overlap by 30 frames; half of that overlap is trimmed from
//! each side of every answer, so the stitched timeline uses only each
//! window's middle, where the model saw context on both sides. A
//! quarter window of silence is put in front of the song so its first
//! real frame is a middle frame too.
//!
//! ⚠️ The trimmed middle (142 frames, 36 352 samples) is a little
//! LONGER than the hop between windows (36 164 samples), so the
//! stitched frames drift ahead of the audio by a fixed amount per
//! window. [`frame_time`] takes that drift back out, exactly as the
//! reference implementation does — without it a note in the fourth
//! minute would be placed a tenth of a second late.
//!
//! Pure — tested.

/// The model's sample rate.
pub const SAMPLE_RATE: u32 = 22_050;
/// Samples per frame.
pub const FFT_HOP: usize = 256;
/// Frames per second, as the reference counts them (integer division).
pub const FPS: usize = SAMPLE_RATE as usize / FFT_HOP;
/// Frames the model answers per window: two seconds of them.
pub const WINDOW_FRAMES: usize = FPS * 2;
/// Samples per window: two seconds, less one hop.
pub const WINDOW_SAMPLES: usize = SAMPLE_RATE as usize * 2 - FFT_HOP;
/// Frames the windows overlap by.
pub const OVERLAP_FRAMES: usize = 30;
/// Samples the windows overlap by.
pub const OVERLAP_SAMPLES: usize = OVERLAP_FRAMES * FFT_HOP;
/// Samples from one window's start to the next.
pub const HOP_SAMPLES: usize = WINDOW_SAMPLES - OVERLAP_SAMPLES;
/// Pitch bins per frame: the piano's 88 keys.
pub const PITCHES: usize = 88;
/// The MIDI number of the first pitch bin (A0).
pub const MIDI_OFFSET: u8 = 21;

/// The windows the model is run on, each [`WINDOW_SAMPLES`] long,
/// zero-padded at the end. Silence of half the overlap is prepended.
#[must_use]
pub fn windows(samples: &[f32]) -> Vec<Vec<f32>> {
    let mut padded = vec![0.0f32; OVERLAP_SAMPLES / 2];
    padded.extend_from_slice(samples);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < padded.len() {
        let end = (start + WINDOW_SAMPLES).min(padded.len());
        let mut window = padded[start..end].to_vec();
        window.resize(WINDOW_SAMPLES, 0.0);
        out.push(window);
        start += HOP_SAMPLES;
    }
    out
}

/// How many stitched frames describe `samples` samples of audio.
#[must_use]
pub fn frames_for(samples: usize) -> usize {
    samples * FPS / SAMPLE_RATE as usize
}

/// Stitch per-window answers of `width` values a frame into one
/// timeline of `frames_for(original_samples)` frames: each window's
/// first and last `OVERLAP_FRAMES / 2` frames are dropped, the rest
/// concatenated, and the end trimmed to the audio.
///
/// Every window's answer must hold [`WINDOW_FRAMES`] × `width` values;
/// one that does not is refused with its index.
///
/// # Errors
/// When a window's answer has the wrong size.
pub fn stitch(
    answers: &[Vec<f32>],
    width: usize,
    original_samples: usize,
) -> Result<Vec<f32>, usize> {
    let trim = OVERLAP_FRAMES / 2;
    let mut out = Vec::with_capacity(answers.len() * (WINDOW_FRAMES - 2 * trim) * width);
    for (index, answer) in answers.iter().enumerate() {
        if answer.len() != WINDOW_FRAMES * width {
            return Err(index);
        }
        out.extend_from_slice(&answer[trim * width..(WINDOW_FRAMES - trim) * width]);
    }
    out.truncate(frames_for(original_samples) * width);
    Ok(out)
}

/// The song time of stitched frame `frame`, in seconds — the frame's
/// nominal time less the drift the stitching adds per window (see the
/// module notes), plus the reference's own fixed 1.8 ms.
#[must_use]
pub fn frame_time(frame: usize) -> f64 {
    let hop_s = FFT_HOP as f64 / f64::from(SAMPLE_RATE);
    let nominal = frame as f64 * hop_s;
    let window = (frame / WINDOW_FRAMES) as f64;
    let offset = hop_s * (WINDOW_FRAMES as f64 - WINDOW_SAMPLES as f64 / FFT_HOP as f64) + 0.0018;
    nominal - offset * window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_are_the_references() {
        assert_eq!(FPS, 86);
        assert_eq!(WINDOW_FRAMES, 172);
        assert_eq!(WINDOW_SAMPLES, 43_844);
        assert_eq!(OVERLAP_SAMPLES, 7_680);
        assert_eq!(HOP_SAMPLES, 36_164);
    }

    #[test]
    fn a_song_is_cut_into_padded_overlapping_windows() {
        let samples: Vec<f32> = (0..100_000).map(|i| i as f32).collect();
        let cut = windows(&samples);
        // 3 840 of silence first, then windows every 36 164 samples.
        assert_eq!(cut.len(), (100_000 + 3_840usize).div_ceil(HOP_SAMPLES));
        assert!(cut.iter().all(|w| w.len() == WINDOW_SAMPLES));
        assert_eq!(cut[0][3_839], 0.0);
        assert_eq!(cut[0][3_840], 0.0, "the first real sample");
        assert_eq!(cut[0][3_841], 1.0);
        assert_eq!(cut[1][0], (HOP_SAMPLES - 3_840) as f32);
        // The last one is padded with silence.
        assert_eq!(
            *cut.last().expect("a window").last().expect("a sample"),
            0.0
        );
    }

    #[test]
    fn stitching_keeps_the_middles_and_trims_to_the_audio() {
        // Two windows, width 1: frame values are window*1000 + frame.
        let answers: Vec<Vec<f32>> = (0..2)
            .map(|w| (0..WINDOW_FRAMES).map(|f| (w * 1000 + f) as f32).collect())
            .collect();
        let samples = 60_000;
        let out = stitch(&answers, 1, samples).expect("stitched");
        assert_eq!(out.len(), frames_for(samples));
        assert_eq!(out[0], 15.0, "the first 15 frames of a window are dropped");
        assert_eq!(out[141], 156.0, "and the last 15");
        assert_eq!(out[142], 1015.0, "the next window's middle follows");
        // A wrong-sized answer is named, not stitched.
        let bad = vec![answers[0].clone(), vec![0.0; 3]];
        assert_eq!(stitch(&bad, 1, samples), Err(1));
    }

    #[test]
    fn frame_times_take_the_stitching_drift_back_out() {
        let hop = FFT_HOP as f64 / f64::from(SAMPLE_RATE);
        assert!((frame_time(0) + 0.0).abs() < 1e-12);
        assert!((frame_time(10) - 10.0 * hop).abs() < 1e-12);
        // One window in, the drift of one window is subtracted: about
        // 10.3 ms. Ten windows in, ten of them.
        let drift = WINDOW_FRAMES as f64 * hop - frame_time(WINDOW_FRAMES);
        assert!((drift - 0.010_326).abs() < 1e-5, "{drift}");
        let later = 1_720usize;
        assert!((later as f64 * hop - frame_time(later) - 10.0 * drift).abs() < 1e-9);
    }
}
