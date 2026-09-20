//! The store: one SQLite file, opened once, written in batches.
//!
//! Nothing in here runs on the frame thread — [`crate::writer`] owns
//! a `Store` on a thread of its own and is the only caller during a
//! song. The offline tools use it directly, which is the point of
//! keeping it a plain synchronous type.

use std::path::Path;

use rusqlite::{Connection, params};

use crate::Result;
use crate::model::{
    Completion, Detail, Event, EventType, Flags, InputDevice, Outcome, Provenance, Rating,
    SessionRow,
};
use crate::schema;

/// A session's row id.
pub type SessionId = i64;

/// Something the player said about a run, after it ended.
///
/// The one signal telemetry cannot derive (ADR-0011 A5). Kept in its
/// own small table rather than in the event stream: a comment is text,
/// and a text column on a million-row table costs every row that will
/// never have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerNote {
    /// The one-key fun rating, 1 (no fun) … 5 (loved it).
    Fun(u8),
    /// A sentence about the run.
    Comment(String),
    /// This chart version against the one it was derived from.
    Versus {
        /// Whether it felt better than its parent.
        better: bool,
        /// The parent version's chart hash, so the verdict names both
        /// sides even after the pointer moves.
        parent: String,
    },
}

impl PlayerNote {
    /// The stored kind code.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        match self {
            PlayerNote::Fun(_) => 0,
            PlayerNote::Comment(_) => 1,
            PlayerNote::Versus { .. } => 2,
        }
    }
}

/// A session as it came back out of the store.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSession {
    /// The row id.
    pub id: SessionId,
    /// Everything that was true at the start.
    pub row: SessionRow,
    /// When it ended — `None` means it never did: the process went
    /// away mid-song. That is a fact about the run, not a defect.
    pub ended_ms: Option<u64>,
    /// How it ended.
    pub completion: Completion,
    /// Events the queue could not take.
    pub dropped: u32,
    /// Whether the recording is known to be whole.
    pub complete: bool,
}

/// A local telemetry database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the store at `path`, migrating it to this
    /// build's schema.
    ///
    /// Write-ahead logging is on: a reader — the CLI, an export —
    /// never blocks the game that is writing, and an unclean shutdown
    /// loses at most what had not been committed.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Store::prepare(conn)
    }

    /// An ephemeral store — what the tests and the benchmark use.
    pub fn open_in_memory() -> Result<Store> {
        Store::prepare(Connection::open_in_memory()?)
    }

    fn prepare(mut conn: Connection) -> Result<Store> {
        // WAL is a no-op on an in-memory database and rusqlite
        // reports the resulting journal mode as a row, so this is a
        // query rather than an execute.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        // `normal` under WAL syncs at checkpoints rather than at every
        // commit: the batch still lands, and a power cut costs the
        // tail rather than the file. Durability beyond that is the
        // operating system's to give, and this module does not claim
        // it (see the crate docs).
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::apply(&mut conn, schema::MIGRATIONS)?;
        Ok(Store { conn })
    }

    /// The schema version this store is at.
    pub fn version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Whether a session with this uid is already stored — what makes
    /// importing the same legacy file twice a no-op.
    pub fn has_uid(&self, uid: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM gameplay_session WHERE uid = ?1",
            params![uid],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Open a session and return its id.
    pub fn begin(&mut self, row: &SessionRow) -> Result<SessionId> {
        self.conn.execute(
            "INSERT INTO gameplay_session (
                uid, started_ms, title, artist, genre, chart_hash, chart_file,
                difficulty, player_slot, player_id,
                game_version, chart_format, generator_version, scoring_version,
                analysis_version, vocal_version, telemetry_schema, detail,
                input_device, input_offset_ms, video_offset_ms, mic_offset_ms,
                tap_mode, no_fail, practice, autopilot, notes_total
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10,
                ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22,
                ?23, ?24, ?25, ?26, ?27
             )",
            params![
                row.uid,
                row.started_ms as i64,
                row.title,
                row.artist,
                row.genre,
                row.chart_hash,
                row.chart_file,
                row.difficulty,
                row.player_slot,
                row.player_id.map(|id| id as i64),
                row.provenance.game,
                row.provenance.chart_format,
                row.provenance.generator,
                row.provenance.scoring,
                row.provenance.analysis,
                row.provenance.vocal,
                row.telemetry_schema,
                row.detail.code(),
                row.input_device.code(),
                row.input_offset_ms.map(f64::from),
                row.video_offset_ms.map(f64::from),
                row.mic_offset_ms.map(f64::from),
                row.tap_mode,
                row.no_fail,
                row.practice,
                row.autopilot,
                row.notes_total,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Append a batch of events in one transaction.
    ///
    /// One transaction per batch is the whole performance story: a
    /// commit per event would fsync per note.
    pub fn append(&mut self, session: SessionId, events: &[(u32, Event)]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR REPLACE INTO gameplay_event (
                    session_id, sequence, song_time_us, event_type,
                    note_index, action, delta_us, rating, value, flags
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for (sequence, event) in events {
                insert.execute(params![
                    session,
                    sequence,
                    event.song_time_us,
                    event.kind.code(),
                    event.note_index,
                    event.action.map(crate::model::Action::code),
                    event.delta_us,
                    event.rating.map(Rating::code),
                    event.value,
                    event.flags.0,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Close a session.
    ///
    /// `telemetry_complete` is derived here and nowhere else: a run
    /// that dropped an event is not a whole recording, and an
    /// analysis must never read a hole as a quiet passage.
    pub fn finish(&mut self, session: SessionId, outcome: Outcome) -> Result<()> {
        self.conn.execute(
            "UPDATE gameplay_session
                SET ended_ms = ?2, completion = ?3, dropped_events = ?4,
                    telemetry_complete = ?5
              WHERE session_id = ?1",
            params![
                session,
                outcome.ended_ms as i64,
                outcome.completion.code(),
                outcome.dropped,
                outcome.dropped == 0,
            ],
        )?;
        Ok(())
    }

    /// Record what the player said about a run.
    pub fn add_note(
        &mut self,
        session: SessionId,
        written_ms: u64,
        note: &PlayerNote,
    ) -> Result<()> {
        let (value, text): (Option<i64>, Option<String>) = match note {
            PlayerNote::Fun(score) => (Some(i64::from(*score)), None),
            PlayerNote::Comment(text) => (None, Some(text.clone())),
            PlayerNote::Versus { better, parent } => {
                (Some(i64::from(*better)), Some(parent.clone()))
            }
        };
        self.conn.execute(
            "INSERT INTO session_note (session_id, written_ms, kind, value, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session, written_ms as i64, note.kind(), value, text],
        )?;
        Ok(())
    }

    /// How many sessions are stored.
    pub fn session_count(&self) -> Result<u64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM gameplay_session", [], |row| {
                    row.get(0)
                })?;
        Ok(count.max(0) as u64)
    }

    /// How many events are stored.
    pub fn event_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM gameplay_event", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Every event of one session, in sequence order.
    ///
    /// Rows whose `event_type` this build does not know are skipped,
    /// not guessed at — the same rule the JSONL reader has always
    /// followed.
    pub fn events(&self, session: SessionId) -> Result<Vec<(u32, Event)>> {
        let mut statement = self.conn.prepare(
            "SELECT sequence, song_time_us, event_type, note_index, action,
                    delta_us, rating, value, flags
               FROM gameplay_event
              WHERE session_id = ?1
              ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![session], |row| {
            let sequence: u32 = row.get(0)?;
            let song_time_us: Option<i64> = row.get(1)?;
            let type_code: u8 = row.get(2)?;
            let note_index: Option<u32> = row.get(3)?;
            let action: Option<u8> = row.get(4)?;
            let delta_us: Option<i32> = row.get(5)?;
            let rating: Option<u8> = row.get(6)?;
            let value: Option<i64> = row.get(7)?;
            let flags: u16 = row.get(8)?;
            Ok(EventType::from_code(type_code).map(|kind| {
                (
                    sequence,
                    Event {
                        song_time_us,
                        kind,
                        note_index,
                        action: action.and_then(crate::model::Action::from_code),
                        delta_us,
                        rating: rating.and_then(Rating::from_code),
                        value,
                        flags: Flags(flags),
                    },
                )
            }))
        })?;
        let mut out = Vec::new();
        for row in rows {
            if let Some(event) = row? {
                out.push(event);
            }
        }
        Ok(out)
    }

    /// The player's notes on one session, oldest first.
    pub fn notes(&self, session: SessionId) -> Result<Vec<PlayerNote>> {
        let mut statement = self.conn.prepare(
            "SELECT kind, value, text FROM session_note
              WHERE session_id = ?1 ORDER BY written_ms, rowid",
        )?;
        let rows = statement.query_map(params![session], |row| {
            let kind: u8 = row.get(0)?;
            let value: Option<i64> = row.get(1)?;
            let text: Option<String> = row.get(2)?;
            Ok(match kind {
                0 => Some(PlayerNote::Fun(value.unwrap_or(0).clamp(0, 255) as u8)),
                1 => text.map(PlayerNote::Comment),
                2 => text.map(|parent| PlayerNote::Versus {
                    better: value.unwrap_or(0) != 0,
                    parent,
                }),
                _ => None,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            if let Some(note) = row? {
                out.push(note);
            }
        }
        Ok(out)
    }

    /// One session by id.
    pub fn session(&self, id: SessionId) -> Result<Option<StoredSession>> {
        let mut statement = self
            .conn
            .prepare(&format!("{SESSION_COLUMNS} WHERE session_id = ?1"))?;
        let mut rows = statement.query_map(params![id], read_session)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Every session, newest first, capped at `limit`.
    pub fn sessions(&self, limit: usize) -> Result<Vec<StoredSession>> {
        let mut statement = self.conn.prepare(&format!(
            "{SESSION_COLUMNS} ORDER BY started_ms DESC, session_id DESC LIMIT ?1"
        ))?;
        let rows = statement.query_map(params![limit as i64], read_session)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Run a read-only query and hand the rows to a closure.
    ///
    /// The escape hatch the analytics module and the benchmark use so
    /// that neither has to own a connection. Writes are rejected by
    /// SQLite itself inside a read-only statement preparation.
    pub fn query<T>(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
        mut row: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> Result<Vec<T>> {
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement.query_map(params, |r| row(r))?;
        let mut out = Vec::new();
        for value in rows {
            out.push(value?);
        }
        Ok(out)
    }

    /// The file size SQLite reports for this database, in bytes.
    /// What the debug view shows and the benchmark measures.
    pub fn size_bytes(&self) -> Result<u64> {
        let pages: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok((pages.max(0) as u64) * (page_size.max(0) as u64))
    }

    /// Hand out the connection for a caller that needs raw SQL —
    /// the benchmark's bulk loader and nothing else in the game.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// The column list every session read shares.
const SESSION_COLUMNS: &str = "SELECT session_id, uid, started_ms, ended_ms, title, artist, genre, \
     chart_hash, chart_file, difficulty, player_slot, player_id, game_version, \
     chart_format, generator_version, scoring_version, analysis_version, \
     vocal_version, telemetry_schema, detail, input_device, input_offset_ms, \
     video_offset_ms, mic_offset_ms, tap_mode, no_fail, practice, autopilot, \
     completion, notes_total, dropped_events, telemetry_complete \
     FROM gameplay_session";

#[allow(clippy::too_many_lines)] // one line per column; splitting it would hide the mapping
fn read_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSession> {
    let started_ms: i64 = row.get(2)?;
    let ended_ms: Option<i64> = row.get(3)?;
    let player_id: Option<i64> = row.get(11)?;
    let input_offset_ms: Option<f64> = row.get(21)?;
    let video_offset_ms: Option<f64> = row.get(22)?;
    let mic_offset_ms: Option<f64> = row.get(23)?;
    let completion: u8 = row.get(28)?;
    let detail: u8 = row.get(19)?;
    let device: u8 = row.get(20)?;
    Ok(StoredSession {
        id: row.get(0)?,
        row: SessionRow {
            uid: row.get(1)?,
            started_ms: started_ms.max(0) as u64,
            title: row.get(4)?,
            artist: row.get(5)?,
            genre: row.get(6)?,
            chart_hash: row.get(7)?,
            chart_file: row.get(8)?,
            difficulty: row.get(9)?,
            player_slot: row.get(10)?,
            player_id: player_id.map(|id| id as u64),
            provenance: Provenance {
                game: row.get(12)?,
                chart_format: row.get(13)?,
                generator: row.get(14)?,
                scoring: row.get(15)?,
                analysis: row.get(16)?,
                vocal: row.get(17)?,
            },
            telemetry_schema: row.get(18)?,
            detail: Detail::from_code(detail),
            input_device: InputDevice::from_code(device),
            input_offset_ms: input_offset_ms.map(|value| value as f32),
            video_offset_ms: video_offset_ms.map(|value| value as f32),
            mic_offset_ms: mic_offset_ms.map(|value| value as f32),
            tap_mode: row.get(24)?,
            no_fail: row.get(25)?,
            practice: row.get(26)?,
            autopilot: row.get(27)?,
            notes_total: row.get(29)?,
        },
        ended_ms: ended_ms.map(|value| value.max(0) as u64),
        completion: Completion::from_code(completion),
        dropped: row.get(30)?,
        complete: row.get(31)?,
    })
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("sessions", &self.session_count().unwrap_or(0))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Rating, micros};
    use beatbyte_core::Lane;

    pub(crate) fn a_session(uid: &str) -> SessionRow {
        SessionRow {
            uid: uid.to_owned(),
            started_ms: 1_700_000_000_000,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            genre: Some("rock".to_owned()),
            chart_hash: "abc123".to_owned(),
            chart_file: Some("chart.v3.json".to_owned()),
            difficulty: 1,
            player_slot: 0,
            player_id: Some(7),
            provenance: Provenance {
                game: "0.18.0".to_owned(),
                chart_format: 1,
                generator: Some("claude".to_owned()),
                scoring: 1,
                analysis: Some(2),
                vocal: None,
            },
            telemetry_schema: schema::schema_version(),
            detail: Detail::Actions,
            input_device: InputDevice::Keyboard,
            input_offset_ms: Some(-12.5),
            video_offset_ms: Some(0.0),
            mic_offset_ms: None,
            tap_mode: true,
            no_fail: true,
            practice: false,
            autopilot: false,
            notes_total: 328,
        }
    }

    #[test]
    fn a_session_comes_back_exactly_as_it_went_in() {
        let mut store = Store::open_in_memory().expect("a store");
        let row = a_session("one");
        let id = store.begin(&row).expect("begins");
        let back = store.session(id).expect("reads").expect("is there");
        assert_eq!(back.row, row, "every field of a session survives the trip");
        assert_eq!(back.ended_ms, None, "an open session has not ended");
        assert_eq!(back.completion, Completion::Unknown);
        assert!(back.complete, "nothing dropped yet");
    }

    #[test]
    fn events_come_back_in_order_and_unchanged() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        let events = vec![
            (
                1,
                Event::new(EventType::Action, micros(1.0)).acting(Action::FretDown(Lane::Three)),
            ),
            (
                2,
                Event::new(EventType::NoteHit, micros(1.002))
                    .about(17)
                    .off_by(0.002)
                    .judged(Rating::Perfect)
                    .flagged(Flags::CHORD),
            ),
            (3, Event::new(EventType::Paused, micros(4.0))),
        ];
        store.append(id, &events).expect("appends");
        let back = store.events(id).expect("reads");
        assert_eq!(back, events, "an event is what was written");
        assert_eq!(store.event_count().expect("counts"), 3);
    }

    #[test]
    fn a_gap_in_the_sequence_survives_because_it_is_the_evidence() {
        // A dropped event leaves a hole in the numbering. That hole is
        // how a reader sees the drop at the exact place it happened,
        // so nothing may renumber it away.
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        store
            .append(
                id,
                &[
                    (1, Event::new(EventType::Overstrum, micros(1.0))),
                    (9, Event::new(EventType::Overstrum, micros(2.0))),
                ],
            )
            .expect("appends");
        let sequences: Vec<u32> = store
            .events(id)
            .expect("reads")
            .iter()
            .map(|(sequence, _)| *sequence)
            .collect();
        assert_eq!(sequences, vec![1, 9]);
    }

    #[test]
    fn finishing_a_run_that_dropped_events_marks_it_incomplete() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        store
            .finish(
                id,
                Outcome {
                    ended_ms: 1_700_000_200_000,
                    completion: Completion::Completed,
                    dropped: 3,
                },
            )
            .expect("finishes");
        let back = store.session(id).expect("reads").expect("is there");
        assert_eq!(back.dropped, 3);
        assert!(
            !back.complete,
            "a recording with a hole in it must never read as whole"
        );
        assert_eq!(back.completion, Completion::Completed);
        assert_eq!(back.ended_ms, Some(1_700_000_200_000));
    }

    #[test]
    fn an_unknown_event_type_is_skipped_not_guessed() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        store
            .append(id, &[(1, Event::new(EventType::NoteMiss, micros(1.0)))])
            .expect("appends");
        // A row a later build wrote.
        store
            .connection()
            .execute(
                "INSERT INTO gameplay_event (session_id, sequence, song_time_us, event_type, flags)
                 VALUES (?1, 2, 2000000, 250, 0)",
                params![id],
            )
            .expect("a future row goes in");
        let back = store.events(id).expect("reads");
        assert_eq!(back.len(), 1, "the unknown row is skipped");
        assert_eq!(back[0].1.kind, EventType::NoteMiss);
        assert_eq!(
            store.event_count().expect("counts"),
            2,
            "…and it is still on disk, for the build that understands it"
        );
    }

    #[test]
    fn a_uid_is_claimed_once() {
        let mut store = Store::open_in_memory().expect("a store");
        store.begin(&a_session("same")).expect("begins");
        assert!(store.has_uid("same").expect("asks"));
        assert!(!store.has_uid("other").expect("asks"));
        assert!(
            store.begin(&a_session("same")).is_err(),
            "importing the same run twice must not double its evidence"
        );
    }

    #[test]
    fn player_notes_keep_their_kind_and_their_words() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        for note in [
            PlayerNote::Fun(4),
            PlayerNote::Comment("the chorus drags".to_owned()),
            PlayerNote::Versus {
                better: true,
                parent: "old-hash".to_owned(),
            },
        ] {
            store.add_note(id, 1_700_000_300_000, &note).expect("adds");
        }
        let back = store.notes(id).expect("reads");
        assert_eq!(back.len(), 3);
        assert_eq!(back[0], PlayerNote::Fun(4));
        assert_eq!(back[1], PlayerNote::Comment("the chorus drags".to_owned()));
        assert_eq!(
            back[2],
            PlayerNote::Versus {
                better: true,
                parent: "old-hash".to_owned()
            }
        );
    }

    #[test]
    fn events_cannot_outlive_the_session_they_belong_to() {
        let mut store = Store::open_in_memory().expect("a store");
        let id = store.begin(&a_session("one")).expect("begins");
        store
            .append(id, &[(1, Event::new(EventType::NoteMiss, 0))])
            .expect("appends");
        // Foreign keys on: an event for a session that does not exist
        // is refused rather than becoming an orphan nothing can join.
        assert!(
            store
                .append(id + 999, &[(1, Event::new(EventType::NoteMiss, 0))])
                .is_err(),
            "an event without a session is evidence about nothing"
        );
        store
            .connection()
            .execute(
                "DELETE FROM gameplay_session WHERE session_id = ?1",
                params![id],
            )
            .expect("deletes");
        assert_eq!(
            store.event_count().expect("counts"),
            0,
            "and deleting a session takes its events with it"
        );
    }

    #[test]
    fn sessions_come_back_newest_first() {
        let mut store = Store::open_in_memory().expect("a store");
        let mut older = a_session("older");
        older.started_ms = 1_000;
        let mut newer = a_session("newer");
        newer.started_ms = 2_000;
        store.begin(&older).expect("begins");
        store.begin(&newer).expect("begins");
        let uids: Vec<String> = store
            .sessions(10)
            .expect("lists")
            .into_iter()
            .map(|session| session.row.uid)
            .collect();
        assert_eq!(uids, vec!["newer".to_owned(), "older".to_owned()]);
        assert_eq!(store.session_count().expect("counts"), 2);
    }

    #[test]
    fn a_store_on_disk_is_still_there_when_it_is_opened_again() {
        let dir =
            std::env::temp_dir().join(format!("beatbyte-telemetry-reopen-{}", std::process::id()));
        let path = dir.join("telemetry.db");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut store = Store::open(&path).expect("creates");
            let id = store.begin(&a_session("kept")).expect("begins");
            store
                .append(id, &[(1, Event::new(EventType::NoteMiss, micros(3.0)))])
                .expect("appends");
        }
        let store = Store::open(&path).expect("reopens");
        assert_eq!(store.session_count().expect("counts"), 1);
        assert_eq!(store.event_count().expect("counts"), 1);
        assert_eq!(store.version().expect("version"), schema::schema_version());
        assert!(store.size_bytes().expect("size") > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
