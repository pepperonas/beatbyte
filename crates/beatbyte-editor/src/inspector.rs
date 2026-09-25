//! The note inspector's fields: a typed value → the operation.
//!
//! The time field takes seconds (`12.345`) or a musical position
//! (`3:2:096`, bar:beat:tick over the song's grid); the lane field
//! `1`–`5`; the length field seconds (`0` = a tap). What the player
//! typed is either an exact edit or a message that says what was
//! wrong — never a guess.

use beatbyte_chart::ChartNote;
use beatbyte_core::Difficulty;

use crate::ops::EditOp;
use crate::timecode::{Grid, Position};
use crate::view::LANES;

/// A field of the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The note's time.
    Time,
    /// Its lane.
    Lane,
    /// Its length.
    Length,
}

impl Field {
    /// The field's name in the editor.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Field::Time => "time (s or bar:beat:tick)",
            Field::Lane => "lane (1-5)",
            Field::Length => "length (s, 0 = tap)",
        }
    }

    /// What the field shows before anything is typed.
    #[must_use]
    pub fn current(self, note: &ChartNote) -> String {
        match self {
            Field::Time => format!("{:.4}", note.time),
            Field::Lane => (note.lane + 1).to_string(),
            Field::Length => format!("{:.3}", note.len),
        }
    }

    /// Whether a typed character belongs in this field.
    #[must_use]
    pub fn accepts(self, c: char) -> bool {
        match self {
            Field::Time => c.is_ascii_digit() || c == '.' || c == ':',
            Field::Lane => c.is_ascii_digit(),
            Field::Length => c.is_ascii_digit() || c == '.',
        }
    }
}

/// Parse a time: seconds, or `bar:beat:tick` / `bar:beat` over the grid.
///
/// # Errors
/// A message that says what is wrong with the text.
pub fn parse_time(text: &str, grid: Option<&Grid>) -> Result<f64, String> {
    let text = text.trim();
    if text.contains(':') {
        let parts: Vec<&str> = text.split(':').collect();
        let number = |s: &str| {
            s.parse::<i64>()
                .map_err(|_| format!("`{s}` is not a number"))
        };
        let (bar, beat, tick) = match parts.as_slice() {
            [bar, beat] => (number(bar)?, number(beat)?, 0),
            [bar, beat, tick] => (number(bar)?, number(beat)?, number(tick)?),
            _ => return Err("a position is bar:beat or bar:beat:tick".to_owned()),
        };
        let grid = grid.ok_or("this song has no beat grid - type seconds")?;
        let position = Position {
            bar,
            beat: u32::try_from(beat).map_err(|_| "the beat must be 1 or more")?,
            tick: u32::try_from(tick).map_err(|_| "the tick must be 0 or more")?,
        };
        return grid
            .time_of(position)
            .filter(|t| *t >= 0.0)
            .ok_or_else(|| format!("{position} is not a place in this song"));
    }
    let seconds: f64 = text
        .parse()
        .map_err(|_| format!("`{text}` is not a time"))?;
    if seconds.is_finite() && seconds >= 0.0 {
        Ok(seconds)
    } else {
        Err("a time is 0 or more seconds".to_owned())
    }
}

/// The operation a typed value asks for on `note`; `Ok(None)` when it
/// changes nothing.
///
/// # Errors
/// A message that says what is wrong with the text.
pub fn apply_field(
    difficulty: Difficulty,
    note: &ChartNote,
    field: Field,
    text: &str,
    grid: Option<&Grid>,
) -> Result<Option<EditOp>, String> {
    match field {
        Field::Time => {
            let time = parse_time(text, grid)?;
            Ok(
                ((time - note.time).abs() > 1e-9).then_some(EditOp::MoveNote {
                    difficulty,
                    from_time: note.time,
                    from_lane: note.lane,
                    to_time: time,
                    to_lane: note.lane,
                }),
            )
        }
        Field::Lane => {
            let lane: u8 = text
                .trim()
                .parse()
                .map_err(|_| format!("`{}` is not a lane", text.trim()))?;
            if !(1..=LANES).contains(&lane) {
                return Err(format!("lanes are 1 to {LANES}"));
            }
            let lane = lane - 1;
            Ok((lane != note.lane).then_some(EditOp::MoveNote {
                difficulty,
                from_time: note.time,
                from_lane: note.lane,
                to_time: note.time,
                to_lane: lane,
            }))
        }
        Field::Length => {
            let len: f64 = text
                .trim()
                .parse()
                .map_err(|_| format!("`{}` is not a length", text.trim()))?;
            if !len.is_finite() || len < 0.0 {
                return Err("a length is 0 or more seconds".to_owned());
            }
            Ok(((len - note.len).abs() > 1e-9).then_some(EditOp::SetLen {
                difficulty,
                time: note.time,
                lane: note.lane,
                len,
                previous: note.len,
            }))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        let marks: Vec<(f64, bool)> = (0..32)
            .map(|i| (1.0 + f64::from(i) * 0.5, i % 4 == 0))
            .collect();
        Grid::from_marks(&marks).unwrap()
    }

    fn note() -> ChartNote {
        ChartNote {
            time: 2.0,
            lane: 1,
            len: 0.0,
            hopo: false,
        }
    }

    #[test]
    fn a_time_is_seconds_or_a_position() {
        let g = grid();
        assert!((parse_time("12.345", Some(&g)).unwrap() - 12.345).abs() < 1e-12);
        assert!((parse_time("2:1", Some(&g)).unwrap() - 3.0).abs() < 1e-9);
        assert!((parse_time("1:3:096", Some(&g)).unwrap() - 2.25).abs() < 1e-9);
        assert!(parse_time("2:5", Some(&g)).is_err(), "no fifth beat");
        assert!(parse_time("1:2", None).is_err(), "no grid");
        assert!(parse_time("-1", Some(&g)).is_err());
        assert!(parse_time("abc", Some(&g)).is_err());
        assert!(parse_time("1:2:3:4", Some(&g)).is_err());
    }

    #[test]
    fn each_field_becomes_its_operation() {
        let n = note();
        let d = Difficulty::Expert;
        let g = grid();
        assert_eq!(
            apply_field(d, &n, Field::Time, "2.5", Some(&g)).unwrap(),
            Some(EditOp::MoveNote {
                difficulty: d,
                from_time: 2.0,
                from_lane: 1,
                to_time: 2.5,
                to_lane: 1
            })
        );
        assert_eq!(
            apply_field(d, &n, Field::Lane, "5", Some(&g)).unwrap(),
            Some(EditOp::MoveNote {
                difficulty: d,
                from_time: 2.0,
                from_lane: 1,
                to_time: 2.0,
                to_lane: 4
            })
        );
        assert_eq!(
            apply_field(d, &n, Field::Length, "0.75", Some(&g)).unwrap(),
            Some(EditOp::SetLen {
                difficulty: d,
                time: 2.0,
                lane: 1,
                len: 0.75,
                previous: 0.0
            })
        );
        // The value it already has: nothing to do.
        assert_eq!(
            apply_field(d, &n, Field::Lane, "2", Some(&g)).unwrap(),
            None
        );
        assert_eq!(
            apply_field(d, &n, Field::Time, "2.0", Some(&g)).unwrap(),
            None
        );
    }

    #[test]
    fn a_bad_value_says_what_is_wrong() {
        let n = note();
        let d = Difficulty::Expert;
        assert!(
            apply_field(d, &n, Field::Lane, "6", None)
                .unwrap_err()
                .contains("1 to 5")
        );
        assert!(apply_field(d, &n, Field::Lane, "0", None).is_err());
        assert!(apply_field(d, &n, Field::Length, "-1", None).is_err());
        assert!(apply_field(d, &n, Field::Length, "x", None).is_err());
    }

    #[test]
    fn the_fields_take_only_their_characters() {
        assert!(Field::Time.accepts(':') && Field::Time.accepts('.'));
        assert!(!Field::Lane.accepts('.') && !Field::Length.accepts(':'));
        assert_eq!(Field::Time.current(&note()), "2.0000");
        assert_eq!(Field::Lane.current(&note()), "2");
    }
}
