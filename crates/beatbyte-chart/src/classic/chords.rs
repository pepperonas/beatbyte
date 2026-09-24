//! Chords where the recording strikes them (K5).
//!
//! The early games charted about 30 % of Hard and Expert notes as
//! chords; ours carry 2–4 %, and those few were placed from loudness —
//! a guess. This ingredient writes a chord only where a polyphonic
//! transcription of the song ([`crate::poly`]) heard **several notes
//! struck at that moment**, and shapes it from what it heard: the
//! interval between the two lowest struck pitches decides how far
//! apart the frets are, the way the early games drew a fifth wider
//! than a third.
//!
//! What counts as struck together:
//! - notes that START within [`STRUCK_WINDOW_S`] of the event, at
//!   [`MIN_AMPLITUDE`] or more;
//! - ⚠️ **not** a note sitting an overtone above a lower struck note
//!   (an octave, an octave and a fifth, two octaves …) unless it is
//!   nearly as strong: one plucked string rings at those intervals by
//!   physics, and reading them as a second string would put a chord
//!   on every distorted single note;
//! - two or more different pitch CLASSES among what is left — an
//!   octave doubling is still one note to a hand on five frets.
//!
//! What a level may hold follows the research's shape table: Expert
//! takes pairs and triples but never green with orange; Hard pairs
//! only, never green with orange; Medium pairs spanning at most two
//! frets on four frets; Easy none. A chord needs room on both sides,
//! counted in beats ([`min_spacing_beats`]): no chord on a sixteenth on
//! Hard and Expert, where chord runs on eighths are the norm (76 % of
//! their chords follow a chord), and no quick chord changes on Medium.
//! And the share is a ceiling, never a target: at most
//! [`MAX_CHORD_SHARE`] of a level's events, the strongest evidence
//! first.
//!
//! ⚠️ The spacing was first taken as 200 ms from a config field of the
//! early games (`min_combo_spacing`) whose MEANING the research marks
//! as unknown. On a real song at 156 BPM it blocked every chord run on
//! eighths — evidence at 36 % of the events, chords written at 9 % —
//! so a number nobody could explain was deciding the ingredient. The
//! rule is now the one the shape table and the chord runs do support.
//!
//! Pure — tested.

use super::ladder::{Event, events_of};
use crate::poly::{PolyFile, PolyNote};
use crate::schema::{ChartDef, ChartNote};
use beatbyte_core::Difficulty;

/// How close to an event a note must start to have been struck with
/// it: a sixteenth at 125 BPM, well over the model's 12 ms frames and
/// a snapped chart note's distance from its onset.
pub const STRUCK_WINDOW_S: f64 = 0.06;

/// A struck note weaker than this is not evidence.
pub const MIN_AMPLITUDE: f32 = 0.35;

/// An overtone counts as a second note only this close to the note
/// under it in strength.
pub const OVERTONE_RATIO: f32 = 0.8;

/// Semitones above a note where its own overtones ring: harmonics 2
/// to 6.
const OVERTONES: [u8; 5] = [12, 19, 24, 28, 31];

/// How close, in beats, a chord's neighbours may be on `level`: a
/// sixteenth (a quarter beat) is too close on Hard and Expert, where
/// chords run on eighths; on Medium, whose authoring rule is "no quick
/// chord changes", a chord wants about a beat of room — chords there
/// change at most on the quarter.
#[must_use]
pub fn min_spacing_beats(level: Difficulty) -> f64 {
    match level {
        Difficulty::Medium | Difficulty::Easy => 0.9,
        Difficulty::Hard | Difficulty::Expert => 0.3,
    }
}

/// At most this share of a level's chords are triples — the most any
/// of the early games wrote (16 %; GH2 6.6 %, GH1 none). A separated
/// `other` stem carries keyboards as well as guitars, and their full
/// chords would otherwise make a fifth of the chords three-fret
/// shapes. The weakest evidence gives way first and becomes a pair.
pub const MAX_TRIPLE_SHARE: f64 = 0.16;

/// At most this share of a level's events become chords (theirs:
/// 27–33 % on Hard, 29–36 % on Expert).
pub const MAX_CHORD_SHARE: f64 = 0.36;

/// What the recording says about one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evidence {
    /// How many different pitch classes were struck (2 or 3; more
    /// reads as 3).
    pub classes: usize,
    /// The interval between the two lowest, in semitones folded into
    /// one octave (1–11).
    pub interval: u8,
    /// How strong the evidence is: the mean amplitude of the struck
    /// notes that counted.
    pub strength: f32,
}

/// The chord evidence at `time_s`, or `None` where the recording has
/// one note (or none) struck there. Pure — tested.
#[must_use]
pub fn evidence_at(poly: &PolyFile, time_s: f64) -> Option<Evidence> {
    let mut struck: Vec<PolyNote> = poly
        .struck_at(time_s, STRUCK_WINDOW_S)
        .into_iter()
        .filter(|n| n.amplitude >= MIN_AMPLITUDE)
        .collect();
    struck.sort_by_key(|n| n.midi);
    // Drop overtones of a lower struck note unless nearly as strong.
    let mut kept: Vec<PolyNote> = Vec::new();
    for note in struck {
        let overtone = kept.iter().any(|low| {
            note.midi > low.midi
                && OVERTONES.contains(&(note.midi - low.midi))
                && note.amplitude < low.amplitude * OVERTONE_RATIO
        });
        if !overtone {
            kept.push(note);
        }
    }
    let mut classes: Vec<u8> = Vec::new();
    let mut lowest_two: Vec<u8> = Vec::new();
    for note in &kept {
        let class = note.midi % 12;
        if !classes.contains(&class) {
            classes.push(class);
            if lowest_two.len() < 2 {
                lowest_two.push(note.midi);
            }
        }
    }
    if classes.len() < 2 {
        return None;
    }
    let interval = (lowest_two[1] - lowest_two[0]) % 12;
    let strength = kept.iter().map(|n| n.amplitude).sum::<f32>() / kept.len() as f32;
    Some(Evidence {
        classes: classes.len().min(3),
        interval: if interval == 0 { 12 } else { interval },
        strength,
    })
}

/// How many frets apart a pair's notes sit for an interval: a second
/// or a third side by side, a fourth or a fifth one fret between (the
/// 1-3 shape most of theirs are), anything wider two frets between.
#[must_use]
pub fn span_for(interval: u8) -> u8 {
    match interval {
        1..=4 => 1,
        5..=7 => 2,
        _ => 3,
    }
}

/// The frets of a chord built around `lane` for `evidence` on
/// `level`, or `None` where the level takes no chord of that kind.
/// The note already there stays one of the frets — the melody's lane
/// is kept, the chord grows from it. Pure — tested.
#[must_use]
pub fn shape(lane: u8, evidence: Evidence, level: Difficulty) -> Option<Vec<u8>> {
    let (top, triples, max_span) = match level {
        Difficulty::Easy => return None,
        Difficulty::Medium => (3u8, false, 2u8),
        Difficulty::Hard => (4, false, 3),
        Difficulty::Expert => (4, true, 3),
    };
    let lane = lane.min(top);
    if triples && evidence.classes >= 3 {
        // Three neighbouring frets, as close to the melody's lane as
        // the neck allows.
        let low = lane.min(top - 2);
        return Some(vec![low, low + 1, low + 2]);
    }
    // Never more than three frets apart, so green with orange — never
    // a chord in these games — cannot come out of this.
    //
    // ⚠️ In the middle of the neck a wide pair may fit neither way —
    // red-to-orange on yellow is three up and there are two frets
    // above yellow and two below. The pair is then drawn in to the
    // room there is, on the roomier side (upwards on a tie), rather
    // than reaching below green. The first version subtracted blindly
    // and a real song's first wide chord on yellow panicked it.
    let wanted = span_for(evidence.interval).min(max_span);
    let (up, down) = (top - lane, lane);
    Some(if wanted <= up {
        vec![lane, lane + wanted]
    } else if wanted <= down {
        vec![lane - wanted, lane]
    } else if up >= down {
        vec![lane, lane + up]
    } else {
        vec![lane - down, lane]
    })
}

/// Write chords into one level where the evidence has them. Returns
/// how many events became chords. Existing chords are left as they
/// are. Pure — tested.
pub fn chord_level(def: &mut ChartDef, poly: &PolyFile, beats: &super::Beats) -> usize {
    let events = events_of(&def.notes);
    let count = events.len();
    // Candidates: single notes with room, strongest evidence first.
    let mut candidates: Vec<(usize, Vec<u8>, Evidence, u8)> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if event.notes.len() != 1 {
            continue;
        }
        let beat = beats.at(event.time);
        let beat = if beat > 0.0 { beat } else { 0.5 };
        let spacing = min_spacing_beats(def.difficulty) * beat;
        let room =
            |other: Option<&Event>| other.is_none_or(|o| (o.time - event.time).abs() >= spacing);
        if !room(index.checked_sub(1).map(|i| &events[i])) || !room(events.get(index + 1)) {
            continue;
        }
        let Some(evidence) = evidence_at(poly, event.time) else {
            continue;
        };
        let lane = event.notes[0].lane;
        if let Some(lanes) = shape(lane, evidence, def.difficulty) {
            candidates.push((index, lanes, evidence, lane));
        }
    }
    let existing = events.iter().filter(|e| e.notes.len() > 1).count();
    let ceiling = ((count as f64) * MAX_CHORD_SHARE).floor() as usize;
    let room_left = ceiling.saturating_sub(existing);
    candidates.sort_by(|a, b| b.2.strength.total_cmp(&a.2.strength).then(a.0.cmp(&b.0)));
    candidates.truncate(room_left);
    let chosen = candidates.len();
    // Triples past their ceiling become pairs, weakest first (the list
    // is strongest first, so the ones kept are the first ones).
    let triples_allowed = ((chosen as f64) * MAX_TRIPLE_SHARE).floor() as usize;
    let mut triples = 0usize;
    for (_, lanes, evidence, lane) in &mut candidates {
        if lanes.len() < 3 {
            continue;
        }
        triples += 1;
        if triples > triples_allowed {
            let pair = Evidence {
                classes: 2,
                ..*evidence
            };
            if let Some(shaped) = shape(*lane, pair, def.difficulty) {
                *lanes = shaped;
            }
        }
    }

    let mut out: Vec<ChartNote> = Vec::with_capacity(def.notes.len() + chosen * 2);
    for (index, event) in events.into_iter().enumerate() {
        match candidates.iter().find(|(i, ..)| *i == index) {
            Some((_, lanes, ..)) => {
                let base = event.notes[0];
                for lane in lanes {
                    out.push(ChartNote {
                        lane: *lane,
                        // A chord is strummed in every one of these
                        // games; the flag is cleared here, not left to
                        // the hammer-on ingredient.
                        hopo: false,
                        ..base
                    });
                }
            }
            None => out.extend(event.notes),
        }
    }
    def.notes = out;
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poly::POLY_FORMAT;

    fn poly(notes: &[(f64, u8, f32)]) -> PolyFile {
        let mut notes: Vec<PolyNote> = notes
            .iter()
            .map(|(start_s, midi, amplitude)| PolyNote {
                start_s: *start_s,
                end_s: start_s + 0.4,
                midi: *midi,
                amplitude: *amplitude,
            })
            .collect();
        notes.sort_by(|a, b| a.start_s.total_cmp(&b.start_s).then(a.midi.cmp(&b.midi)));
        PolyFile {
            format: POLY_FORMAT.to_owned(),
            model: "basic-pitch".to_owned(),
            model_sha256: "x".to_owned(),
            source: "other stem".to_owned(),
            notes,
        }
    }

    #[test]
    fn a_power_chord_is_two_classes_a_fifth_apart() {
        // E2 + B2 + E3: root, fifth, octave.
        let p = poly(&[(1.0, 40, 0.8), (1.01, 47, 0.7), (1.0, 52, 0.7)]);
        let evidence = evidence_at(&p, 1.0).expect("a chord");
        assert_eq!(evidence.classes, 2);
        assert_eq!(evidence.interval, 7);
    }

    #[test]
    fn one_note_and_its_octave_are_one_note() {
        let p = poly(&[(1.0, 40, 0.8), (1.0, 52, 0.8)]);
        assert_eq!(evidence_at(&p, 1.0), None);
    }

    /// ⚠️ A single plucked string rings an octave and a fifth above
    /// itself; read as a second note it would chord every distorted
    /// single note. A weak one is the string's own overtone …
    #[test]
    fn an_overtone_of_one_note_is_not_a_second_note() {
        let p = poly(&[(1.0, 40, 0.8), (1.0, 59, 0.4)]);
        assert_eq!(evidence_at(&p, 1.0), None);
        // … one nearly as strong as the note under it is a string of
        // its own.
        let p = poly(&[(1.0, 40, 0.8), (1.0, 59, 0.75)]);
        assert!(evidence_at(&p, 1.0).is_some());
        // And a fifth that is NOT an overtone interval counts even
        // when weak.
        let p = poly(&[(1.0, 40, 0.8), (1.0, 47, 0.4)]);
        assert!(evidence_at(&p, 1.0).is_some());
    }

    #[test]
    fn only_what_starts_near_the_moment_is_struck_with_it() {
        let p = poly(&[(1.0, 40, 0.8), (1.2, 47, 0.8)]);
        assert_eq!(evidence_at(&p, 1.0), None, "a later note joined the chord");
        let p = poly(&[(1.0, 40, 0.8), (1.0, 47, MIN_AMPLITUDE - 0.01)]);
        assert_eq!(evidence_at(&p, 1.0), None, "a weak note counted");
    }

    #[test]
    fn three_classes_read_as_three() {
        let p = poly(&[
            (1.0, 40, 0.8),
            (1.0, 44, 0.8),
            (1.0, 47, 0.8),
            (1.0, 50, 0.8),
        ]);
        let evidence = evidence_at(&p, 1.0).expect("a chord");
        assert_eq!(evidence.classes, 3);
        assert_eq!(evidence.interval, 4, "a major third at the bottom");
    }

    #[test]
    fn the_interval_decides_how_wide_the_pair_is() {
        assert_eq!(span_for(3), 1);
        assert_eq!(span_for(4), 1);
        assert_eq!(span_for(5), 2);
        assert_eq!(span_for(7), 2);
        assert_eq!(span_for(9), 3);
        assert_eq!(span_for(12), 3);
    }

    fn ev(classes: usize, interval: u8) -> Evidence {
        Evidence {
            classes,
            interval,
            strength: 0.8,
        }
    }

    #[test]
    fn each_level_takes_the_shapes_it_allows() {
        // A fifth on red: red + blue on every level that takes one.
        assert_eq!(shape(1, ev(2, 7), Difficulty::Hard), Some(vec![1, 3]));
        assert_eq!(shape(1, ev(2, 7), Difficulty::Medium), Some(vec![1, 3]));
        assert_eq!(shape(1, ev(2, 7), Difficulty::Easy), None);
        // At the top of the neck the pair grows downwards.
        assert_eq!(shape(4, ev(2, 7), Difficulty::Hard), Some(vec![2, 4]));
        // Medium has four frets: an orange melody note is on blue.
        assert_eq!(shape(4, ev(2, 3), Difficulty::Medium), Some(vec![2, 3]));
        // Triples only on Expert, near the melody's fret.
        assert_eq!(shape(4, ev(3, 4), Difficulty::Expert), Some(vec![2, 3, 4]));
        assert_eq!(shape(4, ev(3, 4), Difficulty::Hard), Some(vec![3, 4]));
        // Medium never spans more than two frets.
        assert_eq!(shape(0, ev(2, 9), Difficulty::Medium), Some(vec![0, 2]));
        // The widest pair is three apart: green with orange never comes.
        assert_eq!(shape(0, ev(2, 12), Difficulty::Expert), Some(vec![0, 3]));
        assert_eq!(shape(4, ev(2, 12), Difficulty::Expert), Some(vec![1, 4]));
    }

    /// ⚠️ Every lane, every interval, every class count, every level:
    /// a shape never panics, keeps the melody's fret, stays on the
    /// level's frets, spans what the level allows and is never green
    /// with orange. The first version was tested on the neck's edges
    /// only, and a wide pair on yellow underflowed on a real song.
    #[test]
    fn every_shape_is_one_the_level_can_hold() {
        for level in Difficulty::ALL {
            for lane in 0..=4u8 {
                for interval in 1..=12u8 {
                    for classes in 2..=3usize {
                        let Some(frets) = shape(lane, ev(classes, interval), level) else {
                            assert_eq!(level, Difficulty::Easy);
                            continue;
                        };
                        let top = if level == Difficulty::Medium { 3 } else { 4 };
                        let context = format!("{level:?} lane {lane} interval {interval}");
                        assert!(frets.contains(&lane.min(top)), "{context}: {frets:?}");
                        assert!(frets.iter().all(|f| *f <= top), "{context}: {frets:?}");
                        assert!(frets.windows(2).all(|w| w[0] < w[1]), "{context}");
                        assert_ne!(frets, vec![0, 4], "{context}");
                        let span = frets[frets.len() - 1] - frets[0];
                        let max = if level == Difficulty::Medium { 2 } else { 3 };
                        assert!(span <= max, "{context}: {frets:?}");
                        if level != Difficulty::Expert {
                            assert_eq!(frets.len(), 2, "{context}");
                        }
                    }
                }
            }
        }
        // The case that panicked: a wide pair on yellow.
        assert_eq!(shape(2, ev(2, 9), Difficulty::Hard), Some(vec![2, 4]));
    }

    fn def(difficulty: Difficulty, times: &[(f64, u8)]) -> ChartDef {
        ChartDef {
            difficulty,
            lanes: 5,
            notes: times
                .iter()
                .map(|(time, lane)| ChartNote {
                    time: *time,
                    lane: *lane,
                    len: 0.25,
                    hopo: true,
                })
                .collect(),
            phrases: vec![],
        }
    }

    #[test]
    fn a_level_is_chorded_where_the_evidence_is_and_nowhere_else() {
        let mut level = def(Difficulty::Hard, &[(1.0, 1), (2.0, 2), (3.0, 3)]);
        let p = poly(&[(1.0, 40, 0.8), (1.0, 47, 0.8), (2.0, 45, 0.8)]);
        assert_eq!(chord_level(&mut level, &p, &beats()), 1);
        let at = |t: f64| {
            level
                .notes
                .iter()
                .filter(|n| n.time == t)
                .map(|n| n.lane)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(1.0), vec![1, 3]);
        assert_eq!(at(2.0), vec![2], "a single struck note became a chord");
        assert_eq!(at(3.0), vec![3]);
        // The chord keeps the note's length and is never a hammer-on.
        let chord: Vec<&ChartNote> = level.notes.iter().filter(|n| n.time == 1.0).collect();
        assert!(
            chord
                .iter()
                .all(|n| (n.len - 0.25).abs() < 1e-12 && !n.hopo)
        );
    }

    /// 120 BPM: a beat is half a second.
    fn beats() -> super::super::Beats {
        super::super::Beats::constant(0.5)
    }

    /// Room is counted in beats: at 120 BPM a run of eighths (0.25 s)
    /// leaves room on Hard and not on Medium.
    #[test]
    fn a_chord_run_on_eighths_is_allowed_where_the_level_allows_it() {
        let run: Vec<(f64, u8)> = (0..8).map(|i| (1.0 + f64::from(i) * 0.25, 1)).collect();
        let notes: Vec<(f64, u8, f32)> = (0..8)
            .flat_map(|i| {
                let t = 1.0 + f64::from(i) * 0.25;
                [(t, 40, 0.8), (t, 47, 0.8)]
            })
            .collect();
        let mut hard = def(Difficulty::Hard, &run);
        assert!(
            chord_level(&mut hard, &poly(&notes), &beats()) > 0,
            "no chord on eighths"
        );
        let mut medium = def(Difficulty::Medium, &run);
        assert_eq!(chord_level(&mut medium, &poly(&notes), &beats()), 0);
        assert!((min_spacing_beats(Difficulty::Hard) - 0.3).abs() < 1e-12);
    }

    #[test]
    fn a_chord_needs_room_on_both_sides() {
        let mut level = def(Difficulty::Hard, &[(1.0, 1), (1.1, 2), (3.0, 3)]);
        let p = poly(&[
            (1.0, 40, 0.8),
            (1.0, 47, 0.8),
            (3.0, 40, 0.8),
            (3.0, 47, 0.8),
        ]);
        assert_eq!(
            chord_level(&mut level, &p, &beats()),
            1,
            "a crowded note took a chord"
        );
        assert_eq!(level.notes.iter().filter(|n| n.time == 1.0).count(), 1);
    }

    /// The share is a ceiling: where the evidence says "everything is
    /// a chord", the strongest evidence wins and the rest stays single.
    #[test]
    fn the_chord_share_is_a_ceiling_strongest_first() {
        let times: Vec<(f64, u8)> = (0..10).map(|i| (f64::from(i), 1)).collect();
        let mut level = def(Difficulty::Hard, &times);
        let notes: Vec<(f64, u8, f32)> = (0..10)
            .flat_map(|i| {
                let strength = 0.4 + 0.05 * i as f32;
                [(f64::from(i), 40, strength), (f64::from(i), 47, strength)]
            })
            .collect();
        let chosen = chord_level(&mut level, &poly(&notes), &beats());
        assert_eq!(chosen, 3, "36 % of ten events");
        let chorded: Vec<f64> = events_of(&level.notes)
            .iter()
            .filter(|e| e.notes.len() > 1)
            .map(|e| e.time)
            .collect();
        assert_eq!(chorded, vec![7.0, 8.0, 9.0], "not the strongest evidence");
    }

    /// A chord already in the chart is its author's: the evidence
    /// never reshapes it, even where it would draw it differently.
    #[test]
    fn an_existing_chord_is_left_as_it_is() {
        // Ten events, so the ceiling (three) leaves room for a chord.
        let mut times: Vec<(f64, u8)> = (2..11).map(|i| (f64::from(i), 0)).collect();
        times.extend([(1.0, 1), (1.0, 2)]);
        let mut level = def(Difficulty::Hard, &times);
        let p = poly(&[(1.0, 40, 0.8), (1.0, 47, 0.8)]);
        assert_eq!(chord_level(&mut level, &p, &beats()), 0);
        let at_one: Vec<u8> = level
            .notes
            .iter()
            .filter(|n| n.time == 1.0)
            .map(|n| n.lane)
            .collect();
        assert_eq!(at_one, vec![1, 2], "a fifth redrew an existing chord");
    }

    /// Triples are held to their share of the chords; the weakest past
    /// it become pairs, and the pair keeps the melody's fret.
    #[test]
    fn triples_are_held_to_their_ceiling() {
        // Twenty-five well-spaced events, every one a strong triad.
        // On blue: a pair that forgot the melody's fret would start
        // from green and could still happen to contain red.
        let times: Vec<(f64, u8)> = (0..25).map(|i| (f64::from(i), 3)).collect();
        let mut level = def(Difficulty::Expert, &times);
        let notes: Vec<(f64, u8, f32)> = (0..25)
            .flat_map(|i| {
                let strength = 0.5 + 0.01 * i as f32;
                let t = f64::from(i);
                [(t, 40, strength), (t, 44, strength), (t, 47, strength)]
            })
            .collect();
        let chosen = chord_level(&mut level, &poly(&notes), &beats());
        assert_eq!(chosen, 9, "36 % of 25");
        let shapes: Vec<(f64, usize)> = events_of(&level.notes)
            .iter()
            .filter(|e| e.notes.len() > 1)
            .map(|e| (e.time, e.notes.len()))
            .collect();
        let triples: Vec<f64> = shapes
            .iter()
            .filter(|(_, n)| *n == 3)
            .map(|(t, _)| *t)
            .collect();
        // 16 % of nine chords is one triple: the strongest evidence.
        assert_eq!(triples, vec![24.0], "not the strongest triad kept");
        assert!(shapes.iter().all(|(_, n)| *n <= 3));
        assert!(
            level
                .notes
                .iter()
                .filter(|n| n.time == 16.0)
                .any(|n| n.lane == 3),
            "a downgraded pair lost the melody's fret"
        );
    }

    #[test]
    fn existing_chords_are_kept_and_count_against_the_ceiling() {
        let mut times: Vec<(f64, u8)> = (0..10).map(|i| (f64::from(i), 1)).collect();
        times.extend([(0.0, 3), (1.0, 3), (2.0, 3)]);
        let mut level = def(Difficulty::Hard, &times);
        let notes: Vec<(f64, u8, f32)> = (0..10)
            .flat_map(|i| [(f64::from(i), 40, 0.8), (f64::from(i), 47, 0.8)])
            .collect();
        assert_eq!(chord_level(&mut level, &poly(&notes), &beats()), 0);
    }
}
