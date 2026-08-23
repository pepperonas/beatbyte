//! The five gameplay lanes and lane sets.
//!
//! BeatByte uses the classic five-lane fret model. Lanes are identified
//! by index (0–4) everywhere in the domain; colors and visuals are a
//! presentation concern and live in the theme system.

use serde::{Deserialize, Serialize};

/// Number of gameplay lanes in the classic fret model.
pub const LANE_COUNT: usize = 5;

/// One of the five fret lanes, ordered left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Lane {
    /// Leftmost lane (index 0).
    One,
    /// Lane index 1.
    Two,
    /// Center lane (index 2).
    Three,
    /// Lane index 3.
    Four,
    /// Rightmost lane (index 4).
    Five,
}

impl Lane {
    /// All lanes, left to right.
    pub const ALL: [Lane; LANE_COUNT] = [Lane::One, Lane::Two, Lane::Three, Lane::Four, Lane::Five];

    /// The zero-based lane index (0–4).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Build a lane from a zero-based index, if it is in range.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Lane> {
        match index {
            0 => Some(Lane::One),
            1 => Some(Lane::Two),
            2 => Some(Lane::Three),
            3 => Some(Lane::Four),
            4 => Some(Lane::Five),
            _ => None,
        }
    }
}

impl From<Lane> for u8 {
    fn from(lane: Lane) -> Self {
        lane as u8
    }
}

impl TryFrom<u8> for Lane {
    type Error = InvalidLane;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Lane::from_index(value as usize).ok_or(InvalidLane(value))
    }
}

/// Error returned when a lane index is out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLane(pub u8);

impl core::fmt::Display for InvalidLane {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid lane index {} (expected 0–4)", self.0)
    }
}

impl core::error::Error for InvalidLane {}

/// A set of lanes, used for chords and held frets.
///
/// Backed by a bitmask (bit *n* = lane *n*), so set operations are cheap
/// and equality is exact — important in the hot judgment path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LaneSet(u8);

impl LaneSet {
    /// The empty set.
    pub const EMPTY: LaneSet = LaneSet(0);

    /// Bitmask covering all five lanes.
    const FULL_MASK: u8 = 0b0001_1111;

    /// Create a set containing a single lane.
    #[must_use]
    pub const fn single(lane: Lane) -> LaneSet {
        LaneSet(1 << lane.index() as u8)
    }

    /// Create a set from an iterator of lanes.
    pub fn from_lanes<I: IntoIterator<Item = Lane>>(lanes: I) -> LaneSet {
        let mut set = LaneSet::EMPTY;
        for lane in lanes {
            set.insert(lane);
        }
        set
    }

    /// Create a set directly from a bitmask; out-of-range bits are dropped.
    #[must_use]
    pub const fn from_bits_truncated(bits: u8) -> LaneSet {
        LaneSet(bits & Self::FULL_MASK)
    }

    /// The raw bitmask.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Add a lane to the set.
    pub const fn insert(&mut self, lane: Lane) {
        self.0 |= 1 << lane.index() as u8;
    }

    /// Remove a lane from the set.
    pub const fn remove(&mut self, lane: Lane) {
        self.0 &= !(1 << lane.index() as u8);
    }

    /// Whether the given lane is in the set.
    #[must_use]
    pub const fn contains(self, lane: Lane) -> bool {
        self.0 & (1 << lane.index() as u8) != 0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Number of lanes in the set.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The highest (rightmost) lane in the set, if any.
    ///
    /// Used by the anchoring rule: for single notes, only the highest
    /// held fret has to match.
    #[must_use]
    pub const fn highest(self) -> Option<Lane> {
        if self.0 == 0 {
            return None;
        }
        Lane::from_index(7 - self.0.leading_zeros() as usize)
    }

    /// Iterate the lanes in the set, left to right.
    pub fn iter(self) -> impl Iterator<Item = Lane> {
        Lane::ALL
            .into_iter()
            .filter(move |lane| self.contains(*lane))
    }
}

impl FromIterator<Lane> for LaneSet {
    fn from_iter<I: IntoIterator<Item = Lane>>(iter: I) -> Self {
        LaneSet::from_lanes(iter)
    }
}

impl From<Lane> for LaneSet {
    fn from(lane: Lane) -> Self {
        LaneSet::single(lane)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lane_indices_round_trip() {
        for (i, lane) in Lane::ALL.iter().enumerate() {
            assert_eq!(lane.index(), i);
            assert_eq!(Lane::from_index(i), Some(*lane));
        }
    }

    #[test]
    fn out_of_range_index_is_rejected() {
        assert_eq!(Lane::from_index(5), None);
        assert_eq!(Lane::try_from(5u8), Err(InvalidLane(5)));
        assert_eq!(Lane::try_from(255u8), Err(InvalidLane(255)));
    }

    #[test]
    fn there_are_exactly_five_lanes() {
        assert_eq!(Lane::ALL.len(), LANE_COUNT);
        assert_eq!(LANE_COUNT, 5);
    }

    #[test]
    fn lane_set_insert_remove_contains() {
        let mut set = LaneSet::EMPTY;
        assert!(set.is_empty());

        set.insert(Lane::Two);
        set.insert(Lane::Five);
        assert!(set.contains(Lane::Two));
        assert!(set.contains(Lane::Five));
        assert!(!set.contains(Lane::One));
        assert_eq!(set.len(), 2);

        set.remove(Lane::Two);
        assert!(!set.contains(Lane::Two));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn lane_set_highest() {
        assert_eq!(LaneSet::EMPTY.highest(), None);
        assert_eq!(LaneSet::single(Lane::One).highest(), Some(Lane::One));

        let set = LaneSet::from_lanes([Lane::One, Lane::Four]);
        assert_eq!(set.highest(), Some(Lane::Four));

        let all = LaneSet::from_lanes(Lane::ALL);
        assert_eq!(all.highest(), Some(Lane::Five));
    }

    #[test]
    fn lane_set_iterates_left_to_right() {
        let set = LaneSet::from_lanes([Lane::Four, Lane::One, Lane::Three]);
        let lanes: Vec<Lane> = set.iter().collect();
        assert_eq!(lanes, vec![Lane::One, Lane::Three, Lane::Four]);
    }

    #[test]
    fn lane_set_truncates_out_of_range_bits() {
        let set = LaneSet::from_bits_truncated(0b1110_0001);
        assert_eq!(set.bits(), 0b0000_0001);
        assert_eq!(set.len(), 1);
    }
}
