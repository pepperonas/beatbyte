//! Favourites and star ratings on disk (`ratings.json`).
//!
//! The model and its merge rule live in [`beatbyte_sync::ratings`];
//! this module keeps the file as a resource and says which keys a
//! library entry has. Same shape as [`crate::players`]: loaded once at
//! startup, written when it changes, a failed write warns rather than
//! taking the browser with it.
//!
//! Three values belong to a SONG, whichever of its versions is shown —
//! the favourite, the song's stars and the lyrics' stars — and one to a
//! CHART version (GS, CL and every BG number are different charts).
//! Everything is the current player's; with nobody selected, it is the
//! shared slot `0`.

use beatbyte_sync::ratings::{Ratings, chart_key_id, chart_key_named, song_key};
use bevy::prelude::*;

use crate::library::SongEntry;
use crate::players::Players;

/// The ratings as a resource, with a counter that moves on every change
/// (the browser redraws its rows when it moves).
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct SongRatings {
    /// The file.
    pub ratings: Ratings,
    /// Bumped on every change this session.
    pub generation: u64,
}

/// Where the ratings live — beside `scores.json`.
#[must_use]
pub fn ratings_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("ratings.json"))
}

/// Load the ratings (missing → empty; unreadable → empty, the file
/// left as it is to be looked at, and a warning).
#[must_use]
pub fn load_ratings() -> SongRatings {
    let Some(path) = ratings_path() else {
        return SongRatings::default();
    };
    let ratings = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Ratings>(&text).map_or_else(
            |error| {
                warn!("ratings: {} is not readable: {error}", path.display());
                Ratings::default()
            },
            |r| r.sanitized(),
        ),
        Err(_) => Ratings::default(),
    };
    SongRatings {
        ratings,
        generation: 0,
    }
}

/// Write the ratings out (atomically: a crash mid-write must not cost
/// every rating).
pub fn save_ratings(ratings: &SongRatings) {
    // A unit test must never write into the player's real data folder.
    if cfg!(test) {
        return;
    }
    let Some(path) = ratings_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&ratings.ratings)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    };
    if let Err(error) = write() {
        warn!("ratings: could not write {}: {error}", path.display());
    }
}

/// The title without any twin prefix: `[CL] [GS] Maria` → `Maria`.
#[must_use]
pub fn bare_title(title: &str) -> &str {
    let mut title = title;
    while let Some(base) = beatbyte_chart::twin::base_title(title) {
        title = base;
    }
    title
}

/// The key of the SONG an entry is a version of. Pure — tested.
#[must_use]
pub fn song_key_of(entry: &SongEntry) -> String {
    song_key(&entry.artist, bare_title(&entry.title))
}

/// The key of this one chart version. Pure — tested.
#[must_use]
pub fn chart_key_of(entry: &SongEntry) -> String {
    entry.song_id.as_deref().map_or_else(
        || chart_key_named(&entry.artist, &entry.title),
        chart_key_id,
    )
}

/// The player the ratings are filed under: the selected one, else `0`.
#[must_use]
pub fn player_key(players: &Players) -> String {
    players
        .0
        .current()
        .map_or_else(|| "0".to_owned(), |p| p.id.to_string())
}

impl SongRatings {
    /// Is the entry's song a favourite of this player?
    #[must_use]
    pub fn favorite(&self, player: &str, entry: &SongEntry) -> bool {
        self.ratings.get(
            player,
            &song_key_of(entry),
            beatbyte_sync::ratings::FAVORITE,
        ) > 0
    }

    /// The value of one field for an entry: chart stars by version,
    /// everything else by song.
    #[must_use]
    pub fn value(&self, player: &str, entry: &SongEntry, field: &str) -> u8 {
        self.ratings.get(player, &key_for(entry, field), field)
    }

    /// Set one field now, and write the file.
    pub fn set(&mut self, player: &str, entry: &SongEntry, field: &str, value: u8) {
        if self.ratings.set(
            player,
            &key_for(entry, field),
            field,
            value,
            crate::players::now_ms(),
        ) {
            self.generation += 1;
            info!(
                "rating: {} {field} = {value} (player {player})",
                entry.title
            );
            save_ratings(self);
        }
    }
}

fn key_for(entry: &SongEntry, field: &str) -> String {
    if field == beatbyte_sync::ratings::CHART {
        chart_key_of(entry)
    } else {
        song_key_of(entry)
    }
}

/// Inserts the ratings at startup.
pub struct RatingsPlugin;

impl Plugin for RatingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_ratings());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Polish, SongSource};
    use beatbyte_sync::ratings::{CHART, FAVORITE, LYRICS, SONG};
    use std::path::PathBuf;

    fn entry(title: &str, id: Option<&str>) -> SongEntry {
        SongEntry {
            title: title.to_owned(),
            artist: "Rick Astley".to_owned(),
            bpm: 113.0,
            duration_s: Some(213.0),
            difficulties: vec![],
            note_counts: vec![],
            genre: None,
            has_lyrics: false,
            preview_start_s: None,
            loudness: None,
            song_id: id.map(str::to_owned),
            polish: Polish::default(),
            source: SongSource::File {
                chart_path: PathBuf::from("/lib/x/chart.json"),
                audio_path: PathBuf::from("/lib/x/song.m4a"),
            },
        }
    }

    #[test]
    fn every_version_of_a_song_shares_its_song_key() {
        let keys: Vec<String> = [
            "Never Gonna Give You Up",
            "[GS] Never Gonna Give You Up",
            "[CL] [GS] Never Gonna Give You Up",
            "[BG-02] Never Gonna Give You Up",
        ]
        .into_iter()
        .map(|t| song_key_of(&entry(t, None)))
        .collect();
        assert!(keys.iter().all(|k| k == &keys[0]), "{keys:?}");
    }

    #[test]
    fn a_chart_is_its_version_by_id_and_by_name_without_one() {
        assert_eq!(
            chart_key_of(&entry("[BG-01] X", Some("bb_1"))),
            "chart:id:bb_1"
        );
        assert_ne!(
            chart_key_of(&entry("[BG-01] X", None)),
            chart_key_of(&entry("[BG-02] X", None)),
            "two versions without ids are still two charts"
        );
    }

    #[test]
    fn song_fields_follow_the_song_and_chart_stars_the_version() {
        let mut r = SongRatings::default();
        let normal = entry("Never Gonna", Some("bb_a"));
        let bridge = entry("[BG-01] Never Gonna", Some("bb_b"));
        r.ratings.set("1", &song_key_of(&normal), FAVORITE, 1, 1);
        r.ratings.set("1", &song_key_of(&normal), SONG, 4, 1);
        r.ratings.set("1", &song_key_of(&normal), LYRICS, 2, 1);
        r.ratings.set("1", &chart_key_of(&normal), CHART, 5, 1);
        assert!(r.favorite("1", &bridge), "the favourite is the song's");
        assert_eq!(r.value("1", &bridge, SONG), 4);
        assert_eq!(r.value("1", &bridge, LYRICS), 2);
        assert_eq!(
            r.value("1", &bridge, CHART),
            0,
            "the BG chart is another chart"
        );
        assert_eq!(r.value("1", &normal, CHART), 5);
        assert!(
            !r.favorite("2", &normal),
            "another player's favourites are theirs"
        );
    }
}
