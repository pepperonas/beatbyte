//! Copy, paste, duplicate — and star-power phrases over a selection.
//!
//! A clip is notes relative to the first one, so it lands wherever it
//! is pasted with its rhythm intact: lengths, lanes, HOPO flags and the
//! spacing between notes carried exactly. Pasting is a batch of adds
//! — one undo step, and all or nothing: a clip that would land on a
//! note already there is refused whole, never half-pasted.

use beatbyte_chart::{ChartNote, ChartPhrase};
use beatbyte_core::Difficulty;

use crate::ops::EditOp;
use crate::view::{NoteKey, same_note};

/// Copied notes, relative to the first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clip {
    notes: Vec<ChartNote>,
}

impl Clip {
    /// The selected notes of `notes` as a clip.
    #[must_use]
    pub fn copy(notes: &[ChartNote], selection: &[NoteKey]) -> Clip {
        let mut chosen: Vec<ChartNote> = notes
            .iter()
            .filter(|note| selection.iter().any(|key| same_note(*key, note)))
            .copied()
            .collect();
        chosen.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.lane.cmp(&b.lane)));
        let Some(first) = chosen.first().map(|n| n.time) else {
            return Clip::default();
        };
        for note in &mut chosen {
            note.time -= first;
        }
        Clip { notes: chosen }
    }

    /// Whether there is nothing to paste.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// How many notes it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// The time from its first note to its last note's end.
    #[must_use]
    pub fn span(&self) -> f64 {
        self.notes
            .iter()
            .map(|n| n.time + n.len)
            .fold(0.0, f64::max)
    }

    /// The adds that paste it with its first note at `at`, and the
    /// keys of the pasted notes (the new selection).
    #[must_use]
    pub fn paste(&self, difficulty: Difficulty, at: f64) -> (Vec<EditOp>, Vec<NoteKey>) {
        let mut ops = Vec::with_capacity(self.notes.len());
        let mut keys = Vec::with_capacity(self.notes.len());
        for note in &self.notes {
            let mut placed = *note;
            placed.time = at + note.time;
            keys.push((placed.time, placed.lane));
            ops.push(EditOp::AddNote {
                difficulty,
                note: placed,
            });
        }
        (ops, keys)
    }
}

/// The time span of a selection: first note to last note's end.
#[must_use]
pub fn selection_span(notes: &[ChartNote], selection: &[NoteKey]) -> Option<(f64, f64)> {
    notes
        .iter()
        .filter(|note| selection.iter().any(|key| same_note(*key, note)))
        .fold(None, |span, note| {
            let end = note.time + note.len;
            Some(match span {
                None => (note.time, end),
                Some((lo, hi)) => (f64::min(lo, note.time), f64::max(hi, end)),
            })
        })
}

/// A star-power phrase over `span`, as ONE step: phrases it overlaps
/// are replaced by it (a phrase is a stretch of the song, and two of
/// them overlapping is a validation error). A span of a single tap
/// note still makes a phrase, of [`MIN_PHRASE_S`].
#[must_use]
pub fn phrase_over(
    difficulty: Difficulty,
    phrases: &[ChartPhrase],
    span: (f64, f64),
) -> Vec<EditOp> {
    let (start, end) = (span.0, span.1.max(span.0 + MIN_PHRASE_S));
    let mut ops: Vec<EditOp> = phrases
        .iter()
        .filter(|p| p.start <= end && p.end >= start)
        .map(|phrase| EditOp::RemovePhrase {
            difficulty,
            phrase: *phrase,
        })
        .collect();
    ops.push(EditOp::AddPhrase {
        difficulty,
        phrase: ChartPhrase { start, end },
    });
    ops
}

/// A phrase over one tap note lasts at least this long.
pub const MIN_PHRASE_S: f64 = 0.05;

/// The phrase at a time, if one covers it.
#[must_use]
pub fn phrase_at(phrases: &[ChartPhrase], time: f64) -> Option<ChartPhrase> {
    phrases
        .iter()
        .copied()
        .find(|p| time >= p.start - 1e-6 && time <= p.end + 1e-6)
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

    fn session(notes: Vec<ChartNote>, phrases: Vec<ChartPhrase>) -> EditorSession {
        let chart = ChartFile {
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
                phrases,
            }],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        };
        EditorSession::new(chart, Difficulty::Expert).unwrap()
    }

    fn notes(session: &EditorSession) -> Vec<ChartNote> {
        session.chart().charts[0].notes.clone()
    }

    /// A clip keeps its rhythm, lanes, lengths and HOPO flags, lands
    /// where it is pasted, and is one undo step.
    #[test]
    fn a_paste_carries_the_rhythm_exactly() {
        let mut held = note(1.25, 3);
        held.len = 0.5;
        held.hopo = true;
        let mut s = session(vec![note(1.0, 0), held, note(1.0, 2)], vec![]);
        let clip = Clip::copy(&notes(&s), &[(1.0, 0), (1.25, 3), (1.0, 2)]);
        assert_eq!(clip.len(), 3);
        assert!((clip.span() - 0.75).abs() < 1e-9);
        let (ops, keys) = clip.paste(Difficulty::Expert, 4.0);
        s.edit_batch(ops).unwrap();
        assert_eq!(s.undo_depth(), 1);
        let after = notes(&s);
        let pasted: Vec<&ChartNote> = after.iter().filter(|n| n.time >= 4.0).collect();
        assert_eq!(pasted.len(), 3);
        let copy = pasted.iter().find(|n| n.lane == 3).unwrap();
        assert!((copy.time - 4.25).abs() < 1e-9 && (copy.len - 0.5).abs() < 1e-9 && copy.hopo);
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&(4.0, 0)) && keys.contains(&(4.0, 2)));
    }

    /// A clip that would land on an existing note is refused whole.
    #[test]
    fn a_paste_onto_a_note_is_refused_whole() {
        let mut s = session(vec![note(1.0, 0), note(1.5, 1), note(3.5, 1)], vec![]);
        let before = s.chart().clone();
        let clip = Clip::copy(&notes(&s), &[(1.0, 0), (1.5, 1)]);
        let (ops, _) = clip.paste(Difficulty::Expert, 3.0);
        assert!(s.edit_batch(ops).is_err());
        assert_eq!(s.chart(), &before, "half a paste landed");
    }

    #[test]
    fn copying_nothing_is_an_empty_clip() {
        let clip = Clip::copy(&[note(1.0, 0)], &[(9.0, 0)]);
        assert!(clip.is_empty());
        assert!(clip.paste(Difficulty::Expert, 1.0).0.is_empty());
    }

    #[test]
    fn a_selection_span_reaches_the_last_tail() {
        let mut held = note(2.0, 1);
        held.len = 1.5;
        let all = [note(1.0, 0), held, note(3.0, 2)];
        assert_eq!(
            selection_span(&all, &[(1.0, 0), (2.0, 1)]),
            Some((1.0, 3.5))
        );
        assert_eq!(selection_span(&all, &[]), None);
    }

    /// A phrase over a span replaces the phrases it overlaps — one step
    /// — and leaves the others; the result validates.
    #[test]
    fn a_phrase_replaces_what_it_overlaps() {
        let old = [
            ChartPhrase {
                start: 1.0,
                end: 2.0,
            },
            ChartPhrase {
                start: 5.0,
                end: 6.0,
            },
        ];
        let mut s = session(vec![note(1.5, 0), note(3.0, 1)], old.to_vec());
        let ops = phrase_over(Difficulty::Expert, &old, (1.5, 3.0));
        s.edit_batch(ops).unwrap();
        let phrases = &s.chart().charts[0].phrases;
        assert_eq!(
            phrases,
            &vec![
                ChartPhrase {
                    start: 1.5,
                    end: 3.0
                },
                ChartPhrase {
                    start: 5.0,
                    end: 6.0
                }
            ]
        );
        assert!(s.is_valid());
        assert_eq!(s.undo_depth(), 1);
        s.undo();
        assert_eq!(s.chart().charts[0].phrases, old.to_vec());
    }

    #[test]
    fn a_phrase_over_one_tap_has_a_length() {
        let ops = phrase_over(Difficulty::Expert, &[], (2.0, 2.0));
        let Some(EditOp::AddPhrase { phrase, .. }) = ops.last() else {
            panic!("an add")
        };
        assert!(phrase.end > phrase.start);
        assert_eq!(phrase_at(&[*phrase], 2.01), Some(*phrase));
        assert_eq!(phrase_at(&[*phrase], 3.0), None);
    }
}
