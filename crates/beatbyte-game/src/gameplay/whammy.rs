//! Whammy is presentation only; scoring still belongs to TrackSession.
use super::{PlayerDevice, PlayerSession};
use crate::{
    audio_sys::Music,
    multiplayer::DeviceId,
    states::{AppState, GamePhase},
    xplorer::GuitarWhammy,
};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(super) struct WhammyLevel {
    pub depth: f32,
    pub players: std::collections::HashMap<usize, f32>,
    pub input: f32,
}

/// A pinned, bounded wave shared by the flat and solid sustain renderers.
pub(super) fn wave_offset(position: f32, seconds: f32, depth: f32, lane_step: f32) -> f32 {
    let p = position.clamp(0.0, 1.0);
    lane_step
        * 0.38
        * depth
        * p.powf(0.85)
        * (p * std::f32::consts::TAU * 2.2 - seconds * std::f32::consts::TAU * 6.5).sin()
}

#[allow(clippy::too_many_arguments)] // Bevy system dependencies
pub(super) fn update_whammy(
    state: Res<State<AppState>>,
    phase: Option<Res<State<GamePhase>>>,
    music: Option<Res<Music>>,
    players: Query<(&super::PlayerIndex, &PlayerDevice, &PlayerSession)>,
    pads: Query<(Entity, &Gamepad)>,
    native: Res<GuitarWhammy>,
    injector: Option<Res<crate::autopilot::InjectorOwnsInput>>,
    mut previous: ResMut<WhammyLevel>,
    mut generation: Local<u64>,
) {
    let playing = *state.get() == AppState::Gameplay
        && phase
            .as_deref()
            .is_some_and(|p| *p.get() == GamePhase::Playing)
        && injector.is_none();
    if *state.get() == AppState::InputTest {
        return;
    }
    previous.players.clear();
    previous.input = pads
        .iter()
        .map(|(entity, pad)| crate::xplorer::whammy_depth(entity, pad, &native))
        .chain(
            native
                .0
                .values()
                .map(|v| crate::xplorer::normalize_whammy(*v)),
        )
        .fold(0.0_f32, f32::max);
    let depth = if playing {
        let solo = players.iter().count() == 1;
        for (index, device, session) in &players {
            if session.session.active_sustain().is_none() {
                continue;
            }
            let depth = match device.0 {
                DeviceId::Pad(entity) => pads.get(entity).map_or(0.0, |(_, pad)| {
                    crate::xplorer::whammy_depth(entity, pad, &native)
                }),
                DeviceId::Keyboard if solo => previous.input,
                DeviceId::Keyboard => 0.0,
            };
            previous.players.insert(index.0, depth);
        }
        previous.players.values().copied().fold(0.0_f32, f32::max)
    } else {
        0.0
    };
    let current_generation = music.as_ref().map_or(0, |music| music.0.generation());
    if depth != previous.depth || current_generation != *generation {
        if let Some(music) = music {
            music.0.set_whammy(depth);
        }
        previous.depth = depth;
        *generation = current_generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::{
        Difficulty, GameInput, InputKind, Lane, LaneSet, NoteEvent, NoteKind, ScoreConfig,
        TempoMap, TimingWindows, Track, TrackSession,
    };
    use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent};

    #[test]
    fn only_the_playing_players_held_sustain_can_drive_whammy_and_pause_clears_it() {
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(State::new(AppState::Gameplay))
            .insert_resource(State::new(GamePhase::Playing))
            .init_resource::<GuitarWhammy>()
            .init_resource::<WhammyLevel>()
            .add_systems(Update, update_whammy);
        let guitar = app.world_mut().spawn_empty().id();
        let other = app.world_mut().spawn_empty().id();
        for entity in [guitar, other] {
            app.world_mut().write_message(GamepadConnectionEvent {
                gamepad: entity,
                connection: GamepadConnection::Connected {
                    name: "Guitar Hero X-plorer".into(),
                    vendor_id: Some(0x1430),
                    product_id: Some(0x4748),
                },
            });
        }
        let track = Track::new(
            Difficulty::Medium,
            TempoMap::constant(120.0, 0.0),
            vec![NoteEvent {
                time_s: 1.0,
                lanes: LaneSet::single(Lane::One),
                sustain_s: 2.0,
                kind: NoteKind::Strum,
            }],
            vec![],
        )
        .expect("valid test fixture");
        let player = app
            .world_mut()
            .spawn((
                super::super::PlayerIndex(0),
                PlayerDevice(DeviceId::Pad(guitar)),
                PlayerSession {
                    session: TrackSession::new(
                        track,
                        TimingWindows::default(),
                        ScoreConfig::default(),
                    ),
                    frame_events: vec![],
                    spawn_cursor: 0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<GuitarWhammy>()
            .0
            .insert(other, 1.0);
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 0.0);
        app.world_mut()
            .resource_mut::<GuitarWhammy>()
            .0
            .insert(guitar, 1.0);
        app.update();
        assert_eq!(
            app.world().resource::<WhammyLevel>().depth,
            0.0,
            "no hit sustain"
        );
        {
            let mut session = app
                .world_mut()
                .get_mut::<PlayerSession>(player)
                .expect("valid test fixture");
            let mut events = vec![];
            session.session.handle(
                GameInput {
                    time_s: 1.0,
                    kind: InputKind::FretDown(Lane::One),
                },
                &mut events,
            );
            session.session.handle(
                GameInput {
                    time_s: 1.0,
                    kind: InputKind::Strum,
                },
                &mut events,
            );
        }
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 1.0);
        app.insert_resource(State::new(GamePhase::Paused));
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 0.0);
        app.insert_resource(State::new(GamePhase::Playing));
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 1.0);
        app.world_mut()
            .get_mut::<PlayerSession>(player)
            .expect("valid test fixture")
            .session
            .handle(
                GameInput {
                    time_s: 1.5,
                    kind: InputKind::FretUp(Lane::One),
                },
                &mut vec![],
            );
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 0.0);
    }

    #[test]
    fn solo_player_on_default_keyboard_device_can_drive_whammy_from_guitar() {
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(State::new(AppState::Gameplay))
            .insert_resource(State::new(GamePhase::Playing))
            .init_resource::<GuitarWhammy>()
            .init_resource::<WhammyLevel>()
            .add_systems(Update, update_whammy);
        let guitar = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent {
            gamepad: guitar,
            connection: GamepadConnection::Connected {
                name: "Guitar Hero X-plorer".into(),
                vendor_id: Some(0x1430),
                product_id: Some(0x4748),
            },
        });
        let track = Track::new(
            Difficulty::Medium,
            TempoMap::constant(120.0, 0.0),
            vec![NoteEvent {
                time_s: 1.0,
                lanes: LaneSet::single(Lane::One),
                sustain_s: 2.0,
                kind: NoteKind::Strum,
            }],
            vec![],
        )
        .expect("valid test fixture");
        // Default single-player device is Keyboard:
        let player = app
            .world_mut()
            .spawn((
                super::super::PlayerIndex(0),
                PlayerDevice(DeviceId::Keyboard),
                PlayerSession {
                    session: TrackSession::new(
                        track,
                        TimingWindows::default(),
                        ScoreConfig::default(),
                    ),
                    frame_events: vec![],
                    spawn_cursor: 0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<GuitarWhammy>()
            .0
            .insert(guitar, 1.0);
        // Before sustain is hit, depth is 0
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 0.0);

        // Hit and hold the sustain note
        {
            let mut session = app
                .world_mut()
                .get_mut::<PlayerSession>(player)
                .expect("valid test fixture");
            let mut events = vec![];
            session.session.handle(
                GameInput {
                    time_s: 1.0,
                    kind: InputKind::FretDown(Lane::One),
                },
                &mut events,
            );
            session.session.handle(
                GameInput {
                    time_s: 1.0,
                    kind: InputKind::Strum,
                },
                &mut events,
            );
        }
        app.update();
        assert_eq!(
            app.world().resource::<WhammyLevel>().depth,
            1.0,
            "solo player on Keyboard device must receive whammy from connected guitar"
        );
        let second_session = app
            .world()
            .get::<PlayerSession>(player)
            .expect("valid test fixture")
            .session
            .clone();
        let second = app
            .world_mut()
            .spawn((
                super::super::PlayerIndex(1),
                PlayerDevice(DeviceId::Keyboard),
                PlayerSession {
                    session: second_session,
                    frame_events: vec![],
                    spawn_cursor: 0,
                },
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<WhammyLevel>().depth,
            0.0,
            "unassigned guitar must not leak into multiplayer keyboard players"
        );
        app.world_mut().despawn(second);
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 1.0);
        app.insert_resource(crate::autopilot::InjectorOwnsInput);
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().depth, 0.0);
        app.world_mut()
            .remove_resource::<crate::autopilot::InjectorOwnsInput>();
        app.world_mut().resource_mut::<GuitarWhammy>().0.clear();
        app.world_mut().write_message(GamepadConnectionEvent {
            gamepad: guitar,
            connection: GamepadConnection::Disconnected,
        });
        app.update();
        assert_eq!(
            app.world().resource::<WhammyLevel>().depth,
            0.0,
            "disconnect resets solo effect"
        );
    }
}

#[cfg(test)]
mod wave_tests {
    use super::wave_offset;
    #[test]
    fn wave_is_pinned_bounded_and_pressure_driven() {
        for tick in 0..100 {
            let seconds = tick as f32 / 100.0;
            assert_eq!(wave_offset(0.0, seconds, 1.0, 76.0), 0.0);
            for i in 0..100 {
                let u = i as f32 / 100.0;
                let full = wave_offset(u, seconds, 1.0, 76.0);
                assert!(full.abs() <= 76.0 * 0.38 + 1e-4);
                assert_eq!(wave_offset(u, seconds, 0.0, 76.0), 0.0);
                assert!((wave_offset(u, seconds, 0.5, 76.0) - full * 0.5).abs() < 0.00001);
            }
        }
        assert_ne!(
            wave_offset(0.3, 0.0, 1.0, 76.0),
            wave_offset(0.3, 0.04, 1.0, 76.0)
        );
    }
}
