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
    /// The lane's flat-view x (the depth view converges it).
    pub flat_x: f32,
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
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn spawn_highways(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    shapes: Res<crate::shapes::LaneShapes>,
    settings: Res<Settings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<bevy::sprite_render::ColorMaterial>>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
) {
    let theme = theme.0;
    let shapes = &*shapes;
    let round = settings.round_gems;
    let perspective = settings.perspective;
    for index in players.iter() {
        let player = index.0;
        let origin = layout.origin(player);
        // Highway bed (tagged: the beat pulse modulates its
        // brightness). Round style: a vertical depth gradient reads
        // as distance instead of a flat plate. Depth view: a real
        // trapezoid mesh converging on the vanishing point — a
        // rectangle sprite cannot do that.
        if perspective {
            let half = layout.bed_width() / 2.0;
            let far_scale = depth::project(f32::MAX).1.max(0.02);
            let near_y = RECEPTOR_Y - 200.0;
            let mesh = trapezoid_mesh(half, half * far_scale, near_y, depth::HORIZON_Y);
            commands.spawn((
                GameplayScreen,
                Mesh2d(meshes.add(mesh)),
                MeshMaterial2d(materials.add(bevy::sprite_render::ColorMaterial {
                    color: theme.surface,
                    texture: if round {
                        Some(shapes.bed_gradient())
                    } else {
                        None
                    },
                    ..Default::default()
                })),
                Transform::from_xyz(origin, 0.0, -10.0),
            ));
        } else {
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
        }
        // Stage vignette (round style): darkened corners focus the
        // lit highway. Sits above backdrop/bed/guides, below notes.
        if round {
            commands.spawn((
                GameplayScreen,
                Sprite {
                    image: shapes.vignette(),
                    color: Color::BLACK,
                    custom_size: Some(Vec2::new(4200.0, 2400.0)),
                    ..Default::default()
                },
                Transform::from_xyz(0.0, 0.0, -3.0),
            ));
        }
        // Lane guide lines — soft glow strips in the round style; in
        // the depth view they lean toward the vanishing point.
        for lane in Lane::ALL {
            let lane_x = layout.lane_x(player, lane);
            let transform = if perspective {
                let top = Vec2::new(origin, depth::HORIZON_Y);
                let bottom = Vec2::new(
                    depth::extend_below(origin, lane_x, 200.0),
                    RECEPTOR_Y - 200.0,
                );
                let delta = top - bottom;
                let mid = (top + bottom) / 2.0;
                Transform::from_xyz(mid.x, mid.y, -9.0)
                    .with_rotation(Quat::from_rotation_z(-delta.x.atan2(delta.y)))
            } else {
                Transform::from_xyz(lane_x, 0.0, -9.0)
            };
            let length = if perspective {
                (Vec2::new(origin, depth::HORIZON_Y)
                    - Vec2::new(
                        depth::extend_below(origin, lane_x, 200.0),
                        RECEPTOR_Y - 200.0,
                    ))
                .length()
            } else {
                900.0
            };
            commands.spawn((
                GameplayScreen,
                Sprite {
                    image: if round {
                        shapes.glow_strip()
                    } else {
                        Handle::default()
                    },
                    color: palette::dimmed(theme.lane_color(lane), if round { 0.16 } else { 0.06 }),
                    custom_size: Some(Vec2::new(if round { 10.0 } else { 2.0 }, length)),
                    ..Default::default()
                },
                transform,
            ));
        }
        // Receptor row: ring look = the gem body under a smaller
        // background copy. 8-bit style keeps the per-lane shapes
        // (colorblind-safe default); round style uses discs.
        let receptor = layout.receptor_size();
        // Depth view: receptors LIE on the board — a flattened ring
        // sells the perspective; and a glowing hit line spans the
        // highway where notes are judged.
        let squash = if perspective { 0.62 } else { 1.0 };
        if perspective {
            commands.spawn((
                GameplayScreen,
                Sprite {
                    image: if round {
                        shapes.glow_strip()
                    } else {
                        Handle::default()
                    },
                    color: Color::srgba(1.0, 1.0, 1.0, 0.16),
                    custom_size: Some(Vec2::new(layout.bed_width() * 1.02, 5.0)),
                    ..Default::default()
                },
                Transform::from_xyz(origin, RECEPTOR_Y, -6.0)
                    .with_rotation(Quat::from_rotation_z(core::f32::consts::FRAC_PI_2)),
            ));
        }
        for lane in Lane::ALL {
            let x = layout.lane_x(player, lane);
            let base_scale = Vec3::new(1.0, squash, 1.0);
            // The halo sits behind everything and is invisible until
            // the fret is touched.
            commands.spawn((
                GameplayScreen,
                ReceptorGlow {
                    player,
                    lane,
                    base_scale: base_scale * 2.1,
                },
                Sprite {
                    image: shapes.soft_dot(),
                    color: theme.lane_color(lane).with_alpha(0.0),
                    custom_size: Some(Vec2::splat(receptor * 1.9)),
                    ..Default::default()
                },
                Transform::from_xyz(x, RECEPTOR_Y, -7.0).with_scale(base_scale * 2.1),
            ));
            commands.spawn((
                GameplayScreen,
                Receptor { player, lane },
                ReceptorFx {
                    press: 0.0,
                    hit: 0.0,
                    base_scale,
                },
                gem_sprite(
                    shapes,
                    lane,
                    round,
                    palette::dimmed(theme.lane_color(lane), 0.35),
                    receptor,
                ),
                Transform::from_xyz(x, RECEPTOR_Y, -5.0).with_scale(base_scale),
            ));
            commands.spawn((
                GameplayScreen,
                gem_sprite(shapes, lane, round, theme.background, receptor * 0.72),
                Transform::from_xyz(x, RECEPTOR_Y, -4.0).with_scale(Vec3::new(1.0, squash, 1.0)),
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
                    flat_x: layout.lane_x(player, lane),
                },
                gem_sprite(shapes, lane, round, body_color, gem),
                Transform::from_xyz(layout.lane_x(player, lane), 2000.0, 0.0),
            ))
            .id();
        if round {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite {
                        image: shapes.soft_dot(),
                        color: color.with_alpha(0.4),
                        custom_size: Some(Vec2::splat(gem * 2.4)),
                        ..Default::default()
                    },
                    Transform::from_xyz(0.0, 0.0, -0.5),
                ));
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
    layout: Res<HighwayLayout>,
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
        let z = ((event.time_s - now) as f32) * settings.scroll_speed;
        if settings.perspective {
            let center = layout.origin(note.player);
            let (y, scale) = depth::project(z);
            transform.translation.y = if z >= 0.0 { y } else { RECEPTOR_Y + z };
            transform.translation.x = depth::lane_x(center, note.flat_x, scale.min(1.0));
            transform.scale = Vec3::splat(scale.clamp(0.35, 1.0));
        } else {
            transform.translation.y = RECEPTOR_Y + z;
        }
        if transform.translation.y < RECEPTOR_Y - 260.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// The perspective ("depth") view: a vanishing-point projection of
/// the note timeline. PRESENTATION ONLY — judgment is input-stamp
/// driven and never sees these numbers (proven by identical autopilot
/// scores in both views).
pub mod depth {
    use super::RECEPTOR_Y;

    /// Screen y the highway converges toward.
    pub const HORIZON_Y: f32 = 430.0;
    /// Perspective strength: world-pixels of travel that halve the
    /// scale. Smaller = more dramatic foreshortening.
    pub const FOCAL: f32 = 620.0;
    /// Scale at the horizon (nothing shrinks to literal zero).
    pub const MIN_SCALE: f32 = 0.02;

    /// Project a world distance ahead of the hit line (px, as the
    /// flat view would scroll it) to (screen_y, scale).
    #[must_use]
    pub fn project(z_px: f32) -> (f32, f32) {
        let z = z_px.max(0.0);
        let scale = (FOCAL / (FOCAL + z)).max(MIN_SCALE);
        let y = HORIZON_Y - (HORIZON_Y - RECEPTOR_Y) * scale;
        (y, scale)
    }

    /// Horizontal position: lane offsets converge toward the center.
    #[must_use]
    pub fn lane_x(center: f32, flat_x: f32, scale: f32) -> f32 {
        center + (flat_x - center) * scale
    }

    /// World-space point of a lane position `z` px ahead of the hit
    /// line — the SAME rule [`super::move_notes`] applies to gems
    /// (projected above the line, straight fall below). Anything
    /// drawn between two such points stays on the lane line.
    #[must_use]
    pub fn point(center: f32, flat_x: f32, z: f32) -> (f32, f32) {
        if z >= 0.0 {
            let (y, scale) = project(z);
            (lane_x(center, flat_x, scale.min(1.0)), y)
        } else {
            (flat_x, RECEPTOR_Y + z)
        }
    }

    /// Where the lane's vanishing line sits `dy_below` px BELOW the
    /// hit line — the same straight line notes travel, extended past
    /// the receptors. The first guides used a different line (full
    /// lane width at receptor−200 aimed at the vanishing point) and
    /// every note visibly missed its string.
    #[must_use]
    pub fn extend_below(center: f32, flat_x: f32, dy_below: f32) -> f32 {
        flat_x + (flat_x - center) * dy_below / (HORIZON_Y - RECEPTOR_Y)
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
    mut lines: Query<(&FretLine, &mut Transform, &mut Sprite)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    for (line, mut transform, mut sprite) in &mut lines {
        let z = ((line.time_s - now) as f32) * settings.scroll_speed;
        if settings.perspective {
            let (y, scale) = depth::project(z);
            transform.translation.y = if z >= 0.0 { y } else { RECEPTOR_Y + z };
            transform.scale = Vec3::new(scale.min(1.0), 1.0, 1.0);
            // Fade with distance — a wall of equal-strength lines
            // near the horizon reads as clutter, not depth.
            sprite.color = sprite.color.with_alpha(0.02 + 0.10 * scale.min(1.0));
        } else {
            transform.translation.y = RECEPTOR_Y + z;
        }
    }
}

/// While a sustain is HELD, its visuals come alive: the gem pins to
/// the receptor line, the tail is consumed from the bottom (remaining
/// hold time = remaining length) and both pulse. Released or ended,
/// everything falls back to the plain scrolling look. Runs after
/// [`move_notes`], which positions everything by chart time first.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn animate_sustains(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    mut notes: Query<(&NoteSprite, &mut Transform, &mut Sprite, &Children)>,
    mut tails: Query<(&SustainTail, &mut Transform, &mut Sprite), Without<NoteSprite>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let perspective = settings.perspective;
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
        let center = layout.origin(note.player);
        if held {
            // Pin the gem to the hit line; pulse it toward white.
            transform.translation.y = RECEPTOR_Y;
            transform.translation.x = note.flat_x;
            transform.scale = Vec3::ONE;
            let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 9.0).sin();
            sprite.color = color.mix(&Color::WHITE, 0.25 + 0.35 * pulse);
            let remaining =
                (((event.time_s + event.sustain_s - now).max(0.0)) as f32).min(f32::MAX);
            for child in children {
                if let Ok((tail, mut tail_transform, mut tail_sprite)) = tails.get_mut(*child) {
                    let flat_height = (remaining * tail.full_height
                        / (event.sustain_s as f32).max(f32::EPSILON))
                    .clamp(0.0, tail.full_height);
                    // Depth view: the far end sits where the
                    // projection puts that moment ON THE LANE LINE —
                    // both climbing and leaning, so the tail hugs its
                    // string instead of standing vertical.
                    if perspective {
                        let (fx, fy) = depth::point(center, note.flat_x, flat_height);
                        align_tail(
                            &mut tail_transform,
                            &mut tail_sprite,
                            tail.width,
                            fx - note.flat_x,
                            fy - RECEPTOR_Y,
                            1.0,
                        );
                    } else {
                        tail_sprite.custom_size = Some(Vec2::new(tail.width, flat_height));
                        tail_transform.translation.y = flat_height / 2.0;
                    }
                    tail_sprite.color = color.with_alpha(0.55 + 0.3 * pulse);
                }
            }
        } else {
            if note.resolved {
                // Released early or finished: a spent, dim tail.
                for child in children {
                    if let Ok((_, _, mut tail_sprite)) = tails.get_mut(*child) {
                        tail_sprite.color = color.with_alpha(0.15);
                    }
                }
            }
            // Approaching (or sliding past): in the depth view the
            // tail must follow the leaning, foreshortened lane line
            // from the gem to the projected point of its far end.
            if perspective {
                let z0 = ((event.time_s - now) as f32) * settings.scroll_speed;
                let head = (transform.translation.x, transform.translation.y);
                let parent_scale = transform.scale.x;
                for child in children {
                    if let Ok((tail, mut tail_transform, mut tail_sprite)) = tails.get_mut(*child) {
                        let (fx, fy) = depth::point(center, note.flat_x, z0 + tail.full_height);
                        align_tail(
                            &mut tail_transform,
                            &mut tail_sprite,
                            tail.width,
                            fx - head.0,
                            fy - head.1,
                            parent_scale,
                        );
                    }
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

/// Animation state of one receptor.
///
/// Kept per receptor rather than recomputed from the session each
/// frame because both quantities are *decays*: how hard the fret is
/// being pressed right now, and how recently a note landed on it.
#[derive(Component)]
pub struct ReceptorFx {
    /// 0..1, eased toward whether the fret is held.
    press: f32,
    /// 1.0 at the instant of a hit, decaying to 0.
    hit: f32,
    /// The scale the receptor rests at (carries the depth squash).
    base_scale: Vec3,
}

/// The halo behind a receptor, which is what makes a press read from
/// across the room.
#[derive(Component)]
pub struct ReceptorGlow {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
    /// Resting scale, carrying the depth squash.
    base_scale: Vec3,
}

/// How fast a press builds and releases (per second). Building is
/// faster than releasing: a fret should feel instant to light up and
/// linger just long enough to be seen.
const PRESS_ATTACK: f32 = 26.0;
/// Release rate of the press highlight.
const PRESS_RELEASE: f32 = 13.0;
/// How fast the hit flash decays (per second).
const HIT_DECAY: f32 = 6.5;

/// Receptors light up while their player holds the fret.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
pub fn update_receptors(
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    theme: Res<crate::theme::ActiveTheme>,
    mut feedback: MessageReader<SessionFeedback>,
    mut receptors: Query<(&Receptor, &mut ReceptorFx, &mut Sprite, &mut Transform)>,
    mut glows: Query<
        (&ReceptorGlow, &mut Sprite, &mut Transform),
        (Without<Receptor>, Without<NoteSprite>),
    >,
) {
    // Which frets took a hit this frame, and how good it was.
    let mut struck: Vec<(usize, Lane, f32)> = Vec::new();
    for message in feedback.read() {
        let SessionEvent::NoteHit {
            event_index,
            judgment,
            ..
        } = message.event
        else {
            continue;
        };
        let Some((_, player)) = players
            .iter()
            .find(|(index, _)| index.0 == message.player_index)
        else {
            continue;
        };
        let Some(event) = player.session.track().events().get(event_index) else {
            continue;
        };
        // A Perfect should land harder than a Good.
        let force = match judgment {
            beatbyte_core::Judgment::Perfect => 1.0,
            beatbyte_core::Judgment::Great => 0.8,
            _ => 0.62,
        };
        for lane in event.lanes.iter() {
            struck.push((message.player_index, lane, force));
        }
    }

    let delta = time.delta_secs();
    for (receptor, mut fx, mut sprite, mut transform) in &mut receptors {
        let held = players
            .iter()
            .find(|(index, _)| index.0 == receptor.player)
            .is_some_and(|(_, player)| player.session.held().contains(receptor.lane));

        // Press: fast toward 1 while held, slower back to 0.
        let rate = if held { PRESS_ATTACK } else { PRESS_RELEASE };
        let target = if held { 1.0 } else { 0.0 };
        fx.press += (target - fx.press) * (rate * delta).min(1.0);

        if let Some((_, _, force)) = struck
            .iter()
            .find(|(p, lane, _)| *p == receptor.player && *lane == receptor.lane)
        {
            fx.hit = fx.hit.max(*force);
        }
        fx.hit = (fx.hit - HIT_DECAY * delta).max(0.0);

        let color = theme.0.lane_color(receptor.lane);
        // Resting frets stay dim; a pressed one goes to full colour
        // and then washes toward white, which is what separates
        // "armed" from "played" at a glance.
        let lit = palette::dimmed(color, 0.35).mix(&color, fx.press);
        sprite.color = lit.mix(&Color::WHITE, 0.55 * fx.hit + 0.18 * fx.press);
        // Squash-preserving pop: press swells it, a hit punches it.
        let swell = 0.14f32.mul_add(fx.press, 1.0) + 0.34 * fx.hit;
        transform.scale = fx.base_scale * swell;
    }

    for (glow, mut sprite, mut transform) in &mut glows {
        let Some((_, fx, _, _)) = receptors.iter().find(|(receptor, _, _, _)| {
            receptor.player == glow.player && receptor.lane == glow.lane
        }) else {
            continue;
        };
        let color = theme.0.lane_color(glow.lane);
        let intensity = 0.55f32.mul_add(fx.press, 0.9 * fx.hit).min(1.0);
        sprite.color = color.mix(&Color::WHITE, 0.4 * fx.hit).with_alpha(intensity);
        transform.scale = glow.base_scale * 0.55f32.mul_add(fx.hit, 1.0);
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

/// A trapezoid running from a wide near edge to a narrow far edge —
/// the depth view's highway bed. UV v=1 at the near edge matches the
/// bed gradient's "lighter near" orientation.
fn trapezoid_mesh(near_half: f32, far_half: f32, near_y: f32, far_y: f32) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-near_half, near_y, 0.0],
            [near_half, near_y, 0.0],
            [far_half, far_y, 0.0],
            [-far_half, far_y, 0.0],
        ],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 2, 3, 0]));
    mesh
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

/// Point a tail sprite from its parent (the gem) to a world-space
/// offset `(dx, dy)`: length, midpoint and rotation in the parent's
/// LOCAL space (the parent carries a uniform `parent_scale`). The
/// depth view leans lanes toward the vanishing point — a tail left
/// vertical visibly detaches from its string (user screenshot).
fn align_tail(
    transform: &mut Transform,
    sprite: &mut Sprite,
    width: f32,
    dx: f32,
    dy: f32,
    parent_scale: f32,
) {
    let s = parent_scale.max(f32::EPSILON);
    let (lx, ly) = (dx / s, dy / s);
    sprite.custom_size = Some(Vec2::new(width, lx.hypot(ly)));
    transform.translation.x = lx / 2.0;
    transform.translation.y = ly / 2.0;
    // Rotation that maps the sprite's +Y axis onto (lx, ly).
    transform.rotation = Quat::from_rotation_z((-lx).atan2(ly));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod depth_tests {
    use super::RECEPTOR_Y;
    use super::depth;

    #[test]
    fn the_hit_line_is_the_identity_point() {
        let (y, scale) = depth::project(0.0);
        assert!((y - RECEPTOR_Y).abs() < 1e-4);
        assert!((scale - 1.0).abs() < 1e-4);
    }

    #[test]
    fn distance_climbs_toward_the_horizon_and_shrinks() {
        let mut last_y = RECEPTOR_Y - 1.0;
        let mut last_scale = 1.1;
        for step in 0..40 {
            let (y, scale) = depth::project(step as f32 * 200.0);
            assert!(y > last_y, "y must climb monotonically");
            assert!(scale < last_scale, "scale must shrink monotonically");
            assert!(y < depth::HORIZON_Y + 1e-3, "never past the horizon");
            last_y = y;
            last_scale = scale;
        }
    }

    #[test]
    fn guide_extension_is_collinear_with_the_note_path() {
        // The line notes travel: (flat_x, RECEPTOR_Y) -> (center, HORIZON_Y).
        let (center, flat_x, dy) = (100.0_f32, 400.0_f32, 200.0_f32);
        let below_x = depth::extend_below(center, flat_x, dy);
        let slope_path = (center - flat_x) / (depth::HORIZON_Y - RECEPTOR_Y);
        let slope_ext = (flat_x - below_x) / dy;
        assert!(
            (slope_path - slope_ext).abs() < 1e-4,
            "guide extension bends off the note path: {slope_path} vs {slope_ext}"
        );
    }

    #[test]
    fn lane_points_are_collinear_with_the_string() {
        // Every projected point must sit on the straight screen line
        // (flat_x, RECEPTOR_Y) -> (center, HORIZON_Y): that line IS
        // the drawn string. A sustain tail drawn between two such
        // points therefore hugs it.
        let (center, flat_x) = (640.0_f32, 210.0_f32);
        for z in [0.0_f32, 150.0, 400.0, 900.0, 2500.0] {
            let (x, y) = depth::point(center, flat_x, z);
            // f64 cross product: the operands are ~3e5, where f32's
            // own rounding already costs ~0.04 — 0.5 is still five
            // orders of magnitude below a visible bend (the original
            // guide bug measured in the thousands).
            let cross = (f64::from(x) - f64::from(flat_x))
                * f64::from(depth::HORIZON_Y - RECEPTOR_Y)
                - (f64::from(y) - f64::from(RECEPTOR_Y)) * f64::from(center - flat_x);
            assert!(
                cross.abs() < 0.5,
                "point at z={z} leaves the string: cross={cross}"
            );
        }
        // Below the hit line the note falls straight down.
        let (x, y) = depth::point(center, flat_x, -120.0);
        assert!((x - flat_x).abs() < 1e-4);
        assert!((y - (RECEPTOR_Y - 120.0)).abs() < 1e-4);
    }

    #[test]
    fn tails_rotate_onto_their_string() {
        use bevy::prelude::{Sprite, Transform, Vec3};
        let mut transform = Transform::default();
        let mut sprite = Sprite::default();
        // Straight up: no rotation, full length, midpoint at half.
        super::align_tail(&mut transform, &mut sprite, 10.0, 0.0, 200.0, 1.0);
        assert!(
            transform
                .rotation
                .to_euler(bevy::math::EulerRot::ZYX)
                .0
                .abs()
                < 1e-5
        );
        assert!((sprite.custom_size.unwrap().y - 200.0).abs() < 1e-3);
        assert!((transform.translation.y - 100.0).abs() < 1e-3);
        // Leaning left and up: positive Z rotation (counterclockwise),
        // length is the hypotenuse, all divided by the parent scale.
        super::align_tail(&mut transform, &mut sprite, 10.0, -60.0, 80.0, 0.5);
        let angle = transform.rotation.to_euler(bevy::math::EulerRot::ZYX).0;
        assert!(
            angle > 0.0,
            "left lean must rotate counterclockwise: {angle}"
        );
        assert!(
            (sprite.custom_size.unwrap().y - 200.0).abs() < 1e-3,
            "hypot(120,160)=200"
        );
        assert!(
            (transform.translation - Vec3::new(-60.0, 80.0, 0.0)).length() < 1e-3,
            "midpoint in parent-local units"
        );
    }

    #[test]
    fn lanes_converge_on_the_center() {
        let near = depth::lane_x(0.0, 300.0, 1.0);
        let far = depth::lane_x(0.0, 300.0, depth::project(4000.0).1);
        assert!((near - 300.0).abs() < 1e-4);
        assert!(far.abs() < 60.0, "far lanes must pull toward center: {far}");
    }
}
