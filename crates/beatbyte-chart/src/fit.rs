//! How well a chart sits on its song — the objective half of the
//! judgement.
//!
//! The ear decides whether a chart is fun (ADR-0009), and nothing
//! here changes that. But two questions about a chart have answers
//! that do not need an ear, and both were asked by hand before this
//! module existed:
//!
//! * **Is every note on the grid?** The generator quantizes to the
//!   beat grid, but [`BeatGrid::quantize`] returns the RAW time when
//!   no subdivision is within [`SNAP_TOLERANCE_S`] — so a chart can
//!   silently be a mixture. Measured 2026-09-14 over the library's
//!   guitar-study and normal charts: 100 % on the grid, which is how
//!   that suspicion was retired.
//! * **Does a note fall where the recording has an attack?** A chart
//!   built from a separated stem can place a note the mix never
//!   sounds — right in time, and still not something a player hears.
//!   Measured on Nothing Else Matters (medium): the stem-charted
//!   pilot 56 %, the mix-charted default 62.9 %.
//!
//! Pure: analysis and chart in, numbers out.

use beatbyte_core::music::Onset;

use crate::grid::{BeatGrid, SNAP_TOLERANCE_S};
use crate::schema::ChartNote;

/// A note counts as sitting on the grid when quantizing moves it less
/// than this. Not a tolerance for judging — a bookkeeping epsilon, so
/// float noise from a rewrite does not read as "off the grid".
pub const ON_GRID_EPSILON_S: f64 = 0.001;

/// How far a note may be from an onset and still be that onset's
/// note. Deliberately [`SNAP_TOLERANCE_S`]: the project already calls
/// that distance "the same note rather than a different one".
pub const ATTACK_TOLERANCE_S: f64 = SNAP_TOLERANCE_S;

/// Beyond this a nearest onset says nothing about the note, so it is
/// left out of the signed statistic instead of dragging it.
pub const NEIGHBOURHOOD_S: f64 = 0.25;

/// What a chart's notes do against the grid and the recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartFit {
    /// Notes inside the measured window.
    pub notes: usize,
    /// Share of them sitting on a grid subdivision, 0..1. `None` when
    /// the chart carries no grid to sit on.
    pub on_grid: Option<f64>,
    /// Share with a detected onset within [`ATTACK_TOLERANCE_S`],
    /// 0..1 — "a player hears something there".
    pub with_attack: f64,
    /// Median signed distance to the nearest onset within
    /// [`NEIGHBOURHOOD_S`], seconds; positive = the note is charted
    /// LATER than the attack. `None` when no note had one.
    pub median_offset_s: Option<f64>,
    /// Notes per second over the window.
    pub notes_per_s: f64,
    /// Share of notes carrying a sustain, 0..1.
    pub sustain_share: f64,
}

/// Measure the notes inside `[from, to)` against grid and onsets.
#[must_use]
pub fn measure(
    notes: &[ChartNote],
    grid: Option<&BeatGrid>,
    onsets: &[Onset],
    from: f64,
    to: f64,
) -> ChartFit {
    let window: Vec<&ChartNote> = notes
        .iter()
        .filter(|n| n.time >= from && n.time < to)
        .collect();
    let span = (to - from).max(f64::MIN_POSITIVE);
    if window.is_empty() {
        return ChartFit {
            notes: 0,
            on_grid: grid.map(|_| 0.0),
            with_attack: 0.0,
            median_offset_s: None,
            notes_per_s: 0.0,
            sustain_share: 0.0,
        };
    }

    let on_grid = grid.map(|grid| {
        let on = window
            .iter()
            .filter(|n| {
                grid.quantize(n.time, SNAP_TOLERANCE_S)
                    .is_some_and(|snapped| (snapped - n.time).abs() <= ON_GRID_EPSILON_S)
            })
            .count();
        on as f64 / window.len() as f64
    });

    let times: Vec<f64> = onsets.iter().map(|o| o.time_s).collect();
    let deltas: Vec<f64> = window
        .iter()
        .filter_map(|n| nearest_delta(n.time, &times))
        .collect();
    let with_attack = deltas
        .iter()
        .filter(|d| d.abs() <= ATTACK_TOLERANCE_S)
        .count() as f64
        / window.len() as f64;
    let mut near: Vec<f64> = deltas
        .into_iter()
        .filter(|d| d.abs() <= NEIGHBOURHOOD_S)
        .collect();
    near.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_offset_s = near.get(near.len() / 2).copied();

    ChartFit {
        notes: window.len(),
        on_grid,
        with_attack,
        median_offset_s,
        notes_per_s: window.len() as f64 / span,
        sustain_share: window.iter().filter(|n| n.len > 0.0).count() as f64 / window.len() as f64,
    }
}

/// Signed distance from `time` to the nearest entry of an ASCENDING
/// list (positive = `time` is later). `None` for an empty list.
fn nearest_delta(time: f64, ascending: &[f64]) -> Option<f64> {
    if ascending.is_empty() {
        return None;
    }
    let at = ascending.partition_point(|t| *t < time);
    let before = at.checked_sub(1).map(|i| time - ascending[i]);
    let after = ascending.get(at).map(|t| time - *t);
    match (before, after) {
        (Some(b), Some(a)) => Some(if b.abs() <= a.abs() { b } else { a }),
        (Some(b), None) => Some(b),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64) -> ChartNote {
        ChartNote {
            time,
            lane: 0,
            len: 0.0,
            hopo: false,
        }
    }

    fn onset(time_s: f64) -> Onset {
        Onset {
            time_s,
            strength: 1.0,
            brightness: 0.5,
        }
    }

    #[test]
    fn the_nearest_onset_keeps_its_side() {
        let onsets = [1.0, 2.0];
        // Later than the attack reads positive, earlier negative —
        // the same sign convention as a judged hit.
        assert!((nearest_delta(1.02, &onsets).expect("a neighbour") - 0.02).abs() < 1e-9);
        assert!((nearest_delta(0.98, &onsets).expect("a neighbour") + 0.02).abs() < 1e-9);
        // The NEARER one wins even when both exist.
        assert!((nearest_delta(1.9, &onsets).expect("a neighbour") + 0.1).abs() < 1e-9);
        assert_eq!(nearest_delta(1.0, &[]), None);
    }

    #[test]
    fn a_note_without_an_attack_is_counted_as_one() {
        // Three notes: one ON an attack, one a tenth of a second past
        // it — near enough to be THAT attack's note in the signed
        // statistic, far too late to be something the player hears
        // there — and one in silence. The middle note is the whole
        // point: with one tolerance doing both jobs it would count as
        // heard (a mutation swapping them was seen to pass before it
        // existed).
        let notes = [note(1.0), note(2.1), note(5.0)];
        let onsets = [onset(1.0), onset(2.0)];
        let fit = measure(&notes, None, &onsets, 0.0, 10.0);
        assert_eq!(fit.notes, 3);
        assert!(
            (fit.with_attack - 1.0 / 3.0).abs() < 1e-9,
            "only the note ON an attack is heard there: {}",
            fit.with_attack
        );
        assert_eq!(fit.on_grid, None, "no grid, no claim about one");
        // The 5 s note is three seconds from any onset, so it must not
        // drag the signed statistic — that is what the neighbourhood
        // is for; the 0.1 s one does belong in it.
        let median = fit.median_offset_s.expect("a median");
        assert!(
            (median - 0.1).abs() < 1e-9,
            "the median runs over the near ones: {median}"
        );
    }

    #[test]
    fn the_window_bounds_what_is_measured() {
        let notes = [note(1.0), note(2.0), note(9.0)];
        let fit = measure(&notes, None, &[onset(1.0)], 0.0, 3.0);
        assert_eq!(fit.notes, 2, "the 9 s note is outside");
        assert!((fit.notes_per_s - 2.0 / 3.0).abs() < 1e-9);
        let empty = measure(&notes, None, &[], 20.0, 30.0);
        assert_eq!(empty.notes, 0);
        assert_eq!(empty.median_offset_s, None);
        assert_eq!(empty.notes_per_s, 0.0);
    }

    #[test]
    fn sitting_on_the_grid_is_measured_against_the_grid_s_own_rule() {
        // Beats every half second; the generator snaps to subdivisions
        // of that, so 1.0 and 1.25 are on it and 1.12 is not.
        let grid = BeatGrid::from_beats(&[0.0, 0.5, 1.0, 1.5, 2.0]);
        let on = measure(&[note(1.0), note(1.25)], Some(&grid), &[], 0.0, 3.0);
        assert_eq!(on.on_grid, Some(1.0));
        let off = measure(&[note(1.12)], Some(&grid), &[], 0.0, 3.0);
        assert_eq!(
            off.on_grid,
            Some(0.0),
            "0.12 past the beat is no subdivision of a 0.5 s beat"
        );
    }

    #[test]
    fn sustains_are_counted_apart_from_taps() {
        let mut held = note(1.0);
        held.len = 0.8;
        let fit = measure(&[held, note(2.0)], None, &[], 0.0, 3.0);
        assert!((fit.sustain_share - 0.5).abs() < 1e-9);
    }
}
