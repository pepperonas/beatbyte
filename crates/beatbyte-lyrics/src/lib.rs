//! # beatbyte-lyrics
//!
//! Word- and character-level karaoke timing for a song the player
//! owns, from lyric text the player already has: the known text is
//! **force-aligned** against the song's own audio (plan
//! `docs/plans/ai-song-graph-upgrade.md`, Track L). Not transcription
//! — the words are given; only *where* they are is asked, which is a
//! far smaller question and one a constrained decoder cannot
//! hallucinate through.
//!
//! - [`transcript`] — lyric text as it arrives → the model's letters,
//!   by word and line
//! - [`ctc`] — the Viterbi forced alignment over emissions, pure
//! - [`emissions`] — windows of 16 kHz audio through the acoustic
//!   model (via `beatbyte-ml`), stitched into one emission matrix
//! - [`words`] — the `words.json` schema and its enhanced-LRC export
//! - [`gate`](mod@gate) — confidence gating and fallback: a result
//!   never ships worse than the line-level lyrics the player had
//! - [`job`] — one song start to finish, with progress and cancel:
//!   the path the CLI and the game share
//! - [`align`](mod@align) — the whole pipeline, audio in, alignment out
//!
//! Everything a result depends on is recorded in it: the audio's
//! hash, the model's hash, the runtime fingerprint and a pipeline
//! version. A cached alignment never changes silently under the
//! player.

pub mod align;
pub mod ctc;
pub mod emissions;
pub mod gate;
pub mod job;
pub mod transcript;
pub mod words;

pub use align::{AlignOutcome, align};
pub use gate::{GateConfig, GateReport, Verdict, gate};
pub use job::{JobError, JobProgress, JobStage, Summary, align_file};
pub use transcript::Transcript;
pub use words::{AlignedLine, AlignedWord, Alignment};

/// Bumped whenever the pipeline's output for the same audio, text
/// and model would change — a cached `words.json` with another
/// version is recomputed rather than trusted.
pub const PIPELINE_VERSION: u32 = 1;
