//! The mouse in the editor: place, select, drag, length, box, scrub,
//! scroll and zoom.
//!
//! Every decision is [`beatbyte_editor::view`]'s — what is under the
//! pointer, what a drag means as operations; this system only reads
//! the pointer, asks, and applies.
//!
//! - click in a lane: a note at the snapped time (Alt: exact), selected;
//! - click a note: select it (Shift adds / removes); drag it: move the
//!   selection (one undo step);
//! - drag the tab above a note: its length;
//! - drag in empty lane space: box selection (Shift adds);
//! - right-click: the menu (delete, HOPO, copy, cut, paste here,
//!   duplicate, star phrase, type time / lane / length) on the note
//!   under the pointer, or on empty space (the selection cleared);
//! - click or drag the ruler / waveform: move the playhead (the music
//!   follows while it plays); Shift-drag there: a loop region;
//! - wheel: scroll; Cmd/Ctrl + wheel: zoom around the pointer.
//!
//! A pointer over a toolbar chip belongs to the chip, never to the
//! timeline underneath.

use beatbyte_editor::EditOp;
use beatbyte_editor::view::{self, Hit};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::{BOTTOM_Y, Drag, EditorState, TOP_Y, alt_held, command_held, seek, shift_held};
use crate::audio_sys::{GameClock, Music};

/// How far (world units) a press may wander before it is a drag.
const DRAG_SLOP: f32 = 5.0;

/// World units one wheel line scrolls.
const LINE: f64 = 60.0;

/// A pointer position the editor drill injects, in world units — the
/// harness drives the REAL gesture code from here on; only the
/// window-to-world conversion is bypassed.
#[derive(Resource, Debug, Clone, Copy)]
pub struct InjectedPointer(pub Vec2);

/// The pointer in world coordinates, if it is over the window.
fn world_cursor(
    injected: Option<&InjectedPointer>,
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    if let Some(injected) = injected {
        return Some(injected.0);
    }
    let window = windows.single().ok()?;
    let position = window.cursor_position()?;
    let (camera, transform) = cameras.single().ok()?;
    camera.viewport_to_world_2d(transform, position).ok()
}

/// The lane under an x, clamped to the highway (a box or a move that
/// leaves the lanes sideways keeps the outermost one).
fn clamped_lane(x: f64) -> u8 {
    view::lane_at(x).unwrap_or(if x < 0.0 { 0 } else { view::LANES - 1 })
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
#[allow(clippy::too_many_lines)] // one pointer, every gesture, in order
/// Read the pointer and turn gestures into edits (see the module
/// documentation for the full list).
pub fn editor_pointer(
    state: Option<ResMut<EditorState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<MouseWheel>,
    chips: Query<&Interaction, With<crate::ui_kit::ActionChip>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    time: Res<Time>,
    injected: Option<Res<InjectedPointer>>,
) {
    let Some(mut state) = state else {
        wheel.clear();
        return;
    };
    let over_toolbar = injected.is_none() && chips.iter().any(|i| *i != Interaction::None);
    let Some(at) = world_cursor(injected.as_deref(), &windows, &cameras) else {
        wheel.clear();
        return;
    };
    let (x, y) = (f64::from(at.x), f64::from(at.y));
    let in_band = (BOTTOM_Y..=TOP_Y).contains(&y) && !over_toolbar && !state.help;
    let now = time.elapsed_secs_f64();
    let free = alt_held(&keys);
    let shift = shift_held(&keys);

    // Wheel: scroll, or zoom around the pointer with Cmd/Ctrl.
    let mut lines = 0.0f64;
    for event in wheel.read() {
        lines += match event.unit {
            MouseScrollUnit::Line => f64::from(event.y),
            MouseScrollUnit::Pixel => f64::from(event.y) / LINE,
        };
    }
    if lines != 0.0 && !over_toolbar {
        if command_held(&keys) {
            state.view.zoom_around(y, 1.15f64.powf(lines));
        } else {
            state.view.scroll(lines * LINE);
            // The player took the view: stop following until the
            // next play (or F).
            state.follow = false;
        }
        state.dirty_view = true;
    }

    let hit = view::hit_test(state.notes(), &state.view, x, y);
    let time_here = state.view.time_at(y);
    if state.drag.is_none() {
        let hover = in_band.then_some(hit);
        if hover != state.hover {
            state.hover = hover;
            state.dirty_view = true;
        }
    }

    // A click beside an open menu closes it and does nothing else.
    if state.menu.is_some() && buttons.just_pressed(MouseButton::Left) && !over_toolbar {
        state.menu = None;
        state.dirty_view = true;
        return;
    }

    // Press.
    if buttons.just_pressed(MouseButton::Left) && in_band {
        state.dirty_view = true;
        match hit {
            Hit::Handle { time, lane } => {
                state.selection = vec![(time, lane)];
                let end = state
                    .notes()
                    .iter()
                    .find(|n| view::same_note((time, lane), n))
                    .map_or(time, |n| n.time + n.len);
                state.drag = Some(Drag::Length {
                    key: (time, lane),
                    end,
                });
            }
            Hit::Head { time, lane } => {
                let key = (time, lane);
                let selected = state
                    .selection
                    .iter()
                    .any(|k| k.1 == lane && (k.0 - time).abs() <= beatbyte_editor::EDIT_EPSILON_S);
                if shift {
                    if selected {
                        state.selection.retain(|k| {
                            !(k.1 == lane && (k.0 - time).abs() <= beatbyte_editor::EDIT_EPSILON_S)
                        });
                    } else {
                        state.selection.push(key);
                    }
                } else {
                    if !selected {
                        state.selection = vec![key];
                    }
                    state.drag = Some(Drag::Move {
                        anchor: key,
                        press_time: time_here,
                        press_lane: lane,
                        dt: 0.0,
                        dlane: 0,
                    });
                }
            }
            Hit::Lane { time, lane } => {
                state.drag = Some(Drag::Pending { time, lane, at });
            }
            Hit::Ruler { time } if shift => {
                state.drag = Some(Drag::LoopRegion {
                    from: time,
                    to: time,
                });
            }
            Hit::Ruler { time } => {
                state.drag = Some(Drag::Scrub);
                seek(&mut state, &music, &mut game_clock, time, now);
                state.follow = false;
            }
        }
    }

    // Drag.
    if buttons.pressed(MouseButton::Left)
        && let Some(drag) = state.drag
    {
        let next = match drag {
            Drag::Pending {
                time,
                lane,
                at: from,
            } => {
                if from.distance(at) > DRAG_SLOP {
                    Some(Drag::Box {
                        t0: time,
                        l0: lane,
                        t1: time_here,
                        l1: clamped_lane(x),
                        additive: shift,
                    })
                } else {
                    Some(drag)
                }
            }
            Drag::Box {
                t0, l0, additive, ..
            } => Some(Drag::Box {
                t0,
                l0,
                t1: time_here,
                l1: clamped_lane(x),
                additive,
            }),
            Drag::Move {
                anchor,
                press_time,
                press_lane,
                ..
            } => {
                let target = state.snap_with(anchor.0 + (time_here - press_time), free);
                Some(Drag::Move {
                    anchor,
                    press_time,
                    press_lane,
                    dt: target - anchor.0,
                    dlane: i32::from(clamped_lane(x)) - i32::from(press_lane),
                })
            }
            Drag::Length { key, .. } => Some(Drag::Length {
                key,
                end: state.snap_with(time_here, free),
            }),
            Drag::Scrub => {
                seek(&mut state, &music, &mut game_clock, time_here, now);
                Some(Drag::Scrub)
            }
            Drag::LoopRegion { from, .. } => Some(Drag::LoopRegion {
                from,
                to: state.snap_with(time_here, free),
            }),
        };
        if next != state.drag {
            state.drag = next;
            state.dirty_view = true;
        }
    }

    // Release: the drag becomes an edit.
    if buttons.just_released(MouseButton::Left)
        && let Some(drag) = state.drag.take()
    {
        state.dirty_view = true;
        let difficulty = state.session.difficulty;
        match drag {
            Drag::Pending { time, lane, .. } => {
                let placed = state.snap_with(time, free);
                let note = beatbyte_chart::ChartNote {
                    time: placed,
                    lane,
                    len: 0.0,
                    hopo: false,
                };
                if state.apply(vec![EditOp::AddNote { difficulty, note }], "note placed") {
                    state.selection = vec![(placed, lane)];
                }
            }
            Drag::Box {
                t0,
                l0,
                t1,
                l1,
                additive,
            } => {
                let picked = view::in_box(state.notes(), (t0, t1), (l0, l1));
                if additive {
                    for key in picked {
                        if !state.selection.contains(&key) {
                            state.selection.push(key);
                        }
                    }
                } else {
                    state.selection = picked;
                }
                state.status = format!("{} note(s) selected", state.selection.len());
            }
            Drag::Move { dt, dlane, .. } => {
                if dt.abs() > 1e-9 || dlane != 0 {
                    let selection = state.selection.clone();
                    match view::plan_move(difficulty, state.notes(), &selection, dt, dlane) {
                        Some((ops, moved)) => {
                            if state.apply(ops, "moved") {
                                state.selection = moved;
                            }
                        }
                        None => state.status = "cannot move there".to_owned(),
                    }
                }
            }
            Drag::Length { key, end } => {
                let note = state
                    .notes()
                    .iter()
                    .find(|n| view::same_note(key, n))
                    .copied();
                if let Some(op) = note.and_then(|n| view::plan_length(difficulty, &n, end)) {
                    state.apply(vec![op], "length set");
                }
            }
            Drag::Scrub => {}
            Drag::LoopRegion { from, to } => {
                let from = state.snap_with(from, free);
                match beatbyte_editor::playback::LoopRegion::between(from, to) {
                    Some(region) => {
                        state.loop_region = Some(region);
                        state.looping = true;
                        state.status = format!("loop {:.3} - {:.3} s", region.start, region.end);
                    }
                    None => state.status = "a loop needs some length".to_owned(),
                }
            }
        }
    }

    // Right-click: the menu, on the note under the pointer (selected
    // with it) or on empty space (the selection cleared).
    if buttons.just_pressed(MouseButton::Right) && in_band {
        state.dirty_view = true;
        let target = match hit {
            Hit::Head { time, lane } | Hit::Handle { time, lane } => {
                let key = (time, lane);
                let in_selection = state
                    .notes()
                    .iter()
                    .find(|n| view::same_note(key, n))
                    .is_some_and(|n| state.is_selected(n));
                if !in_selection {
                    state.selection = vec![key];
                }
                Some(key)
            }
            Hit::Lane { .. } | Hit::Ruler { .. } => {
                state.selection.clear();
                None
            }
        };
        state.menu = Some(super::Menu {
            at,
            time: time_here,
            target,
        });
    }
}
