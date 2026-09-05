//! The serde schema of the chart file format (v1).
//!
//! Field names are short because note lists are large; all times are
//! seconds (`f64`) on the song timeline. Unknown fields are tolerated so
//! newer minor revisions can add data without breaking old readers; the
//! `format_version` gate protects against genuinely incompatible files.

use beatbyte_core::Difficulty;
use serde::{Deserialize, Serialize};

/// A complete chart file: one song, several difficulty charts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartFile {
    /// Format version; readers reject files newer than they understand.
    pub format_version: u32,
    /// Song metadata.
    pub song: SongMeta,
    /// One entry per difficulty.
    pub charts: Vec<ChartDef>,
    /// Where a chart VERSION came from (ADR-0011). Absent on
    /// originals; carried by files a redesign wrote. Metadata only:
    /// [`chart_hash`] deliberately does not see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    /// Which audio timeline the times in this file are on. `Some`:
    /// the container's encoder priming was skipped by the decode
    /// that produced (or migrated) them, so time 0 is the master's
    /// sample 0. `None`: a file from before the skip (v0.14.9 and
    /// earlier), whose times sit `priming` late — the reader moves
    /// them once ([`ChartFile::retime`]). Metadata about the axis,
    /// not the content: [`chart_hash`] does not see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_trim: Option<AudioTrim>,
}

/// The encoder priming a chart's audio was decoded without.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioTrim {
    /// Frames skipped before the master's first sample.
    pub priming_samples: u32,
    /// The rate those frames are counted at.
    pub sample_rate: u32,
}

impl AudioTrim {
    /// The marker for a decode that skipped `priming_samples` counted
    /// at `timescale` — or declared nothing (`timescale` 0), in which
    /// case the marker records zero at the decode's own rate, so a
    /// WAV or MP3 chart is marked too and never re-migrated.
    #[must_use]
    pub fn declared(priming_samples: u32, timescale: u32, decode_rate: u32) -> AudioTrim {
        if timescale == 0 || priming_samples == 0 {
            AudioTrim {
                priming_samples: 0,
                sample_rate: decode_rate.max(1),
            }
        } else {
            AudioTrim {
                priming_samples,
                sample_rate: timescale,
            }
        }
    }

    /// The skipped span in seconds; 0 when nothing was declared.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            f64::from(self.priming_samples) / f64::from(self.sample_rate)
        }
    }
}

impl ChartFile {
    /// Move a pre-skip chart onto the trimmed timeline: every time in
    /// it shifts earlier by `trim`'s span and the file is marked. A
    /// chart already marked is left alone, whatever `trim` says — the
    /// operation is idempotent, and a second call cannot double-move.
    /// Returns whether anything changed.
    ///
    /// Why a shift and not a re-import: the times may be hand-judged
    /// design work (the hard/expert redesigns), which a regeneration
    /// would throw away. The shift is exact and its inverse is one
    /// line, so nothing is lost either way.
    pub fn retime(&mut self, trim: AudioTrim) -> bool {
        if self.audio_trim.is_some() {
            return false;
        }
        let shift = trim.seconds();
        if shift > 0.0 {
            let earlier = |t: f64| (t - shift).max(0.0);
            self.song.offset_s -= shift;
            if let Some(preview) = self.song.preview_start_s.as_mut() {
                *preview = earlier(*preview);
            }
            for chart in &mut self.charts {
                for note in &mut chart.notes {
                    note.time = earlier(note.time);
                }
                for phrase in &mut chart.phrases {
                    phrase.start = earlier(phrase.start);
                    phrase.end = earlier(phrase.end);
                }
            }
        }
        self.audio_trim = Some(trim);
        true
    }

    /// Parse a chart from JSON text.
    pub fn from_json(json: &str) -> Result<ChartFile, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The chart for a given difficulty, if present.
    #[must_use]
    pub fn chart_for(&self, difficulty: Difficulty) -> Option<&ChartDef> {
        self.charts.iter().find(|c| c.difficulty == difficulty)
    }
}

/// Song metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongMeta {
    /// Song title.
    pub title: String,
    /// Artist name.
    #[serde(default = "default_artist")]
    pub artist: String,
    /// Relative path to the audio file, resolved against the chart's
    /// directory. Validated against path traversal.
    pub audio: String,
    /// Tempo in beats per minute (constant in format v1).
    pub bpm: f64,
    /// Song-timeline offset of beat 0 in seconds (audio lead-in).
    #[serde(default)]
    pub offset_s: f64,
    /// Optional preview start for the song browser, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_start_s: Option<f64>,
    /// Optional total song duration in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<f64>,
    /// Musical genre, when known — read from the audio file's tags at
    /// import, or set via `beatbyte-cli set-genre`. Display metadata:
    /// [`chart_hash`] deliberately does not see it, so tagging a song
    /// never orphans its recorded sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
}

fn default_artist() -> String {
    "Unknown".to_owned()
}

/// One difficulty's chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartDef {
    /// The difficulty this chart implements.
    pub difficulty: Difficulty,
    /// Number of lanes; format v1 requires 5.
    pub lanes: u8,
    /// The notes, ideally sorted by time (sorted on conversion anyway).
    pub notes: Vec<ChartNote>,
    /// Special (Hype) phrases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<ChartPhrase>,
}

/// A single per-lane note. Simultaneous notes on different lanes form a
/// chord when converted to gameplay events.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartNote {
    /// Hit time in seconds on the song timeline.
    pub time: f64,
    /// Lane index 0–4.
    pub lane: u8,
    /// Sustain length in seconds (0 = tap note).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub len: f64,
    /// Hammer-on/pull-off note.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hopo: bool,
}

/// A special (Hype) phrase: an inclusive time range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartPhrase {
    /// Phrase start in seconds.
    pub start: f64,
    /// Phrase end in seconds.
    pub end: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde requires &T
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde requires &T
fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn minimal_json() -> &'static str {
        r#"{
            "format_version": 1,
            "song": { "title": "T", "audio": "a.ogg", "bpm": 120.0 },
            "charts": [
                { "difficulty": "expert", "lanes": 5,
                  "notes": [ { "time": 1.0, "lane": 2 } ] }
            ]
        }"#
    }

    #[test]
    fn minimal_chart_parses_with_defaults() {
        let chart = ChartFile::from_json(minimal_json()).unwrap();
        assert_eq!(chart.format_version, 1);
        assert_eq!(chart.song.artist, "Unknown");
        assert_eq!(chart.song.offset_s, 0.0);
        let note = chart.charts[0].notes[0];
        assert_eq!(note.len, 0.0);
        assert!(!note.hopo);
    }

    #[test]
    fn round_trip_preserves_content() {
        let chart = ChartFile::from_json(minimal_json()).unwrap();
        let json = chart.to_json_pretty().unwrap();
        let back = ChartFile::from_json(&json).unwrap();
        assert_eq!(chart, back);
    }

    #[test]
    fn compact_serialization_skips_defaults() {
        let chart = ChartFile::from_json(minimal_json()).unwrap();
        let json = chart.to_json_pretty().unwrap();
        assert!(!json.contains("\"hopo\""), "default hopo must be skipped");
        assert!(!json.contains("\"len\""), "zero len must be skipped");
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let json = r#"{
            "format_version": 1,
            "future_field": true,
            "song": { "title": "T", "audio": "a.ogg", "bpm": 120.0, "mood": "heavy" },
            "charts": []
        }"#;
        assert!(ChartFile::from_json(json).is_ok());
    }

    #[test]
    fn chart_for_finds_difficulty() {
        let chart = ChartFile::from_json(minimal_json()).unwrap();
        assert!(chart.chart_for(Difficulty::Expert).is_some());
        assert!(chart.chart_for(Difficulty::Easy).is_none());
    }
}

/// Where a chart version came from — the paper trail a redesign
/// leaves (ADR-0011). Self-describing on purpose: a copied file still
/// says what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// [`chart_hash`] of the version this one was derived from.
    pub parent_hash: String,
    /// Who produced it (`"design-session"`, `"editor"`, …).
    pub designer: String,
    /// Unix milliseconds when it was written.
    pub created_ms: u64,
    /// The directive problem it answers (`"low_accuracy"`, …), when
    /// there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directive: Option<String>,
}

/// FNV-1a over a byte slice: deterministic, dependency-free, and not
/// adversarial — this is an identity for chart *versions*, not a
/// security boundary.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The content hash a telemetry session binds to (ADR-0011).
///
/// Hashed over the canonical serde serialization rather than the disk
/// bytes, because builtin songs have no file and formatting must not
/// matter: the same notes are the same chart however they were
/// indented.
/// Provenance is metadata about a version, not part of what was
/// played, so it is stripped before hashing — otherwise touching the
/// paper trail would orphan every recorded session. Because the field
/// is skipped when `None`, a chart without provenance serializes
/// byte-identically to the pre-provenance format, and every hash
/// recorded before the field existed stays valid.
#[must_use]
pub fn chart_hash(chart: &ChartFile) -> String {
    let mut playable = chart.clone();
    playable.provenance = None;
    playable.song.genre = None;
    // The axis label, not the content: a WAV chart gains the marker
    // with no time moved, and that must not orphan its sessions. A
    // chart whose times DID move hashes differently by its times.
    playable.audio_trim = None;
    let canonical = serde_json::to_vec(&playable).unwrap_or_default();
    format!("{:016x}", fnv1a64(&canonical))
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use beatbyte_core::Difficulty;

    fn tiny_chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Test".to_owned(),
                artist: "Unit".to_owned(),
                audio: "t.wav".to_owned(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(10.0),
                genre: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Medium,
                lanes: 5,
                notes: vec![ChartNote {
                    time: 1.0,
                    lane: 0,
                    len: 0.0,
                    hopo: false,
                }],
                phrases: Vec::new(),
            }],
            provenance: None,
            audio_trim: None,
        }
    }

    #[test]
    fn provenance_does_not_change_a_charts_identity() {
        // Provenance is the paper trail, not the music. If it fed the
        // hash, touching metadata would orphan every recorded session
        // of an unchanged chart.
        let chart = tiny_chart();
        let mut annotated = tiny_chart();
        annotated.provenance = Some(Provenance {
            parent_hash: "abc".to_owned(),
            designer: "test".to_owned(),
            created_ms: 1,
            directive: None,
        });
        assert_eq!(chart_hash(&chart), chart_hash(&annotated));
    }

    #[test]
    fn genre_does_not_change_a_charts_identity() {
        // Genre is display metadata. If it fed the hash, running
        // set-genre on a song would orphan every one of its recorded
        // sessions - notes unchanged, evidence gone.
        let chart = tiny_chart();
        let mut tagged = tiny_chart();
        tagged.song.genre = Some("New Wave".to_owned());
        assert_eq!(chart_hash(&chart), chart_hash(&tagged));
    }

    #[test]
    fn the_hash_of_a_plain_chart_is_stable_across_releases() {
        // Golden value, computed when the provenance field was added.
        // Recorded sessions bind to these hashes forever; if this
        // test breaks, a schema change just orphaned every telemetry
        // file ever written — that is a decision to take knowingly,
        // never a side effect.
        assert_eq!(chart_hash(&tiny_chart()), "06808da3a174344e");
    }

    #[test]
    fn the_hash_binds_to_the_notes_not_the_wrapper() {
        // The whole point: an edited chart is a different chart. One
        // note moved by 10 ms must change the identity a telemetry
        // session binds to.
        let chart = tiny_chart();
        let mut edited = tiny_chart();
        edited.charts[0].notes[0].time = 1.01;
        assert_eq!(chart_hash(&chart), chart_hash(&chart), "not deterministic");
        assert_ne!(
            chart_hash(&chart),
            chart_hash(&edited),
            "an edited note did not change the hash"
        );
    }

    #[test]
    fn the_timeline_marker_alone_does_not_change_a_charts_identity() {
        // A WAV chart gains the marker with no time moved; its
        // recorded sessions must still find it.
        let chart = tiny_chart();
        let mut marked = tiny_chart();
        assert!(marked.retime(AudioTrim::declared(0, 0, 44_100)));
        assert_eq!(marked.audio_trim.map(|t| t.priming_samples), Some(0));
        assert_eq!(chart_hash(&chart), chart_hash(&marked));
        // Whereas a chart whose times DID move is a different chart —
        // its old sessions were played on the old timeline.
        let mut moved = tiny_chart();
        assert!(moved.retime(AudioTrim::declared(1024, 44_100, 44_100)));
        assert_ne!(chart_hash(&chart), chart_hash(&moved));
    }

    #[test]
    fn retime_moves_every_time_earlier_by_the_priming_once() {
        let mut chart = tiny_chart();
        chart.song.offset_s = 0.5;
        chart.song.preview_start_s = Some(0.01);
        chart.charts[0].phrases.push(ChartPhrase {
            start: 2.0,
            end: 3.0,
        });
        chart.charts[0].notes.push(ChartNote {
            time: 0.01,
            lane: 1,
            len: 0.4,
            hopo: false,
        });
        let trim = AudioTrim::declared(1024, 44_100, 44_100);
        let shift = 1024.0 / 44_100.0;
        assert!(chart.retime(trim));
        let notes = &chart.charts[0].notes;
        assert!(
            (notes[0].time - (1.0 - shift)).abs() < 1e-9,
            "a note moved earlier"
        );
        assert_eq!(notes[0].len, 0.0, "a length is a duration, not a position");
        assert!((notes[1].len - 0.4).abs() < 1e-12);
        assert_eq!(
            notes[1].time, 0.0,
            "clamped at the song's start, never negative"
        );
        assert!((chart.charts[0].phrases[0].start - (2.0 - shift)).abs() < 1e-9);
        assert!((chart.charts[0].phrases[0].end - (3.0 - shift)).abs() < 1e-9);
        assert!(
            (chart.song.offset_s - (0.5 - shift)).abs() < 1e-9,
            "beat 0 moved too"
        );
        assert_eq!(chart.song.preview_start_s, Some(0.0));
        assert_eq!(chart.audio_trim, Some(trim));
        // Idempotent: a second call, even with a different trim, is
        // a no-op — the marker says it has been done.
        let before = chart.clone();
        assert!(!chart.retime(AudioTrim::declared(2112, 44_100, 44_100)));
        assert_eq!(chart, before, "a marked chart is never moved again");
    }

    #[test]
    fn a_file_without_the_marker_reads_as_unmarked() {
        // Files from before v0.14.10 carry no `audio_trim`; they must
        // parse, and read as "not yet moved".
        let json = r#"{"format_version":1,"song":{"title":"T","artist":"A","audio":"t.wav","bpm":120.0},"charts":[]}"#;
        let chart = ChartFile::from_json(json).expect("old file parses");
        assert_eq!(chart.audio_trim, None);
        // And a marked file round-trips its marker.
        let mut marked = chart.clone();
        marked.retime(AudioTrim::declared(1024, 44_100, 44_100));
        let text = marked.to_json_pretty().expect("serializes");
        assert!(text.contains("\"audio_trim\""));
        let back = ChartFile::from_json(&text).expect("parses");
        assert_eq!(back.audio_trim, marked.audio_trim);
    }
}
