//! The alignment as it is stored: `<song>.words.json` beside the
//! player's audio (schema `beatbyte.lyrics/1`, plan §L4), and its
//! export as enhanced LRC for any other player.
//!
//! Both live next to the user's own audio and never in this
//! repository — lyrics are copyrighted.

use serde::{Deserialize, Serialize};

/// The schema this crate writes.
pub const SCHEMA: &str = "beatbyte.lyrics/1";

/// One aligned word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedWord {
    /// As written, for display.
    pub text: String,
    /// Song seconds where the word begins.
    pub start: f64,
    /// Song seconds where it ends.
    pub end: f64,
    /// The model's confidence, 0..1 (geometric mean over the word's
    /// letters). 0 for an estimated word.
    pub conf: f32,
    /// `true` when the model placed no letters for this word (a
    /// number, a symbol) and its span is interpolated between its
    /// neighbours.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub estimated: bool,
    /// Per-letter spans `[start, end]`, one per letter the model
    /// aligned — what the per-glyph fill draws from. Empty for an
    /// estimated word.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chars: Vec<[f64; 2]>,
}

/// One aligned line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedLine {
    /// Song seconds where the line's first word begins.
    pub start: f64,
    /// Song seconds where its last word ends.
    pub end: f64,
    /// The line's text, for display.
    pub text: String,
    /// Its words.
    pub words: Vec<AlignedWord>,
}

/// Where the inputs came from — the provenance every result carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The lyric text's origin (`lrclib`, `file:<name>`, …).
    pub text: String,
    /// The separator used, if any (`none` while the aligner runs on
    /// the mix).
    pub separator: String,
    /// The aligner: model id `@sha256:` hash, and the runtime.
    pub aligner: String,
}

/// A whole alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    /// [`SCHEMA`].
    pub schema: String,
    /// SHA-256 of the audio file this was computed on.
    pub audio_sha256: String,
    /// [`crate::PIPELINE_VERSION`] at the time.
    pub pipeline_version: u32,
    /// BCP-47-ish language tag of the transcript (`en`).
    pub language: String,
    /// Provenance.
    pub source: Source,
    /// A constant shift applied at display time, in milliseconds
    /// (per song; 0 when nothing has been adjusted).
    pub offset_ms: i32,
    /// What the confidence gate decided, when it ran (absent in a
    /// raw alignment). See [`crate::gate`](mod@crate::gate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<crate::gate::GateReport>,
    /// The lines.
    pub lines: Vec<AlignedLine>,
}

impl Alignment {
    /// All words, in order.
    pub fn words(&self) -> impl Iterator<Item = &AlignedWord> {
        self.lines.iter().flat_map(|l| l.words.iter())
    }

    /// Serialise for `<song>.words.json`.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a `<song>.words.json`.
    pub fn from_json(json: &str) -> Result<Alignment, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Enhanced LRC (`[mm:ss.xx] <mm:ss.xx>word …`), the interchange
    /// form. Character spans do not survive it — they are a rendering
    /// nicety — and `offset_ms` is folded into the stamps so the file
    /// stands on its own.
    #[must_use]
    pub fn to_lrc(&self) -> String {
        let shift = f64::from(self.offset_ms) / 1000.0;
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(&format!("[{}]", stamp(line.start + shift)));
            for word in &line.words {
                out.push(' ');
                out.push_str(&format!("<{}>{}", stamp(word.start + shift), word.text));
            }
            out.push('\n');
        }
        out
    }
}

/// `mm:ss.xx` for LRC.
fn stamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    format!("{minutes:02.0}:{rest:05.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Alignment {
        Alignment {
            schema: SCHEMA.to_owned(),
            audio_sha256: "00".repeat(32),
            pipeline_version: 1,
            language: "en".to_owned(),
            source: Source {
                text: "file:test.lrc".to_owned(),
                separator: "none".to_owned(),
                aligner: "wav2vec2-base-960h@sha256:e466".to_owned(),
            },
            offset_ms: 0,
            gate: None,
            lines: vec![AlignedLine {
                start: 44.12,
                end: 45.24,
                text: "Ooh, wanna".to_owned(),
                words: vec![
                    AlignedWord {
                        text: "Ooh,".to_owned(),
                        start: 44.12,
                        end: 44.91,
                        conf: 0.93,
                        estimated: false,
                        chars: vec![[44.12, 44.26], [44.26, 44.48], [44.48, 44.91]],
                    },
                    AlignedWord {
                        text: "wanna".to_owned(),
                        start: 44.95,
                        end: 45.24,
                        conf: 0.0,
                        estimated: true,
                        chars: Vec::new(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn the_json_round_trips_and_omits_what_is_default() {
        let json = sample().to_json().expect("serialises");
        assert!(json.contains("\"schema\": \"beatbyte.lyrics/1\""));
        assert!(json.contains("\"estimated\": true"));
        // A confident word carries no `estimated` key and an
        // estimated one no `chars` key.
        let first_word = json.split("\"wanna\"").next().expect("first half");
        assert!(!first_word.contains("\"estimated\""));
        let second_word = json.split("\"wanna\"").nth(1).expect("second half");
        assert!(!second_word.contains("\"chars\""));
        assert_eq!(Alignment::from_json(&json).expect("parses"), sample());
    }

    #[test]
    fn the_lrc_export_stamps_the_line_and_every_word() {
        let lrc = sample().to_lrc();
        assert_eq!(lrc, "[00:44.12] <00:44.12>Ooh, <00:44.95>wanna\n");
        let mut shifted = sample();
        shifted.offset_ms = -120;
        assert!(
            shifted
                .to_lrc()
                .starts_with("[00:44.00] <00:44.00>Ooh, <00:44.83>wanna")
        );
    }
}
