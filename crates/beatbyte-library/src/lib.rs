//! BeatByte song metadata: the document a song folder carries.
//!
//! A song in BeatByte is a **folder**. It holds the audio, the chart
//! and every analysis artefact beside it, and copying that folder to
//! another machine is how a song travels. This crate adds the one
//! thing the folder was missing: a document that says who the song
//! is, where it came from, what has been measured about it, which
//! analyser measured it, and which of those values the player has
//! taken into their own hands.
//!
//! # Three domains, deliberately not one
//!
//! ```text
//!   SONG  (this crate, in the folder)    what the song IS
//!   INDEX (a projection, rebuildable)    how to FIND it
//!   EVENTS (beatbyte-telemetry)          what HAPPENED when it was played
//! ```
//!
//! The split is the load-bearing decision. Metadata describes a song
//! and belongs with it; an index is a convenience that must never
//! become the only copy; a play is an event, and events are recorded
//! once and aggregated later. A `play_count` on a song would be a
//! fourth copy of something the session log already knows exactly,
//! and the two would disagree within a week.
//!
//! # What this crate deliberately does not do
//!
//! No IO, no SQL, no clock, no network. It is the model and its
//! rules: [`may_replace`] decides what a refresh may overwrite,
//! [`clean`] decides what counts as a value at all, [`SongId`] is the
//! name a song keeps, and [`Lifecycle`] pins what each timestamp
//! means. Everything that touches a disk is built on top.
//!
//! # The rules this crate enforces
//!
//! - **Absent is `None`.** Never `""`, never `"Unknown"`, never a
//!   release year of `0` ([`clean`]).
//! - **A confidence only exists where something estimated.** A tag
//!   that says `2018` is not 82 % sure ([`Sourced`]).
//! - **A field the player edited is closed to refreshes**, whatever
//!   authority the new source claims ([`may_replace`]).
//! - **`imported_at` is written once**, and `updated_at` moves only
//!   on a real change ([`Lifecycle`]).

pub mod clean;
pub mod doc;
pub mod id;
pub mod lifecycle;
pub mod source;

pub use doc::{
    AnalysisRun, ChartStats, Completeness, DOC_FILE, Descriptive, ExternalIds, Features, FileInfo,
    GameplayMeta, Identity, Instrument, InstrumentCharts, Key, LyricsMeta, Mode, Musical,
    SCHEMA_VERSION, SongDoc, SourceInfo, SourceKind, VocalMeta, field,
};
pub use id::SongId;
pub use lifecycle::{Lifecycle, Millis};
pub use source::{MetaSource, Sourced, may_replace};

#[cfg(test)]
mod tests {
    use super::*;

    fn a_doc() -> SongDoc {
        let mut doc = SongDoc::new(
            SongId::from_parts(1_726_000_000_000, 42),
            Sourced::stated("Maria".to_owned(), MetaSource::Embedded),
            "Blondie - Maria.m4a".to_owned(),
            SourceKind::LocalFile,
            1_726_000_000_000,
        );
        doc.identity.artists = vec!["Blondie".to_owned()];
        doc.descriptive.genres = vec![Sourced::analyzed("House".to_owned(), 0.61)];
        doc
    }

    #[test]
    fn a_document_survives_a_round_trip_through_json() {
        let doc = a_doc();
        let text = serde_json::to_string_pretty(&doc).expect("a document serialises");
        let back: SongDoc = serde_json::from_str(&text).expect("and reads back");
        assert_eq!(back, doc);
    }

    #[test]
    fn what_is_absent_is_absent_from_the_file_too() {
        // Every empty option is skipped, so a fresh document is a
        // short file rather than a page of nulls — which is what
        // makes it readable in a folder, and reviewable in a diff.
        let text = serde_json::to_string(&a_doc()).expect("serialises");
        for absent in [
            "album",
            "release_date",
            "key",
            "features",
            "vocals",
            "lyrics",
        ] {
            assert!(
                !text.contains(absent),
                "{absent} is not known and must not appear at all: {text}"
            );
        }
    }

    #[test]
    fn a_document_from_a_future_build_still_reads() {
        // Forward compatibility is not a nicety here: the folder is
        // portable, so a song can arrive from a machine running a
        // newer BeatByte. Unknown fields are ignored, and everything
        // this build understands still arrives.
        let mut value = serde_json::to_value(a_doc()).expect("serialises");
        value["schema_version"] = serde_json::json!(SCHEMA_VERSION + 7);
        value["identity"]["something_invented_later"] = serde_json::json!({ "a": 1 });
        value["brand_new_section"] = serde_json::json!([1, 2, 3]);
        let back: SongDoc = serde_json::from_value(value).expect("reads anyway");
        assert_eq!(back.identity.title.value, "Maria");
        assert_eq!(
            back.schema_version,
            SCHEMA_VERSION + 7,
            "and it keeps the version it was written with, so a \
             migration can tell where it came from"
        );
    }

    #[test]
    fn the_players_edit_survives_a_refresh_from_a_better_source() {
        // The commission's own example, end to end.
        let mut doc = a_doc();
        assert!(
            doc.accepts(
                field::GENRE,
                Some(MetaSource::Analyzed),
                MetaSource::External
            ),
            "before the player touches it, a catalogue may correct the analyser"
        );

        doc.descriptive.genres = vec![Sourced::by_user("Deep House".to_owned())];
        doc.mark_override(field::GENRE);

        assert!(
            !doc.accepts(field::GENRE, Some(MetaSource::User), MetaSource::External),
            "after it, nothing may"
        );
        assert!(
            doc.accepts(field::YEAR, None, MetaSource::External),
            "and the lock is per field, not per document"
        );
    }

    #[test]
    fn an_id_outlives_everything_the_song_can_change() {
        let mut doc = a_doc();
        let id = doc.identity.song_id.clone();

        // Renamed, moved, re-tagged, re-encoded, redesigned.
        doc.file.filename = "01 Maria (Remaster).flac".to_owned();
        doc.file.sha256 = Some("a".repeat(64));
        doc.identity.title = Sourced::by_user("María".to_owned());
        doc.gameplay.chart_hash = Some("beefbeefbeefbeef".to_owned());
        doc.gameplay.chart_version = Some(4);
        doc.lifecycle.touch(1_726_000_999_999, true);

        assert_eq!(
            doc.identity.song_id, id,
            "not one of those is the song's name"
        );
        assert_eq!(doc.lifecycle.imported_at, 1_726_000_000_000);
    }
}
