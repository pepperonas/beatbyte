//! Notes, note events and playable tracks.
//!
//! The gameplay unit is the [`NoteEvent`]: everything the player must
//! answer with a single action at one instant — a single note or a chord,
//! optionally sustained. Charts on disk store per-lane notes; the chart
//! loader groups simultaneous notes into events (see `beatbyte-chart`).

use serde::{Deserialize, Serialize};

use crate::difficulty::Difficulty;
use crate::lane::LaneSet;
use crate::timing::TempoMap;

/// How a note event may be hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    /// Requires a strum with the correct frets held.
    Strum,
    /// Hammer-on / pull-off: may be hit by fretting alone while the
    /// streak is alive; strumming also works.
    Hopo,
}

/// One gameplay instant: a note or chord, optionally sustained.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteEvent {
    /// Song time in seconds at which the event must be hit.
    pub time_s: f64,
    /// The lanes involved. One lane = single note; several = chord.
    pub lanes: LaneSet,
    /// Sustain length in seconds after `time_s` (`0.0` = tap note).
    pub sustain_s: f64,
    /// How the event may be hit.
    pub kind: NoteKind,
}

impl NoteEvent {
    /// A plain strummed tap note/chord.
    #[must_use]
    pub fn tap(time_s: f64, lanes: LaneSet) -> NoteEvent {
        NoteEvent {
            time_s,
            lanes,
            sustain_s: 0.0,
            kind: NoteKind::Strum,
        }
    }

    /// Whether this event is a chord (more than one lane).
    #[must_use]
    pub fn is_chord(&self) -> bool {
        self.lanes.len() > 1
    }

    /// Whether this event has a sustain tail.
    #[must_use]
    pub fn is_sustain(&self) -> bool {
        self.sustain_s > 0.0
    }

    /// The song time at which the sustain tail ends.
    #[must_use]
    pub fn end_time_s(&self) -> f64 {
        self.time_s + self.sustain_s
    }
}

/// A special phrase: hit every event inside `[start_s, end_s]` without
/// missing to earn a chunk of Hype meter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    /// Inclusive phrase start on the song timeline, in seconds.
    pub start_s: f64,
    /// Inclusive phrase end, in seconds.
    pub end_s: f64,
}

impl Phrase {
    /// Whether the given time lies within the phrase.
    #[must_use]
    pub fn contains(&self, time_s: f64) -> bool {
        (self.start_s..=self.end_s).contains(&time_s)
    }
}

/// A playable track: one difficulty's content for one song.
///
/// Invariants (enforced by [`Track::new`]):
/// - events are sorted by time and strictly increasing
///   (no two events at the same instant — simultaneous notes belong to
///   one chord event),
/// - phrases are sorted and non-overlapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// The difficulty this track represents.
    pub difficulty: Difficulty,
    /// Tempo map for beat↔second conversion (sustain scoring, effects).
    pub tempo: TempoMap,
    events: Vec<NoteEvent>,
    phrases: Vec<Phrase>,
}

impl Track {
    /// Minimum spacing between two events, in seconds. Events closer
    /// than this are considered simultaneous and rejected.
    pub const MIN_EVENT_SPACING_S: f64 = 1e-6;

    /// Build a track, normalizing order and enforcing invariants.
    pub fn new(
        difficulty: Difficulty,
        tempo: TempoMap,
        mut events: Vec<NoteEvent>,
        mut phrases: Vec<Phrase>,
    ) -> Result<Track, TrackError> {
        for (i, event) in events.iter().enumerate() {
            if !event.time_s.is_finite() || event.time_s < 0.0 {
                return Err(TrackError::InvalidEventTime {
                    index: i,
                    time_s: event.time_s,
                });
            }
            if !event.sustain_s.is_finite() || event.sustain_s < 0.0 {
                return Err(TrackError::InvalidSustain {
                    index: i,
                    sustain_s: event.sustain_s,
                });
            }
            if event.lanes.is_empty() {
                return Err(TrackError::EmptyLanes { index: i });
            }
        }
        events.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        for pair in events.windows(2) {
            if pair[1].time_s - pair[0].time_s < Self::MIN_EVENT_SPACING_S {
                return Err(TrackError::SimultaneousEvents {
                    time_s: pair[0].time_s,
                });
            }
        }

        for (i, phrase) in phrases.iter().enumerate() {
            if !phrase.start_s.is_finite()
                || !phrase.end_s.is_finite()
                || phrase.start_s < 0.0
                || phrase.end_s < phrase.start_s
            {
                return Err(TrackError::InvalidPhrase { index: i });
            }
        }
        phrases.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        for pair in phrases.windows(2) {
            if pair[1].start_s <= pair[0].end_s {
                return Err(TrackError::OverlappingPhrases {
                    time_s: pair[1].start_s,
                });
            }
        }

        Ok(Track {
            difficulty,
            tempo,
            events,
            phrases,
        })
    }

    /// The note events, sorted by time.
    #[must_use]
    pub fn events(&self) -> &[NoteEvent] {
        &self.events
    }

    /// The special phrases, sorted and non-overlapping.
    #[must_use]
    pub fn phrases(&self) -> &[Phrase] {
        &self.phrases
    }

    /// Total number of note events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the track has no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The end of playable content: last event time plus its sustain,
    /// or `0.0` for an empty track.
    #[must_use]
    pub fn content_end_s(&self) -> f64 {
        self.events
            .iter()
            .map(NoteEvent::end_time_s)
            .fold(0.0, f64::max)
    }
}

/// Errors when constructing a [`Track`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackError {
    /// An event time was negative or non-finite.
    InvalidEventTime {
        /// Index of the offending event (pre-sort order).
        index: usize,
        /// The offending time.
        time_s: f64,
    },
    /// A sustain length was negative or non-finite.
    InvalidSustain {
        /// Index of the offending event (pre-sort order).
        index: usize,
        /// The offending sustain length.
        sustain_s: f64,
    },
    /// An event had no lanes.
    EmptyLanes {
        /// Index of the offending event (pre-sort order).
        index: usize,
    },
    /// Two events shared (nearly) the same instant.
    SimultaneousEvents {
        /// The shared time.
        time_s: f64,
    },
    /// A phrase had invalid bounds.
    InvalidPhrase {
        /// Index of the offending phrase (pre-sort order).
        index: usize,
    },
    /// Two phrases overlapped.
    OverlappingPhrases {
        /// Start time of the second phrase.
        time_s: f64,
    },
}

impl core::fmt::Display for TrackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TrackError::InvalidEventTime { index, time_s } => {
                write!(f, "event {index}: invalid time {time_s}")
            }
            TrackError::InvalidSustain { index, sustain_s } => {
                write!(f, "event {index}: invalid sustain length {sustain_s}")
            }
            TrackError::EmptyLanes { index } => {
                write!(f, "event {index}: no lanes")
            }
            TrackError::SimultaneousEvents { time_s } => {
                write!(
                    f,
                    "two events at the same instant ({time_s}s); simultaneous notes must form one chord event"
                )
            }
            TrackError::InvalidPhrase { index } => {
                write!(f, "phrase {index}: invalid bounds")
            }
            TrackError::OverlappingPhrases { time_s } => {
                write!(f, "phrases overlap at {time_s}s")
            }
        }
    }
}

impl core::error::Error for TrackError {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::lane::{Lane, LaneSet};

    fn tempo() -> TempoMap {
        TempoMap::constant(120.0, 0.0)
    }

    fn ev(time_s: f64, lane: Lane) -> NoteEvent {
        NoteEvent::tap(time_s, LaneSet::single(lane))
    }

    #[test]
    fn track_sorts_events_by_time() {
        let track = Track::new(
            Difficulty::Easy,
            tempo(),
            vec![ev(2.0, Lane::One), ev(1.0, Lane::Two), ev(3.0, Lane::Three)],
            vec![],
        )
        .unwrap();
        let times: Vec<f64> = track.events().iter().map(|e| e.time_s).collect();
        assert_eq!(times, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn simultaneous_events_are_rejected() {
        let err = Track::new(
            Difficulty::Easy,
            tempo(),
            vec![ev(1.0, Lane::One), ev(1.0, Lane::Two)],
            vec![],
        )
        .unwrap_err();
        assert!(matches!(err, TrackError::SimultaneousEvents { .. }));
    }

    #[test]
    fn chords_are_one_event() {
        let chord = NoteEvent::tap(1.0, LaneSet::from_lanes([Lane::One, Lane::Two]));
        assert!(chord.is_chord());
        let track = Track::new(Difficulty::Hard, tempo(), vec![chord], vec![]).unwrap();
        assert_eq!(track.len(), 1);
    }

    #[test]
    fn invalid_events_are_rejected() {
        assert!(matches!(
            Track::new(Difficulty::Easy, tempo(), vec![ev(-1.0, Lane::One)], vec![]),
            Err(TrackError::InvalidEventTime { .. })
        ));
        assert!(matches!(
            Track::new(
                Difficulty::Easy,
                tempo(),
                vec![NoteEvent {
                    time_s: 1.0,
                    lanes: LaneSet::single(Lane::One),
                    sustain_s: -0.5,
                    kind: NoteKind::Strum,
                }],
                vec![]
            ),
            Err(TrackError::InvalidSustain { .. })
        ));
        assert!(matches!(
            Track::new(
                Difficulty::Easy,
                tempo(),
                vec![NoteEvent::tap(1.0, LaneSet::EMPTY)],
                vec![]
            ),
            Err(TrackError::EmptyLanes { .. })
        ));
    }

    #[test]
    fn phrases_must_not_overlap() {
        let err = Track::new(
            Difficulty::Easy,
            tempo(),
            vec![],
            vec![
                Phrase {
                    start_s: 0.0,
                    end_s: 2.0,
                },
                Phrase {
                    start_s: 1.5,
                    end_s: 3.0,
                },
            ],
        )
        .unwrap_err();
        assert!(matches!(err, TrackError::OverlappingPhrases { .. }));
    }

    #[test]
    fn content_end_includes_sustains() {
        let track = Track::new(
            Difficulty::Easy,
            tempo(),
            vec![
                NoteEvent {
                    time_s: 1.0,
                    lanes: LaneSet::single(Lane::One),
                    sustain_s: 4.0,
                    kind: NoteKind::Strum,
                },
                ev(2.0, Lane::Two),
            ],
            vec![],
        )
        .unwrap();
        assert!((track.content_end_s() - 5.0).abs() < 1e-9);
    }
}
