//! Player input → gameplay sessions, routed per device.
//!
//! Each player entity carries its [`PlayerDevice`]; the keyboard
//! player only hears the keyboard, a pad player only its own pad.
//! Physical inputs resolve to game actions via [`InputMap`]; the
//! session only ever sees actions, timestamped with the song time
//! minus the calibration offset (ADR-0004).

use beatbyte_core::{GameInput, InputKind};
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use super::{PlayerDevice, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::controls::{GameAction, InputMap, InputSources};
use crate::multiplayer::DeviceId;

/// Feed this frame's inputs into each player's session.
pub fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    map: Res<InputMap>,
    mut players: Query<(&PlayerDevice, &mut PlayerSession)>,
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
    // An empty keyboard stand-in so pad players never hear key events.
    let silent_keys = ButtonInput::<KeyCode>::default();

    for (device, mut player) in &mut players {
        let sources = match device.0 {
            DeviceId::Keyboard => InputSources {
                keys: &keys,
                pads: Vec::new(),
            },
            DeviceId::Pad(pad_entity) => InputSources {
                keys: &silent_keys,
                pads: pads
                    .get(pad_entity)
                    .map(|(_, pad)| vec![pad])
                    .unwrap_or_default(),
            },
        };
        let player = &mut *player;
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
}
