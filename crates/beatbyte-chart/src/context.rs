//! What the analysis said about each note's moment in the song.
//!
//! # Why this file exists
//!
//! The generator reads a [`SongAnalysis`] — onsets, an energy
//! envelope, a beat grid, repeated spans — turns it into notes and
//! throws it away. Nothing else in BeatByte ever persists it. So the
//! question the telemetry store exists to answer
//! ([ADR-0018](https://github.com/pepperonas/beatbyte/blob/main/docs/decisions/ADR-0018-gameplay-telemetry-store.md)
//! §20) — *do the notes everybody misses have something in common
//! musically?* — could not be asked at all: the miss is recorded and
//! the moment it happened in is gone.
//!
//! A context sidecar keeps the few numbers that question needs, per
//! note, beside the chart.
//!
//! # Two rules it lives by
//!
//! **It indexes TRACK EVENTS, not chart notes.** `ChartFile::to_track`
//! merges simultaneous notes into chord events, and the telemetry
//! store records that index. A sidecar keyed the other way would join
//! to the wrong note on every chord.
//!
//! **It never touches `chart_hash`.** It is a separate file, so a
//! chart that gains one keeps its identity — and every session ever
//! recorded against that chart keeps its evidence. A field inside the
//! chart would have invalidated the lot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use beatbyte_core::{Difficulty, SongAnalysis};
use serde::{Deserialize, Serialize};

use crate::schema::ChartFile;

/// The sidecar's format tag.
pub const CONTEXT_FORMAT: &str = "beatbyte.context/1";

/// How close to an onset a note has to be to be *that* onset.
///
/// Half a sixteenth at 120 BPM. Wider and a note would inherit the
/// salience of a different hit; narrower and the generator's own
/// snapping would lose the onset it placed the note from.
const ONSET_WINDOW_S: f64 = 0.06;

/// How close to a beat a note has to be to count as on it.
const BEAT_WINDOW_S: f64 = 0.03;

/// Beats per bar, when nothing knows better.
const BEATS_PER_BAR: usize = 4;

/// The analysis at one note's moment, five bytes wide.
///
/// Everything is a `u8` on purpose. These are not measurements to be
/// reported, they are dimensions to group by — "did the misses
/// cluster in the loud passages" does not need more than a byte of
/// energy, and 170 songs' worth of `f32` would be megabytes of
/// precision nobody will ever use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoteContext {
    /// Onset salience, `0` (no onset near it) to `255` (the
    /// strongest onset in the song).
    pub onset: u8,
    /// The energy envelope there, `0`–`255`.
    pub energy: u8,
    /// Spectral brightness of the onset, `0` bassy to `255` bright.
    pub brightness: u8,
    /// Where in the bar it falls, in sixteenths (`0` = the downbeat),
    /// or `255` when no grid could say.
    pub bar_phase: u8,
    /// Which repeated span it is in: `0` for none, else the span's
    /// number.
    ///
    /// **Not a section name.** BeatByte has no structure
    /// segmentation, so nothing here knows the word "chorus"; what it
    /// does know is that some spans of a song occur twice. Calling
    /// that a chorus would be a guess written into a file.
    pub repeat: u8,
    /// [`ContextFlags`].
    pub flags: u8,
}

/// Bits of [`NoteContext::flags`].
pub struct ContextFlags;

impl ContextFlags {
    /// The note sits on a beat of the grid.
    pub const ON_BEAT: u8 = 1 << 0;
    /// …and that beat is a downbeat.
    pub const DOWNBEAT: u8 = 1 << 1;
    /// No onset was found near it: the generator placed it from the
    /// grid or the melody, not from something audible striking.
    pub const NO_ONSET: u8 = 1 << 2;
}

/// The wire form: six numbers, so a sidecar for a seventy-song
/// library is kilobytes rather than megabytes.
type Packed = [u8; 6];

impl From<NoteContext> for Packed {
    fn from(context: NoteContext) -> Packed {
        [
            context.onset,
            context.energy,
            context.brightness,
            context.bar_phase,
            context.repeat,
            context.flags,
        ]
    }
}

impl From<Packed> for NoteContext {
    fn from(packed: Packed) -> NoteContext {
        NoteContext {
            onset: packed[0],
            energy: packed[1],
            brightness: packed[2],
            bar_phase: packed[3],
            repeat: packed[4],
            flags: packed[5],
        }
    }
}

/// One chart's contexts, per difficulty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartContext {
    /// [`CONTEXT_FORMAT`].
    pub format: String,
    /// The chart these describe, by content hash. A sidecar whose
    /// hash does not match the chart beside it describes a chart that
    /// no longer exists, and is ignored rather than trusted.
    pub chart_hash: String,
    /// Difficulty id → one entry per **track event**.
    pub tracks: BTreeMap<String, Vec<Packed>>,
}

impl ChartContext {
    /// Whether this sidecar describes that chart.
    #[must_use]
    pub fn is_current_for(&self, chart: &ChartFile) -> bool {
        self.format == CONTEXT_FORMAT && self.chart_hash == crate::schema::chart_hash(chart)
    }

    /// One note's context.
    #[must_use]
    pub fn get(&self, difficulty: Difficulty, event_index: usize) -> Option<NoteContext> {
        self.tracks
            .get(difficulty.id())?
            .get(event_index)
            .copied()
            .map(NoteContext::from)
    }

    /// How many notes it describes in total.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.values().map(Vec::len).sum()
    }

    /// Whether it describes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Where a chart's sidecar lives: `chart.v3.json` → `chart.v3.context.json`.
#[must_use]
pub fn context_path(chart_path: &Path) -> PathBuf {
    let stem = chart_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = stem.strip_suffix(".json").unwrap_or(&stem);
    chart_path.with_file_name(format!("{base}.context.json"))
}

/// Build the sidecar for a chart from the analysis it was made from.
///
/// Deterministic and pure: the same chart and the same analysis give
/// the same bytes.
#[must_use]
pub fn context_for(chart: &ChartFile, analysis: &SongAnalysis) -> ChartContext {
    let mut tracks = BTreeMap::new();
    for definition in &chart.charts {
        let Ok(track) = chart.to_track(definition.difficulty) else {
            continue;
        };
        let packed: Vec<Packed> = track
            .events()
            .iter()
            .map(|event| Packed::from(context_at(analysis, event.time_s)))
            .collect();
        tracks.insert(definition.difficulty.id().to_owned(), packed);
    }
    ChartContext {
        format: CONTEXT_FORMAT.to_owned(),
        chart_hash: crate::schema::chart_hash(chart),
        tracks,
    }
}

/// The analysis at one moment.
#[must_use]
pub fn context_at(analysis: &SongAnalysis, time_s: f64) -> NoteContext {
    let mut flags = 0u8;

    // The nearest onset, if one is close enough to be this note's.
    let onset = nearest_onset(analysis, time_s);
    let (salience, brightness) = match onset {
        Some(onset) => (byte(onset.strength), byte(onset.brightness)),
        None => {
            flags |= ContextFlags::NO_ONSET;
            (0, 0)
        }
    };

    // The beat this note is nearest to, and how far off it is.
    let beat = nearest_index(&analysis.beats, time_s);
    if let Some(index) = beat
        && (analysis.beats[index] - time_s).abs() <= BEAT_WINDOW_S
    {
        flags |= ContextFlags::ON_BEAT;
        if is_downbeat(analysis, index) {
            flags |= ContextFlags::DOWNBEAT;
        }
    }

    NoteContext {
        onset: salience,
        energy: byte(analysis.energy_at(time_s)),
        brightness,
        bar_phase: bar_phase(analysis, time_s),
        repeat: repeat_of(analysis, beat),
        flags,
    }
}

/// A 0–1 float as a byte, saturating and never panicking on a NaN.
fn byte(value: f32) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The index of the value nearest `time_s` in an ascending list.
fn nearest_index(times: &[f64], time_s: f64) -> Option<usize> {
    if times.is_empty() {
        return None;
    }
    let after = times.partition_point(|value| *value < time_s);
    let before = after.checked_sub(1);
    match (before, times.get(after)) {
        (Some(before), Some(next)) => {
            if (time_s - times[before]).abs() <= (next - time_s).abs() {
                Some(before)
            } else {
                Some(after)
            }
        }
        (Some(before), None) => Some(before),
        (None, Some(_)) => Some(after),
        (None, None) => None,
    }
}

/// The onset this note came from, if any is close enough.
fn nearest_onset(analysis: &SongAnalysis, time_s: f64) -> Option<beatbyte_core::Onset> {
    let times: Vec<f64> = analysis.onsets.iter().map(|onset| onset.time_s).collect();
    let index = nearest_index(&times, time_s)?;
    let onset = analysis.onsets.get(index)?;
    ((onset.time_s - time_s).abs() <= ONSET_WINDOW_S).then_some(*onset)
}

/// Whether the beat at `index` starts a bar.
fn is_downbeat(analysis: &SongAnalysis, index: usize) -> bool {
    let Some(time) = analysis.beats.get(index) else {
        return false;
    };
    if analysis.downbeats.is_empty() {
        // Nothing knew where the bars are, so bars are counted from
        // the first beat — which is what every consumer of an
        // analysis without downbeats already does.
        return index.is_multiple_of(BEATS_PER_BAR);
    }
    analysis
        .downbeats
        .iter()
        .any(|downbeat| (downbeat - time).abs() <= BEAT_WINDOW_S)
}

/// Where in its bar a moment falls, in sixteenths.
fn bar_phase(analysis: &SongAnalysis, time_s: f64) -> u8 {
    if analysis.beats.len() < 2 {
        return 255;
    }
    let Some(index) = nearest_index(&analysis.beats, time_s) else {
        return 255;
    };
    // The beat this moment is IN, not the one it is nearest to.
    let beat = if analysis.beats[index] > time_s {
        index.saturating_sub(1)
    } else {
        index
    };
    let start = analysis.beats[beat];
    let next = analysis
        .beats
        .get(beat + 1)
        .copied()
        .unwrap_or(start + analysis.beat_interval_s());
    let span = (next - start).max(f64::EPSILON);
    let within = ((time_s - start) / span * 4.0).round().clamp(0.0, 3.0) as u8;
    let bar_beat = beats_since_downbeat(analysis, beat);
    bar_beat.saturating_mul(4).saturating_add(within)
}

/// How many beats into its bar a beat is.
fn beats_since_downbeat(analysis: &SongAnalysis, beat: usize) -> u8 {
    if analysis.downbeats.is_empty() {
        return u8::try_from(beat % BEATS_PER_BAR).unwrap_or(0);
    }
    let Some(time) = analysis.beats.get(beat) else {
        return 0;
    };
    // The last downbeat at or before this beat, counted in beats.
    let mut since = 0usize;
    for index in (0..=beat).rev() {
        let Some(candidate) = analysis.beats.get(index) else {
            break;
        };
        if analysis
            .downbeats
            .iter()
            .any(|downbeat| (downbeat - candidate).abs() <= BEAT_WINDOW_S)
        {
            return u8::try_from(since.min(15)).unwrap_or(0);
        }
        if *candidate > *time {
            break;
        }
        since += 1;
    }
    u8::try_from(since.min(15)).unwrap_or(0)
}

/// Which repeated span a beat falls in, one-based; `0` for none.
fn repeat_of(analysis: &SongAnalysis, beat: Option<usize>) -> u8 {
    let Some(beat) = beat else { return 0 };
    for (index, repeat) in analysis.repeats.iter().enumerate() {
        let first = repeat.first_beat..repeat.first_beat + repeat.beats;
        let second = repeat.second_beat..repeat.second_beat + repeat.beats;
        if first.contains(&beat) || second.contains(&beat) {
            return u8::try_from(index + 1).unwrap_or(u8::MAX);
        }
    }
    0
}

/// Write a sidecar beside its chart, atomically.
///
/// Same `.part`-then-rename as every other write in this crate: a
/// half-written sidecar beside a good chart would be worse than none.
pub fn save_context(chart_path: &Path, context: &ChartContext) -> std::io::Result<PathBuf> {
    let path = context_path(chart_path);
    let text = serde_json::to_string(context)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let part = path.with_extension("part");
    std::fs::write(&part, text.as_bytes())?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// Read the sidecar beside a chart, if there is a current one.
#[must_use]
pub fn load_context(chart_path: &Path, chart: &ChartFile) -> Option<ChartContext> {
    let text = std::fs::read_to_string(context_path(chart_path)).ok()?;
    let context: ChartContext = serde_json::from_str(&text).ok()?;
    context.is_current_for(chart).then_some(context)
}

/// Carry a chart's analysis sidecar over to a chart derived from it.
///
/// ⚠️ A file copy would be ignored, not wrong-but-useful: a sidecar
/// names the chart it describes by content hash, and one whose hash
/// does not match is discarded as describing a chart that no longer
/// exists. So the hash is rewritten — which is only honest because
/// this ingredient changes FLAGS and nothing else, so the track
/// events are the same events in the same order and one packed entry
/// still describes one of them.
///
/// ⚠️ Verified rather than assumed: the entry count per difficulty is
/// checked against the new chart's own tracks first. A sidecar that
/// does not line up is worse than none — every later reading would
/// attribute one note's analysis to another.
///
/// `Ok(false)` means the parent had no current sidecar; there was
/// nothing to carry and that is not a failure.
///
/// # Errors
/// When the entries do not line up, or the write fails.
pub fn carry(
    parent_path: &Path,
    parent: &ChartFile,
    next_path: &Path,
    next: &ChartFile,
) -> Result<bool, String> {
    let Some(mut sidecar) = load_context(parent_path, parent) else {
        return Ok(false);
    };
    for definition in &next.charts {
        let id = definition.difficulty.id();
        let events = next
            .to_track(definition.difficulty)
            .map(|track| track.events().len())
            .map_err(|error| format!("{id}: cannot read the new track: {error}"))?;
        let entries = sidecar.tracks.get(id).map_or(0, Vec::len);
        if entries != events {
            return Err(format!(
                "{id}: the parent's sidecar describes {entries} events, the new version has \
                 {events} — refusing to write one that does not line up"
            ));
        }
    }
    sidecar.chart_hash = crate::chart_hash(next);
    debug_assert!(sidecar.is_current_for(next));
    save_context(next_path, &sidecar)
        .map(|_| true)
        .map_err(|error| format!("cannot write the sidecar: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chart whose notes carry the flags given, at the times given.
    fn carry_chart(flags: &[(f64, bool)]) -> ChartFile {
        let notes: Vec<ChartNote> = flags
            .iter()
            .enumerate()
            .map(|(i, (time, hopo))| ChartNote {
                time: *time,
                lane: (i % 5) as u8,
                len: 0.0,
                hopo: *hopo,
            })
            .collect();
        ChartFile::from_json(&format!(
            r#"{{"format_version":1,
                "song":{{"title":"T","artist":"A","audio":"a.m4a","bpm":120.0,
                         "duration_s":100.0,"offset_s":0.0}},
                "charts":[{{"difficulty":"hard","lanes":5,"notes":{},"phrases":[]}}]}}"#,
            serde_json::to_string(&notes).expect("notes serialize")
        ))
        .expect("the fixture parses")
    }

    /// ⚠️ A sidecar names the chart it describes by content hash, so
    /// a straight copy would be DISCARDED — "this describes a chart
    /// that no longer exists" — and the new version would silently
    /// have no analysis behind it. Rewriting the hash is honest here
    /// only because the ingredient changes flags and nothing else.
    #[test]
    fn the_parents_sidecar_is_carried_over_and_still_describes_the_new_version() {
        let dir = std::env::temp_dir().join(format!(
            "bb-classic-ctx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        // 120 BPM: a beat is 0.5 s, so the threshold is 0.177 s.
        // ⚠️ 0.2 s was the first try and changed nothing — 0.4 of a
        // beat is over it. The guard below caught the fixture.
        let parent = carry_chart(&[(0.0, false), (0.15, false), (1.0, false)]);
        let mut next = parent.clone();
        crate::classic::apply_hopo_rule(&mut next);
        assert_ne!(
            crate::chart_hash(&parent),
            crate::chart_hash(&next),
            "the fixture changed no flag, so it proves nothing"
        );

        let parent_path = dir.join("chart.json");
        let next_path = dir.join("chart.v2.json");
        crate::save_chart_file(&parent_path, &parent).expect("the parent");
        crate::save_chart_file(&next_path, &next).expect("the new version");

        // A sidecar for the parent: one entry per track event.
        let events = parent
            .to_track(Difficulty::Hard)
            .expect("a track")
            .events()
            .len();
        let mut tracks = std::collections::BTreeMap::new();
        tracks.insert(Difficulty::Hard.id().to_owned(), vec![[7u8; 6]; events]);
        let sidecar = ChartContext {
            format: CONTEXT_FORMAT.to_owned(),
            chart_hash: crate::chart_hash(&parent),
            tracks,
        };
        save_context(&parent_path, &sidecar).expect("the parent's sidecar");

        assert_eq!(carry(&parent_path, &parent, &next_path, &next), Ok(true));
        // The point: the reader accepts it for the NEW chart.
        let read = load_context(&next_path, &next)
            .expect("the carried sidecar must describe the new version");
        assert_eq!(read.len(), events, "the entries did not travel");

        // And a sidecar that does not line up is refused rather than
        // written: every later reading would blame the wrong note.
        let mut wrong = sidecar.clone();
        wrong
            .tracks
            .get_mut(Difficulty::Hard.id())
            .expect("the track")
            .pop();
        save_context(&parent_path, &wrong).expect("a short sidecar");
        let outcome = carry(&parent_path, &parent, &next_path, &next);
        assert!(
            matches!(&outcome, Err(reason) if reason.contains("does not line up")),
            "a mismatched sidecar was accepted: {outcome:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    use crate::schema::{ChartDef, ChartNote, SongMeta};
    use beatbyte_core::{Onset, Repeat};

    fn analysis() -> SongAnalysis {
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats: (0..16).map(|index| f64::from(index) * 0.5).collect(),
            downbeats: vec![0.0, 2.0, 4.0, 6.0],
            onsets: vec![
                Onset {
                    time_s: 0.0,
                    strength: 1.0,
                    brightness: 0.2,
                },
                Onset {
                    time_s: 2.0,
                    strength: 0.5,
                    brightness: 0.8,
                },
            ],
            energy: vec![0.1, 0.9, 0.5],
            energy_hop_s: 1.0,
            duration_s: 8.0,
            melody: Vec::new(),
            repeats: vec![Repeat {
                first_beat: 0,
                second_beat: 8,
                beats: 4,
                similarity: 0.9,
            }],
        }
    }

    fn chart(times: &[f64]) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Test".to_owned(),
                artist: "Unit".to_owned(),
                audio: "t.wav".to_owned(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(8.0),
                genre: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Medium,
                lanes: 5,
                notes: times
                    .iter()
                    .map(|time| ChartNote {
                        time: *time,
                        lane: 0,
                        len: 0.0,
                        hopo: false,
                    })
                    .collect(),
                phrases: Vec::new(),
            }],
            provenance: None,
            audio_trim: None,
            grid: None,
            rules: None,
        }
    }

    #[test]
    fn a_note_on_a_strong_downbeat_says_all_three_things() {
        let context = context_at(&analysis(), 0.0);
        assert_eq!(context.onset, 255, "the strongest onset in the song");
        assert_eq!(context.brightness, 51, "and a bassy one");
        assert!(context.flags & ContextFlags::ON_BEAT != 0);
        assert!(context.flags & ContextFlags::DOWNBEAT != 0);
        assert_eq!(context.flags & ContextFlags::NO_ONSET, 0);
        assert_eq!(context.bar_phase, 0);
        assert_eq!(context.repeat, 1, "inside the repeated span");
    }

    #[test]
    fn a_note_the_generator_placed_from_the_grid_says_so() {
        // 1.0 s: on a beat, but no onset within the window.
        let context = context_at(&analysis(), 1.0);
        assert_eq!(context.onset, 0);
        assert!(
            context.flags & ContextFlags::NO_ONSET != 0,
            "a zero salience and 'nothing was there' must not be the \
             same reading — one is a quiet hit, the other is no hit"
        );
        assert!(context.flags & ContextFlags::ON_BEAT != 0);
        assert_eq!(context.flags & ContextFlags::DOWNBEAT, 0);
        assert_eq!(context.bar_phase, 8, "the third beat of the bar");
    }

    #[test]
    fn an_offbeat_note_lands_between_the_sixteenths() {
        // 2.25 s: a quarter past the downbeat at 2.0.
        let context = context_at(&analysis(), 2.25);
        assert_eq!(context.flags & ContextFlags::ON_BEAT, 0);
        assert_eq!(context.bar_phase, 2, "halfway through the first beat");
        assert_eq!(context.energy, 128, "the envelope at two seconds");
    }

    #[test]
    fn a_song_without_a_grid_admits_it_rather_than_guessing() {
        let mut bare = analysis();
        bare.beats.clear();
        bare.downbeats.clear();
        let context = context_at(&bare, 1.0);
        assert_eq!(context.bar_phase, 255, "no grid, no phase");
        assert_eq!(context.flags & ContextFlags::ON_BEAT, 0);
    }

    #[test]
    fn a_sidecar_indexes_merged_chord_events_not_chart_notes() {
        // Two notes at the same instant are ONE track event; the
        // telemetry store records that index, and a sidecar keyed the
        // other way would join to the wrong note on every chord.
        let mut file = chart(&[0.0, 2.0]);
        file.charts[0].notes.push(ChartNote {
            time: 0.0,
            lane: 1,
            len: 0.0,
            hopo: false,
        });
        let context = context_for(&file, &analysis());
        let track = file.to_track(Difficulty::Medium).expect("a track");
        assert_eq!(track.events().len(), 2, "three notes, two events");
        assert_eq!(
            context.tracks["medium"].len(),
            2,
            "and two contexts, not three"
        );
        assert_eq!(
            context.get(Difficulty::Medium, 1).map(|c| c.brightness),
            Some(204),
            "the second event takes the bright onset at 2.0 s"
        );
    }

    #[test]
    fn a_sidecar_knows_which_chart_it_describes() {
        let file = chart(&[0.0, 2.0]);
        let context = context_for(&file, &analysis());
        assert!(context.is_current_for(&file));
        assert_eq!(context.len(), 2);
        assert!(!context.is_empty());

        let mut other = file.clone();
        other.charts[0].notes.push(ChartNote {
            time: 3.0,
            lane: 2,
            len: 0.0,
            hopo: false,
        });
        assert!(
            !context.is_current_for(&other),
            "a sidecar for a chart that changed describes notes that \
             no longer exist, and must be ignored rather than trusted"
        );
    }

    #[test]
    fn the_sidecar_goes_beside_its_own_chart_version() {
        assert_eq!(
            context_path(Path::new("/songs/x/chart.v3.json")),
            PathBuf::from("/songs/x/chart.v3.context.json")
        );
        assert_eq!(
            context_path(Path::new("chart.json")),
            PathBuf::from("chart.context.json")
        );
    }

    #[test]
    fn a_sidecar_survives_being_written_and_read() {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-context-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let chart_path = dir.join("chart.v2.json");
        let file = chart(&[0.0, 2.0]);
        let context = context_for(&file, &analysis());
        let written = save_context(&chart_path, &context).expect("writes");
        assert!(written.ends_with("chart.v2.context.json"));
        let back = load_context(&chart_path, &file).expect("reads");
        assert_eq!(back, context);

        // A chart that has moved on gets nothing back.
        let mut other = file.clone();
        other.song.bpm = 140.0;
        assert!(load_context(&chart_path, &other).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_packing_is_lossless_and_six_bytes_wide() {
        let context = NoteContext {
            onset: 200,
            energy: 12,
            brightness: 7,
            bar_phase: 13,
            repeat: 3,
            flags: ContextFlags::ON_BEAT | ContextFlags::DOWNBEAT,
        };
        let packed = Packed::from(context);
        assert_eq!(packed.len(), 6);
        assert_eq!(NoteContext::from(packed), context);
    }

    #[test]
    fn a_nan_in_the_analysis_becomes_a_zero_and_not_a_panic() {
        let mut broken = analysis();
        broken.onsets[0].strength = f32::NAN;
        broken.energy[0] = f32::INFINITY;
        let context = context_at(&broken, 0.0);
        assert_eq!(context.onset, 0);
        assert_eq!(context.energy, 0);
    }
}
