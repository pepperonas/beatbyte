//! Gamepad hot-plug during a song (roadmap C4).
//!
//! A pad that vanishes mid-song used to vanish silently: its player's
//! input query found no entity, every note from then on was a miss,
//! and plugging the pad back in helped nothing — the player still
//! pointed at the despawned entity. Now:
//!
//! - **A disconnect PAUSES the song** (from Playing) and marks the
//!   players who were bound to that pad as [`PadLost`]. The pause
//!   screen says whose controller is gone.
//! - **A reconnect hands the new pad to the longest-waiting lost
//!   player** — the same player slot, session and score carry on;
//!   only the entity behind [`PlayerDevice`] changes. Resuming stays
//!   the player's call (ESC / START), so nobody is thrown back into a
//!   song they did not see restart.
//!
//! The decision is a pure function of the events and the players
//! ([`react`]); the system only applies it, so the whole policy is
//! tested without a controller.

use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent};
use bevy::prelude::*;

use super::{PlayerDevice, PlayerIndex};
use crate::multiplayer::DeviceId;
use crate::states::GamePhase;

/// On a player whose pad went away, since when (monotonic seconds).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PadLost {
    /// When the pad vanished.
    pub since: f64,
}

/// One player as the policy sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seat {
    /// The player entity.
    pub entity: Entity,
    /// Its device.
    pub device: DeviceId,
    /// Waiting for a pad since, if its pad is gone.
    pub lost_since: Option<f64>,
}

/// What a connection change asks the game to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Pause the song (the phase was Playing).
    Pause,
    /// This player's pad is gone.
    MarkLost(Entity),
    /// This player takes this pad.
    Rebind {
        /// The player.
        player: Entity,
        /// The pad's entity.
        pad: Entity,
    },
}

/// The policy. `playing` is whether the song is running (only then
/// is there something to pause). Pure — tested.
#[must_use]
pub fn react(event: &GamepadConnectionEvent, seats: &[Seat], playing: bool) -> Vec<Action> {
    let mut actions = Vec::new();
    match &event.connection {
        GamepadConnection::Disconnected => {
            // Any pad going away pauses the song: a solo player
            // holds every device, and the one that vanished may
            // well have been the guitar in their hands.
            if playing {
                actions.push(Action::Pause);
            }
            for seat in seats {
                if seat.device == DeviceId::Pad(event.gamepad) && seat.lost_since.is_none() {
                    actions.push(Action::MarkLost(seat.entity));
                }
            }
        }
        GamepadConnection::Connected { .. } => {
            // The pad goes to whoever has waited longest; a pad
            // already driving someone is not up for grabs.
            let taken = seats
                .iter()
                .any(|s| s.device == DeviceId::Pad(event.gamepad));
            if taken {
                return actions;
            }
            let waiting = seats
                .iter()
                .filter(|s| s.lost_since.is_some())
                .min_by(|a, b| {
                    a.lost_since
                        .partial_cmp(&b.lost_since)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(seat) = waiting {
                actions.push(Action::Rebind {
                    player: seat.entity,
                    pad: event.gamepad,
                });
            }
        }
    }
    actions
}

/// The pause screen's line about lost pads: which players are
/// waiting, or nothing when nobody is. Pure — tested.
#[must_use]
pub fn note_text(lost_players: &[usize]) -> Option<String> {
    match lost_players {
        [] => None,
        [one] => Some(format!(
            "P{}'S CONTROLLER DISCONNECTED - PLUG IT BACK IN",
            one + 1
        )),
        many => {
            let list: Vec<String> = many.iter().map(|p| format!("P{}", p + 1)).collect();
            Some(format!(
                "{} CONTROLLERS DISCONNECTED - PLUG THEM BACK IN",
                list.join(" AND ")
            ))
        }
    }
}

/// Apply the policy to the world for every connection change.
#[allow(clippy::type_complexity)] // Bevy query
pub fn watch_pads(
    mut commands: Commands,
    mut events: MessageReader<GamepadConnectionEvent>,
    phase: Res<State<GamePhase>>,
    mut next_phase: ResMut<NextState<GamePhase>>,
    time: Res<Time>,
    mut players: Query<(Entity, &PlayerIndex, &mut PlayerDevice, Option<&PadLost>)>,
) {
    let mut playing = *phase.get() == GamePhase::Playing;
    for event in events.read() {
        let seats: Vec<Seat> = players
            .iter()
            .map(|(entity, _, device, lost)| Seat {
                entity,
                device: device.0,
                lost_since: lost.map(|l| l.since),
            })
            .collect();
        for action in react(event, &seats, playing) {
            match action {
                Action::Pause => {
                    info!("hotplug: a controller disconnected - paused");
                    next_phase.set(GamePhase::Paused);
                    playing = false;
                }
                Action::MarkLost(player) => {
                    if let Ok((_, index, _, _)) = players.get(player) {
                        info!("hotplug: P{}'s controller is gone", index.0 + 1);
                    }
                    commands.entity(player).insert(PadLost {
                        since: time.elapsed_secs_f64(),
                    });
                }
                Action::Rebind { player, pad } => {
                    if let Ok((_, index, mut device, _)) = players.get_mut(player) {
                        info!(
                            "hotplug: P{} continues on the reconnected controller",
                            index.0 + 1
                        );
                        device.0 = DeviceId::Pad(pad);
                    }
                    commands.entity(player).remove::<PadLost>();
                }
            }
        }
    }
}

/// The pause screen's hot-plug line.
#[derive(Component)]
pub struct PadNote;

/// Keep the pause screen's line current: named while someone waits,
/// empty otherwise (a reconnect clears it live, without leaving the
/// menu).
pub fn refresh_pad_note(
    players: Query<&PlayerIndex, With<PadLost>>,
    mut notes: Query<&mut Text, With<PadNote>>,
) {
    let Ok(mut text) = notes.single_mut() else {
        return;
    };
    let mut lost: Vec<usize> = players.iter().map(|p| p.0).collect();
    lost.sort_unstable();
    let wanted = note_text(&lost).unwrap_or_default();
    if text.0 != wanted {
        text.0 = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disconnect(pad: Entity) -> GamepadConnectionEvent {
        GamepadConnectionEvent {
            gamepad: pad,
            connection: GamepadConnection::Disconnected,
        }
    }

    fn connect(pad: Entity) -> GamepadConnectionEvent {
        GamepadConnectionEvent {
            gamepad: pad,
            connection: GamepadConnection::Connected {
                name: "pad".to_owned(),
                vendor_id: None,
                product_id: None,
            },
        }
    }

    fn seat(entity: u32, device: DeviceId, lost_since: Option<f64>) -> Seat {
        Seat {
            entity: Entity::from_raw_u32(entity).expect("non-zero"),
            device,
            lost_since,
        }
    }

    fn pad(n: u32) -> Entity {
        Entity::from_raw_u32(n).expect("non-zero")
    }

    #[test]
    fn a_disconnect_pauses_and_marks_the_players_on_that_pad() {
        let seats = [
            seat(1, DeviceId::Keyboard, None),
            seat(2, DeviceId::Pad(pad(10)), None),
            seat(3, DeviceId::Pad(pad(11)), None),
        ];
        let actions = react(&disconnect(pad(10)), &seats, true);
        assert_eq!(
            actions,
            vec![Action::Pause, Action::MarkLost(seats[1].entity)],
            "only P2 loses its pad; the song pauses"
        );
        // Already paused: no second pause, still marked.
        let actions = react(&disconnect(pad(11)), &seats, false);
        assert_eq!(actions, vec![Action::MarkLost(seats[2].entity)]);
    }

    #[test]
    fn a_solo_keyboard_player_is_paused_but_not_marked() {
        // Solo owns every device; the vanished pad may have been the
        // guitar in hand. Pause, but nobody is "lost": the keyboard
        // still plays.
        let seats = [seat(1, DeviceId::Keyboard, None)];
        assert_eq!(
            react(&disconnect(pad(10)), &seats, true),
            vec![Action::Pause]
        );
    }

    #[test]
    fn a_reconnect_goes_to_the_longest_waiting_player() {
        let seats = [
            seat(1, DeviceId::Pad(pad(10)), Some(5.0)),
            seat(2, DeviceId::Pad(pad(11)), Some(2.0)),
            seat(3, DeviceId::Pad(pad(12)), None),
        ];
        let actions = react(&connect(pad(20)), &seats, false);
        assert_eq!(
            actions,
            vec![Action::Rebind {
                player: seats[1].entity,
                pad: pad(20)
            }],
            "P2 has waited since 2.0, P1 only since 5.0"
        );
        // Nobody waiting: a new pad changes nothing.
        let calm = [seat(3, DeviceId::Pad(pad(12)), None)];
        assert!(react(&connect(pad(21)), &calm, true).is_empty());
        // A pad that already drives a player is not handed out again.
        assert!(react(&connect(pad(12)), &seats, false).is_empty());
    }

    #[test]
    fn a_marked_player_is_not_marked_twice() {
        let seats = [seat(1, DeviceId::Pad(pad(10)), Some(1.0))];
        assert!(react(&disconnect(pad(10)), &seats, false).is_empty());
    }

    #[test]
    fn the_pause_line_names_who_is_waiting() {
        assert_eq!(note_text(&[]), None);
        assert_eq!(
            note_text(&[1]).as_deref(),
            Some("P2'S CONTROLLER DISCONNECTED - PLUG IT BACK IN")
        );
        assert_eq!(
            note_text(&[0, 2]).as_deref(),
            Some("P1 AND P3 CONTROLLERS DISCONNECTED - PLUG THEM BACK IN")
        );
    }

    /// The real systems in a real (headless) app: states, messages,
    /// the pause line — everything but a controller.
    mod wired {
        use super::super::*;
        use super::{connect, disconnect};
        use crate::gameplay::PlayerSession;
        use crate::states::AppState;
        use beatbyte_core::{
            Difficulty, ScoreConfig, TempoMap, TimingWindows, Track, TrackSession,
        };
        use bevy::state::app::StatesPlugin;

        fn app() -> App {
            let mut app = App::new();
            app.add_plugins(StatesPlugin)
                .init_state::<AppState>()
                .add_sub_state::<GamePhase>()
                .add_message::<GamepadConnectionEvent>()
                .init_resource::<Time>()
                .add_systems(Update, (watch_pads, refresh_pad_note).chain());
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Gameplay);
            app.update();
            assert_eq!(
                *app.world().resource::<State<GamePhase>>().get(),
                GamePhase::Playing
            );
            app
        }

        fn player(app: &mut App, index: usize, device: DeviceId) -> Entity {
            let track = Track::new(
                Difficulty::Medium,
                TempoMap::constant(120.0, 0.0),
                vec![],
                vec![],
            )
            .expect("an empty track is a track");
            let session =
                TrackSession::new(track, TimingWindows::default(), ScoreConfig::default());
            app.world_mut()
                .spawn((
                    PlayerIndex(index),
                    PlayerDevice(device),
                    PlayerSession {
                        session,
                        frame_events: Vec::new(),
                        spawn_cursor: 0,
                    },
                ))
                .id()
        }

        fn send(app: &mut App, event: GamepadConnectionEvent) {
            app.world_mut().write_message(event);
            // One frame reads the message; the state transition it
            // asks for is applied at the start of the next.
            app.update();
            app.update();
        }

        fn phase(app: &App) -> GamePhase {
            *app.world().resource::<State<GamePhase>>().get()
        }

        #[test]
        fn unplug_pauses_marks_and_names_the_player_and_replug_hands_the_pad_back() {
            let mut app = app();
            let pad = app.world_mut().spawn_empty().id();
            let p1 = player(&mut app, 0, DeviceId::Keyboard);
            let p2 = player(&mut app, 1, DeviceId::Pad(pad));
            let note = app.world_mut().spawn((PadNote, Text::new(""))).id();

            send(&mut app, disconnect(pad));
            assert_eq!(phase(&app), GamePhase::Paused, "the song pauses");
            assert!(app.world().get::<PadLost>(p2).is_some(), "P2 is waiting");
            assert!(app.world().get::<PadLost>(p1).is_none(), "P1 is not");
            app.update();
            assert_eq!(
                app.world().get::<Text>(note).map(|t| t.0.as_str()),
                Some("P2'S CONTROLLER DISCONNECTED - PLUG IT BACK IN")
            );

            let fresh = app.world_mut().spawn_empty().id();
            send(&mut app, connect(fresh));
            assert_eq!(
                app.world().get::<PlayerDevice>(p2).map(|d| d.0),
                Some(DeviceId::Pad(fresh)),
                "the same player continues on the new pad"
            );
            assert!(app.world().get::<PadLost>(p2).is_none());
            app.update();
            assert_eq!(
                app.world().get::<Text>(note).map(|t| t.0.as_str()),
                Some(""),
                "the line clears without leaving the menu"
            );
            assert_eq!(
                phase(&app),
                GamePhase::Paused,
                "resuming stays the player's call"
            );
            assert_eq!(
                app.world().get::<PlayerDevice>(p1).map(|d| d.0),
                Some(DeviceId::Keyboard),
                "P1 untouched"
            );
        }
    }
}
