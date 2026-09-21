//! What the files in a song folder are.
//!
//! A song folder holds a chart, its audio, and a growing pile of
//! JSON that is not a chart: the loudness sidecar, the analysis
//! context, a word alignment, a version pointer, and now the
//! document. Everything that walks a folder needs the same answer to
//! "is this a chart?", and the answer must live in one place — the
//! scanner and the migration disagreeing about it is how a folder
//! ends up with two songs, or none.

use std::path::Path;

use beatbyte_audio::loudness::Report;
use beatbyte_chart::versions;

use crate::build::LoudnessFacts;

/// Whether a file name could be a chart rather than a sidecar.
///
/// A name, not a path: the rule is about how a song folder names its
/// files, and it is deliberately a whitelist by exclusion — a chart
/// made by hand may be called anything, so the sidecars are what we
/// can name for certain.
#[must_use]
pub fn is_chart_candidate(name: &str) -> bool {
    name.ends_with(".json")
        && name != versions::POINTER_FILE
        && name != crate::DOC_FILE
        && !name.ends_with(".context.json")
        && !name.ends_with(".loudness.json")
        && !name.ends_with(".words.json")
}

/// What the loudness sidecar tells a document about the file.
///
/// One conversion, because the import has the report in hand and the
/// migration reads it back off the disk — and two copies of "which
/// field goes where" is how a document ends up describing a file
/// that no longer exists.
#[must_use]
pub fn loudness_facts(report: &Report) -> LoudnessFacts {
    LoudnessFacts {
        bytes: Some(report.bytes),
        duration_s: Some(report.measurement.duration_s),
        integrated_lufs: report.measurement.integrated_lufs,
        loudness_range_lu: report.measurement.loudness_range_lu,
        sample_rate: Some(report.quality.sample_rate),
        channels: u16::try_from(report.quality.channels).ok(),
        bitrate_kbps: report.quality.bitrate_kbps,
        lossy: Some(report.quality.lossy),
    }
}

/// The same, read back from the sidecar beside `audio`.
#[must_use]
pub fn read_loudness_facts(audio: &Path) -> Option<LoudnessFacts> {
    beatbyte_audio::loudness::read_report(audio)
        .as_ref()
        .map(loudness_facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sidecar_is_not_mistaken_for_a_chart() {
        assert!(is_chart_candidate("chart.json"));
        assert!(is_chart_candidate("chart.v4.json"));
        assert!(
            is_chart_candidate("girls.chart.json"),
            "a hand-made folder names its chart what it likes, and the \
             migration found one that did"
        );
        for sidecar in [
            "chart.context.json",
            "song.loudness.json",
            "song.words.json",
            "chart-active.json",
            "song.json",
            "notes.txt",
        ] {
            assert!(!is_chart_candidate(sidecar), "{sidecar}");
        }
    }
}
