//! Conversion from the file schema into playable [`beatbyte_core`]
//! tracks: per-lane notes are grouped into note events (chords), and
//! the tempo/phrase data becomes domain types.

use beatbyte_core::{Difficulty, Lane, LaneSet, NoteEvent, NoteKind, Phrase, TempoMap, Track};
use thiserror::Error;

use crate::schema::{ChartDef, ChartFile};

/// Notes within this distance are considered simultaneous (one chord).
pub const CHORD_EPSILON_S: f64 = 0.005;

/// Errors converting a chart into a playable track.
#[derive(Debug, Error, PartialEq)]
pub enum ConvertError {
    /// The requested difficulty is not present in the file.
    #[error("chart has no `{0}` difficulty")]
    MissingDifficulty(Difficulty),
    /// A note referenced an invalid lane (validation should catch this
    /// first; conversion refuses to guess).
    #[error("note at {time_s}s references invalid lane {lane}")]
    InvalidLane {
        /// The offending time.
        time_s: f64,
        /// The offending lane index.
        lane: u8,
    },
    /// The grouped events violated a track invariant.
    #[error("track construction failed: {0}")]
    Track(beatbyte_core::note::TrackError),
}

impl ChartFile {
    /// Convert one difficulty into a playable [`Track`].
    ///
    /// Simultaneous notes (within [`CHORD_EPSILON_S`]) merge into chord
    /// events: lanes union, sustain = longest, HOPO only if every merged
    /// note is a HOPO.
    pub fn to_track(&self, difficulty: Difficulty) -> Result<Track, ConvertError> {
        let chart = self
            .chart_for(difficulty)
            .ok_or(ConvertError::MissingDifficulty(difficulty))?;
        let tempo = TempoMap::constant(self.song.bpm, self.song.offset_s);
        let events = group_notes(chart)?;
        let phrases = chart
            .phrases
            .iter()
            .map(|p| Phrase {
                start_s: p.start,
                end_s: p.end,
            })
            .collect();
        Track::new(difficulty, tempo, events, phrases).map_err(ConvertError::Track)
    }

    /// Convert every difficulty present in the file.
    pub fn to_tracks(&self) -> Result<Vec<Track>, ConvertError> {
        self.charts
            .iter()
            .map(|c| self.to_track(c.difficulty))
            .collect()
    }
}

fn group_notes(chart: &ChartDef) -> Result<Vec<NoteEvent>, ConvertError> {
    let mut notes = chart.notes.clone();
    notes.sort_by(|a, b| a.time.total_cmp(&b.time));

    let mut events: Vec<NoteEvent> = Vec::new();
    for note in &notes {
        let lane = Lane::from_index(note.lane as usize).ok_or(ConvertError::InvalidLane {
            time_s: note.time,
            lane: note.lane,
        })?;

        match events.last_mut() {
            Some(last) if (note.time - last.time_s).abs() <= CHORD_EPSILON_S => {
                // Merge into the chord anchored at the first note's time.
                last.lanes.insert(lane);
                last.sustain_s = last.sustain_s.max(note.len);
                if !note.hopo {
                    last.kind = NoteKind::Strum;
                }
            }
            _ => {
                events.push(NoteEvent {
                    time_s: note.time,
                    lanes: LaneSet::single(lane),
                    sustain_s: note.len.max(0.0),
                    kind: if note.hopo {
                        NoteKind::Hopo
                    } else {
                        NoteKind::Strum
                    },
                });
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::schema::{ChartNote, SongMeta};

    fn file_with_notes(notes: Vec<ChartNote>) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "T".into(),
                artist: "A".into(),
                audio: "a.ogg".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Expert,
                lanes: 5,
                notes,
                phrases: vec![],
            }],
            provenance: None,
        }
    }

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    #[test]
    fn simultaneous_notes_become_a_chord() {
        let file = file_with_notes(vec![note(1.0, 0), note(1.0, 2), note(2.0, 1)]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        assert_eq!(track.len(), 2);
        let chord = track.events()[0];
        assert!(chord.is_chord());
        assert_eq!(chord.lanes.len(), 2);
        assert!(chord.lanes.contains(Lane::One));
        assert!(chord.lanes.contains(Lane::Three));
    }

    #[test]
    fn near_simultaneous_notes_merge_within_epsilon() {
        let file = file_with_notes(vec![note(1.0, 0), note(1.003, 1)]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        assert_eq!(track.len(), 1);
        assert!(track.events()[0].is_chord());
    }

    #[test]
    fn distinct_notes_stay_separate() {
        let file = file_with_notes(vec![note(1.0, 0), note(1.05, 1)]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        assert_eq!(track.len(), 2);
    }

    #[test]
    fn chord_takes_longest_sustain() {
        let file = file_with_notes(vec![
            ChartNote {
                time: 1.0,
                lane: 0,
                len: 0.5,
                hopo: false,
            },
            ChartNote {
                time: 1.0,
                lane: 1,
                len: 2.0,
                hopo: false,
            },
        ]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        assert!((track.events()[0].sustain_s - 2.0).abs() < 1e-9);
    }

    #[test]
    fn chord_is_hopo_only_if_all_notes_are() {
        let file = file_with_notes(vec![
            ChartNote {
                time: 1.0,
                lane: 0,
                len: 0.0,
                hopo: true,
            },
            ChartNote {
                time: 1.0,
                lane: 1,
                len: 0.0,
                hopo: false,
            },
            ChartNote {
                time: 2.0,
                lane: 0,
                len: 0.0,
                hopo: true,
            },
        ]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        assert_eq!(track.events()[0].kind, NoteKind::Strum);
        assert_eq!(track.events()[1].kind, NoteKind::Hopo);
    }

    #[test]
    fn unsorted_input_is_sorted() {
        let file = file_with_notes(vec![note(3.0, 0), note(1.0, 1), note(2.0, 2)]);
        let track = file.to_track(Difficulty::Expert).unwrap();
        let times: Vec<f64> = track.events().iter().map(|e| e.time_s).collect();
        assert_eq!(times, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn missing_difficulty_errors() {
        let file = file_with_notes(vec![note(1.0, 0)]);
        assert_eq!(
            file.to_track(Difficulty::Easy).unwrap_err(),
            ConvertError::MissingDifficulty(Difficulty::Easy)
        );
    }

    #[test]
    fn invalid_lane_errors() {
        let file = file_with_notes(vec![note(1.0, 7)]);
        assert!(matches!(
            file.to_track(Difficulty::Expert).unwrap_err(),
            ConvertError::InvalidLane { lane: 7, .. }
        ));
    }
}
