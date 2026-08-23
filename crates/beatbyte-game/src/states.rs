//! Top-level application states.
//!
//! BeatByte uses explicit state management — no scattered booleans.
//! `AppState` is the screen flow; `GamePhase` is a sub-state that only
//! exists while in gameplay.

use bevy::prelude::*;

/// The top-level state machine of the application.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Initial boot screen while the demo song renders/analyzes.
    #[default]
    Boot,
    /// The main menu: play, settings, calibration.
    MainMenu,
    /// The multiplayer join screen.
    MultiplayerSetup,
    /// The song browser.
    SongSelect,
    /// The settings screen.
    Settings,
    /// The controls remapping screen.
    Controls,
    /// The latency calibration screen.
    Calibration,
    /// The chart editor.
    Editor,
    /// Playing a song.
    Gameplay,
    /// Post-song results.
    Results,
}

/// Gameplay sub-state: exists only while [`AppState::Gameplay`] is
/// active.
#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(AppState = AppState::Gameplay)]
pub enum GamePhase {
    /// The song is running.
    #[default]
    Playing,
    /// Paused by the player.
    Paused,
}
