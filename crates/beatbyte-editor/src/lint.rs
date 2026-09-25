//! What looks wrong in a chart: the editor's warnings.
//!
//! Not the validation (which refuses a chart outright) — the things a
//! chart may contain and still be wrong to play: a note under another
//! note's tail on the same lane, a length that makes no sense, a note
//! the music has already stopped for. Each warning names a time (and a
//! lane), so the editor can jump to it.

use beatbyte_chart::ChartNote;

/// Notes on one lane closer than this are stacked — no hand plays
/// them as two.
pub const STACKED_S: f64 = 0.03;

/// A length beyond this is almost certainly a slip, not a held note.
pub const LONGEST_HOLD_S: f64 = 30.0;

/// What is wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A note starts while the previous note on its lane still holds.
    UnderTail {
        /// Where the earlier note's tail ends.
        tail_end: f64,
    },
    /// Two notes on one lane closer than [`STACKED_S`].
    Stacked,
    /// A length that is negative, not a number, or longer than
    /// [`LONGEST_HOLD_S`].
    BadLength,
    /// The note (or its tail) lies after the music has ended.
    PastTheMusic {
        /// Where the music ends.
        end: f64,
    },
    /// Before the song starts.
    BeforeTheSong,
}

/// One warning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warning {
    /// Where.
    pub time: f64,
    /// On which lane.
    pub lane: u8,
    /// What.
    pub kind: Kind,
}

impl Warning {
    /// One line for the information block.
    #[must_use]
    pub fn describe(&self) -> String {
        let what = match self.kind {
            Kind::UnderTail { tail_end } => {
                format!("starts under a held note (it ends at {tail_end:.3} s)")
            }
            Kind::Stacked => "stacked on another note".to_owned(),
            Kind::BadLength => "length makes no sense".to_owned(),
            Kind::PastTheMusic { end } => format!("after the music ends ({end:.2} s)"),
            Kind::BeforeTheSong => "before the song starts".to_owned(),
        };
        format!("{:.3} s lane {}: {what}", self.time, self.lane + 1)
    }
}

/// Every warning for a difficulty's notes, in time order. `sounding_end`
/// is where the music actually ends (not the file's length — a rip
/// can carry a minute of silence), when known.
#[must_use]
pub fn lint(notes: &[ChartNote], sounding_end: Option<f64>) -> Vec<Warning> {
    let mut sorted: Vec<ChartNote> = notes.to_vec();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.lane.cmp(&b.lane)));
    let mut out = Vec::new();
    let mut last_on_lane: [Option<ChartNote>; 256] = [None; 256];
    for note in &sorted {
        let warn = |kind| Warning {
            time: note.time,
            lane: note.lane,
            kind,
        };
        if note.time < 0.0 {
            out.push(warn(Kind::BeforeTheSong));
        }
        if !note.len.is_finite() || note.len < 0.0 || note.len > LONGEST_HOLD_S {
            out.push(warn(Kind::BadLength));
        }
        if let Some(end) = sounding_end
            && note.time + note.len.max(0.0) > end + 0.05
        {
            out.push(warn(Kind::PastTheMusic { end }));
        }
        if let Some(previous) = last_on_lane[usize::from(note.lane)] {
            let gap = note.time - previous.time;
            let tail_end = previous.time + previous.len.max(0.0);
            if gap < STACKED_S {
                out.push(warn(Kind::Stacked));
            } else if previous.len > 0.0 && note.time < tail_end - 1e-6 {
                out.push(warn(Kind::UnderTail { tail_end }));
            }
        }
        last_on_lane[usize::from(note.lane)] = Some(*note);
    }
    out
}

/// The first warning after `time` (wrapping to the first), for "next
/// warning".
#[must_use]
pub fn next_after(warnings: &[Warning], time: f64) -> Option<Warning> {
    warnings
        .iter()
        .copied()
        .find(|w| w.time > time + 1e-6)
        .or_else(|| warnings.first().copied())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn note(time: f64, lane: u8, len: f64) -> ChartNote {
        ChartNote {
            time,
            lane,
            len,
            hopo: false,
        }
    }

    #[test]
    fn a_clean_chart_has_no_warnings() {
        let notes = [note(1.0, 0, 0.5), note(1.6, 0, 0.0), note(1.0, 1, 0.0)];
        assert!(lint(&notes, Some(10.0)).is_empty());
    }

    /// A note under another's tail on the SAME lane is flagged; on
    /// another lane it is a chord and fine.
    #[test]
    fn a_note_under_a_tail_is_flagged() {
        let notes = [note(1.0, 0, 1.0), note(1.5, 0, 0.0), note(1.5, 1, 0.0)];
        let warnings = lint(&notes, None);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].lane, 0);
        assert!(
            matches!(warnings[0].kind, Kind::UnderTail { tail_end } if (tail_end - 2.0).abs() < 1e-9)
        );
    }

    #[test]
    fn stacked_notes_are_flagged() {
        let warnings = lint(&[note(1.0, 2, 0.0), note(1.02, 2, 0.0)], None);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, Kind::Stacked);
    }

    #[test]
    fn a_senseless_length_is_flagged() {
        for len in [-0.5, f64::NAN, 45.0] {
            let warnings = lint(&[note(1.0, 0, len)], None);
            assert!(warnings.iter().any(|w| w.kind == Kind::BadLength), "{len}");
        }
    }

    /// Past the music: the note, or only its tail — measured against
    /// where the music ends, with a little slack.
    #[test]
    fn a_note_after_the_music_is_flagged() {
        let notes = [note(9.0, 0, 0.0), note(9.5, 1, 1.0), note(11.0, 2, 0.0)];
        let warnings = lint(&notes, Some(10.0));
        let flagged: Vec<u8> = warnings.iter().map(|w| w.lane).collect();
        assert_eq!(flagged, vec![1, 2]);
        assert!(
            lint(&notes, None).is_empty(),
            "an unknown end flags nothing"
        );
        assert!(lint(&[note(-0.1, 0, 0.0)], None)[0].kind == Kind::BeforeTheSong);
    }

    #[test]
    fn next_warning_wraps_around() {
        let warnings = lint(
            &[
                note(1.0, 0, 0.0),
                note(1.01, 0, 0.0),
                note(5.0, 1, 0.0),
                note(5.01, 1, 0.0),
            ],
            None,
        );
        assert_eq!(warnings.len(), 2);
        assert!((next_after(&warnings, 0.0).unwrap().time - 1.01).abs() < 1e-9);
        assert!((next_after(&warnings, 2.0).unwrap().time - 5.01).abs() < 1e-9);
        assert!(
            (next_after(&warnings, 9.0).unwrap().time - 1.01).abs() < 1e-9,
            "wraps"
        );
        assert!(next_after(&[], 0.0).is_none());
        assert!(warnings[0].describe().contains("lane 1"));
    }
}
