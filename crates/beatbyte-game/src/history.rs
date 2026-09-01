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
