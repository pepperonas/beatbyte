//! The check track: a song with a click on every aligned word, so an
//! alignment can be judged **by ear** instead of by number.
//!
//! Ground truth for the own fixture set (`docs/lyrics/fixtures.md`)
//! has to be corrected by a person, and a table of times is not
//! something a person can check. A click that lands on the word is
//! right, a click that lands beside it is wrong, and anybody hears
//! the difference in one pass.
//!
//! The click itself lives in [`beatbyte_audio::mark`] — a chart's
//! notes ask the same question of the same recording, so the marking
//! is not the lyrics pipeline's private business.

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::mark::marked;

/// A check track for an alignment: the audio with a click on every
/// word that carries real timing. Estimated words are marked too —
/// they are exactly the ones worth listening to.
#[must_use]
pub fn check_track(audio: &AudioData, alignment: &crate::words::Alignment) -> AudioData {
    let onsets: Vec<f64> = alignment.words().map(|w| w.start).collect();
    marked(audio, &onsets)
}

/// The word list a listener reads along with the check track: one
/// line per lyric line, each word with its onset, estimated words
/// marked. Pure — tested.
#[must_use]
pub fn word_sheet(alignment: &crate::words::Alignment) -> String {
    let mut out = String::new();
    for (index, line) in alignment.lines.iter().enumerate() {
        out.push_str(&format!("{:>3}. [{:>8.3}] ", index + 1, line.start));
        for word in &line.words {
            out.push_str(&format!(
                "{}{}({:.3})  ",
                word.text,
                if word.estimated { "*" } else { "" },
                word.start
            ));
        }
        out.push('\n');
    }
    out.push_str("\n* = estimated (no acoustic evidence of its own)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::words::{AlignedLine, AlignedWord, Alignment, SCHEMA, Source};

    #[test]
    fn the_sheet_reads_like_something_a_person_can_follow() {
        let alignment = Alignment {
            schema: SCHEMA.to_owned(),
            audio_sha256: "00".repeat(32),
            pipeline_version: 1,
            language: "en".to_owned(),
            source: Source {
                text: "t".to_owned(),
                separator: "none".to_owned(),
                aligner: "a".to_owned(),
            },
            offset_ms: 0,
            gate: None,
            lines: vec![AlignedLine {
                start: 1.0,
                end: 2.0,
                text: "Hi there".to_owned(),
                words: vec![
                    AlignedWord {
                        text: "Hi".to_owned(),
                        start: 1.0,
                        end: 1.4,
                        conf: 0.5,
                        estimated: false,
                        chars: Vec::new(),
                    },
                    AlignedWord {
                        text: "there".to_owned(),
                        start: 1.5,
                        end: 2.0,
                        conf: 0.0,
                        estimated: true,
                        chars: Vec::new(),
                    },
                ],
            }],
        };
        let sheet = word_sheet(&alignment);
        assert!(sheet.contains("Hi(1.000)"), "{sheet}");
        assert!(
            sheet.contains("there*(1.500)"),
            "an estimated word is marked"
        );
        assert!(sheet.contains("  1. [   1.000]"));
    }
}
