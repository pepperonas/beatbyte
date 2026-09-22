//! A song's whole document, as lines a screen can draw.
//!
//! The substance lives here rather than in the screen that shows it:
//! what a document says, in what order, with the provenance of each
//! claim beside it — why it is Deep House, when it arrived, which
//! analyser said what, and what is simply not known. A screen turns
//! that into nodes and nothing else.
//!
//! ⚠️ **An absent field produces no row.** Not "Unknown", not "—",
//! not a year of 0. The whole point of the document is that missing
//! and known are different, and a view that renders them the same
//! undoes it. What IS missing is said once, in its own section, as a
//! list of areas rather than as a wall of empty labels.

use crate::doc::{Completeness, Key, Mode, SongDoc};
use crate::lifecycle::Millis;
use crate::source::MetaSource;

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// What it is.
    pub label: String,
    /// What it says.
    pub value: String,
    /// Where it came from, when that is worth saying: the source,
    /// and a confidence if the source is one that estimates.
    pub note: Option<String>,
}

/// A group of lines under a heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The heading.
    pub title: &'static str,
    /// Its lines. A section with none is dropped before returning.
    pub rows: Vec<Row>,
}

/// Name a source the way a reader would say it.
#[must_use]
pub fn source_name(source: MetaSource) -> &'static str {
    match source {
        MetaSource::Inferred => "guessed",
        MetaSource::Analyzed => "measured",
        MetaSource::Source => "from the download",
        MetaSource::Embedded => "from the file",
        MetaSource::External => "from a catalogue",
        MetaSource::User => "yours",
    }
}

/// A source with its confidence, when it has one.
fn provenance(source: MetaSource, confidence: Option<f32>) -> Option<String> {
    Some(match confidence {
        Some(value) => format!("{} · {value:.2}", source_name(source)),
        None => source_name(source).to_owned(),
    })
}

/// A key as a musician writes it.
#[must_use]
pub fn key_name(key: Key) -> String {
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let name = NAMES[(key.tonic as usize).min(11)];
    match key.mode {
        Mode::Major => format!("{name} major"),
        Mode::Minor => format!("{name} minor"),
    }
}

/// A UTC timestamp as a date a reader can place.
///
/// Days since the epoch to a civil date by Howard Hinnant's
/// algorithm — no dependency, and the whole thing is arithmetic, so
/// it is pinned against known dates rather than trusted.
#[must_use]
pub fn day_of(ms: Millis) -> String {
    let days = (ms / 86_400_000) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// A length in seconds as minutes and seconds.
#[must_use]
pub fn length_of(seconds: f64) -> String {
    let whole = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Say what an area's completeness means in words.
#[must_use]
pub fn completeness_word(state: Completeness) -> &'static str {
    match state {
        Completeness::Complete => "complete",
        Completeness::Partial => "partly known",
        Completeness::Missing => "nothing known",
        Completeness::NotApplicable => "does not apply",
    }
}

/// Build the report.
#[must_use]
pub fn describe(doc: &SongDoc) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut push = |title: &'static str, rows: Vec<Row>| {
        if !rows.is_empty() {
            sections.push(Section { title, rows });
        }
    };
    let plain = |label: &str, value: String| Row {
        label: label.to_owned(),
        value,
        note: None,
    };
    let sourced = |label: &str, value: String, source: MetaSource, confidence: Option<f32>| Row {
        label: label.to_owned(),
        value,
        note: provenance(source, confidence),
    };

    let mut song = vec![sourced(
        "title",
        doc.identity.title.value.clone(),
        doc.identity.title.source,
        doc.identity.title.confidence,
    )];
    if !doc.identity.artists.is_empty() {
        song.push(plain("artist", doc.identity.artists.join(", ")));
    }
    if let Some(album) = doc.identity.album.as_ref() {
        song.push(sourced(
            "album",
            album.value.clone(),
            album.source,
            album.confidence,
        ));
    }
    if let Some(artist) = doc.identity.album_artist.as_ref() {
        song.push(plain("album artist", artist.clone()));
    }
    if let Some(track) = doc.identity.track_number {
        song.push(plain("track", track.to_string()));
    }
    if let Some(disc) = doc.identity.disc_number {
        song.push(plain("disc", disc.to_string()));
    }
    song.push(plain("id", doc.identity.song_id.as_str().to_owned()));
    // An external id is never the key to anything here — it is a
    // claim by somebody else about which recording this is, and the
    // catalogue run in History says who made it and when.
    if let Some(mbid) = doc.identity.external.musicbrainz_recording.as_ref() {
        song.push(plain("musicbrainz", mbid.clone()));
    }
    push("Song", song);

    let mut about = Vec::new();
    for genre in &doc.descriptive.genres {
        about.push(sourced(
            "genre",
            genre.value.clone(),
            genre.source,
            genre.confidence,
        ));
    }
    if let Some(year) = doc.descriptive.release_year.as_ref() {
        about.push(sourced(
            "released",
            year.value.to_string(),
            year.source,
            year.confidence,
        ));
    }
    if let Some(label) = doc.descriptive.label.as_ref() {
        about.push(plain("label", label.clone()));
    }
    if let Some(language) = doc.descriptive.language.as_ref() {
        about.push(sourced(
            "language",
            language.value.clone(),
            language.source,
            language.confidence,
        ));
    }
    for (label, people) in [
        ("writer", &doc.descriptive.writers),
        ("composer", &doc.descriptive.composers),
        ("producer", &doc.descriptive.producers),
    ] {
        if !people.is_empty() {
            about.push(plain(label, people.join(", ")));
        }
    }
    push("About", about);

    let mut music = Vec::new();
    if let Some(duration) = doc.musical.duration_s {
        music.push(plain("length", length_of(duration)));
    }
    if let Some(sounding) = doc.musical.sounding_s {
        music.push(plain("sounding", length_of(sounding)));
    }
    if let Some(bpm) = doc.musical.bpm.as_ref() {
        music.push(sourced(
            "tempo",
            format!("{:.1} BPM", bpm.value),
            bpm.source,
            bpm.confidence,
        ));
    }
    if let Some(key) = doc.musical.key.as_ref() {
        music.push(sourced(
            "key",
            key_name(key.value),
            key.source,
            key.confidence,
        ));
    }
    if let Some(preview) = doc.musical.preview_start_s {
        music.push(plain("preview at", length_of(preview)));
    }
    push("Music", music);

    let mut measured = Vec::new();
    if let Some(features) = doc.features.as_ref() {
        for (label, value) in [
            ("energy", features.energy),
            ("bass", features.bass_energy),
            ("middle", features.mid_energy),
            ("treble", features.high_energy),
            ("beat strength", features.beat_strength),
        ] {
            if let Some(value) = value {
                measured.push(plain(label, format!("{:.0}%", value * 100.0)));
            }
        }
        if let Some(hz) = features.spectral_centroid_hz {
            measured.push(plain("brightness", format!("{hz:.0} Hz")));
        }
        if let Some(density) = features.onset_density {
            measured.push(plain("onsets", format!("{density:.1} /s")));
        }
        if let Some(lufs) = features.loudness_lufs {
            measured.push(plain("loudness", format!("{lufs:.1} LUFS")));
        }
        if let Some(range) = features.dynamic_range_lu {
            measured.push(plain("dynamics", format!("{range:.1} LU")));
        }
    }
    push("Measured", measured);

    let mut file = vec![plain("file", doc.file.filename.clone())];
    if let Some(bytes) = doc.file.size_bytes {
        file.push(plain("size", format!("{:.1} MB", bytes as f64 / 1e6)));
    }
    if let Some(codec) = doc.file.codec.as_ref() {
        file.push(plain("codec", codec.clone()));
    }
    if let Some(rate) = doc.file.sample_rate {
        file.push(plain("sample rate", format!("{rate} Hz")));
    }
    if let Some(channels) = doc.file.channels {
        file.push(plain("channels", channels.to_string()));
    }
    if let Some(kbps) = doc.file.bitrate_kbps {
        file.push(plain("bitrate", format!("{kbps} kbps")));
    }
    if let Some(hash) = doc.file.content_hash.as_ref() {
        file.push(plain("fingerprint", hash.clone()));
    }
    file.push(plain("came from", format!("{:?}", doc.source.kind)));
    push("File", file);

    let mut words = Vec::new();
    if let Some(lyrics) = doc.lyrics.as_ref() {
        words.push(plain(
            "lyrics",
            if lyrics.has_lyrics { "yes" } else { "no" }.to_owned(),
        ));
        if let Some(aligned) = lyrics.aligned {
            words.push(plain(
                "alignment",
                if aligned {
                    "word by word"
                } else {
                    "by the line"
                }
                .to_owned(),
            ));
        }
        if let Some(lines) = lyrics.line_count {
            words.push(plain("lines", lines.to_string()));
        }
        if let Some(count) = lyrics.word_count {
            words.push(plain("words", count.to_string()));
        }
    }
    push("Words", words);

    let mut play = Vec::new();
    if let Some(name) = doc.gameplay.chart_file.as_ref() {
        play.push(plain("chart", name.clone()));
    }
    if let Some(version) = doc.gameplay.chart_version {
        play.push(plain("generation", version.to_string()));
    }
    if let Some(designer) = doc.gameplay.generator_version.as_ref() {
        play.push(plain("designed by", designer.clone()));
    }
    for instrument in &doc.gameplay.charts {
        for (difficulty, stats) in &instrument.difficulties {
            let mut value = format!("{} notes", stats.note_count);
            if stats.chord_count > 0 {
                value.push_str(&format!(", {} chords", stats.chord_count));
            }
            if stats.sustain_count > 0 {
                value.push_str(&format!(", {} sustains", stats.sustain_count));
            }
            if let Some(nps) = stats.notes_per_second {
                value.push_str(&format!(" · {nps:.1}/s"));
            }
            play.push(plain(&format!("{difficulty:?}").to_lowercase(), value));
        }
    }
    push("Play", play);

    let mut history = vec![
        plain("imported", day_of(doc.lifecycle.imported_at)),
        plain("updated", day_of(doc.lifecycle.updated_at)),
    ];
    for run in &doc.analysis {
        history.push(Row {
            label: run.stage.clone(),
            value: format!("{} v{}", run.analyzer, run.version),
            note: Some(day_of(run.analyzed_at)),
        });
    }
    if !doc.overrides.is_empty() {
        history.push(plain(
            "your edits",
            doc.overrides.iter().cloned().collect::<Vec<_>>().join(", "),
        ));
    }
    history.push(plain("schema", doc.schema_version.to_string()));
    push("History", history);

    // ⚠️ What is missing is said ONCE, here, as areas. Rendering an
    // empty row per unknown field would fill the screen with
    // nothing and bury what IS known.
    let status = crate::completeness::status(doc);
    let missing = status.missing();
    let mut gaps = Vec::new();
    if missing.is_empty() {
        gaps.push(plain(
            "nothing missing",
            "everything known is here".to_owned(),
        ));
    } else {
        for area in missing {
            let state = match area {
                "identity" => status.identity,
                "descriptive" => status.descriptive,
                "file" => status.file,
                "musical" => status.musical,
                "lyrics" => status.lyrics,
                _ => status.gameplay,
            };
            gaps.push(plain(area, completeness_word(state).to_owned()));
        }
    }
    push("Still unknown", gaps);

    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{ChartStats, Instrument, InstrumentCharts, LyricsMeta};
    use crate::{MetaSource, SongId, SourceKind, Sourced};

    fn doc() -> SongDoc {
        SongDoc::new(
            SongId::from_parts(1_700_000_000_000, 1),
            Sourced::stated("Maria".to_owned(), MetaSource::Inferred),
            "maria.m4a".to_owned(),
            SourceKind::LocalFile,
            1_700_000_000_000,
        )
    }

    fn row<'a>(sections: &'a [Section], title: &str, label: &str) -> Option<&'a Row> {
        sections
            .iter()
            .find(|section| section.title == title)?
            .rows
            .iter()
            .find(|row| row.label == label)
    }

    #[test]
    fn a_date_is_the_day_it_really_was() {
        // Pure arithmetic, so it is pinned rather than trusted.
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(1_700_000_000_000), "2023-11-14");
        // A leap day, which is where a hand-rolled calendar breaks.
        assert_eq!(day_of(1_709_164_800_000), "2024-02-29");
    }

    #[test]
    fn a_claim_carries_where_it_came_from() {
        // "Why is it Deep House" is the question this answers.
        let mut d = doc();
        d.descriptive.genres = vec![Sourced::stated(
            "Deep House".to_owned(),
            MetaSource::Embedded,
        )];
        d.musical.key = Some(Sourced::analyzed(
            Key {
                tonic: 9,
                mode: Mode::Minor,
            },
            0.31,
        ));
        let sections = describe(&d);
        assert_eq!(
            row(&sections, "About", "genre").map(|r| (r.value.as_str(), r.note.as_deref())),
            Some(("Deep House", Some("from the file")))
        );
        // A source that ESTIMATES shows its confidence; one that
        // states a fact has none to show.
        assert_eq!(
            row(&sections, "Music", "key").map(|r| (r.value.as_str(), r.note.as_deref())),
            Some(("A minor", Some("measured · 0.31")))
        );
    }

    #[test]
    fn an_absent_field_produces_no_row_at_all() {
        // ⚠️ The rule the whole document exists for. A view that
        // renders "album: unknown" has undone it.
        let sections = describe(&doc());
        for absent in ["album", "released", "label", "key", "fingerprint"] {
            for section in &sections {
                assert!(
                    !section.rows.iter().any(|row| row.label == absent),
                    "{absent} has a row in {}",
                    section.title
                );
            }
        }
        // …and a section with nothing in it is not drawn either.
        assert!(!sections.iter().any(|section| section.title == "Measured"));
        assert!(!sections.iter().any(|section| section.rows.is_empty()));
    }

    #[test]
    fn what_is_not_known_is_said_once_as_areas() {
        let sections = describe(&doc());
        let gaps = sections
            .iter()
            .find(|section| section.title == "Still unknown")
            .expect("a section");
        assert!(gaps.rows.iter().any(|row| row.label == "descriptive"));
        assert!(
            gaps.rows.iter().all(|row| !row.value.is_empty()),
            "each gap says how much is known"
        );
    }

    #[test]
    fn a_catalogue_claim_is_shown_as_somebody_elses_id() {
        let mut d = doc();
        assert!(
            !describe(&d)
                .iter()
                .any(|section| section.rows.iter().any(|row| row.label == "musicbrainz")),
            "absent means no row, here as everywhere"
        );
        d.identity.external.musicbrainz_recording = Some("abc-123".to_owned());
        assert_eq!(
            row(&describe(&d), "Song", "musicbrainz").map(|r| r.value.as_str()),
            Some("abc-123")
        );
    }

    #[test]
    fn a_chart_is_described_by_difficulty() {
        let mut d = doc();
        d.gameplay.charts = vec![InstrumentCharts {
            instrument: Instrument::Guitar,
            difficulties: vec![(
                beatbyte_core::Difficulty::Medium,
                ChartStats {
                    note_count: 301,
                    chord_count: 12,
                    sustain_count: 40,
                    notes_per_second: Some(1.5),
                    peak_notes_per_second: None,
                },
            )],
        }];
        let sections = describe(&d);
        let row = row(&sections, "Play", "medium").expect("a row");
        assert_eq!(row.value, "301 notes, 12 chords, 40 sustains · 1.5/s");
    }

    #[test]
    fn the_analysis_log_says_which_analyser_and_when() {
        let mut d = doc();
        d.analysis.push(crate::doc::AnalysisRun {
            stage: "features".to_owned(),
            analyzer: "beatbyte-features".to_owned(),
            version: 1,
            analyzed_at: 1_700_000_000_000,
        });
        d.lyrics = Some(LyricsMeta {
            has_lyrics: true,
            aligned: Some(false),
            line_count: Some(64),
            ..LyricsMeta::default()
        });
        let sections = describe(&d);
        let run = row(&sections, "History", "features").expect("a row");
        assert_eq!(run.value, "beatbyte-features v1");
        assert_eq!(run.note.as_deref(), Some("2023-11-14"));
        // And a failed alignment reads as what it is.
        assert_eq!(
            row(&sections, "Words", "alignment").map(|r| r.value.as_str()),
            Some("by the line")
        );
    }
}
