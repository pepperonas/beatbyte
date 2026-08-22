//! # beatbyte-audio
//!
//! Audio infrastructure for BeatByte: decoding user-provided music,
//! playback with an accurate song clock, and the music-analysis pipeline
//! (BPM, beats, onsets) that feeds automatic chart generation.
//!
//! This crate is engine-free. Playback owns its own threads; analysis
//! functions are pure `samples in → events out` stages so they can be
//! tested deterministically.
//!
//! Implemented in Milestone 3.

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
