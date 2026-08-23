//! Player input → gameplay session, through the binding table.
//!
//! Physical inputs (keyboard keys, buttons on any connected gamepad —
//! guitar-style controllers included) resolve to game actions via
//! [`InputMap`]; the session only ever sees actions. Inputs are
//! timestamped with the current song time minus the calibration
//! offset (ADR-0004).

use beatbyte_core::{GameInput, InputKind};
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use super::PlayerSession;
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::controls::{GameAction, InputMap, InputSources};

/// Feed this frame's inputs into the (single-player) session.
/// Per-player device routing arrives with local multiplayer.
pub fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    map: Res<InputMap>,
    mut players: Query<&mut PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    let Some(raw_now) = game_clock.song_time(&time) else {
        return;
    };
    // Calibration: a positive offset means the player's inputs arrive
    // late; subtracting re-aligns them with the song timeline.
    let now = raw_now - settings.latency_offset_s();
    let Ok(mut player) = players.single_mut() else {
        return;
    };
    let player = &mut *player;
    let sources = InputSources {
        keys: &keys,
        pads: pads.iter().collect(),
    };
    let mut send = |kind: InputKind| {
        player
            .session
            .handle(GameInput { time_s: now, kind }, &mut player.frame_events);
    };

    for index in 0..5u8 {
        let action = GameAction::Fret(index);
        let Some(lane) = action.lane() else { continue };
        if sources.just_pressed(&map, action) {
            send(InputKind::FretDown(lane));
        }
        if sources.just_released(&map, action) {
            send(InputKind::FretUp(lane));
        }
    }
    if sources.just_pressed(&map, GameAction::StrumUp)
        || sources.just_pressed(&map, GameAction::StrumDown)
    {
        send(InputKind::Strum);
    }
    if sources.just_pressed(&map, GameAction::Hype) {
        send(InputKind::ActivateHype);
    }
}
