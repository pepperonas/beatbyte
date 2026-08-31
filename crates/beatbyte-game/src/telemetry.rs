//! Per-note session telemetry — layer 1 of adaptive charting
//! (ADR-0011).
//!
//! The engine already produces a judgment and a signed timing offset
//! for every note and throws them away after the session; this module
//! stops the deletion. Each session is written as one JSONL file
//! beside `scores.json`: a header line, then one line per note event.
//!
//! The schema itself lives in `beatbyte_core::telemetry` and the
//! chart identity in `beatbyte_chart::chart_hash` — one
//! implementation, shared with the CLI that reads the files (the
//! mechanics reference's shared-library rule). This module is only
//! the recording side: buffer in memory while playing, write once on
//! the way out, and never let a failure touch gameplay.

use std::io::Write as _;
use std::path::PathBuf;

use beatbyte_chart::chart_hash;
use beatbyte_core::telemetry::{
    NoteLine, SCHEMA_VERSION, SessionHeader, judgment_label, nearest_judged_index, render_session,
};
use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;

use crate::gameplay::{PlayerSession, SessionFeedback};
use crate::states::AppState;

/// Where session files live.
#[must_use]
pub fn telemetry_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("telemetry"))
}

// ── Bevy side ───────────────────────────────────────────────────────

/// The in-flight recording: per-player line buffers plus the facts
/// captured while the session ran.
#[derive(Resource)]
pub struct SessionRecorder {
    /// Unix milliseconds at session start.
    started_ms: u64,
    /// Lines per player index.
    lines: Vec<(usize, Vec<NoteLine>)>,
    /// Track event count, captured from the live session on the first
    /// frame (the chart's note count is NOT it — chords merge).
    notes_total: Option<usize>,
}

/// Buffer one player's line.
fn push_line(recorder: &mut SessionRecorder, player: usize, line: NoteLine) {
    if let Some((_, lines)) = recorder.lines.iter_mut().find(|(p, _)| *p == player) {
        lines.push(line);
    } else {
        recorder.lines.push((player, vec![line]));
    }
}

/// Start a fresh recording when gameplay begins.
fn begin_recording(mut commands: Commands) {
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    commands.insert_resource(SessionRecorder {
        started_ms,
        lines: Vec::new(),
        notes_total: None,
    });
}

/// Buffer this frame's session events. Pure bookkeeping — no IO.
fn record_feedback(
    mut recorder: ResMut<SessionRecorder>,
    mut feedback: MessageReader<SessionFeedback>,
    sessions: Query<&PlayerSession>,
) {
    // Captured here rather than at setup so this plugin needs no
    // ordering agreement with the gameplay spawn chain: any frame
    // with a live session will do, including the first.
    if recorder.notes_total.is_none()
        && let Some(session) = sessions.iter().next()
    {
        recorder.notes_total = Some(session.session.track().events().len());
    }
    for message in feedback.read() {
        let player = message.player_index;
        match &message.event {
            SessionEvent::NoteHit {
                event_index,
                judgment,
                offset_s,
            } => {
                let line = if *judgment == Judgment::Miss {
                    NoteLine::Miss {
                        i: *event_index,
                        j: "miss".to_owned(),
                    }
                } else {
                    NoteLine::Hit {
                        i: *event_index,
                        j: judgment_label(*judgment).to_owned(),
                        off_ms: offset_s * 1000.0,
                    }
                };
                push_line(&mut recorder, player, line);
            }
            SessionEvent::NoteMissed { event_index } => {
                push_line(
                    &mut recorder,
                    player,
                    NoteLine::Miss {
                        i: *event_index,
                        j: "miss".to_owned(),
                    },
                );
            }
            SessionEvent::SustainEnded {
                event_index,
                completed,
            } => {
                push_line(
                    &mut recorder,
                    player,
                    NoteLine::Sustain {
                        s: *event_index,
                        done: *completed,
                    },
                );
            }
            SessionEvent::Overstrum => {
                // The session does not position an overstrum; the most
                // recently judged event is how analytics localize it
                // to a passage (its absence in the first log was the
                // clue that placed a flake in the count-in).
                let near = recorder
                    .lines
                    .iter()
                    .find(|(p, _)| *p == player)
                    .and_then(|(_, lines)| nearest_judged_index(lines));
                push_line(&mut recorder, player, NoteLine::Overstrum { o: 1, near });
            }
            _ => {}
        }
    }
}

/// Write the session out when gameplay ends — however it ends. An
/// abandoned session is evidence too (fewer judged events than the
/// header's total says exactly that).
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn finalize_recording(
    mut commands: Commands,
    recorder: Option<Res<SessionRecorder>>,
    song: Option<Res<crate::boot::LoadedSong>>,
    difficulty: Option<Res<crate::song_select::SelectedDifficulty>>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    practice: Option<Res<crate::gameplay::PracticeState>>,
) {
    let Some(recorder) = recorder else {
        return;
    };
    // Practice runs leave NO telemetry: sessions played slowed (or
    // part-slowed) would poison the design loop's evidence, and a
    // marked-but-present file would still need every reader to know
    // the flag. The empty file list also hides the results screen's
    // rating offer — feedback about a practice run is feedback
    // about the speed, not the chart.
    if practice.as_ref().is_some_and(|p| p.used) {
        commands.insert_resource(SessionLogFiles { files: Vec::new() });
        commands.remove_resource::<SessionRecorder>();
        return;
    }
    if let (Some(song), Some(difficulty)) = (song, difficulty) {
        let files = write_session(
            &recorder,
            &song,
            difficulty.0,
            autopilot.is_some_and(|a| a.enabled),
        );
        // The results screen appends the player's feedback (A5) to
        // the files this run just wrote — carried by resource,
        // because the recorder itself is gone by then.
        commands.insert_resource(SessionLogFiles { files });
    }
    commands.remove_resource::<SessionRecorder>();
}

/// The session files the last gameplay run wrote — where the results
/// screen's feedback lines (fun rating, versus verdict) are appended.
/// Overwritten by every finalized run; empty when nothing was
/// written (no data dir, empty session).
#[derive(Resource, Default)]
pub struct SessionLogFiles {
    /// One JSONL file per player.
    pub files: Vec<std::path::PathBuf>,
}

/// Append one feedback line to every session file of the last run.
/// Same failure policy as the writer: warn and drop, never panic.
pub fn append_feedback(logs: &SessionLogFiles, line: &NoteLine) {
    let Ok(text) = serde_json::to_string(line) else {
        return;
    };
    for path in &logs.files {
        let result = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{text}"));
        if let Err(error) = result {
            warn!("telemetry: cannot append to {}: {error}", path.display());
        }
    }
}

/// The one function that touches the disk. Every failure is a warning
/// and a dropped file, never a panic — telemetry must not be able to
/// hurt the game that produces it.
fn write_session(
    recorder: &SessionRecorder,
    song: &crate::boot::LoadedSong,
    difficulty: beatbyte_core::Difficulty,
    autopilot: bool,
) -> Vec<std::path::PathBuf> {
    let Some(dir) = telemetry_dir() else {
        warn!("telemetry: no data directory on this platform");
        return Vec::new();
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        warn!("telemetry: cannot create {}: {error}", dir.display());
        return Vec::new();
    }
    let hash = chart_hash(&song.chart);
    // A session that saw no player at all (entered and left within a
    // frame) has nothing to bind and nothing to say.
    let Some(notes_total) = recorder.notes_total else {
        return Vec::new();
    };
    // Players that produced no events still get a file: an abandoned
    // session with zero judged notes is the strongest abandonment
    // signal there is.
    let players: Vec<usize> = if recorder.lines.is_empty() {
        vec![0]
    } else {
        recorder.lines.iter().map(|(p, _)| *p).collect()
    };
    let mut written = Vec::new();
    for player in players {
        let empty = Vec::new();
        let lines = recorder
            .lines
            .iter()
            .find(|(p, _)| *p == player)
            .map_or(&empty, |(_, l)| l);
        let header = SessionHeader {
            schema: SCHEMA_VERSION,
            title: song.chart.song.title.clone(),
            artist: song.chart.song.artist.clone(),
            difficulty: difficulty.display_name().to_lowercase(),
            chart_hash: hash.clone(),
            generator: env!("CARGO_PKG_VERSION").to_owned(),
            started_ms: recorder.started_ms,
            player,
            autopilot,
            notes_total,
        };
        let path = dir.join(format!("{}-p{player}.jsonl", recorder.started_ms));
        let body = render_session(&header, lines);
        let result =
            std::fs::File::create(&path).and_then(|mut file| file.write_all(body.as_bytes()));
        match result {
            Ok(()) => {
                info!("telemetry: {} lines -> {}", lines.len(), path.display());
                written.push(path);
            }
            Err(error) => warn!("telemetry: cannot write {}: {error}", path.display()),
        }
    }
    written
}

/// The telemetry plugin: record while playing, write on the way out.
pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Gameplay), begin_recording)
            .add_systems(Update, record_feedback.run_if(in_state(AppState::Gameplay)))
            .add_systems(OnExit(AppState::Gameplay), finalize_recording);
    }
}
