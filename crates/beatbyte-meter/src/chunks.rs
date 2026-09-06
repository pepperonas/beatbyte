//! Cutting a spectrogram into the pieces the model was trained on.
//!
//! Beat This! sees 1500 frames (30 s) at a time. A longer song is cut
//! into chunks that overlap by twice a border of 6 frames; the border
//! of every prediction is discarded (the model's edges are unreliable
//! for want of context) and the first chunk to cover a frame keeps
//! it. The last chunk is pulled back to end on the song's last frame
//! rather than run short. This is the reference implementation's
//! `split_predict_aggregate` with `keep_first`, written out; the
//! numbers are the reference's, not tuned here.

/// Frames per chunk.
pub const CHUNK: usize = 1500;
/// Frames discarded at each edge of a prediction.
pub const BORDER: usize = 6;
/// Frames between chunk starts.
pub const STRIDE: usize = CHUNK - 2 * BORDER;

/// Where each chunk starts, in frames; the first is negative (left
/// padding), the last is pulled back to end on the final frame once
/// the song is longer than one stride.
#[must_use]
pub fn starts(frames: usize) -> Vec<i64> {
    let frames_i = frames as i64;
    let border = BORDER as i64;
    let mut out = Vec::new();
    let mut pos = -border;
    while pos < frames_i - border {
        out.push(pos);
        pos += STRIDE as i64;
    }
    if out.is_empty() {
        out.push(-border);
    }
    if frames > STRIDE
        && let Some(last) = out.last_mut()
    {
        *last = frames_i - (CHUNK as i64 - border);
    }
    out
}

/// One chunk, zero-padded where it reaches past the song.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// The frame this chunk's first row corresponds to (negative =
    /// padding before the song).
    pub start: i64,
    /// Rows in `data`.
    pub frames: usize,
    /// `frames * bands` values, row-major.
    pub data: Vec<f32>,
}

/// Cut the chunk at `start` from a `frames × bands` spectrogram.
/// Right padding is capped at [`BORDER`], as the reference does: a
/// short song yields a short chunk, a long one a full chunk.
#[must_use]
pub fn cut(spectrogram: &[f32], frames: usize, bands: usize, start: i64) -> Chunk {
    let frames_i = frames as i64;
    let first = start.max(0);
    let end = (start + CHUNK as i64).min(frames_i);
    let pad_left = (-start).max(0) as usize;
    let rows = (end - first).max(0) as usize;
    let pad_right = (start + CHUNK as i64 - frames_i).min(BORDER as i64).max(0) as usize;
    let total = pad_left + rows + pad_right;
    let mut data = vec![0.0f32; total * bands];
    let src = &spectrogram[first as usize * bands..(first as usize + rows) * bands];
    data[pad_left * bands..(pad_left + rows) * bands].copy_from_slice(src);
    Chunk {
        start,
        frames: total,
        data,
    }
}

/// The song's logits, assembled keep-first from chunk predictions.
#[derive(Debug, Clone, PartialEq)]
pub struct Stitched {
    beats: Vec<f32>,
    downbeats: Vec<f32>,
    written: Vec<bool>,
}

impl Stitched {
    /// Room for `frames` frames, none written yet.
    #[must_use]
    pub fn new(frames: usize) -> Stitched {
        Stitched {
            beats: vec![f32::NEG_INFINITY; frames],
            downbeats: vec![f32::NEG_INFINITY; frames],
            written: vec![false; frames],
        }
    }

    /// Take a chunk's prediction (one value per chunk row) minus its
    /// borders; a frame already written keeps its earlier value.
    pub fn write(&mut self, start: i64, beats: &[f32], downbeats: &[f32]) {
        let rows = beats.len().min(downbeats.len());
        if rows <= 2 * BORDER {
            return;
        }
        for row in BORDER..rows - BORDER {
            let dest = start + row as i64;
            if dest < 0 {
                continue;
            }
            let dest = dest as usize;
            if dest >= self.written.len() || self.written[dest] {
                continue;
            }
            self.beats[dest] = beats[row];
            self.downbeats[dest] = downbeats[row];
            self.written[dest] = true;
        }
    }

    /// Whether every frame received a prediction.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.written.iter().all(|&w| w)
    }

    /// `(beat logits, downbeat logits)`.
    #[must_use]
    pub fn finish(self) -> (Vec<f32>, Vec<f32>) {
        (self.beats, self.downbeats)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn short_songs_are_one_chunk_and_long_ones_end_on_the_last_frame() {
        assert_eq!(starts(100), vec![-6]);
        // Exactly one chunk of rows still leaves the last twelve frames
        // uncovered after the borders come off: a second chunk.
        assert_eq!(starts(1500), vec![-6, 6]);
        assert_eq!(starts(2000), vec![-6, 506]);
        assert_eq!(starts(5000), vec![-6, 1482, 2970, 3506]);
        assert_eq!(starts(0), vec![-6]);
    }

    #[test]
    fn every_frame_is_covered_exactly_once_after_stitching() {
        for frames in [1, 50, 100, 500, 1488, 1500, 2000, 3000, 5000, 7800] {
            let mut stitched = Stitched::new(frames);
            for start in starts(frames) {
                let chunk = cut(&vec![0.0; frames * 2], frames, 2, start);
                let rows = vec![start as f32; chunk.frames];
                stitched.write(start, &rows, &rows);
            }
            assert!(stitched.complete(), "{frames} frames left a hole");
            let (beats, _) = stitched.finish();
            // Keep-first: a frame carries the EARLIEST chunk's mark.
            for (i, w) in beats.windows(2).enumerate() {
                assert!(
                    w[1] >= w[0],
                    "frame {i}: a later chunk overwrote an earlier one"
                );
            }
        }
    }

    #[test]
    fn a_chunk_is_padded_left_by_the_border_and_right_by_at_most_the_border() {
        let frames = 100;
        let spec: Vec<f32> = (0..frames * 3).map(|i| i as f32).collect();
        let chunk = cut(&spec, frames, 3, -6);
        assert_eq!(chunk.frames, 6 + 100 + 6);
        assert!(chunk.data[..6 * 3].iter().all(|&v| v == 0.0));
        assert_eq!(&chunk.data[6 * 3..7 * 3], &[0.0, 1.0, 2.0]);
        assert!(chunk.data[(6 + 100) * 3..].iter().all(|&v| v == 0.0));

        let long: Vec<f32> = vec![1.0; 5000 * 3];
        let middle = cut(&long, 5000, 3, 100);
        assert_eq!(middle.frames, CHUNK);
        assert!(middle.data.iter().all(|&v| v == 1.0));
        let first = cut(&long, 5000, 3, -6);
        assert_eq!(first.frames, CHUNK);
        assert!(first.data[..6 * 3].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn a_prediction_shorter_than_two_borders_writes_nothing() {
        let mut stitched = Stitched::new(20);
        stitched.write(0, &[1.0; 12], &[1.0; 12]);
        assert!(!stitched.complete());
        assert!(stitched.finish().0.iter().all(|v| v.is_infinite()));
    }
}
