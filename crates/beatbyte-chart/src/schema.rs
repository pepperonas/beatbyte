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
}

impl ChartFile {
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
