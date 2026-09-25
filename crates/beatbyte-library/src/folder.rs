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
        sounding_s: report.measurement.sounding_s,
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

/// A file's content fingerprint: FNV-1a 64 over every byte, plus
/// the size as a nearly free second factor.
///
/// Not a cryptographic hash, and the value says so — it is written
/// as `fnv1a64:<hex>:<size>`, so a later build that has a reason to
/// use a stronger one can tell the two apart instead of silently
/// comparing apples to pears. What it is FOR is "is this the same
/// recording": spotting a song imported twice, and noticing that a
/// file has been replaced under a chart that was written for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileFingerprint {
    /// FNV-1a 64 of the file's bytes.
    pub hash: u64,
    /// The file's size in bytes.
    pub size: u64,
}

impl FileFingerprint {
    /// The form the document stores, algorithm included.
    #[must_use]
    pub fn tagged(&self) -> String {
        format!("fnv1a64:{:016x}:{}", self.hash, self.size)
    }
}

/// The FNV-1a 64 offset basis (the empty input's hash).
pub const FNV_BASIS: u64 = 0xCBF2_9CE4_8422_2325;

/// FNV-1a 64, one chunk at a time. Pure — tested against the
/// published vectors.
#[must_use]
pub fn fnv1a_update(mut hash: u64, chunk: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    for byte in chunk {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Fingerprint a file by streaming it. `None` when it cannot be read.
///
/// ⚠️ This reads every byte. On this library that is 2.4 GB, which is
/// why nothing calls it during a scan or a start-up.
#[must_use]
pub fn fingerprint(path: &Path) -> Option<FileFingerprint> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = FNV_BASIS;
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hash = fnv1a_update(hash, &buffer[..read]);
    }
    Some(FileFingerprint { hash, size })
}

/// What the lyric files beside a song say about themselves.
///
/// ⚠️ `aligned` is not "a `words.json` exists". A failed alignment
/// writes the same file with every line fallen back to its own
/// stamps, and a document claiming WORD for that would state the
/// wrong thing about the song — the same distinction the browser's
/// LYRICS column makes.
#[must_use]
pub fn lyric_facts(audio: &Path, chart: &Path) -> crate::build::LyricFacts {
    let words = beatbyte_chart::lyrics::words_path(audio);
    let mut facts = crate::build::LyricFacts {
        has_lrc: beatbyte_chart::lyrics::lyrics_exist_beside(audio, chart),
        has_words: words.exists(),
        word_level: beatbyte_chart::lyrics::alignment_is_word_level(&words),
        ..crate::build::LyricFacts::default()
    };
    if let Some(lyrics) = beatbyte_chart::lyrics::lyrics_beside(audio, chart) {
        facts.line_count = u32::try_from(lyrics.lines.len()).ok();
        let words: usize = lyrics.lines.iter().map(|line| line.words.len()).sum();
        facts.word_count = u32::try_from(words).ok();
        // Every line carrying a start is what makes a lyric file
        // singable; a plain text dump does not.
        facts.synced = Some(!lyrics.lines.is_empty());
    }
    facts
}

/// Measure a song's features by decoding it.
///
/// ⚠️ The expensive one. Decoding is most of the cost, so this
/// belongs where the fingerprint belongs: a command the player ran,
/// or one song at a time in the background. Never in a scan.
#[must_use]
pub fn measure_features(audio: &Path) -> Option<beatbyte_audio::features::SongFeatures> {
    let data = beatbyte_audio::decode_file(audio).ok()?;
    beatbyte_audio::features::measure(data.samples(), data.sample_rate())
}

/// One song, as the duplicate report sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongPrint {
    /// Its file's fingerprint.
    pub fingerprint: String,
    /// Its title, as its document states it.
    pub title: String,
    /// Whether it lives in a study-twin folder.
    pub is_twin: bool,
}

/// Two songs that are the same recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    /// The fingerprint they share.
    pub fingerprint: String,
    /// Every title that shares it, twins included, for context.
    pub titles: Vec<String>,
}

/// Songs whose files are byte-for-byte the same recording.
///
/// ⚠️ A **study twin is not a duplicate.** BeatByte deliberately keeps
/// a second folder over the same audio for the slowed-down practice
/// chart, and on this library that is every single match — 85 of 85
/// measured.
///
/// ⚠️ And a twin is told by its **folder**, not by its title. The
/// first version of this compared folded titles and reported
/// "Nothing Else Matters" against "Metallica- Nothing Else Matters"
/// — a real pair, a genuine twin, whose parent had simply been
/// renamed afterwards. The folder relationship is structural; the
/// title is cosmetic and the player may change it.
///
/// Pure, and it **never deletes anything**: it says what it found.
#[must_use]
pub fn duplicates(songs: &[SongPrint]) -> Vec<Duplicate> {
    let mut by_print: std::collections::BTreeMap<&str, Vec<&SongPrint>> =
        std::collections::BTreeMap::new();
    for song in songs {
        by_print
            .entry(song.fingerprint.as_str())
            .or_default()
            .push(song);
    }
    by_print
        .into_iter()
        // A twin shares its song's audio on purpose. What is left
        // after setting the twins aside is what nobody asked for.
        .filter(|(_, group)| group.iter().filter(|song| !song.is_twin).count() > 1)
        .map(|(fingerprint, group)| Duplicate {
            fingerprint: fingerprint.to_owned(),
            titles: group.iter().map(|song| song.title.clone()).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report's sounding length reaches the facts a document is
    /// built from — the one field the catalogue lookup asks with.
    #[test]
    fn the_sounding_length_travels_from_the_report_to_the_facts() {
        use beatbyte_audio::decode::Channels;
        // Two seconds of tone, then eight of silence.
        let mut samples = vec![0.0f32; 10 * 8_000];
        for (i, s) in samples.iter_mut().take(2 * 8_000).enumerate() {
            *s = (i as f32 * 0.3).sin() * 0.5;
        }
        let channels = Channels {
            interleaved: samples,
            channels: 1,
            sample_rate: 8_000,
            truncated: false,
        };
        let measurement = beatbyte_audio::loudness::measure(&channels);
        let quality = beatbyte_audio::quality::assess(&channels, 1_000, false, 0.0);
        let report = Report {
            schema: beatbyte_audio::loudness::REPORT_SCHEMA.to_owned(),
            measured_by: "test".to_owned(),
            audio: "a.wav".to_owned(),
            bytes: 1_000,
            measurement,
            quality,
        };
        let facts = loudness_facts(&report);
        assert_eq!(facts.duration_s, Some(10.0));
        let sounding = facts.sounding_s.expect("the sounding length travelled");
        assert!((sounding - 2.0).abs() < 0.01, "{sounding}");
    }

    #[test]
    fn fnv1a_matches_the_published_vectors() {
        assert_eq!(fnv1a_update(FNV_BASIS, b""), FNV_BASIS);
        assert_eq!(fnv1a_update(FNV_BASIS, b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_update(FNV_BASIS, b"foobar"), 0x85944171f73967e8_u64);
    }

    #[test]
    fn a_fingerprint_names_the_algorithm_it_used() {
        // A bare hex string invites a later build to compare it with
        // a stronger hash's and find every file "changed".
        let print = FileFingerprint {
            hash: 0xaf63_dc4c_8601_ec8c,
            size: 1,
        };
        assert_eq!(print.tagged(), "fnv1a64:af63dc4c8601ec8c:1");
    }

    fn song(print: &str, title: &str, is_twin: bool) -> SongPrint {
        SongPrint {
            fingerprint: print.to_owned(),
            title: title.to_owned(),
            is_twin,
        }
    }

    #[test]
    fn a_song_and_its_study_twin_are_not_a_duplicate() {
        // Measured on this library: every one of the 85 matching
        // pairs was a song and its own practice twin. A report that
        // called those duplicates would be one nobody could act on.
        let found = duplicates(&[
            song("fnv1a64:1:1", "Maria", false),
            song("fnv1a64:1:1", "[GS] Maria", true),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_twin_whose_song_was_renamed_is_still_a_twin() {
        // The real pair this rule was written for: the twin says
        // "Nothing Else Matters" and its song was renamed to
        // "Metallica- Nothing Else Matters" afterwards. Folding the
        // titles reported it; the folder never lied.
        let found = duplicates(&[
            song("fnv1a64:1:1", "Metallica- Nothing Else Matters", false),
            song("fnv1a64:1:1", "[GS] Nothing Else Matters", true),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_same_recording_imported_twice_is_a_duplicate() {
        let found = duplicates(&[
            song("fnv1a64:1:1", "Maria", false),
            song("fnv1a64:1:1", "maria (1)", false),
            song("fnv1a64:1:1", "[GS] Maria", true),
            song("fnv1a64:2:2", "Something Else", false),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].titles.len(),
            3,
            "the twin is listed for context, it is just not the reason"
        );
        // And it only ever reports. Nothing here deletes a file.
    }

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
