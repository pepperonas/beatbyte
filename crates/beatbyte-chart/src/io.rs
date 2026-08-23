//! Loading and saving chart files, with untrusted-input guards.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::schema::ChartFile;

/// Maximum accepted chart file size in bytes (guards against memory
/// bombs; a huge legitimate chart is ~10 MB of JSON).
pub const MAX_CHART_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Errors reading or writing chart files.
#[derive(Debug, Error)]
pub enum ChartIoError {
    /// Filesystem error.
    #[error("io error on `{path}`: {source}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file exceeds [`MAX_CHART_FILE_BYTES`].
    #[error("`{path}` is {size} bytes, larger than the {limit} byte limit")]
    TooLarge {
        /// The file involved.
        path: PathBuf,
        /// Its size.
        size: u64,
        /// The limit.
        limit: u64,
    },
    /// The JSON did not parse as a chart.
    #[error("`{path}` is not a valid chart file: {source}")]
    Parse {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: serde_json::Error,
    },
    /// The chart's audio reference escapes the chart directory.
    #[error("audio path `{audio}` is not allowed: {reason}")]
    UnsafeAudioPath {
        /// The offending reference.
        audio: String,
        /// Why it was rejected.
        reason: String,
    },
}

/// Load and parse a chart file. This performs **no** semantic
/// validation — call [`ChartFile::validate`] on the result.
pub fn load_chart_file(path: &Path) -> Result<ChartFile, ChartIoError> {
    let io_err = |source| ChartIoError::Io {
        path: path.to_path_buf(),
        source,
    };
    let metadata = fs::metadata(path).map_err(io_err)?;
    if metadata.len() > MAX_CHART_FILE_BYTES {
        return Err(ChartIoError::TooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
            limit: MAX_CHART_FILE_BYTES,
        });
    }
    let mut text = String::with_capacity(metadata.len() as usize);
    fs::File::open(path)
        .map_err(io_err)?
        // Belt and braces: cap the read even if the file grew after stat.
        .take(MAX_CHART_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(io_err)?;
    ChartFile::from_json(&text).map_err(|source| ChartIoError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Serialize and write a chart file as pretty JSON.
pub fn save_chart_file(path: &Path, chart: &ChartFile) -> Result<(), ChartIoError> {
    let json = chart
        .to_json_pretty()
        .map_err(|source| ChartIoError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    fs::write(path, json).map_err(|source| ChartIoError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Resolve the chart's audio reference against the chart's directory,
/// refusing anything that escapes it (path traversal, absolute paths).
pub fn resolve_audio_path(chart_dir: &Path, audio: &str) -> Result<PathBuf, ChartIoError> {
    let unsafe_path = |reason: &str| ChartIoError::UnsafeAudioPath {
        audio: audio.to_owned(),
        reason: reason.to_owned(),
    };
    if audio.trim().is_empty() {
        return Err(unsafe_path("empty path"));
    }
    let normalized = audio.replace('\\', "/");
    let relative = Path::new(&normalized);
    if relative.is_absolute() || normalized.starts_with('/') {
        return Err(unsafe_path("absolute paths are not allowed"));
    }
    // Windows drive letters (`C:`) are not `Prefix` components on Unix,
    // so catch them (and URL-like schemes) explicitly.
    if normalized.contains(':') {
        return Err(unsafe_path("`:` is not allowed in audio paths"));
    }
    for component in relative.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            _ => return Err(unsafe_path("path must stay inside the chart directory")),
        }
    }
    Ok(chart_dir.join(relative))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beatbyte-chart-test-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn minimal_chart_json() -> &'static str {
        r#"{
            "format_version": 1,
            "song": { "title": "T", "audio": "a.ogg", "bpm": 120.0 },
            "charts": [ { "difficulty": "easy", "lanes": 5, "notes": [] } ]
        }"#
    }

    #[test]
    fn load_save_round_trip() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join("chart.json");
        fs::write(&path, minimal_chart_json()).unwrap();

        let chart = load_chart_file(&path).unwrap();
        let out = dir.join("out.json");
        save_chart_file(&out, &chart).unwrap();
        let back = load_chart_file(&out).unwrap();
        assert_eq!(chart, back);
    }

    #[test]
    fn malformed_json_is_a_parse_error_not_a_panic() {
        let dir = scratch_dir("malformed");
        let path = dir.join("bad.json");
        fs::write(&path, "{ definitely not json").unwrap();
        assert!(matches!(
            load_chart_file(&path),
            Err(ChartIoError::Parse { .. })
        ));
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let dir = scratch_dir("missing");
        assert!(matches!(
            load_chart_file(&dir.join("nope.json")),
            Err(ChartIoError::Io { .. })
        ));
    }

    #[test]
    fn audio_path_resolution_stays_inside_the_chart_dir() {
        let dir = Path::new("/songs/mysong");
        let ok = resolve_audio_path(dir, "audio.ogg").unwrap();
        assert_eq!(ok, dir.join("audio.ogg"));
        let ok = resolve_audio_path(dir, "media/audio.ogg").unwrap();
        assert_eq!(ok, dir.join("media/audio.ogg"));

        for bad in [
            "../outside.ogg",
            "/etc/passwd",
            "a/../../b.ogg",
            "..\\x.ogg",
        ] {
            assert!(
                resolve_audio_path(dir, bad).is_err(),
                "`{bad}` must be rejected"
            );
        }
    }
}
