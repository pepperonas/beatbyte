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
    /// Note count per difficulty, parallel to `difficulties`.
    pub note_counts: Vec<usize>,
    /// Musical genre, when known (chart field, else the audio file's
    /// own tag).
    pub genre: Option<String>,
    /// The hook: where a browser preview should start, when the
    /// chart carries one (the generator stores the loudest ten
    /// seconds). `None` for a chart written before the field, or one
    /// whose analysis found nothing.
    pub preview_start_s: Option<f64>,
    /// Where the audio lives.
    pub source: SongSource,
    /// Whether karaoke lyrics sit beside this song.
    ///
    /// Existence only — a stat, not a parse: the browser rebuilds
    /// its rows on every view change and reading fifty lyric files
    /// to draw fifty markers would be work for nothing. A file that
    /// turns out unparseable simply sings nothing; the marker is a
    /// promise about the file, not about its contents.
    pub has_lyrics: bool,
}

impl SongEntry {
    /// Notes in the given difficulty's chart, if it exists.
    #[must_use]
    pub fn note_count(&self, difficulty: Difficulty) -> Option<usize> {
        self.difficulties
            .iter()
            .position(|d| *d == difficulty)
            .and_then(|i| self.note_counts.get(i).copied())
    }

    /// A 1-5 challenge rating for the given difficulty, from note
    /// density. `None` when the chart or the duration is missing.
    #[must_use]
    pub fn rating(&self, difficulty: Difficulty) -> Option<u8> {
        let notes = self.note_count(difficulty)?;
        let duration = self.duration_s?;
        Some(density_rating(notes, duration))
    }
}

/// Notes-per-second folded into a 1-5 challenge rating.
///
/// The bands were read off the real library, not invented: medium
/// charts there run 1.2-2.5 notes/second and should span the middle
/// of the scale, an expert chart at 4+ should peg it.
#[must_use]
pub fn density_rating(notes: usize, duration_s: f64) -> u8 {
    if duration_s <= 0.0 {
        return 1;
    }
    let nps = notes as f64 / duration_s;
    match nps {
        n if n < 0.9 => 1,
        n if n < 1.6 => 2,
        n if n < 2.4 => 3,
        n if n < 3.4 => 4,
        _ => 5,
    }
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

/// Every directory songs are read from, in scan order.
///
/// ONE definition: the scan walks these, and the settings screen
/// shows them. Two lists would drift, and the screen would then be
/// telling the player about a folder the game does not read.
#[must_use]
pub fn scan_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from(SONGS_DIR)];
    if let Some(user_dir) = user_songs_dir() {
        roots.push(user_dir);
    }
    roots
}

/// The scan roots that exist on disk, as absolute paths — what the
/// settings screen shows. A root that is not there delivers no
/// tracks and naming it would only puzzle the reader.
#[must_use]
pub fn live_scan_roots() -> Vec<PathBuf> {
    scan_roots()
        .into_iter()
        .filter(|root| root.is_dir())
        .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
        .collect()
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
            preview_start_s: chart.song.preview_start_s,
            bpm: chart.song.bpm,
            duration_s: chart.song.duration_s,
            difficulties: chart.charts.iter().map(|c| c.difficulty).collect(),
            note_counts: chart.charts.iter().map(|c| c.notes.len()).collect(),
            genre: chart.song.genre.clone(),
            source: SongSource::Builtin(index),
            // Filled in by the caller: only the game knows which
            // built-in carries lyrics, and it is loaded, not scanned.
            has_lyrics: false,
        })
        .collect();
    let builtin_count = entries.len();

    let roots = scan_roots();
    // Title+artist dedupe across ALL scan roots: the same song can
    // legitimately exist twice on disk (imported in the repo folder
    // AND in the user songs dir — exactly what put "Girls Just Want
    // to Have Fun" twice into the browser). First find wins.
    let mut seen: std::collections::HashSet<(String, String)> = builtins
        .iter()
        .map(|b| (b.song.title.to_lowercase(), b.song.artist.to_lowercase()))
        .collect();
    let found: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| find_chart_files(root))
        .collect();
    // Every version file, not only the active one: a pointer revert
    // must land on a chart that is on the same timeline as the
    // audio it plays against.
    migrate_chart_timelines(&found, &migration_backup_dir());
    let candidates = select_active_versions(found);
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

/// Where the one-time migration keeps the original of every chart it
/// rewrites: beside the settings, under `migrations/audio-trim/`.
fn migration_backup_dir() -> Option<PathBuf> {
    crate::config::settings_path().and_then(|settings| {
        settings
            .parent()
            .map(|dir| dir.join("migrations").join("audio-trim"))
    })
}

/// What happened to one chart file in [`migrate_chart_timelines`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// Already on the trimmed timeline: nothing to do.
    Current,
    /// Times moved earlier by the priming and the file rewritten.
    Moved,
    /// No priming declared: marked, nothing moved.
    Marked,
    /// Left alone — the audio could not be probed, the file could
    /// not be parsed, or the write failed. Reported, never fatal.
    Skipped,
}

/// Move every chart from before the priming skip (v0.14.9 and
/// earlier, no `audio_trim`) onto the trimmed timeline its audio now
/// plays on. Idempotent: a marked file is never touched again.
///
/// Rewrites the user's files, so: the original is copied to
/// `backup_dir/<its absolute path>` first (once — an existing backup
/// is never overwritten), the new text goes to a sibling temp file and
/// is renamed into place, and a chart whose audio cannot be probed is
/// left exactly as it was.
pub fn migrate_chart_timelines(files: &[PathBuf], backup_dir: &Option<PathBuf>) -> Vec<Migration> {
    let outcomes: Vec<Migration> = files
        .iter()
        .map(|path| migrate_one(path, backup_dir.as_deref()))
        .collect();
    let moved = outcomes.iter().filter(|m| **m == Migration::Moved).count();
    let marked = outcomes.iter().filter(|m| **m == Migration::Marked).count();
    let skipped = outcomes
        .iter()
        .filter(|m| **m == Migration::Skipped)
        .count();
    if moved + marked > 0 {
        info!(
            "audio-trim migration: {moved} chart file(s) moved onto the trimmed timeline, \
             {marked} marked (no priming), {skipped} left alone"
        );
    }
    outcomes
}

/// A file's absolute path as a relative one, to mirror it under a
/// backup directory: `/a/b/chart.json` → `a/b/chart.json`.
fn mirror_path(path: &std::path::Path) -> PathBuf {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    absolute
        .components()
        .filter(|c| {
            !matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        })
        .collect()
}

fn migrate_one(chart_path: &std::path::Path, backup_dir: Option<&std::path::Path>) -> Migration {
    let Ok(mut chart) = load_chart_file(chart_path) else {
        return Migration::Skipped;
    };
    if chart.audio_trim.is_some() {
        return Migration::Current;
    }
    let Some(dir) = chart_path.parent() else {
        return Migration::Skipped;
    };
    let Ok(audio_path) = resolve_audio_path(dir, &chart.song.audio) else {
        return Migration::Skipped;
    };
    let Some(rate) = beatbyte_audio::decode::probe_sample_rate(&audio_path) else {
        return Migration::Skipped;
    };
    let priming = beatbyte_audio::priming::container_priming(&audio_path);
    let trim = beatbyte_chart::AudioTrim::declared(priming.samples, priming.timescale, rate);
    let outcome = if trim.seconds() > 0.0 {
        Migration::Moved
    } else {
        Migration::Marked
    };
    chart.retime(trim);

    // The original, kept once, under its own ABSOLUTE path: two
    // scan roots can hold a song folder of the same name (the same
    // import in the repo and in the user directory — it happened),
    // and a backup keyed by folder name alone would keep only the
    // first and silently rewrite the second without one.
    if let Some(backups) = backup_dir {
        let keep = backups.join(mirror_path(chart_path));
        if !keep.exists()
            && (keep
                .parent()
                .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
                || std::fs::copy(chart_path, &keep).is_err())
        {
            warn!(
                "audio-trim migration: cannot back up `{}` — left alone",
                chart_path.display()
            );
            return Migration::Skipped;
        }
    }
    // Then the new text, atomically.
    let Ok(json) = chart.to_json_pretty() else {
        return Migration::Skipped;
    };
    let temp = chart_path.with_extension("json.migrating");
    if std::fs::write(&temp, json).is_err() || std::fs::rename(&temp, chart_path).is_err() {
        let _ = std::fs::remove_file(&temp);
        warn!(
            "audio-trim migration: cannot rewrite `{}` — left alone",
            chart_path.display()
        );
        return Migration::Skipped;
    }
    if outcome == Migration::Moved {
        info!(
            "audio-trim migration: `{}` moved {:.1} ms earlier",
            chart_path.display(),
            trim.seconds() * 1000.0
        );
    }
    outcome
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
    // Genre: the chart's own field wins; a file that predates the
    // field falls back to the audio's tag (a metadata probe, cheap
    // enough for a scan; the chart is NEVER mutated implicitly -
    // hashes are identities).
    let genre = chart
        .song
        .genre
        .clone()
        .or_else(|| beatbyte_audio::read_genre(&audio_path));
    // An alignment or an `.lrc` beside the audio or the chart - the
    // same places `beatbyte_chart::lyrics::lyrics_beside` reads at
    // start.
    let has_lyrics = beatbyte_chart::lyrics::lyrics_exist_beside(&audio_path, chart_path);
    Ok(Some(SongEntry {
        title: chart.song.title.clone(),
        artist: chart.song.artist.clone(),
        bpm: chart.song.bpm,
        duration_s: chart.song.duration_s,
        difficulties: chart.charts.iter().map(|c| c.difficulty).collect(),
        note_counts: chart.charts.iter().map(|c| c.notes.len()).collect(),
        genre,
        has_lyrics,
        preview_start_s: chart.song.preview_start_s,
        source: SongSource::File {
            chart_path: chart_path.to_path_buf(),
            audio_path,
        },
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{find_chart_files, load_entry};

    #[test]
    fn a_song_is_marked_when_a_lyrics_file_sits_beside_it() {
        // Through `load_entry`, not by re-checking the file myself:
        // the first version of this test only asserted that
        // `is_file()` works, which is true no matter what the
        // scanner does with it (it survived the mutation that set
        // the flag to a constant `false`).
        let root = std::env::temp_dir().join(format!("bb-lyrmark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp dir");
        let audio = root.join("song.wav");
        std::fs::write(&audio, b"not really audio").expect("audio");
        let chart = root.join("chart.json");
        std::fs::write(&chart, MINIMAL_CHART).expect("chart");

        let without = load_entry(&chart).expect("loads").expect("an entry");
        assert!(!without.has_lyrics, "no .lrc yet, no marker");

        std::fs::write(audio.with_extension("lrc"), "[00:01.00]la").expect("lyrics");
        let with = load_entry(&chart).expect("loads").expect("an entry");
        assert!(with.has_lyrics, "a .lrc beside the audio marks the song");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The smallest chart `load_entry` accepts, pointing at
    /// `song.wav` beside it. Synthetic throughout - no real song.
    const MINIMAL_CHART: &str = r#"{
        "format_version": 1,
        "song": {
            "title": "Synthetic",
            "artist": "The Null Pointers",
            "audio": "song.wav",
            "bpm": 120.0,
            "duration_s": 60.0
        },
        "charts": [
            {
                "difficulty": "medium",
                "lanes": 5,
                "notes": [{ "time": 1.0, "lane": 0 }],
                "phrases": []
            }
        ]
    }"#;

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

#[cfg(test)]
mod rating_tests {
    use super::density_rating;

    #[test]
    fn the_bands_match_the_real_library() {
        // Calibrated on the actual imports, not invented: a sparse
        // easy chart (~0.6 nps) is a 1, the typical medium (~1.9) a
        // 3, a dense expert (4+) pegs the scale.
        assert_eq!(density_rating(150, 250.0), 1); // 0.6 nps
        assert_eq!(density_rating(300, 250.0), 2); // 1.2
        assert_eq!(density_rating(470, 250.0), 3); // 1.88
        assert_eq!(density_rating(700, 250.0), 4); // 2.8
        assert_eq!(density_rating(1100, 250.0), 5); // 4.4
    }

    #[test]
    fn degenerate_durations_do_not_divide_by_zero() {
        assert_eq!(density_rating(100, 0.0), 1);
        assert_eq!(density_rating(100, -5.0), 1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod migration_tests {
    use std::path::PathBuf;

    use beatbyte_chart::{ChartFile, load_chart_file};

    use super::{Migration, migrate_chart_timelines};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beatbyte-mig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../beatbyte-audio/tests/fixtures")
            .join(name)
    }

    /// A pre-skip chart (no `audio_trim`) over `audio`, one note at
    /// 1.0 s, one phrase 2.0..3.0.
    fn old_chart(dir: &std::path::Path, audio: &str) -> PathBuf {
        let json = format!(
            r#"{{"format_version":1,"song":{{"title":"T","artist":"A","audio":"{audio}","bpm":120.0,"offset_s":0.5}},
                "charts":[{{"difficulty":"medium","lanes":5,"notes":[{{"time":1.0,"lane":0}}],"phrases":[{{"start":2.0,"end":3.0}}]}}]}}"#
        );
        let path = dir.join("chart.json");
        std::fs::write(&path, json).expect("write chart");
        path
    }

    #[test]
    fn an_old_chart_over_an_m4a_moves_by_its_priming_once_with_a_backup() {
        let dir = scratch("m4a");
        std::fs::copy(fixture("click-ffmpeg.m4a"), dir.join("song.m4a")).expect("audio");
        let chart = old_chart(&dir, "song.m4a");
        let backups = Some(dir.join("backups"));

        let first = migrate_chart_timelines(std::slice::from_ref(&chart), &backups);
        assert_eq!(first, vec![Migration::Moved]);
        let moved = load_chart_file(&chart).expect("rewritten file parses");
        let shift = 1024.0 / 44_100.0;
        assert!((moved.charts[0].notes[0].time - (1.0 - shift)).abs() < 1e-9);
        assert!((moved.charts[0].phrases[0].start - (2.0 - shift)).abs() < 1e-9);
        assert!((moved.song.offset_s - (0.5 - shift)).abs() < 1e-9);
        assert_eq!(moved.audio_trim.map(|t| t.priming_samples), Some(1024));
        // The original is kept, byte for byte, where the settings
        // live — under the chart's own absolute path, so two song
        // folders of the same name in different roots cannot share
        // (and lose) a backup slot.
        let kept = dir.join("backups").join(super::mirror_path(&chart));
        let original = ChartFile::from_json(&std::fs::read_to_string(&kept).unwrap()).unwrap();
        assert_eq!(original.charts[0].notes[0].time, 1.0);
        assert_eq!(original.audio_trim, None);
        // No temp file left behind.
        assert!(!dir.join("chart.json.migrating").exists());

        // Second scan: nothing to do, and the file is byte-identical.
        let text_before = std::fs::read_to_string(&chart).unwrap();
        let second = migrate_chart_timelines(std::slice::from_ref(&chart), &backups);
        assert_eq!(second, vec![Migration::Current]);
        assert_eq!(std::fs::read_to_string(&chart).unwrap(), text_before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_song_folders_of_the_same_name_both_keep_their_original() {
        // The flaw the first run had: the repo and the user directory
        // held the same import, the backup was keyed by folder name,
        // and the second original was rewritten without one.
        let base = scratch("twins");
        let backups = Some(base.join("backups"));
        let mut charts = Vec::new();
        for root in ["repo", "user"] {
            let dir = base.join(root).join("same-song");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(fixture("click-ffmpeg.m4a"), dir.join("song.m4a")).unwrap();
            charts.push(old_chart(&dir, "song.m4a"));
        }
        let outcomes = migrate_chart_timelines(&charts, &backups);
        assert_eq!(outcomes, vec![Migration::Moved, Migration::Moved]);
        for chart in &charts {
            let kept = base.join("backups").join(super::mirror_path(chart));
            assert!(kept.is_file(), "no backup for {}", chart.display());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_chart_over_audio_without_priming_is_marked_and_not_moved() {
        let dir = scratch("wav");
        std::fs::copy(fixture("tone.wav"), dir.join("song.wav")).expect("audio");
        let chart = old_chart(&dir, "song.wav");
        assert_eq!(
            migrate_chart_timelines(std::slice::from_ref(&chart), &None),
            vec![Migration::Marked]
        );
        let marked = load_chart_file(&chart).expect("parses");
        assert_eq!(marked.charts[0].notes[0].time, 1.0, "nothing moved");
        assert_eq!(marked.audio_trim.map(|t| t.priming_samples), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chart_whose_audio_is_missing_is_left_exactly_as_it_was() {
        // Nothing can be probed, so nothing is guessed: the file stays
        // untouched and comes up again next scan.
        let dir = scratch("missing");
        let chart = old_chart(&dir, "gone.m4a");
        let before = std::fs::read_to_string(&chart).unwrap();
        assert_eq!(
            migrate_chart_timelines(std::slice::from_ref(&chart), &None),
            vec![Migration::Skipped]
        );
        assert_eq!(std::fs::read_to_string(&chart).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
