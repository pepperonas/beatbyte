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
pub(super) struct WhammyLevel(pub f32);

#[allow(clippy::too_many_arguments)] // Bevy system dependencies
pub(super) fn update_whammy(
    state: Res<State<AppState>>,
    phase: Option<Res<State<GamePhase>>>,
    music: Option<Res<Music>>,
    players: Query<(&PlayerDevice, &PlayerSession)>,
    pads: Query<&Gamepad>,
    native: Res<GuitarWhammy>,
    injector: Option<Res<crate::autopilot::InjectorOwnsInput>>,
    mut previous: ResMut<WhammyLevel>,
) {
    let playing = *state.get() == AppState::Gameplay
        && phase
            .as_deref()
            .is_some_and(|p| *p.get() == GamePhase::Playing)
        && injector.is_none();
    let depth = if playing {
        players
            .iter()
            .filter_map(|(device, session)| {
                session.session.active_sustain()?;
                let DeviceId::Pad(entity) = device.0 else {
                    return None;
                };
                let pad = pads.get(entity).ok()?;
                Some(crate::xplorer::whammy_depth(entity, pad, &native))
            })
            .fold(0.0_f32, f32::max)
    } else {
        0.0
    };
    if depth != previous.0 {
        if let Some(music) = music {
            music.0.set_whammy(depth);
        }
        previous.0 = depth;
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
        assert_eq!(app.world().resource::<WhammyLevel>().0, 0.0);
        app.world_mut()
            .resource_mut::<GuitarWhammy>()
            .0
            .insert(guitar, 1.0);
        app.update();
        assert_eq!(
            app.world().resource::<WhammyLevel>().0,
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
        assert_eq!(app.world().resource::<WhammyLevel>().0, 1.0);
        app.insert_resource(State::new(GamePhase::Paused));
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().0, 0.0);
        app.insert_resource(State::new(GamePhase::Playing));
        app.update();
        assert_eq!(app.world().resource::<WhammyLevel>().0, 1.0);
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
        assert_eq!(app.world().resource::<WhammyLevel>().0, 0.0);
    }
}
