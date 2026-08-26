//! The solid 3D stage: a lit highway with real geometry.
//!
//! This is a third view alongside the flat and depth ones rather than
//! a replacement, for the same reason the depth view was: judgment is
//! input-stamp driven and must not change when the presentation does.
//! A run in 3D has to score exactly what the same run scores in 2D,
//! and keeping all three alive is what makes that testable.
//!
//! Geometry conventions, once, so nothing downstream has to guess:
//!
//! - **+X** is right across the neck, **+Y** is up out of the
//!   highway, **−Z** runs away from the player toward the horizon.
//! - The hit line is `z = 0`. A note `t` seconds in the future sits at
//!   `z = -t * scroll_speed * WORLD_PER_PIXEL`, so the existing
//!   scroll-speed setting keeps meaning what it meant.
//! - One world unit is [`WORLD_PER_PIXEL`] of the 2D layout, so lane
//!   spacing, receptor sizes and scroll speed all carry over instead
//!   of being re-tuned by eye.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use beatbyte_core::Lane;

use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession};
use crate::audio_sys::GameClock;
use crate::config::Settings;
use crate::states::AppState;

/// World units per pixel of the 2D layout.
///
/// The 2D stage is laid out in a 1280x720 pixel space; dividing by
/// 220 puts a five-lane neck at roughly 2.2 units wide, which is a
/// comfortable size for a perspective camera with a 45° field of
/// view and keeps the numbers readable while tuning.
pub const WORLD_PER_PIXEL: f32 = 1.0 / 220.0;

/// How far ahead the highway is drawn, in world units.
const HIGHWAY_LENGTH: f32 = 26.0;

/// How far behind the hit line the highway continues, so the near
/// edge is never visible as a cut-off.
const HIGHWAY_BEHIND: f32 = 2.5;

/// Radius of a note's coloured face, in world units. Large relative
/// to the lane spacing, as in the games this borrows from — a gem
/// nearly fills its lane, which is what makes a chord read as one
/// shape rather than three dots.
const GEM_RADIUS: f32 = 0.17;

/// Render layer for the 3D stage. The HUD camera renders layer 0 and
/// the stage camera renders this one, so neither draws the other's
/// entities — without it the 2D backdrop appears inside the 3D scene.
pub const STAGE_LAYER: usize = 1;

/// Marker for everything belonging to the 3D stage.
#[derive(Component)]
pub struct Stage3d;

/// A note rendered as 3D geometry.
#[derive(Component)]
pub struct Note3d {
    /// Which player's highway this belongs to.
    pub player: usize,
    /// Index into the track's events.
    pub event_index: usize,
    /// Lane, which fixes the x position.
    pub lane: Lane,
}

/// A bar line across the neck, scrolling with the song.
///
/// The fretboard's cross-bars are not decoration: they are the only
/// thing that tells a player how far away a note is in BEATS rather
/// than in pixels, which is what makes a gap readable at speed.
#[derive(Component)]
pub struct FretBar {
    /// Song time this bar sits on.
    pub time_s: f64,
}

/// A receptor rendered as 3D geometry.
#[derive(Component)]
pub struct Receptor3d {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
}

/// Whether the 3D stage is the active view.
#[must_use]
pub fn active(settings: &Settings) -> bool {
    settings.stage_3d
}

/// Lane centre in world units for a player's highway.
#[must_use]
pub fn lane_x(layout: &HighwayLayout, player: usize, lane: Lane) -> f32 {
    layout.lane_x(player, lane) * WORLD_PER_PIXEL
}

/// Distance ahead of the hit line for a note `seconds` away.
#[must_use]
pub fn note_z(seconds: f64, scroll_speed: f32) -> f32 {
    -(seconds as f32) * scroll_speed * WORLD_PER_PIXEL
}

/// Set up camera, lights and highway geometry.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn setup_stage(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let stage = theme.0;

    // The camera sits behind and above the hit line, tilted down the
    // neck. This is the framing the genre settled on: close enough
    // that the receptors are large and readable, high enough that the
    // approaching notes separate instead of overlapping.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        Camera3d::default(),
        Camera {
            order: -1,
            ..default()
        },
        // HDR is a marker component in this Bevy version; bloom
        // requires it.
        bevy::camera::Hdr,
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0f32.to_radians(),
            ..default()
        }),
        // Further back and higher than the first attempt: at 2.35/3.6
        // the nearest gems filled a third of the screen and the row of
        // receptors ran off both edges of the bed.
        Transform::from_xyz(0.0, 3.1, 5.2).looking_at(Vec3::new(0.0, 0.05, -7.5), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
        bevy::post_process::bloom::Bloom {
            intensity: 0.18,
            ..bevy::post_process::bloom::Bloom::NATURAL
        },
    ));

    // Key light down the neck plus a soft fill, so gems read as
    // spheres rather than flat discs.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        DirectionalLight {
            illuminance: 5_500.0,
            ..default()
        },
        Transform::from_xyz(2.0, 6.0, 2.0).looking_at(Vec3::new(0.0, 0.0, -8.0), Vec3::Y),
        RenderLayers::layer(STAGE_LAYER),
    ));
    // Ambient light is a component on the camera in this version.
    commands.spawn((
        GameplayScreen,
        Stage3d,
        AmbientLight {
            color: stage.accent,
            brightness: 220.0,
            ..default()
        },
        RenderLayers::layer(STAGE_LAYER),
    ));

    let bed = meshes.add(Cuboid::new(1.0, 0.06, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let rail = meshes.add(Cuboid::new(0.035, 0.05, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    let lane_strip = meshes.add(Cuboid::new(0.018, 0.012, HIGHWAY_LENGTH + HIGHWAY_BEHIND));
    // A ring, not a disc: with both drawn as discs a resting receptor
    // and an approaching note were the same shape.
    let receptor_mesh = meshes.add(Torus::new(GEM_RADIUS * 0.82, GEM_RADIUS * 1.12));
    let hit_bar = meshes.add(Cuboid::new(1.0, 0.02, 0.06));

    for index in &players {
        let player = index.0;
        let origin = layout.origin(player) * WORLD_PER_PIXEL;
        // A little wider than the lane span so the outer receptors
        // sit ON the neck rather than half off it.
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.18;
        let centre = -HIGHWAY_LENGTH / 2.0 + HIGHWAY_BEHIND / 2.0;

        // The bed. Dark and slightly reflective so the lights and the
        // gems have something to sit on.
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(bed.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                // Light enough to read AS a fretboard: against a
                // black bed the gems floated in a void and the neck
                // had no surface for the lights to land on.
                base_color: stage.background.mix(&Color::WHITE, 0.16),
                perceptual_roughness: 0.42,
                metallic: 0.2,
                ..default()
            })),
            Transform::from_xyz(origin, -0.03, centre).with_scale(Vec3::new(width, 1.0, 1.0)),
            RenderLayers::layer(STAGE_LAYER),
        ));

        // Bright rails down both edges. They frame the neck and, more
        // usefully, give the eye a fixed reference for how wide the
        // playfield is as it recedes.
        for side in [-1.0f32, 1.0] {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(rail.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: stage.accent,
                    emissive: stage.accent.to_linear() * 2.6,
                    ..default()
                })),
                Transform::from_xyz(origin + side * width / 2.0, 0.015, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // One glowing strip per lane, which is what gives the neck its
        // sense of depth as it recedes.
        for lane in Lane::ALL {
            let colour = stage.lane_color(lane);
            commands.spawn((
                GameplayScreen,
                Stage3d,
                Mesh3d(lane_strip.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour,
                    emissive: colour.to_linear() * 1.4,
                    unlit: false,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.005, centre),
                RenderLayers::layer(STAGE_LAYER),
            ));

            commands.spawn((
                GameplayScreen,
                Stage3d,
                Receptor3d { player, lane },
                Mesh3d(receptor_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: colour,
                    emissive: colour.to_linear() * 0.35,
                    perceptual_roughness: 0.35,
                    ..default()
                })),
                Transform::from_xyz(lane_x(&layout, player, lane), 0.02, 0.0),
                RenderLayers::layer(STAGE_LAYER),
            ));
        }

        // The hit line itself.
        commands.spawn((
            GameplayScreen,
            Stage3d,
            Mesh3d(hit_bar.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::rgb(1.6, 1.6, 1.8),
                ..default()
            })),
            Transform::from_xyz(origin, 0.01, 0.0).with_scale(Vec3::new(width * 1.02, 1.0, 1.0)),
            RenderLayers::layer(STAGE_LAYER),
        ));
    }
}

/// Press and hit feedback for the 3D receptors.
///
/// The same two decays the 2D stage animates — how hard the fret is
/// held and how recently a note landed — expressed in geometry and
/// emission instead of sprite colour, so both views say the same
/// thing in their own vocabulary.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
pub fn update_receptors(
    time: Res<Time>,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut feedback: MessageReader<super::SessionFeedback>,
    mut receptors: Query<(
        &Receptor3d,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<Vec<(usize, Lane, f32, f32)>>,
) {
    if !active(&settings) {
        return;
    }
    // Which frets were struck this frame, and how cleanly.
    let mut struck: Vec<(usize, Lane, f32)> = Vec::new();
    for message in feedback.read() {
        let beatbyte_core::SessionEvent::NoteHit {
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
    for (receptor, mut transform, material) in &mut receptors {
        let held = players
            .iter()
            .find(|(index, _)| index.0 == receptor.player)
            .is_some_and(|(_, player)| player.session.held().contains(receptor.lane));

        let slot = state
            .iter()
            .position(|(p, l, _, _)| *p == receptor.player && *l == receptor.lane)
            .unwrap_or_else(|| {
                state.push((receptor.player, receptor.lane, 0.0, 0.0));
                state.len() - 1
            });
        let (_, _, press, hit) = &mut state[slot];
        let rate = if held { 26.0 } else { 13.0 };
        let target = if held { 1.0 } else { 0.0 };
        *press += (target - *press) * (rate * delta).min(1.0);
        if let Some((_, _, force)) = struck
            .iter()
            .find(|(p, l, _)| *p == receptor.player && *l == receptor.lane)
        {
            *hit = hit.max(*force);
        }
        *hit = (*hit - 7.5 * delta).max(0.0);
        let (press, hit) = (*press, *hit);

        // Pressed frets sink into the neck; a hit makes one jump.
        transform.translation.y = 0.045f32.mul_add(-press, 0.02) + 0.06 * hit;
        transform.scale = Vec3::splat(0.12f32.mul_add(hit, 1.0));

        // Emission is the 3D equivalent of the 2D fill: a held fret
        // burns, a struck one flares white through the bloom pass.
        if let Some(mut surface) = materials.get_mut(&material.0) {
            let colour = theme.0.lane_color(receptor.lane);
            let glow = 0.35f32.mul_add(1.0, 2.4 * press) + 6.0 * hit;
            surface.emissive = colour.to_linear() * glow;
            surface.base_color = colour.mix(&Color::WHITE, 0.6 * hit);
        }
    }
}

/// Lay bar lines across the neck, one per bar of the song.
pub fn spawn_fret_bars(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    song: Res<crate::boot::LoadedSong>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let bpm = song.chart.song.bpm.clamp(20.0, 400.0);
    let bar_s = 240.0 / bpm;
    let start = song.chart.song.offset_s;
    let end = song.chart.song.duration_s.unwrap_or(start + 240.0);
    let mesh = meshes.add(Cuboid::new(1.0, 0.012, 0.045));
    for index in &players {
        let origin = layout.origin(index.0) * WORLD_PER_PIXEL;
        let width = layout.bed_width() * WORLD_PER_PIXEL * 1.18;
        let mut t = start;
        while t < end {
            // Its own material, because each bar fades by its own
            // distance — sharing one handle made every bar in the song
            // pile into a solid white wedge at the horizon.
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.75, 0.78, 0.85, 1.0),
                emissive: LinearRgba::rgb(0.35, 0.36, 0.42),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            commands.spawn((
                GameplayScreen,
                Stage3d,
                FretBar { time_s: t },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                // Parked off-screen until the scroll system places it.
                Transform::from_xyz(origin, 0.012, -900.0).with_scale(Vec3::new(width, 1.0, 1.0)),
                RenderLayers::layer(STAGE_LAYER),
            ));
            t += bar_s;
        }
    }
}

/// Scroll the bar lines with the song, and fade them with distance so
/// a wall of equal-strength lines near the horizon does not read as
/// clutter.
pub fn move_fret_bars(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut bars: Query<(
        &FretBar,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
        &mut Visibility,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    for (bar, mut transform, material, mut visibility) in &mut bars {
        let z = note_z(bar.time_s - now, settings.scroll_speed);
        transform.translation.z = z;
        // Beyond the drawn highway there is nothing to see, and a
        // hundred bars stacked at the vanishing point read as a solid
        // wedge rather than as depth.
        let distance = -z;
        let visible = (-1.0..HIGHWAY_LENGTH).contains(&distance);
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !visible {
            continue;
        }
        if let Some(mut surface) = materials.get_mut(&material.0) {
            // Full strength close by, gone by the far end.
            let fade = (1.0 - (distance / HIGHWAY_LENGTH).clamp(0.0, 1.0)).powf(1.6);
            surface.base_color = Color::srgba(0.75, 0.78, 0.85, fade * 0.85);
            surface.emissive = LinearRgba::rgb(0.35, 0.36, 0.42) * fade;
        }
    }
}

/// Reusable geometry and materials for the 3D notes, built once.
#[derive(Resource)]
pub struct NoteAssets {
    gem: Handle<Mesh>,
    rim: Handle<Mesh>,
    hopo: Handle<Mesh>,
    hopo_rim: Handle<Mesh>,
    sustain: Handle<Mesh>,
    rim_material: Handle<StandardMaterial>,
    lane_material: Vec<Handle<StandardMaterial>>,
}

/// Build the note assets when the stage comes up.
pub fn setup_note_assets(
    mut commands: Commands,
    settings: Res<Settings>,
    theme: Res<crate::theme::ActiveTheme>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !active(&settings) {
        return;
    }
    // The genre's gem is a flat BUTTON lying on the fretboard — a
    // coloured disc inside a dark rim — not a floating sphere. Seen
    // from the player's angle it reads as an ellipse, which is what
    // makes a five-lane row scannable at speed.
    commands.insert_resource(NoteAssets {
        gem: meshes.add(Cylinder::new(GEM_RADIUS, 0.055)),
        rim: meshes.add(Cylinder::new(GEM_RADIUS * 1.28, 0.042)),
        // A HOPO is smaller and reads as a different object, the way
        // the 2D views distinguish it.
        hopo: meshes.add(Cylinder::new(GEM_RADIUS * 0.62, 0.05)),
        hopo_rim: meshes.add(Cylinder::new(GEM_RADIUS * 0.86, 0.04)),
        sustain: meshes.add(Cylinder::new(0.05, 1.0)),
        rim_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.05, 0.07),
            perceptual_roughness: 0.6,
            ..default()
        }),
        lane_material: Lane::ALL
            .iter()
            .map(|lane| {
                let colour = theme.0.lane_color(*lane);
                materials.add(StandardMaterial {
                    base_color: colour,
                    // Emissive is what makes a gem glow through the
                    // bloom pass instead of merely being lit.
                    emissive: colour.to_linear() * 2.2,
                    perceptual_roughness: 0.25,
                    metallic: 0.35,
                    ..default()
                })
            })
            .collect(),
    });
}

/// Spawn 3D geometry for notes coming into view.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn spawn_due_notes(
    mut commands: Commands,
    mut players: Query<(&PlayerIndex, &mut PlayerSession)>,
    layout: Res<HighwayLayout>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
    assets: Option<Res<NoteAssets>>,
) {
    if !active(&settings) {
        return;
    }
    let (Some(now), Some(assets)) = (game_clock.song_time(&time), assets) else {
        return;
    };
    for (index, mut player) in &mut players {
        while player.spawn_cursor < player.session.track().events().len() {
            let cursor = player.spawn_cursor;
            let event = player.session.track().events()[cursor];
            if event.time_s > now + super::SPAWN_LOOKAHEAD_S {
                break;
            }
            let z = note_z(event.time_s - now, settings.scroll_speed);
            for lane in event.lanes.iter() {
                let material = assets.lane_material[lane.index()].clone();
                let hopo = event.kind == beatbyte_core::NoteKind::Hopo;
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Note3d {
                        player: index.0,
                        event_index: cursor,
                        lane,
                    },
                    Mesh3d(if hopo {
                        assets.hopo.clone()
                    } else {
                        assets.gem.clone()
                    }),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(lane_x(&layout, index.0, lane), 0.075, z),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                // The dark rim the coloured face sits in.
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Note3d {
                        player: index.0,
                        event_index: cursor,
                        lane,
                    },
                    Mesh3d(if hopo {
                        assets.hopo_rim.clone()
                    } else {
                        assets.rim.clone()
                    }),
                    MeshMaterial3d(assets.rim_material.clone()),
                    Transform::from_xyz(lane_x(&layout, index.0, lane), 0.06, z),
                    RenderLayers::layer(STAGE_LAYER),
                ));
                // A sustain is a tube running back up the neck from
                // the gem — length is the note's own held time.
                if event.is_sustain() {
                    let length = (event.sustain_s as f32) * settings.scroll_speed * WORLD_PER_PIXEL;
                    commands.spawn((
                        GameplayScreen,
                        Stage3d,
                        Note3d {
                            player: index.0,
                            event_index: cursor,
                            lane,
                        },
                        Mesh3d(assets.sustain.clone()),
                        MeshMaterial3d(material),
                        // The cylinder is built along Y, so it is
                        // rotated onto the neck's Z axis and pushed
                        // back by half its length.
                        Transform::from_xyz(lane_x(&layout, index.0, lane), 0.05, z - length / 2.0)
                            .with_rotation(Quat::from_rotation_x(core::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(1.0, length, 1.0)),
                        RenderLayers::layer(STAGE_LAYER),
                    ));
                }
            }
            player.spawn_cursor += 1;
        }
    }
}

/// Keep the 3D notes in step with the song.
pub fn move_notes(
    mut commands: Commands,
    mut notes: Query<(Entity, &Note3d, &mut Transform)>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    layout: Res<HighwayLayout>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    settings: Res<Settings>,
) {
    if !active(&settings) {
        return;
    }
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let Some((_, reference)) = players.iter().next() else {
        return;
    };
    let events = reference.session.track().events();
    for (entity, note, mut transform) in &mut notes {
        let Some(event) = events.get(note.event_index) else {
            continue;
        };
        let head = note_z(event.time_s - now, settings.scroll_speed);
        // A sustain tube is offset back by half its own length; its
        // scale carries that length, so the offset is recomputed
        // rather than remembered.
        let z = if transform.scale.y > 1.5 {
            head - transform.scale.y / 2.0
        } else {
            head
        };
        transform.translation.x = lane_x(&layout, note.player, note.lane);
        transform.translation.z = z;
        // Past the camera: gone.
        if z > 4.5 {
            commands.entity(entity).despawn();
        }
    }
}

/// The plugin: only its own systems, all gated on the 3D view.
pub struct Stage3dPlugin;

impl Plugin for Stage3dPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Gameplay),
            (setup_stage, setup_note_assets, spawn_fret_bars)
                .chain()
                .after(super::setup_gameplay),
        )
        .add_systems(
            Update,
            (
                spawn_due_notes,
                move_notes,
                move_fret_bars,
                update_receptors,
            )
                .chain()
                .run_if(in_state(crate::states::GamePhase::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hit_line_is_the_origin_and_the_future_is_negative_z() {
        assert!(note_z(0.0, 420.0).abs() < 1e-6);
        assert!(note_z(1.0, 420.0) < 0.0, "the future runs away from you");
        assert!(note_z(-0.2, 420.0) > 0.0, "the past runs past the camera");
    }

    #[test]
    fn scroll_speed_still_means_what_it_meant() {
        // Doubling the setting must double the distance a note covers
        // in a second, exactly as in the 2D views.
        let slow = note_z(1.0, 300.0).abs();
        let fast = note_z(1.0, 600.0).abs();
        assert!((fast - slow * 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_second_of_lead_time_fits_on_the_drawn_highway() {
        // At the default scroll speed the player must be able to SEE
        // roughly a second and a half ahead, or notes appear from
        // nowhere.
        let distance = note_z(1.5, 420.0).abs();
        assert!(
            distance < HIGHWAY_LENGTH,
            "1.5 s of lead time ({distance}) runs off a {HIGHWAY_LENGTH}-unit highway"
        );
    }
}
