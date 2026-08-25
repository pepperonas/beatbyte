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
    settings: Res<Settings>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
) {
    let theme = theme.0;
    let shapes = &*shapes;
    let round = settings.round_gems;
    for index in players.iter() {
        let player = index.0;
        let origin = layout.origin(player);
        // Highway bed (tagged: the beat pulse modulates its
        // brightness). Round style: a vertical depth gradient reads
        // as distance instead of a flat plate.
        commands.spawn((
            GameplayScreen,
            super::fx::HighwayBed,
            Sprite {
                image: if round {
                    shapes.bed_gradient()
                } else {
                    Handle::default()
                },
                color: theme.surface,
                custom_size: Some(Vec2::new(layout.bed_width(), 900.0)),
                ..Default::default()
            },
            Transform::from_xyz(origin, 0.0, -10.0),
        ));
        // Lane guide lines — soft glow strips in the round style.
        for lane in Lane::ALL {
            commands.spawn((
                GameplayScreen,
                Sprite {
                    image: if round {
                        shapes.glow_strip()
                    } else {
                        Handle::default()
                    },
                    color: palette::dimmed(theme.lane_color(lane), if round { 0.16 } else { 0.06 }),
                    custom_size: Some(Vec2::new(if round { 10.0 } else { 2.0 }, 900.0)),
                    ..Default::default()
                },
                Transform::from_xyz(layout.lane_x(player, lane), 0.0, -9.0),
            ));
        }
        // Receptor row: ring look = the gem body under a smaller
        // background copy. 8-bit style keeps the per-lane shapes
        // (colorblind-safe default); round style uses discs.
        let receptor = layout.receptor_size();
        for lane in Lane::ALL {
            let x = layout.lane_x(player, lane);
            commands.spawn((
                GameplayScreen,
                Receptor { player, lane },
                gem_sprite(
                    shapes,
                    lane,
                    round,
                    palette::dimmed(theme.lane_color(lane), 0.35),
                    receptor,
                ),
                Transform::from_xyz(x, RECEPTOR_Y, -5.0),
            ));
            commands.spawn((
                GameplayScreen,
                gem_sprite(shapes, lane, round, theme.background, receptor * 0.72),
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
                settings.round_gems,
                index.0,
                cursor,
                &event,
                settings.scroll_speed,
            );
            player.spawn_cursor += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)] // internal helper mirroring the system's DI
fn spawn_event_sprites(
    commands: &mut Commands,
    layout: &HighwayLayout,
    theme: &crate::theme::Theme,
    shapes: &crate::shapes::LaneShapes,
    round: bool,
    player: usize,
    event_index: usize,
    event: &beatbyte_core::NoteEvent,
    scroll_speed: f32,
) {
    let size = layout.note_size();
    let hopo = matches!(event.kind, beatbyte_core::NoteKind::Hopo);
    for lane in event.lanes.iter() {
        let color = theme.lane_color(lane);
        // 8-bit: HOPOs render smaller with a bright core. Round: all
        // gems the same size, white center on every note, dark ring
        // ONLY on strum notes — the documented classic distinction.
        let gem = if hopo && !round { size * 0.78 } else { size };
        // Round gems run slightly emissive so the HDR bloom makes
        // them glow.
        let body_color = if round { emissive(color, 1.35) } else { color };
        let entity = commands
            .spawn((
                GameplayScreen,
                NoteSprite {
                    player,
                    event_index,
                    resolved: false,
                },
                gem_sprite(shapes, lane, round, body_color, gem),
                Transform::from_xyz(layout.lane_x(player, lane), 2000.0, 0.0),
            ))
            .id();
        if round {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        image: shapes.sphere_gloss(),
                        color: Color::WHITE,
                        custom_size: Some(Vec2::splat(gem)),
                        ..Default::default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.6),
                ));
                parent.spawn((
                    Sprite {
                        image: shapes.round_core(),
                        color: Color::WHITE.with_alpha(0.9),
                        custom_size: Some(Vec2::splat(gem)),
                        ..Default::default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.5),
                ));
                if !hopo {
                    parent.spawn((
                        Sprite {
                            image: shapes.round_ring(),
                            color: Color::BLACK.with_alpha(0.8),
                            custom_size: Some(Vec2::splat(gem)),
                            ..Default::default()
                        },
                        Transform::from_xyz(0.0, 0.0, 0.4),
                    ));
                }
            });
        } else if hopo {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    shape_sprite(shapes, lane, Color::WHITE.with_alpha(0.85), gem * 0.42),
                    Transform::from_xyz(0.0, 0.0, 0.5),
                ));
            });
        }

        // Sustain tail: extends upward (later in time). Round style:
        // a soft glowing tube instead of a hard rectangle.
        if event.is_sustain() {
            let tail_height = (event.sustain_s as f32) * scroll_speed;
            let tail_width = if round { size * 0.55 } else { size * 0.35 };
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    SustainTail {
                        full_height: tail_height,
                        width: tail_width,
                    },
                    Sprite {
                        image: if round {
                            shapes.tube()
                        } else {
                            Handle::default()
                        },
                        color: color.with_alpha(0.35),
                        custom_size: Some(Vec2::new(tail_width, tail_height)),
                        ..Default::default()
                    },
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

/// A bar line ("fret") across the highway — round style only, the
/// classic look's fretboard feel. Spawned once per bar, scrolled by
/// [`move_fret_lines`].
#[derive(Component)]
pub struct FretLine {
    /// Song time this line sits on.
    pub time_s: f64,
}

/// Spawn one line per bar over the whole song (a few dozen thin
/// sprites — cheap, and most sit far off screen).
pub fn spawn_fret_lines(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    settings: Res<Settings>,
    shapes: Res<crate::shapes::LaneShapes>,
    song: Res<crate::boot::LoadedSong>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
) {
    if !settings.round_gems {
        return;
    }
    let bpm = song.chart.song.bpm.clamp(20.0, 400.0);
    let bar_s = 240.0 / bpm;
    let start = song.chart.song.offset_s;
    let end = song.chart.song.duration_s.unwrap_or(start + 240.0);
    for index in players.iter() {
        let origin = layout.origin(index.0);
        let mut t = start;
        while t < end {
            commands.spawn((
                GameplayScreen,
                FretLine { time_s: t },
                Sprite {
                    image: shapes.glow_strip(),
                    color: Color::srgba(1.0, 1.0, 1.0, 0.10),
                    custom_size: Some(Vec2::new(layout.bed_width(), 6.0)),
                    ..Default::default()
                },
                Transform::from_xyz(origin, 2000.0, -8.0),
            ));
            t += bar_s;
        }
    }
}

/// Scroll the bar lines with the song, like notes.
pub fn move_fret_lines(
    mut lines: Query<(&FretLine, &mut Transform)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    for (line, mut transform) in &mut lines {
        transform.translation.y = RECEPTOR_Y + ((line.time_s - now) as f32) * settings.scroll_speed;
    }
}

/// While a sustain is HELD, its visuals come alive: the gem pins to
/// the receptor line, the tail is consumed from the bottom (remaining
/// hold time = remaining length) and both pulse. Released or ended,
/// everything falls back to the plain scrolling look. Runs after
/// [`move_notes`], which positions everything by chart time first.
pub fn animate_sustains(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    theme: Res<crate::theme::ActiveTheme>,
    mut notes: Query<(&NoteSprite, &mut Transform, &mut Sprite, &Children)>,
    mut tails: Query<(&SustainTail, &mut Transform, &mut Sprite), Without<NoteSprite>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let active: Vec<(usize, usize)> = players
        .iter()
        .filter_map(|(index, player)| player.session.active_sustain().map(|e| (index.0, e)))
        .collect();
    let Some((_, reference)) = players.iter().next() else {
        return;
    };
    let events = reference.session.track().events();

    for (note, mut transform, mut sprite, children) in &mut notes {
        let Some(event) = events.get(note.event_index) else {
            continue;
        };
        if !event.is_sustain() {
            continue;
        }
        let lane = event.lanes.highest().unwrap_or(beatbyte_core::Lane::Three);
        let color = theme.0.lane_color(lane);
        let held = active.contains(&(note.player, note.event_index));
        if held {
            // Pin the gem to the hit line; pulse it toward white.
            transform.translation.y = RECEPTOR_Y;
            let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 9.0).sin();
            sprite.color = color.mix(&Color::WHITE, 0.25 + 0.35 * pulse);
            let remaining =
                (((event.time_s + event.sustain_s - now).max(0.0)) as f32).min(f32::MAX);
            for child in children {
                if let Ok((tail, mut tail_transform, mut tail_sprite)) = tails.get_mut(*child) {
                    let height = (remaining * tail.full_height
                        / (event.sustain_s as f32).max(f32::EPSILON))
                    .clamp(0.0, tail.full_height);
                    tail_sprite.custom_size = Some(Vec2::new(tail.width, height));
                    tail_transform.translation.y = height / 2.0;
                    tail_sprite.color = color.with_alpha(0.55 + 0.3 * pulse);
                }
            }
        } else if note.resolved {
            // Released early or finished: a spent, dim tail.
            for child in children {
                if let Ok((_, _, mut tail_sprite)) = tails.get_mut(*child) {
                    tail_sprite.color = color.with_alpha(0.15);
                }
            }
        }
    }
}

/// Marker + geometry for a sustain's tail sprite.
#[derive(Component)]
pub struct SustainTail {
    /// Height representing the full sustain length.
    pub full_height: f32,
    /// Tail width in pixels.
    pub width: f32,
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

/// A color pushed past 1.0 in linear space — under the HDR camera the
/// bloom pass turns the excess into glow. Alpha stays untouched.
fn emissive(color: Color, factor: f32) -> Color {
    let linear = color.to_linear();
    Color::LinearRgba(bevy::color::LinearRgba {
        red: linear.red * factor,
        green: linear.green * factor,
        blue: linear.blue * factor,
        alpha: linear.alpha,
    })
}

/// The gem body in the active note style (8-bit lane shape or disc).
fn gem_sprite(
    shapes: &crate::shapes::LaneShapes,
    lane: Lane,
    round: bool,
    color: Color,
    size: f32,
) -> Sprite {
    Sprite {
        image: shapes.body(lane, round),
        color,
        custom_size: Some(Vec2::splat(size)),
        ..Default::default()
    }
}
