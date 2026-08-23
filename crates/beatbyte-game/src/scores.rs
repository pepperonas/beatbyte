//! High scores: a small persistent save file in the platform data
//! directory. No accounts, no network — a local record of glory.

use std::collections::HashMap;

use beatbyte_core::Difficulty;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// One best result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BestScore {
    /// The score achieved.
    pub score: u64,
    /// Weighted accuracy 0.0–1.0.
    pub accuracy: f64,
    /// Longest streak.
    pub best_streak: u32,
}

/// All best results, keyed by song + difficulty.
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreBoard {
    /// `title|artist|difficulty` → best.
    pub entries: HashMap<String, BestScore>,
}

impl ScoreBoard {
    fn key(title: &str, artist: &str, difficulty: Difficulty) -> String {
        format!("{title}|{artist}|{}", difficulty.id())
    }

    /// The stored best for a song/difficulty.
    #[must_use]
    pub fn best(&self, title: &str, artist: &str, difficulty: Difficulty) -> Option<BestScore> {
        self.entries
            .get(&Self::key(title, artist, difficulty))
            .copied()
    }

    /// Record a result. Returns `true` when it is a new record.
    pub fn record(
        &mut self,
        title: &str,
        artist: &str,
        difficulty: Difficulty,
        result: BestScore,
    ) -> bool {
        let key = Self::key(title, artist, difficulty);
        match self.entries.get(&key) {
            Some(best) if best.score >= result.score => false,
            _ => {
                self.entries.insert(key, result);
                true
            }
        }
    }
}

/// Where the scores file lives.
#[must_use]
pub fn scores_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("scores.json"))
}

/// Load the scoreboard (missing/corrupt → empty with a warning).
#[must_use]
pub fn load_scores() -> ScoreBoard {
    let Some(path) = scores_path() else {
        return ScoreBoard::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            warn!(
                "scores file {} is invalid ({error}); starting fresh",
                path.display()
            );
            ScoreBoard::default()
        }),
        Err(_) => ScoreBoard::default(),
    }
}

/// Persist the scoreboard (best effort).
pub fn save_scores(scores: &ScoreBoard) {
    let Some(path) = scores_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &path,
            serde_json::to_string_pretty(scores).unwrap_or_default(),
        )
    };
    if let Err(error) = write() {
        warn!("cannot save scores to {}: {error}", path.display());
    }
}

/// Plugin: loads the scoreboard at startup.
pub struct ScoresPlugin;

impl Plugin for ScoresPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_scores());
    }
}
