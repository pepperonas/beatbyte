//! Musical time for the editor: seconds ↔ bar:beat:tick, and snapping
//! to a subdivision of the beat.
//!
//! Everything is computed over the chart's BEAT MARKS
//! ([`beatbyte_chart::ChartFile::beat_marks`]) — the tracked grid when
//! the chart carries one, else the constant grid — so the numbers the
//! editor shows, the lines it draws and the positions it snaps to come
//! from one source. The highway draws its lines from the same marks.
//!
//! Beyond the last mark (and before the first) the grid is extended
//! with the nearest beat length: a note may sit after the analysis
//! stopped, and it must still have a position and a snap.
//!
//! Positions are 1-based like a sequencer (bar 1, beat 1); ticks count
//! [`TICKS_PER_BEAT`] per beat. Beats before the first bar mark belong
//! to bar 0 (a pickup).

/// Ticks in one beat — divisible by 2, 3, 4, 6, 8, 12, 16, 24, 32 and
/// 48, so every snap division lands on a whole tick.
pub const TICKS_PER_BEAT: u32 = 192;

/// A musical position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// 1-based bar (0 = before the first bar line).
    pub bar: i64,
    /// 1-based beat within the bar.
    pub beat: u32,
    /// Tick within the beat, `0..TICKS_PER_BEAT`.
    pub tick: u32,
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{:03}", self.bar, self.beat, self.tick)
    }
}

/// The grid the editor measures with: beat times and which of them
/// start a bar. Built once per chart (the marks do not change while
/// notes are edited).
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    beats: Vec<f64>,
    /// For every beat: the bar it belongs to and its 1-based index in
    /// that bar.
    place: Vec<(i64, u32)>,
}

impl Grid {
    /// A grid from beat marks `(time, starts_a_bar)`. Marks must rise;
    /// ones that do not are dropped. `None` for fewer than two marks —
    /// a grid needs an interval.
    #[must_use]
    pub fn from_marks(marks: &[(f64, bool)]) -> Option<Grid> {
        let mut beats: Vec<f64> = Vec::with_capacity(marks.len());
        let mut place = Vec::with_capacity(marks.len());
        let mut bar = 0i64;
        let mut beat = 0u32;
        for &(time, downbeat) in marks {
            if !time.is_finite() || beats.last().is_some_and(|last| time <= *last) {
                continue;
            }
            if downbeat {
                bar += 1;
                beat = 1;
            } else {
                beat += 1;
            }
            beats.push(time);
            place.push((bar, beat.max(1)));
        }
        (beats.len() >= 2).then_some(Grid { beats, place })
    }

    /// The beat times.
    #[must_use]
    pub fn beats(&self) -> &[f64] {
        &self.beats
    }

    /// Beat marks `(time, starts_a_bar)` in `[from, to]`, the grid
    /// extended past its ends — what the timeline draws.
    #[must_use]
    pub fn marks_between(&self, from: f64, to: f64) -> Vec<(f64, bool)> {
        let first = self.beat_at(from).floor() as i64;
        let last = self.beat_at(to).ceil() as i64;
        (first..=last)
            .filter_map(|index| {
                let time = self.time_at(index as f64);
                (time >= from - 1e-9 && time <= to + 1e-9)
                    .then(|| (time, self.place_of(index).1 == 1))
            })
            .collect()
    }

    /// The (bar, beat) of beat `index`, extended past the ends.
    fn place_of(&self, index: i64) -> (i64, u32) {
        let last = self.beats.len() as i64 - 1;
        if (0..=last).contains(&index) {
            return self.place[index as usize];
        }
        // Past an end: keep counting in the bar length of the nearest
        // complete bar (4 when the grid has none).
        let bar_len = self.bar_length();
        let (anchor, anchor_place) = if index > last {
            (last, self.place[last as usize])
        } else {
            (0, self.place[0])
        };
        let offset = index - anchor + i64::from(anchor_place.1) - 1;
        let bars = offset.div_euclid(i64::from(bar_len));
        let beat = offset.rem_euclid(i64::from(bar_len)) as u32 + 1;
        (anchor_place.0 + bars, beat)
    }

    /// The beats of the grid's most common complete bar (4 when it
    /// has none). ⚠️ The MOST COMMON, not the longest: a tracked grid
    /// carries the odd irregular bar, and the longest one turned the
    /// pickup before the first bar line into "bar -1, beat 5".
    fn bar_length(&self) -> u32 {
        let mut counts = std::collections::BTreeMap::<u32, usize>::new();
        for window in self.place.windows(2) {
            // A complete bar ends where the next begins; the first one
            // may be a pickup (bar 0) and is not counted.
            if window[1].0 != window[0].0 && window[0].0 >= 1 {
                *counts.entry(window[0].1).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(beats, count)| (*count, std::cmp::Reverse(*beats)))
            .map_or(4, |(beats, _)| beats)
    }

    /// The fractional beat index of a time (0 = the first mark),
    /// extended linearly past both ends.
    #[must_use]
    pub fn beat_at(&self, time: f64) -> f64 {
        let n = self.beats.len();
        if time <= self.beats[0] {
            let len = self.beats[1] - self.beats[0];
            return (time - self.beats[0]) / len;
        }
        if time >= self.beats[n - 1] {
            let len = self.beats[n - 1] - self.beats[n - 2];
            return (n - 1) as f64 + (time - self.beats[n - 1]) / len;
        }
        let upper = self.beats.partition_point(|beat| *beat <= time);
        let lower = upper - 1;
        let len = self.beats[upper] - self.beats[lower];
        lower as f64 + (time - self.beats[lower]) / len
    }

    /// The time of a fractional beat index, extended past both ends.
    #[must_use]
    pub fn time_at(&self, beat: f64) -> f64 {
        let n = self.beats.len();
        if beat <= 0.0 {
            return self.beats[0] + beat * (self.beats[1] - self.beats[0]);
        }
        if beat >= (n - 1) as f64 {
            let len = self.beats[n - 1] - self.beats[n - 2];
            return self.beats[n - 1] + (beat - (n - 1) as f64) * len;
        }
        let lower = beat.floor() as usize;
        let frac = beat - lower as f64;
        self.beats[lower] + frac * (self.beats[lower + 1] - self.beats[lower])
    }

    /// Snap a time to the nearest `1/division` of a beat.
    #[must_use]
    pub fn snap(&self, time: f64, division: u32) -> f64 {
        let step = 1.0 / f64::from(division.max(1));
        let beat = (self.beat_at(time) / step).round() * step;
        self.time_at(beat)
    }

    /// Move a time by whole `1/division` steps from its snapped
    /// position.
    #[must_use]
    pub fn step(&self, time: f64, division: u32, steps: i64) -> f64 {
        let step = 1.0 / f64::from(division.max(1));
        let beat = (self.beat_at(time) / step).round() * step + steps as f64 * step;
        self.time_at(beat)
    }

    /// The musical position of a time. Ticks are rounded; a time that
    /// rounds to the next beat IS that beat.
    #[must_use]
    pub fn position(&self, time: f64) -> Position {
        let beat = self.beat_at(time);
        let mut index = beat.floor() as i64;
        let mut tick = ((beat - index as f64) * f64::from(TICKS_PER_BEAT)).round() as u32;
        if tick >= TICKS_PER_BEAT {
            index += 1;
            tick = 0;
        }
        let (bar, beat) = self.place_of(index);
        Position { bar, beat, tick }
    }

    /// The time of a musical position — the inverse of
    /// [`Grid::position`] to within a tick. `None` for a beat the bar
    /// does not have.
    #[must_use]
    pub fn time_of(&self, position: Position) -> Option<f64> {
        if position.beat == 0 || position.tick >= TICKS_PER_BEAT {
            return None;
        }
        // Find the beat index with this (bar, beat) — search the grid,
        // then extend.
        let index = (0..self.beats.len() as i64)
            .find(|i| self.place_of(*i) == (position.bar, position.beat))
            .or_else(|| {
                let bar_len = i64::from(self.bar_length());
                let last = self.beats.len() as i64 - 1;
                let (last_bar, last_beat) = self.place_of(last);
                let (first_bar, first_beat) = self.place_of(0);
                let after = (position.bar - last_bar) * bar_len + i64::from(position.beat)
                    - i64::from(last_beat);
                if after > 0 {
                    return Some(last + after);
                }
                let before = (position.bar - first_bar) * bar_len + i64::from(position.beat)
                    - i64::from(first_beat);
                (before < 0).then_some(before)
            })?;
        if self.place_of(index) != (position.bar, position.beat) {
            return None;
        }
        Some(self.time_at(index as f64 + f64::from(position.tick) / f64::from(TICKS_PER_BEAT)))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 120 BPM, a bar line every four beats, from 1.0 s.
    fn steady() -> Grid {
        let marks: Vec<(f64, bool)> = (0..32)
            .map(|i| (1.0 + f64::from(i) * 0.5, i % 4 == 0))
            .collect();
        Grid::from_marks(&marks).unwrap()
    }

    #[test]
    fn a_position_names_bar_beat_and_tick() {
        let grid = steady();
        assert_eq!(
            grid.position(1.0),
            Position {
                bar: 1,
                beat: 1,
                tick: 0
            }
        );
        assert_eq!(
            grid.position(1.5),
            Position {
                bar: 1,
                beat: 2,
                tick: 0
            }
        );
        assert_eq!(
            grid.position(3.0),
            Position {
                bar: 2,
                beat: 1,
                tick: 0
            }
        );
        // Half a beat into beat 3 of bar 1.
        assert_eq!(
            grid.position(2.25),
            Position {
                bar: 1,
                beat: 3,
                tick: 96
            }
        );
        assert_eq!(grid.position(2.25).to_string(), "1:3:096");
    }

    /// Every tick of four bars converts there and back exactly.
    #[test]
    fn position_and_time_round_trip() {
        let grid = steady();
        for beat in 0..16 {
            for tick in (0..TICKS_PER_BEAT).step_by(12) {
                let time =
                    grid.time_at(f64::from(beat) + f64::from(tick) / f64::from(TICKS_PER_BEAT));
                let position = grid.position(time);
                let back = grid.time_of(position).unwrap();
                assert!(
                    (back - time).abs() < 1e-9,
                    "{beat}/{tick}: {time} → {position} → {back}"
                );
            }
        }
    }

    /// A tempo change: the second half is twice as fast. Beats and
    /// snaps follow the LOCAL beat length.
    #[test]
    fn a_tempo_change_is_followed() {
        let mut marks: Vec<(f64, bool)> = (0..8).map(|i| (f64::from(i), i % 4 == 0)).collect();
        marks.extend((1..=8).map(|i| (7.0 + f64::from(i) * 0.5, (7 + i) % 4 == 0)));
        let grid = Grid::from_marks(&marks).unwrap();
        // Beat 8 (7.5 s) starts bar 3; 8.0 s is its second beat.
        assert_eq!(
            grid.position(7.5),
            Position {
                bar: 3,
                beat: 1,
                tick: 0
            }
        );
        assert_eq!(
            grid.position(8.0),
            Position {
                bar: 3,
                beat: 2,
                tick: 0
            }
        );
        // An eighth after 8.0 s is 0.25 s there, 0.5 s in the slow part.
        assert!((grid.step(8.0, 2, 1) - 8.25).abs() < 1e-9);
        assert!((grid.step(2.0, 2, 1) - 2.5).abs() < 1e-9);
        // Snap to sixteenths in the fast part.
        assert!((grid.snap(8.13, 4) - 8.125).abs() < 1e-9);
    }

    /// Past the last mark the grid goes on with the last beat length,
    /// bars and all — a note after the analysis stopped still has a
    /// place.
    #[test]
    fn the_grid_extends_past_both_ends() {
        let grid = steady();
        let end = grid.beats().last().copied().unwrap();
        let position = grid.position(end + 0.5);
        assert_eq!(
            position,
            Position {
                bar: 9,
                beat: 1,
                tick: 0
            }
        );
        assert!((grid.time_of(position).unwrap() - (end + 0.5)).abs() < 1e-9);
        // Before the first mark: the pickup bar 0.
        let pickup = grid.position(0.5);
        assert_eq!(
            pickup,
            Position {
                bar: 0,
                beat: 4,
                tick: 0
            }
        );
        assert!((grid.time_of(pickup).unwrap() - 0.5).abs() < 1e-9);
        assert!(
            (grid.snap(-0.26, 1) - (-0.5)).abs() < 1e-9,
            "snapping works before the grid too"
        );
    }

    /// One irregular 5-beat bar in a grid of 4-beat bars does not make
    /// the grid "5 beats a bar" past its ends.
    #[test]
    fn the_common_bar_length_extends_the_grid() {
        let mut marks = Vec::new();
        let mut t = 1.0;
        for bar in 0..6 {
            let beats = if bar == 2 { 5 } else { 4 };
            for beat in 0..beats {
                marks.push((t, beat == 0));
                t += 0.5;
            }
        }
        let grid = Grid::from_marks(&marks).unwrap();
        let before = grid.position(0.5);
        assert_eq!(
            before,
            Position {
                bar: 0,
                beat: 4,
                tick: 0
            },
            "a pickup, not beat 5"
        );
        let after = grid.position(t + 2.0);
        assert!(after.beat <= 4, "{after}");
    }

    #[test]
    fn a_snap_lands_on_the_division() {
        let grid = steady();
        assert!((grid.snap(1.26, 1) - 1.5).abs() < 1e-9);
        assert!((grid.snap(1.26, 2) - 1.25).abs() < 1e-9);
        assert!((grid.snap(1.26, 4) - 1.25).abs() < 1e-9);
        assert!((grid.snap(1.20, 4) - 1.25).abs() < 1e-9);
        // Triplets.
        assert!((grid.snap(1.17, 3) - (1.0 + 0.5 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn marks_between_include_the_bar_lines() {
        let grid = steady();
        let marks = grid.marks_between(2.9, 5.1);
        let times: Vec<f64> = marks.iter().map(|m| m.0).collect();
        assert_eq!(times, vec![3.0, 3.5, 4.0, 4.5, 5.0]);
        assert_eq!(
            marks.iter().filter(|m| m.1).count(),
            2,
            "3.0 and 5.0 start bars"
        );
    }

    #[test]
    fn a_grid_needs_two_rising_marks() {
        assert!(Grid::from_marks(&[(1.0, true)]).is_none());
        assert!(Grid::from_marks(&[(1.0, true), (1.0, false)]).is_none());
        assert!(Grid::from_marks(&[(1.0, true), (0.5, false), (2.0, false)]).is_some());
    }

    #[test]
    fn a_beat_the_bar_does_not_have_has_no_time() {
        let grid = steady();
        assert!(
            grid.time_of(Position {
                bar: 2,
                beat: 5,
                tick: 0
            })
            .is_none()
        );
        assert!(
            grid.time_of(Position {
                bar: 2,
                beat: 0,
                tick: 0
            })
            .is_none()
        );
        assert!(
            grid.time_of(Position {
                bar: 2,
                beat: 1,
                tick: TICKS_PER_BEAT
            })
            .is_none()
        );
    }
}
