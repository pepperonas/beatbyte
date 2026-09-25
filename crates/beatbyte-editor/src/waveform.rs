//! The waveform strip: an envelope of the song, computed once.
//!
//! The editor draws the song beside the lanes so a stroke can be SEEN
//! where it is heard. Drawing samples is out of the question (a
//! three-minute song is eight million of them), so the song is reduced
//! once to a peak per bucket, and a screen row asks for the loudest
//! bucket in its time span — which stays right at every zoom, because
//! a peak of peaks is the peak.

/// Buckets per second of the envelope: 5 ms each — finer than a pixel
/// at the editor's deepest zoom shows per row, coarse enough that a
/// long song stays small (a ten-minute song is 120 000 floats).
pub const BUCKETS_PER_S: f64 = 200.0;

/// A song's envelope: the absolute peak of every bucket, 0–1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Envelope {
    peaks: Vec<f32>,
}

impl Envelope {
    /// Reduce mono samples to the envelope.
    #[must_use]
    pub fn from_samples(samples: &[f32], sample_rate: u32) -> Envelope {
        if sample_rate == 0 {
            return Envelope::default();
        }
        let per_bucket = (f64::from(sample_rate) / BUCKETS_PER_S).round().max(1.0) as usize;
        let peaks: Vec<f32> = samples
            .chunks(per_bucket)
            .map(|chunk| chunk.iter().fold(0.0f32, |peak, s| peak.max(s.abs())))
            .collect();
        // Scale to the loudest bucket so a quiet master still reads.
        let loudest = peaks.iter().copied().fold(0.0f32, f32::max);
        if loudest <= f32::EPSILON {
            return Envelope { peaks };
        }
        Envelope {
            peaks: peaks.into_iter().map(|p| (p / loudest).min(1.0)).collect(),
        }
    }

    /// The song's length the envelope covers, seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.peaks.len() as f64 / BUCKETS_PER_S
    }

    /// Whether there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peaks.is_empty()
    }

    /// The loudest point in `[t0, t1)`, 0–1 (0 outside the song).
    #[must_use]
    pub fn peak(&self, t0: f64, t1: f64) -> f32 {
        if self.peaks.is_empty() || t1 <= 0.0 {
            return 0.0;
        }
        let first = (t0.max(0.0) * BUCKETS_PER_S).floor() as usize;
        let last = ((t1 * BUCKETS_PER_S).ceil() as usize).max(first + 1);
        if first >= self.peaks.len() {
            return 0.0;
        }
        self.peaks[first..last.min(self.peaks.len())]
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of silence, then a click of 0.5 at 1.5 s, then
    /// silence: the envelope finds the click at every resolution.
    #[test]
    fn a_peak_is_found_at_every_zoom() {
        let rate = 8_000u32;
        let mut samples = vec![0.0f32; rate as usize * 3];
        samples[(rate as f64 * 1.5) as usize] = -0.5;
        samples[100] = 0.25;
        let envelope = Envelope::from_samples(&samples, rate);
        assert!((envelope.duration_s() - 3.0).abs() < 0.01);
        assert!(
            (envelope.peak(1.49, 1.51) - 1.0).abs() < 1e-6,
            "scaled to the loudest"
        );
        assert!((envelope.peak(0.0, 3.0) - 1.0).abs() < 1e-6);
        assert!((envelope.peak(0.0, 0.1) - 0.5).abs() < 1e-6);
        assert_eq!(envelope.peak(0.5, 1.0), 0.0);
        assert_eq!(envelope.peak(10.0, 11.0), 0.0, "past the end is silence");
        assert_eq!(envelope.peak(-2.0, -1.0), 0.0);
        // A span narrower than one bucket still reads its bucket.
        assert!(envelope.peak(1.5, 1.5001) > 0.9);
    }

    #[test]
    fn silence_and_nothing_are_empty_not_a_crash() {
        assert!(Envelope::from_samples(&[], 44_100).is_empty());
        assert!(Envelope::from_samples(&[0.1], 0).is_empty());
        let quiet = Envelope::from_samples(&[0.0; 1000], 1000);
        assert_eq!(quiet.peak(0.0, 1.0), 0.0);
    }
}
