//! The queryable projection of a library.
//!
//! `library.db` exists so that "Deep House between 115 and 125 BPM
//! that I have never played" is an index scan instead of a hundred
//! and seventy file parses. It holds **nothing of its own**: every
//! row is derived from a [`SongDoc`] in a song folder, and
//! [`Index::rebuild`] reproduces the whole database from those
//! documents.
//!
//! That is the load-bearing property of ADR-0019, and it is pinned by
//! a test rather than promised: an index that quietly became the only
//! copy of something would make a deleted database a data loss, and
//! the whole point is that it is not.
//!
//! ## Why a second database rather than a table in `telemetry.db`
//!
//! The telemetry store is evidence the game never reads back and that
//! a player may discard wholesale (ADR-0018). This is the opposite:
//! authoritative-by-projection data the game reads constantly. Same
//! technology, different lifetimes — and the migration runner over
//! there was already written to take an arbitrary list, so a second
//! schema costs almost nothing.

use std::path::Path;

use rusqlite::{Connection, params};

use crate::doc::{Instrument, SongDoc};

/// One numbered step of the index schema's history.
pub struct Migration {
    /// What it does, for a human reading the list.
    pub name: &'static str,
    /// The statements, run as one script inside one transaction.
    pub sql: &'static str,
}

/// The schema's history, oldest first. **Append only** — a shipped
/// migration is never edited, because somebody's database has
/// already run it.
pub const MIGRATIONS: &[Migration] = &[Migration {
    name: "songs, their genres and their charts",
    sql: V1,
}];

/// v1 — the projection as first shipped.
const V1: &str = r"
CREATE TABLE song (
    song_id       TEXT    PRIMARY KEY,
    folder        TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    artist        TEXT,
    album         TEXT,
    release_year  INTEGER,
    language      TEXT,
    duration_s    REAL,
    bpm           REAL,
    key_tonic     INTEGER,
    key_minor     INTEGER,
    energy        REAL,
    loudness_lufs REAL,
    has_lyrics    INTEGER NOT NULL DEFAULT 0,
    has_vocals    INTEGER,
    chart_hash    TEXT,
    chart_version INTEGER,
    imported_at   INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    schema_seen   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE song_genre (
    song_id TEXT    NOT NULL REFERENCES song(song_id) ON DELETE CASCADE,
    genre   TEXT    NOT NULL,
    rank    INTEGER NOT NULL,
    PRIMARY KEY (song_id, genre)
) WITHOUT ROWID;

CREATE TABLE song_chart (
    song_id      TEXT    NOT NULL REFERENCES song(song_id) ON DELETE CASCADE,
    instrument   INTEGER NOT NULL,
    difficulty   INTEGER NOT NULL,
    note_count   INTEGER NOT NULL,
    sustain_count INTEGER NOT NULL,
    chord_count  INTEGER NOT NULL,
    nps          REAL,
    peak_nps     REAL,
    PRIMARY KEY (song_id, instrument, difficulty)
) WITHOUT ROWID;

CREATE INDEX ix_song_bpm      ON song(bpm)         WHERE bpm IS NOT NULL;
CREATE INDEX ix_song_imported ON song(imported_at);
CREATE INDEX ix_song_duration ON song(duration_s)  WHERE duration_s IS NOT NULL;
CREATE INDEX ix_song_artist   ON song(artist, title);
CREATE INDEX ix_genre_name    ON song_genre(genre, song_id);
CREATE INDEX ix_chart_density ON song_chart(difficulty, note_count);
";

/// The queryable projection.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open (or create) an index and bring its schema up to date.
    pub fn open(path: &Path) -> rusqlite::Result<Index> {
        let mut conn = Connection::open(path)?;
        Index::prepare(&mut conn, MIGRATIONS)?;
        Ok(Index { conn })
    }

    /// An index in memory, for tests and for a rebuild that is about
    /// to be compared against a real one.
    pub fn in_memory() -> rusqlite::Result<Index> {
        let mut conn = Connection::open_in_memory()?;
        Index::prepare(&mut conn, MIGRATIONS)?;
        Ok(Index { conn })
    }

    /// Foreign keys on, then every migration the database has not
    /// seen. Split out so a test can drive it with a different list —
    /// the only way to prove that an older database survives a newer
    /// build, which is what the mechanism exists for.
    ///
    /// ⚠️ **Removing the pragma changes nothing in this build, and it
    /// stays anyway.** Measured rather than assumed: rusqlite's
    /// bundled SQLite is compiled with `DEFAULT_FOREIGN_KEYS=1`, so a
    /// raw connection already reads `1`. That is a property of how a
    /// dependency happens to be built, not of SQLite — plain SQLite
    /// defaults foreign keys OFF, and a build that ever linked a
    /// system library would silently stop cascading. The mutation
    /// probe that shows "no test fails without this line" is an
    /// equivalent mutant here, not a blind test.
    pub fn prepare(conn: &mut Connection, migrations: &[Migration]) -> rusqlite::Result<u32> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, migration) in migrations.iter().enumerate().skip(current as usize) {
            let next = index as u32 + 1;
            let tx = conn.transaction()?;
            tx.execute_batch(migration.sql)?;
            tx.pragma_update(None, "user_version", next)?;
            tx.commit()?;
        }
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
    }

    /// The schema version this database is on.
    pub fn version(&self) -> rusqlite::Result<u32> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
    }

    /// Put one song in, replacing whatever was there.
    ///
    /// `folder` is stored as the caller gives it — relative to the
    /// scan root, so a library that moves as a whole still resolves.
    pub fn put(&mut self, doc: &SongDoc, folder: &str) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        let id = doc.identity.song_id.as_str();
        tx.execute("DELETE FROM song WHERE song_id = ?1", params![id])?;
        let (tonic, minor) = match doc.musical.key.as_ref().map(|k| k.value) {
            Some(key) => (
                Some(i64::from(key.tonic)),
                Some(i64::from(key.mode == crate::Mode::Minor)),
            ),
            None => (None, None),
        };
        tx.execute(
            "INSERT INTO song (song_id, folder, title, artist, album, release_year, language,
                               duration_s, bpm, key_tonic, key_minor, energy, loudness_lufs,
                               has_lyrics, has_vocals, chart_hash, chart_version,
                               imported_at, updated_at, schema_seen)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                id,
                folder,
                doc.identity.title.value,
                doc.artist_line(),
                doc.identity.album.as_ref().map(|a| a.value.clone()),
                doc.descriptive
                    .release_year
                    .as_ref()
                    .map(|y| i64::from(y.value)),
                doc.descriptive.language.as_ref().map(|l| l.value.clone()),
                doc.musical.duration_s,
                doc.musical.bpm.as_ref().map(|b| b.value),
                tonic,
                minor,
                doc.features.as_ref().and_then(|f| f.energy),
                doc.features.as_ref().and_then(|f| f.loudness_lufs),
                i64::from(doc.lyrics.as_ref().is_some_and(|l| l.has_lyrics)),
                doc.vocals
                    .as_ref()
                    .and_then(|v| v.has_vocals)
                    .map(i64::from),
                doc.gameplay.chart_hash,
                doc.gameplay.chart_version,
                i64::try_from(doc.lifecycle.imported_at).unwrap_or(i64::MAX),
                i64::try_from(doc.lifecycle.updated_at).unwrap_or(i64::MAX),
                doc.schema_version,
            ],
        )?;
        for (rank, genre) in doc.descriptive.genres.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO song_genre (song_id, genre, rank) VALUES (?1, ?2, ?3)",
                params![id, genre.value, rank as i64],
            )?;
        }
        for instrument in &doc.gameplay.charts {
            for (difficulty, stats) in &instrument.difficulties {
                tx.execute(
                    "INSERT OR REPLACE INTO song_chart
                     (song_id, instrument, difficulty, note_count, sustain_count,
                      chord_count, nps, peak_nps)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        id,
                        instrument_code(instrument.instrument),
                        *difficulty as i64,
                        stats.note_count,
                        stats.sustain_count,
                        stats.chord_count,
                        stats.notes_per_second,
                        stats.peak_notes_per_second,
                    ],
                )?;
            }
        }
        tx.commit()
    }

    /// Take a song out — it left the library.
    pub fn remove(&mut self, song_id: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM song WHERE song_id = ?1", params![song_id])?;
        Ok(())
    }

    /// Throw the projection away and build it again from documents.
    ///
    /// The point of the whole design: this must reproduce the
    /// database exactly, or the index has quietly become the only
    /// copy of something.
    pub fn rebuild<'a>(
        &mut self,
        docs: impl IntoIterator<Item = (&'a SongDoc, &'a str)>,
    ) -> rusqlite::Result<usize> {
        self.conn.execute_batch("DELETE FROM song")?;
        let mut count = 0;
        for (doc, folder) in docs {
            self.put(doc, folder)?;
            count += 1;
        }
        Ok(count)
    }

    /// How many songs are indexed.
    pub fn len(&self) -> rusqlite::Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM song", [], |row| row.get::<_, i64>(0))
            .map(|n| usize::try_from(n).unwrap_or(0))
    }

    /// Whether the index holds nothing.
    pub fn is_empty(&self) -> rusqlite::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Every row of every table, in a fixed order, as one string.
    ///
    /// Only for comparing two databases — which is how "a rebuild
    /// reproduces the projection" is checked. Deliberately dumb and
    /// total: a clever comparison that skipped a column would pass
    /// while the column drifted.
    pub fn snapshot(&self) -> rusqlite::Result<String> {
        let mut out = String::new();
        for (table, order) in [
            ("song", "song_id"),
            ("song_genre", "song_id, rank"),
            ("song_chart", "song_id, instrument, difficulty"),
        ] {
            let mut statement = self
                .conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                out.push_str(table);
                for column in 0..columns {
                    let value: rusqlite::types::Value = row.get(column)?;
                    out.push('|');
                    out.push_str(&format!("{value:?}"));
                }
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// The connection, for queries this type does not wrap.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// The stable integer an instrument is stored under.
///
/// Explicit rather than `as i64` over the enum: the numbers are in a
/// database, so the day a variant is inserted in the middle they must
/// not shift.
fn instrument_code(instrument: Instrument) -> i64 {
    match instrument {
        Instrument::Guitar => 0,
        Instrument::Vocals => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{ChartStats, GameplayMeta, InstrumentCharts, LyricsMeta, SourceKind};
    use crate::{MetaSource, SongDoc, SongId, Sourced};
    use beatbyte_core::Difficulty;

    fn doc(id: u64, title: &str, bpm: f64, genres: &[&str]) -> SongDoc {
        let mut doc = SongDoc::new(
            SongId::from_parts(1_700_000_000_000 + id, id),
            Sourced::stated(title.to_owned(), MetaSource::Inferred),
            format!("{title}.m4a"),
            SourceKind::LocalFile,
            1_700_000_000_000 + id,
        );
        doc.identity.artists = vec!["Someone".to_owned()];
        doc.musical.bpm = Some(Sourced::stated(bpm, MetaSource::Analyzed));
        doc.musical.duration_s = Some(200.0);
        doc.descriptive.genres = genres
            .iter()
            .map(|g| Sourced::stated((*g).to_owned(), MetaSource::Embedded))
            .collect();
        doc.lyrics = Some(LyricsMeta {
            has_lyrics: true,
            ..LyricsMeta::default()
        });
        doc.gameplay = GameplayMeta {
            chart_hash: Some(format!("{id:016x}")),
            chart_version: Some(1),
            chart_format: Some(1),
            generator_version: None,
            charts: vec![InstrumentCharts {
                instrument: Instrument::Guitar,
                difficulties: vec![(
                    Difficulty::Medium,
                    ChartStats {
                        note_count: 300 + u32::try_from(id).unwrap_or(0),
                        sustain_count: 20,
                        chord_count: 5,
                        notes_per_second: Some(1.5),
                        peak_notes_per_second: Some(6.0),
                    },
                )],
            }],
        };
        doc
    }

    #[test]
    fn a_rebuild_from_the_documents_reproduces_the_projection() {
        // The property the whole design rests on: the index is a
        // projection, so deleting it can never be a data loss.
        let docs = vec![
            (doc(1, "Alpha", 120.0, &["House", "Deep House"]), "a"),
            (doc(2, "Beta", 128.0, &["Techno"]), "b"),
            (doc(3, "Gamma", 96.0, &[]), "c"),
        ];
        let mut grown = Index::in_memory().expect("an index");
        for (doc, folder) in &docs {
            grown.put(doc, folder).expect("indexes");
        }

        // Rebuilt into an index that already holds something else —
        // rebuilding into an empty one would pass even if `rebuild`
        // never cleared, and then a deleted song would live on in
        // the projection for ever.
        let mut fresh = Index::in_memory().expect("another");
        fresh
            .put(&doc(99, "Stale", 90.0, &["Ambient"]), "gone")
            .expect("something to clear");
        let count = fresh
            .rebuild(docs.iter().map(|(d, f)| (d, *f)))
            .expect("rebuilds");

        assert_eq!(count, 3);
        assert_eq!(fresh.len().expect("counts"), 3, "the stale song is gone");
        assert_eq!(
            fresh.snapshot().expect("dumps"),
            grown.snapshot().expect("dumps"),
            "an index built row by row and one rebuilt from scratch must \
             be the same database, or the projection has state of its own"
        );
    }

    #[test]
    fn indexing_a_song_twice_leaves_one_of_it() {
        let mut index = Index::in_memory().expect("an index");
        let song = doc(1, "Alpha", 120.0, &["House", "Deep House"]);
        index.put(&song, "a").expect("first");
        index.put(&song, "a").expect("again");
        assert_eq!(index.len().expect("counts"), 1);
        let genres: i64 = index
            .connection()
            .query_row("SELECT COUNT(*) FROM song_genre", [], |row| row.get(0))
            .expect("counts genres");
        assert_eq!(genres, 2, "and one of each of its genres");
    }

    #[test]
    fn removing_a_song_takes_its_rows_with_it() {
        // Foreign keys with ON DELETE CASCADE, which only work if the
        // pragma is actually on — it is off by default in SQLite, and
        // a forgotten pragma leaves orphans nothing ever notices.
        let mut index = Index::in_memory().expect("an index");
        let on: i64 = index
            .connection()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("reads the pragma");
        assert_eq!(
            on, 1,
            "the cascade below depends on this being on; see the note \
             on `prepare` for why removing the pragma does not fail \
             this assertion in THIS build"
        );
        let song = doc(1, "Alpha", 120.0, &["House"]);
        index.put(&song, "a").expect("indexes");
        index
            .remove(song.identity.song_id.as_str())
            .expect("removes");
        for table in ["song", "song_genre", "song_chart"] {
            let left: i64 = index
                .connection()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("counts");
            assert_eq!(left, 0, "{table} kept an orphan");
        }
    }

    #[test]
    fn the_commissions_queries_use_the_indexes() {
        // A query plan that says SCAN is a query that will be slow at
        // library size — and the whole reason this database exists.
        let index = Index::in_memory().expect("an index");
        for (sql, wanted) in [
            (
                "SELECT song_id FROM song WHERE bpm BETWEEN 115 AND 125",
                "ix_song_bpm",
            ),
            (
                "SELECT song_id FROM song WHERE imported_at > 0 ORDER BY imported_at DESC",
                "ix_song_imported",
            ),
            (
                "SELECT song_id FROM song_genre WHERE genre = 'Deep House'",
                "ix_genre_name",
            ),
        ] {
            let plan: String = index
                .connection()
                .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |row| {
                    row.get::<_, String>(3)
                })
                .expect("plans");
            assert!(
                plan.contains(wanted),
                "`{sql}` does not use {wanted}: {plan}"
            );
        }
    }

    #[test]
    fn an_older_database_survives_a_newer_build() {
        // Driven with a shortened list, which is the only way to
        // create a database at an older version and then bring it
        // forward the way a real upgrade does.
        let mut conn = Connection::open_in_memory().expect("a database");
        assert_eq!(Index::prepare(&mut conn, &[]).expect("no migrations"), 0);
        let first = Index::prepare(&mut conn, MIGRATIONS).expect("migrates");
        assert_eq!(first, MIGRATIONS.len() as u32);
        let again = Index::prepare(&mut conn, MIGRATIONS).expect("is idempotent");
        assert_eq!(again, first, "a second pass must apply nothing");
    }

    #[test]
    fn a_song_with_nothing_known_still_indexes() {
        // Most of a fresh library is like this, and a projection that
        // needed a genre would simply lose those songs.
        let mut index = Index::in_memory().expect("an index");
        let bare = SongDoc::new(
            SongId::from_parts(1, 1),
            Sourced::stated("Bare".to_owned(), MetaSource::Inferred),
            "bare.wav".to_owned(),
            SourceKind::LocalFile,
            1_000,
        );
        index.put(&bare, "bare").expect("indexes anyway");
        assert_eq!(index.len().expect("counts"), 1);
        let bpm: Option<f64> = index
            .connection()
            .query_row("SELECT bpm FROM song", [], |row| row.get(0))
            .expect("reads");
        assert_eq!(bpm, None, "and unknown stays unknown rather than zero");
    }
}
