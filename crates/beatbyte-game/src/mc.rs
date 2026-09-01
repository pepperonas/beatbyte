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

/// The browser-side queue while the set is being put together:
/// library indices, in the order they were added.
#[derive(Resource, Default)]
pub struct McQueue(pub Vec<usize>);

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
