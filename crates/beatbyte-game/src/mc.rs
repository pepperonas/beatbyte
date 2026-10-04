//! The MC set: a playlist played as ONE continuous performance.
//!
//! Songs queued in the browser (`Q`) play back to back with a DJ
//! crossfade between them — the outgoing song keeps sounding while
//! the next one fades in on the audio thread's second player, and
//! the next chart's notes are already approaching during the
//! handover's count-in. Every song is prepared UP FRONT, so a
//! library rescan mid-set cannot pull a queued song out from under
//! the performance.

use bevy::prelude::*;

use crate::boot::LoadedSong;
use beatbyte_core::Difficulty;

/// Seconds the outgoing song keeps sounding while the next fades in.
pub const MC_CROSSFADE_S: f32 = 4.0;

/// The running set: every song pre-loaded, in play order.
#[derive(Resource)]
pub struct McSet {
    /// The prepared songs, in order.
    pub songs: Vec<LoadedSong>,
    /// Index of the song currently playing.
    pub position: usize,
}

impl McSet {
    /// Whether another song follows the current one.
    #[must_use]
    pub fn has_next(&self) -> bool {
        next_position(self.position, self.songs.len()).is_some()
    }

    /// Step to the next song and return it. `None` at the set's end.
    pub fn advance(&mut self) -> Option<&LoadedSong> {
        let next = next_position(self.position, self.songs.len())?;
        self.position = next;
        self.songs.get(self.position)
    }
}

/// The set's stepping rule: forward only, never wrapping, and a
/// refused step moves nothing. Pure — tested.
#[must_use]
pub fn next_position(position: usize, len: usize) -> Option<usize> {
    let next = position + 1;
    (next < len).then_some(next)
}

/// A queued song by what it IS, not where it sits in the list: its
/// folder, or the built-in it is.
///
/// The queue used to hold library indices, and a rescan while songs
/// were queued — an import, a delete, a revision chosen — shifts every
/// index after the change: P then played the neighbours of what was
/// queued. A folder survives a rescan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueuedSong {
    /// A song folder on disk.
    Folder(std::path::PathBuf),
    /// A built-in song, by its built-in number.
    Builtin(usize),
}

impl QueuedSong {
    /// The key of a library entry. `None` for a file entry whose chart
    /// has no folder (never in practice).
    #[must_use]
    pub fn of(entry: &crate::library::SongEntry) -> Option<QueuedSong> {
        match &entry.source {
            crate::library::SongSource::File { chart_path, .. } => chart_path
                .parent()
                .map(|folder| QueuedSong::Folder(folder.to_path_buf())),
            crate::library::SongSource::Builtin(index) => Some(QueuedSong::Builtin(*index)),
        }
    }
}

/// The browser-side queue while the set is being put together, in the
/// order songs were added.
#[derive(Resource, Default)]
pub struct McQueue(pub Vec<QueuedSong>);

/// The queue against the library as it is NOW: the library index of
/// every queued song that still exists, in queue order, and the queued
/// songs that do not (deleted since). `library` is each entry's key,
/// in library order. Pure — tested.
#[must_use]
pub fn resolve(
    queue: &[QueuedSong],
    library: &[Option<QueuedSong>],
) -> (Vec<usize>, Vec<QueuedSong>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for queued in queue {
        match library.iter().position(|key| key.as_ref() == Some(queued)) {
            Some(index) => found.push(index),
            None => missing.push(queued.clone()),
        }
    }
    (found, missing)
}

/// The difficulty a set song actually plays on: the selected one
/// when the chart offers it, otherwise the chart's first offered
/// difficulty — a set must not die because one song lacks Expert.
/// Pure — tested.
#[must_use]
pub fn set_difficulty(offered: &[Difficulty], selected: Difficulty) -> Option<Difficulty> {
    if offered.contains(&selected) {
        return Some(selected);
    }
    offered.first().copied()
}

/// Sent the moment the set swaps songs mid-gameplay, so the
/// per-song scenery (fret bars, phrase bands) rebuilds for the new
/// chart.
#[derive(Message)]
pub struct McSwapped;

/// A run condition: did the set just swap songs?
pub fn mc_swapped(mut swaps: MessageReader<McSwapped>) -> bool {
    swaps.read().count() > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_advances_in_order_and_ends_honestly() {
        // Forward only, never wrapping, refused steps move nothing.
        assert_eq!(next_position(0, 3), Some(1));
        assert_eq!(next_position(1, 3), Some(2));
        assert_eq!(next_position(2, 3), None, "the set ends, it does not wrap");
        assert_eq!(next_position(0, 1), None, "a one-song set has no next");
        assert_eq!(next_position(0, 0), None, "an empty set has no next");
    }

    #[test]
    fn a_queued_song_is_found_where_it_is_now_not_where_it_was() {
        use std::path::PathBuf;
        let song = |name: &str| Some(QueuedSong::Folder(PathBuf::from(name)));
        let queue = vec![
            QueuedSong::Folder(PathBuf::from("b")),
            QueuedSong::Builtin(0),
            QueuedSong::Folder(PathBuf::from("d")),
        ];
        // Queued against [builtin 0, a, b, c, d]; then "a" was
        // imported... no: "x" was imported in front and "c" deleted.
        let library = vec![
            Some(QueuedSong::Builtin(0)),
            song("x"),
            song("a"),
            song("b"),
            song("d"),
        ];
        let (found, missing) = resolve(&queue, &library);
        assert_eq!(
            found,
            vec![3, 0, 4],
            "each song where it is now, in queue order"
        );
        assert!(missing.is_empty());
        // A queued song that was deleted is reported, not replaced by
        // whatever took its place.
        let library = vec![Some(QueuedSong::Builtin(0)), song("b")];
        let (found, missing) = resolve(&queue, &library);
        assert_eq!(found, vec![1, 0]);
        assert_eq!(missing, vec![QueuedSong::Folder(PathBuf::from("d"))]);
        // An entry without a key matches nothing.
        let (found, _) = resolve(&queue, &[None, None]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_missing_difficulty_falls_back_instead_of_killing_the_set() {
        use Difficulty::{Easy, Expert, Medium};
        assert_eq!(
            set_difficulty(&[Easy, Medium, Expert], Expert),
            Some(Expert)
        );
        assert_eq!(
            set_difficulty(&[Easy, Medium], Expert),
            Some(Easy),
            "the chart's first offered difficulty carries the song"
        );
        assert_eq!(
            set_difficulty(&[], Expert),
            None,
            "an empty chart is honest"
        );
    }
}
