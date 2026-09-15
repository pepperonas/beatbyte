//! The roster on disk: who plays on this machine.
//!
//! Persistence only — the model is [`beatbyte_core::player`], the
//! screen is [`crate::players_ui`]. Same shape as [`crate::scores`]:
//! a small JSON file beside `scores.json`, loaded once at startup,
//! written when it changes, and a failure to write warns rather than
//! taking gameplay with it.

use beatbyte_core::history::claim_unattributed;
use beatbyte_core::player::{PlayerId, Roster};
use bevy::prelude::*;

/// The roster as a resource. Wraps the core model so the game can
/// hold it in the ECS without core knowing what an ECS is.
#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub struct Players(pub Roster);

/// Where the roster lives — beside `scores.json` and `history.jsonl`.
#[must_use]
pub fn roster_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("players.json"))
}

/// Load the roster (missing or corrupt → empty, with a warning).
///
/// A corrupt roster is NOT fatal and is not repaired in place: the
/// file is left exactly as it is so it can be inspected, and the
/// session runs with an empty roster. Overwriting somebody's list of
/// names because one byte went bad is the worse failure.
#[must_use]
pub fn load_roster() -> Players {
    let Some(path) = roster_path() else {
        return Players::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Roster>(&text) {
            Ok(roster) => Players(roster),
            Err(error) => {
                warn!("players: {} is not readable: {error}", path.display());
                Players::default()
            }
        },
        // No file yet is the normal first run, not a problem.
        Err(_) => Players::default(),
    }
}

/// Write the roster out.
pub fn save_roster(players: &Players) {
    let Some(path) = roster_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&players.0)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::write(&path, text)
    };
    if let Err(error) = write() {
        warn!("cannot save the roster to {}: {error}", path.display());
    }
}

/// Unix milliseconds now, for a creation stamp.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The roster resource, loaded at startup.
pub struct PlayersPlugin;

impl Plugin for PlayersPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_roster());
    }
}

/// Give a player every run in the log that nobody is credited with.
///
/// Runs a single rewrite of `history.jsonl`: each line whose slot one
/// has no player and which is not an autopilot run gets one. Returns
/// how many lines were claimed.
///
/// **Why this exists.** The play log predates the roster by months —
/// on the machine this was written for, 58 human runs across 22
/// songs. Without the adoption the first player's statistics would
/// open empty beside a log full of their own play, and every chart
/// would need a year to become interesting. It fires once, for the
/// first player created ([`Roster::adopts_orphans`]).
///
/// **Why it is safe.** A timestamped copy of the log is written
/// first and never removed; the rewrite only ever ADDS a `player`
/// field to a line that had none, and a line that fails to parse is
/// copied through byte for byte rather than dropped.
pub fn adopt_orphan_runs(player: PlayerId) -> usize {
    let Some(path) = crate::history::history_path() else {
        return 0;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return 0;
    };
    let backup = path.with_extension(format!("pre-roster.{}.jsonl", now_ms()));
    if let Err(error) = std::fs::write(&backup, &text) {
        warn!("players: cannot back the history up, not adopting: {error}");
        return 0;
    }
    let (rewritten, claimed) = claim_unattributed(&text, player);
    if let Err(error) = std::fs::write(&path, rewritten) {
        warn!("players: cannot rewrite the history: {error}");
        return 0;
    }
    info!(
        "players: {claimed} unattributed runs are now {player}'s (copy kept at {})",
        backup.display()
    );
    claimed
}
