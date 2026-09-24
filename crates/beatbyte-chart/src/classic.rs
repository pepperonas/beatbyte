//! Classic: the rules the early guitar games played by, one
//! ingredient at a time.
//!
//! Every function here **re-flags or re-shapes an existing chart**
//! rather than generating one. That is the point: the blind test
//! ([`crate::study`]'s twin, the browser's `T`) can only answer a
//! question about ONE variable, so an ingredient changes exactly one
//! thing and leaves the notes, the frets, the times, the sustains
//! and the phrases where they were.
//!
//! ⚠️ Only numbers and rules from the research come in here, never a
//! chart or a note of anybody else's music — the rule CLAUDE.md
//! states about assets. A threshold is a fact about an engine; a
//! chart is a work.
//!
//! The ingredients are meant to be switched on one at a time, in the
//! order they were measured to matter. This module carries the
//! first.

use crate::grid::BeatGrid;
use crate::schema::{ChartDef, ChartFile, ChartNote};
use beatbyte_core::Difficulty;

/// The HOPO threshold the early games shipped: 170 ticks of the 480
/// that make a beat.
///
/// ⚠️ **Beats, not seconds, and exclusive.** That is the whole
/// difference this ingredient makes. Our generator asks whether the
/// gap is at most 0.22 s (Expert) or 0.26 s (Hard), which is a
/// question about the CLOCK: past about 136 BPM on Expert and 115 on
/// Hard, plain eighth notes fall under it and become hammer-ons, and
/// the library's Hard charts are 39 % HOPO because of it. Asking in
/// beats instead, a plain eighth is 0.5 beats and never qualifies;
/// an eighth-note triplet is 0.333 and always does. So: triplets and
/// faster are hammered, straight eighths are strummed, at every
/// tempo.
pub const HOPO_BEATS: f64 = 170.0 / 480.0;

/// The beat ruler a chart carries, lifted out so the flags can be
/// rewritten while it is read.
///
/// ⚠️ It reads the TRACKED grid where there is one. A chart's
/// constant `bpm` is not its grid — the analysis has tracked a
/// time-varying one since format v0.14.30, and on a live recording
/// the two drift more than a second apart by the end. A beat-
/// relative rule asked against the wrong ruler is a rule about
/// nothing.
#[derive(Debug, Clone)]
pub struct Beats {
    grid: Option<BeatGrid>,
    /// The constant fallback, for a chart from before the grid.
    constant_s: f64,
}

impl Beats {
    /// The ruler this chart means.
    #[must_use]
    pub fn of(chart: &ChartFile) -> Beats {
        Beats {
            grid: chart.grid.clone(),
            constant_s: if chart.song.bpm > 0.0 {
                60.0 / chart.song.bpm
            } else {
                0.0
            },
        }
    }

    /// A ruler of one fixed beat length, for tests and for a chart
    /// that has no grid.
    #[must_use]
    pub const fn constant(beat_s: f64) -> Beats {
        Beats {
            grid: None,
            constant_s: beat_s,
        }
    }

    /// How long a beat is at `time_s`. Zero when nothing says.
    #[must_use]
    pub fn at(&self, time_s: f64) -> f64 {
        self.grid
            .as_ref()
            .and_then(|grid| grid.beat_length_at(time_s))
            .filter(|beat| beat.is_finite() && *beat > 0.0)
            .unwrap_or(self.constant_s)
    }
}

/// One note event: the indices of the notes struck together.
///
/// Grouped by [`crate::convert::CHORD_EPSILON_S`], the same window
/// the engine uses when it turns a chart into a track — so "chord"
/// here means what the player's hand has to do, not what the file
/// happens to round to.
fn events(notes: &[ChartNote]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|a, b| {
        notes[*a]
            .time
            .partial_cmp(&notes[*b].time)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(notes[*a].lane.cmp(&notes[*b].lane))
    });
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut anchor = f64::NEG_INFINITY;
    for index in order {
        let time = notes[index].time;
        match out.last_mut() {
            Some(last) if (time - anchor).abs() <= crate::convert::CHORD_EPSILON_S => {
                last.push(index);
            }
            _ => {
                anchor = time;
                out.push(vec![index]);
            }
        }
    }
    out
}

/// Re-flag one difficulty's hammer-ons under the classic rule.
///
/// Pure, and it touches **nothing but the `hopo` flags** — a pin
/// says so, because the blind test this feeds can only compare one
/// variable at a time. Returns how many notes changed.
///
/// The rule, as the early games ran it:
/// - never the first event;
/// - never a chord, and never a note inside one;
/// - a single note whose gap to the event before it is **under**
///   [`HOPO_BEATS`] of the local beat;
/// - whose fret is not one the hand already had down — a different
///   lane from the previous single note, and not a member of the
///   previous chord.
pub fn reflag_hopos(notes: &mut [ChartNote], beats: &Beats) -> usize {
    let grouped = events(notes);
    let mut changed = 0usize;
    let mut previous: Option<(f64, Vec<u8>)> = None;
    for group in grouped {
        let time = notes[group[0]].time;
        let lanes: Vec<u8> = group.iter().map(|i| notes[*i].lane).collect();
        let hopo = match &previous {
            // The first event is a strum: there is no chain to
            // continue, and nothing to pull off from.
            None => false,
            Some((before, held)) => {
                let beat = beats.at(time);
                let window = HOPO_BEATS * beat;
                // Chords are strummed in every one of these games.
                group.len() == 1
                    && beat > 0.0
                    && time - before < window
                    && !held.contains(&lanes[0])
            }
        };
        for index in &group {
            if notes[*index].hopo != hopo {
                notes[*index].hopo = hopo;
                changed += 1;
            }
        }
        previous = Some((time, lanes));
    }
    changed
}

/// Re-flag every difficulty of a chart. Returns what changed, in the
/// file's own order.
pub fn apply_hopo_rule(chart: &mut ChartFile) -> Vec<(Difficulty, usize)> {
    let beats = Beats::of(chart);
    chart
        .charts
        .iter_mut()
        .map(|def: &mut ChartDef| (def.difficulty, reflag_hopos(&mut def.notes, &beats)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chart of single notes at `times`, all on alternating lanes
    /// so the fret rule never gets in the way of the timing rule.
    fn singles(times: &[f64]) -> Vec<ChartNote> {
        times
            .iter()
            .enumerate()
            .map(|(i, t)| ChartNote {
                time: *t,
                lane: (i % 5) as u8,
                len: 0.0,
                hopo: false,
            })
            .collect()
    }

    fn flags(notes: &[ChartNote]) -> Vec<bool> {
        notes.iter().map(|n| n.hopo).collect()
    }

    /// ⚠️ The ingredient itself: at 120 BPM a straight eighth is
    /// 0.25 s and a beat is 0.5 s, so the gap is exactly half a beat
    /// — over the threshold, strummed. An eighth-note triplet is
    /// a third of a beat, under it, hammered. The old rule asked in
    /// seconds and called both of them HOPOs.
    #[test]
    fn straight_eighths_are_strummed_and_triplets_are_hammered() {
        let beat = 0.5; // 120 BPM
        let beats = Beats::constant(beat);

        let mut eighths = singles(&[0.0, 0.25, 0.5, 0.75]);
        reflag_hopos(&mut eighths, &beats);
        assert_eq!(
            flags(&eighths),
            vec![false; 4],
            "a straight eighth hammered"
        );

        // ⚠️ All THREE of the notes after the first hammer, the
        // fourth included: it sits a third of a beat after the one
        // before it like the others do, and a beat-relative rule
        // reads gaps, not bar positions. I wrote `false` here first
        // and the test corrected the arithmetic.
        let mut triplets = singles(&[0.0, beat / 3.0, 2.0 * beat / 3.0, beat]);
        reflag_hopos(&mut triplets, &beats);
        assert_eq!(
            flags(&triplets),
            vec![false, true, true, true],
            "the triplets did not hammer"
        );
        // A note a whole beat later is a strum again.
        let mut after = singles(&[0.0, beat / 3.0, 2.0 * beat]);
        reflag_hopos(&mut after, &beats);
        assert_eq!(flags(&after), vec![false, true, false]);

        let mut sixteenths = singles(&[0.0, 0.125, 0.25, 0.375]);
        reflag_hopos(&mut sixteenths, &beats);
        assert_eq!(flags(&sixteenths), vec![false, true, true, true]);
    }

    /// ⚠️ Exclusive, not inclusive. A gap of exactly the threshold
    /// is a strum — the engine's own comparison is `<`, and a rule
    /// that rounds the other way turns a whole class of notes over.
    #[test]
    fn a_gap_exactly_at_the_threshold_is_strummed() {
        let beats = Beats::constant(1.0);
        let mut at = singles(&[0.0, HOPO_BEATS]);
        reflag_hopos(&mut at, &beats);
        assert_eq!(flags(&at), vec![false, false], "the boundary hammered");

        let mut under = singles(&[0.0, HOPO_BEATS - 1e-9]);
        reflag_hopos(&mut under, &beats);
        assert_eq!(flags(&under), vec![false, true]);
    }

    /// The rule is about the LOCAL beat, so the same gap answers
    /// differently in a fast bar and a slow one. A chart's constant
    /// `bpm` is not its grid.
    #[test]
    fn the_same_gap_answers_differently_at_different_tempi() {
        let gap = 0.2;
        // At 60 BPM a beat is a second: 0.2 s is a fifth of it.
        let mut slow = singles(&[0.0, gap]);
        reflag_hopos(&mut slow, &Beats::constant(1.0));
        assert_eq!(flags(&slow), vec![false, true]);
        // At 200 BPM a beat is 0.3 s: the same gap is two thirds.
        let mut fast = singles(&[0.0, gap]);
        reflag_hopos(&mut fast, &Beats::constant(0.3));
        assert_eq!(flags(&fast), vec![false, false]);
    }

    /// A tracked grid that slows down is followed, note by note.
    #[test]
    fn a_tracked_grid_is_followed_rather_than_the_constant_bpm() {
        // Beats a second apart, then half a second apart.
        let grid = BeatGrid {
            beats: vec![0.0, 1.0, 2.0, 2.5, 3.0, 3.5],
            downbeats: Vec::new(),
        };
        let beats = Beats {
            grid: Some(grid),
            constant_s: 1.0,
        };
        // A 0.2 s gap early (a fifth of a beat) hammers; the same gap
        // late (two fifths of a half-second beat) does not.
        let mut notes = singles(&[0.0, 0.2]);
        reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, true], "the early gap strummed");

        let mut late = singles(&[2.5, 2.7]);
        reflag_hopos(&mut late, &beats);
        assert_eq!(flags(&late), vec![false, false], "the late gap hammered");
    }

    /// A fret the hand already has down is a strum, whether it came
    /// from the note before or from the chord before.
    #[test]
    fn a_repeated_fret_is_strummed_after_a_note_and_after_a_chord() {
        let beats = Beats::constant(1.0);
        let close = 0.1;

        let mut repeat = vec![
            ChartNote {
                time: 0.0,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: close,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut repeat, &beats);
        assert_eq!(
            flags(&repeat),
            vec![false, false],
            "a repeated fret hammered"
        );

        // A chord, then one of its own frets, then a fret it did not
        // contain. ⚠️ The middle one is the case the early engines
        // call out by name: it was already held, so it is a strum.
        let mut after_chord = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.0,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: close,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 2.0 * close,
                lane: 3,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut after_chord, &beats);
        assert_eq!(
            flags(&after_chord),
            vec![false, false, false, true],
            "a fret out of the chord hammered, or the new fret did not"
        );
    }

    /// Chords are strummed in all of these games, however close they
    /// fall — and a chord that arrives already flagged is cleared.
    #[test]
    fn a_chord_is_never_a_hammer_on() {
        let beats = Beats::constant(1.0);
        let mut notes = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.1,
                lane: 2,
                len: 0.0,
                hopo: true,
            },
            ChartNote {
                time: 0.1,
                lane: 3,
                len: 0.0,
                hopo: true,
            },
        ];
        let changed = reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, false, false]);
        assert_eq!(changed, 2, "the chord's flags were not cleared");
    }

    /// Simultaneous within the engine's own window is one chord —
    /// not two events a tenth of a millisecond apart, which would be
    /// a hammer-on the hand cannot play.
    #[test]
    fn notes_a_hair_apart_are_one_chord_the_way_the_engine_reads_them() {
        let beats = Beats::constant(1.0);
        let apart = crate::convert::CHORD_EPSILON_S * 0.5;
        let mut notes = vec![
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: apart,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        reflag_hopos(&mut notes, &beats);
        assert_eq!(flags(&notes), vec![false, false], "a chord split in two");
    }

    /// Nothing says how long a beat is → nothing is a hammer-on.
    /// Silence is the safe answer; guessing a tempo is not.
    #[test]
    fn without_a_beat_nothing_hammers() {
        let mut notes = singles(&[0.0, 0.01, 0.02]);
        reflag_hopos(&mut notes, &Beats::constant(0.0));
        assert_eq!(flags(&notes), vec![false; 3]);
    }

    /// ⚠️⚠️ The pin the whole method rests on: an ingredient may
    /// change the `hopo` flags and **nothing else**. The blind test
    /// plays the new version against its parent, and a person can
    /// only answer a question about one variable — if a note, a
    /// fret, a time, a sustain or a phrase moved as well, the
    /// verdict would be about a different question than the one
    /// asked.
    #[test]
    fn an_ingredient_changes_the_flags_and_nothing_else() {
        let text = r#"{"format_version":1,
            "song":{"title":"Maria","artist":"Blondie","audio":"maria.m4a",
                    "bpm":132.0,"duration_s":200.0,"offset_s":0.0,
                    "preview_start_s":31.5},
            "grid":{"beats":[0.0,0.45,0.9,1.35,1.8,2.25,2.7]},
            "charts":[
              {"difficulty":"hard","lanes":5,
               "notes":[{"time":0.0,"lane":0,"len":1.25},
                        {"time":0.22,"lane":2},
                        {"time":0.45,"lane":2,"hopo":true},
                        {"time":0.52,"lane":3,"hopo":true},
                        {"time":0.9,"lane":1},
                        {"time":0.9,"lane":3},
                        {"time":1.8,"lane":4,"len":0.5}],
               "phrases":[{"start":0.0,"end":1.8}]},
              {"difficulty":"expert","lanes":5,
               "notes":[{"time":0.1,"lane":1},{"time":0.2,"lane":4,"hopo":true}],
               "phrases":[]}
            ]}"#;
        let before = ChartFile::from_json(text).expect("the fixture parses");
        let mut after = before.clone();
        let changed = apply_hopo_rule(&mut after);
        assert!(
            changed.iter().any(|(_, n)| *n > 0),
            "the fixture exercises nothing: {changed:?}"
        );

        // Everything outside the flags, read back field by field.
        assert_eq!(after.song, before.song, "the song metadata moved");
        assert_eq!(after.grid, before.grid, "the grid moved");
        assert_eq!(after.audio_trim, before.audio_trim);
        assert_eq!(after.format_version, before.format_version);
        assert_eq!(after.charts.len(), before.charts.len());
        for (new, old) in after.charts.iter().zip(&before.charts) {
            assert_eq!(new.difficulty, old.difficulty);
            assert_eq!(new.lanes, old.lanes);
            assert_eq!(new.phrases, old.phrases, "a phrase moved");
            assert_eq!(new.notes.len(), old.notes.len(), "a note appeared or left");
            for (a, b) in new.notes.iter().zip(&old.notes) {
                assert_eq!(a.time, b.time, "a note moved in time");
                assert_eq!(a.lane, b.lane, "a note changed fret");
                assert_eq!(a.len, b.len, "a sustain changed length");
            }
        }
        // And the counts it reports are the flags that really moved.
        for (difficulty, count) in changed {
            let new = after.charts.iter().find(|c| c.difficulty == difficulty);
            let old = before.charts.iter().find(|c| c.difficulty == difficulty);
            let really = new
                .zip(old)
                .map(|(n, o)| {
                    n.notes
                        .iter()
                        .zip(&o.notes)
                        .filter(|(a, b)| a.hopo != b.hopo)
                        .count()
                })
                .unwrap_or(0);
            assert_eq!(count, really, "{difficulty:?} miscounted its changes");
        }
    }

    /// The file's note ORDER survives, because the hash sees it.
    #[test]
    fn the_notes_keep_the_order_the_file_had() {
        let beats = Beats::constant(1.0);
        // Deliberately out of time order, as a hand-edited file may be.
        let mut notes = vec![
            ChartNote {
                time: 0.5,
                lane: 1,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.0,
                lane: 0,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 0.1,
                lane: 2,
                len: 0.0,
                hopo: false,
            },
        ];
        let before: Vec<(f64, u8)> = notes.iter().map(|n| (n.time, n.lane)).collect();
        reflag_hopos(&mut notes, &beats);
        let after: Vec<(f64, u8)> = notes.iter().map(|n| (n.time, n.lane)).collect();
        assert_eq!(before, after, "the notes were reordered");
        // And the rule still read them in time order: 0.1 follows
        // 0.0 closely on another fret.
        assert_eq!(flags(&notes), vec![false, false, true]);
    }
}
