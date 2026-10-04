//! Ticks to seconds.
//!
//! Both source formats count in ticks — `resolution` ticks per quarter
//! note — and carry a list of tempo changes. A tick's time is the sum
//! of every whole tempo segment before it plus its share of the one
//! it sits in. The changes are sorted and deduplicated once; a lookup
//! is a binary search, so converting tens of thousands of notes stays
//! linear-ish.

/// A tempo map in ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    resolution: u32,
    /// `(tick, microseconds per quarter, seconds at that tick)`.
    segments: Vec<(u64, f64, f64)>,
}

/// The tempo assumed before the first change (and when there is none).
pub const DEFAULT_US_PER_QUARTER: f64 = 500_000.0;

/// Slowest and fastest tempo a change may set; anything outside is
/// not music and is ignored rather than allowed to stretch the song to
/// hours or crush it to nothing.
pub const US_PER_QUARTER_RANGE: core::ops::RangeInclusive<f64> = 60_000.0..=6_000_000.0;

impl TempoMap {
    /// The map for `resolution` ticks per quarter and the given
    /// `(tick, microseconds per quarter)` changes, in any order.
    /// Implausible tempos are skipped; at a tick with two changes the
    /// last one given wins. Pure — tested.
    #[must_use]
    pub fn new(resolution: u32, changes: &[(u64, f64)]) -> TempoMap {
        let resolution = resolution.max(1);
        let mut sorted: Vec<(u64, f64)> = changes
            .iter()
            .copied()
            .filter(|(_, us)| us.is_finite() && US_PER_QUARTER_RANGE.contains(us))
            .collect();
        // Stable: two changes at one tick keep their file order, and
        // the later one is what the tick plays at.
        sorted.sort_by_key(|(tick, _)| *tick);
        let mut deduped: Vec<(u64, f64)> = Vec::with_capacity(sorted.len() + 1);
        for (tick, us) in sorted {
            match deduped.last_mut() {
                Some(last) if last.0 == tick => last.1 = us,
                _ => deduped.push((tick, us)),
            }
        }
        if deduped.first().is_none_or(|(tick, _)| *tick != 0) {
            deduped.insert(0, (0, DEFAULT_US_PER_QUARTER));
        }
        let mut segments = Vec::with_capacity(deduped.len());
        let mut seconds = 0.0;
        for (index, &(tick, us)) in deduped.iter().enumerate() {
            if index > 0 {
                let (prev_tick, prev_us) = deduped[index - 1];
                seconds += span(tick - prev_tick, prev_us, resolution);
            }
            segments.push((tick, us, seconds));
        }
        TempoMap {
            resolution,
            segments,
        }
    }

    /// Ticks per quarter note.
    #[must_use]
    pub fn resolution(&self) -> u32 {
        self.resolution
    }

    /// The song seconds of `tick`.
    #[must_use]
    pub fn seconds(&self, tick: u64) -> f64 {
        let index = self
            .segments
            .partition_point(|(start, _, _)| *start <= tick)
            .saturating_sub(1);
        let (start, us, at) = self.segments[index];
        at + span(tick - start, us, self.resolution)
    }

    /// The tempo at `tick` in beats per minute.
    #[must_use]
    pub fn bpm_at(&self, tick: u64) -> f64 {
        let index = self
            .segments
            .partition_point(|(start, _, _)| *start <= tick)
            .saturating_sub(1);
        60_000_000.0 / self.segments[index].1
    }

    /// The tempo that governs the most TIME up to `end_tick`, in BPM —
    /// what a single-tempo reader should call the song's tempo. Pure —
    /// tested.
    #[must_use]
    pub fn dominant_bpm(&self, end_tick: u64) -> f64 {
        // Summed in a map keyed by the tempo's exact value: a linear
        // search per segment is quadratic in the number of distinct
        // tempos, and untrusted input decides that number.
        let mut weights: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
        for (index, &(start, us, at)) in self.segments.iter().enumerate() {
            if start >= end_tick && index > 0 {
                break;
            }
            let until = self
                .segments
                .get(index + 1)
                .map_or(end_tick, |next| next.0.min(end_tick));
            let duration = self.seconds(until.max(start)) - at;
            *weights.entry(us.to_bits()).or_insert(0.0) += duration;
        }
        weights
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(120.0, |(us, _)| 60_000_000.0 / f64::from_bits(us))
    }
}

fn span(ticks: u64, us_per_quarter: f64, resolution: u32) -> f64 {
    ticks as f64 / f64::from(resolution) * us_per_quarter / 1_000_000.0
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn one_tempo_is_a_straight_line() {
        let map = TempoMap::new(480, &[(0, 500_000.0)]);
        assert!(close(map.seconds(0), 0.0));
        assert!(close(map.seconds(480), 0.5));
        assert!(close(map.seconds(960 * 10), 10.0));
    }

    #[test]
    fn a_tempo_change_bends_the_line_at_its_tick() {
        // 120 BPM for two beats, then 60 BPM.
        let map = TempoMap::new(192, &[(384, 1_000_000.0), (0, 500_000.0)]);
        assert!(close(map.seconds(384), 1.0));
        assert!(close(map.seconds(384 + 192), 2.0));
        assert!(close(map.bpm_at(100), 120.0));
        assert!(close(map.bpm_at(384), 60.0));
    }

    #[test]
    fn no_change_at_zero_means_the_default_tempo_until_the_first() {
        let map = TempoMap::new(480, &[(480, 250_000.0)]);
        assert!(close(map.seconds(480), 0.5));
        assert!(close(map.seconds(960), 0.75));
    }

    #[test]
    fn two_changes_on_one_tick_play_the_later_one() {
        let map = TempoMap::new(480, &[(0, 500_000.0), (0, 1_000_000.0)]);
        assert!(close(map.seconds(480), 1.0));
    }

    #[test]
    fn implausible_tempos_are_ignored() {
        let map = TempoMap::new(480, &[(0, 500_000.0), (480, 0.0), (960, f64::NAN)]);
        assert!(close(map.seconds(1440), 1.5));
    }

    #[test]
    fn the_dominant_tempo_is_the_one_that_lasts_longest_in_time() {
        // 120 BPM for 1 beat (0.5 s), then 200 BPM for 8 beats (2.4 s).
        let map = TempoMap::new(480, &[(0, 500_000.0), (480, 300_000.0)]);
        assert!(close(map.dominant_bpm(480 * 9), 200.0));
        // Cut before the change and the first wins.
        assert!(close(map.dominant_bpm(480), 120.0));
    }
}
