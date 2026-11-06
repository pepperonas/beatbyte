//! Charts from the community library that the Bridge downloader
//! fetches (ADR-0022): a folder with `notes.chart` or `notes.mid`,
//! a `song.ini`, and the audio.
//!
//! Both chart formats count in ticks against a tempo map and describe
//! a five-fret guitar on four difficulties, so both parse into one
//! intermediate form ([`TickSong`]) and one assembly turns that into a
//! BeatByte [`ChartFile`]:
//!
//! - frets 0–4 are lanes 0–4; notes on one tick are a chord;
//! - a HOPO is decided the way those charts expect it — a single note
//!   close enough to a DIFFERENT previous note is one naturally, and a
//!   forcing marker flips (`.chart`) or sets (`.mid`) that;
//! - a tap note plays as a HOPO, the closest thing BeatByte has;
//! - an open (no-fret) note has no lane here and is left out, counted
//!   in the [`Report`];
//! - star power becomes a Hype phrase;
//! - the tempo map becomes the chart's exact beat grid.
//!
//! Untrusted input throughout: the parsers bound every count and
//! skip what they cannot read, and the assembled chart still goes
//! through [`ChartFile::validate`] before anything writes it. Pure —
//! no I/O; the folder, the audio and the twin live in
//! `beatbyte-library::bridge`.

pub mod ini;
pub mod midi;
pub mod tempo;
pub mod text;

use beatbyte_core::Difficulty;

use crate::grid::BeatGrid;
use crate::schema::{ChartDef, ChartFile, ChartNote, ChartPhrase, SongMeta};
use crate::{FORMAT_VERSION, MAX_NOTES_PER_CHART, MAX_SONG_LENGTH_S};
pub use ini::{SongIni, parse_ini};
use tempo::TempoMap;

/// How a forcing marker changes a note's natural HOPO state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Force {
    /// No marker: the note is what its spacing makes it.
    #[default]
    Natural,
    /// `.chart`'s forced flag: the opposite of what it would be.
    Flip,
    /// `.mid`'s force-HOPO range.
    Hopo,
    /// `.mid`'s force-strum range.
    Strum,
}

/// One chord (or single note) in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickNote {
    /// Where it sits.
    pub tick: u64,
    /// Which frets, bit 0 = green … bit 4 = orange.
    pub frets: u8,
    /// Whether it is an open (no-fret) note.
    pub open: bool,
    /// Each fret's sustain in ticks (index = fret).
    pub sustain: [u64; 5],
    /// Its forcing marker.
    pub force: Force,
    /// Whether it is a tap note.
    pub tap: bool,
}

/// One difficulty in ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickTrack {
    /// Which difficulty.
    pub difficulty: Difficulty,
    /// Its chords, in any order.
    pub notes: Vec<TickNote>,
    /// Star power as `(start, length)` in ticks.
    pub star_power: Vec<(u64, u64)>,
}

/// A whole chart in ticks, as a parser leaves it.
#[derive(Debug, Clone, PartialEq)]
pub struct TickSong {
    /// Ticks per quarter note.
    pub resolution: u32,
    /// Tempo changes `(tick, microseconds per quarter)`.
    pub tempos: Vec<(u64, f64)>,
    /// Time signatures `(tick, numerator, denominator)`.
    pub time_signatures: Vec<(u64, u32, u32)>,
    /// The title the chart file names, if any.
    pub name: Option<String>,
    /// The artist the chart file names, if any.
    pub artist: Option<String>,
    /// The chart file's own audio offset in seconds.
    pub offset_s: f64,
    /// The guitar difficulties found.
    pub tracks: Vec<TickTrack>,
    /// Sustains up to this many ticks are plain notes, unless
    /// `song.ini` says otherwise (the format's own convention).
    pub sustain_cutoff: u64,
}

/// What a conversion did that the player should hear about.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    /// Notes per difficulty in the result.
    pub notes: Vec<(Difficulty, usize)>,
    /// Open notes left out (BeatByte has no open lane).
    pub open_notes_dropped: usize,
    /// Notes left out because they fell before the song's start.
    pub early_notes_dropped: usize,
    /// Tap notes played as HOPOs.
    pub taps_as_hopo: usize,
    /// Levels the download did not have, derived from the one above
    /// it by the classic ladder. Includes the [`copied`](Self::copied)
    /// ones.
    pub derived: Vec<Difficulty>,
    /// Levels the download had only as a copy of the level above —
    /// note for note the same chart, so choosing them changed nothing —
    /// or as a stub of a few notes. They were thrown away and derived
    /// like missing ones.
    pub copied: Vec<Difficulty>,
}

/// Why a folder cannot be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// The chart file could not be read as its format.
    Parse(String),
    /// It parsed, but holds no five-fret guitar part.
    NoGuitar,
}

impl core::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BridgeError::Parse(reason) => write!(f, "cannot read the chart: {reason}"),
            BridgeError::NoGuitar => f.write_str("the chart has no five-fret guitar part"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// The natural-HOPO threshold in ticks: `song.ini` overrides, an
/// eighth note when it asks for that, else 65/192 of a beat — the
/// format's long-standing default. Pure — tested.
#[must_use]
pub fn hopo_threshold(resolution: u32, ini: &SongIni) -> u64 {
    if let Some(ticks) = ini.hopo_frequency {
        return u64::from(ticks);
    }
    if ini.eighthnote_hopo {
        return u64::from(resolution) / 2;
    }
    u64::from(resolution) * 65 / 192
}

/// Whether each chord of `notes` (sorted by tick) plays as a HOPO.
///
/// Naturally: a single note that follows the previous chord within
/// `threshold` ticks and does not share a fret with it (a repeated
/// note, or one taken out of the chord before, is strummed). A chord
/// is never one naturally. Then the marker: a flip inverts, a range
/// sets; a tap is always one. Pure — tested.
#[must_use]
pub fn hopos(notes: &[TickNote], threshold: u64) -> Vec<bool> {
    let mut out = Vec::with_capacity(notes.len());
    for (index, note) in notes.iter().enumerate() {
        let mask = |n: &TickNote| n.frets | if n.open { 1 << 5 } else { 0 };
        let single = mask(note).count_ones() == 1;
        let natural = index > 0 && single && {
            let previous = &notes[index - 1];
            note.tick - previous.tick <= threshold && mask(note) & mask(previous) == 0
        };
        let hopo = match note.force {
            Force::Natural => natural,
            Force::Flip => !natural,
            Force::Hopo => true,
            Force::Strum => false,
        };
        out.push(hopo || note.tap);
    }
    out
}

/// Assemble `song` into a BeatByte chart playing `audio`. Pure —
/// tested.
///
/// # Errors
/// [`BridgeError::NoGuitar`] when no difficulty has a single note.
pub fn assemble(
    song: &TickSong,
    ini: &SongIni,
    audio: &str,
) -> Result<(ChartFile, Report), BridgeError> {
    let map = TempoMap::new(song.resolution, &song.tempos);
    let shift = ini.delay_ms / 1000.0 + song.offset_s;
    let at = |tick: u64| map.seconds(tick) + shift;
    let threshold = hopo_threshold(song.resolution, ini);
    let cutoff = ini.sustain_cutoff.map_or(song.sustain_cutoff, u64::from);
    let mut report = Report::default();

    let mut charts = Vec::new();
    let mut last_tick = 0u64;
    for difficulty in Difficulty::ALL {
        let Some(track) = song.tracks.iter().find(|t| t.difficulty == difficulty) else {
            continue;
        };
        let mut chords = track.notes.clone();
        chords.sort_by_key(|n| n.tick);
        let flags = hopos(&chords, threshold);
        let mut notes: Vec<ChartNote> = Vec::new();
        for (chord, hopo) in chords.iter().zip(flags) {
            if chord.open && chord.frets == 0 {
                report.open_notes_dropped += 1;
                continue;
            }
            let time = at(chord.tick);
            if !(0.0..=MAX_SONG_LENGTH_S).contains(&time) {
                report.early_notes_dropped += 1;
                continue;
            }
            if chord.tap {
                report.taps_as_hopo += 1;
            }
            for lane in 0..5u8 {
                if chord.frets & (1 << lane) == 0 {
                    continue;
                }
                let ticks = chord.sustain[usize::from(lane)];
                // Saturating, and bounded in TIME: a sustain of 2^64
                // ticks must neither overflow nor stretch the song
                // (and its beat grid) to the end of the universe.
                let end = chord.tick.saturating_add(ticks);
                let len = if ticks > cutoff {
                    let len = (at(end) - time).clamp(0.0, crate::validate::MAX_SUSTAIN_S);
                    if time + len <= MAX_SONG_LENGTH_S {
                        last_tick = last_tick.max(end);
                    }
                    len
                } else {
                    0.0
                };
                notes.push(ChartNote {
                    time,
                    lane,
                    len,
                    hopo,
                });
            }
            last_tick = last_tick.max(chord.tick);
        }
        // One note per lane per millisecond (the validator's rule):
        // two files' worth of a chord at one tick collapse.
        notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.lane.cmp(&b.lane)));
        notes.dedup_by(|b, a| a.lane == b.lane && (a.time - b.time).abs() < 0.0005);
        notes.truncate(MAX_NOTES_PER_CHART);
        if notes.is_empty() {
            continue;
        }
        report.notes.push((difficulty, notes.len()));
        charts.push(ChartDef {
            difficulty,
            lanes: 5,
            notes,
            phrases: phrases(&track.star_power, &at),
        });
    }
    if charts.is_empty() {
        return Err(BridgeError::NoGuitar);
    }

    let end_tick = last_tick.saturating_add(u64::from(map.resolution()) * 8);
    let mut chart = ChartFile {
        format_version: FORMAT_VERSION,
        song: SongMeta {
            title: ini
                .name
                .clone()
                .or_else(|| song.name.clone())
                .unwrap_or_else(|| "Untitled".to_owned()),
            artist: ini
                .artist
                .clone()
                .or_else(|| song.artist.clone())
                .unwrap_or_else(|| "Unknown".to_owned()),
            audio: audio.to_owned(),
            bpm: playable_bpm(map.dominant_bpm(end_tick)),
            offset_s: shift.clamp(-60.0, 60.0),
            preview_start_s: ini
                .preview_start_ms
                .map(|ms| ms / 1000.0)
                .filter(|s| *s > 0.0 && *s <= MAX_SONG_LENGTH_S),
            duration_s: ini
                .song_length_ms
                .map(|ms| ms / 1000.0)
                .filter(|s| *s <= MAX_SONG_LENGTH_S),
            genre: ini
                .genre
                .clone()
                .filter(|g| !g.trim().is_empty() && g.len() <= 48),
        },
        charts,
        provenance: None,
        audio_trim: None,
        grid: Some(grid(&map, &song.time_signatures, end_tick, shift)),
        rules: None,
    };
    (report.derived, report.copied) = restore_levels(&mut chart);
    Ok((chart, report))
}

/// A level above with at least this many notes can have a stub
/// under it — below that, a sparse level may simply be a sparse song.
const STUB_BASE: usize = 100;

/// A level with fewer than 1/`STUB_RATIO` of the notes of the level
/// above is a stub. The ladder never thins that far (Easy keeps about
/// a fifth of Expert in the library).
const STUB_RATIO: usize = 10;

/// Levels below the top that are no level at all: a copy of the
/// nearest level above them present — note for note the same chart —
/// or a stub with under a tenth of its notes. Some charters fill the
/// levels they did not chart by pasting one track into all of them,
/// others leave a handful of notes there (59 of 8 630 library charts,
/// 2026-11-06): choosing such a level changes nothing, or leaves the
/// player with a silent song. Pure — tested.
pub fn copied_levels(chart: &ChartFile) -> Vec<Difficulty> {
    let order = [
        Difficulty::Expert,
        Difficulty::Hard,
        Difficulty::Medium,
        Difficulty::Easy,
    ];
    let mut above: Option<&ChartDef> = None;
    let mut copies = Vec::new();
    for level in order {
        let Some(this) = chart.chart_for(level) else {
            continue;
        };
        if let Some(upper) = above {
            let copy = !this.notes.is_empty() && this.notes == upper.notes;
            let stub =
                upper.notes.len() >= STUB_BASE && this.notes.len() * STUB_RATIO < upper.notes.len();
            if copy || stub {
                copies.push(level);
            }
        }
        above = Some(this);
    }
    copies
}

/// Replace every [copied level](copied_levels) by one derived from
/// the level above it, as if the download had left it out. A copy
/// whose derivation comes out empty is put back — a level the player
/// could choose before must not vanish. Returns the levels replaced.
/// Pure — tested.
pub fn relevel(chart: &mut ChartFile) -> Vec<Difficulty> {
    restore_levels(chart).1
}

/// Throw out the copied levels, then derive every level missing — the
/// copies and the ones the download never had — in one top-down pass,
/// so a copy is judged against what the download wrote, never against
/// a level derived a moment earlier. Returns (every level derived,
/// the copies among them).
fn restore_levels(chart: &mut ChartFile) -> (Vec<Difficulty>, Vec<Difficulty>) {
    let copies = copied_levels(chart);
    let mut removed = Vec::new();
    chart.charts.retain(|c| {
        if copies.contains(&c.difficulty) {
            removed.push(c.clone());
            false
        } else {
            true
        }
    });
    let derived = fill_missing_levels(chart);
    let mut replaced = Vec::new();
    for def in removed {
        if derived.contains(&def.difficulty) {
            replaced.push(def.difficulty);
        } else {
            chart.charts.push(def);
        }
    }
    chart.charts.sort_by_key(|c| c.difficulty);
    replaced.sort_by_key(|d| std::cmp::Reverse(*d));
    (derived, replaced)
}

/// Derive every level below the highest one present that the
/// download lacks, each from the one above it, by the classic ladder
/// (ADR-0020: levels by deletion only, never an invented note). A
/// chart with only Expert would otherwise refuse to start on Hard.
/// Returns what it derived. Pure — tested.
fn fill_missing_levels(chart: &mut ChartFile) -> Vec<Difficulty> {
    use crate::classic::{Beats, ladder};
    let beats = Beats::of(chart);
    let mut derived = Vec::new();
    for level in [Difficulty::Hard, Difficulty::Medium, Difficulty::Easy] {
        if chart.chart_for(level).is_some() {
            continue;
        }
        let find = |d: Difficulty| chart.charts.iter().find(|c| c.difficulty == d);
        let made = match level {
            Difficulty::Hard => find(Difficulty::Expert).map(|e| ladder::derive_hard(e, &beats)),
            Difficulty::Medium => match (find(Difficulty::Hard), find(Difficulty::Expert)) {
                (Some(hard), Some(expert)) => Some(ladder::derive_medium(hard, expert, &beats).0),
                (Some(hard), None) => Some(ladder::derive_medium(hard, hard, &beats).0),
                _ => None,
            },
            _ => find(Difficulty::Medium).map(|m| ladder::derive_easy(m, &beats)),
        };
        if let Some(def) = made.filter(|d| !d.notes.is_empty()) {
            chart.charts.push(def);
            derived.push(level);
        }
    }
    chart.charts.sort_by_key(|c| c.difficulty);
    derived
}

/// A tempo the chart format accepts: folded by octaves into 20–400.
fn playable_bpm(bpm: f64) -> f64 {
    let mut bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        120.0
    };
    while bpm > 400.0 {
        bpm /= 2.0;
    }
    while bpm < 20.0 {
        bpm *= 2.0;
    }
    bpm
}

/// Star power as Hype phrases: in seconds, sorted, and merged where
/// they touch or overlap (the validator refuses both).
fn phrases(star_power: &[(u64, u64)], at: &impl Fn(u64) -> f64) -> Vec<ChartPhrase> {
    let mut out: Vec<ChartPhrase> = star_power
        .iter()
        .map(|&(start, len)| ChartPhrase {
            start: at(start),
            end: at(start.saturating_add(len)),
        })
        .filter(|p| p.start >= 0.0 && p.end <= MAX_SONG_LENGTH_S && p.end >= p.start)
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut merged: Vec<ChartPhrase> = Vec::with_capacity(out.len());
    for phrase in out {
        match merged.last_mut() {
            Some(last) if phrase.start <= last.end => last.end = last.end.max(phrase.end),
            _ => merged.push(phrase),
        }
    }
    merged
}

/// The exact beat grid of the tempo map: a beat every quarter note up
/// to `end_tick`, the downbeats where the time signatures put bars.
fn grid(map: &TempoMap, signatures: &[(u64, u32, u32)], end_tick: u64, shift: f64) -> BeatGrid {
    use crate::grid::MAX_GRID_BEATS;
    let beat = u64::from(map.resolution());
    // Bounded twice: by count (untrusted input may ask for any number
    // of ticks) and by the song's longest legal length in time.
    let in_song = |t: f64| t <= MAX_SONG_LENGTH_S + 60.0;
    let beats: Vec<f64> = (0..=end_tick / beat)
        .take(MAX_GRID_BEATS)
        .map(|i| map.seconds(i.saturating_mul(beat)) + shift)
        .take_while(|t| in_song(*t))
        .filter(|t| *t >= 0.0)
        .collect();
    let mut signatures: Vec<(u64, u32, u32)> = signatures
        .iter()
        .copied()
        .filter(|(_, num, den)| (1..=64).contains(num) && den.is_power_of_two() && *den <= 64)
        .take(MAX_GRID_BEATS)
        .collect();
    signatures.sort_by_key(|s| s.0);
    if signatures.first().is_none_or(|s| s.0 != 0) {
        signatures.insert(0, (0, 4, 4));
    }
    let mut downbeats = Vec::new();
    'bars: for (index, &(start, num, den)) in signatures.iter().enumerate() {
        let until = signatures
            .get(index + 1)
            .map_or(end_tick, |s| s.0.min(end_tick));
        let bar = (u64::from(num) * beat * 4 / u64::from(den)).max(1);
        let mut tick = start;
        while tick < until {
            let at = map.seconds(tick) + shift;
            if downbeats.len() >= MAX_GRID_BEATS || !in_song(at) {
                break 'bars;
            }
            downbeats.push(at);
            tick = tick.saturating_add(bar);
        }
    }
    BeatGrid::from_beats(&beats).with_downbeats(&downbeats)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn note(tick: u64, frets: u8) -> TickNote {
        TickNote {
            tick,
            frets,
            ..TickNote::default()
        }
    }

    fn song(notes: Vec<TickNote>) -> TickSong {
        TickSong {
            resolution: 192,
            tempos: vec![(0, 500_000.0)],
            time_signatures: vec![(0, 4, 4)],
            name: Some("Song".into()),
            artist: Some("Band".into()),
            offset_s: 0.0,
            tracks: vec![TickTrack {
                difficulty: Difficulty::Expert,
                notes,
                star_power: vec![],
            }],
            sustain_cutoff: 0,
        }
    }

    #[test]
    fn the_default_threshold_is_65_192ths_of_a_beat() {
        let ini = SongIni::default();
        assert_eq!(hopo_threshold(192, &ini), 65);
        assert_eq!(hopo_threshold(480, &ini), 162);
        let eighth = SongIni {
            eighthnote_hopo: true,
            ..SongIni::default()
        };
        assert_eq!(hopo_threshold(480, &eighth), 240);
        let set = SongIni {
            hopo_frequency: Some(170),
            ..eighth
        };
        assert_eq!(hopo_threshold(480, &set), 170);
    }

    #[test]
    fn a_close_different_single_note_is_a_hopo_and_nothing_else_is() {
        let g = 1;
        let r = 2;
        let notes = [
            note(0, g),        // first: strummed
            note(48, r),       // close, different: HOPO
            note(96, r),       // close, same note: strummed
            note(144, g | r),  // chord: strummed
            note(192, g),      // close but inside the chord before: strummed
            note(400, r),      // far: strummed
            note(448, 1 << 2), // close, different: HOPO
        ];
        assert_eq!(
            hopos(&notes, 65),
            vec![false, true, false, false, false, false, true]
        );
    }

    #[test]
    fn markers_flip_set_and_taps_win() {
        let mut notes = [note(0, 1), note(48, 2), note(500, 4), note(600, 8)];
        notes[1].force = Force::Flip; // natural HOPO → strum
        notes[2].force = Force::Flip; // natural strum → HOPO
        notes[3].tap = true;
        assert_eq!(hopos(&notes, 65), vec![false, false, true, true]);
        notes[1].force = Force::Strum;
        notes[2].force = Force::Hopo;
        assert_eq!(hopos(&notes, 65), vec![false, false, true, true]);
    }

    #[test]
    fn assembly_turns_ticks_into_seconds_lanes_and_sustains() {
        let mut chord = note(384, 0b10001);
        chord.sustain = [192, 0, 0, 0, 96];
        let (chart, report) = assemble(
            &song(vec![note(192, 0b00100), chord]),
            &SongIni::default(),
            "song.m4a",
        )
        .unwrap();
        let notes = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        assert_eq!(notes.len(), 3);
        assert_eq!((notes[0].time, notes[0].lane), (0.5, 2));
        assert_eq!((notes[1].time, notes[1].lane, notes[1].len), (1.0, 0, 0.5));
        assert_eq!((notes[2].lane, notes[2].len), (4, 0.25));
        assert_eq!(chart.song.title, "Song");
        assert_eq!(chart.song.bpm, 120.0);
        assert_eq!(report.notes, vec![(Difficulty::Expert, 3)]);
        assert!(
            chart
                .validate()
                .iter()
                .all(|i| i.severity != crate::Severity::Error)
        );
    }

    #[test]
    fn open_notes_are_dropped_and_counted() {
        let mut open = note(0, 0);
        open.open = true;
        let (chart, report) =
            assemble(&song(vec![open, note(96, 1)]), &SongIni::default(), "a.m4a").unwrap();
        assert_eq!(chart.chart_for(Difficulty::Expert).unwrap().notes.len(), 1);
        assert_eq!(report.open_notes_dropped, 1);
    }

    #[test]
    fn a_short_sustain_under_the_cutoff_is_a_plain_note() {
        let mut short = note(0, 1);
        short.sustain = [40, 0, 0, 0, 0];
        let ini = SongIni {
            sustain_cutoff: Some(48),
            ..SongIni::default()
        };
        let (chart, _) = assemble(&song(vec![short]), &ini, "a.m4a").unwrap();
        assert_eq!(
            chart.chart_for(Difficulty::Expert).unwrap().notes[0].len,
            0.0
        );
    }

    #[test]
    fn delay_moves_every_note_and_the_grid_and_negative_ones_are_dropped() {
        let ini = SongIni {
            delay_ms: -600.0,
            ..SongIni::default()
        };
        let (chart, report) =
            assemble(&song(vec![note(0, 1), note(384, 1)]), &ini, "a.m4a").unwrap();
        assert_eq!(report.early_notes_dropped, 1);
        assert!((chart.chart_for(Difficulty::Expert).unwrap().notes[0].time - 0.4).abs() < 1e-9);
        let grid = chart.grid.unwrap();
        assert!(grid.beats.iter().all(|b| *b >= 0.0));
        assert!((grid.beats[0] - 0.4).abs() < 1e-9);
    }

    #[test]
    fn star_power_becomes_merged_hype_phrases() {
        let mut s = song(vec![note(0, 1), note(1000, 1)]);
        s.tracks[0].star_power = vec![(768, 192), (0, 192), (192, 96)];
        let (chart, _) = assemble(&s, &SongIni::default(), "a.m4a").unwrap();
        let phrases = &chart.chart_for(Difficulty::Expert).unwrap().phrases;
        assert_eq!(phrases.len(), 2, "{phrases:?}");
        assert_eq!((phrases[0].start, phrases[0].end), (0.0, 0.75));
        assert_eq!((phrases[1].start, phrases[1].end), (2.0, 2.5));
    }

    #[test]
    fn the_grid_follows_tempo_and_time_signature() {
        let mut s = song(vec![note(192 * 12, 1)]);
        s.tempos = vec![(0, 500_000.0), (192 * 4, 1_000_000.0)];
        s.time_signatures = vec![(0, 4, 4), (192 * 4, 3, 4)];
        let (chart, _) = assemble(&s, &SongIni::default(), "a.m4a").unwrap();
        let grid = chart.grid.unwrap();
        assert!((grid.beats[4] - 2.0).abs() < 1e-9);
        assert!((grid.beats[5] - 3.0).abs() < 1e-9);
        // Bars: 0, 2.0 (4/4 at 120), then every 3 beats at 60 BPM.
        assert_eq!(&grid.downbeats[..3], &[0.0, 2.0, 5.0]);
    }

    #[test]
    fn a_download_with_only_expert_gets_every_level_below_it() {
        // Eight bars of eighths across all five frets, with chords a
        // fret apart — a shape every level down to Medium keeps, so
        // only Easy's own rule can take them away.
        let notes: Vec<TickNote> = (0..64u64)
            .map(|i| note(i * 96, if i % 8 == 0 { 0b00101 } else { 1 << (i % 5) }))
            .collect();
        let (chart, report) = assemble(&song(notes), &SongIni::default(), "a.m4a").unwrap();
        assert_eq!(
            report.derived,
            vec![Difficulty::Hard, Difficulty::Medium, Difficulty::Easy]
        );
        let count = |d| chart.chart_for(d).map_or(0, |c| c.notes.len());
        assert!(count(Difficulty::Expert) > count(Difficulty::Hard));
        assert!(count(Difficulty::Hard) > count(Difficulty::Medium));
        assert!(count(Difficulty::Medium) > count(Difficulty::Easy));
        let medium = chart.chart_for(Difficulty::Medium).unwrap();
        let chords = |notes: &[ChartNote]| {
            notes
                .windows(2)
                .filter(|w| (w[0].time - w[1].time).abs() < 1e-9)
                .count()
        };
        assert!(
            chords(&medium.notes) > 0,
            "the fixture must reach Easy with chords"
        );
        let easy = chart.chart_for(Difficulty::Easy).unwrap();
        assert!(
            easy.notes.iter().all(|n| n.lane < 4),
            "no fifth lane on Easy"
        );
        let mut times: Vec<i64> = easy.notes.iter().map(|n| (n.time * 1e6) as i64).collect();
        let before = times.len();
        times.dedup();
        assert_eq!(times.len(), before, "no chords on Easy");
        // A level the download has is left alone.
        assert!(report.notes.iter().all(|(d, _)| *d == Difficulty::Expert));
        assert!(
            chart
                .validate()
                .iter()
                .all(|i| i.severity != crate::Severity::Error)
        );
    }

    /// The download the levels were pasted into: one track under
    /// every name. Each level below the top comes out derived and
    /// sparser than the one above — choosing a level means something
    /// again — and the conversion says so.
    #[test]
    fn levels_that_copy_the_one_above_are_derived_like_missing_ones() {
        let notes: Vec<TickNote> = (0..64u64)
            .map(|i| note(i * 96, if i % 8 == 0 { 0b00101 } else { 1 << (i % 5) }))
            .collect();
        let mut pasted = song(notes.clone());
        for difficulty in [Difficulty::Hard, Difficulty::Medium, Difficulty::Easy] {
            pasted.tracks.push(TickTrack {
                difficulty,
                notes: notes.clone(),
                star_power: vec![],
            });
        }
        let (chart, report) = assemble(&pasted, &SongIni::default(), "a.m4a").unwrap();
        let all_below = vec![Difficulty::Hard, Difficulty::Medium, Difficulty::Easy];
        assert_eq!(report.copied, all_below);
        assert_eq!(report.derived, all_below);
        let count = |d| chart.chart_for(d).map_or(0, |c| c.notes.len());
        assert!(count(Difficulty::Expert) > count(Difficulty::Hard));
        assert!(count(Difficulty::Hard) > count(Difficulty::Medium));
        assert!(count(Difficulty::Medium) > count(Difficulty::Easy));
        assert!(copied_levels(&chart).is_empty());
    }

    fn file_with(levels: &[(Difficulty, Vec<f64>)]) -> ChartFile {
        let (mut chart, _) =
            assemble(&song(vec![note(0, 1)]), &SongIni::default(), "a.m4a").unwrap();
        chart.charts = levels
            .iter()
            .map(|(difficulty, times)| ChartDef {
                difficulty: *difficulty,
                notes: times
                    .iter()
                    .enumerate()
                    .map(|(i, t)| ChartNote {
                        time: *t,
                        lane: (i % 5) as u8,
                        ..chart.charts[0].notes[0]
                    })
                    .collect(),
                ..chart.charts[0].clone()
            })
            .collect();
        chart
    }

    /// A level with a handful of notes under a charted one is a stub;
    /// a merely sparse level, or a sparse song, is not.
    #[test]
    fn a_stub_under_a_charted_level_counts_as_a_copy() {
        let full: Vec<f64> = (0..200).map(|i| 1.0 + f64::from(i) * 0.25).collect();
        let five: Vec<f64> = full[..5].to_vec();
        let chart = file_with(&[
            (Difficulty::Hard, five.clone()),
            (Difficulty::Expert, full.clone()),
        ]);
        assert_eq!(copied_levels(&chart), vec![Difficulty::Hard]);
        let fifth: Vec<f64> = full[..40].to_vec();
        let chart = file_with(&[
            (Difficulty::Easy, fifth),
            (Difficulty::Expert, full.clone()),
        ]);
        assert!(copied_levels(&chart).is_empty(), "a fifth is a level");
        let sparse: Vec<f64> = full[..60].to_vec();
        let chart = file_with(&[(Difficulty::Hard, five), (Difficulty::Expert, sparse)]);
        assert!(
            copied_levels(&chart).is_empty(),
            "too few notes above to judge"
        );
    }

    /// Only a level identical to the nearest one ABOVE it is a copy:
    /// a charted level stays, a gap in between is skipped over, and
    /// Expert is never one.
    #[test]
    fn a_copy_is_judged_against_the_nearest_level_above() {
        let a = vec![0.5, 1.0, 1.5, 2.0];
        let b = vec![0.5, 1.5];
        let chart = file_with(&[
            (Difficulty::Easy, a.clone()),
            (Difficulty::Medium, b.clone()),
            (Difficulty::Expert, b.clone()),
        ]);
        assert_eq!(copied_levels(&chart), vec![Difficulty::Medium]);
        let chart = file_with(&[(Difficulty::Hard, a.clone()), (Difficulty::Expert, b)]);
        assert!(copied_levels(&chart).is_empty());
        let chart = file_with(&[(Difficulty::Expert, a)]);
        assert!(copied_levels(&chart).is_empty());
    }

    /// A copy the ladder cannot derive anything from stays as it was:
    /// re-levelling may make a level different, never make it vanish.
    #[test]
    fn a_copy_that_derives_to_nothing_is_kept() {
        let mut chart = file_with(&[(Difficulty::Hard, vec![]), (Difficulty::Expert, vec![])]);
        assert!(relevel(&mut chart).is_empty());
        assert_eq!(chart.charts.len(), 2);
        let notes = vec![0.5, 1.0];
        let mut chart = file_with(&[
            (Difficulty::Medium, notes.clone()),
            (Difficulty::Hard, notes.clone()),
            (Difficulty::Expert, notes),
        ]);
        let replaced = relevel(&mut chart);
        for level in [Difficulty::Medium, Difficulty::Hard, Difficulty::Expert] {
            assert!(chart.chart_for(level).is_some_and(|c| !c.notes.is_empty()));
        }
        assert!(replaced.iter().all(|d| *d != Difficulty::Expert));
    }

    /// Untrusted input: ticks at the end of the integer range, a
    /// resolution of one and star power to the end of time must
    /// neither overflow nor produce a grid longer than the cap.
    #[test]
    fn absurd_ticks_neither_overflow_nor_grow_the_grid() {
        let mut s = song(vec![note(1, 1), {
            let mut far = note(u64::MAX - 10, 2);
            far.sustain = [0, u64::MAX, 0, 0, 0];
            far
        }]);
        s.resolution = 1;
        s.tracks[0].notes[0].sustain = [u64::MAX, 0, 0, 0, 0];
        s.tracks[0].star_power = vec![(u64::MAX - 1, u64::MAX)];
        s.time_signatures = vec![(0, 1, 64)];
        let (chart, _) = assemble(&s, &SongIni::default(), "a.m4a").unwrap();
        let grid = chart.grid.clone().unwrap();
        assert!(grid.beats.len() <= crate::grid::MAX_GRID_BEATS);
        assert!(grid.downbeats.len() <= crate::grid::MAX_GRID_BEATS);
        assert!(
            chart
                .validate()
                .iter()
                .all(|i| i.severity != crate::Severity::Error)
        );
    }

    #[test]
    fn a_song_without_guitar_is_refused() {
        let mut s = song(vec![]);
        s.tracks.clear();
        assert_eq!(
            assemble(&s, &SongIni::default(), "a.m4a").unwrap_err(),
            BridgeError::NoGuitar
        );
    }

    #[test]
    fn ini_metadata_wins_over_the_chart_files_own() {
        let ini = SongIni {
            name: Some("Ini Title".into()),
            artist: Some("Ini Artist".into()),
            preview_start_ms: Some(1500.0),
            ..SongIni::default()
        };
        let (chart, _) = assemble(&song(vec![note(0, 1)]), &ini, "a.m4a").unwrap();
        assert_eq!(chart.song.title, "Ini Title");
        assert_eq!(chart.song.artist, "Ini Artist");
        assert_eq!(chart.song.preview_start_s, Some(1.5));
    }

    #[test]
    fn tempos_outside_the_format_fold_by_octaves() {
        assert_eq!(playable_bpm(800.0), 400.0);
        assert_eq!(playable_bpm(10.0), 20.0);
        assert_eq!(playable_bpm(f64::NAN), 120.0);
    }
}
