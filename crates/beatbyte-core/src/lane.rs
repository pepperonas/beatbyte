//! The five gameplay lanes.
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

#[cfg(test)]
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
}
