//! # beatbyte-editor
//!
//! The BeatByte chart editor. This crate reserves the architectural slot
//! for the editor (Milestone 11): waveform view, beat grid, note
//! placement/editing, playback scrubbing and validation.
//!
//! It is intentionally empty until the chart model (`beatbyte-chart`) has
//! stabilized through real gameplay use — building an editor on a moving
//! format would be wasted work. The chart data model is designed with the
//! editor in mind (see ADR-0002 and `docs/chart-format/`).

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
