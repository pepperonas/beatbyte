//! `notes.mid`: the MIDI chart format.
//!
//! A standard MIDI file whose guitar part is the track named
//! `PART GUITAR`. Each difficulty owns a range of note numbers —
//! Easy from 60, Medium 72, Hard 84, Expert 96: the five frets, then
//! force-HOPO and force-strum. A note's length is its sustain. Shared
//! by all difficulties: 116 star power and 104 tap. Opens come two
//! ways: one note under the green with `[ENHANCED_OPENS]`, or a
//! vendor sysex (`50 53 00 00 <diff> 01 <on/off>`) that turns the
//! greens inside it into opens; the same sysex with type 04 marks taps.
//!
//! A small reader of its own rather than a dependency: everything
//! needed is the chunk walk, variable-length deltas, running status
//! and three meta events. Untrusted input: every length is checked
//! against the bytes that are there, counts are bounded, and anything
//! unreadable ends that track rather than the parse.

use std::collections::BTreeMap;

use beatbyte_core::Difficulty;

use super::{BridgeError, Force, TickNote, TickSong, TickTrack};

/// Longest file read, in bytes.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Most tracks read.
const MAX_TRACKS: usize = 128;
/// Most events read per track.
const MAX_EVENTS: usize = 2_000_000;
/// Most tempo and time-signature events kept (the text format's cap).
const MAX_MAP_EVENTS: usize = 100_000;

/// The track that holds the lead guitar, in either spelling.
const GUITAR_TRACKS: [&str; 2] = ["PART GUITAR", "T1 GEMS"];

/// Where each difficulty's notes start.
const BASES: [(Difficulty, u8); 4] = [
    (Difficulty::Easy, 60),
    (Difficulty::Medium, 72),
    (Difficulty::Hard, 84),
    (Difficulty::Expert, 96),
];
/// Star power.
const STAR_POWER: u8 = 116;
/// Tap.
const TAP: u8 = 104;

/// One event of one track, as far as a chart needs it.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    /// A note sounding from `tick` for `len` ticks.
    Note { tick: u64, len: u64, key: u8 },
    /// A text or track-name meta event.
    Text { tick: u64, text: String, name: bool },
    /// A tempo: microseconds per quarter.
    Tempo { tick: u64, us: f64 },
    /// A time signature.
    Signature { tick: u64, num: u32, den: u32 },
    /// The vendor sysex: difficulty (0–3, 0xFF all), kind, on.
    Vendor {
        tick: u64,
        diff: u8,
        kind: u8,
        on: bool,
    },
}

/// Parse a `notes.mid`. Pure — tested.
///
/// # Errors
/// When the bytes are not a standard MIDI file with a tick division.
pub fn parse_midi(bytes: &[u8]) -> Result<TickSong, BridgeError> {
    if bytes.len() > MAX_BYTES {
        return Err(BridgeError::Parse("the MIDI file is too large".into()));
    }
    let bad = |why: &str| BridgeError::Parse(why.to_owned());
    if bytes.len() < 14 || &bytes[..4] != b"MThd" {
        return Err(bad("not a MIDI file"));
    }
    let header_len = be32(&bytes[4..8]) as usize;
    if header_len < 6 || 8 + header_len > bytes.len() {
        return Err(bad("broken MIDI header"));
    }
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    if division == 0 || division & 0x8000 != 0 {
        return Err(bad("the MIDI file is not in ticks per quarter"));
    }
    let resolution = u32::from(division);

    let mut song = TickSong {
        resolution,
        tempos: Vec::new(),
        time_signatures: Vec::new(),
        name: None,
        artist: None,
        offset_s: 0.0,
        tracks: Vec::new(),
        sustain_cutoff: u64::from(resolution) / 3,
    };
    let mut at = 8 + header_len;
    let mut tracks_read = 0;
    let mut guitar: Option<Vec<Event>> = None;
    while at + 8 <= bytes.len() && tracks_read < MAX_TRACKS {
        let kind = &bytes[at..at + 4];
        let len = be32(&bytes[at + 4..at + 8]) as usize;
        let body_start = at + 8;
        let body_end = body_start.saturating_add(len).min(bytes.len());
        at = body_end;
        if kind != b"MTrk" {
            continue;
        }
        tracks_read += 1;
        let events = read_track(&bytes[body_start..body_end]);
        let is_guitar = events.iter().any(|e| {
            matches!(e, Event::Text { text, name: true, .. } if GUITAR_TRACKS.contains(&text.trim()))
        });
        for event in &events {
            match *event {
                Event::Tempo { tick, us } if song.tempos.len() < MAX_MAP_EVENTS => {
                    song.tempos.push((tick, us));
                }
                Event::Signature { tick, num, den }
                    if song.time_signatures.len() < MAX_MAP_EVENTS =>
                {
                    song.time_signatures.push((tick, num, den));
                }
                _ => {}
            }
        }
        if is_guitar && guitar.is_none() {
            guitar = Some(events);
        }
    }
    if tracks_read == 0 {
        return Err(bad("the MIDI file has no tracks"));
    }
    if let Some(events) = guitar {
        song.tracks = guitar_tracks(&events);
    }
    Ok(song)
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// A variable-length quantity at `*at`, advancing past it; `None` when
/// it runs off the end or past four bytes.
fn varlen(data: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for _ in 0..4 {
        let byte = *data.get(*at)?;
        *at += 1;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Every event of a track the chart cares about, notes paired into
/// lengths. Stops quietly at the first byte it cannot read.
fn read_track(data: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    // Open notes: key → start tick.
    let mut sounding: BTreeMap<u8, u64> = BTreeMap::new();
    let close = |events: &mut Vec<Event>, sounding: &mut BTreeMap<u8, u64>, key, tick| {
        if let Some(start) = sounding.remove(&key) {
            events.push(Event::Note {
                tick: start,
                len: tick - start,
                key,
            });
        }
    };
    let mut at = 0;
    let mut tick = 0u64;
    let mut running: Option<u8> = None;
    let mut count = 0;
    while at < data.len() && count < MAX_EVENTS {
        count += 1;
        let Some(delta) = varlen(data, &mut at) else {
            break;
        };
        tick = tick.saturating_add(delta);
        let Some(&first) = data.get(at) else {
            break;
        };
        let status = if first & 0x80 != 0 {
            at += 1;
            first
        } else if let Some(status) = running {
            status
        } else {
            break;
        };
        match status {
            0xFF => {
                running = None;
                let Some(&meta) = data.get(at) else { break };
                at += 1;
                let Some(len) = varlen(data, &mut at) else {
                    break;
                };
                let Some(body) = data.get(at..at.saturating_add(len as usize)) else {
                    break;
                };
                at += len as usize;
                match meta {
                    0x01 | 0x03 => events.push(Event::Text {
                        tick,
                        text: String::from_utf8_lossy(body).into_owned(),
                        name: meta == 0x03,
                    }),
                    0x51 if body.len() == 3 => {
                        let us = u32::from_be_bytes([0, body[0], body[1], body[2]]);
                        events.push(Event::Tempo {
                            tick,
                            us: f64::from(us),
                        });
                    }
                    0x58 if body.len() >= 2 => events.push(Event::Signature {
                        tick,
                        num: u32::from(body[0]),
                        den: 1 << body[1].min(6),
                    }),
                    0x2F => break,
                    _ => {}
                }
            }
            0xF0 | 0xF7 => {
                running = None;
                let Some(len) = varlen(data, &mut at) else {
                    break;
                };
                let Some(body) = data.get(at..at.saturating_add(len as usize)) else {
                    break;
                };
                at += len as usize;
                if let [0x50, 0x53, 0x00, 0x00, diff, kind, on, ..] = *body {
                    events.push(Event::Vendor {
                        tick,
                        diff,
                        kind,
                        on: on != 0,
                    });
                }
            }
            0x80..=0xEF => {
                running = Some(status);
                let data_bytes = if matches!(status & 0xF0, 0xC0 | 0xD0) {
                    1
                } else {
                    2
                };
                let Some(args) = data.get(at..at + data_bytes) else {
                    break;
                };
                at += data_bytes;
                let key = args[0] & 0x7f;
                match status & 0xF0 {
                    0x90 if args[1] > 0 => {
                        // A retrigger ends the note that was sounding.
                        close(&mut events, &mut sounding, key, tick);
                        sounding.insert(key, tick);
                    }
                    0x80 | 0x90 => close(&mut events, &mut sounding, key, tick),
                    _ => {}
                }
            }
            _ => break,
        }
    }
    // A note never released lasts no time.
    for (key, start) in sounding {
        events.push(Event::Note {
            tick: start,
            len: 0,
            key,
        });
    }
    events
}

/// Whether `tick` falls inside any `[start, end)` of `ranges`.
fn inside(ranges: &[(u64, u64)], tick: u64) -> bool {
    ranges
        .iter()
        .any(|&(s, e)| tick >= s && tick < e.max(s + 1))
}

/// The vendor sysex `kind` ranges that apply to `diff_index`.
fn vendor_ranges(events: &[Event], diff_index: u8, kind: u8) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut open_at: Option<u64> = None;
    for event in events {
        if let Event::Vendor {
            tick,
            diff,
            kind: k,
            on,
        } = *event
            && k == kind
            && (diff == diff_index || diff == 0xFF)
        {
            match (on, open_at) {
                (true, None) => open_at = Some(tick),
                (false, Some(start)) => {
                    out.push((start, tick));
                    open_at = None;
                }
                _ => {}
            }
        }
    }
    if let Some(start) = open_at {
        out.push((start, u64::MAX));
    }
    out
}

fn guitar_tracks(events: &[Event]) -> Vec<TickTrack> {
    let enhanced_opens = events
        .iter()
        .any(|e| matches!(e, Event::Text { text, .. } if text.trim() == "[ENHANCED_OPENS]"));
    let notes: Vec<(u64, u64, u8)> = events
        .iter()
        .filter_map(|e| match *e {
            Event::Note { tick, len, key } => Some((tick, len, key)),
            _ => None,
        })
        .collect();
    let ranges = |key: u8| -> Vec<(u64, u64)> {
        notes
            .iter()
            .filter(|n| n.2 == key)
            .map(|&(t, l, _)| (t, t + l))
            .collect()
    };
    let star_power = ranges(STAR_POWER);
    let taps_all = ranges(TAP);
    let mut tracks = Vec::new();
    for (index, (difficulty, base)) in BASES.into_iter().enumerate() {
        let diff_index = index as u8;
        let force_hopo = ranges(base + 5);
        let force_strum = ranges(base + 6);
        let opens = vendor_ranges(events, diff_index, 1);
        let mut taps = vendor_ranges(events, diff_index, 4);
        taps.extend(taps_all.iter().copied());
        let mut chords: BTreeMap<u64, TickNote> = BTreeMap::new();
        for &(tick, len, key) in &notes {
            let open_key = enhanced_opens && key + 1 == base;
            if !(base..base + 5).contains(&key) && !open_key {
                continue;
            }
            if chords.len() >= crate::MAX_NOTES_PER_CHART && !chords.contains_key(&tick) {
                continue;
            }
            let chord = chords.entry(tick).or_insert_with(|| TickNote {
                tick,
                ..TickNote::default()
            });
            if open_key {
                chord.open = true;
            } else {
                let fret = key - base;
                chord.frets |= 1 << fret;
                chord.sustain[usize::from(fret)] = len;
            }
        }
        if chords.is_empty() {
            continue;
        }
        let notes: Vec<TickNote> = chords
            .into_values()
            .map(|mut chord| {
                // A green under an open marker is the open note.
                if inside(&opens, chord.tick) && chord.frets & 1 != 0 {
                    chord.frets &= !1;
                    chord.open = true;
                }
                chord.tap = inside(&taps, chord.tick);
                chord.force = if inside(&force_hopo, chord.tick) {
                    Force::Hopo
                } else if inside(&force_strum, chord.tick) {
                    Force::Strum
                } else {
                    Force::Natural
                };
                chord
            })
            .collect();
        tracks.push(TickTrack {
            difficulty,
            notes,
            star_power: star_power.iter().map(|&(s, e)| (s, e - s)).collect(),
        });
    }
    tracks
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;

    /// A tiny MIDI writer for the tests: one event list per track.
    pub(crate) struct Smf {
        pub division: u16,
        pub tracks: Vec<Vec<(u64, Vec<u8>)>>,
    }

    fn put_varlen(out: &mut Vec<u8>, mut value: u64) {
        let mut stack = vec![(value & 0x7f) as u8];
        value >>= 7;
        while value > 0 {
            stack.push((value & 0x7f) as u8 | 0x80);
            value >>= 7;
        }
        stack.reverse();
        out.extend(stack);
    }

    impl Smf {
        pub(crate) fn bytes(&self) -> Vec<u8> {
            let mut out = b"MThd".to_vec();
            out.extend(6u32.to_be_bytes());
            out.extend(1u16.to_be_bytes());
            out.extend((self.tracks.len() as u16).to_be_bytes());
            out.extend(self.division.to_be_bytes());
            for track in &self.tracks {
                let mut sorted = track.clone();
                sorted.sort_by_key(|(t, _)| *t);
                let mut body = Vec::new();
                let mut last = 0;
                for (tick, event) in sorted {
                    put_varlen(&mut body, tick - last);
                    last = tick;
                    body.extend(event);
                }
                body.extend([0, 0xFF, 0x2F, 0]);
                out.extend(b"MTrk");
                out.extend((body.len() as u32).to_be_bytes());
                out.extend(body);
            }
            out
        }
    }

    pub(crate) fn meta(kind: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, kind, data.len() as u8];
        out.extend(data);
        out
    }

    pub(crate) fn note(tick: u64, len: u64, key: u8) -> [(u64, Vec<u8>); 2] {
        [
            (tick, vec![0x90, key, 100]),
            // Note-off as a zero-velocity note-on, the common spelling.
            (tick + len, vec![0x90, key, 0]),
        ]
    }

    fn tempo_track() -> Vec<(u64, Vec<u8>)> {
        vec![
            (0, meta(0x03, b"tempo")),
            (0, meta(0x51, &[0x07, 0xA1, 0x20])), // 500 000 µs = 120 BPM
            (0, meta(0x58, &[4, 2, 24, 8])),
        ]
    }

    #[test]
    fn the_guitar_track_becomes_four_difficulties_of_chords() {
        let mut guitar = vec![(0, meta(0x03, b"PART GUITAR"))];
        guitar.extend(note(0, 240, 96)); // expert green, sustained
        guitar.extend(note(0, 240, 100)); // expert orange: chord
        guitar.extend(note(480, 60, 98)); // expert yellow
        guitar.extend(note(480, 60, 101)); // force HOPO on it
        guitar.extend(note(0, 30, 60)); // easy green
        guitar.extend(note(0, 960, 116)); // star power
        let bytes = Smf {
            division: 480,
            tracks: vec![tempo_track(), guitar],
        }
        .bytes();
        let song = parse_midi(&bytes).unwrap();
        assert_eq!(song.resolution, 480);
        assert_eq!(song.tempos, vec![(0, 500_000.0)]);
        assert_eq!(song.time_signatures, vec![(0, 4, 4)]);
        let expert = song
            .tracks
            .iter()
            .find(|t| t.difficulty == Difficulty::Expert)
            .unwrap();
        assert_eq!(expert.notes.len(), 2);
        assert_eq!(expert.notes[0].frets, 0b10001);
        assert_eq!(expert.notes[0].sustain[0], 240);
        assert_eq!(expert.notes[1].force, Force::Hopo);
        assert_eq!(expert.star_power, vec![(0, 960)]);
        assert!(song.tracks.iter().any(|t| t.difficulty == Difficulty::Easy));
    }

    #[test]
    fn running_status_and_note_off_messages_are_understood() {
        let guitar = vec![
            (0, meta(0x03, b"PART GUITAR")),
            (0, vec![0x90, 96, 100]),
            (10, vec![97, 100]), // running status: another note-on
            (100, vec![0x80, 96, 0]),
            (110, vec![0x80, 97, 0]),
        ];
        let song = parse_midi(
            &Smf {
                division: 192,
                tracks: vec![guitar],
            }
            .bytes(),
        )
        .unwrap();
        let notes = &song.tracks[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].frets, notes[0].sustain[0]), (1, 100));
        assert_eq!((notes[1].frets, notes[1].sustain[1]), (2, 100));
    }

    #[test]
    fn opens_come_from_enhanced_opens_or_the_vendor_sysex() {
        let mut guitar = vec![
            (0, meta(0x03, b"PART GUITAR")),
            (0, meta(0x01, b"[ENHANCED_OPENS]")),
        ];
        guitar.extend(note(0, 10, 95)); // expert open
        guitar.extend(note(480, 10, 96)); // expert green …
        // … turned open by the sysex for Expert (3), and a tap at 960.
        guitar.push((470, vec![0xF0, 8, 0x50, 0x53, 0, 0, 3, 1, 1, 0xF7]));
        guitar.push((490, vec![0xF0, 8, 0x50, 0x53, 0, 0, 3, 1, 0, 0xF7]));
        guitar.extend(note(960, 10, 97));
        guitar.extend(note(950, 30, 104));
        let song = parse_midi(
            &Smf {
                division: 480,
                tracks: vec![guitar],
            }
            .bytes(),
        )
        .unwrap();
        let expert = &song.tracks[0];
        assert!(expert.notes[0].open && expert.notes[0].frets == 0);
        assert!(expert.notes[1].open && expert.notes[1].frets == 0);
        assert!(expert.notes[2].tap);
    }

    #[test]
    fn garbage_is_refused_or_ends_a_track_without_panicking() {
        assert!(parse_midi(b"RIFF").is_err());
        let mut smpte = Smf {
            division: 480,
            tracks: vec![vec![]],
        }
        .bytes();
        smpte[12] = 0xE7;
        assert!(parse_midi(&smpte).is_err());
        // A truncated track: whatever came before the cut survives.
        let mut guitar = vec![(0, meta(0x03, b"PART GUITAR"))];
        guitar.extend(note(0, 10, 96));
        let mut bytes = Smf {
            division: 480,
            tracks: vec![guitar],
        }
        .bytes();
        bytes.truncate(bytes.len() - 6);
        let song = parse_midi(&bytes).unwrap();
        assert_eq!(song.tracks[0].notes.len(), 1);
        // Every prefix of a valid file parses or errors, never panics.
        let whole = Smf {
            division: 480,
            tracks: vec![tempo_track()],
        }
        .bytes();
        for cut in 0..whole.len() {
            let _ = parse_midi(&whole[..cut]);
        }
    }

    #[test]
    fn a_file_without_a_guitar_track_has_no_tracks() {
        let song = parse_midi(
            &Smf {
                division: 480,
                tracks: vec![tempo_track()],
            }
            .bytes(),
        )
        .unwrap();
        assert!(song.tracks.is_empty());
        assert_eq!(
            super::super::assemble(&song, &super::super::SongIni::default(), "a.m4a").unwrap_err(),
            BridgeError::NoGuitar
        );
    }
}
