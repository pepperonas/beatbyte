//! The store's tables, and the migration runner that installs them.
//!
//! # How a schema moves
//!
//! Migrations are an ordered list of SQL steps. SQLite's own
//! `user_version` pragma counts how many have been applied, so opening
//! a store is: read the number, run the rest, write the new number —
//! inside one transaction per step, so a crash mid-migration leaves
//! either the old schema or the new one and never half of each.
//!
//! **A shipped step is never edited.** Changing one would leave every
//! machine that already ran it with a different schema under the same
//! number, which is the one failure mode a version counter exists to
//! prevent. New shape, new step at the end.
//!
//! # Why a session's facts are columns and an event's are not
//!
//! A session row is written once per song and read by every query, so
//! it spells everything out. An event row is written a thousand times
//! per song and carries only what cannot be joined: which note, how
//! far off, how it was judged. The song's tempo, genre, section and
//! onset strength are NOT here — they belong to the chart and the
//! analysis, and duplicating them per event would trade the whole
//! storage budget for data that was already on disk.

use rusqlite::Connection;

/// The schema version a fresh store is created at — the number of
/// migrations this build carries.
///
/// ⚠️ Not [`beatbyte_core::telemetry::SCHEMA_VERSION`]. That one
/// versions the older per-session JSONL format, which this store
/// imports and eventually replaces; the two count different things
/// and are free to disagree.
#[must_use]
pub fn schema_version() -> u32 {
    MIGRATIONS.len() as u32
}

/// One numbered step of the schema's history.
pub struct Migration {
    /// What it does, for the log and for a human reading the list.
    pub name: &'static str,
    /// The statements, run as one script inside one transaction.
    pub sql: &'static str,
}

/// The schema's history, oldest first. Append only.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "sessions, events and player notes",
        sql: M1,
    },
    Migration {
        name: "what the analysis said at each chart note",
        sql: M2,
    },
    Migration {
        name: "the song a session belongs to",
        sql: M3,
    },
];

/// v1 — the whole store as first shipped.
const M1: &str = r"
CREATE TABLE gameplay_session (
    session_id         INTEGER PRIMARY KEY,
    uid                TEXT    NOT NULL UNIQUE,
    started_ms         INTEGER NOT NULL,
    ended_ms           INTEGER,
    title              TEXT    NOT NULL,
    artist             TEXT    NOT NULL,
    genre              TEXT,
    chart_hash         TEXT    NOT NULL,
    chart_file         TEXT,
    difficulty         INTEGER NOT NULL,
    player_slot        INTEGER NOT NULL,
    player_id          INTEGER,
    game_version       TEXT    NOT NULL,
    chart_format       INTEGER NOT NULL,
    generator_version  TEXT,
    scoring_version    INTEGER NOT NULL,
    analysis_version   INTEGER,
    vocal_version      INTEGER,
    telemetry_schema   INTEGER NOT NULL,
    detail             INTEGER NOT NULL,
    input_device       INTEGER NOT NULL,
    input_offset_ms    REAL,
    video_offset_ms    REAL,
    mic_offset_ms      REAL,
    tap_mode           INTEGER NOT NULL,
    no_fail            INTEGER NOT NULL,
    practice           INTEGER NOT NULL,
    autopilot          INTEGER NOT NULL,
    completion         INTEGER NOT NULL DEFAULT 0,
    notes_total        INTEGER NOT NULL,
    dropped_events     INTEGER NOT NULL DEFAULT 0,
    telemetry_complete INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE gameplay_event (
    session_id   INTEGER NOT NULL REFERENCES gameplay_session(session_id) ON DELETE CASCADE,
    sequence     INTEGER NOT NULL,
    song_time_us INTEGER,
    event_type   INTEGER NOT NULL,
    note_index   INTEGER,
    action       INTEGER,
    delta_us     INTEGER,
    rating       INTEGER,
    value        INTEGER,
    flags        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, sequence)
) WITHOUT ROWID;

CREATE TABLE session_note (
    session_id INTEGER NOT NULL REFERENCES gameplay_session(session_id) ON DELETE CASCADE,
    written_ms INTEGER NOT NULL,
    kind       INTEGER NOT NULL,
    value      INTEGER,
    text       TEXT
);

CREATE INDEX ix_event_note ON gameplay_event(session_id, note_index)
    WHERE note_index IS NOT NULL;
CREATE INDEX ix_event_type ON gameplay_event(event_type, session_id);
CREATE INDEX ix_sess_chart ON gameplay_session(chart_hash, difficulty, started_ms);
CREATE INDEX ix_sess_song  ON gameplay_session(title, artist, started_ms);
CREATE INDEX ix_sess_gen   ON gameplay_session(generator_version, chart_hash);
CREATE INDEX ix_note_session ON session_note(session_id);
";

/// v2 — the musical context of each chart note (ADR-0018 §20).
///
/// Keyed by `(chart_hash, difficulty, note_index)`, which is exactly
/// the reference a gameplay event carries, so the join needs nothing
/// invented. It is chart data, not telemetry: replaced wholesale when
/// a chart is, and deleted with nothing.
const M2: &str = r"
CREATE TABLE note_context (
    chart_hash TEXT    NOT NULL,
    difficulty INTEGER NOT NULL,
    note_index INTEGER NOT NULL,
    onset      INTEGER NOT NULL,
    energy     INTEGER NOT NULL,
    brightness INTEGER NOT NULL,
    bar_phase  INTEGER NOT NULL,
    repeat_id  INTEGER NOT NULL,
    flags      INTEGER NOT NULL,
    PRIMARY KEY (chart_hash, difficulty, note_index)
) WITHOUT ROWID;
";

/// v3 — which SONG a session belongs to (ADR-0019).
///
/// ⚠️ The store already carries `chart_hash`, and that is the right
/// identity for a CHART: it changes with every redesign, which is
/// what makes "did this version play better" answerable. It is the
/// wrong identity for a song, and until now it was the only one
/// there. Measured on the author's library at the time this was
/// added: 564 recorded sessions, of which **219** could be joined
/// back to a song that still exists — *Maria* alone is spread over
/// nine hashes. Sixty-one per cent of a play history that cannot
/// answer "how often have I played this song".
///
/// Nullable, and it stays nullable: a session recorded before the
/// library had documents, or one whose song has since been deleted,
/// has no song id, and writing an invented one would be worse than
/// the gap.
const M3: &str = r"
ALTER TABLE gameplay_session ADD COLUMN song_id TEXT;
CREATE INDEX ix_sess_song_id ON gameplay_session(song_id, started_ms)
    WHERE song_id IS NOT NULL;
";

/// Run every migration the connection has not seen yet, and report
/// the version it ends on.
///
/// Split from [`MIGRATIONS`] so a test can drive it with a different
/// list — which is the only way to prove that an *older* store
/// survives a *newer* build, the property the whole mechanism exists
/// for.
pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> rusqlite::Result<u32> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, migration) in migrations.iter().enumerate().skip(current as usize) {
        let next = index as u32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        // A pragma takes no parameters, hence the format; `next` is a
        // loop counter, not input.
        tx.pragma_update(None, "user_version", next)?;
        tx.commit()?;
    }
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        Connection::open_in_memory().expect("an in-memory database")
    }

    #[test]
    fn a_fresh_store_ends_on_the_version_this_build_carries() {
        let mut conn = fresh();
        let version = apply(&mut conn, MIGRATIONS).expect("migrates");
        assert_eq!(version, schema_version());
        assert!(schema_version() >= 1, "there is at least one migration");
    }

    #[test]
    fn migrating_twice_changes_nothing() {
        let mut conn = fresh();
        apply(&mut conn, MIGRATIONS).expect("migrates");
        conn.execute(
            "INSERT INTO gameplay_session (uid, started_ms, title, artist, chart_hash, \
             difficulty, player_slot, game_version, chart_format, scoring_version, \
             telemetry_schema, detail, input_device, tap_mode, no_fail, practice, \
             autopilot, notes_total) \
             VALUES ('u', 1, 't', 'a', 'h', 1, 0, '0.0.0', 1, 1, 1, 1, 1, 1, 1, 0, 0, 10)",
            [],
        )
        .expect("a row goes in");
        let version = apply(&mut conn, MIGRATIONS).expect("migrates again");
        assert_eq!(version, schema_version());
        let rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM gameplay_session", [], |row| {
                row.get(0)
            })
            .expect("counts");
        assert_eq!(rows, 1, "a second migration run must not touch the data");
    }

    /// The property the version counter exists for: a store written by
    /// an older build, opened by a newer one, keeps its rows and
    /// gains the new shape.
    #[test]
    fn an_older_store_survives_a_newer_build() {
        let mut conn = fresh();
        apply(&mut conn, MIGRATIONS).expect("the old build's migrations");
        conn.execute(
            "INSERT INTO gameplay_session (uid, started_ms, title, artist, chart_hash, \
             difficulty, player_slot, game_version, chart_format, scoring_version, \
             telemetry_schema, detail, input_device, tap_mode, no_fail, practice, \
             autopilot, notes_total) \
             VALUES ('old', 1, 'Maria', 'Blondie', 'h', 1, 0, '0.1.0', 1, 1, 1, 1, 1, \
             1, 1, 0, 0, 328)",
            [],
        )
        .expect("the old build wrote a session");

        // A later build with one more step.
        let mut later: Vec<Migration> = Vec::new();
        for migration in MIGRATIONS {
            later.push(Migration {
                name: migration.name,
                sql: migration.sql,
            });
        }
        later.push(Migration {
            name: "a later build adds a column",
            sql: "ALTER TABLE gameplay_session ADD COLUMN room_lights INTEGER",
        });

        let version = apply(&mut conn, &later).expect("the newer build migrates");
        assert_eq!(version, schema_version() + 1);
        let (title, lights): (String, Option<i64>) = conn
            .query_row(
                "SELECT title, room_lights FROM gameplay_session WHERE uid = 'old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the old row is still there");
        assert_eq!(title, "Maria", "history is not thrown away by an upgrade");
        assert_eq!(lights, None, "and it reads null in the new column");
    }

    #[test]
    fn a_failed_step_leaves_the_version_where_it_was() {
        let mut conn = fresh();
        let broken = [
            Migration {
                name: "good",
                sql: "CREATE TABLE a (x INTEGER)",
            },
            Migration {
                name: "broken",
                sql: "CREATE TABLE b (x INTEGER); THIS IS NOT SQL",
            },
        ];
        assert!(apply(&mut conn, &broken).is_err(), "the bad step fails");
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("reads the version");
        assert_eq!(version, 1, "the good step stuck; the bad one did not");
        let half: rusqlite::Result<u32> =
            conn.query_row("SELECT COUNT(*) FROM b", [], |row| row.get(0));
        assert!(
            half.is_err(),
            "a step is one transaction: no half-applied table may survive it"
        );
    }
}
