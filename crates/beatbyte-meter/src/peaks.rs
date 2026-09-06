//! Logits to times: the reference's "minimal" post-processor.
//!
//! A frame is a beat when its logit is positive (probability above a
//! half) and no frame within three on either side is higher; runs of
//! adjacent peaks are merged at their mean. Downbeats are decoded the
//! same way and then moved onto the nearest beat, because a downbeat
//! that is not a beat is not a bar line. Pure and tested.

use crate::FPS;

/// Half the max-pool window, in frames.
const HALF_WINDOW: usize = 3;

/// Decode `(beats, downbeats)` in seconds from per-frame logits at
/// [`FPS`]. The two slices are read up to the shorter length.
#[must_use]
pub fn decode(beat_logits: &[f32], downbeat_logits: &[f32]) -> (Vec<f64>, Vec<f64>) {
    let n = beat_logits.len().min(downbeat_logits.len());
    let beats = to_seconds(&pick(&beat_logits[..n]));
    let downbeats = snap_to(&beats, &to_seconds(&pick(&downbeat_logits[..n])));
    (beats, downbeats)
}

/// Peak frames (fractional after merging), ascending.
#[must_use]
pub fn pick(logits: &[f32]) -> Vec<f64> {
    let mut peaks = Vec::new();
    for (i, &value) in logits.iter().enumerate() {
        if value <= 0.0 {
            continue;
        }
        let from = i.saturating_sub(HALF_WINDOW);
        let to = (i + HALF_WINDOW + 1).min(logits.len());
        if logits[from..to].iter().all(|&other| other <= value) {
            peaks.push(i);
        }
    }
    merge_adjacent(&peaks, 1)
}

/// Merge peaks no more than `width` frames apart into one at their
/// running mean (the reference keeps the fraction, so do we).
#[must_use]
pub fn merge_adjacent(peaks: &[usize], width: usize) -> Vec<f64> {
    let mut out = Vec::new();
    let Some(&first) = peaks.first() else {
        return out;
    };
    let mut mean = first as f64;
    let mut count = 1.0;
    for &next in &peaks[1..] {
        let next = next as f64;
        if next - mean <= width as f64 {
            count += 1.0;
            mean += (next - mean) / count;
        } else {
            out.push(mean);
            mean = next;
            count = 1.0;
        }
    }
    out.push(mean);
    out
}

/// Frames to seconds.
#[must_use]
pub fn to_seconds(frames: &[f64]) -> Vec<f64> {
    frames.iter().map(|f| f / FPS).collect()
}

/// Each `time` moved onto the nearest of `beats`, sorted, without
/// duplicates. Without beats the times are returned as they are.
#[must_use]
pub fn snap_to(beats: &[f64], times: &[f64]) -> Vec<f64> {
    if beats.is_empty() {
        return times.to_vec();
    }
    let mut out: Vec<f64> = times
        .iter()
        .map(|&t| {
            let i = beats.partition_point(|&b| b < t);
            match (i.checked_sub(1).map(|j| beats[j]), beats.get(i)) {
                (Some(before), Some(&after)) => {
                    if t - before <= after - t {
                        before
                    } else {
                        after
                    }
                }
                (Some(before), None) => before,
                (None, Some(&after)) => after,
                (None, None) => t,
            }
        })
        .collect();
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_is_a_positive_local_maximum() {
        assert_eq!(pick(&[0.0, 0.0, 0.5, 1.0, 0.5, 0.0, 0.0]), vec![3.0]);
        assert!(pick(&[-1.0, -0.5, -2.0, -0.1]).is_empty());
        let mut logits = vec![0.0; 20];
        logits[3] = 2.0;
        logits[15] = 1.5;
        assert_eq!(pick(&logits), vec![3.0, 15.0]);
        // A higher neighbour within three frames suppresses a peak.
        let mut close = vec![0.0; 20];
        close[5] = 1.0;
        close[7] = 2.0;
        assert_eq!(pick(&close), vec![7.0]);
    }

    #[test]
    fn adjacent_equal_peaks_merge_at_their_mean() {
        assert_eq!(pick(&[0.0, 1.0, 1.0, 0.0]), vec![1.5]);
        assert_eq!(merge_adjacent(&[10, 11, 12, 20], 1), vec![10.5, 12.0, 20.0]);
        let merged = merge_adjacent(&[10, 11, 11, 20], 1);
        assert_eq!(merged.len(), 2);
        assert!((merged[0] - 32.0 / 3.0).abs() < 1e-9);
        assert_eq!(merge_adjacent(&[], 1), Vec::<f64>::new());
        assert_eq!(merge_adjacent(&[42], 1), vec![42.0]);
    }

    #[test]
    fn downbeats_land_on_the_nearest_beat_and_never_twice() {
        assert_eq!(snap_to(&[1.0, 2.0, 3.0], &[1.1, 2.8]), vec![1.0, 3.0]);
        assert_eq!(snap_to(&[1.0, 2.0, 3.0], &[1.8, 2.1]), vec![2.0]);
        assert_eq!(snap_to(&[], &[1.0, 2.0]), vec![1.0, 2.0]);
        assert_eq!(snap_to(&[1.0, 2.0], &[]), Vec::<f64>::new());
        assert_eq!(snap_to(&[5.0], &[0.0, 9.0]), vec![5.0]);
    }

    #[test]
    fn decode_reads_frames_as_fifty_a_second() {
        let mut beat = vec![-5.0; 200];
        let mut down = vec![-5.0; 200];
        beat[50] = 3.0;
        beat[100] = 2.5;
        beat[150] = 4.0;
        down[51] = 2.0;
        let (beats, downbeats) = decode(&beat, &down);
        assert_eq!(beats, vec![1.0, 2.0, 3.0]);
        assert_eq!(downbeats, vec![1.0]);
        assert_eq!(decode(&[], &[]), (vec![], vec![]));
        // Mismatched lengths read up to the shorter one, no panic.
        assert_eq!(decode(&[1.0, 2.0], &[1.0]), (vec![0.0], vec![0.0]));
    }
}
