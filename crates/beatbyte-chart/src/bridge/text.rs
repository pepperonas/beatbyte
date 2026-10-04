//! `notes.chart`: the text chart format.
//!
//! Sections in brackets, each a brace block of `tick = TYPE values`
//! lines:
//!
//! ```text
//! [Song]        { Resolution = 192  Offset = 0  Name = "…" … }
//! [SyncTrack]   { 0 = TS 4   0 = B 120000 }   (B: BPM × 1000)
//! [ExpertSingle]{ 768 = N 0 0   768 = N 5 0   960 = S 2 384 }
//! ```
//!
//! In a difficulty section `N 0`–`N 4` are the frets with a sustain in
//! ticks, `N 5` forces (flips) the HOPO state of everything on that
//! tick, `N 6` makes it a tap and `N 7` is an open note; `S 2` is star
//! power. Only the five-fret lead guitar (`…Single`) is read.
//!
//! Untrusted input: line and note counts are bounded, malformed lines
//! are skipped, and a file without a readable resolution is refused.

use std::collections::BTreeMap;

use beatbyte_core::Difficulty;

use super::{BridgeError, Force, TickNote, TickSong, TickTrack};

/// Longest file read, in bytes — the largest real charts are a few MB.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

/// The sections that hold the lead guitar, per difficulty.
const SECTIONS: [(&str, Difficulty); 4] = [
    ("EasySingle", Difficulty::Easy),
    ("MediumSingle", Difficulty::Medium),
    ("HardSingle", Difficulty::Hard),
    ("ExpertSingle", Difficulty::Expert),
];

/// Parse a `notes.chart`. Pure — tested.
///
/// # Errors
/// When the text is too large or names no usable resolution.
pub fn parse_chart(text: &str) -> Result<TickSong, BridgeError> {
    if text.len() > MAX_BYTES {
        return Err(BridgeError::Parse("the chart file is too large".into()));
    }
    let mut song = TickSong {
        resolution: 0,
        tempos: Vec::new(),
        time_signatures: Vec::new(),
        name: None,
        artist: None,
        offset_s: 0.0,
        tracks: Vec::new(),
        sustain_cutoff: 0,
    };
    let mut section = String::new();
    // Per difficulty: chords by tick, and star power.
    let mut chords: BTreeMap<Difficulty, BTreeMap<u64, TickNote>> = BTreeMap::new();
    let mut star: BTreeMap<Difficulty, Vec<(u64, u64)>> = BTreeMap::new();

    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            name.trim().clone_into(&mut section);
            continue;
        }
        if line == "{" || line == "}" || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match section.as_str() {
            "Song" => read_song_line(&mut song, key, value),
            "SyncTrack" => read_sync_line(&mut song, key, value),
            other => {
                let Some(&(_, difficulty)) = SECTIONS.iter().find(|(name, _)| *name == other)
                else {
                    continue;
                };
                let Ok(tick) = key.parse::<u64>() else {
                    continue;
                };
                let mut parts = value.split_whitespace();
                let (Some(kind), Some(a)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let (Ok(a), b) = (
                    a.parse::<u64>(),
                    parts
                        .next()
                        .and_then(|b| b.parse::<u64>().ok())
                        .unwrap_or(0),
                ) else {
                    continue;
                };
                match kind {
                    "N" => {
                        let track = chords.entry(difficulty).or_default();
                        if track.len() >= crate::MAX_NOTES_PER_CHART && !track.contains_key(&tick) {
                            continue;
                        }
                        let chord = track.entry(tick).or_insert_with(|| TickNote {
                            tick,
                            ..TickNote::default()
                        });
                        match a {
                            0..=4 => {
                                chord.frets |= 1 << a;
                                chord.sustain[a as usize] = b;
                            }
                            5 => chord.force = Force::Flip,
                            6 => chord.tap = true,
                            7 => chord.open = true,
                            _ => {}
                        }
                    }
                    "S" if a == 2 => {
                        let list = star.entry(difficulty).or_default();
                        if list.len() < 10_000 {
                            list.push((tick, b));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if song.resolution == 0 {
        return Err(BridgeError::Parse("no resolution in [Song]".into()));
    }
    for (difficulty, notes) in chords {
        // A tick that carried only a marker is not a note.
        let notes: Vec<TickNote> = notes
            .into_values()
            .filter(|n| n.frets != 0 || n.open)
            .collect();
        song.tracks.push(TickTrack {
            difficulty,
            notes,
            star_power: star.remove(&difficulty).unwrap_or_default(),
        });
    }
    Ok(song)
}

fn unquote(value: &str) -> String {
    super::ini::strip_tags(value.trim_matches('"'))
}

fn read_song_line(song: &mut TickSong, key: &str, value: &str) {
    match key {
        "Resolution" => {
            song.resolution = value
                .parse::<u32>()
                .ok()
                .filter(|r| (1..=100_000).contains(r))
                .unwrap_or(0);
        }
        "Offset" => {
            song.offset_s = value
                .parse::<f64>()
                .ok()
                .filter(|o| o.is_finite() && o.abs() <= 60.0)
                .unwrap_or(0.0);
        }
        "Name" => song.name = Some(unquote(value)).filter(|n| !n.is_empty()),
        "Artist" => song.artist = Some(unquote(value)).filter(|a| !a.is_empty()),
        _ => {}
    }
}

fn read_sync_line(song: &mut TickSong, key: &str, value: &str) {
    let Ok(tick) = key.parse::<u64>() else {
        return;
    };
    let mut parts = value.split_whitespace();
    match (
        parts.next(),
        parts.next().and_then(|v| v.parse::<u64>().ok()),
    ) {
        (Some("B"), Some(milli_bpm)) if milli_bpm > 0 && song.tempos.len() < 100_000 => {
            song.tempos
                .push((tick, 60_000_000_000.0 / milli_bpm as f64));
        }
        (Some("TS"), Some(numerator)) if song.time_signatures.len() < 100_000 => {
            // The denominator is a power of two, written as its exponent.
            let exponent = parts
                .next()
                .and_then(|e| e.parse::<u32>().ok())
                .unwrap_or(2)
                .min(6);
            song.time_signatures
                .push((tick, numerator.min(64) as u32, 1 << exponent));
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CHART: &str = "\u{feff}[Song]\n{\n  Name = \"Fire\"\n  Artist = \"Band\"\n  \
        Offset = 0\n  Resolution = 192\n}\n[SyncTrack]\n{\n  0 = TS 4\n  0 = B 120000\n  \
        768 = TS 6 3\n  768 = B 60000\n}\n[Events]\n{\n  0 = E \"section Intro\"\n}\n\
        [ExpertSingle]\n{\n  0 = N 0 0\n  0 = N 4 96\n  192 = N 2 0\n  192 = N 5 0\n  \
        384 = N 7 0\n  576 = N 1 0\n  576 = N 6 0\n  768 = S 2 192\n  960 = N 5 0\n}\n\
        [EasySingle]\n{\n  0 = N 0 0\n}\n[ExpertDoubleBass]\n{\n  0 = N 3 0\n}\n";

    #[test]
    fn the_song_and_sync_sections_are_read() {
        let song = parse_chart(CHART).unwrap();
        assert_eq!(song.resolution, 192);
        assert_eq!(song.name.as_deref(), Some("Fire"));
        assert_eq!(song.artist.as_deref(), Some("Band"));
        assert_eq!(song.tempos, vec![(0, 500_000.0), (768, 1_000_000.0)]);
        assert_eq!(song.time_signatures, vec![(0, 4, 4), (768, 6, 8)]);
    }

    #[test]
    fn notes_group_into_chords_with_their_markers() {
        let song = parse_chart(CHART).unwrap();
        let expert = song
            .tracks
            .iter()
            .find(|t| t.difficulty == Difficulty::Expert)
            .unwrap();
        // The marker-only tick 960 is not a note.
        assert_eq!(expert.notes.len(), 4);
        let chord = expert.notes[0];
        assert_eq!(chord.frets, 0b10001);
        assert_eq!(chord.sustain, [0, 0, 0, 0, 96]);
        assert_eq!(expert.notes[1].force, Force::Flip);
        assert!(expert.notes[2].open);
        assert!(expert.notes[3].tap);
        assert_eq!(expert.star_power, vec![(768, 192)]);
    }

    #[test]
    fn only_the_lead_guitar_is_read() {
        let song = parse_chart(CHART).unwrap();
        let mut difficulties: Vec<Difficulty> = song.tracks.iter().map(|t| t.difficulty).collect();
        difficulties.sort();
        assert_eq!(difficulties, vec![Difficulty::Easy, Difficulty::Expert]);
    }

    #[test]
    fn a_file_without_resolution_is_refused_and_garbage_is_skipped() {
        assert!(parse_chart("[Song]\n{\n}\n").is_err());
        let song = parse_chart(
            "[Song]\n{\nResolution = 480\n}\n[ExpertSingle]\n{\nx = N 0 0\n5 = N\n\
             10 = N 0 0\n20 = Q 9 9\n}\n",
        )
        .unwrap();
        assert_eq!(song.tracks[0].notes.len(), 1);
    }

    #[test]
    fn the_parsed_chart_assembles_into_a_valid_beatbyte_chart() {
        let song = parse_chart(CHART).unwrap();
        let (chart, report) =
            super::super::assemble(&song, &super::super::SongIni::default(), "s.m4a").unwrap();
        assert_eq!(report.open_notes_dropped, 1);
        assert_eq!(report.taps_as_hopo, 1);
        let expert = chart.chart_for(Difficulty::Expert).unwrap();
        // Chord (2 notes), the flipped single, the tap.
        assert_eq!(expert.notes.len(), 4);
        // 192 is a whole beat after the chord — naturally strummed —
        // and its N 5 flips it to a HOPO.
        assert!(expert.notes[2].hopo);
        assert!(expert.notes[3].hopo, "a tap plays as a HOPO");
        assert!(
            chart
                .validate()
                .iter()
                .all(|i| i.severity != crate::Severity::Error)
        );
    }
}
