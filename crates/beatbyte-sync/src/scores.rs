//! Best scores (`scores.json`, v3): per song and difficulty, the
//! better result — by the game's own rule.
//!
//! The board keeps no player: a record is the song's best, whoever
//! set it (`beatbyte-game::scores`). So the merge is per (song,
//! difficulty): the higher score; equal scores → the higher accuracy
//! → the longer streak; a full tie keeps the local record. A song is
//! its `song_id` where it has one (ADR-0019), else its title and
//! artist — and a name-keyed record for a song that has an id-keyed
//! one on the other side collapses onto the id, exactly as
//! `ScoreBoard::record` does when a song gains its document.
//!
//! A song id may itself be remapped (two devices gave one song two
//! documents; ADR-0021 keeps the older id): records follow the remap.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// Song id → the id it now is.
pub type SongRemap = BTreeMap<String, String>;

/// The board format this merge understands.
pub const VERSION: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Song {
    Id(String),
    Named(String, String),
}

fn rank(record: &Value) -> (u64, f64, u64) {
    (
        record.get("score").and_then(Value::as_u64).unwrap_or(0),
        record
            .get("accuracy")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        record
            .get("best_streak")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
}

fn better(a: &Value, b: &Value) -> bool {
    let (ra, rb) = (rank(a), rank(b));
    ra.0 > rb.0 || (ra.0 == rb.0 && (ra.1 > rb.1 || (ra.1 == rb.1 && ra.2 > rb.2)))
}

fn song_of(record: &Value, remap: &SongRemap) -> Option<Song> {
    if let Some(id) = record.get("song_id").and_then(Value::as_str) {
        let id = remap.get(id).cloned().unwrap_or_else(|| id.to_owned());
        return Some(Song::Id(id));
    }
    Some(Song::Named(
        record.get("title")?.as_str()?.to_owned(),
        record.get("artist")?.as_str()?.to_owned(),
    ))
}

/// What merging two boards did.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// The merged board, as the file's JSON.
    pub board: Value,
    /// Records the remote side improved or added.
    pub improved: usize,
}

/// Merge two boards. Pure — tested.
///
/// # Errors
/// When either board is not version 3: a board this merge does not
/// understand is refused, never rewritten.
pub fn merge(local: &Value, remote: &Value, songs: &SongRemap) -> Result<Merged, String> {
    for (side, board) in [("local", local), ("remote", remote)] {
        let version = board.get("version").and_then(Value::as_u64);
        if version != Some(VERSION) {
            return Err(format!(
                "the {side} score board is version {version:?}, this merge knows {VERSION}"
            ));
        }
    }
    let records = |board: &Value| -> Vec<Value> {
        board
            .get("records")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut best: BTreeMap<(Song, String), Value> = BTreeMap::new();
    let mut improved = 0usize;
    for (is_local, list) in [(true, records(local)), (false, records(remote))] {
        for mut record in list {
            let (Some(song), Some(difficulty)) = (
                song_of(&record, songs),
                record
                    .get("difficulty")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            ) else {
                continue;
            };
            if let Song::Id(id) = &song {
                record["song_id"] = Value::from(id.clone());
            }
            let key = (song, difficulty);
            match best.get(&key) {
                Some(have) if !better(&record, have) => {}
                _ => {
                    if !is_local {
                        improved += 1;
                    }
                    best.insert(key, record);
                }
            }
        }
    }
    // A name-keyed record whose song has an id-keyed record collapses
    // onto the id, keeping the better of the two.
    let named: Vec<(Song, String)> = best
        .keys()
        .filter(|(s, _)| matches!(s, Song::Named(..)))
        .cloned()
        .collect();
    for key in named {
        let Song::Named(title, artist) = &key.0 else {
            continue;
        };
        let twin = best
            .iter()
            .find(|((s, d), v)| {
                matches!(s, Song::Id(_))
                    && *d == key.1
                    && v.get("title").and_then(Value::as_str) == Some(title)
                    && v.get("artist").and_then(Value::as_str) == Some(artist)
            })
            .map(|(k, _)| k.clone());
        if let Some(id_key) = twin
            && let Some(named_record) = best.remove(&key)
            && better(&named_record, &best[&id_key])
        {
            let mut moved = named_record;
            if let Song::Id(id) = &id_key.0 {
                moved["song_id"] = Value::from(id.clone());
            }
            best.insert(id_key, moved);
        }
    }
    let records: Vec<Value> = best.into_values().collect();
    Ok(Merged {
        board: json!({ "version": VERSION, "records": records }),
        improved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        song: Option<&str>,
        title: &str,
        diff: &str,
        score: u64,
        acc: f64,
        streak: u64,
    ) -> Value {
        let mut v = json!({"title": title, "artist": "A", "difficulty": diff,
            "score": score, "accuracy": acc, "best_streak": streak});
        if let Some(id) = song {
            v["song_id"] = Value::from(id);
        }
        v
    }

    fn board(records: Vec<Value>) -> Value {
        json!({"version": 3, "records": records})
    }

    fn score_of(m: &Merged, diff: &str) -> Vec<u64> {
        m.board["records"]
            .as_array()
            .expect("records")
            .iter()
            .filter(|r| r["difficulty"] == diff)
            .map(|r| r["score"].as_u64().expect("score"))
            .collect()
    }

    /// Each side's best survives where it is the best.
    #[test]
    fn the_best_of_each_song_and_difficulty_is_kept() {
        let local = board(vec![
            rec(Some("s1"), "T", "hard", 900, 0.9, 50),
            rec(Some("s1"), "T", "easy", 100, 1.0, 9),
        ]);
        let remote = board(vec![
            rec(Some("s1"), "T", "hard", 950, 0.8, 40),
            rec(Some("s1"), "T", "easy", 90, 1.0, 9),
        ]);
        let m = merge(&local, &remote, &SongRemap::new()).expect("merged");
        assert_eq!(score_of(&m, "hard"), vec![950]);
        assert_eq!(score_of(&m, "easy"), vec![100]);
        assert_eq!(m.improved, 1);
    }

    /// Equal scores: accuracy, then streak, decide — the same way on
    /// both devices.
    #[test]
    fn a_tie_on_score_goes_to_accuracy_then_streak() {
        let a = board(vec![rec(Some("s"), "T", "hard", 900, 0.9, 50)]);
        let b = board(vec![rec(Some("s"), "T", "hard", 900, 0.95, 10)]);
        for (l, r) in [(&a, &b), (&b, &a)] {
            let m = merge(l, r, &SongRemap::new()).expect("merged");
            assert_eq!(m.board["records"][0]["accuracy"], 0.95);
        }
        let c = board(vec![rec(Some("s"), "T", "hard", 900, 0.9, 60)]);
        let m = merge(&a, &c, &SongRemap::new()).expect("merged");
        assert_eq!(m.board["records"][0]["best_streak"], 60);
    }

    /// A name-keyed record collapses onto the song's id.
    #[test]
    fn a_named_record_moves_onto_the_id_when_it_is_better() {
        let local = board(vec![rec(None, "T", "hard", 999, 1.0, 9)]);
        let remote = board(vec![rec(Some("s1"), "T", "hard", 500, 1.0, 9)]);
        let m = merge(&local, &remote, &SongRemap::new()).expect("merged");
        let records = m.board["records"].as_array().expect("records");
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0]["song_id"], "s1");
        assert_eq!(records[0]["score"], 999);
    }

    /// Two ids for one song: the records follow the remap and merge.
    #[test]
    fn a_remapped_song_id_takes_its_records_along() {
        let local = board(vec![rec(Some("old"), "T", "hard", 800, 1.0, 9)]);
        let remote = board(vec![rec(Some("new"), "T", "hard", 900, 1.0, 9)]);
        let mut remap = SongRemap::new();
        remap.insert("new".to_owned(), "old".to_owned());
        let m = merge(&local, &remote, &remap).expect("merged");
        let records = m.board["records"].as_array().expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["song_id"], "old");
        assert_eq!(records[0]["score"], 900);
    }

    #[test]
    fn a_board_of_another_version_is_refused() {
        let old = json!({"version": 2, "records": []});
        assert!(merge(&old, &board(vec![]), &SongRemap::new()).is_err());
        assert!(merge(&board(vec![]), &old, &SongRemap::new()).is_err());
    }

    #[test]
    fn both_orders_agree_and_merging_again_changes_nothing() {
        let a = board(vec![
            rec(Some("s"), "T", "hard", 1, 1.0, 1),
            rec(None, "U", "easy", 5, 1.0, 1),
        ]);
        let b = board(vec![
            rec(Some("s"), "T", "hard", 2, 1.0, 1),
            rec(Some("v"), "V", "easy", 3, 1.0, 1),
        ]);
        let ab = merge(&a, &b, &SongRemap::new()).expect("ab");
        let ba = merge(&b, &a, &SongRemap::new()).expect("ba");
        assert_eq!(ab.board, ba.board);
        let again = merge(&ab.board, &ba.board, &SongRemap::new()).expect("again");
        assert_eq!(again.board, ab.board);
        assert_eq!(again.improved, 0);
    }
}
