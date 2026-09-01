//! # beatbyte-core
//!
//! The engine-free domain model of BeatByte: lanes, notes, timing,
//! judgment, scoring and gameplay rules.
//!
//! This crate deliberately has **no** dependency on Bevy, audio backends
//! or any I/O. Every gameplay rule defined here is unit-testable with
//! plain values, which is what makes deterministic rhythm-game timing
//! possible (see ADR-0002).
//!
//! ## Module map
//!
//! - [`lane`] — the five fret lanes and lane sets (chords, held frets)
//! - [`difficulty`] — the four difficulty levels
//! - [`timing`] — tempo maps, hit windows, judgment
//! - [`note`] — note events, phrases, playable tracks
//! - [`score`] — scoring rules and per-player performance
//! - [`session`] — the deterministic gameplay session (judgment engine)
//! - [`music`] — analysis results (beats, onsets) shared with the
//!   audio pipeline and chart generator
//!
//! ## Time convention
//!
//! All times are `f64` **seconds on the song timeline** (`0.0` = start
//! of the audio). Milliseconds appear only at configuration boundaries.

pub mod difficulty;
pub mod history;
pub mod lane;
pub mod music;
pub mod note;
pub mod score;
pub mod session;
pub mod telemetry;
pub mod timing;

pub use difficulty::Difficulty;
pub use lane::{Lane, LaneSet};
pub use music::{MelodyNote, Onset, SongAnalysis};
pub use note::{NoteEvent, NoteKind, Phrase, Track};
pub use score::{PlayerPerformance, ScoreConfig};
pub use session::{GameInput, InputKind, SessionEvent, TrackSession};
pub use timing::{Judgment, TempoMap, TimingWindows};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_workspace_scheme() {
        // Semantic versioning: MAJOR.MINOR.PATCH
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "version component `{part}` must be numeric"
            );
        }
    }
}
