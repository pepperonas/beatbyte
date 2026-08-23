//! The editor session: a chart being edited, with undo/redo.

use beatbyte_chart::{ChartFile, Severity};
use beatbyte_core::Difficulty;

use crate::ops::{EditError, EditOp, apply};

/// A chart under edit.
#[derive(Debug, Clone)]
pub struct EditorSession {
    chart: ChartFile,
    /// The difficulty currently being edited.
    pub difficulty: Difficulty,
    undo: Vec<EditOp>,
    redo: Vec<EditOp>,
    dirty: bool,
}

impl EditorSession {
    /// Start editing a chart at a difficulty it actually contains.
    pub fn new(chart: ChartFile, difficulty: Difficulty) -> Result<EditorSession, EditError> {
        if chart.chart_for(difficulty).is_none() {
            return Err(EditError::MissingDifficulty(difficulty));
        }
        Ok(EditorSession {
            chart,
            difficulty,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        })
    }

    /// The chart in its current state.
    #[must_use]
    pub fn chart(&self) -> &ChartFile {
        &self.chart
    }

    /// Whether there are unsaved edits.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the session saved.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Apply an edit (clears the redo stack).
    pub fn edit(&mut self, op: EditOp) -> Result<(), EditError> {
        let inverse = apply(&mut self.chart, op)?;
        self.undo.push(inverse);
        self.redo.clear();
        self.dirty = true;
        Ok(())
    }

    /// Undo the last edit. Returns whether anything happened.
    pub fn undo(&mut self) -> bool {
        let Some(inverse) = self.undo.pop() else {
            return false;
        };
        match apply(&mut self.chart, inverse) {
            Ok(redo_op) => {
                self.redo.push(redo_op);
                self.dirty = true;
                true
            }
            // An inverse failing means the stacks desynchronized — a
            // bug worth surfacing loudly in tests, but never a crash.
            Err(_) => false,
        }
    }

    /// Redo the last undone edit. Returns whether anything happened.
    pub fn redo(&mut self) -> bool {
        let Some(op) = self.redo.pop() else {
            return false;
        };
        match apply(&mut self.chart, op) {
            Ok(inverse) => {
                self.undo.push(inverse);
                self.dirty = true;
                true
            }
            Err(_) => false,
        }
    }

    /// Depth of the undo stack.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Whether the chart currently validates cleanly (no errors).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self
            .chart
            .validate()
            .iter()
            .any(|issue| issue.severity == Severity::Error)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use beatbyte_chart::{ChartDef, ChartNote, SongMeta};

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
                notes: vec![],
                phrases: vec![],
            }],
        }
    }

    fn add(time: f64, lane: u8) -> EditOp {
        EditOp::AddNote {
            difficulty: Difficulty::Expert,
            note: ChartNote {
                time,
                lane,
                len: 0.0,
                hopo: false,
            },
        }
    }

    #[test]
    fn wrong_difficulty_is_rejected_up_front() {
        assert!(EditorSession::new(chart(), Difficulty::Easy).is_err());
    }

    #[test]
    fn edit_undo_redo_round_trip() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session.edit(add(1.0, 0)).unwrap();
        session.edit(add(2.0, 1)).unwrap();
        assert_eq!(session.chart().charts[0].notes.len(), 2);
        assert!(session.dirty());

        assert!(session.undo());
        assert_eq!(session.chart().charts[0].notes.len(), 1);
        assert!(session.undo());
        assert_eq!(session.chart().charts[0].notes.len(), 0);
        assert!(!session.undo(), "empty stack undoes nothing");

        assert!(session.redo());
        assert!(session.redo());
        assert_eq!(session.chart().charts[0].notes.len(), 2);
        assert!(!session.redo());
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session.edit(add(1.0, 0)).unwrap();
        session.undo();
        session.edit(add(3.0, 2)).unwrap();
        assert!(!session.redo(), "redo history is gone after a new edit");
    }

    #[test]
    fn failed_edits_do_not_touch_the_stacks() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session.edit(add(1.0, 0)).unwrap();
        assert!(session.edit(add(1.0, 0)).is_err());
        assert_eq!(session.undo_depth(), 1);
    }

    #[test]
    fn saved_state_tracks_dirtiness() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session.edit(add(1.0, 0)).unwrap();
        session.mark_saved();
        assert!(!session.dirty());
        session.undo();
        assert!(session.dirty());
    }

    #[test]
    fn validity_follows_the_edits() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        assert!(session.is_valid());
        session.edit(add(1.0, 0)).unwrap();
        assert!(session.is_valid());
    }
}
