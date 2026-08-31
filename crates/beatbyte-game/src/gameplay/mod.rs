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
pub mod stage3d;

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
    /// Whether practice speed was used at any point — such runs stay
    /// out of the scoreboard AND the telemetry (slowed evidence
    /// would poison the design loop).
    pub practice: bool,
}

/// Practice mode (optimization plan P1): the chosen speed and
/// whether this run used it. The percent survives across songs — a
/// practice session repeats — but `used` re-derives per run.
#[derive(Resource)]
pub struct PracticeState {
    /// Playback speed in percent, 50–150.
    pub speed_percent: u32,
    /// Section-loop start in song seconds (practice).
    pub loop_from: Option<f64>,
    /// Section-loop end in song seconds (practice).
    pub loop_to: Option<f64>,
    /// Whether THIS run used practice at any point — speed away from
    /// 100 %, or a loop bound set. Sticky within the run: dropping
    /// back to 100 % or clearing the loop does not un-practice the
    /// part already practiced.
    pub used: bool,
}

impl Default for PracticeState {
    fn default() -> Self {
        PracticeState {
            speed_percent: 100,
            loop_from: None,
            loop_to: None,
            used: false,
        }
    }
}

/// A loop must be at least this long — a shorter one would wrap
/// faster than the lead-in plays.
const LOOP_MIN_SPAN_S: f64 = 1.0;

/// How far before the loop start the wrap lands: reaction room, and
/// the notes scroll in instead of teleporting onto the receptors.
const LOOP_LEAD_S: f64 = 1.5;

impl PracticeState {
    /// The speed as a rate factor (1.0 = normal).
    #[must_use]
    pub fn rate(&self) -> f64 {
        f64::from(self.speed_percent) / 100.0
    }

    /// Step the speed by `direction` × 5 %, clamped to 50–150.
    /// Marks the run as practice whenever the result is not 100 %.
    pub fn step(&mut self, direction: f32) {
        let step = if direction < 0.0 { -5i64 } else { 5 };
        self.speed_percent = (i64::from(self.speed_percent) + step).clamp(50, 150) as u32;
        if self.speed_percent != 100 {
            self.used = true;
        }
    }

    /// The armed loop, when both bounds are set and the span is
    /// real (end after start by at least one second).
    #[must_use]
    pub fn loop_span(&self) -> Option<(f64, f64)> {
        match (self.loop_from, self.loop_to) {
            (Some(from), Some(to)) if to >= from + LOOP_MIN_SPAN_S => Some((from, to)),
            _ => None,
        }
    }

    /// Set a loop bound to a song time (negative times clamp to the
    /// song start — bounds live in the song, not the count-in).
    /// Setting a bound is a practice act.
    pub fn set_loop_bound(&mut self, end: bool, song_s: f64) {
        let value = Some(song_s.max(0.0));
        if end {
            self.loop_to = value;
        } else {
            self.loop_from = value;
        }
        self.used = true;
    }
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
                    notes::spawn_fret_lines,
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
                    notes::move_fret_lines,
                    notes::animate_sustains,
                    notes::update_receptors,
                    notes::apply_note_events,
                    feedback::spawn_feedback,
                    feedback::coach_strum,
                    feedback::animate_feedback,
                    hud::update_huds,
                    hud::update_song_ribbon,
                    check_song_end,
                )
                    .chain()
                    .run_if(in_state(GamePhase::Playing)),
            )
            .add_systems(Update, pause_input.run_if(in_state(AppState::Gameplay)))
            .add_systems(
                Update,
                practice_loop_wrap.run_if(in_state(GamePhase::Playing)),
            )
            .init_resource::<PauseCursor>()
            .init_resource::<PracticeState>()
            .add_systems(
                Update,
                (pause_menu_input, refresh_pause_menu)
                    .chain()
                    .run_if(in_state(GamePhase::Paused)),
            )
            .add_systems(
                OnEnter(GamePhase::Paused),
                (pause_audio, spawn_pause_overlay),
            )
            .add_systems(
                OnExit(GamePhase::Paused),
                (resume_audio, despawn_pause_overlay, persist_pause_settings),
            )
            .add_systems(
                OnExit(AppState::Gameplay),
                (teardown_gameplay, restore_normal_speed),
            )
            .add_plugins(stage3d::Stage3dPlugin);
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn setup_gameplay(
    mut commands: Commands,
    song: Res<LoadedSong>,
    selected: Res<SelectedDifficulty>,
    roster: Res<PlayerRoster>,
    mut game_clock: ResMut<GameClock>,
    music: Res<Music>,
    mut practice: ResMut<PracticeState>,
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
            info!("gameplay ended: the track could not be built");
            next_state.set(AppState::SongSelect);
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
    // Practice speed applies to the WHOLE timeline — count-in
    // included — so the scroll pace never changes mid-approach. The
    // run counts as practice from the start when it begins slowed.
    practice.used = practice.speed_percent != 100;
    // Loop bounds are positions in ONE song; a stale pair from the
    // previous track would wrap this one at nonsense times.
    practice.loop_from = None;
    practice.loop_to = None;
    game_clock
        .clock
        .set_rate(time.elapsed_secs_f64(), practice.rate());
    music.0.set_speed(practice.rate());
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
    practice: Res<PracticeState>,
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
            practice: practice.used,
        });
        // Every way out of gameplay says so, and says which way.
        // Twice in one day a report of the game "jumping back to the
        // menu" could not be answered, because leaving gameplay was
        // silent: the log showed a song starting, then a song
        // starting again, and nothing in between. A line here turns
        // that into a fact, and its ABSENCE is a fact too - it means
        // the window or the process went, not the state machine.
        info!("gameplay ended: song finished at {now:.1}s (content ends {content_end:.1}s)");
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
            // Enter no longer resumes: it steps the selected settings
            // row, like on the settings screen. ESC stays the resume.
            if pause {
                next_phase.set(GamePhase::Playing);
            }
            if keys.just_pressed(KeyCode::KeyQ) {
                info!("gameplay ended: quit from the pause screen");
                // Like the results exit: back to the browser the
                // song was picked in, not the main menu.
                next_state.set(AppState::SongSelect);
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

/// Marker for the pause overlay (`pub(crate)`: the autopilot's pause
/// drill checks that it actually laid out — the invisible-menu bug
/// shipped because nothing ever asked).
#[derive(Component)]
pub(crate) struct PauseOverlay;

/// One pause-menu row: the practice speed, or one of the settings
/// screen's own rows — the latter reused wholesale, one definition
/// of every step size and clamp, two places that draw it. The
/// subset is deliberate: nothing here may change the JUDGMENT of
/// the run in flight. Latency offset and tap mode stay on the
/// settings screen, where flipping them cannot invalidate a paused
/// song. (Practice speed scales the whole timeline — audio, clock,
/// windows together — so relative judgment is untouched; the run is
/// marked practice and stays out of scoreboard and telemetry.)
#[derive(Clone, Copy, PartialEq, Eq)]
enum PauseItem {
    /// Practice speed, 50–150 % (optimization plan P1).
    Speed,
    /// Section-loop start (practice): RIGHT sets it to the paused
    /// moment, LEFT clears it.
    LoopFrom,
    /// Section-loop end, same handling.
    LoopTo,
    /// A reused settings-screen row.
    Setting(crate::settings_ui::Row),
}

/// A song time as the pause menu prints it (m:ss.t).
fn fmt_song_time(song_s: f64) -> String {
    let clamped = song_s.max(0.0);
    let minutes = (clamped / 60.0).floor() as u64;
    let seconds = clamped - (minutes as f64) * 60.0;
    format!("{minutes}:{seconds:04.1}")
}

impl PauseItem {
    fn label(self) -> &'static str {
        match self {
            PauseItem::Speed => "SPEED (PRACTICE)",
            PauseItem::LoopFrom => "LOOP FROM",
            PauseItem::LoopTo => "LOOP TO",
            PauseItem::Setting(row) => row.label(),
        }
    }

    fn value(self, settings: &crate::config::Settings, practice: &PracticeState) -> String {
        match self {
            PauseItem::Speed => format!("{}%", practice.speed_percent),
            PauseItem::LoopFrom | PauseItem::LoopTo => {
                let bound = if self == PauseItem::LoopFrom {
                    practice.loop_from
                } else {
                    practice.loop_to
                };
                match bound {
                    None => "RIGHT sets here".to_owned(),
                    Some(value) => {
                        // Both set but not a real span: say so, on
                        // the row that closes the pair.
                        if self == PauseItem::LoopTo
                            && practice.loop_from.is_some()
                            && practice.loop_span().is_none()
                        {
                            format!("{} (after FROM!)", fmt_song_time(value))
                        } else {
                            fmt_song_time(value)
                        }
                    }
                }
            }
            PauseItem::Setting(row) => row.value(settings),
        }
    }
}

/// Where the SFX-volume row sits in the pause menu — the autopilot's
/// pause drill navigates by CONTENT, so inserting rows above it can
/// never silently retarget the drill again (it did once: the loop
/// rows moved SFX from index 2 to 4 and the drill adjusted a loop
/// bound instead).
pub(crate) fn sfx_row_position() -> usize {
    PAUSE_ROWS
        .iter()
        .position(|item| *item == PauseItem::Setting(crate::settings_ui::Row::SfxVolume))
        .unwrap_or(0)
}

/// The pause menu's rows, in display order.
const PAUSE_ROWS: [PauseItem; 6] = [
    PauseItem::Speed,
    PauseItem::LoopFrom,
    PauseItem::LoopTo,
    PauseItem::Setting(crate::settings_ui::Row::MusicVolume),
    PauseItem::Setting(crate::settings_ui::Row::SfxVolume),
    PauseItem::Setting(crate::settings_ui::Row::ScrollSpeed),
];

/// Whether adjusting this row previews the MISS sound. The SFX
/// volume IS the volume of the error sounds, and while the music is
/// paused there is nothing else to hear — setting it blind would be
/// guesswork, so every step plays the sound being set.
fn previews_the_miss_sound(item: PauseItem) -> bool {
    item == PauseItem::Setting(crate::settings_ui::Row::SfxVolume)
}

/// Which pause row the cursor sits on.
#[derive(Resource, Default)]
struct PauseCursor(usize);

/// A pause-menu row (index into [`PAUSE_ROWS`]); carries `Button`.
#[derive(Component)]
struct PauseRow(usize);

/// A pause row's static label.
#[derive(Component)]
struct PauseRowLabel(usize);

/// A pause row's value text — the part that changes.
#[derive(Component)]
struct PauseRowValue(usize);

fn spawn_pause_overlay(
    mut commands: Commands,
    font: Res<crate::ui::UiFont>,
    mut cursor: ResMut<PauseCursor>,
) {
    cursor.0 = 0;
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
                font.text(crate::ui_kit::WORDMARK),
                TextColor(palette::BRAND),
            ));
            parent.spawn(crate::ui_kit::panel()).with_children(|panel| {
                for (index, item) in PAUSE_ROWS.iter().enumerate() {
                    panel
                        .spawn((PauseRow(index), Button, crate::ui_kit::row()))
                        .with_children(|entry| {
                            entry.spawn((
                                PauseRowLabel(index),
                                Text::new(item.label()),
                                font.text(crate::ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                crate::ui_kit::label_node(),
                            ));
                            entry.spawn((
                                PauseRowValue(index),
                                Text::new(""),
                                font.text(crate::ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                crate::ui_kit::value_node(),
                            ));
                        });
                }
            });
            parent.spawn((
                Text::new("UP/DOWN choose  LEFT/RIGHT adjust  ESC resume  Q quit"),
                font.text(crate::ui_kit::ROW),
                TextColor(palette::TEXT_DIM),
            ));
        });
}

/// Navigate and adjust the pause rows. Gameplay input is gated to
/// [`GamePhase::Playing`], so the arrows (which double as strum keys)
/// can never reach the session from here.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn pause_menu_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    rows: Query<(&PauseRow, &Interaction), Changed<Interaction>>,
    mut cursor: ResMut<PauseCursor>,
    mut settings: ResMut<crate::config::Settings>,
    mut practice: ResMut<PracticeState>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    sfx: Res<crate::sfx::SfxLib>,
) {
    let nav = crate::controls::MenuNav::read(&keys, pads.iter());
    let count = PAUSE_ROWS.len();
    let mut moved = false;
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
        moved = true;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
        moved = true;
    }
    let pointer = crate::ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        cursor.0 = index;
    }
    let mut wheel_step = 0.0;
    for event in wheel.read() {
        wheel_step += event.y.signum();
    }
    let item = PAUSE_ROWS[cursor.0];
    let mut adjust = |direction: f32,
                      settings: &mut crate::config::Settings,
                      practice: &mut PracticeState| match item {
        PauseItem::Speed => {
            practice.step(direction);
            // Applied live: music (paused, takes effect on resume)
            // and clock together — the timeline has ONE speed.
            music.0.set_speed(practice.rate());
            game_clock
                .clock
                .set_rate(time.elapsed_secs_f64(), practice.rate());
        }
        PauseItem::LoopFrom | PauseItem::LoopTo => {
            let end = item == PauseItem::LoopTo;
            if direction < 0.0 {
                // LEFT clears the bound (and with it the loop).
                if end {
                    practice.loop_to = None;
                } else {
                    practice.loop_from = None;
                }
            } else if let Some(now) = game_clock.clock.song_time(time.elapsed_secs_f64()) {
                practice.set_loop_bound(end, now);
            }
        }
        PauseItem::Setting(row) => row.adjust(settings, direction),
    };
    let mut adjusted = false;
    if nav.left || wheel_step < 0.0 {
        adjust(-1.0, &mut settings, &mut practice);
        adjusted = true;
    }
    if nav.right || nav.confirm || pointer.clicked || wheel_step > 0.0 {
        adjust(1.0, &mut settings, &mut practice);
        adjusted = true;
    }
    if adjusted {
        let preview = if previews_the_miss_sound(item) {
            &sfx.miss
        } else {
            &sfx.ui_move
        };
        crate::sfx::play(&mut commands, preview, settings.sfx_volume);
    } else if moved {
        crate::sfx::play(&mut commands, &sfx.ui_move, settings.sfx_volume);
    }
}

/// Row highlight + live values, exactly the settings screen's dress.
fn refresh_pause_menu(
    settings: Res<crate::config::Settings>,
    practice: Res<PracticeState>,
    cursor: Res<PauseCursor>,
    mut rows: Query<(&PauseRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&PauseRowLabel, &mut TextColor), Without<PauseRowValue>>,
    mut values: Query<(&PauseRowValue, &mut Text, &mut TextColor), Without<PauseRowLabel>>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = crate::ui_kit::row_style(crate::ui_kit::state_for(row.0 == cursor.0, false));
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 =
            crate::ui_kit::row_style(crate::ui_kit::state_for(label.0 == cursor.0, false)).label;
    }
    for (value, mut text, mut color) in &mut values {
        let wanted = PAUSE_ROWS[value.0].value(&settings, &practice);
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 =
            crate::ui_kit::row_style(crate::ui_kit::state_for(value.0 == cursor.0, false)).value;
    }
}

/// Changes made in the pause menu persist like the settings screen's:
/// on leaving — which covers both resume and quit, because the phase
/// sub-state exits with the gameplay state.
fn persist_pause_settings(settings: Res<crate::config::Settings>) {
    crate::config::save_settings(&settings);
}

fn despawn_pause_overlay(mut commands: Commands, overlays: Query<Entity, With<PauseOverlay>>) {
    for entity in &overlays {
        commands.entity(entity).despawn();
    }
}

/// The note entities a loop wrap sweeps away (both views' gems).
type LoopedNoteEntities = Or<(With<notes::NoteSprite>, With<stage3d::Note3d>)>;

/// The section loop (optimization plan P1, second half): reaching
/// the loop end jumps the whole run — music, clock, sessions, note
/// entities — back to a lead-in before the loop start, and the
/// section's notes become judgeable again.
fn practice_loop_wrap(
    mut commands: Commands,
    practice: Res<PracticeState>,
    mut game_clock: ResMut<GameClock>,
    music: Res<Music>,
    time: Res<Time>,
    mut players: Query<&mut PlayerSession>,
    notes: Query<Entity, LoopedNoteEntities>,
) {
    let Some((from, to)) = practice.loop_span() else {
        return;
    };
    let mono = time.elapsed_secs_f64();
    let Some(now) = game_clock.clock.song_time(mono) else {
        return;
    };
    if now < to {
        return;
    }
    let lead = (from - LOOP_LEAD_S).max(0.0);
    info!("practice loop: {now:.2}s -> {lead:.2}s (section {from:.2}-{to:.2})");
    music.0.seek_s(lead);
    game_clock.clock.seek(mono, lead);
    // The seek travels to the music thread asynchronously; until it
    // lands, the device still reports the old position — reconciling
    // against that would snap the clock straight back to the loop
    // end and wrap again, forever.
    game_clock.hold_reconcile_until = mono + 0.25;
    for mut player in &mut players {
        player.session.rewind_to(lead);
        let events = player.session.track().events();
        player.spawn_cursor = events
            .iter()
            .position(|event| event.time_s >= lead)
            .unwrap_or(events.len());
    }
    // Every note entity in flight belongs to the abandoned pass;
    // the spawn systems rebuild the section from the reset cursor.
    for entity in &notes {
        commands.entity(entity).despawn();
    }
}

fn restore_normal_speed(music: Res<Music>, mut game_clock: ResMut<GameClock>, time: Res<Time>) {
    // The menus run at life speed whatever the practice setting; the
    // chosen percent itself survives for the next song.
    music.0.set_speed(1.0);
    game_clock.clock.set_rate(time.elapsed_secs_f64(), 1.0);
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

#[cfg(test)]
mod pause_menu_tests {
    use super::{PAUSE_ROWS, PauseItem, PracticeState, previews_the_miss_sound};
    use crate::settings_ui::Row;

    #[test]
    fn the_pause_menu_offers_the_volumes_and_the_practice_speed() {
        // The commissioned rows: the SFX volume governs the miss and
        // overstrum sounds, and the practice speed is the pause
        // menu's own feature (optimization plan P1) — both must be
        // reachable mid-song.
        assert!(PAUSE_ROWS.contains(&PauseItem::Setting(Row::SfxVolume)));
        assert!(PAUSE_ROWS.contains(&PauseItem::Setting(Row::MusicVolume)));
        assert!(PAUSE_ROWS.contains(&PauseItem::Speed));
    }

    #[test]
    fn the_pause_menu_cannot_change_the_judgment_mid_song() {
        // Latency offset moves every timing window; tap mode changes
        // the strum rules. Either one flipped inside a paused run
        // would judge the second half of the song by different laws
        // than the first — they stay on the settings screen.
        assert!(!PAUSE_ROWS.contains(&PauseItem::Setting(Row::LatencyOffset)));
        assert!(!PAUSE_ROWS.contains(&PauseItem::Setting(Row::TapMode)));
    }

    #[test]
    fn adjusting_the_sfx_row_previews_the_error_sound() {
        // The row sets the volume OF the miss sound; with the music
        // paused there is nothing else to hear, so every step plays
        // the sound being set — and only that row does.
        for item in PAUSE_ROWS {
            assert_eq!(
                previews_the_miss_sound(item),
                item == PauseItem::Setting(Row::SfxVolume)
            );
        }
    }

    #[test]
    fn the_loop_arms_only_on_a_real_span() {
        let mut practice = PracticeState::default();
        assert_eq!(practice.loop_span(), None);
        practice.set_loop_bound(false, 10.0);
        assert_eq!(practice.loop_span(), None, "one bound is no loop");
        assert!(practice.used, "setting a bound is a practice act");
        // An end before (or hugging) the start never arms — wrapping
        // faster than the lead-in plays would be a strobe.
        practice.set_loop_bound(true, 10.5);
        assert_eq!(practice.loop_span(), None);
        practice.set_loop_bound(true, 24.0);
        assert_eq!(practice.loop_span(), Some((10.0, 24.0)));
        // Bounds live in the song: a count-in moment clamps to 0.
        practice.set_loop_bound(false, -1.4);
        assert_eq!(practice.loop_span(), Some((0.0, 24.0)));
    }

    #[test]
    fn song_times_print_as_minutes_and_tenths() {
        assert_eq!(super::fmt_song_time(0.0), "0:00.0");
        assert_eq!(super::fmt_song_time(92.35), "1:32.3");
        assert_eq!(super::fmt_song_time(-3.0), "0:00.0");
    }

    #[test]
    fn practice_speed_clamps_and_stays_sticky() {
        let mut practice = PracticeState::default();
        assert!((practice.rate() - 1.0).abs() < 1e-9);
        assert!(!practice.used);
        // Down to the floor: clamped at 50 %, marked as practice.
        for _ in 0..30 {
            practice.step(-1.0);
        }
        assert_eq!(practice.speed_percent, 50);
        assert!((practice.rate() - 0.5).abs() < 1e-9);
        assert!(practice.used);
        // Back to exactly 100 %: the half already played slowly
        // happened — the run STAYS practice.
        for _ in 0..10 {
            practice.step(1.0);
        }
        assert_eq!(practice.speed_percent, 100);
        assert!(
            practice.used,
            "returning to 100% must not un-practice a run"
        );
        // Up to the ceiling.
        for _ in 0..30 {
            practice.step(1.0);
        }
        assert_eq!(practice.speed_percent, 150);
    }
}
