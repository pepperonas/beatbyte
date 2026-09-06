//! The beat grid a chart carries: the beats as the analysis TRACKED
//! them, one time per beat, and the downbeats when a stage knows
//! them.
//!
//! Format v1 stores one `bpm` and one `offset_s`, and everything that
//! placed a note, drew a bar line or counted a phrase used that
//! constant grid — while the analysis had been tracking a
//! time-varying one since v0.11 (`docs/audio-eval-baseline.md`, Phase
//! 2). Measured on the library (2026-09-06), the two grids part ways
//! by up to **1.61 s** on a live recording (Hotel California, 1977;
//! local tempo 140.6–152.0 BPM) and by 0.19–0.36 s on studio songs —
//! a third of a beat to more than three beats. The notes were snapped
//! to the wrong subdivision, the fret lines ran ahead of the drummer,
//! the phrases counted from an arbitrary beat.
//!
//! So the chart keeps the tracked beats. `bpm` and `offset_s` stay
//! what they were — the median tempo and the first beat, for display
//! and for every reader that predates the grid — and a chart without
//! a grid behaves exactly as before: every method here falls back to
//! the constant grid it replaces.

use beatbyte_core::timing::{TempoChange, TempoMap};
use serde::{Deserialize, Serialize};

use crate::schema::ChartNote;

/// The beats as tracked, and the downbeats where known.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BeatGrid {
    /// Song seconds of every tracked beat, ascending.
    pub beats: Vec<f64>,
    /// Song seconds of the downbeats (bar starts), ascending; each
    /// sits on a beat. Empty when no stage determined them — the bar
    /// is then every fourth beat from the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downbeats: Vec<f64>,
}

/// Beats per bar assumed while no downbeats are known.
pub const BEATS_PER_BAR: usize = 4;

/// Most beats a grid may carry (untrusted input): twenty minutes at
/// 400 BPM is 8 000; this is far above it and far below anything
/// that could hurt.
pub const MAX_GRID_BEATS: usize = 100_000;

/// How a stored beat time is rounded: a tenth of a millisecond, an
/// order under any hit window and enough to keep the file short and
/// its floats free of the load/save one-ULP drift.
pub const BEAT_PRECISION_S: f64 = 1e-4;

/// The subdivisions a time may snap to, simplest first: the beat,
/// then halves, quarters and eighths of it.
///
/// **Binary only, deliberately.** A triplet level was tried and
/// removed: at 120 BPM a triplet-eighth grid passes within 42 ms of a
/// genuine sixteenth, so with any tolerance wide enough to be useful
/// it steals straight notes and turns a sixteenth run into a shuffle.
pub const SNAP_LEVELS: [f64; 4] = [1.0, 2.0, 4.0, 8.0];

/// Under this a snap is float noise, not a move (see
/// [`BeatGrid::snap_notes`]).
pub const SNAP_NOISE_S: f64 = 1e-6;

/// How far a hit may sit from a subdivision and still be the same
/// musical event.
///
/// An absolute time, deliberately NOT a fraction of the beat. Human
/// micro-timing and onset-detector scatter are both well under 60 ms
/// regardless of tempo; a hit further than that from every
/// subdivision is a different note, not a mistimed one. Expressing it
/// as a fraction of the beat is a trap: a quarter of a beat at 120
/// BPM is a whole sixteenth, so adjacent sixteenths collapse onto the
/// beat and a sixteenth-note run disappears (measured).
pub const SNAP_TOLERANCE_S: f64 = 0.055;

impl BeatGrid {
    /// The grid from an analysis's tracked beats, rounded to
    /// [`BEAT_PRECISION_S`]; non-finite times are dropped and the
    /// rest kept ascending.
    #[must_use]
    pub fn from_beats(beats: &[f64]) -> BeatGrid {
        let mut out: Vec<f64> = beats
            .iter()
            .copied()
            .filter(|t| t.is_finite())
            .map(|t| (t / BEAT_PRECISION_S).round() / (1.0 / BEAT_PRECISION_S))
            .collect();
        out.dedup();
        BeatGrid {
            beats: out,
            downbeats: Vec::new(),
        }
    }

    /// Whether the grid can place anything: two beats make an
    /// interval.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.beats.len() >= 2 && self.beats.windows(2).all(|w| w[1] > w[0])
    }

    /// Move every time by `shift_s` (the audio-timeline migration),
    /// never below zero.
    pub fn shift(&mut self, shift_s: f64) {
        for t in self.beats.iter_mut().chain(self.downbeats.iter_mut()) {
            *t = (*t + shift_s).max(0.0);
        }
    }

    /// The index of the beat at or before `time_s` (0 before the
    /// first beat).
    #[must_use]
    pub fn beat_index(&self, time_s: f64) -> usize {
        match self.beats.binary_search_by(|b| b.total_cmp(&time_s)) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }

    /// The interval `[start, end)` the time falls in: the beat at or
    /// before it and the next one. Before the first beat the first
    /// interval is extended backwards, after the last the last is
    /// extended forwards, so every time has a local beat length.
    fn interval(&self, time_s: f64) -> Option<(f64, f64)> {
        if !self.is_usable() {
            return None;
        }
        let last = self.beats.len() - 1;
        let i = self.beat_index(time_s).min(last - 1);
        let (a, b) = (self.beats[i], self.beats[i + 1]);
        Some((a, b))
    }

    /// The local beat length at `time_s`, seconds.
    #[must_use]
    pub fn beat_length_at(&self, time_s: f64) -> Option<f64> {
        self.interval(time_s).map(|(a, b)| b - a)
    }

    /// Snap `time_s` to the simplest subdivision of the local beat
    /// within `tolerance_s` — never wider than half a step, or a
    /// level would claim positions belonging to its neighbours. Times
    /// outside the grid's span snap to the same subdivisions of the
    /// nearest interval, continued. `None` when the grid is unusable.
    #[must_use]
    pub fn quantize(&self, time_s: f64, tolerance_s: f64) -> Option<f64> {
        let (a, b) = self.interval(time_s)?;
        let beat = b - a;
        if beat <= 0.0 {
            return None;
        }
        for division in SNAP_LEVELS {
            let step = beat / division;
            let position = (time_s - a) / step;
            let snapped = a + position.round() * step;
            if (snapped - time_s).abs() <= tolerance_s.min(step * 0.5) {
                return Some(snapped);
            }
        }
        Some(time_s)
    }

    /// The bar a time falls in, counted from the first downbeat when
    /// downbeats are known and from the first beat otherwise; bars
    /// before that are bar 0.
    #[must_use]
    pub fn bar_of(&self, time_s: f64) -> usize {
        if !self.downbeats.is_empty() {
            return match self.downbeats.binary_search_by(|d| d.total_cmp(&time_s)) {
                Ok(i) => i,
                Err(0) => 0,
                Err(i) => i - 1,
            };
        }
        self.beat_index(time_s) / BEATS_PER_BAR
    }

    /// Every bar's start, in order: the downbeats when known, else
    /// every [`BEATS_PER_BAR`]th beat from the first.
    #[must_use]
    pub fn bar_starts(&self) -> Vec<f64> {
        if !self.downbeats.is_empty() {
            return self.downbeats.clone();
        }
        self.beats.iter().copied().step_by(BEATS_PER_BAR).collect()
    }

    /// Every beat with whether it starts a bar — what a highway draws
    /// its lines from.
    #[must_use]
    pub fn marks(&self) -> Vec<(f64, bool)> {
        let bars = self.bar_starts();
        self.beats
            .iter()
            .map(|&t| {
                let on_bar = bars
                    .binary_search_by(|d| d.total_cmp(&t))
                    .map(|_| true)
                    .unwrap_or_else(|i| {
                        // A downbeat sits on a beat, but the two were
                        // rounded separately: within one precision step
                        // counts.
                        let near = |j: usize| {
                            bars.get(j)
                                .is_some_and(|d| (d - t).abs() <= BEAT_PRECISION_S)
                        };
                        near(i) || (i > 0 && near(i - 1))
                    });
                (t, on_bar)
            })
            .collect()
    }

    /// The tempo map the grid describes: one change per beat, the
    /// tempo of each interval running until the next beat, the last
    /// interval's tempo continuing past the end. `beats_at` of a beat
    /// time is then exactly its index.
    #[must_use]
    pub fn tempo_map(&self) -> Option<TempoMap> {
        if !self.is_usable() {
            return None;
        }
        let changes: Vec<TempoChange> = self
            .beats
            .windows(2)
            .map(|w| TempoChange {
                time_s: w[0],
                bpm: 60.0 / (w[1] - w[0]),
            })
            .collect();
        TempoMap::from_changes(changes).ok()
    }

    /// Snap every note onto this grid within `tolerance_s` (the
    /// difficulties a redesign carries were placed on the constant
    /// grid; this moves them onto the tracked one without touching
    /// what they are — lane and tail stay). Returns how many moved.
    pub fn snap_notes(&self, notes: &mut [ChartNote], tolerance_s: f64) -> usize {
        let mut moved = 0;
        for note in notes.iter_mut() {
            // A note already on the grid comes back a few ULPs off
            // (`a + k · step` is not the stored float); moving it by
            // that rewrote every chart on every run once. Under a
            // microsecond is not a move.
            if let Some(snapped) = self.quantize(note.time, tolerance_s)
                && (snapped - note.time).abs() > SNAP_NOISE_S
            {
                note.time = snapped;
                moved += 1;
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid whose tempo drifts: intervals grow from 0.50 s to
    /// 0.55 s over forty beats — a live drummer settling in.
    fn drifting() -> BeatGrid {
        let mut t = 1.0;
        let mut beats = Vec::new();
        for i in 0..40 {
            beats.push(t);
            t += 0.50 + 0.05 * f64::from(i) / 39.0;
        }
        BeatGrid::from_beats(&beats)
    }

    #[test]
    fn a_grid_is_rounded_deduplicated_and_only_usable_with_two_rising_beats() {
        let g = BeatGrid::from_beats(&[0.123_456, 0.123_46, 1.0, f64::NAN, 2.0]);
        assert_eq!(g.beats, vec![0.1235, 1.0, 2.0]);
        assert!(g.is_usable());
        assert!(!BeatGrid::from_beats(&[1.0]).is_usable());
        assert!(!BeatGrid::default().is_usable());
        let mut backwards = BeatGrid::from_beats(&[1.0, 2.0]);
        backwards.beats.swap(0, 1);
        assert!(!backwards.is_usable());
    }

    #[test]
    fn the_local_beat_follows_the_drift_and_extends_past_both_ends() {
        let g = drifting();
        let first = g.beat_length_at(1.1).expect("usable");
        let last = g.beat_length_at(g.beats[39] - 0.1).expect("usable");
        assert!((first - 0.50).abs() < 1e-9, "{first}");
        // The 39th interval is 0.5 + 0.05 · 38/39.
        assert!((last - 0.5487).abs() < 1e-3, "{last}");
        // Before the first beat and after the last: the neighbouring
        // interval, continued.
        assert!((g.beat_length_at(-5.0).expect("usable") - 0.50).abs() < 1e-9);
        assert!((g.beat_length_at(100.0).expect("usable") - 0.5487).abs() < 1e-3);
        assert_eq!(BeatGrid::default().beat_length_at(1.0), None);
    }

    #[test]
    fn quantize_snaps_to_the_local_subdivision_not_the_average_one() {
        let g = drifting();
        // The tracked beat 39 sits at 1 + 19.5 + 0.95 = 21.45 s; a
        // constant grid at the average interval (0.5244 s) from beat
        // 0 would put it at 21.45 too — but the beats between are
        // off by up to a quarter beat, the very drift the grid is for.
        let b39 = g.beats[39];
        assert!((b39 - 21.45).abs() < 0.002, "{b39}");
        let average = (b39 - g.beats[0]) / 39.0;
        let worst = (0..40)
            .map(|i| (g.beats[i] - (g.beats[0] + i as f64 * average)).abs())
            .fold(0.0, f64::max);
        assert!(worst > 0.1, "the fixture must drift: {worst}");
        // On the beat, a hair off: snapped onto it.
        let t = g.quantize(b39 + 0.020, 0.055).expect("usable");
        assert!((t - b39).abs() < 1e-9, "{t} vs {b39}");
        // Halfway through the last interval is the half beat there,
        // not the half of an average beat.
        let half = g.beats[38] + (g.beats[39] - g.beats[38]) / 2.0;
        let t = g.quantize(half + 0.01, 0.055).expect("usable");
        assert!((t - half).abs() < 1e-9, "{t} vs {half}");
        // Too far from every level: left alone.
        let odd = g.beats[10] + 0.03;
        assert!((g.quantize(odd, 0.01).expect("usable") - odd).abs() < 1e-12);
        // The tolerance never exceeds half a step: a point between
        // two eighths keeps its distance from both.
        assert_eq!(BeatGrid::default().quantize(1.0, 0.055), None);
    }

    #[test]
    fn bars_come_from_the_downbeats_when_known_and_every_fourth_beat_otherwise() {
        let g = BeatGrid::from_beats(&[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0]);
        assert_eq!(g.bar_of(0.0), 0);
        assert_eq!(g.bar_of(1.9), 0);
        assert_eq!(g.bar_of(2.0), 1);
        assert_eq!(g.bar_of(4.2), 2);
        assert_eq!(g.bar_of(-1.0), 0);
        assert_eq!(g.bar_starts(), vec![0.0, 2.0, 4.0]);
        let marks = g.marks();
        assert_eq!(marks.iter().filter(|m| m.1).count(), 3);
        assert!(marks[0].1 && !marks[1].1 && marks[4].1);
        // With downbeats known (a 3/4 song, say), they rule.
        let mut waltz = g.clone();
        waltz.downbeats = vec![0.0, 1.5, 3.0];
        assert_eq!(waltz.bar_of(1.4), 0);
        assert_eq!(waltz.bar_of(1.5), 1);
        assert_eq!(waltz.bar_starts(), vec![0.0, 1.5, 3.0]);
        assert_eq!(waltz.marks().iter().filter(|m| m.1).count(), 3);
        assert!(waltz.marks()[3].1, "1.5 s starts a bar");
    }

    #[test]
    fn the_tempo_map_puts_each_beat_at_its_own_index() {
        let g = drifting();
        let map = g.tempo_map().expect("usable");
        for (i, &t) in g.beats.iter().enumerate() {
            let beats = map.beats_at(t);
            assert!((beats - i as f64).abs() < 1e-6, "beat {i} at {t}: {beats}");
        }
        // Past the end the last tempo continues.
        let last_interval = g.beats[39] - g.beats[38];
        let after = map.beats_at(g.beats[39] + last_interval);
        assert!((after - 40.0).abs() < 1e-6, "{after}");
        assert_eq!(BeatGrid::default().tempo_map(), None);
    }

    #[test]
    fn carried_notes_snap_onto_the_grid_and_keep_what_they_are() {
        let g = drifting();
        let note = |time: f64| ChartNote {
            time,
            lane: 2,
            len: 0.3,
            hopo: true,
        };
        // One on the constant grid's idea of beat 30 (off the tracked
        // one), one already on a beat, one a tenth past a beat — which
        // is within reach of an eighth, as everything is at this
        // tolerance: the finest level is 64 ms wide here.
        let constant_30 = 1.0 + 30.0 * 0.525;
        let mut notes = vec![note(constant_30), note(g.beats[5]), note(g.beats[7] + 0.1)];
        let moved = g.snap_notes(&mut notes, 0.055);
        assert_eq!(moved, 2, "{notes:?}");
        // It landed on a subdivision of the LOCAL beat.
        let (a, beat) = (
            g.beats[g.beat_index(notes[0].time)],
            g.beat_length_at(notes[0].time).expect("usable"),
        );
        let eighths = (notes[0].time - a) / (beat / 8.0);
        assert!((eighths - eighths.round()).abs() < 1e-6, "{eighths}");
        assert!(notes[0].time != constant_30 && notes[0].lane == 2 && notes[0].hopo);
        assert!((notes[0].len - 0.3).abs() < 1e-12);
        assert!(
            (notes[1].time - g.beats[5]).abs() < 1e-12,
            "on a beat already"
        );
        // Snapping again moves nothing: a note on the grid stays put
        // through the float noise of recomputing its position.
        assert_eq!(g.snap_notes(&mut notes, 0.055), 0, "{notes:?}");
        assert!(
            (notes[2].time - (g.beats[7] + 0.1)).abs() < 0.055 && notes[2].time != g.beats[7] + 0.1
        );
    }

    #[test]
    fn a_shift_moves_every_time_and_never_below_zero() {
        let mut g = BeatGrid::from_beats(&[0.01, 1.0, 2.0]);
        g.downbeats = vec![0.01, 2.0];
        g.shift(-0.02);
        assert_eq!(g.beats, vec![0.0, 0.98, 1.98]);
        assert_eq!(g.downbeats, vec![0.0, 1.98]);
    }
}
