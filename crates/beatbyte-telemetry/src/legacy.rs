//! Importing the older per-session JSONL files.
//!
//! Layer 1 of ADR-0011 shipped as one JSONL file per played session.
//! Those files are real evidence — hundreds of runs, months of
//! playing — and an upgrade that silently left them behind would
//! throw the entire history away on the day the store arrived. So
//! they are read once and written into the store.
//!
//! # What an imported run can and cannot say
//!
//! The old format recorded **which** note, never **when**: there is
//! no song time in a line. Those events are stored with a null time
//! and every time-based query skips them, which is the honest
//! outcome — better than a zero that reads like the start of the
//! song.
//!
//! It also has no action stream, no device, and no calibration
//! offsets. That is not a *hole* in the recording, it is a lower
//! detail level, so an imported session carries
//! [`Detail::Results`] and stays `telemetry_complete`: nothing was
//! dropped. A reader that wants the input path filters on the detail
//! level; a reader that wants whole recordings still gets these.
//!
//! # Idempotence
//!
//! The id of an imported run is derived from the file's own facts, so
//! importing the same directory twice adds nothing the second time.
//! The proof that this works is a second run that reports zero.

use std::path::{Path, PathBuf};

use beatbyte_core::Difficulty;
use beatbyte_core::telemetry::{NoteLine, SessionHeader, parse_session};

use crate::model::{
    Completion, Detail, Event, EventType, InputDevice, Outcome, Provenance, Rating, SessionRow,
};
use crate::schema;
use crate::store::{PlayerNote, Store};
use crate::{Error, Result};

/// What one import pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// Files that were looked at.
    pub seen: usize,
    /// Sessions written.
    pub imported: usize,
    /// Files whose run was already in the store.
    pub already_there: usize,
    /// Files that could not be read as a session.
    pub unreadable: usize,
    /// Observations written.
    pub events: usize,
    /// Player notes written.
    pub notes: usize,
}

/// The id an imported run gets.
///
/// Derived from the run's own facts rather than from a clock, which
/// is what makes a second import a no-op. Prefixed so that a row's
/// provenance is readable at a glance.
#[must_use]
pub fn legacy_uid(started_ms: u64, player: usize) -> String {
    format!("legacy-{started_ms:016x}-{player:04x}")
}

/// Turn a parsed legacy session into the rows it becomes.
///
/// Pure: no clock, no file system, no database — so what an old file
/// turns into can be pinned exactly.
#[must_use]
pub fn convert(
    header: &SessionHeader,
    lines: &[NoteLine],
) -> (SessionRow, Vec<Event>, Vec<PlayerNote>) {
    let difficulty = Difficulty::from_id(&header.difficulty).map_or(0, |value| {
        Difficulty::ALL
            .iter()
            .position(|d| *d == value)
            .unwrap_or(0) as u8
    });
    let row = SessionRow {
        uid: legacy_uid(header.started_ms, header.player),
        started_ms: header.started_ms,
        title: header.title.clone(),
        artist: header.artist.clone(),
        genre: None,
        chart_hash: header.chart_hash.clone(),
        chart_file: None,
        difficulty,
        player_slot: u8::try_from(header.player).unwrap_or(0),
        player_id: None,
        provenance: Provenance {
            // The old header called the game version "generator";
            // this store keeps the two apart, and the field that was
            // actually written is the game's version.
            game: header.generator.clone(),
            chart_format: beatbyte_chart_format(),
            generator: None,
            scoring: 0,
            analysis: None,
            vocal: None,
        },
        telemetry_schema: schema::schema_version(),
        detail: Detail::Results,
        input_device: InputDevice::Unknown,
        input_offset_ms: None,
        video_offset_ms: None,
        mic_offset_ms: None,
        // The old format recorded none of these. `false` would be a
        // claim; these three are what the old files actually stated.
        tap_mode: false,
        no_fail: false,
        practice: false,
        autopilot: header.autopilot,
        notes_total: u32::try_from(header.notes_total).unwrap_or(u32::MAX),
    };

    let mut events = Vec::new();
    let mut notes = Vec::new();
    for line in lines {
        match line {
            NoteLine::Hit { i, j, off_ms } => {
                let rating = match j.as_str() {
                    "perfect" => Rating::Perfect,
                    "great" => Rating::Great,
                    "good" => Rating::Good,
                    _ => Rating::Miss,
                };
                events.push(
                    Event::untimed(EventType::NoteHit)
                        .about(*i)
                        .off_by(off_ms / 1000.0)
                        .judged(rating),
                );
            }
            NoteLine::Miss { i, .. } => {
                events.push(Event::untimed(EventType::NoteMiss).about(*i));
            }
            NoteLine::Sustain { s, done } => {
                let mut event = Event::untimed(EventType::SustainEnded).about(*s);
                if *done {
                    event = event.flagged(crate::model::Flags::DONE);
                }
                events.push(event);
            }
            NoteLine::Overstrum { near, .. } => {
                let mut event = Event::untimed(EventType::Overstrum);
                if let Some(index) = near {
                    event = event.about(*index);
                }
                events.push(event);
            }
            NoteLine::Fun { fun } => notes.push(PlayerNote::Fun(*fun)),
            NoteLine::Comment { comment } => notes.push(PlayerNote::Comment(comment.clone())),
            NoteLine::Versus { versus, parent } => notes.push(PlayerNote::Versus {
                better: versus == "better",
                parent: parent.clone(),
            }),
        }
    }
    (row, events, notes)
}

/// The chart format an imported run is assumed to be on.
///
/// The old header never said, and there has only ever been one.
const fn beatbyte_chart_format() -> u32 {
    1
}

/// Whether an imported session played to the end.
///
/// Derived the way the old readers derived it — every event judged —
/// because the old format stored no completion and deriving it beats
/// recording `Unknown` for a session that clearly finished.
#[must_use]
pub fn completion_of(header: &SessionHeader, lines: &[NoteLine]) -> Completion {
    let judged = beatbyte_core::telemetry::judged_events(lines);
    if header.notes_total > 0 && judged >= header.notes_total {
        Completion::Completed
    } else {
        Completion::Aborted
    }
}

/// A stored session in the older shape.
///
/// The direction back out, so that the readers written against the
/// JSONL format — `beatbyte-cli review` and the design dossier — can
/// move onto the store without their tested logic changing. The
/// conversion is total: everything those readers look at survives it.
/// What does not survive is what the old shape has no room for (the
/// action stream, the device, the offsets), and neither reader asks.
#[must_use]
pub fn to_legacy(
    session: &crate::store::StoredSession,
    events: &[(u32, Event)],
    notes: &[PlayerNote],
) -> (SessionHeader, Vec<NoteLine>) {
    let row = &session.row;
    let header = SessionHeader {
        schema: beatbyte_core::telemetry::SCHEMA_VERSION,
        title: row.title.clone(),
        artist: row.artist.clone(),
        difficulty: Difficulty::ALL
            .get(row.difficulty as usize)
            .copied()
            .unwrap_or(Difficulty::Medium)
            .id()
            .to_owned(),
        chart_hash: row.chart_hash.clone(),
        generator: row.provenance.game.clone(),
        started_ms: row.started_ms,
        player: row.player_slot as usize,
        autopilot: row.autopilot,
        notes_total: row.notes_total as usize,
    };
    let mut lines = Vec::new();
    for (_, event) in events {
        let index = event.note_index.map(|index| index as usize);
        match event.kind {
            EventType::NoteHit => {
                if let Some(index) = index {
                    lines.push(NoteLine::Hit {
                        i: index,
                        j: event.rating.unwrap_or(Rating::Good).label().to_owned(),
                        off_ms: f64::from(event.delta_us.unwrap_or(0)) / 1000.0,
                    });
                }
            }
            EventType::NoteMiss => {
                if let Some(index) = index {
                    lines.push(NoteLine::Miss {
                        i: index,
                        j: "miss".to_owned(),
                    });
                }
            }
            EventType::SustainEnded => {
                if let Some(index) = index {
                    lines.push(NoteLine::Sustain {
                        s: index,
                        done: event.flags.has(crate::model::Flags::DONE),
                    });
                }
            }
            EventType::Overstrum => lines.push(NoteLine::Overstrum { o: 1, near: index }),
            // Everything else is either derivable or has no place in
            // the old shape; a reader of that shape never asked.
            _ => {}
        }
    }
    for note in notes {
        match note {
            PlayerNote::Fun(score) => lines.push(NoteLine::Fun { fun: *score }),
            PlayerNote::Comment(text) => lines.push(NoteLine::Comment {
                comment: text.clone(),
            }),
            PlayerNote::Versus { better, parent } => lines.push(NoteLine::Versus {
                versus: if *better { "better" } else { "worse" }.to_owned(),
                parent: parent.clone(),
            }),
        }
    }
    (header, lines)
}

/// Import one file.
pub fn import_file(store: &mut Store, path: &Path) -> Result<bool> {
    let text = std::fs::read_to_string(path)?;
    let Some((header, lines)) = parse_session(&text) else {
        return Err(Error::Import(format!(
            "{}: no session header",
            path.display()
        )));
    };
    let (row, events, notes) = convert(&header, &lines);
    if store.has_uid(&row.uid)? {
        return Ok(false);
    }
    let completion = completion_of(&header, &lines);
    let id = store.begin(&row)?;
    let numbered: Vec<(u32, Event)> = events
        .into_iter()
        .enumerate()
        .map(|(index, event)| (u32::try_from(index).unwrap_or(u32::MAX), event))
        .collect();
    store.append(id, &numbered)?;
    for note in &notes {
        store.add_note(id, header.started_ms, note)?;
    }
    store.finish(
        id,
        Outcome {
            // The old files recorded no end time. The start is the
            // only honest stamp there is, and a null would lose the
            // fact that the run definitely ended.
            ended_ms: header.started_ms,
            completion,
            dropped: 0,
            // The old writer skipped practice runs entirely, so a
            // file existing at all is evidence that this one was not.
            practice: false,
        },
    )?;
    Ok(true)
}

/// Import every `*.jsonl` in a directory.
pub fn import_dir(store: &mut Store, dir: &Path) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut files: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(report);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            files.push(path);
        }
    }
    // Oldest first, so the store's row ids follow the order the runs
    // were actually played.
    files.sort();
    for path in files {
        report.seen += 1;
        let before_events = store.event_count().unwrap_or(0);
        match import_file(store, &path) {
            Ok(true) => {
                report.imported += 1;
                let after = store.event_count().unwrap_or(before_events);
                report.events += usize::try_from(after.saturating_sub(before_events)).unwrap_or(0);
            }
            Ok(false) => report.already_there += 1,
            Err(_) => report.unreadable += 1,
        }
    }
    report.notes = usize::try_from(
        store
            .query("SELECT COUNT(*) FROM session_note", &[], |row| {
                row.get::<_, i64>(0)
            })?
            .first()
            .copied()
            .unwrap_or(0)
            .max(0),
    )
    .unwrap_or(0);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::telemetry::SCHEMA_VERSION;

    fn header() -> SessionHeader {
        SessionHeader {
            schema: SCHEMA_VERSION,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            difficulty: "hard".to_owned(),
            chart_hash: "abc123".to_owned(),
            generator: "0.14.2".to_owned(),
            started_ms: 1_700_000_000_000,
            player: 0,
            autopilot: false,
            notes_total: 3,
        }
    }

    fn lines() -> Vec<NoteLine> {
        vec![
            NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: -3.0,
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
            NoteLine::Sustain { s: 0, done: true },
            NoteLine::Overstrum {
                o: 1,
                near: Some(1),
            },
            NoteLine::Fun { fun: 4 },
            NoteLine::Comment {
                comment: "the chorus drags".to_owned(),
            },
        ]
    }

    #[test]
    fn an_old_session_becomes_the_same_facts_in_the_new_shape() {
        let (row, events, notes) = convert(&header(), &lines());
        assert_eq!(row.title, "Maria");
        assert_eq!(row.difficulty, 2, "hard is index two");
        assert_eq!(row.provenance.game, "0.14.2");
        assert_eq!(row.detail, Detail::Results);
        assert_eq!(row.notes_total, 3);
        assert_eq!(
            row.input_offset_ms, None,
            "an old file never recorded a calibration, and a zero \
             would read as one"
        );
        assert_eq!(events.len(), 4, "four observations, two player notes");
        assert_eq!(events[0].kind, EventType::NoteHit);
        assert_eq!(events[0].rating, Some(Rating::Perfect));
        assert_eq!(events[0].delta_us, Some(-3_000));
        assert_eq!(
            events[0].song_time_us, None,
            "the old format never said when"
        );
        assert!(events[2].flags.has(crate::model::Flags::DONE));
        assert_eq!(
            events[3].note_index,
            Some(1),
            "an overstrum keeps its anchor"
        );
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], PlayerNote::Fun(4));
    }

    #[test]
    fn completion_is_derived_the_way_the_old_readers_derived_it() {
        assert_eq!(
            completion_of(&header(), &lines()),
            Completion::Aborted,
            "two of three notes judged is a run that was left"
        );
        let full = vec![
            NoteLine::Miss {
                i: 0,
                j: "miss".to_owned(),
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
            NoteLine::Miss {
                i: 2,
                j: "miss".to_owned(),
            },
        ];
        assert_eq!(completion_of(&header(), &full), Completion::Completed);
    }

    #[test]
    fn importing_a_directory_twice_imports_it_once() {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-telemetry-legacy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let body = beatbyte_core::telemetry::render_session(&header(), &lines());
        std::fs::write(dir.join("1700000000000-p0.jsonl"), &body).expect("writes");
        // A file that is not a session at all.
        std::fs::write(dir.join("broken.jsonl"), "not json\n").expect("writes");

        let mut store = Store::open_in_memory().expect("a store");
        let first = import_dir(&mut store, &dir).expect("imports");
        assert_eq!(first.seen, 2);
        assert_eq!(first.imported, 1);
        assert_eq!(first.unreadable, 1, "a broken file is counted, not fatal");
        assert_eq!(first.events, 4);
        assert_eq!(first.notes, 2);

        let second = import_dir(&mut store, &dir).expect("imports again");
        assert_eq!(
            second.imported, 0,
            "the second pass must add nothing at all"
        );
        assert_eq!(second.already_there, 1);
        assert_eq!(store.session_count().expect("counts"), 1);
        assert_eq!(store.event_count().expect("counts"), 4);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_imported_run_is_whole_even_though_it_knows_less() {
        // The distinction that matters: a lower DETAIL level is not
        // an incomplete RECORDING. Marking these incomplete would
        // exclude every historical session from every query that
        // filters on completeness.
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-telemetry-whole-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(
            dir.join("a.jsonl"),
            beatbyte_core::telemetry::render_session(&header(), &lines()),
        )
        .expect("writes");
        let mut store = Store::open_in_memory().expect("a store");
        import_dir(&mut store, &dir).expect("imports");
        let session = store
            .sessions(1)
            .expect("lists")
            .into_iter()
            .next()
            .expect("the run");
        assert!(session.complete);
        assert_eq!(session.dropped, 0);
        assert_eq!(session.row.detail, Detail::Results);
        assert!(
            crate::analytics::incomplete_sessions(&store)
                .expect("reads")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// In and out again: the readers written against the old shape
    /// have to see exactly what they saw before, or moving them onto
    /// the store would change their answers.
    #[test]
    fn a_session_survives_the_round_trip_through_the_store() {
        let mut store = Store::open_in_memory().expect("a store");
        let (row, events, notes) = convert(&header(), &lines());
        let id = store.begin(&row).expect("begins");
        let numbered: Vec<(u32, Event)> = events
            .into_iter()
            .enumerate()
            .map(|(index, event)| (index as u32, event))
            .collect();
        store.append(id, &numbered).expect("appends");
        for note in &notes {
            store.add_note(id, 1, note).expect("adds");
        }
        let stored = store.session(id).expect("reads").expect("is there");
        let (header_back, lines_back) = to_legacy(
            &stored,
            &store.events(id).expect("reads"),
            &store.notes(id).expect("reads"),
        );
        assert_eq!(header_back, header(), "the header is what it was");
        assert_eq!(lines_back, lines(), "and so is every observation, in order");
    }

    #[test]
    fn two_players_of_one_run_do_not_collide() {
        let mut second = header();
        second.player = 1;
        let (one, ..) = convert(&header(), &[]);
        let (two, ..) = convert(&second, &[]);
        assert_ne!(one.uid, two.uid);
        assert_eq!(two.player_slot, 1);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let mut store = Store::open_in_memory().expect("a store");
        let report = import_dir(&mut store, Path::new("/nonexistent/telemetry")).expect("reports");
        assert_eq!(report, ImportReport::default());
    }
}
