//! The song browser's rows: one per SONG, not one per chart.
//!
//! A song in the library can come as several entries — the original,
//! its guitar-study twin `[GS]`, its classic twin `[CL]`, downloads of
//! community charts `[BG-01]`, `[BG-02]` … The browser used to list
//! them all as rows of a tree, so a library of 466 folders read as a
//! wall of `[BG-01]` prefixes. Now a row is a [`Family`]: the song,
//! with its versions as a choice in the detail panel.
//!
//! Families are built over the WHOLE library in its sorted order, not
//! over the filtered list: a search that only matches a `[BG-01]`
//! title must still show the song it belongs to, with the original
//! beside it — a twin that lost its original to the filter would stand
//! alone as a song of its own. Everything here is pure and tested.

use std::path::Path;

use crate::library::SongEntry;

/// One row of the browser: a song and every version of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    /// The song itself — the entry every other member is a twin of.
    pub head: usize,
    /// Every member, the head first, then its twins in the order the
    /// browser has always listed them (`pair_twins`).
    pub members: Vec<usize>,
}

/// The entry a member ultimately belongs to: its original's original,
/// and so on. Guarded against a loop, which titles cannot form (each
/// link's title is shorter than the one naming it) but a library of
/// strangely named folders should not be able to hang the browser.
fn root_of(entries: &[SongEntry], full: &[usize], member: usize) -> usize {
    let mut current = member;
    for _ in 0..entries.len() {
        match crate::song_select::original_in(entries, full, current) {
            Some(original) if original != current => current = original,
            _ => return current,
        }
    }
    current
}

/// The families to show. `full` is the whole library in display order
/// (sorted, NOT filtered); `filtered` is what the filter let through,
/// best match first. A family is shown when any member got through,
/// and families appear in the order of their first member in
/// `filtered` — so the song the search ranked first is the first row.
#[must_use]
pub fn families(entries: &[SongEntry], full: &[usize], filtered: &[usize]) -> Vec<Family> {
    let paired = crate::song_select::pair_twins(entries, full.to_vec());
    let mut members: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for &i in &paired {
        members
            .entry(root_of(entries, full, i))
            .or_default()
            .push(i);
    }
    let mut shown = vec![false; entries.len()];
    let mut out = Vec::new();
    for &i in filtered {
        let head = root_of(entries, full, i);
        if std::mem::replace(&mut shown[head], true) {
            continue;
        }
        let mut list = members.remove(&head).unwrap_or_else(|| vec![head]);
        // The head first, whatever the pairing did.
        if let Some(at) = list.iter().position(|&m| m == head) {
            list.remove(at);
        }
        list.insert(0, head);
        out.push(Family {
            head,
            members: list,
        });
    }
    out
}

/// Which member plays when the row is selected: the one the player
/// chose this session, else the one played last, else the song
/// itself. `chosen` is a folder (it survives a rescan, an index does
/// not); `last_played` answers with a moment for a member that has
/// one. Pure — tested.
#[must_use]
pub fn default_member(
    entries: &[SongEntry],
    family: &Family,
    chosen: Option<&Path>,
    last_played: impl Fn(&SongEntry) -> Option<u64>,
) -> usize {
    if let Some(folder) = chosen
        && let Some(&member) = family
            .members
            .iter()
            .find(|&&m| crate::song_select::entry_folder(&entries[m]).as_deref() == Some(folder))
    {
        return member;
    }
    family
        .members
        .iter()
        .filter_map(|&m| last_played(&entries[m]).map(|at| (at, m)))
        // The latest moment; on a tie the earlier member (the head).
        .max_by_key(|&(at, m)| (at, std::cmp::Reverse(m)))
        .map_or(family.head, |(_, m)| m)
}

/// The title a row shows: the song's own, without a version prefix —
/// the version is a property of the row, chosen in the panel.
#[must_use]
pub fn row_title(entries: &[SongEntry], family: &Family) -> String {
    let title = &entries[family.head].title;
    beatbyte_chart::twin::base_title(title)
        .and_then(|base| {
            // A family whose head is itself a BG download (no original
            // in the library) still reads as the song.
            beatbyte_chart::twin::split_bridge_title(title)
                .map(|(_, rest)| rest.to_owned())
                .or_else(|| Some(base.to_owned()))
        })
        .unwrap_or_else(|| title.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Polish, SongSource};
    use std::path::PathBuf;

    fn entry(title: &str, artist: &str, folder: &str) -> SongEntry {
        SongEntry {
            title: title.to_owned(),
            artist: artist.to_owned(),
            bpm: 120.0,
            duration_s: Some(200.0),
            difficulties: vec![],
            note_counts: vec![],
            genre: None,
            has_lyrics: false,
            preview_start_s: None,
            loudness: None,
            song_id: None,
            polish: Polish {
                chart_version: None,
                aligned: false,
                has_lyrics: false,
            },
            source: SongSource::File {
                chart_path: PathBuf::from(format!("/lib/{folder}/chart.json")),
                audio_path: PathBuf::from(format!("/lib/{folder}/song.m4a")),
            },
        }
    }

    fn library() -> Vec<SongEntry> {
        vec![
            entry("Maria", "Blondie", "maria"),                 // 0
            entry("[GS] Maria", "Blondie", "gs-maria"),         // 1
            entry("[CL] [GS] Maria", "Blondie", "cl-gs-maria"), // 2
            entry("Heroes", "David Bowie", "heroes"),           // 3
            entry("[BG-01] Drive", "Incubus", "bg1-drive"),     // 4 (no original)
            entry("[BG-02] Drive", "Incubus", "bg2-drive"),     // 5
            entry("[BG-01] Maria", "Blondie", "bg1-maria"),     // 6
        ]
    }

    #[test]
    fn every_entry_belongs_to_exactly_one_family_and_twins_join_their_song() {
        let entries = library();
        let all: Vec<usize> = (0..entries.len()).collect();
        let families = families(&entries, &all, &all);
        let heads: Vec<usize> = families.iter().map(|f| f.head).collect();
        assert_eq!(heads, vec![0, 3, 4], "three songs: Maria, Heroes, Drive");
        let maria = &families[0];
        assert_eq!(maria.members[0], 0, "the song itself first");
        let mut members = maria.members.clone();
        members.sort_unstable();
        assert_eq!(members, vec![0, 1, 2, 6], "GS, CL of the GS, and BG-01");
        assert_eq!(families[2].members, vec![4, 5], "two downloads, one song");
        let mut seen: Vec<usize> = families.iter().flat_map(|f| f.members.clone()).collect();
        seen.sort_unstable();
        assert_eq!(seen, all, "nobody dropped, nobody twice");
    }

    #[test]
    fn a_search_that_matches_only_a_version_shows_the_whole_song() {
        let entries = library();
        let all: Vec<usize> = (0..entries.len()).collect();
        // The filter let only the classic twin through.
        let families = families(&entries, &all, &[2]);
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].head, 0);
        assert!(families[0].members.contains(&1), "with its other versions");
    }

    #[test]
    fn families_follow_the_filters_ranking() {
        let entries = library();
        let all: Vec<usize> = (0..entries.len()).collect();
        let families = families(&entries, &all, &[3, 5, 0]);
        let heads: Vec<usize> = families.iter().map(|f| f.head).collect();
        assert_eq!(heads, vec![3, 4, 0]);
    }

    #[test]
    fn the_chosen_version_wins_then_the_last_played_then_the_song() {
        let entries = library();
        let maria = Family {
            head: 0,
            members: vec![0, 1, 2, 6],
        };
        let never = |_: &SongEntry| None;
        assert_eq!(default_member(&entries, &maria, None, never), 0);
        let played = |e: &SongEntry| match e.title.as_str() {
            "[GS] Maria" => Some(10),
            "[BG-01] Maria" => Some(20),
            _ => None,
        };
        assert_eq!(default_member(&entries, &maria, None, played), 6);
        let chosen = PathBuf::from("/lib/gs-maria");
        assert_eq!(default_member(&entries, &maria, Some(&chosen), played), 1);
        let gone = PathBuf::from("/lib/elsewhere");
        assert_eq!(
            default_member(&entries, &maria, Some(&gone), played),
            6,
            "a choice that is no longer a member falls back"
        );
    }

    #[test]
    fn a_row_reads_as_the_song_without_its_version_prefix() {
        let entries = library();
        let head = |h: usize| Family {
            head: h,
            members: vec![h],
        };
        assert_eq!(row_title(&entries, &head(0)), "Maria");
        assert_eq!(row_title(&entries, &head(4)), "Drive");
    }
}
