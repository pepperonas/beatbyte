//! Musical analysis results: the shared vocabulary between the audio
//! analysis pipeline (`beatbyte-audio`) and the chart generator
//! (`beatbyte-chart`).
//!
//! These types live in `core` so the generator never has to depend on
//! the audio stack — analysis produces a [`SongAnalysis`], generation
//! consumes one, and both sides test against plain values.

use serde::{Deserialize, Serialize};

/// A detected onset: something percussive/note-like started here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Onset {
    /// Song time in seconds.
    pub time_s: f64,
    /// Relative salience in 0.0–1.0 (1.0 = strongest onset in the song).
    pub strength: f32,
    /// Spectral brightness at the onset in 0.0–1.0 (0 = bassy,
    /// 1 = bright). Drives lane assignment, not judgment.
    pub brightness: f32,
}

/// The result of analyzing a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongAnalysis {
    /// Estimated tempo in beats per minute.
    pub bpm: f64,
    /// Confidence of the tempo estimate, 0.0–1.0.
    pub bpm_confidence: f64,
    /// The plausible alternative tempo octave (half or double), if any.
    pub alt_bpm: Option<f64>,
    /// Beat grid: song time of every estimated beat, ascending.
    pub beats: Vec<f64>,
    /// Detected onsets, ascending by time.
    pub onsets: Vec<Onset>,
    /// Normalized RMS energy envelope (0.0–1.0), sampled every
    /// [`SongAnalysis::energy_hop_s`] seconds from time 0.
    pub energy: Vec<f32>,
    /// Seconds between energy samples.
    pub energy_hop_s: f64,
    /// Analyzed duration in seconds.
    pub duration_s: f64,
}

impl SongAnalysis {
    /// The energy at a given song time (nearest sample, 0 outside).
    #[must_use]
    pub fn energy_at(&self, time_s: f64) -> f32 {
        if self.energy.is_empty() || self.energy_hop_s <= 0.0 || time_s < 0.0 {
            return 0.0;
        }
        let index = (time_s / self.energy_hop_s).round() as usize;
        self.energy.get(index).copied().unwrap_or(0.0)
    }

    /// The beat interval in seconds implied by the BPM.
    #[must_use]
    pub fn beat_interval_s(&self) -> f64 {
        60.0 / self.bpm.max(f64::EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis() -> SongAnalysis {
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: Some(60.0),
            beats: vec![0.0, 0.5, 1.0],
            onsets: vec![],
            energy: vec![0.0, 0.5, 1.0],
            energy_hop_s: 0.1,
            duration_s: 1.5,
        }
    }

    #[test]
    fn energy_lookup_uses_nearest_sample() {
        let a = analysis();
        assert_eq!(a.energy_at(0.0), 0.0);
        assert_eq!(a.energy_at(0.1), 0.5);
        assert_eq!(a.energy_at(0.09), 0.5);
        assert_eq!(a.energy_at(0.21), 1.0);
        assert_eq!(a.energy_at(5.0), 0.0, "outside → 0");
        assert_eq!(a.energy_at(-1.0), 0.0);
    }

    #[test]
    fn beat_interval_from_bpm() {
        assert!((analysis().beat_interval_s() - 0.5).abs() < 1e-12);
    }
}
