//! # beatbyte-chart
//!
//! The versioned BeatByte chart file format: schema, serialization,
//! validation and conversion into playable [`beatbyte_core`] tracks.
//!
//! Charts are **untrusted input** — everything loaded through this crate
//! is validated before it reaches gameplay: version gates, numeric
//! ranges, note counts, path traversal in the audio reference, and
//! structural rules (duplicate notes, overlapping phrases).
//!
//! The format is JSON with an explicit `format_version` field; the
//! specification lives in `docs/chart-format/` in the repository.
//!
//! ## Typical flow
//!
//! ```
//! use beatbyte_chart::{ChartFile, Severity};
//!
//! let json = r#"{
//!   "format_version": 1,
//!   "song": { "title": "Demo", "artist": "Nobody", "audio": "demo.ogg", "bpm": 120.0 },
//!   "charts": [
//!     { "difficulty": "easy", "lanes": 5,
//!       "notes": [ { "time": 1.0, "lane": 0 } ] }
//!   ]
//! }"#;
//!
//! let chart = ChartFile::from_json(json).expect("parses");
//! let issues = chart.validate();
//! assert!(!issues.iter().any(|i| i.severity == Severity::Error));
//!
//! let track = chart
//!     .to_track(beatbyte_core::Difficulty::Easy)
//!     .expect("difficulty exists and converts");
//! assert_eq!(track.len(), 1);
//! ```

pub mod convert;
pub mod io;
pub mod schema;
pub mod validate;

pub use convert::ConvertError;
pub use io::{ChartIoError, load_chart_file, resolve_audio_path, save_chart_file};
pub use schema::{ChartDef, ChartFile, ChartNote, ChartPhrase, SongMeta};
pub use validate::{Issue, Severity};

/// The chart format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// Hard cap on notes per difficulty chart (untrusted input guard).
pub const MAX_NOTES_PER_CHART: usize = 100_000;

/// Hard cap on the song timeline in seconds (2 hours).
pub const MAX_SONG_LENGTH_S: f64 = 7_200.0;

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
