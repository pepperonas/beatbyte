//! Drawing the editor: the lanes, the grid, the waveform, the notes,
//! what a drag would do, the playhead, the toolbar, the information
//! block and the reference.
//!
//! The notes and lines are rebuilt only when something changed
//! (`dirty_view`) and only for what is on screen; the waveform is a
//! fixed pool of rows that are resized, never respawned, so following
//! the playhead through a long song costs the same as a short one.

use beatbyte_core::Lane;
use beatbyte_editor::view::{self, Hit, LANE_WIDTH};
use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::{BOTTOM_Y, Drag, EditorScreen, EditorState, TOP_Y, chip};
use crate::palette;
use crate::ui::UiFont;
use crate::ui_kit;

/// The waveform strip's centre x and width.
const WAVE_X: f32 = -282.0;
const WAVE_W: f32 = 140.0;
/// The left edge of the grid lines (under the waveform too).
const GRID_LEFT: f32 = WAVE_X - WAVE_W / 2.0;
/// The right edge (the last lane).
const GRID_RIGHT: f32 = 2.5 * LANE_WIDTH as f32;
/// Height of one waveform row.
const WAVE_ROW: f32 = 3.0;
/// A note head.
const HEAD: Vec2 = Vec2::new(54.0, 14.0);

/// The lane names, for the information block.
const LANE_NAMES: [&str; 5] = ["green", "red", "yellow", "blue", "orange"];

/// Rebuilt when the view changes.
#[derive(Component)]
pub(crate) struct EditorDyn;

/// One row of the waveform pool.
#[derive(Component)]
pub(crate) struct WaveRow(usize);

/// The playhead line.
#[derive(Component)]
pub(crate) struct Playhead;

/// The information block.
#[derive(Component)]
pub(crate) struct EditorHud;

/// The reference overlay.
#[derive(Component)]
pub(crate) struct HelpOverlay;

fn wave_rows() -> usize {
    ((TOP_Y - BOTTOM_Y) as f32 / WAVE_ROW).ceil() as usize
}

fn band_mid() -> f32 {
    ((TOP_Y + BOTTOM_Y) / 2.0) as f32
}

fn band_height() -> f32 {
    (TOP_Y - BOTTOM_Y) as f32
}

/// The static parts, once on entry.
pub(crate) fn spawn_editor(
    mut commands: Commands,
    state: Option<ResMut<EditorState>>,
    font: Res<UiFont>,
) {
    let Some(mut state) = state else {
        return;
    };
    state.dirty_view = true;
    state.previewing = false;
    state.status = "editing - F1 shows every key and gesture".to_owned();

    let lanes_w = 5.0 * LANE_WIDTH as f32;
    // The lanes' bed and guides.
    commands.spawn((
        EditorScreen,
        Sprite::from_color(palette::SURFACE, Vec2::new(lanes_w + 8.0, band_height())),
        Transform::from_xyz(0.0, band_mid(), -10.0),
    ));
    for lane in Lane::ALL {
        commands.spawn((
            EditorScreen,
            Sprite::from_color(
                palette::dimmed(palette::lane_color(lane), 0.12),
                Vec2::new(2.0, band_height()),
            ),
            Transform::from_xyz(view::lane_x(lane.index() as u8) as f32, band_mid(), -9.0),
        ));
    }
    // The waveform's bed and its pool of rows.
    commands.spawn((
        EditorScreen,
        Sprite::from_color(
            palette::SURFACE.with_alpha(0.6),
            Vec2::new(WAVE_W, band_height()),
        ),
        Transform::from_xyz(WAVE_X, band_mid(), -10.0),
    ));
    for index in 0..wave_rows() {
        commands.spawn((
            EditorScreen,
            WaveRow(index),
            Sprite::from_color(
                palette::dimmed(palette::TEXT_DIM, 0.55),
                Vec2::new(0.0, WAVE_ROW * 0.8),
            ),
            Transform::from_xyz(
                WAVE_X,
                BOTTOM_Y as f32 + (index as f32 + 0.5) * WAVE_ROW,
                -8.0,
            ),
        ));
    }
    // The playhead: across the waveform and the lanes.
    commands.spawn((
        EditorScreen,
        Playhead,
        Sprite::from_color(
            palette::BRAND.with_alpha(0.9),
            Vec2::new(GRID_RIGHT - GRID_LEFT + 12.0, 2.0),
        ),
        Transform::from_xyz((GRID_LEFT + GRID_RIGHT) / 2.0, 0.0, 4.0),
    ));
    commands.spawn((
        EditorScreen,
        EditorHud,
        Text2d::new(""),
        font.text(ui_kit::SMALL * 0.9),
        TextColor(palette::TEXT),
        Anchor::TOP_LEFT,
        Transform::from_xyz(GRID_RIGHT + 26.0, TOP_Y as f32, 5.0),
    ));
    commands.spawn((
        EditorScreen,
        Text2d::new(
            "click place  drag move  top tab length  drag empty box  right-click menu  \
             wheel scroll  cmd-wheel zoom  shift-drag ruler loop  F1 help",
        ),
        font.text(ui_kit::SMALL * 0.8),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
        Transform::from_xyz(0.0, BOTTOM_Y as f32 - 20.0, 5.0),
    ));
    // The reference, hidden until F1.
    commands
        .spawn((
            EditorScreen,
            HelpOverlay,
            Sprite::from_color(
                palette::BACKGROUND.with_alpha(0.94),
                Vec2::new(900.0, 560.0),
            ),
            Transform::from_xyz(0.0, 0.0, 20.0),
            Visibility::Hidden,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Text2d::new(HELP),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
        });
    // The toolbar: every action as a clickable chip, its key beside it.
    commands
        .spawn((
            EditorScreen,
            Node {
                position_type: PositionType::Absolute,
                top: px(6),
                left: px(0),
                right: px(0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            ui_kit::action_bar(root, &font, &TOOLBAR);
        });
}

/// The toolbar, left to right.
const TOOLBAR: [ui_kit::ChipSpec; 23] = [
    chip_spec(chip::PLAY, "Play P"),
    chip_spec(chip::SPEED, "Speed T"),
    chip_spec(chip::LOOP, "Loop L"),
    chip_spec(chip::GRID, "Grid Tab"),
    chip_spec(chip::SNAP, "Snap G"),
    chip_spec(chip::ZOOM_OUT, "Zoom -"),
    chip_spec(chip::ZOOM_IN, "Zoom +"),
    chip_spec(chip::FOLLOW, "Follow F"),
    chip_spec(chip::UNDO, "Undo U"),
    chip_spec(chip::REDO, "Redo R"),
    chip_spec(chip::HOPO, "HOPO H"),
    chip_spec(chip::DELETE, "Delete Del"),
    chip_spec(chip::SAVE, "Save S"),
    chip_spec(chip::COPY, "Copy Cmd+C"),
    chip_spec(chip::PASTE, "Paste Cmd+V"),
    chip_spec(chip::DUPLICATE, "Dup Cmd+D"),
    chip_spec(chip::PHRASE, "Star Y"),
    chip_spec(chip::LEVEL, "Level Q"),
    chip_spec(chip::FIELD_TIME, "Time ,"),
    chip_spec(chip::FIELD_LANE, "Lane ."),
    chip_spec(chip::FIELD_LENGTH, "Length ;"),
    chip_spec(chip::HELP, "Help F1"),
    chip_spec(chip::BACK, "Back Esc"),
];

/// The right-click menu on a note.
const NOTE_MENU: [ui_kit::ChipSpec; 10] = [
    chip_spec(chip::DELETE, "Delete"),
    chip_spec(chip::HOPO, "HOPO"),
    chip_spec(chip::COPY, "Copy"),
    chip_spec(chip::CUT, "Cut"),
    chip_spec(chip::DUPLICATE, "Duplicate"),
    chip_spec(chip::PHRASE, "Star phrase"),
    chip_spec(chip::FIELD_TIME, "Time..."),
    chip_spec(chip::FIELD_LANE, "Lane..."),
    chip_spec(chip::FIELD_LENGTH, "Length..."),
    chip_spec(chip::MENU_CLOSE, "Close"),
];

/// The right-click menu on empty space.
const SPACE_MENU: [ui_kit::ChipSpec; 3] = [
    chip_spec(chip::PASTE_HERE, "Paste here"),
    chip_spec(chip::PHRASE, "Remove star phrase"),
    chip_spec(chip::MENU_CLOSE, "Close"),
];

/// The height of one menu row (a chip and the bar's gap), logical px.
const MENU_ROW: f32 = 32.0;

/// The open menu's UI.
#[derive(Component)]
pub(crate) struct MenuNode;

/// Show the right-click menu where it was opened; rebuilt only when
/// it opens, moves or closes.
pub(crate) fn sync_menu(
    mut commands: Commands,
    state: Option<Res<EditorState>>,
    open: Query<Entity, With<MenuNode>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    font: Res<UiFont>,
    mut shown: Local<Option<super::Menu>>,
) {
    let wanted = state.and_then(|s| s.menu);
    if wanted == *shown {
        return;
    }
    *shown = wanted;
    for entity in &open {
        commands.entity(entity).despawn();
    }
    let Some(menu) = wanted else {
        return;
    };
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let Ok(at) = camera.world_to_viewport(transform, menu.at.extend(0.0)) else {
        return;
    };
    let items: &[ui_kit::ChipSpec] = if menu.target.is_some() {
        &NOTE_MENU
    } else {
        &SPACE_MENU
    };
    // Kept inside the window: a menu opened near the bottom opens
    // upwards (one row is a chip and the bar's gap, ~32 px).
    let height = items.len() as f32 * MENU_ROW + 16.0;
    let bottom = windows.single().map_or(f32::MAX, Window::height);
    let top = at.y.min(bottom - height - 8.0).max(8.0);
    commands
        .spawn((
            EditorScreen,
            MenuNode,
            Node {
                position_type: PositionType::Absolute,
                left: px(at.x + 8.0),
                top: px(top),
                // One item per row: a chip bar only wraps at the page
                // width, and a menu is a column.
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                padding: UiRect::all(px(ui_kit::PANEL_BORDER * 6.0)),
                border: UiRect::all(px(ui_kit::PANEL_BORDER)),
                ..default()
            },
            BackgroundColor(palette::BACKGROUND.with_alpha(0.95)),
            BorderColor::all(palette::BRAND.with_alpha(0.6)),
            GlobalZIndex(10),
        ))
        .with_children(|panel| {
            for item in items {
                ui_kit::action_bar(panel, &font, std::slice::from_ref(item));
            }
        });
}

const fn chip_spec(id: u8, label: &'static str) -> ui_kit::ChipSpec {
    ui_kit::ChipSpec {
        id,
        label,
        enabled: true,
    }
}

/// The reference behind F1.
pub(crate) const HELP: &str = "\
CHART EDITOR

MOUSE
  click in a lane .......... place a note (Alt: exact, no snap)
  click a note ............. select it   Shift: add / remove
  drag a note .............. move the selection (time and lane)
  drag the tab above ....... set its length (hold)
  drag in empty space ...... box select   Shift: add
  right-click .............. the menu: delete, HOPO, copy, paste ...
  click / drag the ruler ... move the playhead
  Shift + drag the ruler ... a loop region
  wheel .................... scroll   Cmd/Ctrl + wheel: zoom

KEYS
  P / Enter  play, pause        F  follow the playhead
  T  speed 100 / 75 / 50 %      L  loop on / off
  I / O  loop from / to the playhead (or Shift-drag the ruler)
  Up / Down  step the grid      PgUp / PgDn  a bar
  Home / End start, last note   Left / Right  lane
  Space      note at cursor     M  pick up, put down
  Shift + arrows  move the selection
  Tab  grid 1/1 ... 1/16        G  snap on / off
  -  /  =   zoom                H  HOPO
  Del / Backspace  delete       Cmd/Ctrl+A  select all
  U / Cmd+Z  undo               R / Cmd+Shift+Z  redo
  V  range from here, X deletes it
  Cmd+C / X / V  copy, cut, paste at the playhead
  Cmd+D  duplicate after the selection
  Y  star phrase over the selection (or remove the one here)
  Q  next difficulty (a missing one starts empty)
  ,  .  ;  type the time, lane, length of the selected note
  right-click  the menu
  S  save as a new version      Esc  cancel, leave

F1 / ?  close";

/// One rebuilt sprite.
fn put(commands: &mut Commands, bundle: (Sprite, Transform)) {
    commands.spawn((EditorScreen, EditorDyn, bundle.0, bundle.1));
}

fn y32(value: f64) -> f32 {
    value as f32
}

/// Rebuild the notes, lines and previews when anything changed.
#[allow(clippy::too_many_lines)] // one picture, drawn back to front
pub(crate) fn redraw(
    mut commands: Commands,
    state: Option<ResMut<EditorState>>,
    old: Query<Entity, With<EditorDyn>>,
    mut rows: Query<(&WaveRow, &mut Sprite)>,
    font: Res<UiFont>,
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
    let v = state.view;
    let (t_lo, t_hi) = (v.time_at(BOTTOM_Y), v.time_at(TOP_Y));
    let visible = |y: f64| (BOTTOM_Y..=TOP_Y).contains(&y);

    // The waveform pool: each row reads the loudest point of its span.
    for (row, mut sprite) in &mut rows {
        let y0 = BOTTOM_Y + row.0 as f64 * f64::from(WAVE_ROW);
        let peak = state.envelope.as_ref().map_or(0.0, |envelope| {
            envelope.peak(v.time_at(y0), v.time_at(y0 + f64::from(WAVE_ROW)))
        });
        sprite.custom_size = Some(Vec2::new((peak * WAVE_W).max(0.0), WAVE_ROW * 0.8));
    }

    // Grid lines: bars always, beats when they are far enough apart,
    // the snap division when it is too.
    let lanes_w = 5.0 * LANE_WIDTH as f32;
    let full_w = GRID_RIGHT - GRID_LEFT;
    let full_x = (GRID_LEFT + GRID_RIGHT) / 2.0;
    if let Some(grid) = &state.grid {
        let marks = grid.marks_between(t_lo, t_hi);
        let beat_px = marks
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) * v.px_per_s)
            .fold(f64::INFINITY, f64::min);
        let sub_px = beat_px / f64::from(state.division);
        for (index, (time, bar)) in marks.iter().enumerate() {
            let y = y32(v.y_of(*time));
            if *bar {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(
                            palette::dimmed(palette::TEXT_DIM, 0.55),
                            Vec2::new(full_w, 2.0),
                        ),
                        Transform::from_xyz(full_x, y, -7.0),
                    ),
                );
                let position = grid.position(*time);
                commands.spawn((
                    EditorScreen,
                    EditorDyn,
                    Text2d::new(position.bar.to_string()),
                    font.text(ui_kit::SMALL * 0.8),
                    TextColor(palette::dimmed(palette::TEXT_DIM, 0.9)),
                    Anchor::CENTER_RIGHT,
                    Transform::from_xyz(GRID_LEFT - 6.0, y, 5.0),
                ));
            } else if beat_px >= 8.0 {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(
                            palette::dimmed(palette::TEXT_DIM, 0.25),
                            Vec2::new(full_w, 1.0),
                        ),
                        Transform::from_xyz(full_x, y, -7.0),
                    ),
                );
            }
            // Subdivisions up to the next beat, inside the lanes.
            if sub_px >= 12.0
                && state.division > 1
                && let Some(next) = marks.get(index + 1)
            {
                for step in 1..state.division {
                    let t = time + (next.0 - time) * f64::from(step) / f64::from(state.division);
                    put(
                        &mut commands,
                        (
                            Sprite::from_color(
                                palette::dimmed(palette::TEXT_DIM, 0.13),
                                Vec2::new(lanes_w, 1.0),
                            ),
                            Transform::from_xyz(0.0, y32(v.y_of(t)), -7.5),
                        ),
                    );
                }
            }
        }
    }

    // The loop region, across the ruler and the lanes.
    let loop_band = match state.drag {
        Some(Drag::LoopRegion { from, to }) => Some((from.min(to), from.max(to), true)),
        _ => state.loop_region.map(|r| (r.start, r.end, state.looping)),
    };
    if let Some((start, end, on)) = loop_band {
        let (lo, hi) = (v.y_of(start).max(BOTTOM_Y), v.y_of(end).min(TOP_Y));
        if hi > lo {
            let alpha = if on { 0.03 } else { 0.012 };
            put(
                &mut commands,
                (
                    Sprite::from_color(
                        palette::HYPE.with_alpha(alpha),
                        Vec2::new(full_w, (hi - lo) as f32),
                    ),
                    Transform::from_xyz(full_x, ((lo + hi) / 2.0) as f32, -7.8),
                ),
            );
        }
        for edge in [start, end] {
            let y = v.y_of(edge);
            if visible(y) {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(
                            palette::HYPE.with_alpha(if on { 0.9 } else { 0.4 }),
                            Vec2::new(full_w, 2.0),
                        ),
                        Transform::from_xyz(full_x, y as f32, -6.5),
                    ),
                );
            }
        }
    }

    // Star-power phrases: a band across the lanes and a stripe beside.
    if let Some(def) = state.session.chart().chart_for(state.session.difficulty) {
        for phrase in &def.phrases {
            if phrase.end < t_lo || phrase.start > t_hi {
                continue;
            }
            let (lo, hi) = (
                v.y_of(phrase.start).max(BOTTOM_Y),
                v.y_of(phrase.end).min(TOP_Y),
            );
            let (mid, height) = (((lo + hi) / 2.0) as f32, (hi - lo).max(2.0) as f32);
            put(
                &mut commands,
                (
                    Sprite::from_color(palette::HYPE.with_alpha(0.018), Vec2::new(lanes_w, height)),
                    Transform::from_xyz(0.0, mid, -8.2),
                ),
            );
            put(
                &mut commands,
                (
                    Sprite::from_color(palette::HYPE.with_alpha(0.85), Vec2::new(6.0, height)),
                    Transform::from_xyz(GRID_RIGHT + 8.0, mid, 0.0),
                ),
            );
        }
    }

    // The keyboard lane.
    put(
        &mut commands,
        (
            Sprite::from_color(
                // ⚠️ Bevy blends alpha in LINEAR space: 0.035 white read
                // as a solid grey column. A whisper needs a whisper.
                Color::WHITE.with_alpha(0.008),
                Vec2::new(LANE_WIDTH as f32 - 4.0, band_height()),
            ),
            Transform::from_xyz(
                view::lane_x(state.lane.index() as u8) as f32,
                band_mid(),
                -8.5,
            ),
        ),
    );

    // The V range.
    if let Some(anchor) = state.select_anchor {
        let (a, b) = (v.y_of(anchor), v.y_of(state.cursor_s));
        let (lo, hi) = (a.min(b).max(BOTTOM_Y), a.max(b).min(TOP_Y));
        if hi > lo {
            put(
                &mut commands,
                (
                    Sprite::from_color(
                        palette::BRAND.with_alpha(0.08),
                        Vec2::new(lanes_w, (hi - lo) as f32),
                    ),
                    Transform::from_xyz(0.0, ((lo + hi) / 2.0) as f32, -8.0),
                ),
            );
        }
    }

    // The notes.
    let moving = match state.drag {
        Some(Drag::Move { dt, dlane, .. }) => Some((dt, dlane)),
        _ => None,
    };
    let lengthening = match state.drag {
        Some(Drag::Length { key, end }) => Some((key, end)),
        _ => None,
    };
    let hovered = match state.hover {
        Some(Hit::Head { time, lane } | Hit::Handle { time, lane }) => Some((time, lane)),
        _ => None,
    };
    for note in state.notes() {
        let tail_len = match lengthening {
            Some((key, end)) if view::same_note(key, note) => (end - note.time).max(0.0),
            _ => note.len,
        };
        if note.time + tail_len < t_lo || note.time > t_hi {
            continue;
        }
        let color = palette::lane_color(Lane::from_index(note.lane as usize).unwrap_or(Lane::One));
        let selected = state.is_selected(note);
        let x = view::lane_x(note.lane) as f32;
        let y = y32(v.y_of(note.time));
        // Tail.
        if tail_len > 0.0 {
            let top = y32(v.y_of(note.time + tail_len)).min(TOP_Y as f32);
            let bottom = y.max(BOTTOM_Y as f32);
            if top > bottom {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(color.with_alpha(0.45), Vec2::new(12.0, top - bottom)),
                        Transform::from_xyz(x, (top + bottom) / 2.0, -1.0),
                    ),
                );
            }
        }
        if visible(f64::from(y)) {
            if selected {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(Color::WHITE, HEAD + Vec2::splat(6.0)),
                        Transform::from_xyz(x, y, -0.5),
                    ),
                );
            } else if hovered.is_some_and(|key| view::same_note(key, note)) {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(Color::WHITE.with_alpha(0.35), HEAD + Vec2::splat(4.0)),
                        Transform::from_xyz(x, y, -0.5),
                    ),
                );
            }
            put(
                &mut commands,
                (
                    Sprite::from_color(color, HEAD),
                    Transform::from_xyz(x, y, 0.0),
                ),
            );
            if note.hopo {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(Color::WHITE, Vec2::new(26.0, 5.0)),
                        Transform::from_xyz(x, y, 0.5),
                    ),
                );
            }
        }
        // The length handle on a selected note.
        if selected {
            let head_top = f64::from(y) + view::HEAD_REACH;
            let top = v.y_of(note.time + tail_len).max(head_top);
            let handle_y = top + view::HANDLE_REACH / 2.0;
            if visible(handle_y) {
                put(
                    &mut commands,
                    (
                        Sprite::from_color(Color::WHITE.with_alpha(0.85), Vec2::new(22.0, 6.0)),
                        Transform::from_xyz(x, handle_y as f32, 0.6),
                    ),
                );
            }
        }
        // Where a move would put it.
        if selected && let Some((dt, dlane)) = moving {
            let lane = i32::from(note.lane) + dlane;
            if (0..5).contains(&lane) {
                let gy = y32(v.y_of(note.time + dt));
                if visible(f64::from(gy)) {
                    put(
                        &mut commands,
                        (
                            Sprite::from_color(color.with_alpha(0.45), HEAD),
                            Transform::from_xyz(view::lane_x(lane as u8) as f32, gy, 1.0),
                        ),
                    );
                }
            }
        }
    }

    // The note a click would place.
    if state.drag.is_none()
        && let Some(Hit::Lane { time, lane }) = state.hover
    {
        let placed = state.snap_with(time, false);
        let gy = y32(v.y_of(placed));
        let color = palette::lane_color(Lane::from_index(lane as usize).unwrap_or(Lane::One));
        put(
            &mut commands,
            (
                Sprite::from_color(color.with_alpha(0.28), HEAD),
                Transform::from_xyz(view::lane_x(lane) as f32, gy, 1.0),
            ),
        );
    }

    // A box being drawn.
    if let Some(Drag::Box { t0, l0, t1, l1, .. }) = state.drag {
        let (a, b) = (v.y_of(t0), v.y_of(t1));
        let (lo, hi) = (a.min(b).max(BOTTOM_Y), a.max(b).min(TOP_Y));
        let (la, lb) = (l0.min(l1), l0.max(l1));
        let left = view::lane_x(la) - LANE_WIDTH / 2.0;
        let right = view::lane_x(lb) + LANE_WIDTH / 2.0;
        put(
            &mut commands,
            (
                Sprite::from_color(
                    palette::BRAND.with_alpha(0.035),
                    Vec2::new((right - left) as f32, (hi - lo).max(1.0) as f32),
                ),
                Transform::from_xyz(((left + right) / 2.0) as f32, ((lo + hi) / 2.0) as f32, 2.0),
            ),
        );
        // Its outline: the fill alone is a whisper (linear blending).
        let (w, h) = ((right - left) as f32, (hi - lo).max(1.0) as f32);
        let (cx, cy) = (((left + right) / 2.0) as f32, ((lo + hi) / 2.0) as f32);
        for (size, dx, dy) in [
            (Vec2::new(w, 1.5), 0.0, h / 2.0),
            (Vec2::new(w, 1.5), 0.0, -h / 2.0),
            (Vec2::new(1.5, h), w / 2.0, 0.0),
            (Vec2::new(1.5, h), -w / 2.0, 0.0),
        ] {
            put(
                &mut commands,
                (
                    Sprite::from_color(palette::BRAND.with_alpha(0.8), size),
                    Transform::from_xyz(cx + dx, cy + dy, 2.1),
                ),
            );
        }
    }

    // The note picked up with M.
    if let Some((time, lane)) = state.grabbed {
        let gy = v.y_of(time);
        if visible(gy) {
            put(
                &mut commands,
                (
                    Sprite::from_color(palette::BRAND, HEAD + Vec2::splat(8.0)),
                    Transform::from_xyz(view::lane_x(lane) as f32, gy as f32, -0.6),
                ),
            );
        }
    }
}

/// The playhead follows the cursor every frame.
pub(crate) fn place_playhead(
    state: Option<Res<EditorState>>,
    mut playhead: Query<(&mut Transform, &mut Visibility), With<Playhead>>,
) {
    let (Some(state), Ok((mut transform, mut visibility))) = (state, playhead.single_mut()) else {
        return;
    };
    let y = state.view.y_of(state.cursor_s);
    transform.translation.y = y as f32;
    *visibility = if (BOTTOM_Y..=TOP_Y).contains(&y) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// The information block: where the playhead is, what the pointer is
/// over, what is selected — the stored values, exactly.
pub(crate) fn refresh_hud(
    state: Option<Res<EditorState>>,
    mut hud: Query<&mut Text2d, With<EditorHud>>,
    mut help: Query<&mut Visibility, With<HelpOverlay>>,
) {
    let Some(state) = state else {
        return;
    };
    if let Ok(mut visibility) = help.single_mut() {
        let wanted = if state.help {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let position = |time: f64| {
        state
            .grid
            .as_ref()
            .map_or_else(String::new, |grid| grid.position(time).to_string())
    };
    let mut lines = vec![
        format!(
            "{} [{}]",
            state.session.chart().song.title,
            state.session.difficulty
        ),
        format!(
            "{}  {}{}",
            if state.session.dirty() {
                "UNSAVED"
            } else {
                "saved"
            },
            state
                .chart_path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            if state.session.is_valid() {
                ""
            } else {
                "  INVALID"
            }
        ),
        String::new(),
        format!(
            "playhead {}  {:.3} s",
            position(state.cursor_s),
            state.cursor_s
        ),
        format!(
            "grid 1/{}  snap {}  zoom {:.0}/s{}",
            state.division,
            if state.snap_on { "on" } else { "off" },
            state.view.px_per_s,
            if state.previewing { "  PLAYING" } else { "" }
        ),
        format!(
            "speed {:.0} %  {}",
            state.speed * 100.0,
            match (state.loop_region, state.looping) {
                (Some(r), true) => format!("loop {:.2}-{:.2} s", r.start, r.end),
                (Some(_), false) => "loop off".to_owned(),
                (None, _) => "no loop".to_owned(),
            }
        ),
        format!(
            "notes {}  undo {}",
            state.notes().len(),
            state.session.undo_depth()
        ),
        String::new(),
    ];
    match state.hover {
        Some(Hit::Lane { time, lane }) => {
            let placed = state.snap_with(time, false);
            lines.push(format!(
                "here {} {:.3} s  lane {} {}",
                position(placed),
                placed,
                lane + 1,
                LANE_NAMES[lane as usize]
            ));
        }
        Some(Hit::Ruler { time }) => {
            lines.push(format!("ruler {} {:.3} s", position(time), time));
        }
        _ => lines.push(String::new()),
    }
    lines.push(String::new());
    let selected: Vec<_> = state
        .notes()
        .iter()
        .filter(|note| state.is_selected(note))
        .collect();
    match selected.as_slice() {
        [] => lines.push("nothing selected".to_owned()),
        [note] => {
            lines.push("SELECTED".to_owned());
            lines.push(format!("  time   {:.4} s", note.time));
            lines.push(format!("         {}", position(note.time)));
            lines.push(format!(
                "  lane   {} {}",
                note.lane + 1,
                LANE_NAMES[note.lane as usize]
            ));
            lines.push(format!("  length {:.3} s", note.len));
            lines.push(format!("  hopo   {}", if note.hopo { "yes" } else { "no" }));
        }
        many => {
            let first = many.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
            let last = many
                .iter()
                .map(|n| n.time)
                .fold(f64::NEG_INFINITY, f64::max);
            lines.push(format!("SELECTED {} notes", many.len()));
            lines.push(format!("  from {first:.3} s to {last:.3} s"));
        }
    }
    if let Some((field, text)) = &state.field {
        lines.push(String::new());
        lines.push(format!("{}: {text}_", field.label()));
    }
    lines.push(String::new());
    lines.push(state.status.clone());
    let line = lines.join("\n");
    if text.0 != line {
        text.0 = line;
    }
}
