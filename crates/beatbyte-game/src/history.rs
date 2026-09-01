//! The play history: every track this installation played, as an
//! append-only log.
//!
//! Deliberately **not** the telemetry files. Those exist to judge
//! charts and therefore skip practice runs on purpose
//! (`telemetry::finalize_recording`) — but a track played at half
//! speed was still played, and a report of what was performed may
//! not have a hole in it. They are also per-session files full of
//! per-note lines; this is one short line per run.
//!
//! Deliberately not the scoreboard either: that keeps only the BEST
//! result per song and overwrites the rest, which is the opposite of
//! a history.
//!
//! **The log records facts, the export decides policy.** Every run
//! is written with its real duration and its mode flags — practice,
//! autopilot, whether it ran to the end — so a reader can filter for
//! its own purpose without the recorder having guessed. Anything
//! dropped here would be unrecoverable.

use std::io::Write as _;
use std::path::PathBuf;

pub use beatbyte_core::history::{PlayEntry, parse_log, render_entry};

/// Where the history lives — beside `scores.json`, the way every
/// other persistent file in this game does.
#[must_use]
pub fn history_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("history.jsonl"))
}

/// Append one entry to the log, creating the file and its directory
/// if needed.
///
/// Failure policy of the house: warn and carry on. A history that
/// cannot be written must never take gameplay with it.
pub fn append(entry: &PlayEntry) {
    let Some(path) = history_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        bevy::log::warn!("cannot create the history directory: {error}");
        return;
    }
    let line = match render_entry(entry) {
        Ok(line) => line,
        Err(error) => {
            bevy::log::warn!("cannot serialize a history entry: {error}");
            return;
        }
    };
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(error) = writeln!(file, "{line}") {
                bevy::log::warn!("cannot write the history: {error}");
            }
        }
        Err(error) => bevy::log::warn!("cannot open the history: {error}"),
    }
}

/// Load the whole history (missing file → empty).
#[must_use]
pub fn load() -> Vec<PlayEntry> {
    history_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map_or_else(Vec::new, |text| parse_log(&text))
}

/// Where exports go: the platform's Downloads folder.
///
/// `dirs::download_dir` resolves that per system — `$HOME/Downloads`
/// on macOS, `FOLDERID_Downloads` on Windows, `XDG_DOWNLOAD_DIR` on
/// Linux. The Linux one comes from the user-dirs config and can be
/// absent, so the old location (beside the log) stays as the
/// fallback: an export that silently goes nowhere would be worse
/// than one in an unusual folder, and the row reports the path
/// either way.
#[must_use]
pub fn export_dir() -> Option<PathBuf> {
    dirs::download_dir()
        .or_else(|| history_path().and_then(|path| path.parent().map(std::path::Path::to_path_buf)))
}

/// The file name an export is offered, before collisions.
#[must_use]
pub fn export_stem(now_ms: u64) -> String {
    // The date is UTC, like every timestamp INSIDE the file — one
    // clock for the whole export, so a row and the name it sits in
    // can never disagree.
    let day = beatbyte_core::history::iso_utc(now_ms);
    format!("beatbyte-play-history-{}", &day[..10])
}

/// A file name in `dir` that is not taken yet: the plain one, else
/// `-2`, `-3` … Pure — `taken` is injected, so the rule is tested
/// without touching a disk.
///
/// Downloads is the player's own folder. Writing a fixed name into
/// it would quietly replace the export they made an hour ago, and
/// an export is often the thing somebody is about to send.
#[must_use]
pub fn unique_name(stem: &str, taken: &impl Fn(&str) -> bool) -> String {
    let plain = format!("{stem}.csv");
    if !taken(&plain) {
        return plain;
    }
    // Bounded: after a hundred exports of the same day, reuse the
    // last rather than spin.
    (2..100)
        .map(|n| format!("{stem}-{n}.csv"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| format!("{stem}-100.csv"))
}

/// Write the whole history as CSV beside the log and return the
/// path — the in-app export.
///
/// CSV, not JSON: the in-app button exists for the person who wants
/// to hand a list to someone, and that list opens in a spreadsheet.
/// The CLI keeps both formats and the filters; this is the one that
/// needs no terminal.
///
/// # Errors
/// When there is no data directory, or the file cannot be written.
pub fn export_csv() -> Result<PathBuf, String> {
    let entries = load();
    let dir = export_dir().ok_or_else(|| "no Downloads folder on this platform".to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|error| format!("{error}"))?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    let name = unique_name(&export_stem(now_ms), &|name: &str| dir.join(name).exists());
    let path = dir.join(name);
    std::fs::write(&path, beatbyte_core::history::to_csv(&entries))
        .map_err(|error| format!("{error}"))?;
    Ok(path)
}

// ── Recording side ──────────────────────────────────────────────────

use bevy::prelude::*;

/// Set by `check_song_end` when a run reaches the song's end, read
/// and removed when the run is logged. `LastResults` cannot answer
/// this: it survives across runs.
#[derive(Resource)]
pub struct RunCompleted;

/// When the current run started — wall clock, for the played
/// duration, plus the unix stamp the entry carries.
#[derive(Resource)]
struct RunStart {
    /// Unix milliseconds, for the log line.
    started_ms: u64,
    /// Monotonic start, for the duration (a wall clock can jump).
    at: std::time::Instant,
}

/// Plugin: one line per played track.
pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(crate::states::AppState::Gameplay), begin_run)
            .add_systems(OnExit(crate::states::AppState::Gameplay), log_run);
    }
}

fn begin_run(mut commands: Commands) {
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    commands.insert_resource(RunStart {
        started_ms,
        at: std::time::Instant::now(),
    });
}

/// Write the run's line. Runs on the way out of gameplay, so an
/// abandoned song is logged exactly like a finished one — with
/// `completed: false` and the seconds it actually ran.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system params
fn log_run(
    mut commands: Commands,
    start: Option<Res<RunStart>>,
    song: Option<Res<crate::boot::LoadedSong>>,
    difficulty: Option<Res<crate::song_select::SelectedDifficulty>>,
    results: Option<Res<crate::gameplay::LastResults>>,
    completed: Option<Res<RunCompleted>>,
    practice: Option<Res<crate::gameplay::PracticeState>>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    roster: Option<Res<crate::multiplayer::PlayerRoster>>,
) {
    commands.remove_resource::<RunCompleted>();
    commands.remove_resource::<RunStart>();
    let (Some(start), Some(song), Some(difficulty)) = (start, song, difficulty) else {
        return;
    };
    let completed = completed.is_some();
    // Player one's numbers, and only from a run that actually
    // finished: `LastResults` is the LAST finished run, so reading
    // it after an abort would attribute an older score to this
    // track.
    let (score, accuracy) = results
        .filter(|_| completed)
        .and_then(|results| {
            results
                .players
                .first()
                .map(|p| (p.performance.score(), p.performance.accuracy()))
        })
        .unwrap_or((0, 0.0));
    let entry = PlayEntry {
        title: song.chart.song.title.clone(),
        artist: song.chart.song.artist.clone(),
        difficulty: difficulty.0.display_name().to_lowercase(),
        started_ms: start.started_ms,
        played_s: start.at.elapsed().as_secs_f64(),
        track_s: song.chart.song.duration_s,
        completed,
        players: roster.map_or(1, |roster| roster.devices.len().max(1)),
        practice: practice.is_some_and(|practice| practice.used),
        autopilot: autopilot.is_some_and(|autopilot| autopilot.enabled),
        score,
        accuracy,
        source: match song.audio {
            crate::boot::SongAudio::Memory(_) => "builtin",
            crate::boot::SongAudio::File(_) => "file",
        }
        .to_owned(),
    };
    append(&entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_go_to_the_downloads_folder() {
        // Where a person looks for a file they just exported.
        let Some(dir) = export_dir() else {
            return; // no Downloads and no data dir (headless CI)
        };
        if let Some(downloads) = dirs::download_dir() {
            assert_eq!(dir, downloads);
        } else {
            // The documented fallback, not a silent failure.
            assert_eq!(
                Some(dir.as_path()),
                history_path().as_deref().and_then(std::path::Path::parent)
            );
        }
    }

    #[test]
    fn an_export_never_overwrites_an_earlier_one() {
        // Downloads is the player's own folder, and an export is
        // often the thing they are about to send: replacing
        // yesterday's file without a word is data loss.
        let stem = "beatbyte-play-history-2026-09-01";
        assert_eq!(unique_name(stem, &|_| false), format!("{stem}.csv"));
        let existing = [format!("{stem}.csv"), format!("{stem}-2.csv")];
        let name = unique_name(stem, &|candidate: &str| {
            existing.iter().any(|taken| taken == candidate)
        });
        assert_eq!(name, format!("{stem}-3.csv"));
        // Bounded: a pathological folder must not spin forever.
        assert_eq!(unique_name(stem, &|_| true), format!("{stem}-100.csv"));
    }

    #[test]
    fn the_file_name_carries_the_day_it_was_written() {
        // UTC, the same clock the rows inside use - a name and a row
        // that disagreed about the date would be worse than neither.
        assert_eq!(
            export_stem(1_756_684_800_000),
            "beatbyte-play-history-2025-09-01"
        );
    }

    #[test]
    fn the_log_sits_beside_the_other_save_files() {
        // Same directory as scores.json and the telemetry folder;
        // an export tool finds it by the same rule.
        let Some(path) = history_path() else {
            return; // headless CI without a data dir
        };
        assert!(path.ends_with("beatbyte/history.jsonl"));
        assert_eq!(
            path.parent(),
            crate::scores::scores_path()
                .as_deref()
                .and_then(std::path::Path::parent)
        );
    }
}
