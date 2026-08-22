//! Top-level application states.
//!
//! BeatByte uses explicit state management — no scattered booleans. The
//! set below will grow toward the full flow (song select, gameplay,
//! results, …) as milestones land; states are added when the screens
//! they gate actually exist.

use bevy::prelude::*;

/// The top-level state machine of the application.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Initial boot screen shown while core assets load.
    #[default]
    Boot,
    /// The main menu (placeholder until Milestone 7).
    MainMenu,
}
