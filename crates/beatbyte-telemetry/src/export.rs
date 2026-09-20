//! Handing the store to another tool — or to a person.
//!
//! A database is queryable and a text file is readable, and the
//! repository has always valued the second. So the store can render
//! itself: one session as lines a person can scan, the whole thing as
//! CSV for a spreadsheet or a notebook, one session as JSON for
//! anything else.
//!
//! Nothing here leaves the machine. An export writes a file where the
//! caller says; it is the player's data, handed to the player.

use crate::model::{Event, EventType, seconds};
use crate::store::{SessionId, Store, StoredSession};
use crate::{Result, analytics};

/// One CSV field, quoted if it has to be.
fn field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Join a row of already-rendered fields.
fn row<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut line = String::new();
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        line.push_str(&field(value));
    }
    line.push('\n');
    line
}

/// Every session as CSV, newest first.
pub fn sessions_csv(store: &Store, limit: usize) -> Result<String> {
    let mut out = row([
        "uid",
        "started_ms",
        "ended_ms",
        "title",
        "artist",
        "genre",
        "chart_hash",
        "difficulty",
        "player_slot",
        "game_version",
        "generator",
        "scoring_version",
        "detail",
        "input_device",
        "input_offset_ms",
        "tap_mode",
        "no_fail",
        "practice",
        "autopilot",
        "completion",
        "notes_total",
        "dropped_events",
        "telemetry_complete",
    ]);
    for session in store.sessions(limit)? {
        out.push_str(&session_row(&session));
    }
    Ok(out)
}

fn session_row(session: &StoredSession) -> String {
    let inner = &session.row;
    row([
        inner.uid.as_str(),
        &inner.started_ms.to_string(),
        &session
            .ended_ms
            .map(|ms| ms.to_string())
            .unwrap_or_default(),
        inner.title.as_str(),
        inner.artist.as_str(),
        inner.genre.as_deref().unwrap_or(""),
        inner.chart_hash.as_str(),
        &inner.difficulty.to_string(),
        &inner.player_slot.to_string(),
        inner.provenance.game.as_str(),
        inner.provenance.generator.as_deref().unwrap_or(""),
        &inner.provenance.scoring.to_string(),
        inner.detail.label(),
        inner.input_device.label(),
        &inner
            .input_offset_ms
            .map(|value| format!("{value:.2}"))
            .unwrap_or_default(),
        &inner.tap_mode.to_string(),
        &inner.no_fail.to_string(),
        &inner.practice.to_string(),
        &inner.autopilot.to_string(),
        session.completion.label(),
        &inner.notes_total.to_string(),
        &session.dropped.to_string(),
        &session.complete.to_string(),
    ])
}

/// One session's events as CSV.
pub fn events_csv(store: &Store, session: SessionId) -> Result<String> {
    let mut out = row([
        "sequence",
        "song_time_s",
        "event",
        "note_index",
        "action",
        "delta_ms",
        "rating",
        "value",
        "flags",
    ]);
    for (sequence, event) in store.events(session)? {
        out.push_str(&event_row(sequence, &event));
    }
    Ok(out)
}

fn event_row(sequence: u32, event: &Event) -> String {
    row([
        &sequence.to_string(),
        &event
            .song_time_us
            .map(|us| format!("{:.6}", seconds(us)))
            .unwrap_or_default(),
        event.kind.label(),
        &event
            .note_index
            .map(|index| index.to_string())
            .unwrap_or_default(),
        &event
            .action
            .map(crate::model::Action::label)
            .unwrap_or_default(),
        &event
            .delta_us
            .map(|us| format!("{:.3}", f64::from(us) / 1000.0))
            .unwrap_or_default(),
        &event
            .rating
            .map(|rating| rating.label().to_owned())
            .unwrap_or_default(),
        &event
            .value
            .map(|value| value.to_string())
            .unwrap_or_default(),
        &format!("{:#06x}", event.flags.0),
    ])
}

/// The analysis dataset: one row per judged note, with the session
/// context already joined on.
///
/// This is the shape a notebook wants and the shape this store exists
/// to avoid storing — the join is cheap, the duplication would not
/// have been.
pub fn dataset_csv(store: &Store, limit: usize) -> Result<String> {
    let mut out = row([
        "uid",
        "title",
        "artist",
        "genre",
        "chart_hash",
        "difficulty",
        "generator",
        "input_device",
        "note_index",
        "event",
        "delta_ms",
        "rating",
        "flags",
    ]);
    let rows = store.query(
        "SELECT s.uid, s.title, s.artist, s.genre, s.chart_hash, s.difficulty,
                s.generator_version, s.input_device, e.note_index, e.event_type,
                e.delta_us, e.rating, e.flags
           FROM gameplay_event e JOIN gameplay_session s USING (session_id)
          WHERE e.event_type IN (?1, ?2) AND s.autopilot = 0 AND s.practice = 0
          ORDER BY s.started_ms, e.sequence
          LIMIT ?3",
        &[
            &EventType::NoteHit.code(),
            &EventType::NoteMiss.code(),
            &(limit as i64),
        ],
        |sql| {
            let device: u8 = sql.get(7)?;
            let kind: u8 = sql.get(9)?;
            let rating: Option<u8> = sql.get(11)?;
            let delta: Option<i32> = sql.get(10)?;
            let flags: u16 = sql.get(12)?;
            let note: Option<u32> = sql.get(8)?;
            let cells: [String; 13] = [
                sql.get(0)?,
                sql.get(1)?,
                sql.get(2)?,
                sql.get::<_, Option<String>>(3)?.unwrap_or_default(),
                sql.get(4)?,
                sql.get::<_, u8>(5)?.to_string(),
                sql.get::<_, Option<String>>(6)?.unwrap_or_default(),
                crate::model::InputDevice::from_code(device)
                    .label()
                    .to_owned(),
                note.map(|index| index.to_string()).unwrap_or_default(),
                EventType::from_code(kind)
                    .map_or("unknown", EventType::label)
                    .to_owned(),
                delta
                    .map(|us| format!("{:.3}", f64::from(us) / 1000.0))
                    .unwrap_or_default(),
                rating
                    .and_then(crate::model::Rating::from_code)
                    .map(|value| value.label().to_owned())
                    .unwrap_or_default(),
                format!("{flags:#06x}"),
            ];
            Ok(row(cells.iter().map(String::as_str)))
        },
    )?;
    for line in rows {
        out.push_str(&line);
    }
    Ok(out)
}

/// One session as JSON: the header, its events and the player's notes.
pub fn session_json(store: &Store, session: SessionId) -> Result<String> {
    let Some(stored) = store.session(session)? else {
        return Ok("null".to_owned());
    };
    let events: Vec<serde_json::Value> = store
        .events(session)?
        .into_iter()
        .map(|(sequence, event)| {
            serde_json::json!({
                "sequence": sequence,
                "song_time_s": event.song_time_us.map(seconds),
                "event": event.kind.label(),
                "note_index": event.note_index,
                "action": event.action.map(crate::model::Action::label),
                "delta_ms": event.delta_us.map(|us| f64::from(us) / 1000.0),
                "rating": event.rating.map(|rating| rating.label()),
                "value": event.value,
                "flags": event.flags.0,
            })
        })
        .collect();
    let notes: Vec<serde_json::Value> = store
        .notes(session)?
        .into_iter()
        .map(|note| match note {
            crate::store::PlayerNote::Fun(score) => serde_json::json!({"fun": score}),
            crate::store::PlayerNote::Comment(text) => serde_json::json!({"comment": text}),
            crate::store::PlayerNote::Versus { better, parent } => {
                serde_json::json!({"versus": if better { "better" } else { "worse" }, "parent": parent})
            }
        })
        .collect();
    let document = serde_json::json!({
        "uid": stored.row.uid,
        "started_ms": stored.row.started_ms,
        "ended_ms": stored.ended_ms,
        "title": stored.row.title,
        "artist": stored.row.artist,
        "genre": stored.row.genre,
        "chart_hash": stored.row.chart_hash,
        "difficulty": stored.row.difficulty,
        "player_slot": stored.row.player_slot,
        "game_version": stored.row.provenance.game,
        "generator": stored.row.provenance.generator,
        "scoring_version": stored.row.provenance.scoring,
        "detail": stored.row.detail.label(),
        "input_device": stored.row.input_device.label(),
        "input_offset_ms": stored.row.input_offset_ms,
        "completion": stored.completion.label(),
        "notes_total": stored.row.notes_total,
        "dropped_events": stored.dropped,
        "telemetry_complete": stored.complete,
        "events": events,
        "player_notes": notes,
    });
    Ok(serde_json::to_string_pretty(&document).unwrap_or_else(|_| "null".to_owned()))
}

/// One session, rendered for a person to read.
///
/// The thing a JSONL file gave for free and a database does not: the
/// ability to look at a run without a query.
pub fn session_report(store: &Store, session: SessionId) -> Result<String> {
    let Some(stored) = store.session(session)? else {
        return Ok(String::new());
    };
    let inner = &stored.row;
    let mut out = String::new();
    out.push_str(&format!(
        "{} — {}  ({}, difficulty {}, player {})\n",
        inner.title, inner.artist, inner.chart_hash, inner.difficulty, inner.player_slot
    ));
    out.push_str(&format!(
        "  game {}  generator {}  scoring v{}  detail {}  device {}\n",
        inner.provenance.game,
        inner.provenance.generator.as_deref().unwrap_or("—"),
        inner.provenance.scoring,
        inner.detail.label(),
        inner.input_device.label()
    ));
    out.push_str(&format!(
        "  {} — dropped {}, {}\n",
        stored.completion.label(),
        stored.dropped,
        if stored.complete {
            "recording whole"
        } else {
            "RECORDING INCOMPLETE"
        }
    ));
    for (sequence, event) in store.events(session)? {
        let when = event.song_time_us.map_or_else(
            || "     ?   ".to_owned(),
            |us| format!("{:8.3}s", seconds(us)),
        );
        let mut line = format!("  {sequence:>6} {when} {}", event.kind.label());
        if let Some(note) = event.note_index {
            line.push_str(&format!(" note {note}"));
        }
        if let Some(action) = event.action {
            line.push_str(&format!(" {}", action.label()));
        }
        if let Some(delta) = event.delta_us {
            line.push_str(&format!(" {:+.1} ms", f64::from(delta) / 1000.0));
        }
        if let Some(rating) = event.rating {
            line.push_str(&format!(" {}", rating.label()));
        }
        line.push('\n');
        out.push_str(&line);
    }
    for note in store.notes(session)? {
        out.push_str(&format!("  said: {note:?}\n"));
    }
    Ok(out)
}

/// A short summary of what is in the store — what a CLI prints first.
pub fn overview(store: &Store) -> Result<String> {
    let sessions = store.session_count()?;
    let events = store.event_count()?;
    let bytes = store.size_bytes()?;
    let incomplete = analytics::incomplete_sessions(store)?.len();
    let per_session = events.checked_div(sessions).unwrap_or(0);
    let per_event = bytes.checked_div(events).unwrap_or(0);
    Ok(format!(
        "{sessions} sessions, {events} events ({per_session} per session), \
         {:.1} MB ({per_event} bytes per event), {incomplete} incomplete\n",
        bytes as f64 / 1_048_576.0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Action, Completion, Detail, Event, InputDevice, Outcome, Provenance, Rating, SessionRow,
        micros,
    };
    use crate::schema;
    use crate::store::PlayerNote;

    fn a_store() -> (Store, SessionId) {
        let mut store = Store::open_in_memory().expect("a store");
        let row = SessionRow {
            uid: "run".to_owned(),
            started_ms: 1_700_000_000_000,
            title: "Maria, \"live\"".to_owned(),
            artist: "Blondie".to_owned(),
            genre: Some("rock".to_owned()),
            chart_hash: "abc".to_owned(),
            chart_file: Some("chart.v3.json".to_owned()),
            difficulty: 1,
            player_slot: 0,
            player_id: None,
            provenance: Provenance {
                game: "0.18.0".to_owned(),
                chart_format: 1,
                generator: Some("v17".to_owned()),
                scoring: 1,
                analysis: None,
                vocal: None,
            },
            telemetry_schema: schema::schema_version(),
            detail: Detail::Actions,
            input_device: InputDevice::Guitar,
            input_offset_ms: Some(-12.0),
            video_offset_ms: Some(0.0),
            mic_offset_ms: None,
            tap_mode: false,
            no_fail: true,
            practice: false,
            autopilot: false,
            notes_total: 2,
        };
        let id = store.begin(&row).expect("begins");
        store
            .append(
                id,
                &[
                    (
                        1,
                        Event::new(EventType::Action, micros(1.0)).acting(Action::Strum),
                    ),
                    (
                        2,
                        Event::new(EventType::NoteHit, micros(1.0))
                            .about(0)
                            .off_by(-0.004)
                            .judged(Rating::Perfect),
                    ),
                    (3, Event::untimed(EventType::NoteMiss).about(1)),
                ],
            )
            .expect("appends");
        store
            .add_note(id, 1, &PlayerNote::Comment("the chorus drags".to_owned()))
            .expect("adds");
        store
            .finish(
                id,
                Outcome {
                    ended_ms: 1_700_000_100_000,
                    completion: Completion::Completed,
                    dropped: 0,
                    practice: false,
                },
            )
            .expect("finishes");
        (store, id)
    }

    #[test]
    fn a_comma_in_a_title_does_not_become_a_column() {
        let (store, _) = a_store();
        let csv = sessions_csv(&store, 10).expect("exports");
        let header_columns = csv.lines().next().unwrap_or_default().split(',').count();
        let data_columns = csv.lines().nth(1).unwrap_or_default().split(',').count();
        assert!(
            data_columns > header_columns,
            "the quoted comma is still a comma inside the quotes"
        );
        assert!(
            csv.contains("\"Maria, \"\"live\"\"\""),
            "a quote inside a quoted field is doubled: {csv}"
        );
    }

    #[test]
    fn an_event_export_says_what_it_does_not_know() {
        let (store, id) = a_store();
        let csv = events_csv(&store, id).expect("exports");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 4, "a header and three events");
        assert!(lines[1].contains("strum"));
        assert!(lines[2].contains("perfect"));
        assert!(lines[2].contains("-4.000"), "{}", lines[2]);
        // The untimed one leaves the time column empty rather than
        // writing a zero that reads as the start of the song.
        assert!(lines[3].starts_with("3,,note_miss,1"), "{}", lines[3]);
    }

    #[test]
    fn a_session_renders_as_json_and_as_something_a_person_can_read() {
        let (store, id) = a_store();
        let json = session_json(&store, id).expect("exports");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("is json");
        assert_eq!(parsed["uid"], "run");
        assert_eq!(parsed["events"].as_array().map(Vec::len), Some(3));
        assert_eq!(parsed["player_notes"][0]["comment"], "the chorus drags");
        assert_eq!(parsed["completion"], "completed");

        let report = session_report(&store, id).expect("renders");
        assert!(report.contains("Maria"));
        assert!(report.contains("recording whole"));
        assert!(report.contains("note 0"));
        assert!(report.contains("-4.0 ms"), "{report}");

        assert_eq!(session_json(&store, 9999).expect("exports"), "null");
        assert!(session_report(&store, 9999).expect("renders").is_empty());
    }

    #[test]
    fn the_dataset_joins_the_song_context_the_events_do_not_carry() {
        let (store, _) = a_store();
        let csv = dataset_csv(&store, 100).expect("exports");
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3, "a header and the two judged notes");
        assert!(lines[1].contains("rock"), "the genre is joined on");
        assert!(lines[1].contains("guitar"), "so is the device");
        assert!(
            !lines[1].contains("strum"),
            "an action is not a judged note and does not belong here"
        );
    }

    #[test]
    fn the_overview_counts_what_is_there() {
        let (store, _) = a_store();
        let text = overview(&store).expect("summarises");
        assert!(text.starts_with("1 sessions, 3 events"), "{text}");
        assert!(text.contains("0 incomplete"));
    }

    #[test]
    fn an_empty_store_says_nothing_rather_than_dividing_by_zero() {
        let store = Store::open_in_memory().expect("a store");
        let text = overview(&store).expect("summarises");
        assert!(
            text.starts_with("0 sessions, 0 events (0 per session)"),
            "{text}"
        );
        assert_eq!(
            sessions_csv(&store, 10).expect("exports").lines().count(),
            1
        );
    }
}
