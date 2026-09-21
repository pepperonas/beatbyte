//! Whether a folder's document may stand in for the chart beside it.
//!
//! The browser's job is a list, and everything a list shows — title,
//! artist, tempo, length, difficulties, note counts, genre, the
//! preview anchor — sits in the document. Reading it instead of
//! parsing the chart is the difference between 369 KB and 102 MB on
//! this machine's library, which is the whole point of the document.
//!
//! What makes it safe is this decision, and it is deliberately a
//! **decision** rather than three scattered conditions: anything
//! that rewrote the chart after the document was written invalidates
//! the shortcut, so the slow path is never wrong, only slower.

use crate::doc::SongDoc;
use crate::lifecycle::Millis;

/// The generation a chart file name stands for.
///
/// A folder's base chart is generation 1 and `chart.v4.json` is
/// generation 4. One rule in one place, because a document written
/// under one convention and read under another never matches, and
/// the failure is silent: every song quietly takes the slow path.
#[must_use]
pub fn generation(chart_filename: &str) -> u32 {
    beatbyte_chart::versions::version_number(chart_filename).unwrap_or(1)
}

/// Whether `doc` still describes the chart file beside it.
///
/// `chart_filename` is the file's own name and `chart_modified_ms` its
/// modification time — `None` when it could not be read.
///
/// A `false` here is never an error. It means: open the chart.
#[must_use]
pub fn describes(doc: &SongDoc, chart_filename: &str, chart_modified_ms: Option<Millis>) -> bool {
    // A document written for a folder that had no chart describes a
    // folder, not a chart: it has no note counts to offer.
    if doc.gameplay.charts.is_empty() {
        return false;
    }
    // The document must be about THIS file. A folder is full of
    // JSON that is not a chart, and a sidecar described as a song
    // would appear in the browser as a second copy of it.
    if doc.gameplay.chart_file.as_deref() != Some(chart_filename) {
        return false;
    }
    // Another generation is another song to play. Redundant beside
    // the name today, and kept because the two answer to different
    // writers: a rename is not a redesign.
    if doc.gameplay.chart_version != Some(generation(chart_filename)) {
        return false;
    }
    // An editor save, a redesign, a hand edit — each leaves the file
    // newer than the document that described the older one. A
    // timestamp that cannot be read is treated as newer, because
    // being slow is the harmless answer and being wrong is not.
    chart_modified_ms.is_some_and(|modified| modified <= doc.lifecycle.updated_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{ChartStats, Instrument, InstrumentCharts};
    use crate::{SongId, SourceKind};

    fn doc_with(chart_version: u32, updated_at: Millis) -> SongDoc {
        let mut doc = SongDoc::new(
            SongId::from_parts(1, 1),
            crate::Sourced::stated("Maria".to_owned(), crate::MetaSource::Embedded),
            "maria.m4a".to_owned(),
            SourceKind::LocalFile,
            1_000,
        );
        doc.lifecycle.updated_at = updated_at;
        doc.gameplay.chart_version = Some(chart_version);
        doc.gameplay.chart_file = Some(if chart_version == 1 {
            "chart.json".to_owned()
        } else {
            format!("chart.v{chart_version}.json")
        });
        doc.gameplay.charts = vec![InstrumentCharts {
            instrument: Instrument::Guitar,
            difficulties: vec![(
                beatbyte_core::Difficulty::Medium,
                ChartStats {
                    note_count: 301,
                    ..ChartStats::default()
                },
            )],
        }];
        doc
    }

    #[test]
    fn a_document_written_after_the_chart_describes_it() {
        let doc = doc_with(4, 5_000);
        assert!(describes(&doc, "chart.v4.json", Some(4_999)));
        assert!(
            describes(&doc, "chart.v4.json", Some(5_000)),
            "the same moment"
        );
    }

    #[test]
    fn a_chart_rewritten_after_the_document_is_read_the_slow_way() {
        // The editor saves, a redesign runs, someone edits by hand:
        // whatever the document says about note counts is now the
        // previous chart's.
        let doc = doc_with(4, 5_000);
        assert!(!describes(&doc, "chart.v4.json", Some(5_001)));
    }

    #[test]
    fn the_base_chart_is_generation_one() {
        // The migration records `Some(1)` for a folder's base chart.
        // A reader that called it `None` would never match a single
        // un-redesigned song, and nothing would say so.
        assert_eq!(generation("chart.json"), 1);
        assert_eq!(generation("chart.v4.json"), 4);
        assert_eq!(generation("girls.chart.json"), 1, "a hand-made name");
        let doc = doc_with(1, 5_000);
        assert!(describes(&doc, "chart.json", Some(1_000)));
    }

    #[test]
    fn another_generation_is_another_chart() {
        let doc = doc_with(4, 5_000);
        assert!(!describes(&doc, "chart.v5.json", Some(1_000)));
    }

    #[test]
    fn a_document_with_no_chart_stats_cannot_stand_in_for_a_chart() {
        let mut doc = doc_with(4, 5_000);
        doc.gameplay.charts.clear();
        assert!(!describes(&doc, "chart.v4.json", Some(1_000)));
    }

    #[test]
    fn a_sidecar_beside_the_chart_is_not_the_song_again() {
        // `foo.json` in a folder whose document says generation 1
        // would otherwise pass every other test here and appear in
        // the browser as a second copy of the song.
        let doc = doc_with(1, 5_000);
        assert!(describes(&doc, "chart.json", Some(1_000)));
        assert!(!describes(&doc, "song.words.json", Some(1_000)));
        assert!(!describes(&doc, "song.loudness.json", Some(1_000)));
    }

    #[test]
    fn a_timestamp_that_cannot_be_read_sends_the_caller_to_the_chart() {
        let doc = doc_with(4, 5_000);
        assert!(!describes(&doc, "chart.v4.json", None));
    }
}
