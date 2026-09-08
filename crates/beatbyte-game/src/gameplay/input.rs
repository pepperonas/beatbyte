//! Player input → gameplay sessions, routed per device.
//!
//! Each player entity carries its [`PlayerDevice`]; the keyboard
//! player only hears the keyboard, a pad player only its own pad.
//! Physical inputs resolve to game actions via [`InputMap`]; the
//! session only ever sees actions, timestamped with the song time
//! minus the calibration offset (ADR-0004).
//!
//! # The mouse
//!
//! The **primary mouse button is a strum**, for the keyboard player
//! only. It is deliberately not in [`InputMap`]: that map is keyed by
//! `KeyCode` and `GamepadButton`, and a pad player has a strum bar
//! under their hand already — a mouse on the table is the keyboard
//! player's. Stamped like every other input, so it judges the same.

use beatbyte_core::{GameInput, InputKind};
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use super::{PlayerDevice, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::controls::{GameAction, InputMap, InputSources};
use crate::multiplayer::DeviceId;

/// Whether this frame's primary click is a strum for this device.
///
/// The decision, lifted out so it can be pinned: two branches that
/// each work and a routing rule that picks the wrong one is exactly
/// what a per-branch test misses. A pad player has a strum bar under
/// their hand; the mouse on the table is the keyboard player's, and
/// in a two-player game it must not fire for both.
#[must_use]
pub fn mouse_strums(device: DeviceId, clicked: bool) -> bool {
    clicked && device == DeviceId::Keyboard
}

/// Feed this frame's inputs into each player's session.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
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

    // Solo: the one player owns EVERY device — keyboard and any
    // guitar/pad alike. Strict per-device routing only matters once
    // a second player exists. (Field find: a guitar played into the
    // void because solo always routed as Keyboard.)
    let solo = players.iter().count() == 1;
    for (device, mut player) in &mut players {
        let sources = match device.0 {
            DeviceId::Keyboard => InputSources {
                keys: &keys,
                pads: if solo {
                    pads.iter().map(|(_, pad)| pad).collect()
                } else {
                    Vec::new()
                },
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
        // The mouse belongs to whoever the keyboard belongs to.
        let clicked = mouse_strums(device.0, mouse.just_pressed(MouseButton::Left));
        if clicked
            || sources.just_pressed(&map, GameAction::StrumUp)
            || sources.just_pressed(&map, GameAction::StrumDown)
        {
            send(InputKind::Strum);
        }
        if sources.just_pressed(&map, GameAction::Hype) {
            send(InputKind::ActivateHype);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_primary_click_strums_for_the_keyboard_player_only() {
        assert!(
            mouse_strums(DeviceId::Keyboard, true),
            "a click is a strum for whoever plays on the keyboard"
        );
        assert!(
            !mouse_strums(
                DeviceId::Pad(Entity::from_raw_u32(7).expect("a valid entity index")),
                true
            ),
            "a pad player has a strum bar; the mouse is not theirs, \
             and in a two-player game one click must not strum twice"
        );
        assert!(
            !mouse_strums(DeviceId::Keyboard, false),
            "and no click is no strum"
        );
    }
}
