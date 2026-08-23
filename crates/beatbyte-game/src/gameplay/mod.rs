//! The gameplay screen: highway, notes, judgment, HUD.
//!
//! Layer discipline: every gameplay *rule* lives in
//! [`beatbyte_core::TrackSession`]. This module feeds it inputs with
//! song-clock timestamps, advances it once per frame, and renders what
//! it reports. Rendering derives note positions from the song timeline
//! every frame — nothing here integrates positions incrementally.
//!
//! Players are entities: each carries its own [`PlayerSession`]
//! component. Single-player spawns one; local multiplayer (Milestone 9)
//! spawns more without touching these systems' logic.

pub mod feedback;
pub mod hud;
pub mod input;
pub mod notes;

use beatbyte_core::{PlayerPerformance, ScoreConfig, SessionEvent, TimingWindows, TrackSession};
use bevy::prelude::*;

use crate::audio_sys::{GameClock, Music};
use crate::boot::LoadedSong;
use crate::menu::SelectedDifficulty;
use crate::palette;
use crate::states::{AppState, GamePhase};

/// Horizontal center-to-center lane spacing in pixels.
pub const LANE_STEP: f32 = 76.0;

/// Y position of the receptor row (notes are judged here).
pub const RECEPTOR_Y: f32 = -240.0;

/// Pixels a note travels per second (base scroll speed).
pub const SCROLL_SPEED: f32 = 420.0;

/// Notes spawn when they are this many seconds away.
pub const SPAWN_LOOKAHEAD_S: f64 = 2.6;

/// X position of a lane's center.
#[must_use]
pub fn lane_x(lane: beatbyte_core::Lane) -> f32 {
    (lane.index() as f32 - 2.0) * LANE_STEP
}

/// The y position of a note event's head at the given song time.
#[must_use]
pub fn note_y(event_time_s: f64, song_time_s: f64) -> f32 {
    RECEPTOR_Y + ((event_time_s - song_time_s) as f32) * SCROLL_SPEED
}

/// One player's live gameplay state. A component, not a resource:
/// multiplayer is more entities, not more code paths.
#[derive(Component)]
pub struct PlayerSession {
    /// The deterministic judgment engine.
    pub session: TrackSession,
    /// Session events produced this frame (input + time advance).
    pub frame_events: Vec<SessionEvent>,
}

/// The last finished run, for the results screen.
#[derive(Resource, Clone)]
pub struct LastResults {
    /// The final performance.
    pub performance: PlayerPerformance,
    /// The song title played.
    pub title: String,
    /// The difficulty played.
    pub difficulty: beatbyte_core::Difficulty,
}

/// Marker for everything belonging to the gameplay screen.
#[derive(Component)]
pub struct GameplayScreen;

/// The gameplay plugin.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Gameplay),
            (setup_gameplay, notes::spawn_highway, hud::spawn_hud),
        )
        .add_systems(
            Update,
            (
                input::gameplay_input,
                advance_sessions,
                notes::spawn_due_notes,
                notes::move_notes,
                notes::update_receptors,
                notes::apply_note_events,
                feedback::spawn_feedback,
                feedback::animate_feedback,
                hud::update_hud,
                check_song_end,
            )
                .chain()
                .run_if(in_state(GamePhase::Playing)),
        )
        .add_systems(Update, pause_input.run_if(in_state(AppState::Gameplay)))
        .add_systems(
            OnEnter(GamePhase::Paused),
            (pause_audio, spawn_pause_overlay),
        )
        .add_systems(
            OnExit(GamePhase::Paused),
            (resume_audio, despawn_pause_overlay),
        )
        .add_systems(OnExit(AppState::Gameplay), teardown_gameplay);
    }
}

fn setup_gameplay(
    mut commands: Commands,
    song: Res<LoadedSong>,
    selected: Res<SelectedDifficulty>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let track = match song.chart.to_track(selected.0) {
        Ok(track) => track,
        Err(error) => {
            error!("cannot build track for {}: {error}", selected.0);
            next_state.set(AppState::MainMenu);
            return;
        }
    };
    info!(
        "starting \"{}\" on {} — {} note events",
        song.chart.song.title,
        selected.0,
        track.len()
    );
    commands.spawn((
        GameplayScreen,
        PlayerSession {
            session: TrackSession::new(track, TimingWindows::default(), ScoreConfig::default()),
            frame_events: Vec::new(),
        },
    ));

    music.0.play_buffer(song.audio.clone());
    game_clock.clock.start(time.elapsed_secs_f64(), 0.0);
}

/// Advance every player's judgment engine to the current song time.
pub(crate) fn advance_sessions(
    mut players: Query<&mut PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    for mut player in &mut players {
        let player = &mut *player;
        player.session.advance(now, &mut player.frame_events);
    }
}

/// End of song → snapshot results → results screen.
fn check_song_end(
    mut commands: Commands,
    players: Query<&PlayerSession>,
    song: Res<LoadedSong>,
    selected: Res<SelectedDifficulty>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let all_finished =
        !players.is_empty() && players.iter().all(|player| player.session.finished());
    let content_end = players
        .iter()
        .map(|player| player.session.track().content_end_s())
        .fold(0.0, f64::max);
    if all_finished && now > content_end + 1.5 {
        if let Some(player) = players.iter().next() {
            commands.insert_resource(LastResults {
                performance: player.session.performance().clone(),
                title: song.chart.song.title.clone(),
                difficulty: selected.0,
            });
        }
        next_state.set(AppState::Results);
    }
}

fn pause_input(
    keys: Res<ButtonInput<KeyCode>>,
    phase: Res<State<GamePhase>>,
    mut next_phase: ResMut<NextState<GamePhase>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    match phase.get() {
        GamePhase::Playing => {
            if keys.just_pressed(KeyCode::Escape) {
                next_phase.set(GamePhase::Paused);
            }
        }
        GamePhase::Paused => {
            if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Enter) {
                next_phase.set(GamePhase::Playing);
            }
            if keys.just_pressed(KeyCode::KeyQ) {
                next_state.set(AppState::MainMenu);
            }
        }
    }
}

fn pause_audio(music: Res<Music>, mut game_clock: ResMut<GameClock>, time: Res<Time>) {
    music.0.pause();
    game_clock.clock.pause(time.elapsed_secs_f64());
}

fn resume_audio(music: Res<Music>, mut game_clock: ResMut<GameClock>, time: Res<Time>) {
    music.0.resume();
    game_clock.clock.resume(time.elapsed_secs_f64());
}

/// Marker for the pause overlay.
#[derive(Component)]
struct PauseOverlay;

fn spawn_pause_overlay(mut commands: Commands) {
    commands
        .spawn((
            PauseOverlay,
            GameplayScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(14),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.75)),
            GlobalZIndex(10),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("PAUSED"),
                TextFont {
                    font_size: FontSize::Px(64.0),
                    ..default()
                },
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new("ESC/ENTER resume   |   Q quit"),
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
        });
}

fn despawn_pause_overlay(mut commands: Commands, overlays: Query<Entity, With<PauseOverlay>>) {
    for entity in &overlays {
        commands.entity(entity).despawn();
    }
}

fn teardown_gameplay(
    mut commands: Commands,
    entities: Query<Entity, With<GameplayScreen>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    music.0.stop();
    game_clock.clock.stop();
}
