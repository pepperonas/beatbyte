//! The four playable difficulty levels.

use serde::{Deserialize, Serialize};

/// A chart difficulty. The difficulty system is data-driven: gameplay
/// code never branches on specific difficulty values — charts define
/// their own content per difficulty, and generation parameters are
/// looked up from difficulty profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    /// Sparse charts on beat level, no fifth lane.
    Easy,
    /// Moderate density, simple chords.
    Medium,
    /// Full lane usage, chords, sustains, HOPOs.
    Hard,
    /// Everything the song demands.
    Expert,
}

impl Difficulty {
    /// All difficulties, easiest first.
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ];

    /// A stable lowercase identifier (matches the chart format).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
            Difficulty::Expert => "expert",
        }
    }

    /// The difficulty an id names, or `None` for anything else.
    ///
    /// The inverse of [`Difficulty::id`], and the reader for every
    /// place a difficulty arrives as text: the play history stores it
    /// lowercase, and the scoreboard's legacy keys ended in one.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Difficulty> {
        Difficulty::ALL.into_iter().find(|d| d.id() == id)
    }

    /// Human-readable display name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Expert => "Expert",
        }
    }

    /// Position in [`Difficulty::ALL`] (easiest = 0).
    #[must_use]
    pub const fn rank(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Medium => 1,
            Difficulty::Hard => 2,
            Difficulty::Expert => 3,
        }
    }

    /// The difficulty to play when a chart offers `offered` and the
    /// player prefers `preferred`.
    ///
    /// Returns `preferred` when the chart has it; otherwise the
    /// nearest offered step (ties break toward the easier one). An
    /// empty offer returns `None` — the caller keeps whatever it had.
    /// Pure — tested.
    #[must_use]
    pub fn among(preferred: Difficulty, offered: &[Difficulty]) -> Option<Difficulty> {
        if offered.is_empty() {
            return None;
        }
        if offered.contains(&preferred) {
            return Some(preferred);
        }
        let want = preferred.rank();
        offered.iter().copied().min_by_key(|d| {
            // Prefer easier on a tie so two equal distances do not
            // depend on chart order.
            (d.rank().abs_diff(want), d.rank())
        })
    }
}

impl core::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn difficulties_are_ordered() {
        assert!(Difficulty::Easy < Difficulty::Medium);
        assert!(Difficulty::Medium < Difficulty::Hard);
        assert!(Difficulty::Hard < Difficulty::Expert);
    }

    #[test]
    fn serde_uses_lowercase_ids() {
        for d in Difficulty::ALL {
            let json = serde_json::to_string(&d).unwrap();
            assert_eq!(json, format!("\"{}\"", d.id()));
            let back: Difficulty = serde_json::from_str(&json).unwrap();
            assert_eq!(back, d);
        }
    }

    #[test]
    fn among_keeps_the_preferred_when_offered_and_falls_back_nearest() {
        assert_eq!(
            Difficulty::among(Difficulty::Hard, &[Difficulty::Easy, Difficulty::Hard]),
            Some(Difficulty::Hard)
        );
        // Hard gone: Medium is one step, Expert is one step — easier wins.
        assert_eq!(
            Difficulty::among(
                Difficulty::Hard,
                &[Difficulty::Easy, Difficulty::Medium, Difficulty::Expert]
            ),
            Some(Difficulty::Medium)
        );
        assert_eq!(
            Difficulty::among(Difficulty::Expert, &[Difficulty::Easy]),
            Some(Difficulty::Easy)
        );
        assert_eq!(Difficulty::among(Difficulty::Hard, &[]), None);
    }
}
