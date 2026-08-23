//! Highway, receptors and note sprites.
//!
//! Note positions are a pure function of song time — a dropped frame
//! shifts pixels for one frame, never judgment.

use beatbyte_core::session::NoteState;
use beatbyte_core::{Lane, NoteKind, SessionEvent};
use bevy::prelude::*;

use super::{
    GameplayScreen, LANE_STEP, PlayerSession, RECEPTOR_Y, SCROLL_SPEED, SPAWN_LOOKAHEAD_S,
    SessionFeedback, lane_x, note_y,
};
use crate::audio_sys::GameClock;
use crate::palette;

/// One visible note head (chords have one per lane).
#[derive(Component)]
pub struct NoteSprite {
    /// Index into the track's events.
    pub event_index: usize,
    /// Whether this event was already resolved visually.
    pub resolved: bool,
}

/// A receptor (hit-line target) for one lane.
#[derive(Component)]
pub struct Receptor(pub Lane);

/// Tracks how far note spawning has progressed.
#[derive(Component)]
pub struct SpawnCursor(pub usize);

/// Build the static highway: bed, receptor row, lane guides.
pub fn spawn_highway(mut commands: Commands) {
    // Highway bed (tagged: the beat pulse modulates its brightness).
    commands.spawn((
        GameplayScreen,
        super::fx::HighwayBed,
        Sprite::from_color(palette::SURFACE, Vec2::new(LANE_STEP * 5.0 + 24.0, 900.0)),
        Transform::from_xyz(0.0, 0.0, -10.0),
    ));
    // Lane guide lines.
    for lane in Lane::ALL {
        commands.spawn((
            GameplayScreen,
            Sprite::from_color(
                palette::dimmed(palette::lane_color(lane), 0.06),
                Vec2::new(2.0, 900.0),
            ),
            Transform::from_xyz(lane_x(lane), 0.0, -9.0),
        ));
    }
    // Receptor row: ring look = colored square under a background square.
    for lane in Lane::ALL {
        commands.spawn((
            GameplayScreen,
            Receptor(lane),
            Sprite::from_color(
                palette::dimmed(palette::lane_color(lane), 0.35),
                Vec2::new(44.0, 44.0),
            ),
            Transform::from_xyz(lane_x(lane), RECEPTOR_Y, -5.0),
        ));
        commands.spawn((
            GameplayScreen,
            Sprite::from_color(palette::BACKGROUND, Vec2::new(34.0, 34.0)),
            Transform::from_xyz(lane_x(lane), RECEPTOR_Y, -4.0),
        ));
    }
    // Spawn cursor.
    commands.spawn((GameplayScreen, SpawnCursor(0)));
}

/// Spawn note entities as their time approaches.
pub fn spawn_due_notes(
    mut commands: Commands,
    mut cursor: Query<&mut SpawnCursor>,
    players: Query<&PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
) {
    let Ok(mut cursor) = cursor.single_mut() else {
        return;
    };
    let (Some(now), Ok(player)) = (game_clock.song_time(&time), players.single()) else {
        return;
    };
    let events = player.session.track().events();
    while cursor.0 < events.len() {
        let event = events[cursor.0];
        if event.time_s > now + SPAWN_LOOKAHEAD_S {
            break;
        }
        spawn_event_sprites(&mut commands, cursor.0, &event);
        cursor.0 += 1;
    }
}

fn spawn_event_sprites(
    commands: &mut Commands,
    event_index: usize,
    event: &beatbyte_core::NoteEvent,
) {
    for lane in event.lanes.iter() {
        let color = palette::lane_color(lane);
        let entity = commands
            .spawn((
                GameplayScreen,
                NoteSprite {
                    event_index,
                    resolved: false,
                },
                Sprite::from_color(color, Vec2::new(34.0, 34.0)),
                Transform::from_xyz(lane_x(lane), 2000.0, 0.0),
            ))
            .id();

        // Sustain tail: extends upward (later in time).
        if event.is_sustain() {
            let tail_height = (event.sustain_s as f32) * SCROLL_SPEED;
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(color.with_alpha(0.35), Vec2::new(12.0, tail_height)),
                    Transform::from_xyz(0.0, tail_height / 2.0, -1.0),
                ));
            });
        }
        // HOPO marker: a small bright core.
        if event.kind == NoteKind::Hopo {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(Color::WHITE, Vec2::new(14.0, 14.0)),
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
    players: Query<&PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
) {
    let (Some(now), Ok(player)) = (game_clock.song_time(&time), players.single()) else {
        return;
    };
    let events = player.session.track().events();
    for (entity, note, mut transform) in &mut notes {
        let Some(event) = events.get(note.event_index) else {
            continue;
        };
        transform.translation.y = note_y(event.time_s, now);
        // Off the bottom (well past miss territory): clean up.
        if transform.translation.y < RECEPTOR_Y - 260.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Receptors light up while their fret is held.
pub fn update_receptors(
    players: Query<&PlayerSession>,
    mut receptors: Query<(&Receptor, &mut Sprite)>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let held = player.session.held();
    for (receptor, mut sprite) in &mut receptors {
        let color = palette::lane_color(receptor.0);
        sprite.color = if held.contains(receptor.0) {
            color
        } else {
            palette::dimmed(color, 0.35)
        };
    }
}

/// React to session events: despawn hit notes, gray out missed ones.
pub fn apply_note_events(
    mut commands: Commands,
    mut notes: Query<(Entity, &mut NoteSprite, &mut Sprite)>,
    players: Query<&PlayerSession>,
    mut feedback: MessageReader<SessionFeedback>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    // Collect the events that resolve notes this frame.
    let mut hit_events: Vec<usize> = Vec::new();
    let mut missed_events: Vec<usize> = Vec::new();
    for message in feedback.read() {
        match message.event {
            SessionEvent::NoteHit { event_index, .. } => hit_events.push(event_index),
            SessionEvent::NoteMissed { event_index } => missed_events.push(event_index),
            _ => {}
        }
    }
    if hit_events.is_empty() && missed_events.is_empty() {
        return;
    }
    for (entity, mut note, mut sprite) in &mut notes {
        if note.resolved {
            continue;
        }
        if hit_events.contains(&note.event_index) {
            // A sustain head stays visible (shrunk) while its tail runs.
            let is_sustain = matches!(
                player.session.note_state(note.event_index),
                Some(NoteState::Hit(_))
            ) && player
                .session
                .track()
                .events()
                .get(note.event_index)
                .is_some_and(beatbyte_core::NoteEvent::is_sustain);
            if is_sustain {
                note.resolved = true;
                sprite.custom_size = Some(Vec2::new(22.0, 22.0));
            } else {
                commands.entity(entity).despawn();
            }
        } else if missed_events.contains(&note.event_index) {
            note.resolved = true;
            sprite.color = palette::dimmed(sprite.color, 0.25);
        }
    }
}
