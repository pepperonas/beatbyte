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

/// A record's identity: the song and the difficulty, as a struct.
///
/// It used to be the string `title|artist|difficulty`, and a title
/// with a "|" in it — legal in a file name on macOS and Linux —
/// could spell the same key as a different song and overwrite its
/// record. Fields cannot run into each other.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecordKey {
    /// The song's title.
    pub title: String,
    /// The song's artist.
    pub artist: String,
    /// The difficulty played.
    pub difficulty: Difficulty,
}

/// All best results, keyed by song + difficulty.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ScoreBoard {
    entries: HashMap<RecordKey, BestScore>,
}

impl ScoreBoard {
    fn key(title: &str, artist: &str, difficulty: Difficulty) -> RecordKey {
        RecordKey {
            title: title.to_owned(),
            artist: artist.to_owned(),
            difficulty,
        }
    }

    /// The stored best for a song/difficulty.
    #[must_use]
    pub fn best(&self, title: &str, artist: &str, difficulty: Difficulty) -> Option<BestScore> {
        self.entries
            .get(&Self::key(title, artist, difficulty))
            .copied()
    }

    /// Record a result. Returns `true` when it is a new record.
    pub fn record(
        &mut self,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
        result: BestScore,
    ) -> bool {
        let key = Self::key(title, artist, difficulty);
        match self.entries.get(&key) {
            Some(best) if best.score >= result.score => false,
            _ => {
                self.entries.insert(key, result);
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
#[derive(Debug, Serialize, Deserialize)]
struct StoredRecord {
    title: String,
    artist: String,
    difficulty: Difficulty,
    score: u64,
    accuracy: f64,
    best_streak: u32,
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
const FILE_VERSION: u32 = 2;

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
    let difficulty = Difficulty::ALL.into_iter().find(|d| d.id() == id)?;
    let (title, artist) = rest.split_once('|')?;
    Some(RecordKey {
        title: title.to_owned(),
        artist: artist.to_owned(),
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
                            title: r.title,
                            artist: r.artist,
                            difficulty: r.difficulty,
                        },
                        BestScore {
                            score: r.score,
                            accuracy: r.accuracy,
                            best_streak: r.best_streak,
                        },
                    );
                }
            }
            OnDisk::Legacy(file) => {
                for (key, best) in file.entries {
                    match migrate_legacy_key(&key) {
                        Some(record) => {
                            board.entries.insert(record, best);
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
        let mut records: Vec<(&RecordKey, &BestScore)> = self.entries.iter().collect();
        records.sort_by(|a, b| a.0.cmp(b.0));
        let file = ScoresFile {
            version: FILE_VERSION,
            records: records
                .into_iter()
                .map(|(k, b)| StoredRecord {
                    title: k.title.clone(),
                    artist: k.artist.clone(),
                    difficulty: k.difficulty,
                    score: b.score,
                    accuracy: b.accuracy,
                    best_streak: b.best_streak,
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
        assert!(board.record("Song", "Artist", Difficulty::Medium, score(100)));
        assert_eq!(
            board
                .best("Song", "Artist", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
        );
    }

    #[test]
    fn only_a_higher_score_replaces_the_record() {
        let mut board = ScoreBoard::default();
        board.record("Song", "Artist", Difficulty::Medium, score(100));
        assert!(!board.record("Song", "Artist", Difficulty::Medium, score(80)));
        assert!(
            !board.record("Song", "Artist", Difficulty::Medium, score(100)),
            "matching the record is not beating it"
        );
        assert!(board.record("Song", "Artist", Difficulty::Medium, score(101)));
        assert_eq!(
            board
                .best("Song", "Artist", Difficulty::Medium)
                .map(|b| b.score),
            Some(101)
        );
    }

    #[test]
    fn difficulties_keep_separate_records() {
        // Playing Easy well must never overwrite an Expert record.
        let mut board = ScoreBoard::default();
        board.record("Song", "Artist", Difficulty::Easy, score(500));
        board.record("Song", "Artist", Difficulty::Expert, score(200));
        assert_eq!(
            board
                .best("Song", "Artist", Difficulty::Expert)
                .map(|b| b.score),
            Some(200)
        );
        assert!(board.best("Song", "Artist", Difficulty::Hard).is_none());
    }

    #[test]
    fn songs_are_told_apart_by_title_and_artist() {
        let mut board = ScoreBoard::default();
        board.record("Song", "One", Difficulty::Medium, score(100));
        board.record("Song", "Two", Difficulty::Medium, score(200));
        assert_eq!(
            board
                .best("Song", "One", Difficulty::Medium)
                .map(|b| b.score),
            Some(100)
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
        board.record("A|B", "C", Difficulty::Medium, score(100));
        assert_eq!(board.best("A", "B|C", Difficulty::Medium), None);
        assert_eq!(
            board.best("A|B", "C", Difficulty::Medium).map(|b| b.score),
            Some(100)
        );
    }

    #[test]
    fn legacy_keys_migrate_and_malformed_ones_are_dropped() {
        let key = migrate_legacy_key("All That She Wants|Ace of Base|medium").expect("well-formed");
        assert_eq!(key.title, "All That She Wants");
        assert_eq!(key.artist, "Ace of Base");
        assert_eq!(key.difficulty, Difficulty::Medium);
        // The one ambiguous shape: a pipe inside title or artist. The
        // title ends at the first pipe, as the old lookup read it.
        let key = migrate_legacy_key("A|B|C|expert").expect("readable");
        assert_eq!((key.title.as_str(), key.artist.as_str()), ("A", "B|C"));
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
                .best("The Passenger", "Iggy Pop", Difficulty::Medium)
                .map(|b| b.score),
            Some(147_680)
        );
        assert_eq!(
            board
                .best("Maria", "Blondie", Difficulty::Hard)
                .map(|b| b.best_streak),
            Some(3)
        );
        // Saved, it is version 2 — and reads back identical.
        let text = board.to_json();
        assert!(text.contains("\"version\": 2"));
        assert!(!text.contains('|'), "no key strings in the new file");
        assert_eq!(ScoreBoard::from_json(&text).expect("v2 parses"), board);
    }

    #[test]
    fn the_new_file_round_trips_pipes_and_is_stable() {
        let mut board = ScoreBoard::default();
        board.record("A|B", "C", Difficulty::Medium, score(100));
        board.record("A", "B|C", Difficulty::Medium, score(200));
        board.record("Zed", "Y", Difficulty::Easy, score(1));
        let text = board.to_json();
        let back = ScoreBoard::from_json(&text).expect("parses");
        assert_eq!(back, board);
        assert_eq!(
            back.best("A|B", "C", Difficulty::Medium).map(|b| b.score),
            Some(100)
        );
        assert_eq!(
            back.best("A", "B|C", Difficulty::Medium).map(|b| b.score),
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
