//! The difficulty ladder the early games wrote (K3, K4): each level is
//! the level above it **with notes taken away** — never moved in
//! time, never added.
//!
//! Measured on their charts (the research note, §1): every level is a
//! 99–100 % subset of the next one up; Hard keeps 88–90 % of Expert's
//! notes, Medium 61–76 % at 2.0–2.45 notes a second with 57–63 % of
//! them on a beat. Ours were generated per level from a shared master
//! and sit elsewhere — Hard at 71 % of Expert with a fixed 0.16 s
//! minimum spacing that no fast pair survives, Medium at 1.54 notes a
//! second with half of them on a beat.
//!
//! So a classic level is DERIVED: Hard from the chart's own Expert,
//! Medium from its Hard. What is taken away first is decided by
//! `deletion_score` — crowded notes before spacious ones, off-beat
//! sixteenths before eighths before beats, a fret change inside a
//! fast run before a repeated fret — and it is recomputed after every
//! deletion, because removing one note changes how crowded its
//! neighbours are.
//!
//! ⚠️ Only numbers and rules from the research come in here. The
//! thresholds are facts about how those levels relate; no chart of
//! anybody else's music is read.

use super::Beats;
use crate::convert::CHORD_EPSILON_S;
use crate::schema::{ChartDef, ChartNote, ChartPhrase};
use beatbyte_core::Difficulty;

/// The share of Expert's note events a classic Hard keeps (theirs:
/// 88–90 %).
pub const HARD_SHARE: f64 = 0.89;

/// The band of Expert's note events a classic Medium keeps (theirs:
/// 61–76 %).
pub const MEDIUM_SHARE: (f64, f64) = (0.61, 0.76);

/// The density a classic Medium aims for inside that band (theirs:
/// 2.0–2.45 notes a second).
pub const MEDIUM_NOTES_PER_SECOND: f64 = 2.2;

/// A gap of this many beats ends a passage: the five frets fold to
/// four one passage at a time (`fold_to_four`).
pub const BREATH_BEATS: f64 = 2.0;

/// How far from a subdivision a note may sit and still be on it, as a
/// share of the beat. Generated notes are snapped; this only has to
/// absorb float noise and a tracked grid's wobble.
const PHASE_TOLERANCE: f64 = 0.06;

/// A note event: the notes struck together, lanes ascending.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Event {
    /// When it is struck.
    pub time: f64,
    /// Its notes, by ascending lane.
    pub notes: Vec<ChartNote>,
}

impl Event {
    fn lanes(&self) -> Vec<u8> {
        self.notes.iter().map(|n| n.lane).collect()
    }

    fn is_chord(&self) -> bool {
        self.notes.len() > 1
    }

    fn is_sustain(&self) -> bool {
        self.notes.iter().any(|n| n.len > 0.0)
    }
}

/// Group notes into events the way the engine does
/// ([`CHORD_EPSILON_S`], anchored at the first note), in time order.
pub(crate) fn events_of(notes: &[ChartNote]) -> Vec<Event> {
    let mut sorted = notes.to_vec();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.lane.cmp(&b.lane)));
    let mut out: Vec<Event> = Vec::new();
    for note in sorted {
        match out.last_mut() {
            Some(last) if (note.time - last.time).abs() <= CHORD_EPSILON_S => {
                if !last.notes.iter().any(|n| n.lane == note.lane) {
                    last.notes.push(note);
                }
            }
            _ => out.push(Event {
                time: note.time,
                notes: vec![note],
            }),
        }
    }
    for event in &mut out {
        event.notes.sort_by_key(|n| n.lane);
    }
    out
}

/// The times of a level's note events, the engine's grouping.
#[must_use]
pub fn event_times(notes: &[ChartNote]) -> Vec<f64> {
    events_of(notes).iter().map(|e| e.time).collect()
}

/// Where in the beat an event falls, coarsest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Position {
    /// On a beat.
    Beat,
    /// On the off-beat eighth.
    Eighth,
    /// On a triplet third.
    Triplet,
    /// Anywhere finer: a sixteenth, or off the grid.
    Sixteenth,
}

impl Position {
    /// Classify a phase `[0, 1)` within the beat. Without one — no
    /// grid, no tempo — every event reads as on the beat, and the
    /// ladder falls back on crowding alone.
    #[must_use]
    pub fn of(phase: Option<f64>) -> Position {
        let Some(phase) = phase else {
            return Position::Beat;
        };
        let near = |target: f64| (phase - target).abs() <= PHASE_TOLERANCE;
        if near(0.0) || near(1.0) {
            Position::Beat
        } else if near(0.5) {
            Position::Eighth
        } else if near(1.0 / 3.0) || near(2.0 / 3.0) {
            Position::Triplet
        } else {
            Position::Sixteenth
        }
    }

    /// How readily a note here is taken away.
    fn weight(self) -> f64 {
        match self {
            Position::Beat => 0.0,
            Position::Eighth => 0.4,
            Position::Triplet => 0.6,
            Position::Sixteenth => 0.8,
        }
    }
}

/// How readily an event is taken away, given the events still kept
/// on either side of it. Higher goes first. Pure — tested.
///
/// - **crowding**, `0` at half a beat or more to the nearer neighbour
///   and `1` at nothing: the hardest passages give way first;
/// - **position**: a beat is kept longest, then the off-beat eighth,
///   then a triplet, then a sixteenth;
/// - **movement**: a fret change inside a fast run (under half a
///   beat) goes before a repeated fret — the rule the authoring
///   guides give for Hard, "no sixteenth-note movement";
/// - a **chord** and a **sustain** are protected: they are the
///   accents a level is built on.
pub(crate) fn deletion_score(
    event: &Event,
    before: Option<&Event>,
    after: Option<&Event>,
    beats: &Beats,
) -> f64 {
    let beat = beats.at(event.time);
    let beat = if beat > 0.0 { beat } else { 0.5 };
    let gap = |other: Option<&Event>| other.map_or(f64::INFINITY, |o| (event.time - o.time).abs());
    let nearest = gap(before).min(gap(after)) / beat;
    let crowding = ((0.5 - nearest) / 0.5).clamp(0.0, 1.0);
    let position = Position::of(beats.phase(event.time)).weight();
    let moves = match before {
        Some(previous) if nearest < 0.5 && previous.lanes() != event.lanes() => 0.2,
        _ => 0.0,
    };
    let mut score = crowding + position + moves;
    if event.is_chord() {
        score -= 0.5;
    }
    if event.is_sustain() {
        score -= 0.3;
    }
    score
}

/// Keep `keep` of `events`, taking away the highest
/// `deletion_score` one at a time and re-scoring its neighbours.
/// The first event is never taken — a song starts where it starts.
/// Deterministic: ties go to the earlier event. Pure — tested.
pub(crate) fn thin(events: Vec<Event>, keep: usize, beats: &Beats) -> Vec<Event> {
    let count = events.len();
    let keep = keep.max(1);
    if keep >= count {
        return events;
    }
    let mut alive = vec![true; count];
    // Neighbours among the survivors, as a doubly linked list.
    let mut previous: Vec<Option<usize>> = (0..count).map(|i| i.checked_sub(1)).collect();
    let mut next: Vec<Option<usize>> = (0..count)
        .map(|i| (i + 1 < count).then_some(i + 1))
        .collect();
    let score_of = |i: usize, previous: &[Option<usize>], next: &[Option<usize>]| {
        deletion_score(
            &events[i],
            previous[i].map(|p| &events[p]),
            next[i].map(|n| &events[n]),
            beats,
        )
    };
    let mut scores: Vec<f64> = (0..count).map(|i| score_of(i, &previous, &next)).collect();
    let mut left = count;
    while left > keep {
        let mut best: Option<usize> = None;
        for i in 1..count {
            if alive[i] && best.is_none_or(|b| scores[i] > scores[b]) {
                best = Some(i);
            }
        }
        let Some(gone) = best else {
            break;
        };
        alive[gone] = false;
        left -= 1;
        let (p, n) = (previous[gone], next[gone]);
        if let Some(p) = p {
            next[p] = n;
        }
        if let Some(n) = n {
            previous[n] = p;
        }
        for neighbour in [p, n].into_iter().flatten() {
            scores[neighbour] = score_of(neighbour, &previous, &next);
        }
    }
    events
        .into_iter()
        .zip(alive)
        .filter_map(|(event, alive)| alive.then_some(event))
        .collect()
}

/// Which chord shapes a level allows, and how a shape it does not
/// allow is brought into it.
///
/// - **Hard**: pairs only, never green with orange. A triple keeps its
///   outer frets (the 1-3 shape most of theirs are), or its lower two
///   when the outer pair would be green–orange; a green–orange pair
///   keeps green.
/// - **Medium**: pairs spanning at most two frets. A triple keeps its
///   outer frets; a wider pair is drawn in to span two — "chords one
///   position lower", in the authoring guides' words.
/// - Easy and Expert are not shaped here.
fn shape_chord(event: &mut Event, level: Difficulty) {
    if !event.is_chord() {
        return;
    }
    match level {
        Difficulty::Hard => {
            let low = event.notes[0].lane;
            let high = event.notes[event.notes.len() - 1].lane;
            let keep: Vec<u8> = if event.notes.len() >= 3 {
                if (low, high) == (0, 4) {
                    vec![low, event.notes[1].lane]
                } else {
                    vec![low, high]
                }
            } else if (low, high) == (0, 4) {
                vec![low]
            } else {
                vec![low, high]
            };
            event.notes.retain(|n| keep.contains(&n.lane));
        }
        Difficulty::Medium => {
            let last = event.notes.len() - 1;
            if last >= 2 {
                let (first, final_note) = (event.notes[0], event.notes[last]);
                event.notes = vec![first, final_note];
            }
            let low = event.notes[0].lane;
            if event.notes[1].lane > low + 2 {
                event.notes[1].lane = low + 2;
            }
        }
        Difficulty::Easy | Difficulty::Expert => {}
    }
}

/// Fold five frets to four, one passage at a time (a passage ends at
/// a gap of [`BREATH_BEATS`]). A passage that never reaches orange is
/// left alone; one that never touches green moves down a fret whole,
/// so every step it takes survives; only a passage that spans all
/// five loses its top — orange joins blue there, the flattening the
/// early games' lower levels show too. Returns how many notes moved.
pub(crate) fn fold_to_four(events: &mut [Event], beats: &Beats) -> usize {
    let mut moved = 0usize;
    let mut start = 0usize;
    while start < events.len() {
        let mut end = start + 1;
        while end < events.len() {
            let beat = beats.at(events[end].time);
            let breath = if beat > 0.0 { BREATH_BEATS * beat } else { 1.0 };
            if events[end].time - events[end - 1].time >= breath {
                break;
            }
            end += 1;
        }
        let passage = &mut events[start..end];
        let lanes = passage.iter().flat_map(|e| e.notes.iter().map(|n| n.lane));
        let (low, high) = lanes.fold((u8::MAX, 0u8), |(lo, hi), l| (lo.min(l), hi.max(l)));
        if high > 3 {
            for event in passage.iter_mut() {
                for note in &mut event.notes {
                    let folded = if low >= 1 {
                        note.lane - 1
                    } else {
                        note.lane.min(3)
                    };
                    if folded != note.lane {
                        note.lane = folded;
                        moved += 1;
                    }
                }
                // Two notes of one chord landing on one fret are one.
                event.notes.dedup_by_key(|n| n.lane);
            }
        }
        start = end;
    }
    moved
}

/// The phrases of the level above that still hold a note on this one.
/// Every level keeps the SAME phrases — theirs carry the same number
/// of phrases on each — and one left empty would be a phrase nobody
/// can complete.
fn phrases_for(phrases: &[ChartPhrase], events: &[Event]) -> Vec<ChartPhrase> {
    phrases
        .iter()
        .filter(|p| events.iter().any(|e| e.time >= p.start && e.time <= p.end))
        .copied()
        .collect()
}

/// Flatten events back into notes. A hammer-on flag survives only
/// where the note before it is still the note it was hammered from:
/// a note whose predecessor was taken away would otherwise hammer
/// across a gap that is no longer a run. (The HOPO ingredient
/// re-flags every note anyway when it is on.)
fn notes_of(source: &[Event], kept: &[Event]) -> Vec<ChartNote> {
    let predecessor = |time: f64| -> Option<f64> {
        source
            .iter()
            .take_while(|e| e.time < time - CHORD_EPSILON_S)
            .last()
            .map(|e| e.time)
    };
    let mut out = Vec::new();
    let mut previous_kept: Option<f64> = None;
    for event in kept {
        let same_run = match (predecessor(event.time), previous_kept) {
            (Some(was), Some(is)) => (was - is).abs() <= CHORD_EPSILON_S,
            (None, None) => true,
            _ => false,
        };
        for note in &event.notes {
            let mut note = *note;
            note.hopo = note.hopo && same_run && !event.is_chord();
            out.push(note);
        }
        previous_kept = Some(event.time);
    }
    out
}

/// A classic Hard: the Expert's own events, [`HARD_SHARE`] of them,
/// with chords shaped for Hard (`shape_chord`). Pure — tested.
#[must_use]
pub fn derive_hard(expert: &ChartDef, beats: &Beats) -> ChartDef {
    let source = events_of(&expert.notes);
    let keep = ((source.len() as f64) * HARD_SHARE).round() as usize;
    let mut kept = thin(source.clone(), keep, beats);
    for event in &mut kept {
        shape_chord(event, Difficulty::Hard);
    }
    ChartDef {
        difficulty: Difficulty::Hard,
        lanes: expert.lanes,
        notes: notes_of(&source, &kept),
        phrases: phrases_for(&expert.phrases, &kept),
    }
}

/// How many events a classic Medium keeps: [`MEDIUM_NOTES_PER_SECOND`]
/// over the song's span, held inside [`MEDIUM_SHARE`] of Expert's
/// count, and never more than the Hard it is taken from. Pure —
/// tested.
#[must_use]
pub fn medium_target(expert_events: usize, span_s: f64, hard_events: usize) -> usize {
    let floor = ((expert_events as f64) * MEDIUM_SHARE.0).ceil();
    let ceiling = ((expert_events as f64) * MEDIUM_SHARE.1).floor();
    let wanted = (MEDIUM_NOTES_PER_SECOND * span_s.max(0.0)).round();
    let target = wanted.clamp(floor.min(ceiling), ceiling) as usize;
    target.min(hard_events)
}

/// A classic Medium: the Hard it is given, thinned to
/// [`medium_target`] events, folded to four frets and with chords
/// shaped for Medium. `expert` fixes the band and the span. Returns
/// the level and how many notes the fold moved. Pure — tested.
#[must_use]
pub fn derive_medium(hard: &ChartDef, expert: &ChartDef, beats: &Beats) -> (ChartDef, usize) {
    let source = events_of(&hard.notes);
    let expert_events = events_of(&expert.notes);
    let span = match (expert_events.first(), expert_events.last()) {
        (Some(first), Some(last)) => last.time - first.time,
        _ => 0.0,
    };
    let keep = medium_target(expert_events.len(), span, source.len());
    let mut kept = thin(source.clone(), keep, beats);
    let moved = fold_to_four(&mut kept, beats);
    for event in &mut kept {
        shape_chord(event, Difficulty::Medium);
    }
    let level = ChartDef {
        difficulty: Difficulty::Medium,
        lanes: hard.lanes,
        notes: notes_of(&source, &kept),
        phrases: phrases_for(&hard.phrases, &kept),
    };
    (level, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    fn event(time: f64, lanes: &[u8]) -> Event {
        Event {
            time,
            notes: lanes.iter().map(|l| note(time, *l)).collect(),
        }
    }

    fn def(difficulty: Difficulty, notes: Vec<ChartNote>) -> ChartDef {
        ChartDef {
            difficulty,
            lanes: 5,
            notes,
            phrases: vec![],
        }
    }

    fn times(events: &[Event]) -> Vec<f64> {
        events.iter().map(|e| e.time).collect()
    }

    /// 120 BPM: a beat is half a second.
    fn beats() -> Beats {
        Beats::constant(0.5)
    }

    #[test]
    fn positions_read_the_beat_from_coarse_to_fine() {
        assert_eq!(Position::of(Some(0.0)), Position::Beat);
        assert_eq!(Position::of(Some(0.98)), Position::Beat, "wrap-around");
        assert_eq!(Position::of(Some(0.5)), Position::Eighth);
        assert_eq!(Position::of(Some(1.0 / 3.0)), Position::Triplet);
        assert_eq!(Position::of(Some(2.0 / 3.0)), Position::Triplet);
        assert_eq!(Position::of(Some(0.25)), Position::Sixteenth);
        // The tolerance on either side of the "and", and no further.
        assert_eq!(Position::of(Some(0.45)), Position::Eighth);
        assert_eq!(Position::of(Some(0.55)), Position::Eighth);
        assert_eq!(Position::of(Some(0.43)), Position::Sixteenth);
        assert_eq!(Position::of(Some(0.57)), Position::Sixteenth);
        assert_eq!(Position::of(Some(0.75)), Position::Sixteenth);
        assert_eq!(
            Position::of(None),
            Position::Beat,
            "no grid: crowding alone"
        );
        let b = beats();
        assert_eq!(Position::of(b.phase(1.0)), Position::Beat);
        assert_eq!(Position::of(b.phase(1.25)), Position::Eighth);
        assert_eq!(Position::of(b.phase(1.125)), Position::Sixteenth);
    }

    /// The ladder's whole promise: taking away only. Every event of
    /// the thinned level is an event of the source, at the same time
    /// with the same frets.
    #[test]
    fn thinning_only_takes_away() {
        let source: Vec<Event> = (0..40)
            .map(|i| event(f64::from(i) * 0.125, &[(i % 5) as u8]))
            .collect();
        let kept = thin(source.clone(), 25, &beats());
        assert_eq!(kept.len(), 25);
        for event in &kept {
            assert!(source.contains(event), "{event:?} is not in the source");
        }
        assert_eq!(kept[0], source[0], "the first event was taken away");
        let mut sorted = times(&kept);
        sorted.sort_by(f64::total_cmp);
        assert_eq!(sorted, times(&kept), "the order changed");
    }

    /// A run of sixteenths thins to eighths: the off-beat sixteenths
    /// go, the beats and the "and"s stay.
    #[test]
    fn a_sixteenth_run_thins_to_eighths() {
        let source: Vec<Event> = (0..16)
            .map(|i| event(1.0 + f64::from(i) * 0.125, &[(i % 3) as u8]))
            .collect();
        let kept = thin(source, 8, &beats());
        for event in &kept {
            let position = Position::of(beats().phase(event.time));
            assert!(
                matches!(position, Position::Beat | Position::Eighth),
                "a sixteenth survived at {} while an eighth went",
                event.time
            );
        }
    }

    /// Crowded before spacious: the same off-beat position is kept
    /// where it stands alone and taken where it is in a run.
    #[test]
    fn a_crowded_note_goes_before_a_lone_one() {
        let b = beats();
        // A lone sixteenth-position note far from anything …
        let lone = event(2.125, &[1]);
        let lone_score = deletion_score(&lone, Some(&event(1.0, &[1])), None, &b);
        // … and the same position inside a run — on the same fret, so
        // only the crowding differs.
        let crowded = event(4.125, &[1]);
        let crowded_score = deletion_score(
            &crowded,
            Some(&event(4.0, &[1])),
            Some(&event(4.25, &[1])),
            &b,
        );
        assert!(
            crowded_score > lone_score,
            "{crowded_score} <= {lone_score}"
        );
    }

    /// Inside a fast run a fret change goes before a repeated fret —
    /// "no sixteenth-note movement" on Hard.
    #[test]
    fn a_fret_change_in_a_run_goes_before_a_repeat() {
        let b = beats();
        let before = event(1.0, &[2]);
        let moving = deletion_score(&event(1.125, &[3]), Some(&before), None, &b);
        let repeat = deletion_score(&event(1.125, &[2]), Some(&before), None, &b);
        assert!(moving > repeat);
        // Outside a run the fret does not matter.
        let far = event(0.0, &[2]);
        let moving = deletion_score(&event(1.125, &[3]), Some(&far), None, &b);
        let repeat = deletion_score(&event(1.125, &[2]), Some(&far), None, &b);
        assert!((moving - repeat).abs() < 1e-12);
    }

    /// Chords and sustains are the accents a level is built on.
    #[test]
    fn chords_and_sustains_are_protected() {
        let b = beats();
        let before = event(1.0, &[0]);
        let single = deletion_score(&event(1.125, &[1]), Some(&before), None, &b);
        let chord = deletion_score(&event(1.125, &[1, 3]), Some(&before), None, &b);
        let mut held = event(1.125, &[1]);
        held.notes[0].len = 1.0;
        let sustain = deletion_score(&held, Some(&before), None, &b);
        assert!(chord < single && sustain < single);
    }

    /// ⚠️ Re-scored after every deletion: once a neighbour is gone the
    /// note beside it is no longer crowded. Without that the thinning
    /// would take whole runs out instead of every second note.
    #[test]
    fn neighbours_are_rescored_after_a_deletion() {
        // Four sixteenth-position notes, all equally crowded at first.
        let source: Vec<Event> = [1.0, 1.125, 1.25, 1.375, 1.5]
            .iter()
            .map(|t| event(*t, &[0]))
            .collect();
        let kept = thin(source, 3, &beats());
        // The two sixteenths go (1.125, 1.375) — not two neighbours.
        assert_eq!(times(&kept), vec![1.0, 1.25, 1.5]);
    }

    /// ⚠️ Taking a note away changes how crowded its neighbours are,
    /// so they are scored again. Without that the stale scores take
    /// a whole run out where one note of it was enough. No grid here,
    /// so crowding is the only thing that differs.
    #[test]
    fn a_run_loses_one_note_not_all_of_them() {
        let no_grid = Beats::constant(0.0);
        let source: Vec<Event> = [0.0, 1.0, 1.1, 2.0, 2.15, 3.5]
            .iter()
            .map(|t| event(*t, &[0]))
            .collect();
        let kept = thin(source, 4, &no_grid);
        // 1.0 and 1.1 are equally crowded; the earlier goes, and 1.1
        // then stands alone — the next to go is the other run's.
        assert_eq!(times(&kept), vec![0.0, 1.1, 2.15, 3.5]);
    }

    /// The first event is never taken, even where it is the most
    /// crowded — a song starts where it starts.
    #[test]
    fn the_first_event_stays_even_when_crowded() {
        let no_grid = Beats::constant(0.0);
        let source: Vec<Event> = [0.0, 0.05, 1.0, 2.0]
            .iter()
            .map(|t| event(*t, &[0]))
            .collect();
        let kept = thin(source, 3, &no_grid);
        assert_eq!(times(&kept), vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn hard_shapes_chords_into_pairs_without_green_and_orange() {
        let mut triple = event(1.0, &[1, 2, 3]);
        shape_chord(&mut triple, Difficulty::Hard);
        assert_eq!(triple.lanes(), vec![1, 3], "a triple keeps its outer frets");
        let mut wide = event(1.0, &[0, 2, 4]);
        shape_chord(&mut wide, Difficulty::Hard);
        assert_eq!(wide.lanes(), vec![0, 2], "green–orange is never kept");
        let mut pair = event(1.0, &[0, 4]);
        shape_chord(&mut pair, Difficulty::Hard);
        assert_eq!(pair.lanes(), vec![0]);
        let mut fine = event(1.0, &[1, 3]);
        shape_chord(&mut fine, Difficulty::Hard);
        assert_eq!(fine.lanes(), vec![1, 3]);
    }

    #[test]
    fn medium_draws_wide_chords_in() {
        let mut triple = event(1.0, &[0, 1, 2]);
        shape_chord(&mut triple, Difficulty::Medium);
        assert_eq!(triple.lanes(), vec![0, 2]);
        let mut wide = event(1.0, &[0, 3]);
        shape_chord(&mut wide, Difficulty::Medium);
        assert_eq!(wide.lanes(), vec![0, 2], "green–blue drawn in to span two");
        let mut fine = event(1.0, &[1, 2]);
        shape_chord(&mut fine, Difficulty::Medium);
        assert_eq!(fine.lanes(), vec![1, 2]);
    }

    /// Four frets, passage by passage, keeping every step where it can.
    #[test]
    fn the_fold_keeps_the_melody_where_it_can() {
        let b = beats();
        // A passage without green moves down whole: every step kept.
        let mut up = vec![event(1.0, &[1]), event(1.5, &[2]), event(2.0, &[4])];
        let moved = fold_to_four(&mut up, &b);
        assert_eq!(
            up.iter().map(|e| e.lanes()[0]).collect::<Vec<_>>(),
            vec![0, 1, 3]
        );
        assert_eq!(moved, 3);
        // A passage that never reaches orange is left alone.
        let mut low = vec![event(1.0, &[0]), event(1.5, &[3])];
        assert_eq!(fold_to_four(&mut low, &b), 0);
        // A passage spanning all five loses its top only.
        let mut full = vec![event(1.0, &[0]), event(1.5, &[4]), event(2.0, &[3])];
        fold_to_four(&mut full, &b);
        assert_eq!(
            full.iter().map(|e| e.lanes()[0]).collect::<Vec<_>>(),
            vec![0, 3, 3]
        );
        // Passages are separate: a rest of two beats starts a new one.
        let mut two = vec![
            event(1.0, &[0]),
            event(1.5, &[4]),
            event(3.0, &[1]),
            event(3.5, &[4]),
        ];
        fold_to_four(&mut two, &b);
        assert_eq!(
            two.iter().map(|e| e.lanes()[0]).collect::<Vec<_>>(),
            vec![0, 3, 0, 3],
            "the second passage was folded with the first"
        );
        // A chord whose two frets land on one becomes one note.
        let mut chord = vec![event(1.0, &[0]), event(1.5, &[3, 4])];
        fold_to_four(&mut chord, &b);
        assert_eq!(chord[1].lanes(), vec![3]);
    }

    #[test]
    fn the_medium_target_is_the_density_inside_the_band() {
        // 100 Expert events over 20 s: 2.2/s would be 44, under the
        // band's floor of 61 — the band wins.
        assert_eq!(medium_target(100, 20.0, 100), 61);
        // 100 over 40 s: 88 wanted, over the ceiling of 76.
        assert_eq!(medium_target(100, 40.0, 100), 76);
        // Inside the band the density decides.
        assert_eq!(medium_target(100, 30.0, 100), 66);
        // Never more than the Hard it is taken from.
        assert_eq!(medium_target(100, 30.0, 50), 50);
    }

    /// A hammer-on flag survives only where the note it was hammered
    /// from is still there.
    #[test]
    fn a_flag_does_not_hammer_across_a_gap_the_thinning_opened() {
        let mut source = vec![
            event(1.0, &[0]),
            event(1.125, &[1]),
            event(1.25, &[2]),
            event(1.375, &[3]),
        ];
        for e in &mut source[1..] {
            e.notes[0].hopo = true;
        }
        let kept = vec![source[0].clone(), source[1].clone(), source[3].clone()];
        let notes = notes_of(&source, &kept);
        assert_eq!(
            notes.iter().map(|n| n.hopo).collect::<Vec<_>>(),
            vec![false, true, false]
        );
    }

    /// Hard, end to end: the share, the subset, the phrases.
    #[test]
    fn a_classic_hard_is_its_expert_less_a_ninth() {
        let notes: Vec<ChartNote> = (0..100)
            .map(|i| note(1.0 + f64::from(i) * 0.125, (i % 5) as u8))
            .collect();
        let mut expert = def(Difficulty::Expert, notes);
        expert.phrases = vec![
            ChartPhrase {
                start: 1.0,
                end: 2.0,
            },
            ChartPhrase {
                start: 40.0,
                end: 41.0,
            },
        ];
        let hard = derive_hard(&expert, &beats());
        assert_eq!(hard.difficulty, Difficulty::Hard);
        assert_eq!(hard.notes.len(), 89);
        for n in &hard.notes {
            assert!(
                expert
                    .notes
                    .iter()
                    .any(|e| e.time == n.time && e.lane == n.lane),
                "{n:?} is not an Expert note"
            );
        }
        assert_eq!(
            hard.phrases,
            vec![ChartPhrase {
                start: 1.0,
                end: 2.0
            }],
            "a phrase with no note left was kept, or one with notes dropped"
        );
    }

    /// Medium, end to end: from the Hard given, inside the band, four
    /// frets, mostly on the beat.
    #[test]
    fn a_classic_medium_is_its_hard_thinned_and_folded() {
        // 60 s of eighths and sixteenths at 120 BPM.
        let notes: Vec<ChartNote> = (0..400)
            .map(|i| note(1.0 + f64::from(i) * 0.125, (i % 5) as u8))
            .collect();
        let expert = def(Difficulty::Expert, notes);
        let hard = derive_hard(&expert, &beats());
        let (medium, _) = derive_medium(&hard, &expert, &beats());
        let span = 399.0 * 0.125;
        assert_eq!(
            medium.notes.len(),
            medium_target(400, span, hard.notes.len())
        );
        assert!(medium.notes.iter().all(|n| n.lane <= 3), "orange on Medium");
        for n in &medium.notes {
            assert!(
                hard.notes.iter().any(|h| h.time == n.time),
                "not a Hard moment"
            );
        }
        // Sixteenths go first: every beat and every "and" of the
        // source is still there, and only sixteenths are missing.
        let at = |position: Position| {
            medium
                .notes
                .iter()
                .filter(|n| Position::of(beats().phase(n.time)) == position)
                .count()
        };
        assert_eq!(
            at(Position::Beat),
            100,
            "a beat was taken before a sixteenth"
        );
        assert_eq!(
            at(Position::Eighth),
            100,
            "an eighth was taken before a sixteenth"
        );
    }
}
