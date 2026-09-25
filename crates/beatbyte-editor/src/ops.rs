//! Invertible edit operations on chart files.
//!
//! Every operation applies to a [`ChartFile`] and returns its own
//! inverse — undo/redo falls out of the design instead of being
//! bolted on. Operations are strict: editing a note that is not there
//! is an error, not a silent no-op, so stacks never desynchronize.

use beatbyte_chart::{ChartDef, ChartFile, ChartNote, ChartPhrase};
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
    /// Move a note to a new time and/or lane (keeps its sustain and
    /// HOPO flag).
    MoveNote {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The note's current time.
        from_time: f64,
        /// The note's current lane.
        from_lane: u8,
        /// The destination time.
        to_time: f64,
        /// The destination lane.
        to_lane: u8,
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
    /// Insert a star-power phrase (a time range).
    AddPhrase {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The phrase to insert.
        phrase: ChartPhrase,
    },
    /// Remove a star-power phrase (matched by both bounds).
    RemovePhrase {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The phrase to remove.
        phrase: ChartPhrase,
    },
    /// Change a phrase's bounds.
    SetPhrase {
        /// The difficulty chart to edit.
        difficulty: Difficulty,
        /// The phrase as it is now.
        from: ChartPhrase,
        /// The phrase as it should be.
        to: ChartPhrase,
    },
    /// Add an empty chart for a difficulty the file does not carry —
    /// so a song can be charted from nothing at that level.
    AddDifficulty {
        /// The difficulty to add.
        difficulty: Difficulty,
    },
    /// Remove a difficulty's chart — only an EMPTY one (the inverse of
    /// [`EditOp::AddDifficulty`]); a chart with notes is never dropped
    /// by one keystroke.
    RemoveEmptyDifficulty {
        /// The difficulty to remove.
        difficulty: Difficulty,
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
    /// No phrase with these bounds exists.
    #[error("no phrase at {start:.3}–{end:.3}s")]
    PhraseNotFound {
        /// The searched start.
        start: f64,
        /// The searched end.
        end: f64,
    },
    /// A phrase's end is not after its start.
    #[error("a phrase must end after it starts ({start:.3}–{end:.3}s)")]
    EmptyPhrase {
        /// The start.
        start: f64,
        /// The end.
        end: f64,
    },
    /// The chart already has that difficulty.
    #[error("chart already has `{0}`")]
    DifficultyExists(Difficulty),
    /// Only an empty difficulty may be removed.
    #[error("`{0}` still has notes or phrases")]
    DifficultyNotEmpty(Difficulty),
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

/// Find a phrase by both bounds within the edit epsilon.
fn find_phrase(phrases: &[ChartPhrase], phrase: ChartPhrase) -> Option<usize> {
    phrases.iter().position(|p| {
        (p.start - phrase.start).abs() <= EDIT_EPSILON_S
            && (p.end - phrase.end).abs() <= EDIT_EPSILON_S
    })
}

fn sort_phrases(phrases: &mut [ChartPhrase]) {
    phrases.sort_by(|a, b| a.start.total_cmp(&b.start));
}

/// Apply an operation, returning its inverse.
#[allow(clippy::too_many_lines)] // one match, one arm per operation
pub fn apply(chart: &mut ChartFile, op: EditOp) -> Result<EditOp, EditError> {
    // The two operations on whole difficulties come first: they are
    // the ones that do not start from an existing chart.
    match op {
        EditOp::AddDifficulty { difficulty } => {
            if chart.chart_for(difficulty).is_some() {
                return Err(EditError::DifficultyExists(difficulty));
            }
            chart.charts.push(ChartDef {
                difficulty,
                lanes: 5,
                notes: Vec::new(),
                phrases: Vec::new(),
            });
            chart.charts.sort_by_key(|def| def.difficulty);
            return Ok(EditOp::RemoveEmptyDifficulty { difficulty });
        }
        EditOp::RemoveEmptyDifficulty { difficulty } => {
            let index = chart
                .charts
                .iter()
                .position(|def| def.difficulty == difficulty)
                .ok_or(EditError::MissingDifficulty(difficulty))?;
            let def = &chart.charts[index];
            if !def.notes.is_empty() || !def.phrases.is_empty() {
                return Err(EditError::DifficultyNotEmpty(difficulty));
            }
            chart.charts.remove(index);
            return Ok(EditOp::AddDifficulty { difficulty });
        }
        _ => {}
    }
    let difficulty = match op {
        EditOp::AddNote { difficulty, .. }
        | EditOp::RemoveNote { difficulty, .. }
        | EditOp::ToggleHopo { difficulty, .. }
        | EditOp::MoveNote { difficulty, .. }
        | EditOp::SetLen { difficulty, .. }
        | EditOp::AddPhrase { difficulty, .. }
        | EditOp::RemovePhrase { difficulty, .. }
        | EditOp::SetPhrase { difficulty, .. }
        | EditOp::AddDifficulty { difficulty }
        | EditOp::RemoveEmptyDifficulty { difficulty } => difficulty,
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
        EditOp::MoveNote {
            from_time,
            from_lane,
            to_time,
            to_lane,
            ..
        } => {
            let index =
                find_note(&def.notes, from_time, from_lane).ok_or(EditError::NoteNotFound {
                    time: from_time,
                    lane: from_lane,
                })?;
            // The destination must be free — unless it is the note
            // itself (a nudge within the epsilon is a no-op, not a
            // collision).
            if let Some(occupied) = find_note(&def.notes, to_time, to_lane)
                && occupied != index
            {
                return Err(EditError::Occupied {
                    time: to_time,
                    lane: to_lane,
                });
            }
            def.notes[index].time = to_time;
            def.notes[index].lane = to_lane;
            def.notes.sort_by(|a, b| a.time.total_cmp(&b.time));
            Ok(EditOp::MoveNote {
                difficulty,
                from_time: to_time,
                from_lane: to_lane,
                to_time: from_time,
                to_lane: from_lane,
            })
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
        EditOp::AddPhrase { phrase, .. } => {
            if phrase.end <= phrase.start {
                return Err(EditError::EmptyPhrase {
                    start: phrase.start,
                    end: phrase.end,
                });
            }
            def.phrases.push(phrase);
            sort_phrases(&mut def.phrases);
            Ok(EditOp::RemovePhrase { difficulty, phrase })
        }
        EditOp::RemovePhrase { phrase, .. } => {
            let index = find_phrase(&def.phrases, phrase).ok_or(EditError::PhraseNotFound {
                start: phrase.start,
                end: phrase.end,
            })?;
            let removed = def.phrases.remove(index);
            Ok(EditOp::AddPhrase {
                difficulty,
                phrase: removed,
            })
        }
        EditOp::SetPhrase { from, to, .. } => {
            if to.end <= to.start {
                return Err(EditError::EmptyPhrase {
                    start: to.start,
                    end: to.end,
                });
            }
            let index = find_phrase(&def.phrases, from).ok_or(EditError::PhraseNotFound {
                start: from.start,
                end: from.end,
            })?;
            let was = def.phrases[index];
            def.phrases[index] = to;
            sort_phrases(&mut def.phrases);
            Ok(EditOp::SetPhrase {
                difficulty,
                from: to,
                to: was,
            })
        }
        // Handled above, before a chart is looked up.
        EditOp::AddDifficulty { .. } | EditOp::RemoveEmptyDifficulty { .. } => Ok(op),
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
                genre: None,
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
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
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
    fn move_then_inverse_restores_exactly() {
        let mut c = chart();
        // Give the note a sustain and HOPO so the move must carry them.
        c.charts[0].notes[0].len = 0.8;
        c.charts[0].notes[0].hopo = true;
        let before = c.clone();
        let inverse = apply(
            &mut c,
            EditOp::MoveNote {
                difficulty: Difficulty::Expert,
                from_time: 1.0,
                from_lane: 2,
                to_time: 2.5,
                to_lane: 4,
            },
        )
        .unwrap();
        assert!((c.charts[0].notes[0].time - 2.5).abs() < 1e-9);
        assert_eq!(c.charts[0].notes[0].lane, 4);
        assert!((c.charts[0].notes[0].len - 0.8).abs() < 1e-9);
        assert!(c.charts[0].notes[0].hopo);
        apply(&mut c, inverse).unwrap();
        assert_eq!(c, before, "inverse must restore the exact chart");
    }

    #[test]
    fn move_keeps_notes_sorted() {
        let mut c = chart();
        apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Expert,
                note: note(2.0, 0),
            },
        )
        .unwrap();
        apply(
            &mut c,
            EditOp::MoveNote {
                difficulty: Difficulty::Expert,
                from_time: 2.0,
                from_lane: 0,
                to_time: 0.5,
                to_lane: 0,
            },
        )
        .unwrap();
        let times: Vec<f64> = c.charts[0].notes.iter().map(|n| n.time).collect();
        assert_eq!(times, vec![0.5, 1.0]);
    }

    #[test]
    fn move_onto_another_note_fails() {
        let mut c = chart();
        apply(
            &mut c,
            EditOp::AddNote {
                difficulty: Difficulty::Expert,
                note: note(2.0, 2),
            },
        )
        .unwrap();
        let result = apply(
            &mut c,
            EditOp::MoveNote {
                difficulty: Difficulty::Expert,
                from_time: 2.0,
                from_lane: 2,
                to_time: 1.0005,
                to_lane: 2,
            },
        );
        assert!(matches!(result, Err(EditError::Occupied { .. })));
    }

    #[test]
    fn move_of_missing_note_fails() {
        let mut c = chart();
        let result = apply(
            &mut c,
            EditOp::MoveNote {
                difficulty: Difficulty::Expert,
                from_time: 9.0,
                from_lane: 0,
                to_time: 3.0,
                to_lane: 0,
            },
        );
        assert!(matches!(result, Err(EditError::NoteNotFound { .. })));
    }

    fn phrase(start: f64, end: f64) -> ChartPhrase {
        ChartPhrase { start, end }
    }

    /// Every phrase operation undoes EXACTLY: apply, invert, compare.
    #[test]
    fn phrase_operations_invert_exactly() {
        let mut c = chart();
        let before = c.clone();
        let add = EditOp::AddPhrase {
            difficulty: Difficulty::Expert,
            phrase: phrase(4.0, 8.0),
        };
        let undo_add = apply(&mut c, add).unwrap();
        assert_eq!(c.charts[0].phrases, vec![phrase(4.0, 8.0)]);
        let with_one = c.clone();
        let undo_set = apply(
            &mut c,
            EditOp::SetPhrase {
                difficulty: Difficulty::Expert,
                from: phrase(4.0, 8.0),
                to: phrase(5.0, 9.5),
            },
        )
        .unwrap();
        assert_eq!(c.charts[0].phrases, vec![phrase(5.0, 9.5)]);
        apply(&mut c, undo_set).unwrap();
        assert_eq!(c, with_one);
        let undo_remove = apply(
            &mut c,
            EditOp::RemovePhrase {
                difficulty: Difficulty::Expert,
                phrase: phrase(4.0, 8.0),
            },
        )
        .unwrap();
        assert!(c.charts[0].phrases.is_empty());
        apply(&mut c, undo_remove).unwrap();
        assert_eq!(c, with_one);
        apply(&mut c, undo_add).unwrap();
        assert_eq!(c, before);
    }

    #[test]
    fn an_empty_phrase_is_refused() {
        let mut c = chart();
        let result = apply(
            &mut c,
            EditOp::AddPhrase {
                difficulty: Difficulty::Expert,
                phrase: phrase(4.0, 4.0),
            },
        );
        assert!(matches!(result, Err(EditError::EmptyPhrase { .. })));
        assert!(c.charts[0].phrases.is_empty());
    }

    /// A difficulty can be added empty and removed again — but a chart
    /// with notes is never removed.
    #[test]
    fn a_difficulty_is_added_empty_and_only_removed_empty() {
        let mut c = chart();
        let before = c.clone();
        let undo = apply(
            &mut c,
            EditOp::AddDifficulty {
                difficulty: Difficulty::Easy,
            },
        )
        .unwrap();
        let easy = c.chart_for(Difficulty::Easy).unwrap();
        assert!(easy.notes.is_empty() && easy.lanes == 5);
        assert_eq!(
            c.charts[0].difficulty,
            Difficulty::Easy,
            "kept in difficulty order"
        );
        assert!(matches!(
            apply(
                &mut c,
                EditOp::AddDifficulty {
                    difficulty: Difficulty::Easy
                }
            ),
            Err(EditError::DifficultyExists(_))
        ));
        apply(&mut c, undo).unwrap();
        assert_eq!(c, before);
        assert!(matches!(
            apply(
                &mut c,
                EditOp::RemoveEmptyDifficulty {
                    difficulty: Difficulty::Expert
                }
            ),
            Err(EditError::DifficultyNotEmpty(_))
        ));
        assert_eq!(c, before, "a refused removal changed the chart");
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
