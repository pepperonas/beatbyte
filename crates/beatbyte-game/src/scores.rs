//! High scores: a small persistent save file in the platform data
//! directory. No accounts, no network — a local record of glory.

use std::collections::HashMap;

use beatbyte_core::Difficulty;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// One best result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BestScore {
    /// The score achieved.
    pub score: u64,
    /// Weighted accuracy 0.0–1.0.
    pub accuracy: f64,
    /// Longest streak.
    pub best_streak: u32,
}

/// Which song a record belongs to.
///
/// ⚠️ **A name is not an identity.** Records were keyed on title and
/// artist, so correcting a typo in a title orphaned that song's
/// records — silently, and for ever. A song that has a document in
/// its folder is keyed by its permanent id (ADR-0019) and survives
/// every rename; one that has none — a built-in, or a folder not yet
/// migrated — keeps the old key, because a record under a guessed id
/// would be worse than one under a name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SongRef {
    /// The song's permanent id.
    Id(String),
    /// Title and artist, for a song that has no id.
    Named {
        /// The song's title.
        title: String,
        /// The song's artist.
        artist: String,
    },
}

/// A record's identity: the song and the difficulty, as a struct.
///
/// It used to be the string `title|artist|difficulty`, and a title
/// with a "|" in it — legal in a file name on macOS and Linux —
/// could spell the same key as a different song and overwrite its
/// record. Fields cannot run into each other.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecordKey {
    /// Which song.
    pub song: SongRef,
    /// The difficulty played.
    pub difficulty: Difficulty,
}

/// One record and the name it was set under.
///
/// The name rides along with the value rather than being part of the
/// key: a record keyed by a song id still has to say which song it
/// is about when the file is read by a human — and the key must not
/// change when a title is corrected, which is the whole point.
#[derive(Debug, Clone, PartialEq)]
struct Record {
    best: BestScore,
    title: String,
    artist: String,
    /// The `chart_hash` of the chart the best was played on (records
    /// from before it was kept have none) — a best on an older version
    /// of a chart is not a best on the one that plays now.
    chart: Option<String>,
}

/// All best results, keyed by song + difficulty.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ScoreBoard {
    entries: HashMap<RecordKey, Record>,
}

impl ScoreBoard {
    fn named(title: &str, artist: &str, difficulty: Difficulty) -> RecordKey {
        RecordKey {
            song: SongRef::Named {
                title: title.to_owned(),
                artist: artist.to_owned(),
            },
            difficulty,
        }
    }

    fn by_id(song_id: &str, difficulty: Difficulty) -> RecordKey {
        RecordKey {
            song: SongRef::Id(song_id.to_owned()),
            difficulty,
        }
    }

    /// The stored best for a song/difficulty.
    ///
    /// The id first, then the name: a record set before the library
    /// had documents is still that song's record, and must keep
    /// showing until the song is played again.
    #[must_use]
    pub fn best(
        &self,
        song_id: Option<&str>,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
    ) -> Option<BestScore> {
        song_id
            .and_then(|id| self.entries.get(&Self::by_id(id, difficulty)))
            .or_else(|| self.entries.get(&Self::named(title, artist, difficulty)))
            .map(|record| record.best)
    }

    /// The chart the best was played on, when the record knows it.
    #[must_use]
    pub fn best_chart(
        &self,
        song_id: Option<&str>,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
    ) -> Option<&str> {
        song_id
            .and_then(|id| self.entries.get(&Self::by_id(id, difficulty)))
            .or_else(|| self.entries.get(&Self::named(title, artist, difficulty)))
            .and_then(|record| record.chart.as_deref())
    }

    /// Whether the best was played on ANOTHER chart than `current` —
    /// `false` when either is unknown (an old record says nothing).
    #[must_use]
    pub fn best_is_from_another_chart(
        &self,
        song_id: Option<&str>,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
        current: Option<&str>,
    ) -> bool {
        match (self.best_chart(song_id, title, artist, difficulty), current) {
            (Some(recorded), Some(current)) => recorded != current,
            _ => false,
        }
    }

    /// Record a result. Returns `true` when it is a new record.
    ///
    /// Writing under an id **retires the name-keyed twin**, so a song
    /// carries its record forward the first time it is played after
    /// gaining a document — rather than keeping two records that
    /// slowly diverge.
    pub fn record(
        &mut self,
        song_id: Option<&str>,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
        result: BestScore,
        chart: Option<&str>,
    ) -> bool {
        let key = match song_id {
            Some(id) => Self::by_id(id, difficulty),
            None => Self::named(title, artist, difficulty),
        };
        let previous = song_id
            .and_then(|id| self.entries.get(&Self::by_id(id, difficulty)))
            .or_else(|| self.entries.get(&Self::named(title, artist, difficulty)))
            .cloned();
        if song_id.is_some() {
            self.entries.remove(&Self::named(title, artist, difficulty));
        }
        let named = Record {
            best: result,
            title: title.to_owned(),
            artist: artist.to_owned(),
            chart: chart.map(str::to_owned),
        };
        match previous {
            Some(best) if best.best.score >= result.score => {
                // Not a record — but the entry may still have to move
                // to the id, or the removal above would lose it.
                self.entries.entry(key).or_insert(Record {
                    best: best.best,
                    chart: best.chart,
                    ..named
                });
                false
            }
            _ => {
                self.entries.insert(key, named);
                true
            }
        }
    }

    /// How many records are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no record is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The on-disk format, version 2: a list of records, each carrying
/// its song and difficulty as fields.
#[derive(Debug, Serialize, Deserialize)]
struct ScoresFile {
    version: u32,
    records: Vec<StoredRecord>,
}

/// One record on disk.
///
/// The name is written even for a record keyed by id: a file a human
/// opens should say which song a row is about, and a v3 file read by
/// a v2 build still finds the fields it knows.
#[derive(Debug, Serialize, Deserialize)]
struct StoredRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    song_id: Option<String>,
    title: String,
    artist: String,
    difficulty: Difficulty,
    score: u64,
    accuracy: f64,
    best_streak: u32,
    /// The chart the best was played on (additive: older files and
    /// older builds do without it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chart: Option<String>,
}

/// The format before version 2: a map from `title|artist|difficulty`
/// to the best. Read for migration only, never written.
#[derive(Debug, Deserialize)]
struct LegacyFile {
    entries: HashMap<String, BestScore>,
}

/// Either format, tried newest first.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OnDisk {
    Current(ScoresFile),
    Legacy(LegacyFile),
}

/// The current on-disk version.
///
/// v3 adds the song id. A v2 file reads unchanged — every record in
/// it is simply keyed by name, which is what it was.
const FILE_VERSION: u32 = 3;

/// Whether a file's text is the pre-version-2 map. Pure — tested.
#[must_use]
pub fn is_legacy(text: &str) -> bool {
    matches!(serde_json::from_str::<OnDisk>(text), Ok(OnDisk::Legacy(_)))
}

/// Split a legacy key back into its parts. The difficulty is the
/// piece after the LAST pipe (its ids never contain one), the title
/// ends at the FIRST pipe — which is the one ambiguous choice: a
/// legacy key with a pipe inside the title or the artist was
/// ambiguous on disk already, and this reads it the way the old
/// lookup would have. Keys with no difficulty or no artist are
/// malformed and dropped with a warning.
#[must_use]
pub fn migrate_legacy_key(key: &str) -> Option<RecordKey> {
    let (rest, id) = key.rsplit_once('|')?;
    let difficulty = Difficulty::from_id(id)?;
    let (title, artist) = rest.split_once('|')?;
    Some(RecordKey {
        song: SongRef::Named {
            title: title.to_owned(),
            artist: artist.to_owned(),
        },
        difficulty,
    })
}

impl ScoreBoard {
    /// The board a file's contents describe. Version 2 is read as it
    /// is; the legacy map is migrated key by key, every readable
    /// record kept.
    fn from_on_disk(disk: OnDisk) -> Self {
        let mut board = ScoreBoard::default();
        match disk {
            OnDisk::Current(file) => {
                for r in file.records {
                    board.entries.insert(
                        RecordKey {
                            song: match r.song_id {
                                Some(id) => SongRef::Id(id),
                                None => SongRef::Named {
                                    title: r.title.clone(),
                                    artist: r.artist.clone(),
                                },
                            },
                            difficulty: r.difficulty,
                        },
                        Record {
                            best: BestScore {
                                score: r.score,
                                accuracy: r.accuracy,
                                best_streak: r.best_streak,
                            },
                            title: r.title,
                            artist: r.artist,
                            chart: r.chart,
                        },
                    );
                }
            }
            OnDisk::Legacy(file) => {
                for (key, best) in file.entries {
                    match migrate_legacy_key(&key) {
                        Some(record) => {
                            let (title, artist) = match &record.song {
                                SongRef::Named { title, artist } => (title.clone(), artist.clone()),
                                // A legacy key is always a name: the
                                // format predates song ids entirely.
                                SongRef::Id(_) => (String::new(), String::new()),
                            };
                            board.entries.insert(
                                record,
                                Record {
                                    best,
                                    title,
                                    artist,
                                    chart: None,
                                },
                            );
                        }
                        None => warn!("scores: legacy record {key:?} is unreadable; dropped"),
                    }
                }
            }
        }
        board
    }

    /// Parse a file's text. Pure — tested with both formats.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<OnDisk>(text).map(Self::from_on_disk)
    }

    /// The version-2 text of this board, records in a stable order so
    /// the file reads the same after every save.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut records: Vec<(&RecordKey, &Record)> = self.entries.iter().collect();
        records.sort_by(|a, b| a.0.cmp(b.0));
        let file = ScoresFile {
            version: FILE_VERSION,
            records: records
                .into_iter()
                .map(|(k, b)| StoredRecord {
                    song_id: match &k.song {
                        SongRef::Id(id) => Some(id.clone()),
                        SongRef::Named { .. } => None,
                    },
                    title: b.title.clone(),
                    artist: b.artist.clone(),
                    difficulty: k.difficulty,
                    score: b.best.score,
                    accuracy: b.best.accuracy,
                    best_streak: b.best.best_streak,
                    chart: b.chart.clone(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&file).unwrap_or_default()
    }
}

/// Where the scores file lives.
#[must_use]
pub fn scores_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("scores.json"))
}

/// Load the scoreboard (missing/corrupt → empty with a warning).
#[must_use]
pub fn load_scores() -> ScoreBoard {
    let Some(path) = scores_path() else {
        return ScoreBoard::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            // A legacy file is kept beside the new one, once: the
            // migration is meant to be lossless, and a copy costs
            // nothing if it was.
            if is_legacy(&text) {
                let backup = path.with_extension("v1.bak.json");
                if !backup.exists() {
                    match std::fs::copy(&path, &backup) {
                        Ok(_) => info!("scores: legacy file kept as {}", backup.display()),
                        Err(error) => warn!("scores: cannot back up the legacy file: {error}"),
                    }
                }
            }
            ScoreBoard::from_json(&text).unwrap_or_else(|error| {
                warn!(
                    "scores file {} is invalid ({error}); starting fresh",
                    path.display()
                );
                ScoreBoard::default()
            })
        }
        Err(_) => ScoreBoard::default(),
    }
}

/// Persist the scoreboard (best effort).
pub fn save_scores(scores: &ScoreBoard) {
    let Some(path) = scores_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, scores.to_json())
    };
    if let Err(error) = write() {
        warn!("cannot save scores to {}: {error}", path.display());
    }
}

/// Plugin: loads the scoreboard at startup.
pub struct ScoresPlugin;

impl Plugin for ScoresPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_scores());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(value: u64) -> BestScore {
        BestScore {
            score: value,
            accuracy: 1.0,
            best_streak: 10,
        }
    }

    #[test]
    fn a_first_result_is_always_a_record() {
        let mut board = ScoreBoard::default();
        assert!(board.record(None, "Song", "Artist", Difficulty::Medium, score(100), None));
        assert_eq!(
            board
                .best(None, "Song", "Artist", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
        );
    }

    #[test]
    fn only_a_higher_score_replaces_the_record() {
        let mut board = ScoreBoard::default();
        board.record(None, "Song", "Artist", Difficulty::Medium, score(100), None);
        assert!(!board.record(None, "Song", "Artist", Difficulty::Medium, score(80), None));
        assert!(
            !board.record(None, "Song", "Artist", Difficulty::Medium, score(100), None),
            "matching the record is not beating it"
        );
        assert!(board.record(None, "Song", "Artist", Difficulty::Medium, score(101), None));
        assert_eq!(
            board
                .best(None, "Song", "Artist", Difficulty::Medium)
                .map(|b| b.score),
            Some(101)
        );
    }

    #[test]
    fn difficulties_keep_separate_records() {
        // Playing Easy well must never overwrite an Expert record.
        let mut board = ScoreBoard::default();
        board.record(None, "Song", "Artist", Difficulty::Easy, score(500), None);
        board.record(None, "Song", "Artist", Difficulty::Expert, score(200), None);
        assert_eq!(
            board
                .best(None, "Song", "Artist", Difficulty::Expert)
                .map(|b| b.score),
            Some(200)
        );
        assert!(
            board
                .best(None, "Song", "Artist", Difficulty::Hard)
                .is_none()
        );
    }

    #[test]
    fn songs_are_told_apart_by_title_and_artist() {
        let mut board = ScoreBoard::default();
        board.record(None, "Song", "One", Difficulty::Medium, score(100), None);
        board.record(None, "Song", "Two", Difficulty::Medium, score(200), None);
        assert_eq!(
            board
                .best(None, "Song", "One", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
        );
    }

    /// The chart a best was played on survives the file and decides
    /// "older chart": a different chart is older, an unknown one says
    /// nothing, and a worse run never takes the best's chart away.
    #[test]
    fn a_best_remembers_its_chart() {
        let mut board = ScoreBoard::default();
        board.record(
            Some("id"),
            "S",
            "A",
            Difficulty::Hard,
            score(100),
            Some("h1"),
        );
        board.record(
            Some("id"),
            "S",
            "A",
            Difficulty::Hard,
            score(50),
            Some("h2"),
        );
        assert_eq!(
            board.best_chart(Some("id"), "S", "A", Difficulty::Hard),
            Some("h1")
        );
        let back = ScoreBoard::from_json(&board.to_json()).expect("reads back");
        assert_eq!(
            back.best_chart(Some("id"), "S", "A", Difficulty::Hard),
            Some("h1")
        );
        assert!(back.best_is_from_another_chart(
            Some("id"),
            "S",
            "A",
            Difficulty::Hard,
            Some("h2")
        ));
        assert!(!back.best_is_from_another_chart(
            Some("id"),
            "S",
            "A",
            Difficulty::Hard,
            Some("h1")
        ));
        assert!(!back.best_is_from_another_chart(Some("id"), "S", "A", Difficulty::Hard, None));
        // A name-keyed best that MOVES to the song's id with a worse run
        // keeps its own chart, not the worse run's.
        let mut moving = ScoreBoard::default();
        moving.record(None, "S", "A", Difficulty::Hard, score(100), Some("h1"));
        moving.record(
            Some("id"),
            "S",
            "A",
            Difficulty::Hard,
            score(50),
            Some("h2"),
        );
        assert_eq!(
            moving.best_chart(Some("id"), "S", "A", Difficulty::Hard),
            Some("h1")
        );
        // A record from before charts were kept says nothing.
        let mut old = ScoreBoard::default();
        old.record(None, "S", "A", Difficulty::Hard, score(100), None);
        assert!(!old.best_is_from_another_chart(None, "S", "A", Difficulty::Hard, Some("h9")));
        assert!(
            !old.to_json().contains("\"chart\""),
            "no empty field written"
        );
    }

    #[test]
    fn a_record_keyed_by_the_song_survives_a_rename() {
        // The defect this exists to end: correcting a typo in a title
        // orphaned that song's records, silently and for ever.
        let mut board = ScoreBoard::default();
        board.record(
            Some("bb_song"),
            "Marai",
            "Blondie",
            Difficulty::Hard,
            score(9000),
            None,
        );
        assert_eq!(
            board
                .best(Some("bb_song"), "Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.score),
            Some(9000),
            "the title was corrected; the record is the same record"
        );
    }

    #[test]
    fn a_record_set_before_the_song_had_an_id_is_carried_forward() {
        // A library that has just been migrated is full of these. The
        // record must keep showing, and must MOVE the first time the
        // song is played — two records for one song would diverge.
        let mut board = ScoreBoard::default();
        board.record(
            None,
            "Maria",
            "Blondie",
            Difficulty::Hard,
            score(9000),
            None,
        );
        assert_eq!(
            board
                .best(Some("bb_song"), "Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.score),
            Some(9000),
            "an older record still counts"
        );

        // Played again, worse. Not a new record — but it moves.
        assert!(!board.record(
            Some("bb_song"),
            "Maria",
            "Blondie",
            Difficulty::Hard,
            score(10),
            None
        ));
        assert_eq!(board.len(), 1, "one song, one record");
        assert_eq!(
            board
                .best(Some("bb_song"), "Anything", "At All", Difficulty::Hard)
                .map(|b| b.score),
            Some(9000),
            "and it is now keyed by the song, not by the name"
        );
    }

    #[test]
    fn a_version_two_file_reads_as_records_keyed_by_name() {
        // Every file on every existing machine is one of these.
        let text = r#"{"version":2,"records":[
            {"title":"Maria","artist":"Blondie","difficulty":"hard",
             "score":9000,"accuracy":0.9,"best_streak":3}]}"#;
        let board = ScoreBoard::from_json(text).expect("reads");
        assert_eq!(
            board
                .best(None, "Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.score),
            Some(9000)
        );
        assert_eq!(
            board
                .best(Some("bb_song"), "Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.score),
            Some(9000),
            "and a song that has since gained an id still finds it"
        );
    }

    #[test]
    fn a_pipe_in_a_title_no_longer_collides() {
        // Was `known_limitation_a_pipe_in_a_title_can_collide`: the
        // key used to be `title|artist|difficulty` with no escaping,
        // so "A|B" by "C" and "A" by "B|C" shared a record. Titles
        // come from file names, where "|" is legal on macOS and
        // Linux. The key is a struct now.
        let mut board = ScoreBoard::default();
        board.record(None, "A|B", "C", Difficulty::Medium, score(100), None);
        assert_eq!(board.best(None, "A", "B|C", Difficulty::Medium), None);
        assert_eq!(
            board
                .best(None, "A|B", "C", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
        );
    }

    #[test]
    fn legacy_keys_migrate_and_malformed_ones_are_dropped() {
        let key = migrate_legacy_key("All That She Wants|Ace of Base|medium").expect("well-formed");
        assert_eq!(
            key.song,
            SongRef::Named {
                title: "All That She Wants".to_owned(),
                artist: "Ace of Base".to_owned(),
            }
        );
        assert_eq!(key.difficulty, Difficulty::Medium);
        // The one ambiguous shape: a pipe inside title or artist. The
        // title ends at the first pipe, as the old lookup read it.
        let key = migrate_legacy_key("A|B|C|expert").expect("readable");
        assert_eq!(
            key.song,
            SongRef::Named {
                title: "A".to_owned(),
                artist: "B|C".to_owned(),
            }
        );
        assert_eq!(migrate_legacy_key("no-artist|medium"), None);
        assert_eq!(migrate_legacy_key("Song|Artist|ludicrous"), None);
        assert_eq!(migrate_legacy_key(""), None);
    }

    #[test]
    fn a_legacy_file_survives_the_migration_with_every_record_intact() {
        // The shape of the file the game wrote before version 2.
        let legacy = r#"{
          "entries": {
            "All That She Wants|Ace of Base|medium": {"score": 16885, "accuracy": 0.578, "best_streak": 38},
            "The Passenger|Iggy Pop|medium": {"score": 147680, "accuracy": 1.0, "best_streak": 568},
            "Maria|Blondie|hard": {"score": 200, "accuracy": 0.5, "best_streak": 3}
          }
        }"#;
        let board = ScoreBoard::from_json(legacy).expect("legacy parses");
        assert_eq!(board.len(), 3);
        assert_eq!(
            board
                .best(None, "The Passenger", "Iggy Pop", Difficulty::Medium)
                .map(|b| b.score),
            Some(147_680)
        );
        assert_eq!(
            board
                .best(None, "Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.best_streak),
            Some(3)
        );
        // Saved, it is the current version — and reads back
        // identical. Derived from the constant rather than typed, so
        // a version bump does not need this line edited: what the
        // test means is "the file it writes is the file it reads".
        let text = board.to_json();
        assert!(text.contains(&format!("\"version\": {FILE_VERSION}")));
        assert!(!text.contains('|'), "no key strings in the new file");
        assert_eq!(ScoreBoard::from_json(&text).expect("v2 parses"), board);
    }

    #[test]
    fn the_new_file_round_trips_pipes_and_is_stable() {
        let mut board = ScoreBoard::default();
        board.record(None, "A|B", "C", Difficulty::Medium, score(100), None);
        board.record(None, "A", "B|C", Difficulty::Medium, score(200), None);
        board.record(None, "Zed", "Y", Difficulty::Easy, score(1), None);
        let text = board.to_json();
        let back = ScoreBoard::from_json(&text).expect("parses");
        assert_eq!(back, board);
        assert_eq!(
            back.best(None, "A|B", "C", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
        );
        assert_eq!(
            back.best(None, "A", "B|C", Difficulty::Medium)
                .map(|b| b.score),
            Some(200)
        );
        // Stable order: the same board writes the same bytes.
        assert_eq!(text, back.to_json());
        let a = text.find("\"title\": \"A\"").expect("A is in the file");
        let zed = text.find("\"title\": \"Zed\"").expect("Zed is in the file");
        assert!(a < zed, "sorted by title");
    }

    #[test]
    fn the_legacy_shape_is_recognised_and_the_new_one_is_not() {
        assert!(is_legacy(r#"{"entries": {}}"#));
        assert!(!is_legacy(r#"{"version": 2, "records": []}"#));
        assert!(!is_legacy("nonsense"));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(ScoreBoard::from_json("not json").is_err());
        assert!(ScoreBoard::from_json("{}").is_err());
    }
}
