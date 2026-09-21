//! Making a document out of what a folder already knows.
//!
//! Every value here comes from a file that is already on disk. **No
//! audio is decoded, no analysis is run and nothing is invented** —
//! a field nobody has an answer for stays absent, which is the point
//! of the exercise: a migration that guesses is a migration that
//! writes fiction into 172 folders at once.
//!
//! Pure over facts the caller has already read, the way
//! `beatbyte_chart::versions` is pure over file names: whoever owns
//! the files does the IO ([`crate::store`]).
//!
//! ## Where each value comes from, and what it is worth
//!
//! | Field | From | Recorded as |
//! |---|---|---|
//! | genre | the audio file's own tag, at import | `Embedded` |
//! | title, artist | the file name or a search result | `Inferred` |
//! | bpm | BeatByte's own analysis | `Analyzed` |
//! | duration, file facts, loudness | the loudness sidecar | — |
//! | chart hash, version, note counts | the active chart | — |
//!
//! ⚠️ Title and artist are recorded as **`Inferred`**, the weakest
//! claim there is, and deliberately so. The import derives them from
//! a file name or a video title; nothing verified them. Recording
//! them as `Embedded` would say a tag stated them, and would then
//! outrank the catalogue that could actually fix them.

use beatbyte_chart::schema::{ChartDef, ChartFile};
use beatbyte_core::Difficulty;

use crate::doc::{
    ChartStats, FileInfo, GameplayMeta, Instrument, InstrumentCharts, LyricsMeta, SourceKind,
};
use crate::lifecycle::Millis;
use crate::source::{MetaSource, Sourced, may_replace};
use crate::{Features, SongDoc, SongId, field};

/// What the loudness sidecar said, in the few fields a document
/// keeps. The caller fills this from `beatbyte_audio`'s report, so
/// this crate needs no audio dependency.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoudnessFacts {
    /// Size of the audio file in bytes.
    pub bytes: Option<u64>,
    /// Seconds of audio.
    pub duration_s: Option<f64>,
    /// Integrated loudness, LUFS.
    pub integrated_lufs: Option<f64>,
    /// Loudness range, LU.
    pub loudness_range_lu: Option<f64>,
    /// Samples per second.
    pub sample_rate: Option<u32>,
    /// Channels.
    pub channels: Option<u16>,
    /// Average bitrate, kbps.
    pub bitrate_kbps: Option<u32>,
    /// Whether the container is lossy.
    pub lossy: Option<bool>,
}

/// Which word files sit beside the audio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LyricFacts {
    /// A `.lrc` is there.
    pub has_lrc: bool,
    /// A word-level alignment is there.
    pub has_words: bool,
    /// Whether that alignment really sings word by word. A failed
    /// one writes the same file with every line fallen back to its
    /// own stamps.
    pub word_level: bool,
    /// How many lines, when they have been read.
    pub line_count: Option<u32>,
    /// How many words, when they have been read.
    pub word_count: Option<u32>,
    /// Whether the lines carry stamps at all.
    pub synced: Option<bool>,
}

/// Everything one folder offers, read once by the caller.
#[derive(Debug, Clone)]
pub struct FolderFacts<'a> {
    /// The active chart, when the folder has one.
    pub chart: Option<&'a ChartFile>,
    /// Which version of it is active (`None` or `1` is the original).
    pub chart_version: Option<u32>,
    /// The chart file's name, so a reader can tell that this
    /// document is about that file and not a sidecar beside it.
    pub chart_filename: Option<String>,
    /// The audio file's name inside the folder.
    pub audio_filename: String,
    /// Its extension, lowercased, when it has one.
    pub extension: Option<String>,
    /// The oldest modification time in the folder, Unix
    /// milliseconds — the best available answer to "when did this
    /// song arrive", and the only one a folder that predates
    /// documents can give.
    pub oldest_file_ms: Millis,
    /// What the loudness sidecar said.
    pub loudness: Option<LoudnessFacts>,
    /// Which word files are there.
    pub lyrics: LyricFacts,
    /// The file's content fingerprint, when somebody has paid for
    /// it. `None` means "not measured", never "no fingerprint": a
    /// pass that does not compute one must not erase one.
    pub content_hash: Option<String>,
    /// What the audio file says about itself in its own tags.
    ///
    /// ⚠️ Empty is the normal case for a downloaded file — measured
    /// here, 0 of 173 carry one — and empty must stay empty rather
    /// than becoming "Unknown".
    pub tags: Option<beatbyte_audio::Tags>,
    /// Where the song came from, when anything says so.
    pub source_kind: SourceKind,
}

/// The note EVENTS of a chart: the things a player hits.
///
/// ⚠️ A chart file stores one row per lane, so a three-note chord is
/// three rows. Counting rows would make every chord-heavy chart look
/// three times as dense as it plays — and would disagree with the
/// telemetry store, which counts what the engine judged. So the rows
/// are grouped exactly the way `beatbyte_chart::convert` groups
/// them, by [`beatbyte_chart::convert::CHORD_EPSILON_S`], and that
/// shared constant is what keeps the two counts the same number.
///
/// Returns `(time, lanes)` per event, in time order. Pure — tested.
#[must_use]
pub fn note_events(def: &ChartDef) -> Vec<(f64, u8)> {
    let mut rows: Vec<(f64, bool)> = def.notes.iter().map(|n| (n.time, n.len > 0.0)).collect();
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut events: Vec<(f64, u8)> = Vec::new();
    for (time, _) in rows {
        match events.last_mut() {
            Some(last) if (time - last.0).abs() <= beatbyte_chart::convert::CHORD_EPSILON_S => {
                last.1 = last.1.saturating_add(1);
            }
            _ => events.push((time, 1)),
        }
    }
    events
}

/// The numbers one difficulty's chart carries.
#[must_use]
pub fn chart_stats(def: &ChartDef, duration_s: Option<f64>) -> ChartStats {
    let events = note_events(def);
    let note_count = u32::try_from(events.len()).unwrap_or(u32::MAX);
    let chord_count =
        u32::try_from(events.iter().filter(|(_, lanes)| *lanes > 1).count()).unwrap_or(u32::MAX);
    // A sustain belongs to its event: a chord held down is one
    // sustain, not one per finger.
    let mut sustains = 0u32;
    for (time, _) in &events {
        if def.notes.iter().any(|n| {
            n.len > 0.0 && (n.time - time).abs() <= beatbyte_chart::convert::CHORD_EPSILON_S
        }) {
            sustains = sustains.saturating_add(1);
        }
    }
    let notes_per_second = duration_s
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| note_count as f32 / seconds as f32);
    ChartStats {
        note_count,
        sustain_count: sustains,
        chord_count,
        notes_per_second,
        peak_notes_per_second: peak_density(&events),
    }
}

/// The busiest second of a chart, in note events.
///
/// A sliding window over the event times rather than a histogram:
/// bucketing by whole seconds would split a burst that straddles a
/// boundary and report the wrong peak.
fn peak_density(events: &[(f64, u8)]) -> Option<f32> {
    if events.is_empty() {
        return None;
    }
    let mut best = 0usize;
    let mut start = 0usize;
    for end in 0..events.len() {
        while events[end].0 - events[start].0 > 1.0 {
            start += 1;
        }
        best = best.max(end - start + 1);
    }
    Some(best as f32)
}

/// What a pass over a folder produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The document as it should now be on disk.
    pub doc: SongDoc,
    /// Whether anything actually changed. A second pass over an
    /// unchanged folder must report `false`, or the migration
    /// rewrites the whole library every time it runs.
    pub changed: bool,
}

/// Build or refresh one song's document from its folder.
///
/// With no `existing` this mints a [`SongId`] and takes
/// `imported_at` from the oldest file in the folder. With one, the
/// id and the import moment are **kept**, and every field obeys
/// [`may_replace`]: a value the player edited is never touched, and a
/// weaker source never overwrites a stronger one.
///
/// `new_id` is passed in rather than generated, so the whole function
/// stays pure and a test can pin the outcome.
#[must_use]
pub fn document_for(
    facts: &FolderFacts<'_>,
    existing: Option<SongDoc>,
    new_id: SongId,
    now: Millis,
) -> Built {
    let mut doc = match existing {
        Some(doc) => doc,
        None => SongDoc::new(
            new_id,
            Sourced::stated(String::new(), MetaSource::Inferred),
            facts.audio_filename.clone(),
            facts.source_kind,
            facts.oldest_file_ms,
        ),
    };
    let before = doc.clone();

    apply_chart(&mut doc, facts);
    apply_tags(&mut doc, facts);
    apply_file(&mut doc, facts);
    apply_lyrics(&mut doc, facts);

    let changed = doc != before;
    doc.lifecycle.touch(now, changed);
    Built { doc, changed }
}

/// Everything the active chart knows.
fn apply_chart(doc: &mut SongDoc, facts: &FolderFacts<'_>) {
    let Some(chart) = facts.chart else { return };

    set_sourced(
        &mut doc.identity.title,
        &doc.overrides,
        field::TITLE,
        chart.song.title.clone(),
        MetaSource::Inferred,
    );
    if !doc.is_overridden(field::ARTISTS)
        && let Some(artist) = crate::clean::text(&chart.song.artist)
        && doc.identity.artists != vec![artist.clone()]
    {
        doc.identity.artists = vec![artist];
    }

    // The genre in a chart came from the audio file's own tag at
    // import (`read_genre`), which is the one thing here we can name
    // the source of honestly.
    if let Some(raw) = chart.song.genre.as_deref() {
        let genres = crate::clean::genres(raw);
        let current = doc.descriptive.genres.first().map(|g| g.source);
        if !genres.is_empty()
            && may_replace(
                current,
                MetaSource::Embedded,
                doc.is_overridden(field::GENRE),
            )
        {
            let wanted: Vec<_> = genres
                .into_iter()
                .map(|g| Sourced::stated(g, MetaSource::Embedded))
                .collect();
            if doc.descriptive.genres != wanted {
                doc.descriptive.genres = wanted;
            }
        }
    }

    let bpm_source = doc.musical.bpm.as_ref().map(|b| b.source);
    if chart.song.bpm > 0.0
        && may_replace(
            bpm_source,
            MetaSource::Analyzed,
            doc.is_overridden(field::BPM),
        )
    {
        // No confidence: the chart stores a tempo, not how sure the
        // analysis was of it. Inventing one here would be exactly
        // the scheinpräzise number the commission rules out.
        let wanted = Sourced::stated(chart.song.bpm, MetaSource::Analyzed);
        if doc.musical.bpm.as_ref() != Some(&wanted) {
            doc.musical.bpm = Some(wanted);
        }
    }
    if let Some(duration) = chart.song.duration_s.filter(|d| *d > 0.0)
        && doc.musical.duration_s != Some(duration)
    {
        doc.musical.duration_s = Some(duration);
    }
    if doc.musical.preview_start_s != chart.song.preview_start_s {
        doc.musical.preview_start_s = chart.song.preview_start_s;
    }

    let hash = beatbyte_chart::schema::chart_hash(chart);
    let stats: Vec<(Difficulty, ChartStats)> = chart
        .charts
        .iter()
        .map(|def| (def.difficulty, chart_stats(def, chart.song.duration_s)))
        .collect();
    let gameplay = GameplayMeta {
        chart_hash: Some(hash),
        chart_version: facts.chart_version,
        chart_file: facts.chart_filename.clone(),
        chart_format: Some(chart.format_version),
        generator_version: chart
            .provenance
            .as_ref()
            .map(|provenance| provenance.designer.clone()),
        charts: vec![InstrumentCharts {
            instrument: Instrument::Guitar,
            difficulties: stats,
        }],
    };
    if doc.gameplay != gameplay {
        doc.gameplay = gameplay;
    }
    if let Some(provenance) = chart.provenance.as_ref()
        && doc.lifecycle.chart_generated_at != Some(provenance.created_ms)
    {
        doc.lifecycle.chart_generated_at = Some(provenance.created_ms);
    }
}

/// What the audio file says about itself.
///
/// The file is an [`MetaSource::Embedded`] witness: it outranks
/// anything inferred from a filename, and loses to the player's own
/// hand. ⚠️ Nothing here invents: a tag the file does not carry
/// leaves the field alone, and a placeholder ("Unknown Artist", a
/// year of 0) is filtered by [`crate::clean`] rather than stored.
fn apply_tags(doc: &mut SongDoc, facts: &FolderFacts<'_>) {
    let Some(tags) = facts.tags.as_ref() else {
        return;
    };
    if let Some(album) = tags.album.as_deref().and_then(crate::clean::text) {
        let current = doc.identity.album.as_ref().map(|held| held.source);
        if may_replace(
            current,
            MetaSource::Embedded,
            doc.is_overridden(field::ALBUM),
        ) && doc.identity.album.as_ref().map(|held| &held.value) != Some(&album)
        {
            doc.identity.album = Some(Sourced::stated(album, MetaSource::Embedded));
        }
    }
    if doc.identity.album_artist.is_none() {
        doc.identity.album_artist = tags.album_artist.as_deref().and_then(crate::clean::text);
    }
    if doc.identity.track_number.is_none() {
        doc.identity.track_number = tags.track_number;
    }
    if doc.identity.disc_number.is_none() {
        doc.identity.disc_number = tags.disc_number;
    }
    if let Some(date) = tags.date.as_deref() {
        // A date is stated as a year or as a full date; take the
        // year from the front and keep the rest only when it really
        // is a fuller date.
        if !doc.is_overridden(field::YEAR)
            && doc.descriptive.release_year.is_none()
            && let Some(year) = date
                .get(..4)
                .and_then(|head| head.parse::<i64>().ok())
                .and_then(crate::clean::year)
        {
            doc.descriptive.release_year = Some(Sourced::stated(year, MetaSource::Embedded));
        }
        if doc.descriptive.release_date.is_none() && date.len() > 4 {
            doc.descriptive.release_date =
                Some(Sourced::stated(date.to_owned(), MetaSource::Embedded));
        }
    }
    for (from, into) in [
        (&tags.writers, &mut doc.descriptive.writers),
        (&tags.composers, &mut doc.descriptive.composers),
        (&tags.producers, &mut doc.descriptive.producers),
    ] {
        if into.is_empty() {
            *into = from
                .iter()
                .filter_map(|name| crate::clean::text(name))
                .collect();
        }
    }
    if doc.descriptive.label.is_none() {
        doc.descriptive.label = tags.label.as_deref().and_then(crate::clean::text);
    }
    if doc.descriptive.copyright.is_none() {
        doc.descriptive.copyright = tags.copyright.as_deref().and_then(crate::clean::text);
    }
    if doc.descriptive.comment.is_none() {
        doc.descriptive.comment = tags.comment.as_deref().and_then(crate::clean::text);
    }
    if !doc.is_overridden(field::LANGUAGE)
        && doc.descriptive.language.is_none()
        && let Some(language) = tags.language.as_deref().and_then(crate::clean::text)
    {
        doc.descriptive.language = Some(Sourced::stated(language, MetaSource::Embedded));
    }
}

/// Everything the loudness sidecar knows about the file.
fn apply_file(doc: &mut SongDoc, facts: &FolderFacts<'_>) {
    if doc.file.filename != facts.audio_filename {
        doc.file.filename = facts.audio_filename.clone();
    }
    if doc.file.codec.is_none()
        && let Some(extension) = facts.extension.clone()
    {
        doc.file.codec = Some(extension);
    }
    // A fingerprint is expensive and therefore rare: a pass that did
    // not compute one leaves the one that is there alone.
    if facts.content_hash.is_some() && doc.file.content_hash != facts.content_hash {
        doc.file.content_hash.clone_from(&facts.content_hash);
    }
    let Some(loud) = facts.loudness else { return };

    let file = FileInfo {
        filename: doc.file.filename.clone(),
        size_bytes: loud.bytes.or(doc.file.size_bytes),
        // Not here: hashing 2.4 GB is a job, not a migration step.
        content_hash: doc.file.content_hash.clone(),
        codec: doc.file.codec.clone(),
        sample_rate: loud.sample_rate.or(doc.file.sample_rate),
        bit_depth: doc.file.bit_depth,
        channels: loud.channels.or(doc.file.channels),
        bitrate_kbps: loud.bitrate_kbps.or(doc.file.bitrate_kbps),
        lossy: loud.lossy.or(doc.file.lossy),
    };
    if doc.file != file {
        doc.file = file;
    }
    if let Some(duration) = loud.duration_s.filter(|d| *d > 0.0)
        && doc.musical.duration_s.is_none()
    {
        doc.musical.duration_s = Some(duration);
    }

    let lufs = loud.integrated_lufs.map(|v| v as f32);
    let range = loud.loudness_range_lu.map(|v| v as f32);
    if lufs.is_some() || range.is_some() {
        let mut features = doc.features.clone().unwrap_or_default();
        features.loudness_lufs = lufs.or(features.loudness_lufs);
        features.dynamic_range_lu = range.or(features.dynamic_range_lu);
        if doc.features.as_ref() != Some(&features) {
            doc.features = Some(features);
        }
    }
}

/// Which word files are there — existence only, no parse.
fn apply_lyrics(doc: &mut SongDoc, facts: &FolderFacts<'_>) {
    if !facts.lyrics.has_lrc && !facts.lyrics.has_words {
        return;
    }
    let mut lyrics = doc.lyrics.clone().unwrap_or(LyricsMeta {
        has_lyrics: false,
        ..LyricsMeta::default()
    });
    lyrics.has_lyrics = facts.lyrics.has_lrc;
    // ⚠️ Not `has_words`. A failed alignment writes the same file
    // with every line fallen back to its own stamps, and a document
    // claiming WORD for that states the wrong thing about the song.
    lyrics.aligned = Some(facts.lyrics.word_level);
    if facts.lyrics.line_count.is_some() {
        lyrics.line_count = facts.lyrics.line_count;
        lyrics.word_count = facts.lyrics.word_count;
        lyrics.synced = facts.lyrics.synced;
    }
    if doc.lyrics.as_ref() != Some(&lyrics) {
        doc.lyrics = Some(lyrics);
    }
}

/// Write a value into a `Sourced` field if the rules allow, cleaning
/// it first and leaving the field alone when it says nothing.
fn set_sourced(
    slot: &mut Sourced<String>,
    overrides: &std::collections::BTreeSet<String>,
    path: &str,
    raw: String,
    source: MetaSource,
) {
    let Some(clean) = crate::clean::text(&raw) else {
        return;
    };
    let current = (!slot.value.is_empty()).then_some(slot.source);
    if may_replace(current, source, overrides.contains(path)) && slot.value != clean {
        *slot = Sourced::stated(clean, source);
    }
}

/// Only what `Features` can hold from this migration.
///
/// Named so the absence is deliberate rather than forgotten: energy,
/// the band split, the spectral centroid, onset density and beat
/// strength are all computable from artefacts BeatByte already
/// writes, but not from the two sidecars a folder is guaranteed to
/// have. They belong to a later milestone, and an empty field is the
/// honest state until then.
#[must_use]
pub fn features_from_loudness(loud: &LoudnessFacts) -> Features {
    Features {
        loudness_lufs: loud.integrated_lufs.map(|v| v as f32),
        dynamic_range_lu: loud.loudness_range_lu.map(|v| v as f32),
        ..Features::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::schema::{ChartDef, ChartFile, ChartNote, SongMeta};

    fn note(time: f64, lane: u8, len: f64) -> ChartNote {
        ChartNote {
            time,
            lane,
            len,
            hopo: false,
        }
    }

    fn chart(notes: Vec<ChartNote>) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Maria".to_owned(),
                artist: "Blondie".to_owned(),
                audio: "maria.m4a".to_owned(),
                bpm: 128.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(200.0),
                genre: Some("Electronic; Deep House".to_owned()),
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Medium,
                lanes: 5,
                notes,
                phrases: Vec::new(),
            }],
            provenance: None,
            audio_trim: None,
            grid: None,
        }
    }

    fn facts<'a>(chart: &'a ChartFile) -> FolderFacts<'a> {
        FolderFacts {
            chart: Some(chart),
            chart_version: Some(1),
            chart_filename: Some("chart.json".to_owned()),
            audio_filename: "maria.m4a".to_owned(),
            extension: Some("m4a".to_owned()),
            oldest_file_ms: 1_700_000_000_000,
            loudness: None,
            lyrics: LyricFacts::default(),
            content_hash: None,
            tags: None,
            source_kind: SourceKind::LocalFile,
        }
    }

    #[test]
    fn a_chord_is_one_note_event_the_way_the_engine_judges_it() {
        // Three rows at one instant are one thing to hit. Counting
        // rows would make this chart look three times as dense as it
        // plays — and would disagree with the telemetry store, which
        // counts what was judged.
        let def = ChartDef {
            difficulty: Difficulty::Medium,
            lanes: 5,
            notes: vec![
                note(1.0, 0, 0.0),
                note(1.002, 1, 0.0),
                note(1.004, 2, 0.0),
                note(2.0, 0, 0.5),
            ],
            phrases: Vec::new(),
        };
        let stats = chart_stats(&def, Some(10.0));
        assert_eq!(stats.note_count, 2, "one chord and one single note");
        assert_eq!(stats.chord_count, 1);
        assert_eq!(stats.sustain_count, 1, "the held note, not the chord");
        assert_eq!(stats.peak_notes_per_second, Some(2.0));
    }

    #[test]
    fn rows_further_apart_than_the_chord_window_stay_separate() {
        let def = ChartDef {
            difficulty: Difficulty::Medium,
            lanes: 5,
            notes: vec![note(1.0, 0, 0.0), note(1.02, 1, 0.0)],
            phrases: Vec::new(),
        };
        assert_eq!(
            chart_stats(&def, None).note_count,
            2,
            "20 ms apart is two hits, and the window that decides is \
             the engine's own"
        );
    }

    #[test]
    fn an_empty_chart_reports_no_density_rather_than_zero() {
        let def = ChartDef {
            difficulty: Difficulty::Medium,
            lanes: 5,
            notes: Vec::new(),
            phrases: Vec::new(),
        };
        let stats = chart_stats(&def, Some(100.0));
        assert_eq!(stats.note_count, 0);
        assert_eq!(
            stats.peak_notes_per_second, None,
            "\"no notes\" is not \"zero notes per second\""
        );
    }

    #[test]
    fn a_second_pass_over_an_unchanged_folder_writes_nothing() {
        // The property the whole migration turns on: running it
        // twice must not rewrite 172 folders.
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let facts = facts(&chart);
        let first = document_for(&facts, None, SongId::from_parts(1, 1), 5_000);
        assert!(first.changed);

        let second = document_for(
            &facts,
            Some(first.doc.clone()),
            SongId::from_parts(2, 2),
            9_000,
        );
        assert!(!second.changed, "nothing in the folder moved");
        assert_eq!(second.doc, first.doc, "and so nothing in the document did");
        assert_eq!(
            second.doc.lifecycle.updated_at, first.doc.lifecycle.updated_at,
            "least of all the clock"
        );
    }

    #[test]
    fn the_import_moment_comes_from_the_folder_not_from_the_clock() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let built = document_for(&facts(&chart), None, SongId::from_parts(1, 1), 9_999_999);
        assert_eq!(
            built.doc.lifecycle.imported_at, 1_700_000_000_000,
            "a song that has been on disk for a year did not arrive today"
        );
    }

    #[test]
    fn a_second_pass_keeps_the_id_it_was_given() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let given = SongId::from_parts(1, 1);
        let first = document_for(&facts(&chart), None, given.clone(), 5_000);
        let second = document_for(
            &facts(&chart),
            Some(first.doc),
            SongId::from_parts(7, 7),
            6_000,
        );
        assert_eq!(
            second.doc.identity.song_id, given,
            "a song is named once; a later pass is not a new song"
        );
    }

    fn tagged(album: &str) -> beatbyte_audio::Tags {
        beatbyte_audio::Tags {
            album: Some(album.to_owned()),
            album_artist: Some("Album Artist".to_owned()),
            track_number: Some(3),
            disc_number: Some(1),
            date: Some("1999-03-04".to_owned()),
            composers: vec!["A Composer".to_owned()],
            label: Some("A Label".to_owned()),
            language: Some("deu".to_owned()),
            ..beatbyte_audio::Tags::default()
        }
    }

    #[test]
    fn a_failed_alignment_is_not_recorded_as_word_level() {
        // ⚠️ A words.json EXISTING is not the same as it singing
        // word by word: a failed alignment writes the same file with
        // every line fallen back to its own stamps, and a document
        // claiming WORD for that states the wrong thing about the
        // song. The browser's LYRICS column makes the same
        // distinction; the document used to make none.
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.lyrics = LyricFacts {
            has_lrc: true,
            has_words: true,
            word_level: false,
            line_count: Some(30),
            word_count: Some(180),
            synced: Some(true),
        };
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        let lyrics = doc.lyrics.expect("the song has words");
        assert_eq!(
            lyrics.aligned,
            Some(false),
            "the file exists; it is not aligned"
        );
        assert_eq!(lyrics.line_count, Some(30));
        assert_eq!(lyrics.word_count, Some(180));
        assert_eq!(lyrics.synced, Some(true));

        facts.lyrics.word_level = true;
        let aligned = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(aligned.lyrics.expect("words").aligned, Some(true));
    }

    #[test]
    fn a_files_own_tags_become_the_document() {
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.tags = Some(tagged("Parallel Lines"));
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;

        assert_eq!(
            doc.identity.album.as_ref().map(|a| a.value.as_str()),
            Some("Parallel Lines")
        );
        assert_eq!(
            doc.identity.album.as_ref().map(|a| a.source),
            Some(MetaSource::Embedded),
            "the file itself said so, and that must be recorded"
        );
        assert_eq!(doc.identity.track_number, Some(3));
        assert_eq!(doc.identity.disc_number, Some(1));
        // A date gives both the year and, only when it really is a
        // date, the rest of it.
        assert_eq!(doc.descriptive.release_year.map(|y| y.value), Some(1999));
        assert_eq!(
            doc.descriptive
                .release_date
                .as_ref()
                .map(|d| d.value.as_str()),
            Some("1999-03-04")
        );
        assert_eq!(doc.descriptive.composers, vec!["A Composer".to_owned()]);
        assert_eq!(doc.descriptive.label.as_deref(), Some("A Label"));
    }

    #[test]
    fn a_file_with_no_tags_leaves_every_field_absent() {
        // The normal case here — 0 of 173 files carry a tag. Absent
        // must stay absent: no "Unknown", no year 0, no empty
        // strings standing in for a fact nobody knows.
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.tags = Some(beatbyte_audio::Tags::default());
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(doc.identity.album, None);
        assert_eq!(doc.identity.album_artist, None);
        assert_eq!(doc.identity.track_number, None);
        assert_eq!(doc.descriptive.release_year, None);
        assert_eq!(doc.descriptive.release_date, None);
        assert!(doc.descriptive.composers.is_empty());
        assert_eq!(doc.descriptive.label, None);
    }

    #[test]
    fn a_placeholder_tag_is_not_a_fact() {
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.tags = Some(beatbyte_audio::Tags {
            album: Some("Unknown Album".to_owned()),
            album_artist: Some("  ".to_owned()),
            date: Some("0000".to_owned()),
            ..beatbyte_audio::Tags::default()
        });
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(
            doc.identity.album, None,
            "\"Unknown Album\" is not an album"
        );
        assert_eq!(doc.identity.album_artist, None);
        assert_eq!(doc.descriptive.release_year, None, "year 0 is not a year");
    }

    #[test]
    fn the_players_own_album_survives_a_tagged_file() {
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.tags = Some(tagged("What The File Says"));
        let mut doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        doc.identity.album = Some(Sourced::by_user("What I Say".to_owned()));
        doc.overrides.insert(field::ALBUM.to_owned());

        let again = document_for(&facts, Some(doc), SongId::from_parts(2, 2), 6_000).doc;
        assert_eq!(
            again.identity.album.as_ref().map(|a| a.value.as_str()),
            Some("What I Say")
        );
    }

    #[test]
    fn a_fingerprint_is_never_erased_by_a_pass_that_did_not_compute_one() {
        // Every scan builds a document; almost none of them pay for
        // a fingerprint. If absence meant "no fingerprint" the
        // expensive work would be undone by the next cheap pass.
        let chart = chart(vec![]);
        let mut facts = facts(&chart);
        facts.content_hash = Some("fnv1a64:00000000000000ff:7".to_owned());
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(
            doc.file.content_hash.as_deref(),
            Some("fnv1a64:00000000000000ff:7")
        );

        facts.content_hash = None;
        let again = document_for(&facts, Some(doc), SongId::from_parts(2, 2), 6_000);
        assert_eq!(
            again.doc.file.content_hash.as_deref(),
            Some("fnv1a64:00000000000000ff:7")
        );
        assert!(!again.changed, "and nothing was rewritten");
    }

    #[test]
    fn the_players_genre_survives_a_rebuild_from_the_chart() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let mut doc = document_for(&facts(&chart), None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(doc.descriptive.genres[0].value, "Electronic");

        doc.descriptive.genres = vec![Sourced::by_user("Post-Punk".to_owned())];
        doc.mark_override(field::GENRE);

        let again = document_for(&facts(&chart), Some(doc), SongId::from_parts(2, 2), 6_000);
        assert_eq!(again.doc.descriptive.genres[0].value, "Post-Punk");
        assert!(!again.changed, "and the file is not rewritten for nothing");
    }

    #[test]
    fn a_folder_without_a_loudness_sidecar_invents_no_file_facts() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let built = document_for(&facts(&chart), None, SongId::from_parts(1, 1), 5_000);
        assert_eq!(built.doc.file.sample_rate, None);
        assert_eq!(built.doc.file.size_bytes, None);
        assert_eq!(
            built.doc.features, None,
            "no measurement is not a measurement of zero"
        );
        assert_eq!(
            built.doc.file.content_hash, None,
            "and a migration does not hash 2.4 GB"
        );
    }

    #[test]
    fn the_loudness_sidecar_fills_the_file_and_the_two_features_it_knows() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let mut facts = facts(&chart);
        facts.loudness = Some(LoudnessFacts {
            bytes: Some(14_183_370),
            duration_s: Some(252.416),
            integrated_lufs: Some(-23.0),
            loudness_range_lu: Some(3.39),
            sample_rate: Some(48_000),
            channels: Some(2),
            bitrate_kbps: Some(450),
            lossy: Some(true),
        });
        let doc = document_for(&facts, None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(doc.file.sample_rate, Some(48_000));
        assert_eq!(doc.file.lossy, Some(true));
        assert_eq!(doc.file.codec.as_deref(), Some("m4a"));
        let features = doc.features.expect("two features are known");
        assert_eq!(features.loudness_lufs, Some(-23.0));
        assert_eq!(
            features.energy, None,
            "energy is computable, but not from this sidecar — and an \
             absent feature is the honest state"
        );
    }

    #[test]
    fn a_title_from_a_file_name_is_recorded_as_the_guess_it_is() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let doc = document_for(&facts(&chart), None, SongId::from_parts(1, 1), 5_000).doc;
        assert_eq!(doc.identity.title.source, MetaSource::Inferred);
        assert_eq!(
            doc.descriptive.genres[0].source,
            MetaSource::Embedded,
            "the genre really did come from the file's own tag"
        );
        assert!(
            may_replace(Some(doc.identity.title.source), MetaSource::External, false),
            "so a catalogue may correct the title later"
        );
    }

    #[test]
    fn one_genre_tag_holding_two_genres_becomes_two() {
        let chart = chart(vec![note(1.0, 0, 0.0)]);
        let doc = document_for(&facts(&chart), None, SongId::from_parts(1, 1), 5_000).doc;
        let names: Vec<&str> = doc
            .descriptive
            .genres
            .iter()
            .map(|g| g.value.as_str())
            .collect();
        assert_eq!(names, vec!["Electronic", "Deep House"]);
    }
}
