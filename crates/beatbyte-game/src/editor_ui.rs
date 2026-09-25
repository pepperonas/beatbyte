//! The in-game chart editor: a presentation over
//! [`beatbyte_editor::EditorSession`].
//!
//! Open a file-based song from the browser with `E` (or the EDIT
//! chip). The view is the vertical highway — later is higher — with
//! the song's waveform and the bar and beat lines beside it; the
//! playhead is the edit cursor. Mouse and keyboard are equal: every
//! action has a key, and every key action a pointer path (the chips).
//!
//! The geometry (what the pointer is over, what a drag means) lives in
//! [`beatbyte_editor::view`], where it is tested without an engine;
//! this module only asks it and draws.
//!
//! Saving writes a NEW chart version (`beatbyte_editor::Saver`); the
//! file the editor opened stays as it was.

mod draw;
pub mod pointer;

use beatbyte_chart::ChartNote;
use beatbyte_core::Lane;
use beatbyte_editor::clipboard::{self, Clip};
use beatbyte_editor::inspector::{self, Field};
use beatbyte_editor::playback::{self, LoopRegion};
use beatbyte_editor::timecode::Grid;
use beatbyte_editor::view::{self, Hit, NoteKey, View};
use beatbyte_editor::waveform::Envelope;
use beatbyte_editor::{EditOp, EditorSession, Saved, Saver};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};

use crate::audio_sys::{GameClock, Music};
use crate::states::AppState;

/// The top of the timeline band, world units.
pub(crate) const TOP_Y: f64 = 292.0;
/// The bottom of the timeline band.
pub(crate) const BOTTOM_Y: f64 = -318.0;
/// Where the playhead sits while the view follows it.
pub(crate) const ANCHOR_Y: f64 = -170.0;

/// Snap divisions per beat the grid cycles through.
pub const DIVISIONS: [u32; 8] = [1, 2, 3, 4, 6, 8, 12, 16];

/// A drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drag {
    /// Pressed on empty lane space; a click places a note, a drag
    /// becomes a box selection.
    Pending {
        /// The (unsnapped) time pressed.
        time: f64,
        /// The lane pressed.
        lane: u8,
        /// Where, on screen.
        at: Vec2,
    },
    /// A box selection.
    Box {
        /// First corner's time.
        t0: f64,
        /// First corner's lane.
        l0: u8,
        /// Second corner's time.
        t1: f64,
        /// Second corner's lane.
        l1: u8,
        /// Add to the selection instead of replacing it.
        additive: bool,
    },
    /// Moving the selection by its anchor note.
    Move {
        /// The note that was grabbed.
        anchor: NoteKey,
        /// The pointer's time at the press.
        press_time: f64,
        /// The pointer's lane at the press.
        press_lane: u8,
        /// The move so far (snapped).
        dt: f64,
        /// Lanes moved so far.
        dlane: i32,
    },
    /// Dragging a note's length handle.
    Length {
        /// The note.
        key: NoteKey,
        /// Where the tail would end.
        end: f64,
    },
    /// Scrubbing the playhead on the ruler or waveform.
    Scrub,
    /// Shift-dragging a loop region on the ruler.
    LoopRegion {
        /// Where the drag started.
        from: f64,
        /// Where it is now.
        to: f64,
    },
}

/// Everything the editor screen needs.
#[derive(Resource)]
pub struct EditorState {
    /// The edit session (chart + undo/redo).
    pub session: EditorSession,
    /// The file the chart was opened from — after a save, the version
    /// the save wrote.
    pub chart_path: std::path::PathBuf,
    /// Saves an edit as a new chart version (never an overwrite).
    pub saver: Saver,
    /// Whether anything was saved this session (the library is read
    /// again on the way out, so the browser plays the new version).
    pub saved_any: bool,
    /// The song's audio for preview.
    pub audio_path: std::path::PathBuf,
    /// The playhead / edit cursor on the song timeline.
    pub cursor_s: f64,
    /// Selected lane (keyboard placement).
    pub lane: Lane,
    /// Snap division per beat (see [`DIVISIONS`]).
    pub division: u32,
    /// Whether placements and moves snap to the grid (`Alt` bypasses
    /// it for one action).
    pub snap_on: bool,
    /// Whether audio preview is running.
    pub previewing: bool,
    /// Playback speed (1, 0.75, 0.5 — the pitch drops with it).
    pub speed: f64,
    /// The loop region, once set.
    pub loop_region: Option<LoopRegion>,
    /// Whether playback loops over the region.
    pub looping: bool,
    /// Whether the view follows the playhead while it plays; any
    /// manual scroll turns it off until the next play (or `F`).
    pub follow: bool,
    /// Seconds since an unsaved-exit warning was shown.
    pub exit_armed: f32,
    /// A transient status line ("saved", validation errors, …).
    pub status: String,
    /// A note picked up with M, waiting to be placed (time, lane).
    pub grabbed: Option<(f64, u8)>,
    /// Time-range selection anchor set with V (cursor is the other
    /// end); bulk ops act on every note between them, all lanes.
    pub select_anchor: Option<f64>,
    /// The selected notes.
    pub selection: Vec<NoteKey>,
    /// The vertical mapping between time and screen.
    pub view: View,
    /// The beat grid (bar:beat:tick, snap) from the chart's marks.
    pub grid: Option<Grid>,
    /// The song's envelope, once decoded.
    pub envelope: Option<Envelope>,
    /// The drag in progress.
    pub drag: Option<Drag>,
    /// What the pointer is over.
    pub hover: Option<Hit>,
    /// Whether the key/mouse reference is shown.
    pub help: bool,
    /// What Cmd+C copied.
    pub clip: Clip,
    /// The inspector field being typed into, and the text so far.
    pub field: Option<(Field, String)>,
    /// Set for the frame a field closes, so its Enter / Esc is not
    /// read again as play / leave.
    pub typing_ate_keys: bool,
    /// The right-click menu, while open.
    pub menu: Option<Menu>,
    /// The view needs a rebuild.
    pub dirty_view: bool,
}

impl EditorState {
    /// The notes of the difficulty being edited.
    #[must_use]
    pub fn notes(&self) -> &[ChartNote] {
        self.session
            .chart()
            .chart_for(self.session.difficulty)
            .map_or(&[], |def| def.notes.as_slice())
    }

    /// Snap a time to the current grid — unless snapping is off, or
    /// `free` (Alt) asks for the exact time.
    #[must_use]
    pub fn snap_with(&self, time_s: f64, free: bool) -> f64 {
        match (&self.grid, self.snap_on && !free) {
            (Some(grid), true) => grid.snap(time_s, self.division).max(0.0),
            _ => time_s.max(0.0),
        }
    }

    /// Snap a time to the current grid.
    fn snap(&self, time_s: f64) -> f64 {
        self.snap_with(time_s, false)
    }

    /// Move the cursor by whole grid steps.
    fn step(&mut self, steps: i64) {
        self.cursor_s = match &self.grid {
            Some(grid) => grid.step(self.cursor_s, self.division, steps).max(0.0),
            None => (self.cursor_s + steps as f64 * 0.1).max(0.0),
        };
        self.reveal_cursor();
    }

    /// Scroll so the cursor is on screen.
    fn reveal_cursor(&mut self) {
        let cursor = self.cursor_s;
        self.view.keep_visible(cursor, BOTTOM_Y, TOP_Y, 40.0);
        self.dirty_view = true;
    }

    /// Whether a note is selected.
    #[must_use]
    pub fn is_selected(&self, note: &ChartNote) -> bool {
        self.selection.iter().any(|key| view::same_note(*key, note))
    }

    /// Apply a batch and report; keeps the view in step.
    fn apply(&mut self, ops: Vec<EditOp>, done: &str) -> bool {
        let count = ops.len();
        match self.session.edit_batch(ops) {
            Ok(()) => {
                if count > 0 {
                    self.status = done.to_owned();
                }
                self.dirty_view = true;
                true
            }
            Err(error) => {
                self.status = format!("not possible: {error}");
                self.dirty_view = true;
                false
            }
        }
    }

    /// Delete the selection as one undo step.
    fn delete_selection(&mut self) {
        let difficulty = self.session.difficulty;
        let ops: Vec<EditOp> = self
            .notes()
            .iter()
            .filter(|note| self.is_selected(note))
            .map(|note| EditOp::RemoveNote {
                difficulty,
                note: *note,
            })
            .collect();
        if ops.is_empty() {
            self.status = "nothing selected".to_owned();
            return;
        }
        let count = ops.len();
        if self.apply(ops, &format!("{count} note(s) deleted")) {
            self.selection.clear();
        }
    }

    /// Nudge the selection by grid steps / lanes.
    fn nudge(&mut self, steps: i64, dlane: i32) {
        let Some(anchor) = self
            .notes()
            .iter()
            .find(|note| self.is_selected(note))
            .map(|note| note.time)
        else {
            return;
        };
        let dt = match (&self.grid, steps) {
            (_, 0) => 0.0,
            (Some(grid), _) => grid.step(anchor, self.division, steps) - anchor,
            (None, _) => steps as f64 * 0.01,
        };
        let difficulty = self.session.difficulty;
        match view::plan_move(difficulty, self.notes(), &self.selection, dt, dlane) {
            Some((ops, moved)) => {
                if self.apply(ops, "moved") {
                    self.selection = moved;
                }
            }
            None => self.status = "cannot move there".to_owned(),
        }
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
    let saver = Saver::new(chart_path, chart.clone());
    let grid = Grid::from_marks(&chart.beat_marks());
    // A difficulty the chart lacks: open on one it has (Q switches,
    // and creates the missing one empty).
    let difficulty = if chart.chart_for(difficulty).is_some() {
        difficulty
    } else {
        chart
            .charts
            .first()
            .map(|def| def.difficulty)
            .ok_or("the chart has no difficulty to edit")?
    };
    let session = EditorSession::new(chart, difficulty).map_err(|error| error.to_string())?;
    commands.insert_resource(EditorState {
        session,
        chart_path: chart_path.to_path_buf(),
        saver,
        saved_any: false,
        audio_path: audio_path.to_path_buf(),
        cursor_s: 0.0,
        lane: Lane::Three,
        division: 4,
        snap_on: true,
        previewing: false,
        speed: 1.0,
        loop_region: None,
        looping: false,
        follow: true,
        exit_armed: 0.0,
        status: String::new(),
        grabbed: None,
        select_anchor: None,
        selection: Vec::new(),
        view: View {
            center_s: 0.0,
            px_per_s: 300.0,
            anchor_y: ANCHOR_Y,
        },
        grid,
        envelope: None,
        drag: None,
        hover: None,
        help: false,
        clip: Clip::default(),
        field: None,
        typing_ate_keys: false,
        menu: None,
        dirty_view: true,
    });
    Ok(())
}

/// The right-click menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Menu {
    /// Where it was opened, world units.
    pub at: Vec2,
    /// The song time there (Paste here).
    pub time: f64,
    /// The note it was opened on, if any.
    pub target: Option<NoteKey>,
}

/// The editor plugin.
pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuditionClicks>()
            .init_resource::<ChipActions>()
            .add_systems(
                OnEnter(AppState::Editor),
                (draw::spawn_editor, start_waveform),
            )
            .add_systems(
                Update,
                (
                    collect_waveform,
                    editor_chips,
                    editor_typing,
                    editor_input,
                    pointer::editor_pointer,
                    follow_preview,
                    preview_clicks,
                    draw::redraw,
                    draw::place_playhead,
                    draw::refresh_hud,
                    draw::sync_menu,
                )
                    .chain()
                    .run_if(in_state(AppState::Editor)),
            )
            .add_systems(OnExit(AppState::Editor), teardown_editor);
    }
}

/// Marker for editor screen entities.
#[derive(Component)]
pub(crate) struct EditorScreen;

/// The song's waveform, decoding in the background.
#[derive(Resource)]
struct WaveformTask(Task<Option<Envelope>>);

/// Decode the song once, off the frame thread, for the waveform.
fn start_waveform(mut commands: Commands, state: Option<Res<EditorState>>) {
    let Some(state) = state else {
        return;
    };
    let path = state.audio_path.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let audio = beatbyte_audio::decode_file(&path).ok()?;
        Some(Envelope::from_samples(audio.samples(), audio.sample_rate()))
    });
    commands.insert_resource(WaveformTask(task));
}

fn collect_waveform(
    mut commands: Commands,
    task: Option<ResMut<WaveformTask>>,
    state: Option<ResMut<EditorState>>,
) {
    let (Some(mut task), Some(mut state)) = (task, state) else {
        return;
    };
    let Some(result) =
        bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(&mut task.0))
    else {
        return;
    };
    commands.remove_resource::<WaveformTask>();
    state.envelope = result;
    state.dirty_view = true;
}

/// The toolbar chips' ids.
pub(crate) mod chip {
    /// Play / pause.
    pub const PLAY: u8 = 1;
    /// Cycle the snap division.
    pub const GRID: u8 = 2;
    /// Snap on / off.
    pub const SNAP: u8 = 3;
    /// Zoom out.
    pub const ZOOM_OUT: u8 = 4;
    /// Zoom in.
    pub const ZOOM_IN: u8 = 5;
    /// Undo.
    pub const UNDO: u8 = 6;
    /// Redo.
    pub const REDO: u8 = 7;
    /// Toggle HOPO on the selection.
    pub const HOPO: u8 = 8;
    /// Delete the selection.
    pub const DELETE: u8 = 9;
    /// Save.
    pub const SAVE: u8 = 10;
    /// Follow the playhead.
    pub const FOLLOW: u8 = 11;
    /// The reference.
    pub const HELP: u8 = 12;
    /// Leave.
    pub const BACK: u8 = 13;
    /// Cycle the playback speed.
    pub const SPEED: u8 = 14;
    /// Loop on / off.
    pub const LOOP: u8 = 15;
    /// Copy.
    pub const COPY: u8 = 16;
    /// Paste at the playhead.
    pub const PASTE: u8 = 17;
    /// Duplicate after the selection.
    pub const DUPLICATE: u8 = 18;
    /// Star-power phrase over the selection.
    pub const PHRASE: u8 = 19;
    /// Next difficulty.
    pub const LEVEL: u8 = 20;
    /// Type the selected note's time.
    pub const FIELD_TIME: u8 = 21;
    /// Type its lane.
    pub const FIELD_LANE: u8 = 22;
    /// Type its length.
    pub const FIELD_LENGTH: u8 = 23;
    /// Cut (menu).
    pub const CUT: u8 = 24;
    /// Paste where the menu was opened (menu).
    pub const PASTE_HERE: u8 = 25;
    /// Close the menu.
    pub const MENU_CLOSE: u8 = 26;
}

/// The actions both the keyboard and the chips trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Play,
    Grid,
    Snap,
    ZoomOut,
    ZoomIn,
    Undo,
    Redo,
    Hopo,
    Delete,
    Save,
    Follow,
    Help,
    Back,
    Speed,
    Loop,
    LoopIn,
    LoopOut,
    Copy,
    Cut,
    Paste,
    PasteHere,
    Duplicate,
    Phrase,
    Level,
    Edit(Field),
    CloseMenu,
}

/// The actions the chips asked for this frame (read by `editor_input`).
/// Public to the crate so the editor drill can press a menu item the
/// way a click on it does.
#[derive(Resource, Default)]
pub(crate) struct ChipActions(pub(crate) Vec<Action>);

/// The actions, for the editor drill.
pub(crate) mod actions {
    /// Delete (a menu item).
    pub const DELETE: super::Action = super::Action::Delete;
    /// Copy.
    pub const COPY: super::Action = super::Action::Copy;
    /// Paste at the playhead.
    pub const PASTE: super::Action = super::Action::Paste;
    /// Type the selected note's time.
    pub const EDIT_TIME: super::Action =
        super::Action::Edit(beatbyte_editor::inspector::Field::Time);
    /// A star-power phrase over the selection.
    pub const PHRASE: super::Action = super::Action::Phrase;
    /// The next difficulty.
    pub const LEVEL: super::Action = super::Action::Level;
}

#[allow(clippy::type_complexity)] // Bevy query tuple
fn editor_chips(
    mut chips: Query<(
        &crate::ui_kit::ActionChip,
        &crate::ui_kit::ChipEnabled,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut labels: Query<&mut TextColor>,
    mut out: ResMut<ChipActions>,
) {
    let pressed = crate::ui_kit::read_chips(&mut chips, &mut labels);
    // Appended, never assigned: `editor_input` takes the list each
    // frame, and whatever else asked for an action this frame (the
    // drill pressing a menu item) must not be overwritten.
    let asked: Vec<Action> = pressed
        .into_iter()
        .filter_map(|id| match id {
            chip::PLAY => Some(Action::Play),
            chip::GRID => Some(Action::Grid),
            chip::SNAP => Some(Action::Snap),
            chip::ZOOM_OUT => Some(Action::ZoomOut),
            chip::ZOOM_IN => Some(Action::ZoomIn),
            chip::UNDO => Some(Action::Undo),
            chip::REDO => Some(Action::Redo),
            chip::HOPO => Some(Action::Hopo),
            chip::DELETE => Some(Action::Delete),
            chip::SAVE => Some(Action::Save),
            chip::FOLLOW => Some(Action::Follow),
            chip::HELP => Some(Action::Help),
            chip::BACK => Some(Action::Back),
            chip::SPEED => Some(Action::Speed),
            chip::LOOP => Some(Action::Loop),
            chip::COPY => Some(Action::Copy),
            chip::CUT => Some(Action::Cut),
            chip::PASTE => Some(Action::Paste),
            chip::PASTE_HERE => Some(Action::PasteHere),
            chip::DUPLICATE => Some(Action::Duplicate),
            chip::PHRASE => Some(Action::Phrase),
            chip::LEVEL => Some(Action::Level),
            chip::FIELD_TIME => Some(Action::Edit(Field::Time)),
            chip::FIELD_LANE => Some(Action::Edit(Field::Lane)),
            chip::FIELD_LENGTH => Some(Action::Edit(Field::Length)),
            chip::MENU_CLOSE => Some(Action::CloseMenu),
            _ => None,
        })
        .collect();
    out.0.extend(asked);
}

/// Whether a command modifier (Cmd on macOS, Ctrl elsewhere) is held.
pub(crate) fn command_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ])
}

/// Whether Shift is held.
pub(crate) fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

/// Whether Alt / Option is held.
pub(crate) fn alt_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
}

/// Start or stop the audition from the cursor.
pub(crate) fn toggle_preview(
    state: &mut EditorState,
    music: &Music,
    game_clock: &mut GameClock,
    settings: &crate::config::Settings,
    now: f64,
) {
    if state.previewing {
        music.0.stop();
        game_clock.clock.stop();
        state.previewing = false;
        state.dirty_view = true;
    } else {
        music.0.play_file(state.audio_path.clone());
        music.0.set_song_gain(crate::loudness::song_gain_for(
            &crate::boot::SongAudio::File(state.audio_path.clone()),
            settings,
        ));
        game_clock.expect_song = true;
        if state.looping
            && let Some(region) = state.loop_region
            && let Some(start) = region.wrap(state.cursor_s)
        {
            state.cursor_s = start;
        }
        music.0.seek_s(state.cursor_s);
        game_clock.begin(now, state.cursor_s);
        // Explicitly: the clock keeps whatever rate it last had (a
        // practice run may have left 0.5 behind).
        game_clock.clock.set_rate(now, state.speed);
        music.0.set_speed(state.speed);
        state.previewing = true;
        state.follow = true;
    }
}

/// Jump the playhead (and the music, while it plays).
pub(crate) fn seek(
    state: &mut EditorState,
    music: &Music,
    game_clock: &mut GameClock,
    to: f64,
    now: f64,
) {
    state.cursor_s = to.max(0.0);
    if state.previewing {
        music.0.seek_s(state.cursor_s);
        game_clock.begin(now, state.cursor_s);
    }
    state.dirty_view = true;
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
#[allow(clippy::too_many_lines)] // the key map, read top to bottom
pub(crate) fn editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Option<ResMut<EditorState>>,
    music: Res<Music>,
    settings: Res<crate::config::Settings>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    mut chips: ResMut<ChipActions>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some(mut state) = state else {
        return;
    };
    state.exit_armed = (state.exit_armed - time.delta_secs()).max(0.0);
    // A field being typed into owns the keyboard; the frame it closes
    // on, its Enter / Esc are spent.
    if state.field.is_some() || std::mem::take(&mut state.typing_ate_keys) {
        chips.0.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    let command = command_held(&keys);
    let shift = shift_held(&keys);
    let pressed = |key: KeyCode| keys.just_pressed(key);

    // Every action from the keyboard AND the chips in one list, so
    // both doors lead to the same code.
    let mut actions = std::mem::take(&mut chips.0);
    let key_actions = [
        (
            pressed(KeyCode::KeyP) || pressed(KeyCode::Enter),
            Action::Play,
        ),
        (pressed(KeyCode::Tab), Action::Grid),
        (pressed(KeyCode::KeyG), Action::Snap),
        (pressed(KeyCode::Minus), Action::ZoomOut),
        (pressed(KeyCode::Equal), Action::ZoomIn),
        (
            pressed(KeyCode::KeyU) || (command && !shift && pressed(KeyCode::KeyZ)),
            Action::Undo,
        ),
        (
            pressed(KeyCode::KeyR)
                || (command && shift && pressed(KeyCode::KeyZ))
                || (command && pressed(KeyCode::KeyY)),
            Action::Redo,
        ),
        (pressed(KeyCode::KeyH), Action::Hopo),
        (
            pressed(KeyCode::Delete) || pressed(KeyCode::Backspace),
            Action::Delete,
        ),
        (pressed(KeyCode::KeyS), Action::Save),
        (pressed(KeyCode::KeyF), Action::Follow),
        (
            pressed(KeyCode::F1) || pressed(KeyCode::Slash),
            Action::Help,
        ),
        (pressed(KeyCode::Escape), Action::Back),
        (pressed(KeyCode::KeyT), Action::Speed),
        (pressed(KeyCode::KeyL), Action::Loop),
        (pressed(KeyCode::KeyI), Action::LoopIn),
        (pressed(KeyCode::KeyO), Action::LoopOut),
        (command && pressed(KeyCode::KeyC), Action::Copy),
        (command && pressed(KeyCode::KeyX), Action::Cut),
        (command && pressed(KeyCode::KeyV), Action::Paste),
        (command && pressed(KeyCode::KeyD), Action::Duplicate),
        (pressed(KeyCode::KeyY), Action::Phrase),
        (pressed(KeyCode::KeyQ), Action::Level),
        (pressed(KeyCode::Comma), Action::Edit(Field::Time)),
        (pressed(KeyCode::Period), Action::Edit(Field::Lane)),
        (pressed(KeyCode::Semicolon), Action::Edit(Field::Length)),
    ];
    actions.extend(
        key_actions
            .iter()
            .filter(|(hit, _)| *hit)
            .map(|(_, action)| *action),
    );

    // Navigation: the cursor steps the grid; with Shift the selection
    // moves instead.
    let vertical: [(KeyCode, i64); 4] = [
        (KeyCode::ArrowUp, 1),
        (KeyCode::ArrowDown, -1),
        (KeyCode::PageUp, 16),
        (KeyCode::PageDown, -16),
    ];
    for (key, steps) in vertical {
        if pressed(key) {
            if shift && !state.selection.is_empty() && steps.abs() == 1 {
                state.nudge(steps, 0);
            } else {
                let steps = if steps.abs() == 16 {
                    // A bar: four beats of the current division.
                    steps.signum() * 4 * i64::from(state.division)
                } else {
                    steps
                };
                state.step(steps);
            }
        }
    }
    for (key, direction) in [(KeyCode::ArrowLeft, -1), (KeyCode::ArrowRight, 1)] {
        if pressed(key) {
            if shift && !state.selection.is_empty() {
                state.nudge(0, direction);
            } else {
                let index = state.lane.index() as i32 + direction;
                if let Some(lane) = usize::try_from(index).ok().and_then(Lane::from_index) {
                    state.lane = lane;
                    state.dirty_view = true;
                }
            }
        }
    }
    if pressed(KeyCode::Home) {
        seek(&mut state, &music, &mut game_clock, 0.0, now);
        state.reveal_cursor();
    }
    if pressed(KeyCode::End) {
        let end = state
            .notes()
            .last()
            .map_or(0.0, |note| note.time + note.len);
        seek(&mut state, &music, &mut game_clock, end, now);
        state.reveal_cursor();
    }
    if command && pressed(KeyCode::KeyA) {
        state.selection = state.notes().iter().map(|n| (n.time, n.lane)).collect();
        state.status = format!("{} note(s) selected", state.selection.len());
        state.dirty_view = true;
    }

    // Editing at the cursor (keyboard placement).
    if pressed(KeyCode::Space) {
        let time_s = state.snap(state.cursor_s);
        let lane = state.lane.index() as u8;
        let difficulty = state.session.difficulty;
        let existing = state.notes().iter().copied().find(|note| {
            note.lane == lane && (note.time - time_s).abs() <= beatbyte_editor::EDIT_EPSILON_S
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
        if state.apply(vec![op], "edited") && existing.is_none() {
            state.selection = vec![(time_s, lane)];
        }
    }
    // V: set/clear the time-range anchor at the cursor.
    if pressed(KeyCode::KeyV) && !command {
        state.status = if state.select_anchor.take().is_some() {
            "range cleared".to_owned()
        } else {
            state.select_anchor = Some(state.snap(state.cursor_s));
            "range from here - move, then X deletes / H toggles hopo".to_owned()
        };
    }
    if pressed(KeyCode::KeyX)
        && !command
        && let Some(anchor) = state.select_anchor
    {
        let difficulty = state.session.difficulty;
        let ops = range_ops(&state, anchor, |note| EditOp::RemoveNote {
            difficulty,
            note,
        });
        let count = ops.len();
        state.apply(ops, &format!("{count} note(s) deleted"));
        state.select_anchor = None;
        state.selection.clear();
    }
    // M: grab the note under the cursor, or place a grabbed one.
    if pressed(KeyCode::KeyM) {
        grab_or_place(&mut state);
    }

    for action in actions {
        // Any action from the menu (or anywhere) closes the menu.
        let from_menu = state.menu.take();
        if from_menu.is_some() {
            state.dirty_view = true;
        }
        match action {
            Action::Play => toggle_preview(&mut state, &music, &mut game_clock, &settings, now),
            Action::Grid => {
                let at = DIVISIONS
                    .iter()
                    .position(|d| *d == state.division)
                    .unwrap_or(0);
                state.division = DIVISIONS[(at + 1) % DIVISIONS.len()];
                state.status = format!("grid 1/{}", state.division);
                state.dirty_view = true;
            }
            Action::Snap => {
                state.snap_on = !state.snap_on;
                state.status = if state.snap_on {
                    "snap on".to_owned()
                } else {
                    "snap off - notes go exactly where you put them".to_owned()
                };
            }
            Action::ZoomOut | Action::ZoomIn => {
                let factor = if action == Action::ZoomIn {
                    1.4
                } else {
                    1.0 / 1.4
                };
                let pivot = state.view.y_of(state.cursor_s);
                state.view.zoom_around(pivot, factor);
                state.dirty_view = true;
            }
            Action::Undo => {
                state.status = if state.session.undo() {
                    "undone".to_owned()
                } else {
                    "nothing to undo".to_owned()
                };
                state.selection.clear();
                state.dirty_view = true;
            }
            Action::Redo => {
                state.status = if state.session.redo() {
                    "redone".to_owned()
                } else {
                    "nothing to redo".to_owned()
                };
                state.selection.clear();
                state.dirty_view = true;
            }
            Action::Hopo => toggle_hopo(&mut state),
            Action::Delete => state.delete_selection(),
            Action::Save => state.status = save(&mut state),
            Action::Follow => {
                state.follow = !state.follow;
                state.status = if state.follow {
                    "view follows the playhead".to_owned()
                } else {
                    "view stays put".to_owned()
                };
            }
            Action::Help => state.help = !state.help,
            Action::Back => back(&mut state, &mut next_state),
            Action::Speed => {
                state.speed = playback::next_speed(state.speed);
                if state.previewing {
                    game_clock.clock.set_rate(now, state.speed);
                    music.0.set_speed(state.speed);
                }
                state.status = format!("speed {:.0} %", state.speed * 100.0);
            }
            Action::Loop => {
                if state.loop_region.is_some() {
                    state.looping = !state.looping;
                    state.status = if state.looping {
                        "looping".to_owned()
                    } else {
                        "loop off".to_owned()
                    };
                } else {
                    state.status =
                        "no loop yet - I and O set its ends, or Shift-drag the ruler".to_owned();
                }
                state.dirty_view = true;
            }
            Action::Copy | Action::Cut => {
                state.clip = Clip::copy(state.notes(), &state.selection);
                if state.clip.is_empty() {
                    state.status = "nothing selected to copy".to_owned();
                } else if action == Action::Cut {
                    let count = state.clip.len();
                    state.delete_selection();
                    state.status = format!("{count} note(s) cut");
                } else {
                    state.status = format!("{} note(s) copied", state.clip.len());
                }
            }
            Action::Paste | Action::PasteHere | Action::Duplicate => {
                let at = match (action, from_menu) {
                    (Action::PasteHere, Some(menu)) => Some(state.snap(menu.time)),
                    (Action::Duplicate, _) => {
                        // Right after the selection, one grid step on.
                        state.clip = Clip::copy(state.notes(), &state.selection);
                        clipboard::selection_span(state.notes(), &state.selection).map(|span| {
                            match &state.grid {
                                Some(grid) => grid.step(span.1, state.division, 1),
                                None => span.1 + 0.25,
                            }
                        })
                    }
                    _ => Some(state.snap(state.cursor_s)),
                };
                match at {
                    Some(at) if !state.clip.is_empty() => {
                        let (ops, keys) = state.clip.paste(state.session.difficulty, at);
                        let count = keys.len();
                        if state.apply(ops, &format!("{count} note(s) pasted")) {
                            state.selection = keys;
                        }
                    }
                    _ => state.status = "nothing to paste - copy first (Cmd+C)".to_owned(),
                }
            }
            Action::Phrase => phrase(&mut state),
            Action::Level => {
                let order = [
                    beatbyte_core::Difficulty::Easy,
                    beatbyte_core::Difficulty::Medium,
                    beatbyte_core::Difficulty::Hard,
                    beatbyte_core::Difficulty::Expert,
                ];
                let at = order
                    .iter()
                    .position(|d| *d == state.session.difficulty)
                    .unwrap_or(0);
                let next = order[(at + 1) % order.len()];
                state.selection.clear();
                state.status = match state.session.set_difficulty(next) {
                    Ok(true) => format!("{next}: new and empty (U undoes creating it)"),
                    Ok(false) => format!("editing {next} - the others stay as they are"),
                    Err(error) => error.to_string(),
                };
                state.dirty_view = true;
            }
            Action::Edit(field) => {
                let one = state
                    .notes()
                    .iter()
                    .filter(|n| state.is_selected(n))
                    .copied()
                    .collect::<Vec<_>>();
                match one.as_slice() {
                    [note] => {
                        state.field = Some((field, field.current(note)));
                        state.status =
                            format!("type the {} - Enter sets, Esc cancels", field.label());
                    }
                    _ => state.status = "select exactly one note to type its values".to_owned(),
                }
            }
            Action::CloseMenu => {}
            Action::LoopIn | Action::LoopOut => {
                let at = state.cursor_s;
                let region = match (state.loop_region, action) {
                    (Some(region), Action::LoopIn) => region.with_in(at),
                    (Some(region), _) => region.with_out(at),
                    // The first end: a bar's worth from here.
                    (None, Action::LoopIn) => LoopRegion::between(at, at + 2.0),
                    (None, _) => LoopRegion::between(at - 2.0, at),
                };
                match region {
                    Some(region) => {
                        state.loop_region = Some(region);
                        state.looping = true;
                        state.status = format!("loop {:.3} - {:.3} s", region.start, region.end);
                    }
                    None => state.status = "a loop needs some length".to_owned(),
                }
                state.dirty_view = true;
            }
        }
    }
}

/// ESC / Back: cancel what is pending, else leave (twice if unsaved).
fn back(state: &mut EditorState, next_state: &mut NextState<AppState>) {
    if state.menu.take().is_some() {
        state.dirty_view = true;
    } else if state.help {
        state.help = false;
    } else if state.drag.take().is_some() {
        state.status = "cancelled".to_owned();
        state.dirty_view = true;
    } else if state.grabbed.take().is_some() {
        state.status = "move cancelled".to_owned();
    } else if state.select_anchor.take().is_some() {
        state.status = "range cleared".to_owned();
    } else if !state.selection.is_empty() {
        state.selection.clear();
        state.status = "selection cleared".to_owned();
        state.dirty_view = true;
    } else if state.session.dirty() && state.exit_armed <= 0.0 {
        state.exit_armed = 3.0;
        state.status = "unsaved changes! ESC again to leave without saving, S saves".to_owned();
    } else {
        next_state.set(AppState::SongSelect);
    }
}

/// H: HOPO on the selection, the range, or the note at the cursor.
fn toggle_hopo(state: &mut EditorState) {
    let difficulty = state.session.difficulty;
    let ops: Vec<EditOp> = if !state.selection.is_empty() {
        state
            .notes()
            .iter()
            .filter(|note| state.is_selected(note))
            .map(|note| EditOp::ToggleHopo {
                difficulty,
                time: note.time,
                lane: note.lane,
            })
            .collect()
    } else if let Some(anchor) = state.select_anchor {
        range_ops(state, anchor, |note| EditOp::ToggleHopo {
            difficulty,
            time: note.time,
            lane: note.lane,
        })
    } else {
        vec![EditOp::ToggleHopo {
            difficulty,
            time: state.snap(state.cursor_s),
            lane: state.lane.index() as u8,
        }]
    };
    let count = ops.len();
    state.apply(ops, &format!("hopo toggled on {count} note(s)"));
    state.select_anchor = None;
}

/// M: pick the note at the cursor up, or put it down.
fn grab_or_place(state: &mut EditorState) {
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
        if state.apply(vec![op], "note moved") {
            state.grabbed = None;
            state.selection = vec![(time_s, lane)];
        }
    } else {
        let existing = state.notes().iter().copied().find(|note| {
            note.lane == lane && (note.time - time_s).abs() <= beatbyte_editor::EDIT_EPSILON_S
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

/// Y: a star-power phrase over the selection (replacing what it
/// overlaps), or — nothing selected — remove the phrase at the
/// playhead.
fn phrase(state: &mut EditorState) {
    let difficulty = state.session.difficulty;
    let phrases = state
        .session
        .chart()
        .chart_for(difficulty)
        .map(|def| def.phrases.clone())
        .unwrap_or_default();
    if let Some(span) = clipboard::selection_span(state.notes(), &state.selection) {
        let ops = clipboard::phrase_over(difficulty, &phrases, span);
        state.apply(ops, "star-power phrase over the selection");
    } else if let Some(phrase) = clipboard::phrase_at(&phrases, state.cursor_s) {
        state.apply(
            vec![EditOp::RemovePhrase { difficulty, phrase }],
            "star-power phrase removed",
        );
    } else {
        state.status =
            "select notes for a phrase, or put the playhead in one to remove it".to_owned();
    }
}

/// Typing into an inspector field: digits and separators the field
/// takes, Backspace, Enter sets, Esc cancels.
pub(crate) fn editor_typing(
    state: Option<ResMut<EditorState>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
) {
    let Some(mut state) = state else {
        typed.clear();
        return;
    };
    let Some((field, mut text)) = state.field.clone() else {
        typed.clear();
        return;
    };
    use bevy::input::keyboard::Key;
    let mut close: Option<bool> = None;
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Character(chars) => {
                text.extend(chars.chars().filter(|c| field.accepts(*c)));
            }
            Key::Backspace => {
                text.pop();
            }
            Key::Enter => close = Some(true),
            Key::Escape => close = Some(false),
            _ => {}
        }
    }
    match close {
        None => state.field = Some((field, text)),
        Some(apply) => {
            state.field = None;
            state.typing_ate_keys = true;
            if !apply {
                state.status = "unchanged".to_owned();
                return;
            }
            let note = state.notes().iter().find(|n| state.is_selected(n)).copied();
            let Some(note) = note else {
                return;
            };
            let difficulty = state.session.difficulty;
            match inspector::apply_field(difficulty, &note, field, &text, state.grid.as_ref()) {
                Ok(Some(op)) => {
                    let key = match op {
                        EditOp::MoveNote {
                            to_time, to_lane, ..
                        } => (to_time, to_lane),
                        _ => (note.time, note.lane),
                    };
                    if state.apply(vec![op], "set") {
                        state.selection = vec![key];
                    }
                }
                Ok(None) => state.status = "unchanged".to_owned(),
                Err(message) => state.status = message,
            }
        }
    }
    state.dirty_view = true;
}

/// Save the edit as a new chart version (see [`Saver`]); the status
/// line it leaves.
pub fn save(state: &mut EditorState) -> String {
    let now = crate::players::now_ms();
    let chart = state.session.chart().clone();
    match state.saver.save(&chart, now) {
        Ok(Saved::Unchanged) => {
            state.session.mark_saved();
            "nothing changed - nothing written".to_owned()
        }
        Ok(Saved::Written { name, rewritten }) => {
            state.session.mark_saved();
            state.saved_any = true;
            state.chart_path = state.saver.current_path();
            if rewritten {
                format!("saved ({name})")
            } else {
                format!("saved as {name} - the earlier version is kept")
            }
        }
        Err(error) => error,
    }
}

/// While previewing, the playhead follows the music — around the loop
/// when one plays — and the view follows the playhead unless the
/// player scrolled away.
fn follow_preview(
    state: Option<ResMut<EditorState>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
) {
    let Some(mut state) = state else {
        return;
    };
    if state.previewing
        && let Some(now) = game_clock.song_time(&time)
    {
        if state.looping
            && state.drag.is_none()
            && let Some(region) = state.loop_region
            && let Some(start) = region.wrap(now)
        {
            seek(
                &mut state,
                &music,
                &mut game_clock,
                start,
                time.elapsed_secs_f64(),
            );
            return;
        }
        state.cursor_s = now;
        if state.follow && state.drag.is_none() {
            state.view.center_s = now;
            state.dirty_view = true;
        }
    }
}

fn teardown_editor(
    mut commands: Commands,
    entities: Query<Entity, With<EditorScreen>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    state: Option<Res<EditorState>>,
    builtins: Res<crate::boot::BuiltinSongs>,
    mut library: ResMut<crate::library::SongLibrary>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<WaveformTask>();
    music.0.stop();
    music.0.set_speed(1.0);
    game_clock.clock.stop();
    game_clock.clock.set_rate(0.0, 1.0);
    // A saved edit is a new active version: read the library again so
    // the browser plays it, not the file the scan saw before.
    if state.is_some_and(|state| state.saved_any) {
        *library = crate::boot::scan_with_builtins(&builtins.0);
    }
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
    let beat = state
        .grid
        .as_ref()
        .map_or_else(
            || state.session.chart().tempo_map().beats_at(position),
            |grid| grid.beat_at(position),
        )
        .floor() as i64;
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
/// lanes), mapped to ops — the keyboard's range edit.
fn range_ops(state: &EditorState, anchor: f64, to_op: impl Fn(ChartNote) -> EditOp) -> Vec<EditOp> {
    let cursor = state.snap(state.cursor_s);
    let (lo, hi) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    let epsilon = beatbyte_editor::EDIT_EPSILON_S;
    state
        .notes()
        .iter()
        .copied()
        .filter(|note| note.time >= lo - epsilon && note.time <= hi + epsilon)
        .map(to_op)
        .collect()
}
