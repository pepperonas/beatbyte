//! Invertible edit operations on chart files.
//!
//! Every operation applies to a [`ChartFile`] and returns its own
//! inverse — undo/redo falls out of the design instead of being
//! bolted on. Operations are strict: editing a note that is not there
//! is an error, not a silent no-op, so stacks never desynchronize.

use beatbyte_chart::{ChartFile, ChartNote};
use beatbyte_core::Difficulty;
use thiserror::Error;

/// Notes closer than this (same lane) are "the same note" for editing.
pub const EDIT_EPSILON_S: f64 = 0.003;

/// An edit operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditOp {
    /// Insert a note.
    AddNote {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The note to insert.
        note: ChartNote,
    },
    /// Remove a note (the full note is stored so undo can restore it).
    RemoveNote {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The note to remove.
        note: ChartNote,
    },
    /// Flip a note's HOPO flag.
    ToggleHopo {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The note's time.
        time: f64,
        /// The note's lane.
        lane: u8,
    },
    /// Change a note's sustain length (stores the old one for undo).
    SetLen {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The note's time.
        time: f64,
        /// The note's lane.
        lane: u8,
        /// New sustain length in seconds.
        len: f64,
        /// Previous sustain length (filled by `apply` when inverting).
        previous: f64,
    },
}

/// Errors applying an edit.
#[derive(Debug, Error, PartialEq)]
pub enum EditError {
    /// The difficulty is not present in the chart file.
    #[error("chart has no `{0}` difficulty")]
    MissingDifficulty(Difficulty),
    /// A note already exists at that time and lane.
    #[error("a note already exists at {time:.3}s lane {lane}")]
    Occupied {
        /// The conflicting time.
        time: f64,
        /// The conflicting lane.
        lane: u8,
    },
    /// No note exists at that time and lane.
    #[error("no note at {time:.3}s lane {lane}")]
    NoteNotFound {
        /// The searched time.
        time: f64,
        /// The searched lane.
        lane: u8,
    },
}

/// Find the index of a note at (time, lane) within the edit epsilon.
fn find_note(notes: &[ChartNote], time: f64, lane: u8) -> Option<usize> {
    notes
        .iter()
        .position(|note| note.lane == lane && (note.time - time).abs() <= EDIT_EPSILON_S)
}

/// Apply an operation, returning its inverse.
pub fn apply(chart: &mut ChartFile, op: EditOp) -> Result<EditOp, EditError> {
    let difficulty = match op {
        EditOp::AddNote { difficulty, .. }
        | EditOp::RemoveNote { difficulty, .. }
        | EditOp::ToggleHopo { difficulty, .. }
        | EditOp::SetLen { difficulty, .. } => difficulty,
    };
    let def = chart
        .charts
        .iter_mut()
        .find(|def| def.difficulty == difficulty)
        .ok_or(EditError::MissingDifficulty(difficulty))?;

    match op {
        EditOp::AddNote { note, .. } => {
            if find_note(&def.notes, note.time, note.lane).is_some() {
                return Err(EditError::Occupied {
                    time: note.time,
                    lane: note.lane,
                });
            }
            def.notes.push(note);
            def.notes.sort_by(|a, b| a.time.total_cmp(&b.time));
            Ok(EditOp::RemoveNote { difficulty, note })
        }
        EditOp::RemoveNote { note, .. } => {
            let index =
                find_note(&def.notes, note.time, note.lane).ok_or(EditError::NoteNotFound {
                    time: note.time,
                    lane: note.lane,
                })?;
            let removed = def.notes.remove(index);
            Ok(EditOp::AddNote {
                difficulty,
                note: removed,
            })
        }
        EditOp::ToggleHopo { time, lane, .. } => {
            let index =
                find_note(&def.notes, time, lane).ok_or(EditError::NoteNotFound { time, lane })?;
            def.notes[index].hopo = !def.notes[index].hopo;
            Ok(op)
        }
        EditOp::SetLen {
            time, lane, len, ..
        } => {
            let index =
                find_note(&def.notes, time, lane).ok_or(EditError::NoteNotFound { time, lane })?;
            let previous = def.notes[index].len;
            def.notes[index].len = len.max(0.0);
            Ok(EditOp::SetLen {
                difficulty,
                time,
                lane,
                len: previous,
                previous: len,
            })
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use beatbyte_chart::{ChartDef, SongMeta};

    fn chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Edit Me".into(),
                artist: "Tests".into(),
                audio: "a.ogg".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Expert,
                lanes: 5,
                notes: vec![ChartNote {
                    time: 1.0,
                    lane: 2,
                    len: 0.0,
                    hopo: false,
                }],
                phrases: vec![],
            }],
        }
    }

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    #[test]
    fn add_then_inverse_removes() {
        let mut c = chart();
        let inverse = apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Expert,
                note: note(2.0, 0),
            },
        )
        .unwrap();
        assert_eq!(c.charts[0].notes.len(), 2);
        apply(&mut c, inverse).unwrap();
        assert_eq!(c.charts[0].notes.len(), 1);
    }

    #[test]
    fn add_keeps_notes_sorted() {
        let mut c = chart();
        apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Expert,
                note: note(0.5, 0),
            },
        )
        .unwrap();
        let times: Vec<f64> = c.charts[0].notes.iter().map(|n| n.time).collect();
        assert_eq!(times, vec![0.5, 1.0]);
    }

    #[test]
    fn add_on_occupied_cell_fails() {
        let mut c = chart();
        let result = apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Expert,
                note: note(1.0005, 2),
            },
        );
        assert!(matches!(result, Err(EditError::Occupied { .. })));
    }

    #[test]
    fn remove_missing_note_fails() {
        let mut c = chart();
        let result = apply(
            &mut c,
            EditOp::RemoveNote {
                difficulty: Difficulty::Expert,
                note: note(9.0, 4),
            },
        );
        assert!(matches!(result, Err(EditError::NoteNotFound { .. })));
    }

    #[test]
    fn toggle_hopo_is_its_own_inverse() {
        let mut c = chart();
        let op = EditOp::ToggleHopo {
            difficulty: Difficulty::Expert,
            time: 1.0,
            lane: 2,
        };
        let inverse = apply(&mut c, op).unwrap();
        assert!(c.charts[0].notes[0].hopo);
        apply(&mut c, inverse).unwrap();
        assert!(!c.charts[0].notes[0].hopo);
    }

    #[test]
    fn set_len_inverse_restores() {
        let mut c = chart();
        let inverse = apply(
            &mut c,
            EditOp::SetLen {
                difficulty: Difficulty::Expert,
                time: 1.0,
                lane: 2,
                len: 1.5,
                previous: 0.0,
            },
        )
        .unwrap();
        assert!((c.charts[0].notes[0].len - 1.5).abs() < 1e-9);
        apply(&mut c, inverse).unwrap();
        assert!((c.charts[0].notes[0].len - 0.0).abs() < 1e-9);
    }

    #[test]
    fn missing_difficulty_fails() {
        let mut c = chart();
        let result = apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Easy,
                note: note(2.0, 0),
            },
        );
        assert_eq!(result, Err(EditError::MissingDifficulty(Difficulty::Easy)));
    }
}
