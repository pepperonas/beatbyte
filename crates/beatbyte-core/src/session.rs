//! The deterministic gameplay session: the judgment engine.
//!
//! A [`TrackSession`] consumes a stream of timestamped inputs plus the
//! advancing song clock and produces judgments, score updates and
//! feedback events. It is entirely pure — no engine types, no wall
//! clock, no randomness — so the exact same input sequence always
//! produces the exact same outcome. The presentation layer's only jobs
//! are to feed it real inputs with song-clock timestamps and to render
//! what it reports.
//!
//! ## Hit rules (classic five-lane model)
//!
//! - **Strum**: hits the earliest pending note event within the hit
//!   window whose frets match. Single notes use *anchoring* — only the
//!   highest held fret must match, lower frets may be held. Chords
//!   require an exact fret match.
//! - **Overstrum**: a strum matching no note breaks the streak and ends
//!   any active sustain. The unmatched note (if any) stays pending.
//! - **HOPO** (hammer-on/pull-off): while the chain is alive (previous
//!   event hit, nothing broken since), pressing a matching fret hits the
//!   event without strumming. Strumming a HOPO always works too.
//! - **Sustains**: hold the frets to earn points per musical beat.
//!   Releasing early simply ends the tail; releasing within the final
//!   grace period counts as completed.
//! - **Special phrases**: hit every event inside a phrase to earn Hype
//!   meter. Misses break the phrase; overstrums do not.

use serde::{Deserialize, Serialize};

use crate::lane::{Lane, LaneSet};
use crate::note::{NoteKind, Track};
use crate::score::{PlayerPerformance, ScoreConfig};
use crate::timing::{Judgment, TimingWindows};

/// Releasing a sustain this close to its end still counts as completed.
pub const SUSTAIN_RELEASE_GRACE_S: f64 = 0.05;

/// A player input on the song timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GameInput {
    /// Song time in seconds at which the input happened.
    pub time_s: f64,
    /// What happened.
    pub kind: InputKind,
}

/// The kinds of gameplay input a session understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    /// A fret was pressed.
    FretDown(Lane),
    /// A fret was released.
    FretUp(Lane),
    /// The strum bar was hit (direction is irrelevant to judgment).
    Strum,
    /// The player asked to activate Hype.
    ActivateHype,
}

/// Lifecycle state of a note event within a session.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NoteState {
    /// Not yet resolved.
    Pending,
    /// Hit with the given judgment.
    Hit(Judgment),
    /// The window passed without a hit.
    Missed,
}

/// Feedback emitted by the session for the presentation layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionEvent {
    /// A note event was hit.
    NoteHit {
        /// Index into [`Track::events`].
        event_index: usize,
        /// The judgment earned.
        judgment: Judgment,
        /// Signed offset `hit_time - note_time` in seconds.
        offset_s: f64,
    },
    /// A note event's window passed without a hit.
    NoteMissed {
        /// Index into [`Track::events`].
        event_index: usize,
    },
    /// A strum matched no note.
    Overstrum,
    /// A sustain tail started (its head was hit).
    SustainStarted {
        /// Index into [`Track::events`].
        event_index: usize,
    },
    /// A sustain tail ended.
    SustainEnded {
        /// Index into [`Track::events`].
        event_index: usize,
        /// Whether it was held to (or into the grace period of) its end.
        completed: bool,
    },
    /// Every event of a special phrase was hit.
    PhraseCompleted {
        /// Index into [`Track::phrases`].
        phrase_index: usize,
    },
    /// A special phrase was broken by a miss.
    PhraseBroken {
        /// Index into [`Track::phrases`].
        phrase_index: usize,
    },
    /// Hype was activated.
    HypeActivated,
    /// Hype ran out.
    HypeEnded,
}

/// Per-phrase completion tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PhraseProgress {
    /// Total events inside the phrase.
    total: u32,
    /// Events inside the phrase hit so far.
    hits: u32,
    /// Whether the phrase was broken by a miss.
    broken: bool,
}

/// A running sustain tail.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveSustain {
    event_index: usize,
    /// Song time up to which points were already awarded.
    ticked_to_s: f64,
}

/// One player's deterministic gameplay session over a [`Track`].
#[derive(Debug, Clone)]
pub struct TrackSession {
    track: Track,
    windows: TimingWindows,
    performance: PlayerPerformance,
    /// Session clock: the latest song time processed.
    clock_s: f64,
    /// Currently held frets.
    held: LaneSet,
    /// Per-event lifecycle states (parallel to `track.events()`).
    states: Vec<NoteState>,
    /// Index of the first event that might still be pending.
    scan_from: usize,
    /// Whether the HOPO chain is alive (previous event hit, no break).
    hopo_chain: bool,
    /// Tap mode: every note is hittable on fret press alone (no strum
    /// required) — keyboard-friendly play. Strums still work.
    tap_mode: bool,
    /// The active sustain, if any.
    sustain: Option<ActiveSustain>,
    /// Phrase index per event (`usize::MAX` = not in a phrase).
    event_phrase: Vec<usize>,
    /// Progress per phrase.
    phrases: Vec<PhraseProgress>,
}

impl TrackSession {
    /// Start a session at song time `0.0` (times before the first note
    /// are fine — the clock may even start negative for count-ins).
    #[must_use]
    pub fn new(track: Track, windows: TimingWindows, score: ScoreConfig) -> TrackSession {
        let states = vec![NoteState::Pending; track.events().len()];
        let mut phrases = vec![
            PhraseProgress {
                total: 0,
                hits: 0,
                broken: false
            };
            track.phrases().len()
        ];
        let event_phrase: Vec<usize> = track
            .events()
            .iter()
            .map(|event| {
                track
                    .phrases()
                    .iter()
                    .position(|p| p.contains(event.time_s))
                    .inspect(|&i| phrases[i].total += 1)
                    .unwrap_or(usize::MAX)
            })
            .collect();

        TrackSession {
            track,
            windows,
            performance: PlayerPerformance::new(score),
            clock_s: f64::NEG_INFINITY,
            held: LaneSet::EMPTY,
            states,
            scan_from: 0,
            hopo_chain: false,
            tap_mode: false,
            sustain: None,
            event_phrase,
            phrases,
        }
    }

    /// Enable/disable tap mode (hit on fret press, no strum needed).
    /// Meant to be set before play starts; flipping it mid-song is
    /// harmless but confusing.
    pub fn set_tap_mode(&mut self, on: bool) {
        self.tap_mode = on;
    }

    /// The timing windows this session judges with.
    #[must_use]
    pub fn windows(&self) -> TimingWindows {
        self.windows
    }

    /// Whether tap mode is active.
    #[must_use]
    pub fn tap_mode(&self) -> bool {
        self.tap_mode
    }

    /// The track being played.
    #[must_use]
    pub fn track(&self) -> &Track {
        &self.track
    }

    /// The player's performance so far.
    #[must_use]
    pub fn performance(&self) -> &PlayerPerformance {
        &self.performance
    }

    /// The state of a note event.
    #[must_use]
    pub fn note_state(&self, event_index: usize) -> Option<NoteState> {
        self.states.get(event_index).copied()
    }

    /// The currently held frets.
    #[must_use]
    pub fn held(&self) -> LaneSet {
        self.held
    }

    /// The latest processed song time.
    #[must_use]
    pub fn clock_s(&self) -> f64 {
        self.clock_s
    }

    /// The event index of the currently running sustain, if any
    /// (presentation uses this for hold feedback).
    #[must_use]
    pub fn active_sustain(&self) -> Option<usize> {
        self.sustain.map(|s| s.event_index)
    }

    /// Whether all events are resolved (hit or missed) and no sustain is
    /// running — the track is finished.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.sustain.is_none()
            && self.states[self.scan_from.min(self.states.len())..]
                .iter()
                .all(|s| !matches!(s, NoteState::Pending))
    }

    /// Advance the song clock, resolving misses, sustain points and Hype
    /// drain. Call once per frame with the current song time, and before
    /// processing each input via [`TrackSession::handle`].
    pub fn advance(&mut self, to_s: f64, events: &mut Vec<SessionEvent>) {
        if !to_s.is_finite() {
            return;
        }
        let from_s = self.clock_s;
        if to_s <= from_s {
            return;
        }
        self.clock_s = to_s;

        // 1. Sustain ticking (before misses: independent concerns).
        self.tick_sustain(to_s, events);

        // 2. Hype drain over elapsed musical time.
        if self.performance.hype_active() && from_s.is_finite() {
            let beats = self.track.tempo.beats_at(to_s) - self.track.tempo.beats_at(from_s);
            self.performance.drain_hype_beats(beats);
            if !self.performance.hype_active() {
                events.push(SessionEvent::HypeEnded);
            }
        }

        // 3. Miss detection: pending events whose window has passed.
        let deadline = to_s - self.windows.good_s;
        for index in self.scan_from..self.states.len() {
            let event = self.track.events()[index];
            if event.time_s >= deadline {
                break;
            }
            if matches!(self.states[index], NoteState::Pending) {
                self.states[index] = NoteState::Missed;
                self.performance.register_judgment(Judgment::Miss, 1);
                self.hopo_chain = false;
                events.push(SessionEvent::NoteMissed { event_index: index });
                self.break_phrase_of(index, events);
            }
        }
        self.advance_scan_pointer();
    }

    /// Process one input. Inputs must arrive in chronological order;
    /// timestamps earlier than the session clock are clamped to it.
    pub fn handle(&mut self, input: GameInput, events: &mut Vec<SessionEvent>) {
        self.advance(input.time_s, events);
        let time_s = input.time_s.max(self.clock_s);

        match input.kind {
            InputKind::FretDown(lane) => {
                self.held.insert(lane);
                self.try_hopo_hit(lane, time_s, events);
            }
            InputKind::FretUp(lane) => {
                self.held.remove(lane);
                self.check_sustain_release(time_s, events);
                // Pull-off: releasing a fret can expose a lower held
                // fret as the new highest — that's a HOPO hit chance.
                if let Some(exposed) = self.held.highest() {
                    self.try_hopo_hit(exposed, time_s, events);
                }
            }
            InputKind::Strum => self.strum(time_s, events),
            InputKind::ActivateHype => {
                if self.performance.try_activate_hype() {
                    events.push(SessionEvent::HypeActivated);
                }
            }
        }
    }

    // ---- internals -----------------------------------------------------

    fn strum(&mut self, time_s: f64, events: &mut Vec<SessionEvent>) {
        // Earliest pending event in the window whose frets match.
        let candidate = self
            .pending_in_window(time_s)
            .find(|&index| self.frets_match(self.track.events()[index].lanes));

        match candidate {
            Some(index) => self.hit(index, time_s, events),
            None => {
                self.performance.register_overstrum();
                self.hopo_chain = false;
                self.end_sustain(time_s, events);
                events.push(SessionEvent::Overstrum);
            }
        }
    }

    fn try_hopo_hit(&mut self, pressed: Lane, time_s: f64, events: &mut Vec<SessionEvent>) {
        // Tap mode generalizes the HOPO rule to every note: a fret
        // press may hit regardless of chain state or note kind.
        if !self.tap_mode && !self.hopo_chain {
            return;
        }
        let candidate = self.pending_in_window(time_s).find(|&index| {
            let event = self.track.events()[index];
            (self.tap_mode || event.kind == NoteKind::Hopo)
                && event.lanes.contains(pressed)
                && self.frets_match(event.lanes)
        });
        if let Some(index) = candidate {
            self.hit(index, time_s, events);
        }
    }

    /// Pending events whose window contains `time_s`, earliest first.
    fn pending_in_window(&self, time_s: f64) -> impl Iterator<Item = usize> + '_ {
        let good = self.windows.good_s;
        self.states
            .iter()
            .enumerate()
            .skip(self.scan_from)
            .take_while(move |(i, _)| self.track.events()[*i].time_s <= time_s + good)
            .filter(move |(i, state)| {
                matches!(state, NoteState::Pending)
                    && (self.track.events()[*i].time_s - time_s).abs() <= good
            })
            .map(|(i, _)| i)
    }

    /// Fret matching: exact for chords, anchored (highest held fret must
    /// match) for single notes.
    fn frets_match(&self, lanes: LaneSet) -> bool {
        if lanes.len() > 1 {
            self.held == lanes
        } else {
            self.held.highest() == lanes.highest()
        }
    }

    fn hit(&mut self, index: usize, time_s: f64, events: &mut Vec<SessionEvent>) {
        let event = self.track.events()[index];
        let offset_s = time_s - event.time_s;
        // Candidates are always in-window, so `judge` cannot fail; the
        // fallback is defensive only.
        let judgment = self.windows.judge(offset_s).unwrap_or(Judgment::Good);

        self.states[index] = NoteState::Hit(judgment);
        self.performance
            .register_judgment(judgment, event.lanes.len());
        self.hopo_chain = true;
        events.push(SessionEvent::NoteHit {
            event_index: index,
            judgment,
            offset_s,
        });

        // Phrase progress.
        let phrase = self.event_phrase[index];
        if phrase != usize::MAX {
            let progress = &mut self.phrases[phrase];
            if !progress.broken {
                progress.hits += 1;
                if progress.hits == progress.total {
                    self.performance.complete_phrase();
                    events.push(SessionEvent::PhraseCompleted {
                        phrase_index: phrase,
                    });
                }
            }
        }

        // A new hit replaces any running sustain.
        self.end_sustain(time_s, events);
        if event.is_sustain() {
            self.sustain = Some(ActiveSustain {
                event_index: index,
                ticked_to_s: time_s.max(event.time_s),
            });
            events.push(SessionEvent::SustainStarted { event_index: index });
        }

        self.advance_scan_pointer();
    }

    fn tick_sustain(&mut self, to_s: f64, events: &mut Vec<SessionEvent>) {
        let Some(active) = self.sustain else {
            return;
        };
        let event = self.track.events()[active.event_index];
        let tick_end = to_s.min(event.end_time_s());
        if tick_end > active.ticked_to_s {
            let beats =
                self.track.tempo.beats_at(tick_end) - self.track.tempo.beats_at(active.ticked_to_s);
            self.performance.add_sustain_beats(beats);
            self.sustain = Some(ActiveSustain {
                ticked_to_s: tick_end,
                ..active
            });
        }
        if to_s >= event.end_time_s() {
            self.sustain = None;
            events.push(SessionEvent::SustainEnded {
                event_index: active.event_index,
                completed: true,
            });
        }
    }

    /// End any active sustain immediately (new hit, overstrum).
    fn end_sustain(&mut self, time_s: f64, events: &mut Vec<SessionEvent>) {
        if let Some(active) = self.sustain.take() {
            let event = self.track.events()[active.event_index];
            events.push(SessionEvent::SustainEnded {
                event_index: active.event_index,
                completed: time_s >= event.end_time_s() - SUSTAIN_RELEASE_GRACE_S,
            });
        }
    }

    /// After a fret release, the sustain survives only while all its
    /// lanes stay held.
    fn check_sustain_release(&mut self, time_s: f64, events: &mut Vec<SessionEvent>) {
        if let Some(active) = self.sustain {
            let event = self.track.events()[active.event_index];
            let still_held = event.lanes.iter().all(|lane| self.held.contains(lane));
            if !still_held {
                self.end_sustain(time_s, events);
            }
        }
    }

    fn break_phrase_of(&mut self, event_index: usize, events: &mut Vec<SessionEvent>) {
        let phrase = self.event_phrase[event_index];
        if phrase != usize::MAX {
            let progress = &mut self.phrases[phrase];
            if !progress.broken {
                progress.broken = true;
                events.push(SessionEvent::PhraseBroken {
                    phrase_index: phrase,
                });
            }
        }
    }

    fn advance_scan_pointer(&mut self) {
        while self.scan_from < self.states.len()
            && !matches!(self.states[self.scan_from], NoteState::Pending)
        {
            self.scan_from += 1;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::difficulty::Difficulty;
    use crate::note::NoteEvent;
    use crate::note::Phrase;
    use crate::timing::TempoMap;

    /// 120 BPM, no offset: 1 beat = 0.5 s.
    fn tempo() -> TempoMap {
        TempoMap::constant(120.0, 0.0)
    }

    fn track(events: Vec<NoteEvent>) -> Track {
        Track::new(Difficulty::Expert, tempo(), events, vec![]).unwrap()
    }

    fn track_with_phrases(events: Vec<NoteEvent>, phrases: Vec<Phrase>) -> Track {
        Track::new(Difficulty::Expert, tempo(), events, phrases).unwrap()
    }

    fn session(track: Track) -> TrackSession {
        TrackSession::new(track, TimingWindows::default(), ScoreConfig::default())
    }

    fn tap(time_s: f64, lane: Lane) -> NoteEvent {
        NoteEvent::tap(time_s, LaneSet::single(lane))
    }

    fn hopo(time_s: f64, lane: Lane) -> NoteEvent {
        NoteEvent {
            time_s,
            lanes: LaneSet::single(lane),
            sustain_s: 0.0,
            kind: NoteKind::Hopo,
        }
    }

    /// Press a fret and strum at the given time.
    fn play(session: &mut TrackSession, time_s: f64, lane: Lane) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        session.handle(
            GameInput {
                time_s,
                kind: InputKind::FretDown(lane),
            },
            &mut events,
        );
        session.handle(
            GameInput {
                time_s,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        events
    }

    fn release(session: &mut TrackSession, time_s: f64, lane: Lane) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        session.handle(
            GameInput {
                time_s,
                kind: InputKind::FretUp(lane),
            },
            &mut events,
        );
        events
    }

    #[test]
    fn perfect_hit_on_time() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let events = play(&mut s, 1.0, Lane::One);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::NoteHit {
                event_index: 0,
                judgment: Judgment::Perfect,
                ..
            }]
        ));
        assert_eq!(s.performance().score(), 50);
        assert_eq!(s.performance().streak(), 1);
    }

    #[test]
    fn late_hit_judges_by_offset() {
        // +80 ms → Good with default windows.
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let events = play(&mut s, 1.08, Lane::One);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::NoteHit {
                judgment: Judgment::Good,
                ..
            }]
        ));
    }

    #[test]
    fn early_hit_works_symmetrically() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let events = play(&mut s, 0.95, Lane::One);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::NoteHit {
                judgment: Judgment::Great,
                ..
            }]
        ));
    }

    #[test]
    fn note_expires_into_a_miss() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let mut events = Vec::new();
        s.advance(2.0, &mut events);
        assert!(matches!(
            events.as_slice(),
            [SessionEvent::NoteMissed { event_index: 0 }]
        ));
        assert_eq!(s.performance().counts().miss, 1);
        assert!(s.finished());
    }

    #[test]
    fn wrong_fret_is_an_overstrum_and_note_stays_pending() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let events = play(&mut s, 1.0, Lane::Two);
        assert!(events.contains(&SessionEvent::Overstrum));
        assert!(matches!(s.note_state(0), Some(NoteState::Pending)));

        // The note can still be rescued within its window.
        let mut events = release(&mut s, 1.01, Lane::Two);
        events.extend(play(&mut s, 1.05, Lane::One));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. }))
        );
    }

    #[test]
    fn strum_with_nothing_in_window_is_an_overstrum() {
        let mut s = session(track(vec![tap(5.0, Lane::One)]));
        let events = play(&mut s, 1.0, Lane::One);
        assert_eq!(events, vec![SessionEvent::Overstrum]);
        assert_eq!(s.performance().overstrums(), 1);
    }

    #[test]
    fn anchoring_allows_lower_frets_for_single_notes() {
        let mut s = session(track(vec![tap(1.0, Lane::Three)]));
        let mut events = Vec::new();
        // Hold lanes 1 and 2 below the target lane 3.
        for lane in [Lane::One, Lane::Two, Lane::Three] {
            s.handle(
                GameInput {
                    time_s: 0.9,
                    kind: InputKind::FretDown(lane),
                },
                &mut events,
            );
        }
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. })),
            "anchored single note should hit; got {events:?}"
        );
    }

    #[test]
    fn higher_extra_fret_blocks_single_notes() {
        let mut s = session(track(vec![tap(1.0, Lane::Two)]));
        let mut events = Vec::new();
        for lane in [Lane::Two, Lane::Four] {
            s.handle(
                GameInput {
                    time_s: 0.9,
                    kind: InputKind::FretDown(lane),
                },
                &mut events,
            );
        }
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(events.contains(&SessionEvent::Overstrum));
    }

    #[test]
    fn chords_require_exact_frets() {
        let chord = NoteEvent::tap(1.0, LaneSet::from_lanes([Lane::One, Lane::Two]));
        let mut s = session(track(vec![chord]));
        let mut events = Vec::new();
        for lane in [Lane::One, Lane::Two] {
            s.handle(
                GameInput {
                    time_s: 0.95,
                    kind: InputKind::FretDown(lane),
                },
                &mut events,
            );
        }
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. }))
        );
        // Chord scores per lane: 2 × 50.
        assert_eq!(s.performance().score(), 100);
    }

    #[test]
    fn chord_with_extra_fret_fails() {
        let chord = NoteEvent::tap(1.0, LaneSet::from_lanes([Lane::One, Lane::Two]));
        let mut s = session(track(vec![chord]));
        let mut events = Vec::new();
        for lane in [Lane::One, Lane::Two, Lane::Three] {
            s.handle(
                GameInput {
                    time_s: 0.95,
                    kind: InputKind::FretDown(lane),
                },
                &mut events,
            );
        }
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(events.contains(&SessionEvent::Overstrum));
    }

    #[test]
    fn hopo_hits_without_strum_while_chain_alive() {
        let mut s = session(track(vec![tap(1.0, Lane::One), hopo(1.25, Lane::Two)]));
        play(&mut s, 1.0, Lane::One);

        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 1.25,
                kind: InputKind::FretDown(Lane::Two),
            },
            &mut events,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. })),
            "HOPO should hit on fret press; got {events:?}"
        );
        assert_eq!(s.performance().streak(), 2);
    }

    #[test]
    fn hopo_needs_a_live_chain() {
        // First note is a HOPO: chain starts dead, fret press must not hit.
        let mut s = session(track(vec![hopo(1.0, Lane::Two)]));
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::FretDown(Lane::Two),
            },
            &mut events,
        );
        assert!(events.is_empty());
        // But strumming it works.
        s.handle(
            GameInput {
                time_s: 1.02,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. }))
        );
    }

    #[test]
    fn pull_off_hits_on_fret_release() {
        // Hold lanes 1+3, strum the note on 3, then release 3 to
        // pull off onto the HOPO on lane 1.
        let mut s = session(track(vec![tap(1.0, Lane::Three), hopo(1.25, Lane::One)]));
        let mut events = Vec::new();
        for lane in [Lane::One, Lane::Three] {
            s.handle(
                GameInput {
                    time_s: 0.9,
                    kind: InputKind::FretDown(lane),
                },
                &mut events,
            );
        }
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 0, .. }))
        );

        let events = release(&mut s, 1.25, Lane::Three);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. })),
            "pull-off should hit the HOPO; got {events:?}"
        );
    }

    #[test]
    fn miss_kills_the_hopo_chain() {
        let mut s = session(track(vec![
            tap(1.0, Lane::One),
            tap(2.0, Lane::One),
            hopo(2.25, Lane::Two),
        ]));
        play(&mut s, 1.0, Lane::One);
        // Let the second note expire.
        let mut events = Vec::new();
        s.advance(2.2, &mut events);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteMissed { event_index: 1 }))
        );
        // HOPO fret press must now be ignored.
        s.handle(
            GameInput {
                time_s: 2.25,
                kind: InputKind::FretDown(Lane::Two),
            },
            &mut events,
        );
        assert!(matches!(s.note_state(2), Some(NoteState::Pending)));
    }

    #[test]
    fn sustain_awards_points_over_time_and_completes() {
        // 2-beat sustain at 120 BPM = 1.0 s long, 25 points/beat.
        let note = NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::single(Lane::One),
            sustain_s: 1.0,
            kind: NoteKind::Strum,
        };
        let mut s = session(track(vec![note]));
        let events = play(&mut s, 1.0, Lane::One);
        assert!(events.contains(&SessionEvent::SustainStarted { event_index: 0 }));
        assert_eq!(s.performance().score(), 50);

        let mut events = Vec::new();
        s.advance(1.5, &mut events); // 1 beat held
        assert_eq!(s.performance().score(), 75);

        s.advance(2.5, &mut events); // past the end
        assert_eq!(s.performance().score(), 100);
        assert!(events.contains(&SessionEvent::SustainEnded {
            event_index: 0,
            completed: true
        }));
        assert!(s.finished());
    }

    #[test]
    fn releasing_a_sustain_early_stops_the_points() {
        let note = NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::single(Lane::One),
            sustain_s: 1.0,
            kind: NoteKind::Strum,
        };
        let mut s = session(track(vec![note]));
        play(&mut s, 1.0, Lane::One);

        let mut events = Vec::new();
        s.advance(1.5, &mut events);
        let events = release(&mut s, 1.5, Lane::One);
        assert!(events.contains(&SessionEvent::SustainEnded {
            event_index: 0,
            completed: false
        }));
        let frozen = s.performance().score();
        let mut events = Vec::new();
        s.advance(2.0, &mut events);
        assert_eq!(s.performance().score(), frozen, "no points after release");
    }

    #[test]
    fn overstrum_ends_the_sustain() {
        let note = NoteEvent {
            time_s: 1.0,
            lanes: LaneSet::single(Lane::One),
            sustain_s: 2.0,
            kind: NoteKind::Strum,
        };
        let mut s = session(track(vec![note]));
        play(&mut s, 1.0, Lane::One);
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 1.5,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert!(events.contains(&SessionEvent::Overstrum));
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::SustainEnded {
                completed: false,
                ..
            }
        )));
    }

    #[test]
    fn phrase_completion_grants_hype() {
        let mut s = session(track_with_phrases(
            vec![
                tap(1.0, Lane::One),
                tap(1.5, Lane::Two),
                tap(3.0, Lane::One),
            ],
            vec![Phrase {
                start_s: 0.5,
                end_s: 2.0,
            }],
        ));
        play(&mut s, 1.0, Lane::One);
        assert!((s.performance().hype_meter() - 0.0).abs() < 1e-9);
        let events = play(&mut s, 1.5, Lane::Two);
        assert!(events.contains(&SessionEvent::PhraseCompleted { phrase_index: 0 }));
        assert!((s.performance().hype_meter() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn missing_a_phrase_note_breaks_the_phrase() {
        let mut s = session(track_with_phrases(
            vec![tap(1.0, Lane::One), tap(1.5, Lane::Two)],
            vec![Phrase {
                start_s: 0.5,
                end_s: 2.0,
            }],
        ));
        play(&mut s, 1.0, Lane::One);
        let mut events = Vec::new();
        s.advance(3.0, &mut events);
        assert!(events.contains(&SessionEvent::PhraseBroken { phrase_index: 0 }));
        assert!((s.performance().hype_meter() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hype_activation_and_drain() {
        let mut s = session(track_with_phrases(
            vec![
                tap(1.0, Lane::One),
                tap(2.0, Lane::One),
                tap(20.0, Lane::One),
            ],
            vec![
                Phrase {
                    start_s: 0.9,
                    end_s: 1.1,
                },
                Phrase {
                    start_s: 1.9,
                    end_s: 2.1,
                },
            ],
        ));
        play(&mut s, 1.0, Lane::One);
        play(&mut s, 2.0, Lane::One);
        assert!((s.performance().hype_meter() - 0.5).abs() < 1e-9);

        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 2.5,
                kind: InputKind::ActivateHype,
            },
            &mut events,
        );
        assert!(events.contains(&SessionEvent::HypeActivated));
        assert_eq!(s.performance().multiplier(), 2);

        // Half a meter = 16 beats = 8 s at 120 BPM.
        s.advance(10.6, &mut events);
        assert!(events.contains(&SessionEvent::HypeEnded));
        assert_eq!(s.performance().multiplier(), 1);
    }

    #[test]
    fn note_skipping_hits_the_matching_later_note() {
        // Two nearby notes; the player aims at the second one.
        let mut s = session(track(vec![tap(1.0, Lane::One), tap(1.05, Lane::Two)]));
        let events = play(&mut s, 1.04, Lane::Two);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. })),
            "should hit the matching later note; got {events:?}"
        );
        // The skipped note misses once its window passes.
        let mut events = Vec::new();
        s.advance(2.0, &mut events);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteMissed { event_index: 0 }))
        );
    }

    #[test]
    fn determinism_same_inputs_same_outcome() {
        let build = || {
            session(track(vec![
                tap(1.0, Lane::One),
                hopo(1.25, Lane::Two),
                tap(2.0, Lane::Three),
            ]))
        };
        let run = |mut s: TrackSession| -> (u64, u32, Vec<SessionEvent>) {
            let mut all = Vec::new();
            let inputs = [
                GameInput {
                    time_s: 0.99,
                    kind: InputKind::FretDown(Lane::One),
                },
                GameInput {
                    time_s: 1.0,
                    kind: InputKind::Strum,
                },
                GameInput {
                    time_s: 1.24,
                    kind: InputKind::FretDown(Lane::Two),
                },
            ];
            for input in inputs {
                s.handle(input, &mut all);
            }
            s.advance(5.0, &mut all);
            (s.performance().score(), s.performance().streak(), all)
        };
        let a = run(build());
        let b = run(build());
        assert_eq!(a, b);
    }

    #[test]
    fn out_of_order_input_is_clamped_not_time_traveling() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let mut events = Vec::new();
        s.advance(1.5, &mut events);
        // An input stamped in the past must not resurrect anything.
        s.handle(
            GameInput {
                time_s: 0.2,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        // Note at 1.0 is still within the window at clock 1.5? No —
        // 1.5 - 1.0 = 0.5 > good window, so it was missed during advance
        // and the strum is an overstrum.
        assert!(events.contains(&SessionEvent::NoteMissed { event_index: 0 }));
        assert!(events.contains(&SessionEvent::Overstrum));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tap_mode_tests {
    use super::*;
    use crate::timing::TempoMap;
    use crate::{
        Difficulty, GameInput, InputKind, Lane, LaneSet, NoteEvent, ScoreConfig, TimingWindows,
        Track,
    };

    fn session(events: Vec<NoteEvent>) -> TrackSession {
        let track = Track::new(
            Difficulty::Medium,
            TempoMap::constant(120.0, 0.0),
            events,
            vec![],
        )
        .unwrap();
        TrackSession::new(track, TimingWindows::default(), ScoreConfig::default())
    }

    fn press(session: &mut TrackSession, lane: Lane, t: f64) -> Vec<SessionEvent> {
        let mut out = Vec::new();
        session.handle(
            GameInput {
                time_s: t,
                kind: InputKind::FretDown(lane),
            },
            &mut out,
        );
        out
    }

    #[test]
    fn tap_mode_hits_a_strum_note_on_fret_press_alone() {
        let mut s = session(vec![NoteEvent::tap(1.0, LaneSet::single(Lane::Two))]);
        s.set_tap_mode(true);
        press(&mut s, Lane::Two, 1.0);
        assert_eq!(s.performance().counts().perfect, 1);
        assert_eq!(s.performance().counts().miss, 0);
    }

    #[test]
    fn without_tap_mode_the_same_press_hits_nothing() {
        let mut s = session(vec![NoteEvent::tap(1.0, LaneSet::single(Lane::Two))]);
        press(&mut s, Lane::Two, 1.0);
        assert_eq!(s.performance().counts().total(), 0, "no strum, no hit");
    }

    #[test]
    fn tap_mode_hits_a_chord_when_the_last_fret_arrives() {
        let mut lanes = LaneSet::EMPTY;
        lanes.insert(Lane::One);
        lanes.insert(Lane::Three);
        let mut s = session(vec![NoteEvent::tap(1.0, lanes)]);
        s.set_tap_mode(true);
        press(&mut s, Lane::One, 0.99);
        assert_eq!(s.performance().counts().total(), 0, "chord incomplete");
        press(&mut s, Lane::Three, 1.0);
        assert_eq!(s.performance().counts().perfect, 1);
    }

    #[test]
    fn tap_mode_still_counts_overstrums() {
        let mut s = session(vec![NoteEvent::tap(5.0, LaneSet::single(Lane::One))]);
        s.set_tap_mode(true);
        let mut out = Vec::new();
        s.handle(
            GameInput {
                time_s: 1.0,
                kind: InputKind::Strum,
            },
            &mut out,
        );
        assert_eq!(s.performance().overstrums(), 1);
    }
}
