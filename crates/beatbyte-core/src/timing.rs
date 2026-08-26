//! Musical timing: tempo maps, hit windows and judgment.
//!
//! **Convention:** all times in the domain are `f64` seconds on the song
//! timeline (`0.0` = start of the audio file). Milliseconds appear only
//! at configuration boundaries (e.g. latency calibration UI) and are
//! converted immediately.
//!
//! Gameplay never derives timing from frame counts; the presentation
//! layer asks this module where things are at a given song time.

use serde::{Deserialize, Serialize};

/// A tempo change: from `time_s` onward, the song runs at `bpm`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoChange {
    /// Song time in seconds at which this tempo takes effect.
    pub time_s: f64,
    /// Tempo in beats per minute. Must be finite and positive.
    pub bpm: f64,
}

/// Maps between song time (seconds) and musical time (beats).
///
/// Format v1 charts carry a single BPM, but the domain supports a full
/// tempo map so tempo changes are a file-format addition, not a
/// gameplay rewrite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempoMap {
    /// Tempo changes ordered by time; the first entry's `time_s` is the
    /// musical origin (beat 0), typically the chart offset.
    changes: Vec<TempoChange>,
}

impl TempoMap {
    /// Fallback tempo used to guard against degenerate input.
    const FALLBACK_BPM: f64 = 120.0;

    /// A constant-tempo map: `bpm` starting at `offset_s` (beat 0).
    #[must_use]
    pub fn constant(bpm: f64, offset_s: f64) -> TempoMap {
        let bpm = if bpm.is_finite() && bpm > 0.0 {
            bpm
        } else {
            Self::FALLBACK_BPM
        };
        TempoMap {
            changes: vec![TempoChange {
                time_s: offset_s,
                bpm,
            }],
        }
    }

    /// Build from a list of tempo changes. Changes are sorted by time;
    /// non-finite or non-positive BPMs are rejected.
    pub fn from_changes(mut changes: Vec<TempoChange>) -> Result<TempoMap, TempoMapError> {
        if changes.is_empty() {
            return Err(TempoMapError::Empty);
        }
        for change in &changes {
            if !change.bpm.is_finite() || change.bpm <= 0.0 {
                return Err(TempoMapError::InvalidBpm(change.bpm));
            }
            if !change.time_s.is_finite() {
                return Err(TempoMapError::InvalidTime(change.time_s));
            }
        }
        changes.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        Ok(TempoMap { changes })
    }

    /// The tempo in effect at the given song time.
    #[must_use]
    pub fn bpm_at(&self, time_s: f64) -> f64 {
        let mut bpm = self.changes[0].bpm;
        for change in &self.changes {
            if change.time_s <= time_s {
                bpm = change.bpm;
            } else {
                break;
            }
        }
        bpm
    }

    /// Seconds per beat at the given song time.
    #[must_use]
    pub fn seconds_per_beat_at(&self, time_s: f64) -> f64 {
        60.0 / self.bpm_at(time_s)
    }

    /// Convert a song time to musical beats (beat 0 = first change).
    #[must_use]
    pub fn beats_at(&self, time_s: f64) -> f64 {
        let mut beats = 0.0;
        let mut prev = self.changes[0];
        // Time before the musical origin counts backwards at the first tempo.
        if time_s < prev.time_s {
            return (time_s - prev.time_s) * prev.bpm / 60.0;
        }
        for change in &self.changes[1..] {
            if change.time_s >= time_s {
                break;
            }
            beats += (change.time_s - prev.time_s) * prev.bpm / 60.0;
            prev = *change;
        }
        beats + (time_s - prev.time_s) * prev.bpm / 60.0
    }

    /// Convert musical beats back to song time in seconds.
    #[must_use]
    pub fn time_at_beats(&self, beats: f64) -> f64 {
        let mut remaining = beats;
        let mut prev = self.changes[0];
        if remaining <= 0.0 {
            return prev.time_s + remaining * 60.0 / prev.bpm;
        }
        for change in &self.changes[1..] {
            let segment_beats = (change.time_s - prev.time_s) * prev.bpm / 60.0;
            if remaining <= segment_beats {
                break;
            }
            remaining -= segment_beats;
            prev = *change;
        }
        prev.time_s + remaining * 60.0 / prev.bpm
    }

    /// The ordered tempo changes.
    #[must_use]
    pub fn changes(&self) -> &[TempoChange] {
        &self.changes
    }
}

/// Errors when constructing a [`TempoMap`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TempoMapError {
    /// No tempo changes were provided.
    Empty,
    /// A BPM was non-finite or non-positive.
    InvalidBpm(f64),
    /// A change time was non-finite.
    InvalidTime(f64),
}

impl core::fmt::Display for TempoMapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TempoMapError::Empty => write!(f, "tempo map needs at least one tempo change"),
            TempoMapError::InvalidBpm(bpm) => write!(f, "invalid BPM {bpm}"),
            TempoMapError::InvalidTime(t) => write!(f, "invalid tempo change time {t}"),
        }
    }
}

impl core::error::Error for TempoMapError {}

/// Judgment of a single hit, best to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Judgment {
    /// Dead on the note.
    Perfect,
    /// Close.
    Great,
    /// In the window, but sloppy.
    Good,
    /// Missed entirely (note passed unhit, or wrong frets).
    Miss,
}

impl Judgment {
    /// Weight used for the accuracy percentage (1.0 = flawless).
    #[must_use]
    pub const fn accuracy_weight(self) -> f64 {
        match self {
            Judgment::Perfect => 1.0,
            Judgment::Great => 0.75,
            Judgment::Good => 0.4,
            Judgment::Miss => 0.0,
        }
    }
}

/// Symmetric hit windows, as half-widths in seconds.
///
/// A hit at absolute offset `|Δ| ≤ perfect_s` judges Perfect, then
/// Great, then Good; anything outside `good_s` is not a hit at all.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimingWindows {
    /// Half-width of the Perfect window in seconds.
    pub perfect_s: f64,
    /// Half-width of the Great window in seconds.
    pub great_s: f64,
    /// Half-width of the Good window (= the full hit window) in seconds.
    pub good_s: f64,
}

impl Default for TimingWindows {
    fn default() -> Self {
        // Tuned toward the classic guitar-game feel: a generous overall
        // window (±100 ms) with meaningful accuracy tiers inside it.
        TimingWindows {
            perfect_s: 0.030,
            great_s: 0.060,
            good_s: 0.100,
        }
    }
}

impl TimingWindows {
    /// Judge a hit by its signed offset from the note time
    /// (`hit_time - note_time`). Returns `None` when outside the window.
    #[must_use]
    pub fn judge(&self, offset_s: f64) -> Option<Judgment> {
        let abs = offset_s.abs();
        if abs <= self.perfect_s {
            Some(Judgment::Perfect)
        } else if abs <= self.great_s {
            Some(Judgment::Great)
        } else if abs <= self.good_s {
            Some(Judgment::Good)
        } else {
            None
        }
    }

    /// Whether the windows are sane: positive, ordered, and small enough
    /// to be a hit window rather than a bug.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.perfect_s > 0.0
            && self.perfect_s <= self.great_s
            && self.great_s <= self.good_s
            && self.good_s <= 0.5
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn constant_tempo_beats_round_trip() {
        let map = TempoMap::constant(120.0, 0.5);
        // 120 BPM = 0.5s per beat, origin at 0.5s.
        assert!((map.beats_at(0.5) - 0.0).abs() < EPS);
        assert!((map.beats_at(1.0) - 1.0).abs() < EPS);
        assert!((map.beats_at(2.5) - 4.0).abs() < EPS);
        assert!((map.time_at_beats(4.0) - 2.5).abs() < EPS);
        assert!((map.time_at_beats(0.0) - 0.5).abs() < EPS);
    }

    #[test]
    fn time_before_origin_counts_negative_beats() {
        let map = TempoMap::constant(120.0, 1.0);
        assert!((map.beats_at(0.0) - (-2.0)).abs() < EPS);
        assert!((map.time_at_beats(-2.0) - 0.0).abs() < EPS);
    }

    #[test]
    fn tempo_changes_are_respected() {
        // 60 BPM for 10s (10 beats), then 120 BPM.
        let map = TempoMap::from_changes(vec![
            TempoChange {
                time_s: 0.0,
                bpm: 60.0,
            },
            TempoChange {
                time_s: 10.0,
                bpm: 120.0,
            },
        ])
        .unwrap();

        assert!((map.beats_at(10.0) - 10.0).abs() < EPS);
        assert!((map.beats_at(11.0) - 12.0).abs() < EPS);
        assert!((map.time_at_beats(12.0) - 11.0).abs() < EPS);
        assert!((map.bpm_at(5.0) - 60.0).abs() < EPS);
        assert!((map.bpm_at(10.0) - 120.0).abs() < EPS);
    }

    #[test]
    fn beats_and_time_are_inverse_across_changes() {
        let map = TempoMap::from_changes(vec![
            TempoChange {
                time_s: 0.25,
                bpm: 90.0,
            },
            TempoChange {
                time_s: 8.0,
                bpm: 174.0,
            },
            TempoChange {
                time_s: 30.0,
                bpm: 60.0,
            },
        ])
        .unwrap();

        for &t in &[0.0, 0.25, 3.0, 8.0, 9.5, 30.0, 45.0] {
            let beats = map.beats_at(t);
            let back = map.time_at_beats(beats);
            assert!(
                (back - t).abs() < 1e-6,
                "round trip failed for t={t}: beats={beats}, back={back}"
            );
        }
    }

    #[test]
    fn degenerate_tempo_input_is_rejected() {
        assert_eq!(
            TempoMap::from_changes(vec![]).unwrap_err(),
            TempoMapError::Empty
        );
        assert!(matches!(
            TempoMap::from_changes(vec![TempoChange {
                time_s: 0.0,
                bpm: 0.0
            }]),
            Err(TempoMapError::InvalidBpm(_))
        ));
        assert!(matches!(
            TempoMap::from_changes(vec![TempoChange {
                time_s: f64::NAN,
                bpm: 120.0
            }]),
            Err(TempoMapError::InvalidTime(_))
        ));
        // `constant` guards instead of erroring (it is used with
        // already-validated chart data).
        let map = TempoMap::constant(f64::NAN, 0.0);
        assert!(map.bpm_at(0.0) > 0.0);
    }

    #[test]
    fn window_boundaries_are_inclusive() {
        // A hit EXACTLY on a window edge takes the better tier —
        // `<=`, not `<`. The difference is one representable float,
        // but it is the documented contract (30/60/100 ms).
        let w = TimingWindows::default();
        assert_eq!(w.judge(w.perfect_s), Some(Judgment::Perfect));
        assert_eq!(w.judge(-w.perfect_s), Some(Judgment::Perfect));
        assert_eq!(w.judge(w.perfect_s + 1e-9), Some(Judgment::Great));
        assert_eq!(w.judge(w.great_s), Some(Judgment::Great));
        assert_eq!(w.judge(w.good_s), Some(Judgment::Good));
        assert_eq!(w.judge(-w.good_s), Some(Judgment::Good));
        assert_eq!(w.judge(w.good_s + 1e-9), None);
    }

    #[test]
    fn accuracy_weights_rank_with_quality() {
        let p = Judgment::Perfect.accuracy_weight();
        let g = Judgment::Great.accuracy_weight();
        let o = Judgment::Good.accuracy_weight();
        let m = Judgment::Miss.accuracy_weight();
        assert!(p > g && g > o && o > m, "{p} {g} {o} {m}");
        assert!((p - 1.0).abs() < f64::EPSILON, "perfect must weigh 1.0");
        assert!(m.abs() < f64::EPSILON, "a miss must weigh nothing");
    }

    #[test]
    fn judgment_tiers() {
        let w = TimingWindows::default();
        assert_eq!(w.judge(0.0), Some(Judgment::Perfect));
        assert_eq!(w.judge(0.030), Some(Judgment::Perfect));
        assert_eq!(w.judge(-0.030), Some(Judgment::Perfect));
        assert_eq!(w.judge(0.031), Some(Judgment::Great));
        assert_eq!(w.judge(-0.060), Some(Judgment::Great));
        assert_eq!(w.judge(0.061), Some(Judgment::Good));
        assert_eq!(w.judge(-0.100), Some(Judgment::Good));
        assert_eq!(w.judge(0.101), None);
        assert_eq!(w.judge(-5.0), None);
    }

    #[test]
    fn default_windows_are_valid() {
        assert!(TimingWindows::default().is_valid());
        assert!(
            !TimingWindows {
                perfect_s: 0.1,
                great_s: 0.05,
                good_s: 0.2
            }
            .is_valid()
        );
        assert!(
            !TimingWindows {
                perfect_s: 0.0,
                great_s: 0.05,
                good_s: 0.2
            }
            .is_valid()
        );
    }

    #[test]
    fn judgments_order_best_to_worst() {
        assert!(Judgment::Perfect < Judgment::Great);
        assert!(Judgment::Great < Judgment::Good);
        assert!(Judgment::Good < Judgment::Miss);
    }
}
