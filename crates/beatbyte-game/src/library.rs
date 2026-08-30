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
    // Title+artist dedupe across ALL scan roots: the same song can
    // legitimately exist twice on disk (imported in the repo folder
    // AND in the user songs dir — exactly what put "Girls Just Want
    // to Have Fun" twice into the browser). First find wins.
    let mut seen: std::collections::HashSet<(String, String)> = builtins
        .iter()
        .map(|b| (b.song.title.to_lowercase(), b.song.artist.to_lowercase()))
        .collect();
    let candidates = select_active_versions(
        roots
            .iter()
            .flat_map(|root| find_chart_files(root))
            .collect(),
    );
    for chart_path in candidates {
        match load_entry(&chart_path) {
            Ok(Some(entry)) => {
                let key = (entry.title.to_lowercase(), entry.artist.to_lowercase());
                if !seen.insert(key) {
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

/// Delete a file-based song's files. An entry whose chart lives in a
/// folder under an `imported/` directory owns that folder (the import
/// created it) — the whole folder goes, audio included. Anything else
/// is hand-managed: only the chart file is removed, the audio stays.
pub fn remove_song_files(chart_path: &std::path::Path) -> Result<(), String> {
    let parent = chart_path
        .parent()
        .ok_or_else(|| "chart has no parent directory".to_owned())?;
    let parent_is_import_child = parent
        .parent()
        .and_then(|grand| grand.file_name())
        .is_some_and(|name| name == "imported");
    if parent_is_import_child {
        std::fs::remove_dir_all(parent).map_err(|e| format!("cannot remove folder: {e}"))
    } else {
        std::fs::remove_file(chart_path).map_err(|e| format!("cannot remove chart: {e}"))
    }
}

/// Reduce a flat scan to the files worth loading, version-aware
/// (ADR-0011): per folder, chart VERSION files (`chart.v<N>.json`)
/// are dropped unless the folder's pointer names one — and when it
/// does, the base `chart.json` it supersedes is dropped instead. The
/// pointer file itself is never a chart. Folders without versions
/// pass through untouched, which keeps hand-managed folders with
/// several charts side by side (the builtins) working.
fn select_active_versions(files: Vec<PathBuf>) -> Vec<PathBuf> {
    use beatbyte_chart::versions;
    let name_of = |path: &std::path::Path| -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    // Active name per folder that HAS versioned files.
    let mut active: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();
    for path in &files {
        let name = name_of(path);
        if !versions::is_version_file(&name) {
            continue;
        }
        let Some(dir) = path.parent() else { continue };
        if active.contains_key(dir) {
            continue;
        }
        let siblings: Vec<String> = files
            .iter()
            .filter(|f| f.parent() == Some(dir))
            .map(|f| name_of(f))
            .collect();
        let pointer = std::fs::read_to_string(dir.join(versions::POINTER_FILE)).ok();
        active.insert(
            dir.to_path_buf(),
            versions::resolve_active(pointer.as_deref(), &siblings),
        );
    }
    files
        .into_iter()
        .filter(|path| {
            let name = name_of(path);
            if name == versions::POINTER_FILE {
                return false;
            }
            let Some(chosen) = path.parent().and_then(|dir| active.get(dir)) else {
                // No versions in this folder: everything stays.
                return true;
            };
            // A versioned folder loads exactly its active chart; the
            // superseded base and the passed-over versions are real
            // files that stay on disk, they just do not become songs.
            if versions::is_version_file(&name) || name == versions::BASE_CHART {
                return name == *chosen;
            }
            true
        })
        .collect()
}

/// All `*.json` files under `dir`, up to two directory levels below
/// it — `songs/imported/<song>/chart.json` is the deepest documented
/// layout, and the one-level scan this replaces silently ignored it
/// (found during the import-walkthrough validation). Symlinked
/// directories are not followed: a song tree is untrusted input and
/// must not walk out of its root or cycle.
fn find_chart_files(dir: &std::path::Path) -> Vec<PathBuf> {
    fn walk(dir: &std::path::Path, depth_left: u8, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            // `DirEntry::file_type` does not follow symlinks, so a
            // symlinked directory reports as symlink and is skipped.
            if file_type.is_file() && path.extension().is_some_and(|e| e == "json") {
                found.push(path);
            } else if file_type.is_dir() && depth_left > 0 {
                walk(&path, depth_left - 1, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, 2, &mut found);
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::find_chart_files;

    #[test]
    fn scan_reaches_two_levels_and_skips_symlinks_and_deeper() {
        let root = std::env::temp_dir().join(format!("beatbyte-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("imported/my-song")).unwrap();
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("top.json"), "{}").unwrap();
        std::fs::write(root.join("a/one-deep.json"), "{}").unwrap();
        std::fs::write(root.join("imported/my-song/two-deep.json"), "{}").unwrap();
        std::fs::write(root.join("a/b/c/three-deep.json"), "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();

        let found = find_chart_files(&root);
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"top.json".to_owned()));
        assert!(names.contains(&"one-deep.json".to_owned()));
        assert!(
            names.contains(&"two-deep.json".to_owned()),
            "songs/imported/<song>/ layout must be found, got {names:?}"
        );
        assert!(
            !names.contains(&"three-deep.json".to_owned()),
            "the walk must stay bounded"
        );
        assert_eq!(found.len(), 3, "symlink must not add duplicates: {names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod remove_tests {
    use super::remove_song_files;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("beatbyte-rm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn imported_songs_lose_their_whole_folder() {
        let root = scratch("imported");
        let folder = root.join("songs/imported/my-song");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("chart.json"), "{}").unwrap();
        std::fs::write(folder.join("my-song.mp3"), "x").unwrap();
        remove_song_files(&folder.join("chart.json")).unwrap();
        assert!(!folder.exists(), "the import's folder must be gone");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hand_managed_charts_lose_only_the_chart() {
        let root = scratch("manual");
        let folder = root.join("songs/my-collection");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("song.chart.json"), "{}").unwrap();
        std::fs::write(folder.join("song.ogg"), "x").unwrap();
        remove_song_files(&folder.join("song.chart.json")).unwrap();
        assert!(!folder.join("song.chart.json").exists());
        assert!(
            folder.join("song.ogg").exists(),
            "hand-managed audio must survive"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_files_error_cleanly() {
        assert!(remove_song_files(std::path::Path::new("/nonexistent/chart.json")).is_err());
    }
}

#[cfg(test)]
mod version_scan_tests {
    use super::select_active_versions;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beatbyte-vs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn touch(path: &PathBuf) {
        std::fs::write(path, "{}").expect("write");
    }

    #[test]
    fn a_pointed_at_version_replaces_the_base_chart() {
        let dir = scratch("pointed");
        let base = dir.join("chart.json");
        let v2 = dir.join("chart.v2.json");
        touch(&base);
        touch(&v2);
        std::fs::write(
            dir.join("chart-active.json"),
            "{\"active\":\"chart.v2.json\"}",
        )
        .expect("pointer");
        let kept = select_active_versions(vec![
            base.clone(),
            v2.clone(),
            dir.join("chart-active.json"),
        ]);
        // Exactly ONE chart survives — the browser must show one
        // entry per song, and it is the version the pointer names.
        assert_eq!(kept, vec![v2]);
    }

    #[test]
    fn an_unpointed_version_file_is_not_a_second_song() {
        // The defect this exists for: the flat scan loads every
        // *.json, so a sibling version file would have appeared as a
        // second song (deduped only by luck of scan order).
        let dir = scratch("unpointed");
        let base = dir.join("chart.json");
        let v2 = dir.join("chart.v2.json");
        touch(&base);
        touch(&v2);
        let kept = select_active_versions(vec![base.clone(), v2]);
        assert_eq!(kept, vec![base], "without a pointer the original plays");
    }

    #[test]
    fn a_broken_pointer_still_shows_the_original() {
        let dir = scratch("broken");
        let base = dir.join("chart.json");
        let v2 = dir.join("chart.v2.json");
        touch(&base);
        touch(&v2);
        std::fs::write(dir.join("chart-active.json"), "not json").expect("pointer");
        let kept = select_active_versions(vec![base.clone(), v2, dir.join("chart-active.json")]);
        assert_eq!(kept, vec![base], "a bad pointer must not lose the song");
    }

    #[test]
    fn folders_without_versions_pass_through_untouched() {
        // The builtins keep several charts side by side in ONE
        // folder; version logic must not reduce them to one.
        let dir = scratch("builtin");
        let a = dir.join("circuit-breaker.chart.json");
        let b = dir.join("solder-groove.chart.json");
        touch(&a);
        touch(&b);
        let kept = select_active_versions(vec![a.clone(), b.clone()]);
        assert_eq!(kept, vec![a, b]);
    }
}
