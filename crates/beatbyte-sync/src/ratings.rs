//! Favourites and star ratings (`ratings.json`): per player, per field,
//! the NEWEST change wins.
//!
//! A player marks a song as a favourite and gives it 0–5 stars for the
//! song, for the chart and for the lyrics (the user, 2026-10-06). Three
//! of those belong to the SONG, whichever version is being looked at —
//! the favourite, the song's stars and the lyrics' stars — and one to
//! the CHART, which differs between a song's GS, CL and BG versions.
//! So the file is keyed by player, then by an item key, then by field:
//!
//! - `song:<artist>|<title>` — a song across its versions, by its
//!   normalised artist and its title WITHOUT the twin prefixes
//!   (`[GS] `, `[CL] `, `[BG-01] `). There is no stored id for a song
//!   across versions (each version is a folder with its own id), and a
//!   name survives a library move and another Mac.
//! - `chart:id:<song_id>` — one version, by its folder's permanent id;
//!   `chart:name:<artist>|<title>` for a built-in song, which has none.
//!
//! Every value carries the time it was set. Merging takes the later one
//! per field — a cleared star (0) is a change like any other, so taking
//! a favourite back on one Mac takes it back on the other. On an exact
//! tie the larger value wins, so both devices pick the same. Player ids
//! follow their side's remap, chart ids the library's song remap.
//!
//! What comes from another device is untrusted: a value out of range,
//! a field this build does not know or a key without a known prefix is
//! dropped rather than kept.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::players::{Remap, map};
use crate::scores::SongRemap;

/// The file format this module writes and reads.
pub const VERSION: u32 = 1;

/// The favourite flag of a song (0 or 1).
pub const FAVORITE: &str = "favorite";
/// The song's own stars (0–5).
pub const SONG: &str = "song";
/// The lyrics' stars (0–5), for the song across its versions.
pub const LYRICS: &str = "lyrics";
/// The chart's stars (0–5), for one version.
pub const CHART: &str = "chart";

/// The highest number of stars.
pub const MAX_STARS: u8 = 5;

/// One value and when it was set (ms since the epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    /// Stars, or 1/0 for the favourite.
    pub value: u8,
    /// When it was set.
    pub at: u64,
}

/// Field → value.
pub type Fields = BTreeMap<String, Stamped>;
/// Item key → its fields.
pub type Items = BTreeMap<String, Fields>;

/// The whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ratings {
    /// [`VERSION`].
    pub version: u32,
    /// Player id (as text) → items.
    #[serde(default)]
    pub players: BTreeMap<String, Items>,
}

impl Default for Ratings {
    fn default() -> Self {
        Ratings {
            version: VERSION,
            players: BTreeMap::new(),
        }
    }
}

fn norm(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The key of a song across its versions. `title` must already be
/// stripped of every twin prefix. Pure — tested.
#[must_use]
pub fn song_key(artist: &str, title: &str) -> String {
    format!("song:{}|{}", norm(artist), norm(title))
}

/// The key of one chart version, by its permanent song id.
#[must_use]
pub fn chart_key_id(song_id: &str) -> String {
    format!("chart:id:{song_id}")
}

/// The key of a chart without a song id (a built-in song).
#[must_use]
pub fn chart_key_named(artist: &str, title: &str) -> String {
    format!("chart:name:{}|{}", norm(artist), norm(title))
}

/// The largest value a field may hold, or `None` for a field this
/// build does not know.
#[must_use]
pub fn max_of(field: &str) -> Option<u8> {
    match field {
        FAVORITE => Some(1),
        SONG | LYRICS | CHART => Some(MAX_STARS),
        _ => None,
    }
}

fn valid_key(key: &str) -> bool {
    key.starts_with("song:") || key.starts_with("chart:id:") || key.starts_with("chart:name:")
}

impl Ratings {
    /// The value of one field, 0 when it was never set.
    #[must_use]
    pub fn get(&self, player: &str, key: &str, field: &str) -> u8 {
        self.players
            .get(player)
            .and_then(|items| items.get(key))
            .and_then(|fields| fields.get(field))
            .map_or(0, |s| s.value)
    }

    /// Set one field, clamped to its range. Returns false (and changes
    /// nothing) for a field this build does not know.
    pub fn set(&mut self, player: &str, key: &str, field: &str, value: u8, at: u64) -> bool {
        let Some(max) = max_of(field) else {
            return false;
        };
        self.players
            .entry(player.to_owned())
            .or_default()
            .entry(key.to_owned())
            .or_default()
            .insert(
                field.to_owned(),
                Stamped {
                    value: value.min(max),
                    at,
                },
            );
        true
    }

    /// The same file without anything this build would not write:
    /// unknown fields, unknown key kinds, values out of range. What a
    /// remote device sends goes through this before it is merged.
    #[must_use]
    pub fn sanitized(&self) -> Ratings {
        let mut out = Ratings::default();
        for (player, items) in &self.players {
            for (key, fields) in items {
                if !valid_key(key) {
                    continue;
                }
                for (field, stamped) in fields {
                    if max_of(field).is_some_and(|max| stamped.value <= max) {
                        out.players
                            .entry(player.clone())
                            .or_default()
                            .entry(key.clone())
                            .or_default()
                            .insert(field.clone(), *stamped);
                    }
                }
            }
        }
        out
    }
}

/// Which of two stamped values to keep: the later; on a tie, the
/// larger value, so both devices choose the same.
fn newer(a: Stamped, b: Stamped) -> Stamped {
    if (a.at, a.value) >= (b.at, b.value) {
        a
    } else {
        b
    }
}

fn remap_player(player: &str, remap: &Remap) -> String {
    player
        .parse::<u64>()
        .map_or_else(|_| player.to_owned(), |id| map(remap, id).to_string())
}

fn remap_key(key: &str, songs: &SongRemap) -> String {
    key.strip_prefix("chart:id:")
        .and_then(|id| songs.get(id))
        .map_or_else(|| key.to_owned(), |id| chart_key_id(id))
}

/// Merge two files, each with its side's player remap; chart ids go
/// through the library's song remap. Pure, order-independent and
/// idempotent — tested.
#[must_use]
pub fn merge(
    local: &Ratings,
    local_remap: &Remap,
    remote: &Ratings,
    remote_remap: &Remap,
    songs: &SongRemap,
) -> Ratings {
    let mut out = Ratings::default();
    for (file, remap) in [(local, local_remap), (remote, remote_remap)] {
        for (player, items) in &file.sanitized().players {
            let player = remap_player(player, remap);
            for (key, fields) in items {
                let slot = out
                    .players
                    .entry(player.clone())
                    .or_default()
                    .entry(remap_key(key, songs))
                    .or_default();
                for (field, stamped) in fields {
                    slot.entry(field.clone())
                        .and_modify(|have| *have = newer(*have, *stamped))
                        .or_insert(*stamped);
                }
            }
        }
    }
    out
}

/// How many favourites and how many starred items a file holds, for
/// the sync report.
#[must_use]
pub fn count(file: &Ratings) -> (usize, usize) {
    let mut favorites = 0;
    let mut rated = 0;
    for items in file.players.values() {
        for fields in items.values() {
            if fields.get(FAVORITE).is_some_and(|s| s.value > 0) {
                favorites += 1;
            }
            rated += fields
                .iter()
                .filter(|(f, s)| f.as_str() != FAVORITE && s.value > 0)
                .count();
        }
    }
    (favorites, rated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(entries: &[(&str, &str, &str, u8, u64)]) -> Ratings {
        let mut r = Ratings::default();
        for (player, key, field, value, at) in entries {
            assert!(r.set(player, key, field, *value, *at));
        }
        r
    }

    #[test]
    fn keys_ignore_case_and_spacing_and_name_their_kind() {
        assert_eq!(
            song_key("Rick  Astley", " Never Gonna "),
            "song:rick astley|never gonna"
        );
        assert_eq!(chart_key_id("bb_abc"), "chart:id:bb_abc");
        assert_eq!(chart_key_named("Toto", "Africa"), "chart:name:toto|africa");
    }

    #[test]
    fn set_clamps_to_the_range_and_refuses_unknown_fields() {
        let mut r = Ratings::default();
        assert!(r.set("1", "song:a|b", SONG, 9, 1));
        assert_eq!(r.get("1", "song:a|b", SONG), 5);
        assert!(r.set("1", "song:a|b", FAVORITE, 7, 1));
        assert_eq!(r.get("1", "song:a|b", FAVORITE), 1);
        assert!(!r.set("1", "song:a|b", "mood", 3, 1));
        assert_eq!(r.get("1", "song:a|b", "mood"), 0);
        assert_eq!(r.get("2", "song:a|b", SONG), 0, "never set reads as 0");
    }

    #[test]
    fn the_newest_change_wins_per_field_and_a_cleared_value_travels() {
        let local = with(&[
            ("1", "song:a|b", FAVORITE, 1, 100),
            ("1", "song:a|b", SONG, 4, 300),
        ]);
        // The other Mac took the favourite back later, and starred the
        // lyrics; its song stars are older.
        let remote = with(&[
            ("1", "song:a|b", FAVORITE, 0, 200),
            ("1", "song:a|b", SONG, 2, 250),
            ("1", "song:a|b", LYRICS, 3, 260),
        ]);
        let m = merge(
            &local,
            &Remap::new(),
            &remote,
            &Remap::new(),
            &SongRemap::new(),
        );
        assert_eq!(
            m.get("1", "song:a|b", FAVORITE),
            0,
            "the later un-favourite wins"
        );
        assert_eq!(m.get("1", "song:a|b", SONG), 4, "the later stars win");
        assert_eq!(
            m.get("1", "song:a|b", LYRICS),
            3,
            "a field only one side has is kept"
        );
    }

    #[test]
    fn the_merge_is_order_independent_and_idempotent() {
        let a = with(&[
            ("1", "song:a|b", SONG, 4, 300),
            ("1", "chart:id:x", CHART, 2, 10),
            ("2", "song:c|d", FAVORITE, 1, 5),
        ]);
        let b = with(&[
            ("1", "song:a|b", SONG, 3, 300), // a tie on time: larger value
            ("1", "chart:id:x", CHART, 5, 20),
            ("3", "song:e|f", LYRICS, 1, 7),
        ]);
        let none = Remap::new();
        let songs = SongRemap::new();
        let ab = merge(&a, &none, &b, &none, &songs);
        let ba = merge(&b, &none, &a, &none, &songs);
        assert_eq!(ab, ba, "both devices end on the same state");
        assert_eq!(
            merge(&ab, &none, &ab, &none, &songs),
            ab,
            "a second sync changes nothing"
        );
        assert_eq!(
            ab.get("1", "song:a|b", SONG),
            4,
            "a tie on time keeps the larger value"
        );
        assert_eq!(ab.get("1", "chart:id:x", CHART), 5);
    }

    #[test]
    fn players_and_chart_ids_follow_their_remaps() {
        let local = with(&[("7", "chart:id:old", CHART, 3, 10)]);
        let remote = with(&[("9", "chart:id:new", CHART, 1, 5)]);
        let local_remap: Remap = [(7, 9)].into_iter().collect();
        let songs: SongRemap = [("old".to_owned(), "new".to_owned())].into_iter().collect();
        let m = merge(&local, &local_remap, &remote, &Remap::new(), &songs);
        assert_eq!(m.players.len(), 1, "two ids of one player become one");
        assert_eq!(
            m.get("9", "chart:id:new", CHART),
            3,
            "the later rating on the surviving id"
        );
        assert!(
            !m.players["9"].contains_key("chart:id:old"),
            "nothing left under the old id"
        );
    }

    #[test]
    fn what_another_device_sends_is_checked_before_it_is_kept() {
        let mut remote = Ratings::default();
        let fields: Fields = [
            ("song".to_owned(), Stamped { value: 9, at: 1 }), // out of range
            ("mood".to_owned(), Stamped { value: 1, at: 1 }), // unknown field
            ("lyrics".to_owned(), Stamped { value: 2, at: 1 }),
        ]
        .into_iter()
        .collect();
        let mut items = Items::new();
        items.insert("song:a|b".to_owned(), fields.clone());
        items.insert("../etc".to_owned(), fields); // unknown key kind
        remote.players.insert("1".to_owned(), items);
        let m = merge(
            &Ratings::default(),
            &Remap::new(),
            &remote,
            &Remap::new(),
            &SongRemap::new(),
        );
        let kept = &m.players["1"];
        assert_eq!(kept.len(), 1, "a key of no known kind is dropped");
        assert_eq!(kept["song:a|b"].len(), 1, "only the valid field survives");
        assert_eq!(m.get("1", "song:a|b", LYRICS), 2);
    }

    #[test]
    fn the_file_round_trips_and_counts() {
        let r = with(&[
            ("1", "song:a|b", FAVORITE, 1, 1),
            ("1", "song:a|b", SONG, 3, 1),
            ("1", "song:c|d", FAVORITE, 0, 2),
            ("1", "chart:id:x", CHART, 0, 3),
        ]);
        let text = serde_json::to_string(&r).expect("serialises");
        let back: Ratings = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, r);
        assert_eq!(count(&r), (1, 1), "zeros are changes, not ratings");
    }
}
