//! Per-note session telemetry — layer 1 of adaptive charting
//! (ADR-0011).
//!
//! The engine already produces a judgment and a signed timing offset
//! for every note and throws them away after the session; this module
//! stops the deletion. Each session is written as one JSONL file
//! beside `scores.json`: a header line, then one line per note event.
//!
//! Rules that are load-bearing (see `docs/adaptive-charting.md`):
//!
//! - Every observation binds to the **content hash** of the chart it
//!   was played on. An edited chart starts with zero evidence.
//! - The schema is **versioned**, and readers skip lines they do not
//!   understand instead of failing.
//! - Recording must never affect gameplay: lines are buffered in
//!   memory and written once, when the gameplay state is left; a
//!   write failure logs and drops, never panics.

use std::io::Write as _;
use std::path::PathBuf;

use beatbyte_chart::ChartFile;
use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::gameplay::{PlayerSession, SessionFeedback};
use crate::states::AppState;

/// Version of the on-disk schema. Bump when a line's meaning changes;
/// adding a new line kind does not require it (readers skip unknowns).
pub const SCHEMA_VERSION: u32 = 1;

/// The first line of a session file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    /// [`SCHEMA_VERSION`] at the time of writing.
    pub schema: u32,
    /// Song title — its own field, NOT joined with the artist into
    /// one string. The score board's `title|artist` key is a known
    /// collision (roadmap C5); a new schema must not copy it.
    pub title: String,
    /// Song artist.
    pub artist: String,
    /// Difficulty, lowercase display name.
    pub difficulty: String,
    /// Content hash of the exact chart that was played.
    pub chart_hash: String,
    /// The game version that recorded this.
    pub generator: String,
    /// Unix milliseconds when the session began.
    pub started_ms: u64,
    /// Player index (solo = 0).
    pub player: usize,
    /// Whether the autopilot was driving. Autopilot plays perfectly;
    /// evidence readers must be able to exclude it, or every chart
    /// looks too easy.
    pub autopilot: bool,
    /// Note events in the played track. Completion is derived, not
    /// stored: a session is complete when every event was judged —
    /// storing a second flag would just invite disagreement.
    pub notes_total: usize,
}

/// One recorded observation. Serialized untagged: the field sets are
/// disjoint, so each line stays as small as the spec sketches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NoteLine {
    /// A judged hit: event index, judgment, signed offset in ms.
    Hit {
        /// Index into the track's events.
        i: usize,
        /// Judgment label (`perfect` / `great` / `good`).
        j: String,
        /// Signed `hit - note` offset in milliseconds.
        off_ms: f64,
    },
    /// A missed note event.
    Miss {
        /// Index into the track's events.
        i: usize,
        /// Always `"miss"` — kept so a session file greps uniformly
        /// by judgment.
        j: String,
    },
    /// A sustain ended: played out (`done: true`) or dropped early.
    /// Dropped holds are the evidence that separates "too hard" from
    /// "too easy" — a judgment line alone cannot show them.
    Sustain {
        /// Index into the track's events.
        s: usize,
        /// Whether it was held to (or into the grace period of) its
        /// end.
        done: bool,
    },
    /// An overstrum. Counted apart from note accuracy on purpose: it
    /// breaks the streak but is not a missed note.
    Overstrum {
        /// Always `1`; the field is what makes the line parseable.
        o: u32,
    },
}

/// FNV-1a over a byte slice: deterministic, dependency-free, and not
/// adversarial — this is an identity for chart *versions*, not a
/// security boundary.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The content hash a session binds to.
///
/// Hashed over the canonical serde serialization rather than the disk
/// bytes, because builtin songs have no file and formatting must not
/// matter: the same notes are the same chart however they were
/// indented.
#[must_use]
pub fn chart_hash(chart: &ChartFile) -> String {
    let canonical = serde_json::to_vec(chart).unwrap_or_default();
    format!("{:016x}", fnv1a64(&canonical))
}

/// The stable label a judgment is recorded under.
#[must_use]
pub fn judgment_label(judgment: Judgment) -> &'static str {
    match judgment {
        Judgment::Perfect => "perfect",
        Judgment::Great => "great",
        Judgment::Good => "good",
        Judgment::Miss => "miss",
    }
}

/// Render a full session as JSONL.
#[must_use]
pub fn render_session(header: &SessionHeader, lines: &[NoteLine]) -> String {
    let mut out = String::new();
    if let Ok(head) = serde_json::to_string(header) {
        out.push_str(&head);
        out.push('\n');
    }
    for line in lines {
        if let Ok(text) = serde_json::to_string(line) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

/// Parse a session file. Unknown or malformed note lines are skipped
/// — a reader from schema v1 must survive a file that a later version
/// wrote — but a missing or unreadable header is a `None`: without it
/// the observations bind to nothing.
#[must_use]
pub fn parse_session(text: &str) -> Option<(SessionHeader, Vec<NoteLine>)> {
    let mut lines = text.lines();
    let header: SessionHeader = serde_json::from_str(lines.next()?).ok()?;
    let notes = lines
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Some((header, notes))
}

/// How many note events these lines judged (hits + misses). A session
/// is complete when this equals the header's `notes_total` — derived,
/// never stored, so the two cannot disagree.
#[must_use]
pub fn judged_events(lines: &[NoteLine]) -> usize {
    lines
        .iter()
        .filter(|line| matches!(line, NoteLine::Hit { .. } | NoteLine::Miss { .. }))
        .count()
}

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
                push_line(&mut recorder, player, NoteLine::Overstrum { o: 1 });
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
) {
    let Some(recorder) = recorder else {
        return;
    };
    if let (Some(song), Some(difficulty)) = (song, difficulty) {
        write_session(
            &recorder,
            &song,
            difficulty.0,
            autopilot.is_some_and(|a| a.enabled),
        );
    }
    commands.remove_resource::<SessionRecorder>();
}

/// The one function that touches the disk. Every failure is a warning
/// and a dropped file, never a panic — telemetry must not be able to
/// hurt the game that produces it.
fn write_session(
    recorder: &SessionRecorder,
    song: &crate::boot::LoadedSong,
    difficulty: beatbyte_core::Difficulty,
    autopilot: bool,
) {
    let Some(dir) = telemetry_dir() else {
        warn!("telemetry: no data directory on this platform");
        return;
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        warn!("telemetry: cannot create {}: {error}", dir.display());
        return;
    }
    let hash = chart_hash(&song.chart);
    // A session that saw no player at all (entered and left within a
    // frame) has nothing to bind and nothing to say.
    let Some(notes_total) = recorder.notes_total else {
        return;
    };
    // Players that produced no events still get a file: an abandoned
    // session with zero judged notes is the strongest abandonment
    // signal there is.
    let players: Vec<usize> = if recorder.lines.is_empty() {
        vec![0]
    } else {
        recorder.lines.iter().map(|(p, _)| *p).collect()
    };
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
            Ok(()) => info!("telemetry: {} lines -> {}", lines.len(), path.display()),
            Err(error) => warn!("telemetry: cannot write {}: {error}", path.display()),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::schema::{ChartDef, ChartNote, SongMeta};

    fn tiny_chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Test".to_owned(),
                artist: "Unit".to_owned(),
                audio: "t.wav".to_owned(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(10.0),
            },
            charts: vec![ChartDef {
                difficulty: beatbyte_core::Difficulty::Medium,
                lanes: 5,
                notes: vec![ChartNote {
                    time: 1.0,
                    lane: 0,
                    len: 0.0,
                    hopo: false,
                }],
                phrases: Vec::new(),
            }],
        }
    }

    fn header() -> SessionHeader {
        SessionHeader {
            schema: SCHEMA_VERSION,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            difficulty: "medium".to_owned(),
            chart_hash: "abc".to_owned(),
            generator: "0.0.0".to_owned(),
            started_ms: 1_756_500_000_000,
            player: 0,
            autopilot: false,
            notes_total: 3,
        }
    }

    #[test]
    fn a_session_round_trips_through_its_file_form() {
        let lines = vec![
            NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: -12.3,
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
            NoteLine::Sustain { s: 0, done: false },
            NoteLine::Overstrum { o: 1 },
        ];
        let text = render_session(&header(), &lines);
        let (parsed_header, parsed_lines) =
            parse_session(&text).expect("a rendered session parses");
        assert_eq!(parsed_header, header());
        assert_eq!(parsed_lines, lines);
    }

    #[test]
    fn every_line_kind_survives_untagged_serde() {
        // Untagged serde picks the FIRST variant whose fields match,
        // so a hit without its offset would quietly become a miss.
        // Each kind is proven to come back as itself.
        for line in [
            NoteLine::Hit {
                i: 7,
                j: "great".to_owned(),
                off_ms: 4.0,
            },
            NoteLine::Miss {
                i: 8,
                j: "miss".to_owned(),
            },
            NoteLine::Sustain { s: 7, done: true },
            NoteLine::Overstrum { o: 1 },
        ] {
            let text = serde_json::to_string(&line).expect("serializes");
            let back: NoteLine = serde_json::from_str(&text).expect("parses");
            assert_eq!(back, line, "{text} came back as something else");
        }
    }

    #[test]
    fn the_hash_binds_to_the_notes_not_the_wrapper() {
        // The whole point of the hash: an edited chart is a different
        // chart. One note moved by 10 ms must change it.
        let chart = tiny_chart();
        let mut edited = tiny_chart();
        edited.charts[0].notes[0].time = 1.01;
        assert_eq!(chart_hash(&chart), chart_hash(&chart), "not deterministic");
        assert_ne!(
            chart_hash(&chart),
            chart_hash(&edited),
            "an edited note did not change the hash"
        );
    }

    #[test]
    fn a_reader_skips_lines_it_does_not_understand() {
        // Forward compatibility is a rule, not a hope: a v1 reader
        // must survive a file that a later schema wrote lines into.
        let mut text = render_session(
            &header(),
            &[NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: 1.0,
            }],
        );
        text.push_str("{\"future_kind\": {\"x\": 1}}\n");
        text.push_str("not json at all\n");
        let (_, lines) = parse_session(&text).expect("still parses");
        assert_eq!(lines.len(), 1, "unknown lines should be skipped, not kept");
    }

    #[test]
    fn a_file_without_a_header_is_rejected() {
        // Observations that bind to nothing are worse than no file.
        assert!(parse_session("").is_none());
        assert!(parse_session("{\"i\":0,\"j\":\"miss\"}\n").is_none());
    }

    #[test]
    fn completion_is_judged_events_against_the_total() {
        let lines = vec![
            NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: 0.0,
            },
            NoteLine::Sustain { s: 0, done: true },
            NoteLine::Overstrum { o: 1 },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
        ];
        // Sustains and overstrums are observations about HOW events
        // were played; only hits and misses say THAT one was judged.
        assert_eq!(judged_events(&lines), 2);
    }

    #[test]
    fn every_judgment_has_a_stable_label() {
        assert_eq!(judgment_label(Judgment::Perfect), "perfect");
        assert_eq!(judgment_label(Judgment::Great), "great");
        assert_eq!(judgment_label(Judgment::Good), "good");
        assert_eq!(judgment_label(Judgment::Miss), "miss");
    }
}
