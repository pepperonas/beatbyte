//! The song library: the built-in songs plus every valid chart found
//! in the `songs/` directory.
//!
//! Charts are untrusted input — anything that fails validation is
//! skipped with a log line, never a crash. Audio references resolve
//! through the chart crate's traversal-safe path logic.

use std::path::PathBuf;

use beatbyte_chart::{ChartFile, Severity, load_chart_file, resolve_audio_path};
use beatbyte_core::Difficulty;
use bevy::prelude::*;

/// Where a song's audio comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum SongSource {
    /// A synthesized in-memory built-in song (index into
    /// [`crate::boot::BuiltinSongs`]).
    Builtin(usize),
    /// A chart file on disk with its resolved audio path.
    File {
        /// The chart JSON.
        chart_path: PathBuf,
        /// The resolved audio file.
        audio_path: PathBuf,
    },
}

/// One entry in the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct SongEntry {
    /// Song title.
    pub title: String,
    /// Artist.
    pub artist: String,
    /// Tempo.
    pub bpm: f64,
    /// Duration in seconds, when known.
    pub duration_s: Option<f64>,
    /// Difficulties present in the chart.
    pub difficulties: Vec<Difficulty>,
    /// Where the audio lives.
    pub source: SongSource,
}

/// The scanned library.
#[derive(Resource, Debug, Clone, Default)]
pub struct SongLibrary {
    /// All playable entries; the built-in songs always come first.
    pub entries: Vec<SongEntry>,
}

/// Directory scanned for charts, relative to the working directory
/// (development / portable layouts).
pub const SONGS_DIR: &str = "songs";

/// The user-level songs directory (installed layouts).
#[must_use]
pub fn user_songs_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("songs"))
}

/// Build the library: built-in songs first (in the given order), then
/// every valid chart under [`SONGS_DIR`] and the user songs directory
/// (one level of subdirectories, plus loose files).
#[must_use]
pub fn scan_library(builtins: &[ChartFile]) -> SongLibrary {
    let mut entries: Vec<SongEntry> = builtins
        .iter()
        .enumerate()
        .map(|(index, chart)| SongEntry {
            title: chart.song.title.clone(),
            artist: chart.song.artist.clone(),
            bpm: chart.song.bpm,
            duration_s: chart.song.duration_s,
            difficulties: chart.charts.iter().map(|c| c.difficulty).collect(),
            source: SongSource::Builtin(index),
        })
        .collect();
    let builtin_count = entries.len();

    let mut roots = vec![PathBuf::from(SONGS_DIR)];
    if let Some(user_dir) = user_songs_dir() {
        roots.push(user_dir);
    }
    for chart_path in roots.iter().flat_map(|root| find_chart_files(root)) {
        match load_entry(&chart_path) {
            Ok(Some(entry)) => {
                // The CLI `demo` command writes built-in songs to
                // disk; don't list one twice.
                if builtins
                    .iter()
                    .any(|b| entry.title == b.song.title && entry.artist == b.song.artist)
                {
                    continue;
                }
                entries.push(entry);
            }
            Ok(None) => {}
            Err(reason) => warn!("skipping `{}`: {reason}", chart_path.display()),
        }
    }
    // Built-ins stay first; the rest sorts by title.
    entries[builtin_count..].sort_by_key(|entry| entry.title.to_lowercase());
    SongLibrary { entries }
}

/// All `*.json` files in `dir` and its immediate subdirectories.
fn find_chart_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(top) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in top.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            found.push(path);
        } else if path.is_dir()
            && let Ok(sub) = std::fs::read_dir(&path)
        {
            for file in sub.flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "json") {
                    found.push(path);
                }
            }
        }
    }
    found.sort();
    found
}

/// Load and validate one chart into a library entry.
/// `Ok(None)` = not a chart file at all (ignored silently).
fn load_entry(chart_path: &std::path::Path) -> Result<Option<SongEntry>, String> {
    let chart = match load_chart_file(chart_path) {
        Ok(chart) => chart,
        Err(beatbyte_chart::ChartIoError::Parse { .. }) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let issues = chart.validate();
    if let Some(worst) = issues.iter().find(|i| i.severity == Severity::Error) {
        return Err(format!("invalid chart: {worst}"));
    }
    let chart_dir = chart_path
        .parent()
        .ok_or_else(|| "chart has no parent directory".to_owned())?;
    let audio_path = resolve_audio_path(chart_dir, &chart.song.audio).map_err(|e| e.to_string())?;
    if !audio_path.exists() {
        return Err(format!("audio file `{}` not found", audio_path.display()));
    }
    Ok(Some(SongEntry {
        title: chart.song.title.clone(),
        artist: chart.song.artist.clone(),
        bpm: chart.song.bpm,
        duration_s: chart.song.duration_s,
        difficulties: chart.charts.iter().map(|c| c.difficulty).collect(),
        source: SongSource::File {
            chart_path: chart_path.to_path_buf(),
            audio_path,
        },
    }))
}
