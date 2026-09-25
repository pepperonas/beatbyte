//! The editor's geometry: where a time and a lane are on screen, what
//! the pointer is over, and what a drag means as edit operations.
//!
//! Pure — no engine — so the mouse rules are tested without one. The
//! game draws with these numbers and hands the pointer back to them;
//! a note drawn in one place and hit in another cannot happen,
//! because both ask the same [`View`].
//!
//! The timeline runs UP the screen like the highway (later = higher):
//! `y = anchor_y + (t − center_s) · px_per_s`.

use beatbyte_chart::ChartNote;
use beatbyte_core::Difficulty;

use crate::ops::{EDIT_EPSILON_S, EditOp};

/// Width of one lane column, world units.
pub const LANE_WIDTH: f64 = 76.0;

/// Number of lanes.
pub const LANES: u8 = 5;

/// How far (world units, vertically) from a note head a press still
/// hits it.
pub const HEAD_REACH: f64 = 14.0;

/// The length handle: a tab from the top of a note's tail up this far.
pub const HANDLE_REACH: f64 = 10.0;

/// Zoom limits, world units per second. At the top end one
/// millisecond is 2.4 units — a single stroke sits where it is put.
pub const MIN_PX_PER_S: f64 = 30.0;
/// See [`MIN_PX_PER_S`].
pub const MAX_PX_PER_S: f64 = 2400.0;

/// The x of a lane's centre (lanes 0–4 around x = 0).
#[must_use]
pub fn lane_x(lane: u8) -> f64 {
    (f64::from(lane) - 2.0) * LANE_WIDTH
}

/// The lane under an x, if any.
#[must_use]
pub fn lane_at(x: f64) -> Option<u8> {
    let index = (x / LANE_WIDTH + 2.5).floor();
    (0.0..f64::from(LANES))
        .contains(&index)
        .then_some(index as u8)
}

/// The vertical mapping between time and screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The time drawn at `anchor_y`.
    pub center_s: f64,
    /// Zoom: world units per second.
    pub px_per_s: f64,
    /// The screen y the centre time is drawn at.
    pub anchor_y: f64,
}

impl View {
    /// The screen y of a time.
    #[must_use]
    pub fn y_of(&self, time: f64) -> f64 {
        self.anchor_y + (time - self.center_s) * self.px_per_s
    }

    /// The time at a screen y.
    #[must_use]
    pub fn time_at(&self, y: f64) -> f64 {
        self.center_s + (y - self.anchor_y) / self.px_per_s
    }

    /// Scroll by a distance on screen (positive = later).
    pub fn scroll(&mut self, dy: f64) {
        self.center_s += dy / self.px_per_s;
    }

    /// Zoom by `factor`, keeping the time under `pivot_y` where it is
    /// — the point under the mouse stays under the mouse.
    pub fn zoom_around(&mut self, pivot_y: f64, factor: f64) {
        let pivot = self.time_at(pivot_y);
        self.px_per_s = (self.px_per_s * factor).clamp(MIN_PX_PER_S, MAX_PX_PER_S);
        // Solve y_of(pivot) == pivot_y for the centre.
        self.center_s = pivot - (pivot_y - self.anchor_y) / self.px_per_s;
    }

    /// Scroll just enough that `time` lies between `bottom_y` and
    /// `top_y` (with `margin`). Returns whether the view moved.
    pub fn keep_visible(&mut self, time: f64, bottom_y: f64, top_y: f64, margin: f64) -> bool {
        let y = self.y_of(time);
        if y < bottom_y + margin {
            self.scroll(y - (bottom_y + margin));
            true
        } else if y > top_y - margin {
            self.scroll(y - (top_y - margin));
            true
        } else {
            false
        }
    }
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// A note head (the note's own time and lane).
    Head {
        /// The note's time.
        time: f64,
        /// Its lane.
        lane: u8,
    },
    /// The length handle at the top of a note's tail.
    Handle {
        /// The note's time.
        time: f64,
        /// Its lane.
        lane: u8,
    },
    /// Empty space in a lane at this (unsnapped) time.
    Lane {
        /// The pointer's time.
        time: f64,
        /// The lane.
        lane: u8,
    },
    /// Outside the lanes (the ruler, the waveform) at this time.
    Ruler {
        /// The pointer's time.
        time: f64,
    },
}

/// What is under the pointer at world `(x, y)`. Handles win over
/// heads (a handle is only drawn where the head is not), heads win
/// over empty lane; among heads the nearest.
#[must_use]
pub fn hit_test(notes: &[ChartNote], view: &View, x: f64, y: f64) -> Hit {
    let time = view.time_at(y);
    let Some(lane) = lane_at(x) else {
        return Hit::Ruler { time };
    };
    let mut best: Option<(f64, Hit)> = None;
    for note in notes.iter().filter(|note| note.lane == lane) {
        let head = view.y_of(note.time);
        let top = view
            .y_of(note.time + note.len.max(0.0))
            .max(head + HEAD_REACH);
        // The handle: a tab above the tail's end.
        if y > top && y <= top + HANDLE_REACH {
            let distance = y - top;
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((
                    distance,
                    Hit::Handle {
                        time: note.time,
                        lane,
                    },
                ));
            }
            continue;
        }
        let distance = (y - head).abs();
        if distance <= HEAD_REACH && best.is_none_or(|(d, _)| distance < d) {
            best = Some((
                distance,
                Hit::Head {
                    time: note.time,
                    lane,
                },
            ));
        }
    }
    best.map_or(Hit::Lane { time, lane }, |(_, hit)| hit)
}

/// A note's key: its time and lane (what every op finds it by).
pub type NoteKey = (f64, u8);

/// Whether a key names this note.
#[must_use]
pub fn same_note(key: NoteKey, note: &ChartNote) -> bool {
    key.1 == note.lane && (key.0 - note.time).abs() <= EDIT_EPSILON_S
}

/// The notes inside a box: times `[t0, t1]` (either order), lanes
/// `[l0, l1]` (either order).
#[must_use]
pub fn in_box(notes: &[ChartNote], times: (f64, f64), lanes: (u8, u8)) -> Vec<NoteKey> {
    let (t0, t1) = if times.0 <= times.1 {
        times
    } else {
        (times.1, times.0)
    };
    let (l0, l1) = if lanes.0 <= lanes.1 {
        lanes
    } else {
        (lanes.1, lanes.0)
    };
    notes
        .iter()
        .filter(|n| n.time >= t0 - EDIT_EPSILON_S && n.time <= t1 + EDIT_EPSILON_S)
        .filter(|n| (l0..=l1).contains(&n.lane))
        .map(|n| (n.time, n.lane))
        .collect()
}

/// The operations that move a selection by `dt` seconds and `dlane`
/// lanes, as ONE batch: every selected note removed, then re-added at
/// its new place. Removing all first means a note may move onto
/// another selected note's old spot (a group shifted by one step)
/// without a collision; a spot held by a note OUTSIDE the selection
/// still refuses the whole batch.
///
/// `None` when a note would leave the lanes or start before zero —
/// the drag is then not applied at all, rather than squashed.
#[must_use]
pub fn plan_move(
    difficulty: Difficulty,
    notes: &[ChartNote],
    selection: &[NoteKey],
    dt: f64,
    dlane: i32,
) -> Option<(Vec<EditOp>, Vec<NoteKey>)> {
    let chosen: Vec<ChartNote> = notes
        .iter()
        .filter(|note| selection.iter().any(|key| same_note(*key, note)))
        .copied()
        .collect();
    if chosen.is_empty() || (dt == 0.0 && dlane == 0) {
        return None;
    }
    let mut ops = Vec::with_capacity(chosen.len() * 2);
    let mut moved = Vec::with_capacity(chosen.len());
    let mut added = Vec::with_capacity(chosen.len());
    for note in &chosen {
        let lane = i32::from(note.lane) + dlane;
        if !(0..i32::from(LANES)).contains(&lane) || note.time + dt < 0.0 {
            return None;
        }
        ops.push(EditOp::RemoveNote {
            difficulty,
            note: *note,
        });
        let mut placed = *note;
        placed.time = note.time + dt;
        placed.lane = lane as u8;
        added.push(EditOp::AddNote {
            difficulty,
            note: placed,
        });
        moved.push((placed.time, placed.lane));
    }
    ops.extend(added);
    Some((ops, moved))
}

/// The operation that makes a note's tail end at `end_time` (never
/// shorter than zero; `None` when nothing changes).
#[must_use]
pub fn plan_length(difficulty: Difficulty, note: &ChartNote, end_time: f64) -> Option<EditOp> {
    let len = (end_time - note.time).max(0.0);
    // Below a millisecond a tail is a tap.
    let len = if len < 0.001 { 0.0 } else { len };
    ((len - note.len).abs() > 1e-9).then_some(EditOp::SetLen {
        difficulty,
        time: note.time,
        lane: note.lane,
        len,
        previous: note.len,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::EditorSession;
    use beatbyte_chart::{ChartDef, ChartFile, SongMeta};

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    fn view() -> View {
        View {
            center_s: 10.0,
            px_per_s: 100.0,
            anchor_y: -200.0,
        }
    }

    #[test]
    fn time_and_screen_convert_both_ways() {
        let v = view();
        assert!((v.y_of(10.0) + 200.0).abs() < 1e-9);
        assert!((v.y_of(11.0) + 100.0).abs() < 1e-9, "later is higher");
        for t in [0.0, 9.75, 10.0, 123.456] {
            assert!((v.time_at(v.y_of(t)) - t).abs() < 1e-9);
        }
    }

    /// The time under the mouse stays under the mouse through a zoom.
    #[test]
    fn a_zoom_keeps_the_pivot_in_place() {
        let mut v = view();
        let pivot_y = 37.0;
        let before = v.time_at(pivot_y);
        v.zoom_around(pivot_y, 4.0);
        assert!((v.px_per_s - 400.0).abs() < 1e-9);
        assert!((v.time_at(pivot_y) - before).abs() < 1e-9);
        v.zoom_around(pivot_y, 1000.0);
        assert!((v.px_per_s - MAX_PX_PER_S).abs() < 1e-9, "clamped");
        assert!(
            (v.time_at(pivot_y) - before).abs() < 1e-9,
            "still pinned when clamped"
        );
    }

    #[test]
    fn keep_visible_scrolls_only_when_needed() {
        let mut v = view();
        assert!(!v.keep_visible(10.5, -300.0, 300.0, 20.0));
        assert!(v.keep_visible(20.0, -300.0, 300.0, 20.0));
        assert!((v.y_of(20.0) - 280.0).abs() < 1e-9);
        assert!(v.keep_visible(0.0, -300.0, 300.0, 20.0));
        assert!((v.y_of(0.0) + 280.0).abs() < 1e-9);
    }

    #[test]
    fn lanes_are_found_from_x() {
        for lane in 0..LANES {
            assert_eq!(lane_at(lane_x(lane)), Some(lane));
            assert_eq!(lane_at(lane_x(lane) + LANE_WIDTH * 0.49), Some(lane));
        }
        assert_eq!(lane_at(lane_x(0) - LANE_WIDTH * 0.51), None);
        assert_eq!(lane_at(lane_x(4) + LANE_WIDTH * 0.51), None);
    }

    /// A head, a handle, empty lane and the ruler — each where it is
    /// drawn.
    #[test]
    fn the_pointer_hits_what_is_drawn_there() {
        let v = view();
        let mut sustained = note(11.0, 3);
        sustained.len = 1.0;
        let notes = [note(10.0, 1), sustained];
        let y = |t: f64| v.y_of(t);
        assert_eq!(
            hit_test(&notes, &v, lane_x(1), y(10.0) + 5.0),
            Hit::Head {
                time: 10.0,
                lane: 1
            }
        );
        assert!(matches!(
            hit_test(&notes, &v, lane_x(1), y(10.0) + 40.0),
            Hit::Lane { lane: 1, .. }
        ));
        // Handle above the tail's end at 12.0.
        assert_eq!(
            hit_test(&notes, &v, lane_x(3), y(12.0) + 4.0),
            Hit::Handle {
                time: 11.0,
                lane: 3
            }
        );
        // A tap note's handle sits just above its head.
        assert_eq!(
            hit_test(&notes, &v, lane_x(1), y(10.0) + HEAD_REACH + 4.0),
            Hit::Handle {
                time: 10.0,
                lane: 1
            }
        );
        assert!(matches!(
            hit_test(&notes, &v, -300.0, y(10.0)),
            Hit::Ruler { .. }
        ));
    }

    /// The nearer of two close heads wins.
    #[test]
    fn the_nearest_head_wins() {
        let mut v = view();
        v.px_per_s = 30.0;
        let notes = [note(10.0, 2), note(10.5, 2)];
        let at = v.y_of(10.4);
        assert_eq!(
            hit_test(&notes, &v, lane_x(2), at),
            Hit::Head {
                time: 10.5,
                lane: 2
            }
        );
    }

    #[test]
    fn a_box_selects_in_either_direction() {
        let notes = [note(1.0, 0), note(2.0, 1), note(2.0, 3), note(3.0, 4)];
        let mut picked = in_box(&notes, (2.5, 0.5), (3, 0));
        picked.sort_by_key(|key| key.1);
        assert_eq!(picked, vec![(1.0, 0), (2.0, 1), (2.0, 3)]);
    }

    fn chart(notes: Vec<ChartNote>) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "T".into(),
                artist: "A".into(),
                audio: "a.ogg".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: None,
                genre: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Expert,
                lanes: 5,
                notes,
                phrases: vec![],
            }],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        }
    }

    /// A group shifted by one step onto its own old spots moves as a
    /// whole and undoes as ONE step, lengths and HOPO carried.
    #[test]
    fn a_group_moves_onto_its_own_old_spots() {
        let mut first = note(1.0, 0);
        first.len = 0.5;
        first.hopo = true;
        let notes = vec![first, note(1.5, 0), note(2.0, 0)];
        let mut session = EditorSession::new(chart(notes.clone()), Difficulty::Expert).unwrap();
        let selection = vec![(1.0, 0), (1.5, 0)];
        let (ops, moved) = plan_move(Difficulty::Expert, &notes, &selection, 0.5, 0).unwrap();
        // 1.5 moves onto 2.0 — a note OUTSIDE the selection: refused.
        assert!(session.edit_batch(ops).is_err());
        assert_eq!(
            session.chart().charts[0].notes,
            notes,
            "a refused move left residue"
        );

        let selection = vec![(1.0, 0), (1.5, 0), (2.0, 0)];
        let (ops, moved_all) = plan_move(Difficulty::Expert, &notes, &selection, 0.5, 1).unwrap();
        session.edit_batch(ops).unwrap();
        let after = &session.chart().charts[0].notes;
        assert_eq!(
            after.iter().map(|n| (n.time, n.lane)).collect::<Vec<_>>(),
            moved_all
        );
        assert!((after[0].len - 0.5).abs() < 1e-9 && after[0].hopo);
        assert_eq!(session.undo_depth(), 1);
        session.undo();
        assert_eq!(session.chart().charts[0].notes, notes);
        assert_eq!(moved.len(), 2);
    }

    #[test]
    fn a_move_out_of_the_lanes_or_before_zero_is_not_planned() {
        let notes = vec![note(1.0, 4), note(0.2, 0)];
        assert!(plan_move(Difficulty::Expert, &notes, &[(1.0, 4)], 0.0, 1).is_none());
        assert!(plan_move(Difficulty::Expert, &notes, &[(0.2, 0)], -0.3, 0).is_none());
        assert!(plan_move(Difficulty::Expert, &notes, &[(0.2, 0)], 0.0, 0).is_none());
        assert!(plan_move(Difficulty::Expert, &notes, &[(9.0, 0)], 1.0, 0).is_none());
    }

    #[test]
    fn a_length_drag_sets_the_tail_and_never_goes_negative() {
        let n = note(2.0, 1);
        let Some(EditOp::SetLen { len, previous, .. }) = plan_length(Difficulty::Expert, &n, 2.75)
        else {
            panic!("a set-length op")
        };
        assert!((len - 0.75).abs() < 1e-9 && previous == 0.0);
        // Dragged below its head, a sustain becomes a tap.
        let mut held = n;
        held.len = 0.5;
        let Some(EditOp::SetLen { len, .. }) = plan_length(Difficulty::Expert, &held, 1.0) else {
            panic!("dragging below the head makes a tap")
        };
        assert!(len == 0.0);
        assert!(
            plan_length(Difficulty::Expert, &n, 1.0).is_none(),
            "already a tap: no op"
        );
        assert!(plan_length(Difficulty::Expert, &n, 2.0005).is_none());
    }
}
