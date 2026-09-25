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

/// Music that plays before a playtest's first note, so the notes
/// scroll in rather than start on the strike line.
pub const PLAYTEST_LEAD_S: f64 = 2.0;

/// What a playtest plays: where the music starts and where the test
/// ends (`None` = the song's end). The loop when it is on, else the
/// selection, else from the playhead.
#[must_use]
pub fn playtest_window(
    looping: Option<LoopRegion>,
    selection: Option<(f64, f64)>,
    playhead: f64,
) -> (f64, Option<f64>) {
    let (from, to) = match (looping, selection) {
        (Some(region), _) => (region.start, Some(region.end)),
        (None, Some((lo, hi))) => (lo, Some(hi)),
        (None, None) => (playhead, None),
    };
    ((from - PLAYTEST_LEAD_S).max(0.0), to)
}

/// The chart a playtest plays: the edited chart as it is NOW (saved
/// or not), with the tested difficulty cut to the window — a note
/// before the start would be a miss nobody could hit, one after the
/// end would keep the test running.
#[must_use]
pub fn playtest_chart(
    chart: &beatbyte_chart::ChartFile,
    difficulty: beatbyte_core::Difficulty,
    from: f64,
    to: Option<f64>,
) -> beatbyte_chart::ChartFile {
    let mut out = chart.clone();
    let inside = |t: f64| t >= from - 1e-6 && to.is_none_or(|end| t <= end + 1e-6);
    if let Some(def) = out.charts.iter_mut().find(|d| d.difficulty == difficulty) {
        def.notes.retain(|note| inside(note.time));
        def.phrases.retain(|phrase| inside(phrase.start));
    }
    out
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

    /// The loop wins, then the selection, then the playhead — each
    /// with its lead-in, never before zero.
    #[test]
    fn a_playtest_plays_the_loop_or_the_selection_or_from_here() {
        let region = LoopRegion::between(10.0, 14.0);
        assert_eq!(
            playtest_window(region, Some((20.0, 22.0)), 30.0),
            (8.0, Some(14.0))
        );
        assert_eq!(
            playtest_window(None, Some((20.0, 22.0)), 30.0),
            (18.0, Some(22.0))
        );
        assert_eq!(playtest_window(None, None, 30.0), (28.0, None));
        assert_eq!(playtest_window(None, None, 1.0), (0.0, None));
    }

    /// The tested difficulty is cut to the window; the others and the
    /// song stay as they are, and the source chart is not touched.
    #[test]
    fn a_playtest_chart_is_cut_to_its_window() {
        use beatbyte_chart::{ChartDef, ChartFile, ChartNote, ChartPhrase, SongMeta};
        use beatbyte_core::Difficulty;
        let notes = |times: &[f64]| {
            times
                .iter()
                .map(|t| ChartNote {
                    time: *t,
                    lane: 0,
                    len: 0.0,
                    hopo: false,
                })
                .collect::<Vec<_>>()
        };
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
            charts: vec![
                ChartDef {
                    difficulty: Difficulty::Medium,
                    lanes: 5,
                    notes: notes(&[1.0, 5.0]),
                    phrases: vec![],
                },
                ChartDef {
                    difficulty: Difficulty::Expert,
                    lanes: 5,
                    notes: notes(&[1.0, 5.0, 9.0, 13.0]),
                    phrases: vec![
                        ChartPhrase {
                            start: 0.5,
                            end: 1.5,
                        },
                        ChartPhrase {
                            start: 8.5,
                            end: 9.5,
                        },
                    ],
                },
            ],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        };
        let cut = playtest_chart(&chart, Difficulty::Expert, 4.0, Some(10.0));
        let expert = cut.chart_for(Difficulty::Expert).unwrap();
        assert_eq!(
            expert.notes.iter().map(|n| n.time).collect::<Vec<_>>(),
            vec![5.0, 9.0]
        );
        assert_eq!(expert.phrases.len(), 1);
        assert_eq!(
            cut.chart_for(Difficulty::Medium),
            chart.chart_for(Difficulty::Medium)
        );
        assert_eq!(chart.charts[1].notes.len(), 4, "the source changed");
        let open = playtest_chart(&chart, Difficulty::Expert, 4.0, None);
        assert_eq!(open.chart_for(Difficulty::Expert).unwrap().notes.len(), 3);
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
