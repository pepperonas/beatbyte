//! How much of a song's record is actually there.
//!
//! Two questions, deliberately separate. **What is missing** is for a
//! reader — a screen, a report — and says nothing about what to do
//! about it. **What is left to do** is for the background worker, and
//! names exactly one thing: whether the expensive pass has run.
//!
//! The second is not derived from the first on purpose. Most of what
//! a document lacks can never be filled — this library's files carry
//! no tags at all, so an "incomplete" song is the normal state, and a
//! worker that chased completeness would walk the whole library on
//! every start for ever.

use crate::doc::{Completeness, SongDoc};

/// How complete each area of a document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// Who the song is.
    pub identity: Completeness,
    /// What it is about — genre, year, the credits.
    pub descriptive: Completeness,
    /// The file itself.
    pub file: Completeness,
    /// What the music does.
    pub musical: Completeness,
    /// The words.
    pub lyrics: Completeness,
    /// The playable side.
    pub gameplay: Completeness,
}

impl Status {
    /// The areas that are neither complete nor inapplicable, by name.
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        [
            ("identity", self.identity),
            ("descriptive", self.descriptive),
            ("file", self.file),
            ("musical", self.musical),
            ("lyrics", self.lyrics),
            ("gameplay", self.gameplay),
        ]
        .into_iter()
        .filter(|(_, state)| matches!(state, Completeness::Partial | Completeness::Missing))
        .map(|(name, _)| name)
        .collect()
    }
}

/// Rate an area from how many of its parts are there.
fn rate(present: usize, total: usize) -> Completeness {
    if present == total {
        Completeness::Complete
    } else if present == 0 {
        Completeness::Missing
    } else {
        Completeness::Partial
    }
}

/// What a document holds and what it does not.
#[must_use]
pub fn status(doc: &SongDoc) -> Status {
    let identity = rate(
        usize::from(!doc.identity.title.value.trim().is_empty())
            + usize::from(!doc.identity.artists.is_empty())
            + usize::from(doc.identity.album.is_some()),
        3,
    );
    let descriptive = rate(
        usize::from(!doc.descriptive.genres.is_empty())
            + usize::from(doc.descriptive.release_year.is_some())
            + usize::from(doc.descriptive.label.is_some()),
        3,
    );
    let file = rate(
        usize::from(doc.file.codec.is_some())
            + usize::from(doc.file.sample_rate.is_some())
            + usize::from(doc.file.channels.is_some())
            + usize::from(doc.file.content_hash.is_some()),
        4,
    );
    let musical = rate(
        usize::from(doc.musical.duration_s.is_some())
            + usize::from(doc.musical.bpm.is_some())
            + usize::from(doc.musical.key.is_some()),
        3,
    );
    // A song with no words owes none. This is the one area where
    // absent and inapplicable are told apart, because "no lyrics" is
    // a fact about the song rather than a gap in the record.
    let lyrics = match doc.lyrics.as_ref() {
        None => Completeness::NotApplicable,
        Some(meta) if !meta.has_lyrics && meta.aligned != Some(true) => Completeness::NotApplicable,
        Some(meta) => rate(
            usize::from(meta.aligned == Some(true)) + usize::from(meta.line_count.is_some()),
            2,
        ),
    };
    let gameplay = rate(
        usize::from(!doc.gameplay.charts.is_empty())
            + usize::from(doc.gameplay.chart_hash.is_some()),
        2,
    );
    Status {
        identity,
        descriptive,
        file,
        musical,
        lyrics,
        gameplay,
    }
}

/// Whether the expensive pass still owes this song something.
///
/// Exactly one question: has the file been fingerprinted? Everything
/// else a background pass can fill is cheap enough to ride along, and
/// nothing else terminates — a worker that chased a genre this
/// library's files do not carry would never stop.
#[must_use]
pub fn needs_work(doc: &SongDoc) -> bool {
    doc.file.content_hash.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{ChartStats, Instrument, InstrumentCharts, LyricsMeta};
    use crate::{MetaSource, SongId, SourceKind, Sourced};

    fn doc() -> SongDoc {
        SongDoc::new(
            SongId::from_parts(1, 1),
            Sourced::stated("Maria".to_owned(), MetaSource::Embedded),
            "maria.m4a".to_owned(),
            SourceKind::LocalFile,
            1_000,
        )
    }

    #[test]
    fn a_song_with_nothing_but_a_title_is_missing_nearly_everything() {
        let s = status(&doc());
        assert_eq!(s.identity, Completeness::Partial, "a title and no more");
        assert_eq!(s.descriptive, Completeness::Missing);
        assert_eq!(s.gameplay, Completeness::Missing);
        assert!(s.missing().contains(&"descriptive"));
    }

    #[test]
    fn a_song_with_no_words_owes_none() {
        // The one area where absent and inapplicable differ: "no
        // lyrics" is a fact about the song, not a gap in its record.
        let mut d = doc();
        assert_eq!(status(&d).lyrics, Completeness::NotApplicable);
        assert!(!status(&d).missing().contains(&"lyrics"));
        d.lyrics = Some(LyricsMeta {
            has_lyrics: true,
            aligned: Some(true),
            line_count: Some(42),
            ..LyricsMeta::default()
        });
        assert_eq!(status(&d).lyrics, Completeness::Complete);
    }

    #[test]
    fn a_chart_without_its_hash_is_only_half_a_gameplay_record() {
        let mut d = doc();
        d.gameplay.charts = vec![InstrumentCharts {
            instrument: Instrument::Guitar,
            difficulties: vec![(beatbyte_core::Difficulty::Medium, ChartStats::default())],
        }];
        assert_eq!(status(&d).gameplay, Completeness::Partial);
        d.gameplay.chart_hash = Some("abc".to_owned());
        assert_eq!(status(&d).gameplay, Completeness::Complete);
    }

    #[test]
    fn the_worker_asks_one_question_and_stops_asking_once_it_is_answered() {
        // ⚠️ NOT "is anything missing". This library's files carry no
        // tags at all — measured: 0 of 173 have one — so an
        // incomplete song is the normal state, and a worker driven by
        // completeness would walk the whole library every start for
        // ever.
        let mut d = doc();
        assert!(needs_work(&d), "nothing has been fingerprinted yet");
        d.file.content_hash = Some("fnv1a64:0000000000000001:2".to_owned());
        assert!(!needs_work(&d));
        assert!(
            !status(&d).missing().is_empty(),
            "and it is still far from complete, which is fine"
        );
    }
}
