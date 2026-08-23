//! The song clock: the authoritative gameplay timeline.
//!
//! The audio device reports playback position in coarse buffer-sized
//! steps; gameplay needs a smooth, monotonic-feeling timeline. The
//! [`SongClock`] anchors song time to a monotonic clock and *reconciles*
//! against the reported playback position: large drift snaps, small
//! drift is slewed gradually so the timeline never jumps perceptibly.
//!
//! The clock is a pure state machine — monotonic "now" is always passed
//! in as a parameter, never read from the system — so pause, seek and
//! drift behavior are all unit-testable.

/// Drift at or above this snaps the anchor instead of slewing.
pub const SNAP_THRESHOLD_S: f64 = 0.030;

/// Fraction of the measured drift corrected per reconciliation.
pub const SLEW_FACTOR: f64 = 0.10;

/// Playback state of the clock.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClockState {
    /// No song loaded / stopped.
    Stopped,
    /// Paused at a song time.
    Paused {
        /// The held song time in seconds.
        song_s: f64,
    },
    /// Playing: song time = `anchor_song_s + (mono_now − anchor_mono_s)`.
    Playing {
        /// Monotonic time of the anchor.
        anchor_mono_s: f64,
        /// Song time at the anchor.
        anchor_song_s: f64,
    },
}

/// The authoritative song timeline (see module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongClock {
    state: ClockState,
}

impl Default for SongClock {
    fn default() -> Self {
        SongClock::new()
    }
}

impl SongClock {
    /// A stopped clock.
    #[must_use]
    pub const fn new() -> SongClock {
        SongClock {
            state: ClockState::Stopped,
        }
    }

    /// Start (or restart) playback at `song_s`, anchored at `mono_s`.
    pub fn start(&mut self, mono_s: f64, song_s: f64) {
        self.state = ClockState::Playing {
            anchor_mono_s: mono_s,
            anchor_song_s: song_s,
        };
    }

    /// Pause, holding the current song time.
    pub fn pause(&mut self, mono_s: f64) {
        if let Some(song_s) = self.song_time(mono_s) {
            self.state = ClockState::Paused { song_s };
        }
    }

    /// Resume from pause, re-anchoring at `mono_s`.
    pub fn resume(&mut self, mono_s: f64) {
        if let ClockState::Paused { song_s } = self.state {
            self.start(mono_s, song_s);
        }
    }

    /// Jump to a new song time (keeps the playing/paused state).
    pub fn seek(&mut self, mono_s: f64, song_s: f64) {
        match self.state {
            ClockState::Stopped => {}
            ClockState::Paused { .. } => self.state = ClockState::Paused { song_s },
            ClockState::Playing { .. } => self.start(mono_s, song_s),
        }
    }

    /// Stop the clock entirely.
    pub fn stop(&mut self) {
        self.state = ClockState::Stopped;
    }

    /// The song time at monotonic time `mono_s`, if a song is loaded.
    #[must_use]
    pub fn song_time(&self, mono_s: f64) -> Option<f64> {
        match self.state {
            ClockState::Stopped => None,
            ClockState::Paused { song_s } => Some(song_s),
            ClockState::Playing {
                anchor_mono_s,
                anchor_song_s,
            } => Some(anchor_song_s + (mono_s - anchor_mono_s)),
        }
    }

    /// Whether the clock is advancing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        matches!(self.state, ClockState::Playing { .. })
    }

    /// Reconcile against the playback position reported by the audio
    /// device. Large drift (≥ [`SNAP_THRESHOLD_S`]) snaps the anchor;
    /// small drift is corrected by [`SLEW_FACTOR`] per call, keeping
    /// the timeline visually smooth. Returns the applied correction.
    pub fn reconcile(&mut self, mono_s: f64, reported_song_s: f64) -> f64 {
        let ClockState::Playing {
            anchor_mono_s,
            anchor_song_s,
        } = self.state
        else {
            return 0.0;
        };
        let ours = anchor_song_s + (mono_s - anchor_mono_s);
        let drift = reported_song_s - ours;
        let correction = if drift.abs() >= SNAP_THRESHOLD_S {
            drift
        } else {
            drift * SLEW_FACTOR
        };
        self.state = ClockState::Playing {
            anchor_mono_s,
            anchor_song_s: anchor_song_s + correction,
        };
        correction
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn stopped_clock_has_no_time() {
        assert_eq!(SongClock::new().song_time(123.0), None);
    }

    #[test]
    fn playing_clock_advances_with_monotonic_time() {
        let mut clock = SongClock::new();
        clock.start(100.0, 0.0);
        assert!((clock.song_time(100.0).unwrap() - 0.0).abs() < EPS);
        assert!((clock.song_time(101.5).unwrap() - 1.5).abs() < EPS);
    }

    #[test]
    fn pause_holds_and_resume_continues() {
        let mut clock = SongClock::new();
        clock.start(100.0, 10.0);
        clock.pause(102.0); // song time 12.0
        assert!((clock.song_time(150.0).unwrap() - 12.0).abs() < EPS);
        clock.resume(200.0);
        assert!((clock.song_time(201.0).unwrap() - 13.0).abs() < EPS);
    }

    #[test]
    fn seek_jumps_in_both_states() {
        let mut clock = SongClock::new();
        clock.start(0.0, 0.0);
        clock.seek(10.0, 60.0);
        assert!((clock.song_time(11.0).unwrap() - 61.0).abs() < EPS);
        clock.pause(11.0);
        clock.seek(12.0, 30.0);
        assert!((clock.song_time(20.0).unwrap() - 30.0).abs() < EPS);
    }

    #[test]
    fn large_drift_snaps() {
        let mut clock = SongClock::new();
        clock.start(0.0, 0.0);
        // Device says we're at 1.0s when we think 2.0s: snap.
        let correction = clock.reconcile(2.0, 1.0);
        assert!((correction - (-1.0)).abs() < EPS);
        assert!((clock.song_time(2.0).unwrap() - 1.0).abs() < EPS);
    }

    #[test]
    fn small_drift_slews_gradually() {
        let mut clock = SongClock::new();
        clock.start(0.0, 0.0);
        // 10 ms drift: corrected 10% per reconcile.
        let correction = clock.reconcile(1.0, 1.010);
        assert!((correction - 0.001).abs() < EPS);
        let t = clock.song_time(1.0).unwrap();
        assert!((t - 1.001).abs() < EPS);

        // Repeated reconciliation converges toward the reported time.
        for _ in 0..100 {
            clock.reconcile(1.0, 1.010);
        }
        assert!((clock.song_time(1.0).unwrap() - 1.010).abs() < 1e-4);
    }

    #[test]
    fn reconcile_ignores_paused_and_stopped() {
        let mut clock = SongClock::new();
        assert_eq!(clock.reconcile(0.0, 5.0), 0.0);
        clock.start(0.0, 0.0);
        clock.pause(1.0);
        assert_eq!(clock.reconcile(2.0, 9.0), 0.0);
        assert!((clock.song_time(2.0).unwrap() - 1.0).abs() < EPS);
    }
}
