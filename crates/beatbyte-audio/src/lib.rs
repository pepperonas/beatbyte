//! # beatbyte-audio
//!
//! Audio infrastructure for BeatByte: decoding user-provided music,
//! playback with an accurate song clock, and the music-analysis
//! pipeline (BPM, beats, onsets, energy) that feeds automatic chart
//! generation.
//!
//! This crate is engine-free. Playback owns the audio device threads
//! (rodio); everything else is pure and deterministic:
//!
//! - [`decode`] — audio file → mono sample buffer (untrusted input)
//! - [`analysis`] — samples → [`beatbyte_core::music::SongAnalysis`]
//! - [`clock`] — the authoritative, testable song timeline
//! - [`playback`] — rodio wrapper (play/pause/seek/position)
//! - [`synth`] — deterministic signal synthesis (tests, demo material)
//! - [`demo`] — the original, fully synthesized bundled demo song
//!
//! Architecture: see ADR-0005 and `docs/audio/analysis.md`.

pub mod analysis;
pub mod clock;
pub mod decode;
pub mod demo;
pub mod eval;
pub mod playback;
pub mod synth;

pub use analysis::{Analyzer, AnalyzerConfig, SpectralAnalyzer};
pub use clock::SongClock;
pub use decode::{
    AudioData, DecodeError, decode_file, read_genre, wav_bytes_mono16, write_wav_mono16,
};
pub use playback::{MusicPlayer, PlaybackError};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
