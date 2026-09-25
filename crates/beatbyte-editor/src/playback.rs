//! Playback in the editor: the speeds it offers and the loop region.
//!
//! Pure — the game moves the music and the clock; these decide where
//! to and when.

/// The playback speeds, cycled in this order. Slowing down lowers the
/// pitch (the practice path: the music thread's speed, the clock's
/// rate — notes and audio stay together because both run at it).
pub const SPEEDS: [f64; 3] = [1.0, 0.75, 0.5];

/// The speed after `speed` in the cycle (an unknown one goes to 1×).
#[must_use]
pub fn next_speed(speed: f64) -> f64 {
    let at = SPEEDS.iter().position(|s| (s - speed).abs() < 1e-9);
    at.map_or(SPEEDS[0], |i| SPEEDS[(i + 1) % SPEEDS.len()])
}

/// The shortest loop worth having, seconds — anything shorter is a
/// slip of the mouse, not a loop.
pub const MIN_LOOP_S: f64 = 0.05;

/// A loop region on the song timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopRegion {
    /// Where it starts.
    pub start: f64,
    /// Where it ends (after `start`).
    pub end: f64,
}

impl LoopRegion {
    /// A region between two times in either order; `None` when it
    /// would be shorter than [`MIN_LOOP_S`].
    #[must_use]
    pub fn between(a: f64, b: f64) -> Option<LoopRegion> {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let start = start.max(0.0);
        (end - start >= MIN_LOOP_S).then_some(LoopRegion { start, end })
    }

    /// Where the playhead should jump, if it has run past the end —
    /// or was put before the start while looping (a region that
    /// plays is where the playhead lives).
    #[must_use]
    pub fn wrap(&self, now: f64) -> Option<f64> {
        (now >= self.end || now < self.start - 1e-6).then_some(self.start)
    }

    /// The region with one end moved to `time`: an in-point after the
    /// end, or an out-point before the start, swaps rather than
    /// failing.
    #[must_use]
    pub fn with_in(self, time: f64) -> Option<LoopRegion> {
        LoopRegion::between(time, self.end)
    }

    /// See [`LoopRegion::with_in`].
    #[must_use]
    pub fn with_out(self, time: f64) -> Option<LoopRegion> {
        LoopRegion::between(self.start, time)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_speeds_cycle_and_an_unknown_one_resets() {
        assert!((next_speed(1.0) - 0.75).abs() < 1e-9);
        assert!((next_speed(0.75) - 0.5).abs() < 1e-9);
        assert!((next_speed(0.5) - 1.0).abs() < 1e-9);
        assert!((next_speed(0.9) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_region_is_ordered_and_has_a_length() {
        let region = LoopRegion::between(5.0, 2.0).unwrap();
        assert_eq!((region.start, region.end), (2.0, 5.0));
        assert!(LoopRegion::between(2.0, 2.01).is_none());
        assert_eq!(LoopRegion::between(-1.0, 1.0).unwrap().start, 0.0);
    }

    /// The playhead wraps at the end, and one put before the start
    /// goes to the start; inside, nothing happens.
    #[test]
    fn the_playhead_wraps_at_the_end() {
        let region = LoopRegion::between(2.0, 4.0).unwrap();
        assert_eq!(region.wrap(3.9), None);
        assert_eq!(region.wrap(2.0), None);
        assert_eq!(region.wrap(4.0), Some(2.0));
        assert_eq!(region.wrap(7.5), Some(2.0));
        assert_eq!(region.wrap(1.0), Some(2.0));
    }

    #[test]
    fn in_and_out_points_move_one_end() {
        let region = LoopRegion::between(2.0, 4.0).unwrap();
        assert_eq!(region.with_in(3.0).unwrap().start, 3.0);
        assert_eq!(region.with_out(6.0).unwrap().end, 6.0);
        // An out-point before the start swaps the ends.
        let swapped = region.with_out(1.0).unwrap();
        assert_eq!((swapped.start, swapped.end), (1.0, 2.0));
        assert!(region.with_in(4.0).is_none(), "zero length");
    }
}
