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
    /// Undo steps; each entry is a GROUP of inverses (a single edit is
    /// a group of one, a bulk edit one group) applied in reverse.
    undo: Vec<Vec<EditOp>>,
    redo: Vec<Vec<EditOp>>,
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
        self.edit_batch(vec![op])
    }

    /// Apply several edits as ONE undo step — atomically: if any op
    /// fails, everything already applied is rolled back and the chart
    /// is untouched. Bulk operations (delete range, toggle HOPO on a
    /// selection) compose primitives through this, so undo stays
    /// exact.
    pub fn edit_batch(&mut self, ops: Vec<EditOp>) -> Result<(), EditError> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut inverses = Vec::with_capacity(ops.len());
        for op in ops {
            match apply(&mut self.chart, op) {
                Ok(inverse) => inverses.push(inverse),
                Err(error) => {
                    // Roll back what already applied, newest first.
                    for inverse in inverses.into_iter().rev() {
                        // An inverse of a just-applied op re-applies
                        // cleanly by construction.
                        let _ = apply(&mut self.chart, inverse);
                    }
                    return Err(error);
                }
            }
        }
        self.undo.push(inverses);
        self.redo.clear();
        self.dirty = true;
        Ok(())
    }

    /// Undo the last edit step. Returns whether anything happened.
    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.pop() else {
            return false;
        };
        match Self::apply_group(&mut self.chart, group) {
            Ok(redo_group) => {
                self.redo.push(redo_group);
                self.dirty = true;
                true
            }
            // A group failing means the stacks desynchronized — a
            // bug worth surfacing loudly in tests, but never a crash.
            Err(_) => false,
        }
    }

    /// Redo the last undone edit step. Returns whether anything
    /// happened.
    pub fn redo(&mut self) -> bool {
        let Some(group) = self.redo.pop() else {
            return false;
        };
        match Self::apply_group(&mut self.chart, group) {
            Ok(undo_group) => {
                self.undo.push(undo_group);
                self.dirty = true;
                true
            }
            Err(_) => false,
        }
    }

    /// Apply a stored group newest-first; the produced ops form the
    /// opposite-direction group. Applying THAT reversed again yields
    /// the original order, so undo and redo stay symmetric.
    fn apply_group(chart: &mut ChartFile, group: Vec<EditOp>) -> Result<Vec<EditOp>, EditError> {
        let mut produced = Vec::with_capacity(group.len());
        for op in group.into_iter().rev() {
            produced.push(apply(chart, op)?);
        }
        Ok(produced)
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
    fn batch_is_one_undo_step_and_atomic() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session
            .edit_batch(vec![add(1.0, 0), add(2.0, 1), add(3.0, 2)])
            .unwrap();
        assert_eq!(session.chart().charts[0].notes.len(), 3);
        assert_eq!(session.undo_depth(), 1, "a batch is ONE step");
        assert!(session.undo());
        assert_eq!(session.chart().charts[0].notes.len(), 0);
        assert!(session.redo());
        assert_eq!(session.chart().charts[0].notes.len(), 3);

        // Atomicity: a failing op in the middle leaves NOTHING behind.
        let before = session.chart().clone();
        let result = session.edit_batch(vec![
            add(5.0, 0),
            add(1.0, 0), // occupied — the batch must fail
            add(6.0, 0),
        ]);
        assert!(result.is_err());
        assert_eq!(session.chart(), &before, "failed batch left residue");
        assert_eq!(session.undo_depth(), 1, "failed batch pushed a step");
    }

    #[test]
    fn empty_batch_is_a_no_op() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        session.edit_batch(vec![]).unwrap();
        assert_eq!(session.undo_depth(), 0);
        assert!(!session.dirty());
    }

    #[test]
    fn validity_follows_the_edits() {
        let mut session = EditorSession::new(chart(), Difficulty::Expert).unwrap();
        assert!(session.is_valid());
        session.edit(add(1.0, 0)).unwrap();
        assert!(session.is_valid());
    }
}
