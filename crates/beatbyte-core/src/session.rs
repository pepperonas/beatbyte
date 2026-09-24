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
//!   event without strumming. Strumming a HOPO always works too — and
//!   because the natural motion changes the fret first and lands the
//!   pick after, a strum inside the window of a note that was just
//!   hit by fretting is that note's strum: absorbed once, never an
//!   overstrum.
//! - **Strum grace** (only on a track that carries it,
//!   [`Track::with_strum_grace`]): a strum that lands while a note is
//!   in its window but under the wrong frets is **held** instead of
//!   punished. If the frets come right within the grace, it is that
//!   note's strum, judged at the moment the pick landed; if they do
//!   not, it is the overstrum it always was, charged when the grace
//!   runs out. A strum with no note in its window is never held.
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

/// Notes in this window after a successful Hype activation are hit
/// automatically, so reaching for an activation gesture cannot break the run.
pub const HYPE_ACTIVATION_GRACE_S: f64 = 0.5;

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
    /// The rock meter emptied with failing armed. Emitted once per
    /// session; what happens next (ending the song, or nothing in a
    /// mode that does not fail) is the caller's decision.
    Failed,
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
    /// The event most recently hit by fretting alone (a hammer-on, a
    /// pull-off, a tap-mode press) whose strum may still land: one
    /// strum inside that note's window is absorbed instead of counted
    /// as an overstrum. Cleared by the next strum, hit or rewind.
    fret_hit: Option<usize>,
    /// A strum held by the strum-grace rule: the song time it landed
    /// at. It becomes a hit if the frets come right within the
    /// track's grace, and an overstrum when the grace runs out.
    held_strum: Option<f64>,
    /// Tap mode: every note is hittable on fret press alone (no strum
    /// required) — keyboard-friendly play. Strums still work.
    tap_mode: bool,
    /// The active sustain, if any.
    sustain: Option<ActiveSustain>,
    /// Phrase index per event (`usize::MAX` = not in a phrase).
    event_phrase: Vec<usize>,
    /// Progress per phrase.
    phrases: Vec<PhraseProgress>,
    /// End of the short automatic-hit window opened by Hype activation.
    hype_grace_until_s: f64,
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
            fret_hit: None,
            held_strum: None,
            tap_mode: false,
            sustain: None,
            event_phrase,
            phrases,
            hype_grace_until_s: f64::NEG_INFINITY,
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

    /// The special phrase an event belongs to, if any.
    ///
    /// Fixed when the session is built — phrase bounds are inclusive
    /// and phrases never overlap, so an event belongs to at most one.
    #[must_use]
    pub fn phrase_of(&self, event_index: usize) -> Option<usize> {
        self.event_phrase
            .get(event_index)
            .copied()
            .filter(|&phrase| phrase != usize::MAX)
    }

    /// Whether a phrase has been broken by a miss: its Hype can no
    /// longer be earned, however the rest of it is played. `false` for
    /// an index no phrase has.
    #[must_use]
    pub fn phrase_broken(&self, phrase_index: usize) -> bool {
        self.phrases
            .get(phrase_index)
            .is_some_and(|progress| progress.broken)
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

    /// Rewind the session to `time_s` (the practice section loop):
    /// every event at or after the point becomes judgeable again,
    /// the session clock moves back so nothing is phantom-missed,
    /// and the transients (active sustain, HOPO chain) reset —
    /// half-finished cross-boundary state cannot be honestly
    /// resumed. Phrase progress is recomputed from the surviving
    /// states. The performance totals deliberately stay: rewinding
    /// exists for practice, which records nothing anyway, and a
    /// score that ran backwards would look like a display bug.
    pub fn rewind_to(&mut self, time_s: f64) {
        if !time_s.is_finite() {
            return;
        }
        for (index, event) in self.track.events().iter().enumerate() {
            if event.time_s >= time_s {
                self.states[index] = NoteState::Pending;
            }
        }
        self.scan_from = self
            .states
            .iter()
            .position(|state| matches!(state, NoteState::Pending))
            .unwrap_or(self.states.len());
        self.clock_s = self.clock_s.min(time_s);
        self.sustain = None;
        self.hopo_chain = false;
        self.fret_hit = None;
        self.held_strum = None;
        self.hype_grace_until_s = f64::NEG_INFINITY;
        for progress in &mut self.phrases {
            progress.hits = 0;
            progress.broken = false;
        }
        for (index, &phrase) in self.event_phrase.iter().enumerate() {
            if phrase == usize::MAX {
                continue;
            }
            match self.states[index] {
                NoteState::Hit(_) => self.phrases[phrase].hits += 1,
                NoteState::Missed => self.phrases[phrase].broken = true,
                NoteState::Pending => {}
            }
        }
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

        // 0. A held strum whose grace ran out is the overstrum it
        // would have been, charged at the moment the grace ended.
        self.expire_held_strum(to_s, events);

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

        // 3. A successful Hype activation gives the player's activating hand
        // half a second to return. Hit notes on their own timestamps so this
        // changes neither timing statistics nor the existing Hype path.
        self.auto_hit_hype_notes(to_s, events);

        // 4. Miss detection: pending events whose window has passed.
        // ⚠️ Not while a strum is held for them: a late strum under
        // the wrong fret is waiting for exactly the note whose window
        // is closing, and missing it first would make the grace a
        // promise the late half of every window could never keep.
        let judged_to = self.held_strum.map_or(to_s, |held| held.min(to_s));
        let deadline = judged_to - self.windows.good_s;
        for index in self.scan_from..self.states.len() {
            let event = self.track.events()[index];
            if event.time_s >= deadline {
                break;
            }
            if matches!(self.states[index], NoteState::Pending) {
                self.states[index] = NoteState::Missed;
                let failed = self.performance.register_judgment(Judgment::Miss, 1);
                self.hopo_chain = false;
                events.push(SessionEvent::NoteMissed { event_index: index });
                if failed {
                    events.push(SessionEvent::Failed);
                }
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
                if !self.resolve_held_strum(events) {
                    self.try_hopo_hit(lane, time_s, events);
                }
            }
            InputKind::FretUp(lane) => {
                self.held.remove(lane);
                self.check_sustain_release(time_s, events);
                if !self.resolve_held_strum(events) {
                    // Pull-off: releasing a fret can expose a lower
                    // held fret as the new highest — that's a HOPO
                    // hit chance.
                    if let Some(exposed) = self.held.highest() {
                        self.try_hopo_hit(exposed, time_s, events);
                    }
                }
            }
            InputKind::Strum => self.strum(time_s, events),
            InputKind::ActivateHype => {
                if self.performance.try_activate_hype() {
                    self.hype_grace_until_s = time_s + HYPE_ACTIVATION_GRACE_S;
                    events.push(SessionEvent::HypeActivated);
                    self.auto_hit_hype_notes(time_s, events);
                }
            }
        }
    }

    // ---- internals -----------------------------------------------------

    /// Take every pending note the Hype activation window reaches.
    ///
    /// A note taken here is hit **without a strum**, exactly like a
    /// hammer-on taken by fretting — so it arms the same absorb
    /// marker. Without that, the strum the player was already going
    /// to make lands on a note the window has just taken, matches
    /// nothing, and is punished as an overstrum: activate Hype, play
    /// the next note normally, lose the streak. Found by the
    /// autopilot on a chart that had been blamed for it for weeks
    /// (roadmap C7) — and it only showed where the note needed no
    /// fret change, because a fret change hits the note itself and
    /// the strum is then never sent at all.
    fn auto_hit_hype_notes(&mut self, through_s: f64, events: &mut Vec<SessionEvent>) {
        let through_s = through_s.min(self.hype_grace_until_s);
        if !through_s.is_finite() {
            return;
        }
        while let Some(index) = self
            .states
            .iter()
            .enumerate()
            .skip(self.scan_from)
            .find(|(index, state)| {
                matches!(state, NoteState::Pending)
                    && self.track.events()[*index].time_s <= through_s
            })
            .map(|(index, _)| index)
        {
            let note_time = self.track.events()[index].time_s;
            self.hit(index, note_time, events);
            // A strum already held for this note is its strum — the
            // same rule as a hammer-on taken under a held strum.
            match self.held_strum {
                Some(held) if (held - note_time).abs() <= self.windows.good_s => {
                    self.held_strum = None;
                }
                // `hit` voids the marker; re-arm it for THIS note, the
                // way a fretted HOPO does. The last one taken is the
                // one whose strum is still to come.
                _ => self.fret_hit = Some(index),
            }
        }
    }

    fn strum(&mut self, time_s: f64, events: &mut Vec<SessionEvent>) {
        // A strum still held when the next one lands was never
        // answered by a fret: two strums, one fretting — the first is
        // the extra one.
        if self.held_strum.take().is_some() {
            self.overstrum(time_s, events);
        }
        // Earliest pending event in the window whose frets match.
        let candidate = self
            .pending_in_window(time_s)
            .find(|&index| self.frets_match(self.track.events()[index].lanes));

        match candidate {
            Some(index) => self.hit(index, time_s, events),
            None => {
                // The strum of a note the player just fretted: the
                // pick landing after the fret change, not an extra
                // strum. One per note, inside that note's window.
                if let Some(index) = self.fret_hit.take() {
                    let event = self.track.events()[index];
                    if (time_s - event.time_s).abs() <= self.windows.good_s {
                        return;
                    }
                }
                // The strum-grace rule: a note IS here, only the fret
                // is not yet. Wait for it rather than punish the
                // order the hand happened to move in.
                if self.track.strum_grace_s() > 0.0
                    && self.pending_in_window(time_s).next().is_some()
                {
                    self.held_strum = Some(time_s);
                    return;
                }
                self.overstrum(time_s, events);
            }
        }
    }

    /// Charge an overstrum at `time_s`.
    fn overstrum(&mut self, time_s: f64, events: &mut Vec<SessionEvent>) {
        let failed = self.performance.register_overstrum();
        self.hopo_chain = false;
        if failed {
            events.push(SessionEvent::Failed);
        }
        self.end_sustain(time_s, events);
        events.push(SessionEvent::Overstrum);
    }

    /// The frets just changed: if a strum is held and a note in ITS
    /// window now matches, that strum hits it — judged at the moment
    /// the pick landed, because the strum is the act the timing is
    /// about. Returns whether it did.
    fn resolve_held_strum(&mut self, events: &mut Vec<SessionEvent>) -> bool {
        let Some(strummed_at) = self.held_strum else {
            return false;
        };
        let candidate = self
            .pending_in_window(strummed_at)
            .find(|&index| self.frets_match(self.track.events()[index].lanes));
        let Some(index) = candidate else {
            return false;
        };
        self.held_strum = None;
        self.hit(index, strummed_at, events);
        true
    }

    /// A held strum whose grace has run out by `now` becomes the
    /// overstrum it was held from. The grace is inclusive: a fret
    /// exactly at its end still counts, so this fires only after.
    fn expire_held_strum(&mut self, now: f64, events: &mut Vec<SessionEvent>) {
        let Some(strummed_at) = self.held_strum else {
            return;
        };
        let expires = strummed_at + self.track.strum_grace_s();
        if now <= expires {
            return;
        }
        self.held_strum = None;
        // What was earned up to the moment it ended still counts.
        self.tick_sustain(expires, events);
        self.overstrum(expires, events);
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
            // A strum held for a note in ITS window never gets here:
            // `resolve_held_strum` runs first on every fret change and
            // takes any note the frets now match.
            self.hit(index, time_s, events);
            self.fret_hit = Some(index);
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
        // Whatever strum was still owed to an earlier fretted note
        // is void: this hit is the newer one (a fret hit re-arms it).
        self.fret_hit = None;
        // A hit can only fill the meter; the return is the fail
        // transition and cannot be true here.
        let _ = self
            .performance
            .register_judgment(judgment, event.lanes.len());
        self.performance.register_offset_ms(offset_s * 1000.0);
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
            self.performance.register_sustain(true);
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
            let completed = time_s >= event.end_time_s() - SUSTAIN_RELEASE_GRACE_S;
            // Counted where the event is announced, so the tally and
            // the event can never disagree about the same tail.
            self.performance.register_sustain(completed);
            events.push(SessionEvent::SustainEnded {
                event_index: active.event_index,
                completed,
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

    /// Strum at `time_s` and return what the session said.
    fn strum(session: &mut TrackSession, time_s: f64) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        session.handle(
            GameInput {
                time_s,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        events
    }

    /// Press a fret WITHOUT strumming (a hammer-on) and return what
    /// the session said.
    fn hammer(session: &mut TrackSession, time_s: f64, lane: Lane) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        session.handle(
            GameInput {
                time_s,
                kind: InputKind::FretDown(lane),
            },
            &mut events,
        );
        events
    }

    #[test]
    fn overstrum_kills_the_hopo_chain() {
        let mut s = session(track(vec![tap(1.0, Lane::One), hopo(1.5, Lane::Two)]));
        play(&mut s, 1.0, Lane::One);
        // A strum into nothing (no note within the window).
        assert!(strum(&mut s, 1.2).contains(&SessionEvent::Overstrum));
        // The HOPO now needs a strum: the fret press alone hits nothing.
        assert!(hammer(&mut s, 1.5, Lane::Two).is_empty());
        assert!(matches!(s.note_state(1), Some(NoteState::Pending)));
        assert!(
            strum(&mut s, 1.52)
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. }))
        );
    }

    #[test]
    fn a_fretted_hopo_keeps_the_chain_alive_for_the_next_one() {
        let mut s = session(track(vec![
            tap(1.0, Lane::One),
            hopo(1.25, Lane::Two),
            hopo(1.5, Lane::Three),
        ]));
        play(&mut s, 1.0, Lane::One);
        assert!(
            hammer(&mut s, 1.25, Lane::Two)
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. }))
        );
        assert!(
            hammer(&mut s, 1.5, Lane::Three)
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 2, .. })),
            "the second HOPO of a run must hit on its fret press"
        );
        assert_eq!(s.performance().streak(), 3);
    }

    #[test]
    fn the_strum_of_a_hopo_you_just_fretted_is_its_strum_not_an_overstrum() {
        // The natural motion strums every note: the fret changes,
        // then the pick lands. With the chain alive the fret press
        // already hit the HOPO; the strum that follows inside the
        // note's window is that note's strum and must not count
        // against the player.
        let mut s = session(track(vec![tap(1.0, Lane::One), hopo(1.25, Lane::Two)]));
        play(&mut s, 1.0, Lane::One);
        assert!(
            hammer(&mut s, 1.25, Lane::Two)
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. }))
        );
        let events = strum(&mut s, 1.27);
        assert!(
            !events.contains(&SessionEvent::Overstrum),
            "the strum for a fretted HOPO must be absorbed; got {events:?}"
        );
        assert_eq!(s.performance().overstrums(), 0);
        assert_eq!(s.performance().streak(), 2);
        // One strum per note: the next one is an overstrum as ever.
        assert!(strum(&mut s, 1.29).contains(&SessionEvent::Overstrum));
    }

    #[test]
    fn a_strum_outside_the_fretted_hopo_s_window_is_still_an_overstrum() {
        let mut s = session(track(vec![tap(1.0, Lane::One), hopo(1.25, Lane::Two)]));
        play(&mut s, 1.0, Lane::One);
        hammer(&mut s, 1.25, Lane::Two);
        // 150 ms after the note: past the Good window, not its strum.
        assert!(strum(&mut s, 1.40).contains(&SessionEvent::Overstrum));
        assert_eq!(s.performance().overstrums(), 1);
    }

    #[test]
    fn a_second_strum_on_a_note_you_strummed_is_an_overstrum() {
        // Absorption is for the strum a FRET hit still owes; a note hit
        // by strumming owes none, so strumming it again is the
        // overstrum it always was.
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        play(&mut s, 1.0, Lane::One);
        assert!(strum(&mut s, 1.02).contains(&SessionEvent::Overstrum));
        assert_eq!(s.performance().overstrums(), 1);
    }

    #[test]
    fn a_pulled_off_hopo_absorbs_its_strum_too() {
        let mut s = session(track(vec![tap(1.0, Lane::Three), hopo(1.25, Lane::One)]));
        hammer(&mut s, 0.9, Lane::One);
        play(&mut s, 1.0, Lane::Three);
        assert!(
            release(&mut s, 1.25, Lane::Three)
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. }))
        );
        assert!(!strum(&mut s, 1.26).contains(&SessionEvent::Overstrum));
        assert_eq!(s.performance().overstrums(), 0);
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
    fn a_phrase_reports_which_events_are_in_it_and_whether_it_is_still_earnable() {
        // The mapping is static; only `broken` moves. This is what
        // the renderer asks to decide whether a note still wears a
        // star (Guitar Hero: a missed note converts the phrase's
        // remaining star notes into standard ones).
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
        assert_eq!(s.phrase_of(0), Some(0));
        assert_eq!(s.phrase_of(1), Some(0));
        assert_eq!(s.phrase_of(2), None, "outside the phrase");
        assert_eq!(s.phrase_of(99), None, "no such event");
        assert!(!s.phrase_broken(0));
        assert!(!s.phrase_broken(7), "no such phrase");

        // Miss the first note of the phrase.
        let mut events = Vec::new();
        s.advance(2.5, &mut events);
        assert!(s.phrase_broken(0));
        assert_eq!(
            (s.phrase_of(0), s.phrase_of(1)),
            (Some(0), Some(0)),
            "the mapping does not move when the phrase breaks"
        );

        // A rewind re-opens it: the notes are pending again, so the
        // phrase can be earned on the next pass.
        s.rewind_to(0.0);
        assert!(!s.phrase_broken(0));

        // But a rewind to a point AFTER the phrase leaves the misses
        // standing, and the phrase must stay broken — the state is
        // re-derived from the notes, not simply cleared.
        let mut events = Vec::new();
        s.advance(2.5, &mut events);
        assert!(s.phrase_broken(0));
        s.rewind_to(2.6);
        assert!(
            s.phrase_broken(0),
            "the misses behind the rewind point still count"
        );
    }

    #[test]
    fn an_armed_session_fails_exactly_once_and_an_unarmed_one_never() {
        // Twenty notes nobody plays. Armed: the meter empties after
        // enough misses and Failed is emitted ONCE, even though the
        // misses keep coming. Unarmed (No Fail): the same misses, no
        // event, ever.
        let notes: Vec<NoteEvent> = (0..20)
            .map(|i| tap(1.0 + i as f64 * 0.5, Lane::One))
            .collect();
        for armed in [true, false] {
            let cfg = ScoreConfig {
                fail_when_empty: armed,
                ..ScoreConfig::default()
            };
            let mut s = TrackSession::new(
                track_with_phrases(notes.clone(), vec![]),
                TimingWindows::default(),
                cfg,
            );
            let mut events = Vec::new();
            s.advance(30.0, &mut events);
            let failures = events
                .iter()
                .filter(|e| matches!(e, SessionEvent::Failed))
                .count();
            assert_eq!(s.performance().counts().miss, 20, "every note was missed");
            if armed {
                assert_eq!(failures, 1, "armed: exactly one Failed");
                assert!(s.performance().failed());
            } else {
                assert_eq!(failures, 0, "no fail: never");
                assert!(!s.performance().failed());
                assert!(
                    s.performance().meter().abs() < 1e-9,
                    "but the meter did empty"
                );
            }
        }
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
    fn successful_hype_activation_auto_hits_for_half_a_second() {
        let mut s = session(track_with_phrases(
            vec![
                tap(1.0, Lane::One),
                tap(2.0, Lane::One),
                tap(2.5, Lane::Two),
                tap(2.99, Lane::Three),
                tap(3.01, Lane::Four),
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
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 2.5,
                kind: InputKind::ActivateHype,
            },
            &mut events,
        );
        s.advance(3.3, &mut events);

        assert!(matches!(
            s.note_state(2),
            Some(NoteState::Hit(Judgment::Perfect))
        ));
        assert!(matches!(
            s.note_state(3),
            Some(NoteState::Hit(Judgment::Perfect))
        ));
        assert_eq!(s.note_state(4), Some(NoteState::Missed));
        assert_eq!(s.performance().best_streak(), 4);
    }

    /// The autopilot blamed a chart for this for five weeks (roadmap
    /// C7): activate Hype, then play the next note the way anyone
    /// would. The window takes the note without a strum, and the
    /// strum that was already on its way lands on nothing.
    #[test]
    fn the_strum_of_a_note_the_hype_window_took_is_absorbed() {
        let mut s = session(track_with_phrases(
            vec![
                tap(1.0, Lane::One),
                tap(2.0, Lane::One),
                tap(2.4, Lane::One),
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
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 2.2,
                kind: InputKind::ActivateHype,
            },
            &mut events,
        );
        // The window reaches the third note and takes it — no fret
        // change was needed, so nothing else could have hit it.
        s.advance(2.4, &mut events);
        assert!(
            matches!(s.note_state(2), Some(NoteState::Hit(_))),
            "the window takes the note: {:?}",
            s.note_state(2)
        );

        // …and now the player's strum arrives, on time for that note.
        events.clear();
        s.handle(
            GameInput {
                time_s: 2.4,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert_eq!(
            s.performance().overstrums(),
            0,
            "a strum for a note the window just took is that note's \
             strum, not an extra one: {events:?}"
        );
        assert_eq!(
            s.performance().best_streak(),
            3,
            "and the streak survives it"
        );

        // One, though — a second strum is a real extra one.
        s.handle(
            GameInput {
                time_s: 2.42,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert_eq!(
            s.performance().overstrums(),
            1,
            "the absorb is one per note, as it is for a fretted HOPO"
        );
    }

    #[test]
    fn a_strum_long_after_the_window_took_a_note_is_still_an_overstrum() {
        let mut s = session(track_with_phrases(
            vec![
                tap(1.0, Lane::One),
                tap(2.0, Lane::One),
                tap(2.4, Lane::One),
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
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 2.2,
                kind: InputKind::ActivateHype,
            },
            &mut events,
        );
        s.advance(2.4, &mut events);
        s.handle(
            GameInput {
                time_s: 3.5,
                kind: InputKind::Strum,
            },
            &mut events,
        );
        assert_eq!(
            s.performance().overstrums(),
            1,
            "the absorb is bounded by the note's own window — a strum a \
             second later is a strum at nothing"
        );
    }

    #[test]
    fn failed_hype_attempt_opens_no_automatic_hit_window() {
        let mut s = session(track(vec![tap(1.0, Lane::One)]));
        let mut events = Vec::new();
        s.handle(
            GameInput {
                time_s: 0.8,
                kind: InputKind::ActivateHype,
            },
            &mut events,
        );
        s.advance(1.3, &mut events);
        assert_eq!(s.note_state(0), Some(NoteState::Missed));
        assert!(!events.contains(&SessionEvent::HypeActivated));
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

    #[test]
    fn rewind_reopens_only_the_events_at_or_after_the_point() {
        let mut s = session(track(vec![
            tap(1.0, Lane::One),
            tap(2.0, Lane::Two),
            tap(3.0, Lane::Three),
        ]));
        play(&mut s, 1.0, Lane::One);
        play(&mut s, 2.0, Lane::Two);
        assert!(matches!(s.note_state(0), Some(NoteState::Hit(_))));
        assert!(matches!(s.note_state(1), Some(NoteState::Hit(_))));
        s.rewind_to(1.5);
        // The first hit is history the loop does not touch; the
        // second is inside the section and judgeable again.
        assert!(matches!(s.note_state(0), Some(NoteState::Hit(_))));
        assert!(matches!(s.note_state(1), Some(NoteState::Pending)));
        assert!(matches!(s.note_state(2), Some(NoteState::Pending)));
        assert!(!s.finished());
        let events = play(&mut s, 2.0, Lane::Two);
        assert!(
            matches!(
                events.as_slice(),
                [
                    SessionEvent::NoteHit {
                        event_index: 1,
                        judgment: Judgment::Perfect,
                        ..
                    },
                    ..
                ]
            ),
            "a reopened note judges again: {events:?}"
        );
    }

    #[test]
    fn rewind_moves_the_session_clock_back_without_phantom_misses() {
        let mut s = session(track(vec![tap(2.0, Lane::One)]));
        let mut drain = Vec::new();
        s.advance(5.0, &mut drain);
        assert!(matches!(s.note_state(0), Some(NoteState::Missed)));
        s.rewind_to(0.5);
        assert!(matches!(s.note_state(0), Some(NoteState::Pending)));
        // Advancing to just before the note again must not re-miss
        // it — the session clock went back with the rewind.
        drain.clear();
        s.advance(1.5, &mut drain);
        assert!(
            drain
                .iter()
                .all(|e| !matches!(e, SessionEvent::NoteMissed { .. })),
            "{drain:?}"
        );
        assert!(matches!(s.note_state(0), Some(NoteState::Pending)));
    }

    #[test]
    fn rewind_recomputes_phrases_and_clears_the_transients() {
        use crate::note::Phrase;
        // A phrase over both notes: hit one, miss one, rewind — the
        // phrase must be whole again (unbroken, one hit remembered
        // as zero because both notes reopened).
        let mut s = session(track_with_phrases(
            vec![tap(1.0, Lane::One), tap(2.0, Lane::Two)],
            vec![Phrase {
                start_s: 0.5,
                end_s: 2.5,
            }],
        ));
        play(&mut s, 1.0, Lane::One);
        let mut drain = Vec::new();
        s.advance(5.0, &mut drain); // second note missed -> phrase broken
        s.rewind_to(0.5);
        // Both notes reopened; the phrase is intact and countable.
        play(&mut s, 1.0, Lane::One);
        play(&mut s, 2.0, Lane::Two);
        assert!(
            s.performance().hype_meter() > 0.0,
            "a re-played phrase must complete (meter {})",
            s.performance().hype_meter()
        );
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
    fn tap_mode_absorbs_the_strum_of_a_note_just_hit_by_fretting() {
        let mut s = session(vec![NoteEvent::tap(1.0, LaneSet::single(Lane::Two))]);
        s.set_tap_mode(true);
        press(&mut s, Lane::Two, 1.0);
        let mut out = Vec::new();
        s.handle(
            GameInput {
                time_s: 1.02,
                kind: InputKind::Strum,
            },
            &mut out,
        );
        assert!(!out.contains(&SessionEvent::Overstrum), "got {out:?}");
        assert_eq!(s.performance().overstrums(), 0);
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod strum_grace_tests {
    //! The strum-grace rule (the classic programme, K2).
    use super::*;
    use crate::timing::TempoMap;
    use crate::{
        Difficulty, Lane, LaneSet, NoteEvent, NoteKind, ScoreConfig, TimingWindows, Track,
    };

    fn track(events: Vec<NoteEvent>) -> Track {
        Track::new(
            Difficulty::Expert,
            TempoMap::constant(120.0, 0.0),
            events,
            vec![],
        )
        .unwrap()
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
        let mut events = input(session, time_s, InputKind::FretDown(lane));
        events.extend(input(session, time_s, InputKind::Strum));
        events
    }

    /// A track played by the strum-grace rule.
    fn graced(events: Vec<NoteEvent>, grace_s: f64) -> TrackSession {
        session(track(events).with_strum_grace(grace_s))
    }

    fn input(session: &mut TrackSession, time_s: f64, kind: InputKind) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        session.handle(GameInput { time_s, kind }, &mut events);
        events
    }

    fn overstrums(events: &[SessionEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, SessionEvent::Overstrum))
            .count()
    }

    /// ⚠️ The ingredient itself: strum first, fret a moment later —
    /// the order a hand really moves in on a fast change — and it is
    /// the note's strum, judged where the PICK landed.
    #[test]
    fn a_fret_that_follows_the_strum_within_the_grace_hits_the_note() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        let strummed = input(&mut s, 1.0, InputKind::Strum);
        assert!(
            strummed.is_empty(),
            "the strum was judged on the spot: {strummed:?}"
        );
        let fretted = input(&mut s, 1.04, InputKind::FretDown(Lane::Two));
        assert!(
            matches!(
                fretted.as_slice(),
                [SessionEvent::NoteHit {
                    event_index: 0,
                    judgment: Judgment::Perfect,
                    offset_s,
                }] if offset_s.abs() < 1e-12
            ),
            "{fretted:?}"
        );
        assert_eq!(s.performance().overstrums(), 0);
        assert_eq!(s.performance().streak(), 1);
    }

    /// The same inputs on a track without the rule: the overstrum it
    /// has always been, on the spot. Every chart that does not ask
    /// for the grace plays exactly as before.
    #[test]
    fn without_the_grace_the_same_strum_is_an_overstrum_on_the_spot() {
        let mut s = session(track(vec![tap(1.0, Lane::Two)]));
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        let strummed = input(&mut s, 1.0, InputKind::Strum);
        assert_eq!(overstrums(&strummed), 1);
        input(&mut s, 1.04, InputKind::FretDown(Lane::Two));
        assert_eq!(s.note_state(0), Some(NoteState::Pending));
    }

    /// A fret that comes too late leaves the overstrum it was — charged
    /// when the grace ends, and the note is still there to be played.
    #[test]
    fn a_fret_after_the_grace_leaves_the_overstrum() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.0, InputKind::Strum);
        let late = input(&mut s, 1.07, InputKind::FretDown(Lane::Two));
        assert_eq!(overstrums(&late), 1, "{late:?}");
        assert!(
            !late
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. })),
            "the expired strum still hit: {late:?}"
        );
        assert_eq!(s.note_state(0), Some(NoteState::Pending));
        // And a real strum now takes it.
        let again = input(&mut s, 1.08, InputKind::Strum);
        assert!(matches!(again.as_slice(), [SessionEvent::NoteHit { .. }]));
    }

    /// The grace ends with time, not only with the next input: a held
    /// strum nobody answers becomes an overstrum as the clock passes.
    #[test]
    fn an_unanswered_strum_expires_with_the_clock() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.0, InputKind::Strum);
        let mut events = Vec::new();
        s.advance(1.05, &mut events);
        assert_eq!(overstrums(&events), 0, "expired early: {events:?}");
        s.advance(1.061, &mut events);
        assert_eq!(overstrums(&events), 1, "never expired: {events:?}");
        assert_eq!(s.performance().overstrums(), 1);
    }

    /// Inclusive: a fret exactly at the end of the grace still counts.
    #[test]
    fn a_fret_exactly_at_the_end_of_the_grace_counts() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.0625);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.0, InputKind::Strum);
        let edge = input(&mut s, 1.0625, InputKind::FretDown(Lane::Two));
        assert!(
            matches!(edge.as_slice(), [SessionEvent::NoteHit { .. }]),
            "{edge:?}"
        );
    }

    /// Only a strum with a note in its window is held. A strum into
    /// nothing is not a fret that is late — there is nothing it could
    /// be for.
    #[test]
    fn a_strum_with_no_note_in_its_window_is_never_held() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.4, InputKind::FretDown(Lane::Two));
        let early = input(&mut s, 0.5, InputKind::Strum);
        assert_eq!(overstrums(&early), 1, "{early:?}");
    }

    /// ⚠️ A late strum waits for a note whose window is closing. The
    /// miss must wait with it, or the grace could never help the late
    /// half of a window — the note would be missed before the fret
    /// that rescues it is even read.
    #[test]
    fn a_late_strum_holds_the_note_it_waits_for() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.09, InputKind::Strum);
        let fretted = input(&mut s, 1.13, InputKind::FretDown(Lane::Two));
        assert!(
            matches!(
                fretted.as_slice(),
                [SessionEvent::NoteHit { judgment: Judgment::Good, offset_s, .. }]
                    if (offset_s - 0.09).abs() < 1e-9
            ),
            "the note was missed while its strum waited: {fretted:?}"
        );
        // And once the strum is spent, misses resume as ever.
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.09, InputKind::Strum);
        let mut events = Vec::new();
        s.advance(1.3, &mut events);
        assert_eq!(overstrums(&events), 1);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteMissed { event_index: 0 })),
            "{events:?}"
        );
    }

    /// Two strums before any fret: one fretting, two picks — the first
    /// is the extra one. The second is held in its place.
    #[test]
    fn a_second_strum_before_the_fret_makes_the_first_an_overstrum() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 0.99, InputKind::Strum);
        let second = input(&mut s, 1.01, InputKind::Strum);
        assert_eq!(overstrums(&second), 1, "{second:?}");
        let fretted = input(&mut s, 1.03, InputKind::FretDown(Lane::Two));
        assert!(
            matches!(fretted.as_slice(), [SessionEvent::NoteHit { offset_s, .. }]
                if (offset_s - 0.01).abs() < 1e-9),
            "the second strum was not the one held: {fretted:?}"
        );
    }

    /// A chord waits for its whole shape: the first fret alone matches
    /// nothing, the second completes it.
    #[test]
    fn a_chord_waits_for_its_last_fret() {
        let chord = NoteEvent::tap(1.0, LaneSet::from_lanes([Lane::One, Lane::Three]));
        let mut s = graced(vec![chord], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        assert!(input(&mut s, 1.0, InputKind::Strum).is_empty());
        assert!(
            input(&mut s, 1.02, InputKind::FretDown(Lane::Three))
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { .. }))
        );
        assert_eq!(s.performance().overstrums(), 0);
    }

    /// A release can be the correction too: the wrong higher fret
    /// lifted, the right one under it exposed.
    #[test]
    fn lifting_the_wrong_fret_resolves_the_held_strum() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::Two));
        input(&mut s, 0.9, InputKind::FretDown(Lane::Four));
        assert!(input(&mut s, 1.0, InputKind::Strum).is_empty());
        let lifted = input(&mut s, 1.03, InputKind::FretUp(Lane::Four));
        assert!(
            matches!(lifted.as_slice(), [SessionEvent::NoteHit { .. }]),
            "{lifted:?}"
        );
    }

    /// ⚠️ A held strum answered on a hammer-on is ONE hit: the pick
    /// and the fret were one motion, in the other order. The absorb
    /// marker a fretted hammer-on arms must not be armed as well, or a
    /// second, genuinely extra strum would be forgiven.
    #[test]
    fn a_held_strum_answered_by_a_hammer_on_does_not_forgive_another() {
        let mut s = graced(vec![tap(0.5, Lane::One), hopo(1.0, Lane::Two)], 0.06);
        play(&mut s, 0.5, Lane::One);
        input(&mut s, 0.6, InputKind::FretUp(Lane::One));
        input(&mut s, 0.95, InputKind::FretDown(Lane::Three));
        assert!(input(&mut s, 1.0, InputKind::Strum).is_empty());
        input(&mut s, 1.0, InputKind::FretUp(Lane::Three));
        let hammered = input(&mut s, 1.02, InputKind::FretDown(Lane::Two));
        assert!(
            hammered
                .iter()
                .any(|e| matches!(e, SessionEvent::NoteHit { event_index: 1, .. })),
            "{hammered:?}"
        );
        assert_eq!(s.performance().overstrums(), 0);
        let extra = input(&mut s, 1.05, InputKind::Strum);
        assert_eq!(
            overstrums(&extra),
            1,
            "a second strum was forgiven: {extra:?}"
        );
    }

    /// Rewinding (the practice loop) drops a held strum: it belongs to
    /// a moment that is being replayed.
    #[test]
    fn a_rewind_drops_the_held_strum() {
        let mut s = graced(vec![tap(1.0, Lane::Two)], 0.06);
        input(&mut s, 0.9, InputKind::FretDown(Lane::One));
        input(&mut s, 1.0, InputKind::Strum);
        s.rewind_to(0.5);
        let mut events = Vec::new();
        s.advance(1.2, &mut events);
        assert_eq!(
            overstrums(&events),
            0,
            "the rewound strum came back: {events:?}"
        );
    }

    /// The grace a track may carry is bounded and never negative, and
    /// nonsense switches it off rather than guessing.
    #[test]
    fn the_grace_is_clamped_into_its_range() {
        let t = || track(vec![tap(1.0, Lane::One)]);
        assert_eq!(t().strum_grace_s(), 0.0);
        assert_eq!(t().with_strum_grace(0.06).strum_grace_s(), 0.06);
        assert_eq!(
            t().with_strum_grace(5.0).strum_grace_s(),
            Track::MAX_STRUM_GRACE_S
        );
        assert_eq!(t().with_strum_grace(-1.0).strum_grace_s(), 0.0);
        assert_eq!(t().with_strum_grace(f64::NAN).strum_grace_s(), 0.0);
    }
}
