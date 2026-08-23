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
}
