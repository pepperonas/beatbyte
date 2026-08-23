//! Keyboard → gameplay session input.
//!
//! Milestone 5 keeps a fixed keyboard mapping; the remappable input
//! abstraction (device → action → gameplay) is Milestone 8. Inputs are
//! timestamped with the current song time — one frame of latency at
//! most, which the calibration milestone will let players compensate.

use beatbyte_core::{GameInput, InputKind, Lane};
use bevy::prelude::*;

use super::PlayerSession;
use crate::audio_sys::GameClock;

/// The fixed Milestone-5 fret mapping.
pub const FRET_KEYS: [(KeyCode, Lane); 5] = [
    (KeyCode::KeyA, Lane::One),
    (KeyCode::KeyS, Lane::Two),
    (KeyCode::KeyD, Lane::Three),
    (KeyCode::KeyF, Lane::Four),
    (KeyCode::KeyG, Lane::Five),
];

/// Strum keys (direction is irrelevant to judgment).
pub const STRUM_KEYS: [KeyCode; 2] = [KeyCode::ArrowUp, KeyCode::ArrowDown];

/// Feed this frame's keyboard activity into the (single-player)
/// session. Multiplayer input routing arrives with Milestone 8/9.
pub fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let Ok(mut player) = players.single_mut() else {
        return;
    };
    let player = &mut *player;
    let mut send = |kind: InputKind| {
        player
            .session
            .handle(GameInput { time_s: now, kind }, &mut player.frame_events);
    };

    for (key, lane) in FRET_KEYS {
        if keys.just_pressed(key) {
            send(InputKind::FretDown(lane));
        }
        if keys.just_released(key) {
            send(InputKind::FretUp(lane));
        }
    }
    for key in STRUM_KEYS {
        if keys.just_pressed(key) {
            send(InputKind::Strum);
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        send(InputKind::ActivateHype);
    }
}
