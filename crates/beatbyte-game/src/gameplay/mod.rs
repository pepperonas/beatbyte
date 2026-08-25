//! The gameplay screen: highways, notes, judgment, HUD.
//!
//! Layer discipline: every gameplay *rule* lives in
//! [`beatbyte_core::TrackSession`]. This module feeds it inputs with
//! song-clock timestamps, advances it once per frame, and renders what
//! it reports. Rendering derives note positions from the song timeline
//! every frame — nothing here integrates positions incrementally.
//!
//! Players are entities: each carries its own [`PlayerSession`],
//! [`PlayerIndex`] and [`PlayerDevice`]. Local multiplayer spawns more
//! of them; every system below iterates players instead of assuming
//! one.

pub mod feedback;
pub mod fx;
pub mod hud;
pub mod input;
pub mod notes;

use beatbyte_core::{
    Lane, PlayerPerformance, ScoreConfig, SessionEvent, TimingWindows, TrackSession,
};
use bevy::prelude::*;

use crate::audio_sys::{GameClock, Music};
use crate::boot::{LoadedSong, SongAudio};
use crate::multiplayer::{DeviceId, MultiplayerMode, PLAYER_COLORS, PlayerRoster};
use crate::palette;
use crate::song_select::SelectedDifficulty;
use crate::states::{AppState, GamePhase};

/// Y position of the receptor row (notes are judged here).
pub const RECEPTOR_Y: f32 = -240.0;

/// Notes spawn when they are this many seconds away.
pub const SPAWN_LOOKAHEAD_S: f64 = 2.6;

/// Count-in before the music starts: the clock runs negative while
/// the first notes scroll in, so songs never open with a wall.
pub const PREROLL_S: f64 = 2.0;

/// Where each player's highway sits and how big everything is.
#[derive(Resource, Debug, Clone)]
pub struct HighwayLayout {
    origins: Vec<f32>,
    /// Center-to-center lane spacing in pixels.
    pub lane_step: f32,
}

impl HighwayLayout {
    /// Layout for a player count (1–4).
    #[must_use]
    pub fn for_players(count: usize) -> HighwayLayout {
        let (origins, lane_step) = match count.max(1) {
            1 => (vec![0.0], 76.0),
            2 => (vec![-330.0, 330.0], 64.0),
            3 => (vec![-420.0, 0.0, 420.0], 48.0),
            _ => (vec![-465.0, -155.0, 155.0, 465.0], 40.0),
        };
        HighwayLayout { origins, lane_step }
    }

    /// Number of players in this layout.
    #[must_use]
    pub fn players(&self) -> usize {
        self.origins.len()
    }

    /// The x center of a player's highway.
    #[must_use]
    pub fn origin(&self, player: usize) -> f32 {
        self.origins.get(player).copied().unwrap_or(0.0)
    }

    /// The x position of a lane's center for a player.
    #[must_use]
    pub fn lane_x(&self, player: usize, lane: Lane) -> f32 {
        self.origin(player) + (lane.index() as f32 - 2.0) * self.lane_step
    }

    /// Width of one highway bed.
    #[must_use]
    pub fn bed_width(&self) -> f32 {
        self.lane_step * 5.0 + 24.0
    }

    /// Side length of a note sprite.
    #[must_use]
    pub fn note_size(&self) -> f32 {
        self.lane_step * 0.45
    }

    /// Side length of a receptor.
    #[must_use]
    pub fn receptor_size(&self) -> f32 {
        self.lane_step * 0.58
    }
}

/// Which player an entity belongs to (0-based).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerIndex(pub usize);

/// The device driving a player.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerDevice(pub DeviceId);

/// One player's live gameplay state.
#[derive(Component)]
pub struct PlayerSession {
    /// The deterministic judgment engine.
    pub session: TrackSession,
    /// Session events produced this frame (input + time advance);
    /// drained into [`SessionFeedback`] messages once per frame.
    pub frame_events: Vec<SessionEvent>,
    /// How far note spawning has progressed for this player.
    pub spawn_cursor: usize,
}

/// A session event broadcast to every presentation consumer (note
/// visuals, particles, sounds, popups) — written once per frame by
/// the feedback drain, read via `MessageReader<SessionFeedback>`.
#[derive(Message, Debug, Clone, Copy)]
pub struct SessionFeedback {
    /// The player entity the event belongs to.
    pub player: Entity,
    /// The player's index (avoids a lookup in every consumer).
    pub player_index: usize,
    /// What happened.
    pub event: SessionEvent,
}

/// Publish each player's buffered session events as messages.
fn drain_feedback(
    mut players: Query<(Entity, &PlayerIndex, &mut PlayerSession)>,
    mut writer: MessageWriter<SessionFeedback>,
) {
    for (entity, index, mut player) in &mut players {
        for event in player.frame_events.drain(..) {
            writer.write(SessionFeedback {
                player: entity,
                player_index: index.0,
                event,
            });
        }
    }
}

/// One player's final result.
#[derive(Debug, Clone)]
pub struct PlayerResult {
    /// The player index (0-based).
    pub index: usize,
    /// The final performance.
    pub performance: PlayerPerformance,
}

/// The last finished run, for the results screen.
#[derive(Resource, Clone)]
pub struct LastResults {
    /// The song title played.
    pub title: String,
    /// The artist.
    pub artist: String,
    /// The difficulty played.
    pub difficulty: beatbyte_core::Difficulty,
    /// The mode (relevant for more than one player).
    pub mode: MultiplayerMode,
    /// Every player's outcome, in player order.
    pub players: Vec<PlayerResult>,
    /// Whether tap mode (no-strum assist) was active — such runs stay
    /// out of the scoreboard.
    pub tap_mode: bool,
}

/// Marker for everything belonging to the gameplay screen.
#[derive(Component)]
pub struct GameplayScreen;

/// The song audio waiting for the count-in to elapse.
#[derive(Resource)]
struct PendingMusic(SongAudio);

/// Marker for the count-in banner.
#[derive(Component)]
struct CountIn;

/// The gameplay plugin.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SessionFeedback>()
            .add_plugins(fx::FxPlugin)
            .add_systems(
                OnEnter(AppState::Gameplay),
                (
                    setup_gameplay,
                    notes::spawn_highways,
                    hud::spawn_huds,
                    fx::spawn_fx_scenery,
                    crate::theme::spawn_backdrop,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    run_count_in,
                    input::gameplay_input,
                    advance_sessions,
                    drain_feedback,
                    notes::spawn_due_notes,
                    notes::move_notes,
                    notes::update_receptors,
                    notes::apply_note_events,
                    feedback::spawn_feedback,
                    feedback::animate_feedback,
                    hud::update_huds,
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

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn setup_gameplay(
    mut commands: Commands,
    song: Res<LoadedSong>,
    selected: Res<SelectedDifficulty>,
    roster: Res<PlayerRoster>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    settings: Res<crate::config::Settings>,
    mut theme: ResMut<crate::theme::ActiveTheme>,
    mut clear: ResMut<ClearColor>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    // Stage identity for this song.
    theme.0 = crate::theme::choose_theme(&settings.theme, &song.chart.song.title);
    clear.0 = theme.0.background;

    let track = match song.chart.to_track(selected.0) {
        Ok(track) => track,
        Err(error) => {
            error!("cannot build track for {}: {error}", selected.0);
            next_state.set(AppState::MainMenu);
            return;
        }
    };
    let devices: Vec<DeviceId> = if roster.devices.is_empty() {
        vec![DeviceId::Keyboard]
    } else {
        roster.devices.clone()
    };
    info!(
        "starting \"{}\" on {} — {} note events, {} player(s)",
        song.chart.song.title,
        selected.0,
        track.len(),
        devices.len()
    );
    commands.insert_resource(HighwayLayout::for_players(devices.len()));
    for (index, device) in devices.into_iter().enumerate() {
        let mut session = TrackSession::new(
            track.clone(),
            TimingWindows::default(),
            ScoreConfig::default(),
        );
        session.set_tap_mode(settings.tap_mode);
        commands.spawn((
            GameplayScreen,
            PlayerIndex(index),
            PlayerDevice(device),
            PlayerSession {
                session,
                frame_events: Vec::new(),
                spawn_cursor: 0,
            },
        ));
    }

    // Count-in: the clock starts negative; music starts at zero.
    commands.insert_resource(PendingMusic(song.audio.clone()));
    game_clock.clock.start(time.elapsed_secs_f64(), -PREROLL_S);
}

/// Start the music the moment the count-in ends; run the banner.
fn run_count_in(
    mut commands: Commands,
    pending: Option<Res<PendingMusic>>,
    music: Res<Music>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    font: Res<crate::ui::UiFont>,
    mut banner: Query<(Entity, &mut Text2d), With<CountIn>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    if pending.is_some() && banner.is_empty() {
        commands.spawn((
            GameplayScreen,
            CountIn,
            Text2d::new(""),
            font.text(26.0),
            TextColor(palette::BRAND),
            Transform::from_xyz(0.0, 60.0, 30.0),
        ));
    }
    if let Ok((entity, mut text)) = banner.single_mut() {
        if now < 0.0 {
            let count = format!("{}", (-now).ceil() as i64);
            if text.0 != count {
                text.0 = count;
            }
        } else {
            commands.entity(entity).despawn();
        }
    }
    if now >= 0.0
        && let Some(pending) = pending
    {
        match &pending.0 {
            SongAudio::Memory(audio) => music.0.play_buffer(audio.clone()),
            SongAudio::File(path) => music.0.play_file(path.clone()),
        }
        commands.remove_resource::<PendingMusic>();
    }
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
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn check_song_end(
    mut commands: Commands,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    song: Res<LoadedSong>,
    selected: Res<SelectedDifficulty>,
    roster: Res<PlayerRoster>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let all_finished =
        !players.is_empty() && players.iter().all(|(_, player)| player.session.finished());
    let content_end = players
        .iter()
        .map(|(_, player)| player.session.track().content_end_s())
        .fold(0.0, f64::max);
    if all_finished && now > content_end + 1.5 {
        let mut results: Vec<PlayerResult> = players
            .iter()
            .map(|(index, player)| PlayerResult {
                index: index.0,
                performance: player.session.performance().clone(),
            })
            .collect();
        results.sort_by_key(|result| result.index);
        commands.insert_resource(LastResults {
            title: song.chart.song.title.clone(),
            artist: song.chart.song.artist.clone(),
            difficulty: selected.0,
            mode: roster.mode,
            players: results,
            tap_mode: players.iter().any(|(_, p)| p.session.tap_mode()),
        });
        next_state.set(AppState::Results);
    }
}

fn pause_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    map: Res<crate::controls::InputMap>,
    phase: Res<State<GamePhase>>,
    mut next_phase: ResMut<NextState<GamePhase>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let sources = crate::controls::InputSources {
        keys: &keys,
        pads: pads.iter().collect(),
    };
    let pause = sources.just_pressed(&map, crate::controls::GameAction::Pause);
    match phase.get() {
        GamePhase::Playing => {
            if pause {
                next_phase.set(GamePhase::Paused);
            }
        }
        GamePhase::Paused => {
            if pause || keys.just_pressed(KeyCode::Enter) {
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

fn spawn_pause_overlay(mut commands: Commands, font: Res<crate::ui::UiFont>) {
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
                font.text(40.0),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new("ESC/ENTER resume   |   Q quit"),
                font.text(13.0),
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
    commands.remove_resource::<PendingMusic>();
    music.0.stop();
    game_clock.clock.stop();
}

/// The accent color for a player index.
#[must_use]
pub fn player_color(index: usize) -> Color {
    PLAYER_COLORS[index % PLAYER_COLORS.len()]
}
