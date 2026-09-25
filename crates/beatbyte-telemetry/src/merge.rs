//! Merging another device's store into this one — rows, never files
//! (ADR-0021).
//!
//! A live SQLite file with its write-ahead log cannot be copied: the
//! copy is whatever half-state the pages were in. So a device
//! publishes a [`snapshot`] made with `VACUUM INTO` (consistent by
//! construction, and readable without the WAL), and the receiving
//! device copies ROWS out of it:
//!
//! - which sessions to take is decided elsewhere, by `uid`
//!   (`beatbyte-sync::telemetry::plan`); this module only lists what
//!   each store holds ([`summaries`]) and carries a plan out
//!   ([`apply`]);
//! - a session is inserted under a NEW local `session_id`, its events
//!   and notes renumbered to it in the same transaction;
//! - a session being replaced loses its old events and notes first;
//! - `note_context` merges by its primary key — one chart moment has
//!   one context, whichever device wrote it;
//! - player ids follow the sync's remaps: the LOCAL remap rewrites
//!   rows already here, the REMOTE remap rewrites rows on their way
//!   in; song ids follow the song remap everywhere.
//!
//! One transaction for the lot: a merge that fails half way leaves the
//! store as it was.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use crate::Result;
use crate::store::Store;

/// What a plan needs to know about one stored session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The session's uid.
    pub uid: String,
    /// Whether it ended.
    pub ended: bool,
    /// How many events it holds.
    pub events: u64,
}

/// One session to take from the other store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    /// Which.
    pub uid: String,
    /// Whether a local copy exists and is to be replaced.
    pub replace: bool,
}

/// The id remaps a merge applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Remaps {
    /// Player ids of rows already in this store.
    pub local_players: BTreeMap<u64, u64>,
    /// Player ids of rows coming in.
    pub remote_players: BTreeMap<u64, u64>,
    /// Song ids, both sides.
    pub songs: BTreeMap<String, String>,
}

/// What a merge did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Sessions inserted.
    pub inserted: usize,
    /// Sessions replaced by a more complete copy.
    pub replaced: usize,
    /// Events copied.
    pub events: u64,
    /// Note contexts that were new here.
    pub contexts: u64,
    /// Local rows whose player id was rewritten.
    pub players_rewritten: usize,
}

/// A consistent copy of the store at `live`, written to `out` (which
/// must not exist). `VACUUM INTO` reads through the write-ahead log,
/// so the copy holds everything committed, and needs only a read
/// handle — a running game is not disturbed.
///
/// # Errors
/// When the store cannot be read or `out` cannot be written.
pub fn snapshot(live: &Path, out: &Path) -> Result<()> {
    let store = Store::open_readonly(live)?;
    store
        .connection()
        .execute("VACUUM INTO ?1", params![out.to_string_lossy()])?;
    Ok(())
}

/// Every session a store holds, for the plan.
///
/// # Errors
/// When the store cannot be read.
pub fn summaries(store: &Store) -> Result<Vec<Summary>> {
    let mut statement = store.connection().prepare(
        "SELECT s.uid, s.ended_ms IS NOT NULL,
                (SELECT COUNT(*) FROM gameplay_event e WHERE e.session_id = s.session_id)
           FROM gameplay_session s ORDER BY s.session_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Summary {
            uid: row.get(0)?,
            ended: row.get(1)?,
            events: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The columns of a table, in order, as this build's schema has them.
fn columns(store: &Store, table: &str) -> Result<Vec<String>> {
    let mut statement = store
        .connection()
        .prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    Ok(names.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// `UPDATE … SET column = CASE column WHEN a THEN b … END` for a whole
/// remap in ONE statement — sequential updates would chain `a→b, b→c`
/// into `a→c`.
fn remap_sql(table: &str, column: &str, pairs: usize, extra_where: &str) -> String {
    let whens: String = (0..pairs)
        .map(|i| format!(" WHEN ?{} THEN ?{}", 2 * i + 1, 2 * i + 2))
        .collect();
    let ins: Vec<String> = (0..pairs).map(|i| format!("?{}", 2 * i + 1)).collect();
    format!(
        "UPDATE {table} SET {column} = CASE {column}{whens} ELSE {column} END \
         WHERE {column} IN ({}){extra_where}",
        ins.join(",")
    )
}

/// Carry `takes` out of the store at `other` into `store`, applying
/// `remaps`. The other store must be at this build's schema — open it
/// with [`Store::open`] on a copy first, which migrates it.
///
/// # Errors
/// On any database error; nothing is changed then.
#[allow(clippy::too_many_lines)] // one transaction, read top to bottom
pub fn apply(store: &mut Store, other: &Path, takes: &[Take], remaps: &Remaps) -> Result<Report> {
    let session_cols: Vec<String> = columns(store, "gameplay_session")?
        .into_iter()
        .filter(|c| c != "session_id")
        .collect();
    let event_cols: Vec<String> = columns(store, "gameplay_event")?
        .into_iter()
        .filter(|c| c != "session_id")
        .collect();
    let note_cols: Vec<String> = columns(store, "session_note")?
        .into_iter()
        .filter(|c| c != "session_id")
        .collect();
    let mut report = Report::default();
    let conn = store.connection_mut();
    conn.execute(
        "ATTACH DATABASE ?1 AS remote",
        params![other.to_string_lossy()],
    )?;
    let result = (|| -> Result<()> {
        let tx = conn.transaction()?;
        // 1. The local remap, on rows already here, in one statement.
        if !remaps.local_players.is_empty() {
            let sql = remap_sql(
                "main.gameplay_session",
                "player_id",
                remaps.local_players.len(),
                "",
            );
            let values: Vec<i64> = remaps
                .local_players
                .iter()
                .flat_map(|(a, b)| [*a as i64, *b as i64])
                .collect();
            report.players_rewritten =
                tx.execute(&sql, rusqlite::params_from_iter(values.iter()))?;
        }
        // 2. The sessions, with their events and notes.
        let session_list = session_cols.join(", ");
        for take in takes {
            let Some(remote_id): Option<i64> = tx
                .query_row(
                    "SELECT session_id FROM remote.gameplay_session WHERE uid = ?1",
                    params![take.uid],
                    |row| row.get(0),
                )
                .optional()?
            else {
                continue;
            };
            if take.replace {
                let local_id: Option<i64> = tx
                    .query_row(
                        "SELECT session_id FROM main.gameplay_session WHERE uid = ?1",
                        params![take.uid],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(local_id) = local_id {
                    tx.execute(
                        "DELETE FROM main.gameplay_event WHERE session_id = ?1",
                        params![local_id],
                    )?;
                    tx.execute(
                        "DELETE FROM main.session_note WHERE session_id = ?1",
                        params![local_id],
                    )?;
                    tx.execute(
                        "DELETE FROM main.gameplay_session WHERE session_id = ?1",
                        params![local_id],
                    )?;
                }
            }
            tx.execute(
                &format!(
                    "INSERT INTO main.gameplay_session ({session_list}) \
                     SELECT {session_list} FROM remote.gameplay_session WHERE session_id = ?1"
                ),
                params![remote_id],
            )?;
            let new_id = tx.last_insert_rowid();
            let event_list = event_cols.join(", ");
            report.events += tx.execute(
                &format!(
                    "INSERT INTO main.gameplay_event (session_id, {event_list}) \
                     SELECT ?1, {event_list} FROM remote.gameplay_event WHERE session_id = ?2"
                ),
                params![new_id, remote_id],
            )? as u64;
            let note_list = note_cols.join(", ");
            tx.execute(
                &format!(
                    "INSERT INTO main.session_note (session_id, {note_list}) \
                     SELECT ?1, {note_list} FROM remote.session_note WHERE session_id = ?2"
                ),
                params![new_id, remote_id],
            )?;
            // The remote remap, on this row only.
            if !remaps.remote_players.is_empty() {
                let sql = remap_sql(
                    "main.gameplay_session",
                    "player_id",
                    remaps.remote_players.len(),
                    &format!(" AND session_id = {new_id}"),
                );
                let values: Vec<i64> = remaps
                    .remote_players
                    .iter()
                    .flat_map(|(a, b)| [*a as i64, *b as i64])
                    .collect();
                tx.execute(&sql, rusqlite::params_from_iter(values.iter()))?;
            }
            if take.replace {
                report.replaced += 1;
            } else {
                report.inserted += 1;
            }
        }
        // 3. Chart contexts by their key.
        report.contexts = tx.execute(
            "INSERT OR IGNORE INTO main.note_context SELECT * FROM remote.note_context",
            [],
        )? as u64;
        // 4. Song ids, everywhere.
        for (from, to) in &remaps.songs {
            tx.execute(
                "UPDATE main.gameplay_session SET song_id = ?2 WHERE song_id = ?1",
                params![from, to],
            )?;
        }
        tx.commit()?;
        Ok(())
    })();
    conn.execute("DETACH DATABASE remote", [])?;
    result.map(|()| report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Completion, Event, EventType, Outcome};
    use crate::store::tests::a_session;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bb-tmerge-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// A store with the given sessions, each with `events` events.
    fn store_with(path: &Path, sessions: &[(&str, u64, u32, bool)]) -> Store {
        let mut store = Store::open(path).expect("a store");
        for (uid, player, events, ended) in sessions {
            let mut row = a_session(uid);
            row.player_id = Some(*player);
            let id = store.begin(&row).expect("begins");
            let batch: Vec<(u32, Event)> = (0..*events)
                .map(|i| (i, Event::new(EventType::NoteHit, i64::from(i) * 1_000)))
                .collect();
            store.append(id, &batch).expect("appends");
            if *ended {
                store
                    .finish(
                        id,
                        Outcome {
                            ended_ms: 1,
                            completion: Completion::Completed,
                            dropped: 0,
                            practice: false,
                        },
                    )
                    .expect("finishes");
            }
        }
        store
    }

    fn takes(uids: &[(&str, bool)]) -> Vec<Take> {
        uids.iter()
            .map(|(uid, replace)| Take {
                uid: (*uid).to_owned(),
                replace: *replace,
            })
            .collect()
    }

    /// Sessions come in with their events, under new local ids, and the
    /// snapshot of a live store is readable on its own.
    #[test]
    fn a_session_from_the_other_device_arrives_with_its_events() {
        let dir = scratch("insert");
        let mut here = store_with(&dir.join("here.db"), &[("a", 1, 3, true)]);
        let _there = store_with(
            &dir.join("there.db"),
            &[("b", 1, 5, true), ("c", 1, 2, true)],
        );
        snapshot(&dir.join("there.db"), &dir.join("snap.db")).expect("a snapshot");
        let report = apply(
            &mut here,
            &dir.join("snap.db"),
            &takes(&[("b", false), ("c", false)]),
            &Remaps::default(),
        )
        .expect("merged");
        assert_eq!(report.inserted, 2);
        assert_eq!(report.events, 7);
        assert_eq!(here.session_count().expect("count"), 3);
        assert_eq!(here.event_count().expect("count"), 10);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A replaced session loses its old events: no duplicates.
    #[test]
    fn a_replaced_session_does_not_keep_its_old_events() {
        let dir = scratch("replace");
        let mut here = store_with(&dir.join("here.db"), &[("a", 1, 3, false)]);
        let _there = store_with(&dir.join("there.db"), &[("a", 1, 8, true)]);
        let report = apply(
            &mut here,
            &dir.join("there.db"),
            &takes(&[("a", true)]),
            &Remaps::default(),
        )
        .expect("merged");
        assert_eq!(report.replaced, 1);
        assert_eq!(here.session_count().expect("count"), 1);
        assert_eq!(
            here.event_count().expect("count"),
            8,
            "old events left behind"
        );
        let s = summaries(&here).expect("summaries");
        assert!(s[0].ended);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// ⚠️ Remaps in ONE statement: `2→3` and `3→9` must not chain local
    /// rows of player 2 into 9. And incoming rows get the remote map.
    #[test]
    fn remaps_apply_to_the_right_side_and_never_chain() {
        let dir = scratch("remap");
        let mut here = store_with(
            &dir.join("here.db"),
            &[("p2", 2, 1, true), ("p3", 3, 1, true)],
        );
        let _there = store_with(&dir.join("there.db"), &[("r", 2, 1, true)]);
        let mut remaps = Remaps::default();
        remaps.local_players.insert(2, 3);
        remaps.local_players.insert(3, 9);
        remaps.remote_players.insert(2, 50);
        apply(
            &mut here,
            &dir.join("there.db"),
            &takes(&[("r", false)]),
            &remaps,
        )
        .expect("merged");
        let player_of = |uid: &str| -> i64 {
            here.connection()
                .query_row(
                    "SELECT player_id FROM gameplay_session WHERE uid = ?1",
                    params![uid],
                    |r| r.get(0),
                )
                .expect("a row")
        };
        assert_eq!(player_of("p2"), 3, "chained or not applied");
        assert_eq!(player_of("p3"), 9);
        assert_eq!(player_of("r"), 50, "the incoming row kept the remote id");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A merge applied twice (a second sync) inserts nothing: the plan
    /// sees every uid already here.
    #[test]
    fn summaries_let_a_second_merge_find_nothing_to_do() {
        let dir = scratch("again");
        let mut here = store_with(&dir.join("here.db"), &[]);
        let _there = store_with(&dir.join("there.db"), &[("x", 1, 2, true)]);
        apply(
            &mut here,
            &dir.join("there.db"),
            &takes(&[("x", false)]),
            &Remaps::default(),
        )
        .expect("merged");
        let mine = summaries(&here).expect("here");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].events, 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
