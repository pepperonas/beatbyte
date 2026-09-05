//! The in-game chart editor: a thin presentation over
//! [`beatbyte_editor::EditorSession`].
//!
//! Open a file-based song from the browser with `E`. The view is the
//! familiar vertical highway, frozen around an edit cursor on the
//! beat grid; audio preview plays from the cursor.
//!
//! Keys: UP/DOWN step the grid (PGUP/PGDN by bar), LEFT/RIGHT pick
//! the lane, SPACE toggles a note, M moves one (grab, navigate,
//! place — ESC cancels), H toggles HOPO, TAB cycles the
//! grid division, P previews, U/R undo/redo, S saves (validated),
//! ESC leaves (twice if unsaved).

use beatbyte_chart::ChartNote;
use beatbyte_core::{Lane, TempoMap};
use beatbyte_editor::{EditOp, EditorSession};
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::audio_sys::{GameClock, Music};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// Everything the editor screen needs.
#[derive(Resource)]
pub struct EditorState {
    /// The edit session (chart + undo/redo).
    pub session: EditorSession,
    /// Where the chart is saved.
    pub chart_path: std::path::PathBuf,
    /// The song's audio for preview.
    pub audio_path: std::path::PathBuf,
    /// Cursor position on the song timeline.
    pub cursor_s: f64,
    /// Selected lane.
    pub lane: Lane,
    /// Grid division (1 = beats, 2 = eighths, 4 = sixteenths).
    pub division: u32,
    /// Whether audio preview is running.
    pub previewing: bool,
    /// Seconds since an unsaved-exit warning was shown.
    pub exit_armed: f32,
    /// A transient status line ("saved", validation errors, …).
    pub status: String,
    /// A note picked up with M, waiting to be placed (time, lane).
    pub grabbed: Option<(f64, u8)>,
    /// Time-range selection anchor set with V (cursor is the other
    /// end); bulk ops act on every note between them, all lanes.
    pub select_anchor: Option<f64>,
    /// The view needs a rebuild.
    pub dirty_view: bool,
}

impl EditorState {
    fn tempo(&self) -> TempoMap {
        TempoMap::constant(
            self.session.chart().song.bpm,
            self.session.chart().song.offset_s,
        )
    }

    /// Snap a time to the current grid.
    fn snap(&self, time_s: f64) -> f64 {
        let tempo = self.tempo();
        let step = 1.0 / f64::from(self.division);
        let beats = (tempo.beats_at(time_s) / step).round() * step;
        tempo.time_at_beats(beats).max(0.0)
    }

    /// Move the cursor by whole grid steps.
    fn step(&mut self, steps: f64) {
        let tempo = self.tempo();
        let step = 1.0 / f64::from(self.division);
        let beats = tempo.beats_at(self.cursor_s) + steps * step;
        self.cursor_s = tempo.time_at_beats(beats).max(0.0);
    }
}

/// Open the editor for a file-based chart at a difficulty.
pub fn open_editor(
    commands: &mut Commands,
    chart_path: &std::path::Path,
    audio_path: &std::path::Path,
    difficulty: beatbyte_core::Difficulty,
) -> Result<(), String> {
    let chart = beatbyte_chart::load_chart_file(chart_path).map_err(|error| error.to_string())?;
    let session = EditorSession::new(chart, difficulty).map_err(|error| error.to_string())?;
    commands.insert_resource(EditorState {
        session,
        chart_path: chart_path.to_path_buf(),
        audio_path: audio_path.to_path_buf(),
        cursor_s: 0.0,
        lane: Lane::Three,
        division: 2,
        previewing: false,
        exit_armed: 0.0,
        status: String::new(),
        grabbed: None,
        select_anchor: None,
        dirty_view: true,
    });
    Ok(())
}

/// Pixels per second in the editor view.
const EDITOR_SCROLL: f32 = 300.0;

/// Y of the cursor line.
const CURSOR_Y: f32 = -140.0;

/// The editor plugin.
pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuditionClicks>()
            .add_systems(OnEnter(AppState::Editor), spawn_editor)
            .add_systems(
                Update,
                (
                    editor_input,
                    follow_preview,
                    preview_clicks,
                    redraw_notes,
                    refresh_hud,
                )
                    .chain()
                    .run_if(in_state(AppState::Editor)),
            )
            .add_systems(OnExit(AppState::Editor), teardown_editor);
    }
}

/// Marker for editor screen entities.
#[derive(Component)]
struct EditorScreen;

/// Marker for the rebuilt-note layer.
#[derive(Component)]
struct EditorNote;

/// Marker for the lane highlight.
#[derive(Component)]
struct LaneHighlight;

/// The HUD text block.
#[derive(Component)]
struct EditorHud;

fn spawn_editor(mut commands: Commands, state: Option<ResMut<EditorState>>, font: Res<UiFont>) {
    let Some(mut state) = state else {
        return;
    };
    state.dirty_view = true;
    state.previewing = false;
    state.status = "editing".to_owned();

    // Static stage: bed, lane guides, cursor line, lane highlight.
    commands.spawn((
        EditorScreen,
        Sprite::from_color(palette::SURFACE, Vec2::new(76.0 * 5.0 + 24.0, 900.0)),
        Transform::from_xyz(0.0, 0.0, -10.0),
    ));
    for lane in Lane::ALL {
        commands.spawn((
            EditorScreen,
            Sprite::from_color(
                palette::dimmed(palette::lane_color(lane), 0.08),
                Vec2::new(2.0, 900.0),
            ),
            Transform::from_xyz(lane_x(lane), 0.0, -9.0),
        ));
    }
    commands.spawn((
        EditorScreen,
        Sprite::from_color(
            palette::BRAND.with_alpha(0.85),
            Vec2::new(76.0 * 5.0 + 24.0, 3.0),
        ),
        Transform::from_xyz(0.0, CURSOR_Y, -2.0),
    ));
    commands.spawn((
        EditorScreen,
        LaneHighlight,
        Sprite::from_color(Color::WHITE.with_alpha(0.06), Vec2::new(72.0, 900.0)),
        Transform::from_xyz(lane_x(state.lane), 0.0, -8.0),
    ));
    commands.spawn((
        EditorScreen,
        EditorHud,
        Text2d::new(""),
        font.text(9.0),
        TextColor(palette::TEXT),
        Anchor::TOP_LEFT,
        Transform::from_xyz(-620.0, 340.0, 5.0),
    ));
}

fn lane_x(lane: Lane) -> f32 {
    (lane.index() as f32 - 2.0) * 76.0
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Option<ResMut<EditorState>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    mut highlight: Query<&mut Transform, With<LaneHighlight>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some(mut state) = state else {
        return;
    };
    state.exit_armed = (state.exit_armed - time.delta_secs()).max(0.0);

    // Navigation.
    if keys.just_pressed(KeyCode::ArrowUp) {
        state.step(1.0);
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.step(-1.0);
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        let bar = 4.0 * f64::from(state.division);
        state.step(bar);
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::PageDown) {
        let bar = 4.0 * f64::from(state.division);
        state.step(-bar);
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        let index = state.lane.index();
        if index > 0
            && let Some(lane) = Lane::from_index(index - 1)
        {
            state.lane = lane;
        }
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        let index = state.lane.index();
        if let Some(lane) = Lane::from_index(index + 1) {
            state.lane = lane;
        }
    }
    if let Ok(mut transform) = highlight.single_mut() {
        transform.translation.x = lane_x(state.lane);
    }
    if keys.just_pressed(KeyCode::Tab) {
        state.division = match state.division {
            1 => 2,
            2 => 4,
            _ => 1,
        };
        state.cursor_s = state.snap(state.cursor_s);
        state.dirty_view = true;
    }

    // Editing.
    if keys.just_pressed(KeyCode::Space) {
        let time_s = state.snap(state.cursor_s);
        let lane = state.lane.index() as u8;
        let difficulty = state.session.difficulty;
        let existing = state.session.chart().chart_for(difficulty).and_then(|def| {
            def.notes.iter().copied().find(|note| {
                note.lane == lane && (note.time - time_s).abs() <= beatbyte_editor::EDIT_EPSILON_S
            })
        });
        let op = match existing {
            Some(note) => EditOp::RemoveNote { difficulty, note },
            None => EditOp::AddNote {
                difficulty,
                note: ChartNote {
                    time: time_s,
                    lane,
                    len: 0.0,
                    hopo: false,
                },
            },
        };
        state.status = match state.session.edit(op) {
            Ok(()) => "edited".to_owned(),
            Err(error) => error.to_string(),
        };
        state.dirty_view = true;
    }
    // V: set/clear the selection anchor at the cursor.
    if keys.just_pressed(KeyCode::KeyV) {
        state.status = if state.select_anchor.take().is_some() {
            "selection cleared".to_owned()
        } else {
            state.select_anchor = Some(state.snap(state.cursor_s));
            "selecting - move, then X deletes / H toggles hopo".to_owned()
        };
    }
    // X: delete every note in the selected range (all lanes), as one
    // undo step.
    if keys.just_pressed(KeyCode::KeyX)
        && let Some(anchor) = state.select_anchor
    {
        let difficulty = state.session.difficulty;
        let ops = selection_ops(&state, anchor, |note| EditOp::RemoveNote {
            difficulty,
            note,
        });
        let count = ops.len();
        state.status = match state.session.edit_batch(ops) {
            Ok(()) if count == 0 => "selection is empty".to_owned(),
            Ok(()) => format!("{count} note(s) deleted"),
            Err(error) => error.to_string(),
        };
        state.select_anchor = None;
        state.dirty_view = true;
    }

    // M: grab the note under the cursor, or place a grabbed one.
    if keys.just_pressed(KeyCode::KeyM) {
        let time_s = state.snap(state.cursor_s);
        let lane = state.lane.index() as u8;
        let difficulty = state.session.difficulty;
        if let Some((from_time, from_lane)) = state.grabbed {
            let op = EditOp::MoveNote {
                difficulty,
                from_time,
                from_lane,
                to_time: time_s,
                to_lane: lane,
            };
            state.status = match state.session.edit(op) {
                Ok(()) => {
                    state.grabbed = None;
                    "note moved".to_owned()
                }
                Err(error) => format!("cannot place: {error}"),
            };
            state.dirty_view = true;
        } else {
            let existing = state.session.chart().chart_for(difficulty).and_then(|def| {
                def.notes.iter().copied().find(|note| {
                    note.lane == lane
                        && (note.time - time_s).abs() <= beatbyte_editor::EDIT_EPSILON_S
                })
            });
            state.status = match existing {
                Some(note) => {
                    state.grabbed = Some((note.time, note.lane));
                    "moving - M places, ESC cancels".to_owned()
                }
                None => "no note here to move".to_owned(),
            };
        }
    }

    if keys.just_pressed(KeyCode::KeyH) {
        let difficulty = state.session.difficulty;
        if let Some(anchor) = state.select_anchor {
            let ops = selection_ops(&state, anchor, |note| EditOp::ToggleHopo {
                difficulty,
                time: note.time,
                lane: note.lane,
            });
            let count = ops.len();
            state.status = match state.session.edit_batch(ops) {
                Ok(()) if count == 0 => "selection is empty".to_owned(),
                Ok(()) => format!("hopo toggled on {count} note(s)"),
                Err(error) => error.to_string(),
            };
            state.select_anchor = None;
        } else {
            let op = EditOp::ToggleHopo {
                difficulty,
                time: state.snap(state.cursor_s),
                lane: state.lane.index() as u8,
            };
            state.status = match state.session.edit(op) {
                Ok(()) => "hopo toggled".to_owned(),
                Err(error) => error.to_string(),
            };
        }
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::KeyU) {
        state.status = if state.session.undo() {
            "undone".to_owned()
        } else {
            "nothing to undo".to_owned()
        };
        state.dirty_view = true;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        state.status = if state.session.redo() {
            "redone".to_owned()
        } else {
            "nothing to redo".to_owned()
        };
        state.dirty_view = true;
    }

    // Preview.
    if keys.just_pressed(KeyCode::KeyP) {
        if state.previewing {
            music.0.stop();
            game_clock.clock.stop();
            state.previewing = false;
            state.cursor_s = state.snap(state.cursor_s);
            state.dirty_view = true;
        } else {
            music.0.play_file(state.audio_path.clone());
            game_clock.expect_song = true;
            music.0.seek_s(state.cursor_s);
            game_clock.begin(time.elapsed_secs_f64(), state.cursor_s);
            state.previewing = true;
        }
    }

    // Save.
    if keys.just_pressed(KeyCode::KeyS) {
        if state.session.is_valid() {
            match beatbyte_chart::save_chart_file(&state.chart_path, state.session.chart()) {
                Ok(()) => {
                    state.session.mark_saved();
                    state.status = format!("saved to {}", state.chart_path.display());
                }
                Err(error) => state.status = format!("save failed: {error}"),
            }
        } else {
            state.status = "chart has validation errors - not saved".to_owned();
        }
    }

    // Exit (twice if dirty). ESC first cancels a pending move.
    if keys.just_pressed(KeyCode::Escape) {
        if state.grabbed.take().is_some() {
            state.status = "move cancelled".to_owned();
        } else if state.select_anchor.take().is_some() {
            state.status = "selection cleared".to_owned();
        } else if state.session.dirty() && state.exit_armed <= 0.0 {
            state.exit_armed = 2.0;
            state.status = "unsaved changes! ESC again to discard".to_owned();
        } else {
            next_state.set(AppState::SongSelect);
        }
    }
}

/// While previewing, the cursor follows the music.
fn follow_preview(state: Option<ResMut<EditorState>>, game_clock: Res<GameClock>, time: Res<Time>) {
    let Some(mut state) = state else {
        return;
    };
    if state.previewing
        && let Some(now) = game_clock.song_time(&time)
    {
        state.cursor_s = now;
        state.dirty_view = true;
    }
}

/// Rebuild the visible note window when anything changed.
fn redraw_notes(
    mut commands: Commands,
    state: Option<ResMut<EditorState>>,
    old: Query<Entity, With<EditorNote>>,
) {
    let Some(mut state) = state else {
        return;
    };
    if !state.dirty_view {
        return;
    }
    state.dirty_view = false;
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let Some(def) = state.session.chart().chart_for(state.session.difficulty) else {
        return;
    };
    let window_s = 900.0 / EDITOR_SCROLL as f64;
    for note in &def.notes {
        let dt = note.time - state.cursor_s;
        if dt < -window_s * 0.4 || dt > window_s {
            continue;
        }
        let Some(lane) = Lane::from_index(note.lane as usize) else {
            continue;
        };
        let y = CURSOR_Y + (dt as f32) * EDITOR_SCROLL;
        let color = palette::lane_color(lane);
        let entity = commands
            .spawn((
                EditorScreen,
                EditorNote,
                Sprite::from_color(color, Vec2::splat(30.0)),
                Transform::from_xyz(lane_x(lane), y, 0.0),
            ))
            .id();
        if note.len > 0.0 {
            let tail = note.len as f32 * EDITOR_SCROLL;
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(color.with_alpha(0.35), Vec2::new(10.0, tail)),
                    Transform::from_xyz(0.0, tail / 2.0, -1.0),
                ));
            });
        }
        if note.hopo {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Sprite::from_color(Color::WHITE, Vec2::splat(12.0)),
                    Transform::from_xyz(0.0, 0.0, 1.0),
                ));
            });
        }
    }
}

/// The information block.
fn refresh_hud(state: Option<Res<EditorState>>, mut hud: Query<&mut Text2d, With<EditorHud>>) {
    let (Some(state), Ok(mut text)) = (state, hud.single_mut()) else {
        return;
    };
    let tempo = state.tempo();
    let beats = tempo.beats_at(state.cursor_s).max(0.0);
    let bar = (beats / 4.0).floor() as i64 + 1;
    let beat_in_bar = (beats % 4.0) + 1.0;
    let notes = state
        .session
        .chart()
        .chart_for(state.session.difficulty)
        .map_or(0, |def| def.notes.len());
    let line = format!(
        "EDIT  {} [{}]\nbar {bar} beat {beat_in_bar:.2}  grid 1/{}\nnotes {notes}  undo {}  {}{}\n\nUP/DOWN step  PGUP/PGDN bar\nLEFT/RIGHT lane  SPACE note\nH hopo  M move  V sel  X del-sel
TAB grid  P preview\nU undo  R redo  S save  ESC exit\n\n{}",
        state.session.chart().song.title,
        state.session.difficulty,
        state.division,
        state.session.undo_depth(),
        if state.session.dirty() {
            "UNSAVED"
        } else {
            "saved"
        },
        if state.session.is_valid() {
            ""
        } else {
            "  INVALID"
        },
        state.status,
    );
    if text.0 != line {
        text.0 = line;
    }
}

fn teardown_editor(
    mut commands: Commands,
    entities: Query<Entity, With<EditorScreen>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    music.0.stop();
    game_clock.clock.stop();
}

/// Ticks played by the audition metronome this session — the editor
/// harness asserts the overlay actually fires.
#[derive(Resource, Default)]
pub struct AuditionClicks(pub u32);

/// Metronome overlay while auditioning: one tick per beat, so grid
/// alignment is audible against the actual music (the point of the
/// correction pass). Runs only while previewing.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn preview_clicks(
    mut commands: Commands,
    state: Option<Res<EditorState>>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    sfx: Res<crate::sfx::SfxLib>,
    settings: Res<crate::config::Settings>,
    mut last_beat: Local<Option<i64>>,
    mut clicks: ResMut<AuditionClicks>,
) {
    let Some(state) = state else {
        return;
    };
    if !state.previewing {
        *last_beat = None;
        return;
    }
    let Some(position) = game_clock.song_time(&time) else {
        return;
    };
    let beat = state.tempo().beats_at(position).floor() as i64;
    if *last_beat != Some(beat) {
        // Skip the very first sample so scrubbing does not tick
        // retroactively for the beat the cursor sits inside.
        if last_beat.is_some() && beat >= 0 {
            clicks.0 += 1;
            commands.spawn((
                AudioPlayer::new(sfx.click.clone()),
                bevy::audio::PlaybackSettings::DESPAWN
                    .with_volume(bevy::audio::Volume::Linear(settings.sfx_volume)),
            ));
        }
        *last_beat = Some(beat);
    }
}

/// The notes between the anchor and the cursor (inclusive, all
/// lanes), mapped to ops — the shared plumbing of every bulk edit.
fn selection_ops(
    state: &EditorState,
    anchor: f64,
    to_op: impl Fn(beatbyte_chart::ChartNote) -> EditOp,
) -> Vec<EditOp> {
    let cursor = state.snap(state.cursor_s);
    let (lo, hi) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    let epsilon = beatbyte_editor::EDIT_EPSILON_S;
    state
        .session
        .chart()
        .chart_for(state.session.difficulty)
        .map(|def| {
            def.notes
                .iter()
                .copied()
                .filter(|note| note.time >= lo - epsilon && note.time <= hi + epsilon)
                .map(to_op)
                .collect()
        })
        .unwrap_or_default()
}
