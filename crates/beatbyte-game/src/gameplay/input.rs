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
use std::collections::HashMap;

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

/// Edge detector for either valid four-adjacent-fret activation chord.
#[derive(Debug, Default)]
pub(super) struct HypeChordLatch {
    active: bool,
}

impl HypeChordLatch {
    fn update(&mut self, frets: [bool; 5]) -> bool {
        let active = frets[..4].iter().all(|held| *held) || frets[1..].iter().all(|held| *held);
        let triggered = active && !self.active;
        self.active = active;
        triggered
    }
}

/// Feed this frame's inputs into each player's session.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub(super) fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<(Entity, &Gamepad)>,
    map: Res<InputMap>,
    mut players: Query<(Entity, &PlayerDevice, &mut PlayerSession)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
    injector: Option<Res<crate::autopilot::InjectorOwnsInput>>,
    mut chord_latches: Local<HashMap<Entity, HypeChordLatch>>,
) {
    // While the autopilot's note injector owns the session it plays
    // every note itself: a key, a pad button or a click from the desk
    // belongs to the room, not to the run, and could only add phantom
    // strums (see `autopilot::injector_owns_input`).
    if injector.is_some() {
        return;
    }
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
    for (player_entity, device, mut player) in &mut players {
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
        let frets =
            std::array::from_fn(|index| sources.pressed(&map, GameAction::Fret(index as u8)));
        let chord_triggered = chord_latches
            .entry(player_entity)
            .or_default()
            .update(frets);
        if sources.just_pressed(&map, GameAction::Hype) || chord_triggered {
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

    #[test]
    fn only_four_adjacent_frets_trigger_hype_on_the_rising_edge() {
        let mut latch = HypeChordLatch::default();
        assert!(!latch.update([true, true, true, false, false]));
        assert!(!latch.update([true, true, false, true, true]));
        assert!(latch.update([true, true, true, true, false]));
        assert!(!latch.update([true, true, true, true, false]), "held chord");
        assert!(!latch.update([false, false, false, false, false]));
        assert!(latch.update([false, true, true, true, true]));
    }

    #[test]
    fn all_five_frets_are_one_activation_until_the_chord_is_left() {
        let mut latch = HypeChordLatch::default();
        assert!(latch.update([true; 5]));
        assert!(
            !latch.update([true; 5]),
            "five frets must not double-trigger"
        );
        assert!(!latch.update([false, true, true, true, true]));
        assert!(!latch.update([false, true, true, false, true]));
        assert!(latch.update([true, true, true, true, false]));
    }

    /// The real system in a real (headless) app: one player on the
    /// keyboard, the clock running, an empty track — so every strum
    /// lands on nothing and an overstrum is the proof that the input
    /// reached the session at all.
    mod wired {
        use super::super::*;
        use crate::autopilot::InjectorOwnsInput;
        use beatbyte_core::{
            Difficulty, ScoreConfig, TempoMap, TimingWindows, Track, TrackSession,
        };

        fn overstrums_after_a_strum_key(muted: bool) -> u32 {
            let mut app = App::new();
            app.init_resource::<Time>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<InputMap>()
                .init_resource::<Settings>()
                .init_resource::<GameClock>()
                .add_systems(Update, gameplay_input);
            if muted {
                app.insert_resource(InjectorOwnsInput);
            }
            app.world_mut().resource_mut::<GameClock>().begin(0.0, 0.0);
            let track = Track::new(
                Difficulty::Medium,
                TempoMap::constant(120.0, 0.0),
                vec![],
                vec![],
            )
            .expect("an empty track is a track");
            app.world_mut().spawn((
                PlayerDevice(DeviceId::Keyboard),
                PlayerSession {
                    session: TrackSession::new(
                        track,
                        TimingWindows::default(),
                        ScoreConfig::default(),
                    ),
                    frame_events: Vec::new(),
                    spawn_cursor: 0,
                },
            ));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ArrowDown);
            app.update();
            let mut players = app.world_mut().query::<&PlayerSession>();
            players
                .iter(app.world())
                .next()
                .expect("the one player")
                .session
                .performance()
                .overstrums()
        }

        #[test]
        fn a_strum_key_from_the_desk_reaches_the_session() {
            assert_eq!(
                overstrums_after_a_strum_key(false),
                1,
                "a player at the keyboard must be heard"
            );
        }

        #[test]
        fn the_desk_is_silent_while_the_injector_owns_the_inputs() {
            assert_eq!(
                overstrums_after_a_strum_key(true),
                0,
                "the autopilot plays every note itself; a key pressed in the \
                 room is not part of the run and must not fail it"
            );
        }
    }
}
