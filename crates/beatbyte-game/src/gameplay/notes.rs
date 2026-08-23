//! Highways, receptors and note sprites — one set per player.
//!
//! Note positions are a pure function of song time — a dropped frame
//! shifts pixels for one frame, never judgment.

use beatbyte_core::session::NoteState;
use beatbyte_core::{Lane, NoteKind, SessionEvent};
use bevy::prelude::*;

use super::{
    GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, RECEPTOR_Y, SPAWN_LOOKAHEAD_S,
    SessionFeedback,
};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::palette;

/// One visible note head (chords have one per lane).
#[derive(Component)]
pub struct NoteSprite {
    /// The player this note belongs to.
    pub player: usize,
    /// Index into the track's events.
    pub event_index: usize,
    /// Whether this event was already resolved visually.
    pub resolved: bool,
}

/// A receptor (hit-line target) for one lane of one player.
#[derive(Component)]
pub struct Receptor {
    /// The player.
    pub player: usize,
    /// The lane.
    pub lane: Lane,
}

/// Build every player's highway: bed, receptor row, lane guides.
pub fn spawn_highways(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    shapes: Res<crate::shapes::LaneShapes>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
) {
    let theme = theme.0;
    let shapes = &*shapes;
    for index in players.iter() {
        let player = index.0;
        let origin = layout.origin(player);
        // Highway bed (tagged: the beat pulse modulates its brightness).
        commands.spawn((
            GameplayScreen,
            super::fx::HighwayBed,
            Sprite::from_color(theme.surface, Vec2::new(layout.bed_width(), 900.0)),
            Transform::from_xyz(origin, 0.0, -10.0),
        ));
        // Lane guide lines.
        for lane in Lane::ALL {
            commands.spawn((
                GameplayScreen,
                Sprite::from_color(
                    palette::dimmed(theme.lane_color(lane), 0.06),
                    Vec2::new(2.0, 900.0),
                ),
                Transform::from_xyz(layout.lane_x(player, lane), 0.0, -9.0),
            ));
        }
        // Receptor row: ring look = the lane's shape under a smaller
        // background copy of the same shape. Shapes, not just colors,
        // identify lanes (colorblind accessibility).
        let receptor = layout.receptor_size();
        for lane in Lane::ALL {
            let x = layout.lane_x(player, lane);
            commands.spawn((
                GameplayScreen,
                Receptor { player, lane },
                shape_sprite(
                    shapes,
                    lane,
                    palette::dimmed(theme.lane_color(lane), 0.35),
                    receptor,
                ),
                Transform::from_xyz(x, RECEPTOR_Y, -5.0),
            ));
            commands.spawn((
                GameplayScreen,
                shape_sprite(shapes, lane, theme.background, receptor * 0.72),
                Transform::from_xyz(x, RECEPTOR_Y, -4.0),
            ));
        }
    }
}

/// Spawn note entities as their time approaches.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn spawn_due_notes(
    mut commands: Commands,
    mut players: Query<(&PlayerIndex, &mut PlayerSession)>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
    shapes: Res<crate::shapes::LaneShapes>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    for (index, mut player) in &mut players {
        // The track is shared, but each player's cursor advances alone
        // (their sessions resolve notes independently).
        while player.spawn_cursor < player.session.track().events().len() {
            let cursor = player.spawn_cursor;
            let event = player.session.track().events()[cursor];
            if event.time_s > now + SPAWN_LOOKAHEAD_S {
                break;
            }
            spawn_event_sprites(
                &mut commands,
                &layout,
                &theme.0,
                &shapes,
                index.0,
                cursor,
                &event,
                settings.scroll_speed,
            );
            player.spawn_cursor += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_event_sprites(
    commands: &mut Commands,
    layout: &HighwayLayout,
    theme: &crate::theme::Theme,
    shapes: &crate::shapes::LaneShapes,
    player: usize,
    event_index: usize,
    event: &beatbyte_core::NoteEvent,
    scroll_speed: f32,
) {
    let size = layout.note_size();
    let hopo = matches!(event.kind, beatbyte_core::NoteKind::Hopo);
    for lane in event.lanes.iter() {
        let color = theme.lane_color(lane);
        // HOPOs render smaller with a bright core — until now they
        // looked identical to strums, which hid the mechanic.
        let gem = if hopo { size * 0.78 } else { size };
        let entity = commands
            .spawn((
                GameplayScreen,
                NoteSprite {
                    player,
                    event_index,
                    resolved: false,
                },
                shape_sprite(shapes, lane, color, gem),
                Transform::from_xyz(layout.lane_x(player, lane), 2000.0, 0.0),
            ))
            .id();
        if hopo {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    shape_sprite(shapes, lane, Color::WHITE.with_alpha(0.85), gem * 0.42),
                    Transform::from_xyz(0.0, 0.0, 0.5),
                ));
            });
        }

        // Sustain tail: extends upward (later in time).
        if event.is_sustain() {
            let tail_height = (event.sustain_s as f32) * scroll_speed;
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(color.with_alpha(0.35), Vec2::new(size * 0.35, tail_height)),
                    Transform::from_xyz(0.0, tail_height / 2.0, -1.0),
                ));
            });
        }
        // HOPO marker: a small bright core.
        if event.kind == NoteKind::Hopo {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(Color::WHITE, Vec2::splat(size * 0.4)),
                    Transform::from_xyz(0.0, 0.0, 1.0),
                ));
            });
        }
    }
}

/// Reposition all notes from the song timeline.
pub fn move_notes(
    mut commands: Commands,
    mut notes: Query<(Entity, &NoteSprite, &mut Transform)>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    // The track is identical across players; take any session's view.
    let Some((_, reference)) = players.iter().next() else {
        return;
    };
    let events = reference.session.track().events();
    for (entity, note, mut transform) in &mut notes {
        let Some(event) = events.get(note.event_index) else {
            continue;
        };
        transform.translation.y =
            RECEPTOR_Y + ((event.time_s - now) as f32) * settings.scroll_speed;
        if transform.translation.y < RECEPTOR_Y - 260.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Receptors light up while their player holds the fret.
pub fn update_receptors(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    theme: Res<crate::theme::ActiveTheme>,
    mut receptors: Query<(&Receptor, &mut Sprite)>,
) {
    for (index, player) in &players {
        let held = player.session.held();
        for (receptor, mut sprite) in &mut receptors {
            if receptor.player != index.0 {
                continue;
            }
            let color = theme.0.lane_color(receptor.lane);
            sprite.color = if held.contains(receptor.lane) {
                color
            } else {
                palette::dimmed(color, 0.35)
            };
        }
    }
}

/// React to session events: despawn hit notes, gray out missed ones.
pub fn apply_note_events(
    mut commands: Commands,
    mut notes: Query<(Entity, &mut NoteSprite, &mut Sprite)>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut feedback: MessageReader<SessionFeedback>,
) {
    // (player, event) pairs resolved this frame.
    let mut hit: Vec<(usize, usize)> = Vec::new();
    let mut missed: Vec<(usize, usize)> = Vec::new();
    for message in feedback.read() {
        match message.event {
            SessionEvent::NoteHit { event_index, .. } => {
                hit.push((message.player_index, event_index));
            }
            SessionEvent::NoteMissed { event_index } => {
                missed.push((message.player_index, event_index));
            }
            _ => {}
        }
    }
    if hit.is_empty() && missed.is_empty() {
        return;
    }
    for (entity, mut note, mut sprite) in &mut notes {
        if note.resolved {
            continue;
        }
        let key = (note.player, note.event_index);
        if hit.contains(&key) {
            // A sustain head stays visible (shrunk) while its tail runs.
            let session = players
                .iter()
                .find(|(index, _)| index.0 == note.player)
                .map(|(_, player)| &player.session);
            let is_sustain = session.is_some_and(|session| {
                matches!(
                    session.note_state(note.event_index),
                    Some(NoteState::Hit(_))
                ) && session
                    .track()
                    .events()
                    .get(note.event_index)
                    .is_some_and(beatbyte_core::NoteEvent::is_sustain)
            });
            if is_sustain {
                note.resolved = true;
                let size = sprite.custom_size.unwrap_or(Vec2::splat(30.0));
                sprite.custom_size = Some(size * 0.65);
            } else {
                commands.entity(entity).despawn();
            }
        } else if missed.contains(&key) {
            note.resolved = true;
            sprite.color = palette::dimmed(sprite.color, 0.25);
        }
    }
}

/// A lane-shaped sprite: the generated mask image tinted by `color`.
fn shape_sprite(shapes: &crate::shapes::LaneShapes, lane: Lane, color: Color, size: f32) -> Sprite {
    Sprite {
        image: shapes.image(lane),
        color,
        custom_size: Some(Vec2::splat(size)),
        ..Default::default()
    }
}
