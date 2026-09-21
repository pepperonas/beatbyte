//! The document a song folder carries.
//!
//! One [`SongDoc`] per song, written as `song.json` beside the chart
//! and the audio. It is the **portable** half of the library: copy
//! the folder to another machine and the song arrives with its
//! identity, its history of analysis and the player's own edits
//! intact. The queryable index built from these documents is a
//! projection and can always be thrown away and rebuilt.
//!
//! What is **not** here, on purpose:
//!
//! - the notes — they are the chart's, and the chart is hashed;
//! - the lyrics — they are the `.lrc`'s;
//! - per-note analysis — that is the `.context.json`'s;
//! - anything about how the song was PLAYED — that is an event, and
//!   events live in the telemetry store.

use std::collections::BTreeSet;

use beatbyte_core::Difficulty;
use serde::{Deserialize, Serialize};

use crate::lifecycle::{Lifecycle, Millis};
use crate::source::{MetaSource, Sourced};

/// The document's schema version. Raised whenever a reader older
/// than a writer could misunderstand a file.
pub const SCHEMA_VERSION: u32 = 1;

/// The file a song folder carries.
pub const DOC_FILE: &str = "song.json";

/// Field paths used in [`SongDoc::overrides`].
///
/// Strings rather than an enum: the set is serialised into every
/// document, and a document written by a newer build must not lose a
/// player's edit just because this build has no variant for it.
pub mod field {
    /// The song's title.
    pub const TITLE: &str = "identity.title";
    /// The performing artists.
    pub const ARTISTS: &str = "identity.artists";
    /// The album.
    pub const ALBUM: &str = "identity.album";
    /// The genre list.
    pub const GENRE: &str = "descriptive.genres";
    /// The release year.
    pub const YEAR: &str = "descriptive.release_year";
    /// The language of the words.
    pub const LANGUAGE: &str = "descriptive.language";
    /// The tempo.
    pub const BPM: &str = "musical.bpm";
    /// The musical key.
    pub const KEY: &str = "musical.key";
}

/// Who the song is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    /// The stable internal id. Never derived from a path, never
    /// changed once given — see [`crate::SongId`].
    pub song_id: crate::SongId,
    /// The title.
    pub title: Sourced<String>,
    /// The performing artists, in credit order. Plural because
    /// "A feat. B" is two artists and one string that nothing can
    /// group by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artists: Vec<String>,
    /// The album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<Sourced<String>>,
    /// The album's own artist, when it differs from the track's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// Position on the album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    /// Which disc of a set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    /// Identifiers other systems use. **Never a primary key here**:
    /// they are attributes of the song, and two of them can point at
    /// one recording.
    #[serde(default, skip_serializing_if = "ExternalIds::is_empty")]
    pub external: ExternalIds,
}

/// Identifiers that belong to somebody else's system.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIds {
    /// MusicBrainz recording MBID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musicbrainz_recording: Option<String>,
    /// MusicBrainz release MBID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musicbrainz_release: Option<String>,
    /// International Standard Recording Code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// Whatever the source calls this song (a video id, a track id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

impl ExternalIds {
    /// Whether nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ExternalIds::default()
    }
}

/// What the song is about, as far as anyone has said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Descriptive {
    /// Broad genres. A list because one song can belong to two, and
    /// because a single string cannot be grouped by.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<Sourced<String>>,
    /// Narrower styles under the genres.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subgenres: Vec<Sourced<String>>,
    /// Year of release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_year: Option<Sourced<u16>>,
    /// Full release date, `YYYY-MM-DD`, when a catalogue knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<Sourced<String>>,
    /// Who wrote it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writers: Vec<String>,
    /// Who composed it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composers: Vec<String>,
    /// Who produced it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producers: Vec<String>,
    /// The label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The copyright line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    /// Language of the words, BCP-47 where known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Sourced<String>>,
    /// Whether the words are explicit. `None` means nobody said —
    /// which is not the same as "clean".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit: Option<bool>,
    /// A free note the player attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Where the song came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A file the player already had.
    LocalFile,
    /// Fetched from a video site.
    Youtube,
    /// Something else, named in [`SourceInfo::note`].
    Other,
}

/// How this song got here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// What kind of place it came from.
    pub kind: SourceKind,
    /// The address, when there is one worth keeping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The name the file had when it arrived. Kept because a rename
    /// loses the only clue to how a song was originally labelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
    /// When it was fetched or copied in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded_at: Option<Millis>,
    /// When the source says the thing itself was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_created_at: Option<Millis>,
    /// Anything the fields above cannot hold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The audio file itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInfo {
    /// File name inside the song folder.
    pub filename: String,
    /// Size in bytes at the time of hashing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// SHA-256 of the file. Identity of the BYTES — the same audio
    /// re-encoded is a different hash and the same song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Container/codec as the decoder reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    /// Samples per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// Bits per sample, where the container says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u16>,
    /// Channel count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    /// Average bitrate in kbps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    /// Whether the container is a lossy codec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossy: Option<bool>,
}

/// What the music does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Musical {
    /// Seconds of audio, as the decoder measures it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<f64>,
    /// Seconds that actually sound — a rip with two minutes of
    /// silence at the end is not two minutes longer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounding_s: Option<f64>,
    /// Tempo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<Sourced<f64>>,
    /// Beats per bar, when a meter said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<u8>,
    /// Musical key, when something can determine one. **Nothing in
    /// BeatByte determines one today**, so this stays empty rather
    /// than being filled with a guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Sourced<Key>>,
    /// Where a browser preview should start — the loudest ten
    /// seconds, as the generator found them. Here because it is the
    /// last thing the song list needs, and a list that needs it from
    /// the chart has to open the chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_start_s: Option<f64>,
}

/// A musical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class, `0` = C, rising in semitones.
    pub tonic: u8,
    /// Major or minor.
    pub mode: Mode,
}

/// Major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Major.
    Major,
    /// Minor.
    Minor,
}

/// Song-level audio features.
///
/// **Only what BeatByte can actually measure.** The commission asked
/// for danceability and valence too; there is no model here that
/// determines either, and a number that looks like an answer is
/// worse than a missing one. They are absent rather than guessed,
/// and the struct is additive, so the day a model exists they arrive
/// without a schema break.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// Mean energy of the song, `0.0`–`1.0`, from the analysis
    /// envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy: Option<f32>,
    /// Integrated loudness, LUFS, from the loudness sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f32>,
    /// Loudness range, LU.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_range_lu: Option<f32>,
    /// Share of the spectrum's weight below the low/mid split,
    /// `0.0`–`1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass_energy: Option<f32>,
    /// …in the middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_energy: Option<f32>,
    /// …above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_energy: Option<f32>,
    /// Mean spectral centroid, hertz — "how bright".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_centroid_hz: Option<f32>,
    /// Onsets per second across the song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_density: Option<f32>,
    /// How strongly the beat stands out, `0.0`–`1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_strength: Option<f32>,
}

/// What is known about the singing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VocalMeta {
    /// Whether the song has a sung part at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_vocals: Option<bool>,
    /// Share of the song with a voice in it, `0.0`–`1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocal_share: Option<f32>,
    /// Lowest sung note, MIDI number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_low_midi: Option<u8>,
    /// Highest sung note, MIDI number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_high_midi: Option<u8>,
    /// Notes in the vocal chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_count: Option<u32>,
    /// Phrases in the vocal chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase_count: Option<u32>,
    /// The pipeline that produced it, mirroring the vocal chart's own
    /// version so a stale chart is recognisable without opening it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_version: Option<u32>,
}

/// What is known about the words.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LyricsMeta {
    /// Whether words sit beside the song.
    pub has_lyrics: bool,
    /// Whether they carry times.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced: Option<bool>,
    /// Whether a word-level alignment exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aligned: Option<bool>,
    /// Where they came from (`lrclib`, `user`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Language of the words, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// How well the alignment matched, `0.0`–`1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment_confidence: Option<f32>,
    /// Lines of text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
    /// Words of text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u32>,
}

/// Which instrument a chart is for.
///
/// One variant today and an enum anyway: the commission asks not to
/// hard-wire the guitar, and adding a variant later is free while
/// un-hard-wiring a field is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrument {
    /// The five-lane guitar.
    Guitar,
    /// The sung part.
    Vocals,
}

/// What one difficulty of one instrument's chart contains.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChartStats {
    /// Note events (a chord counts once).
    pub note_count: u32,
    /// Events that carry a sustain.
    #[serde(default)]
    pub sustain_count: u32,
    /// Events with more than one lane.
    #[serde(default)]
    pub chord_count: u32,
    /// Mean events per second over the charted span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_per_second: Option<f32>,
    /// Busiest second of the chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_notes_per_second: Option<f32>,
}

/// The playable side of a song.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameplayMeta {
    /// Content hash of the active chart, the same one the telemetry
    /// store references. Mirrored here so the index can join without
    /// opening chart files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_hash: Option<String>,
    /// Which chart version is active: `None` or `1` is the import's
    /// own first draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_version: Option<u32>,
    /// The chart file this describes, by name.
    ///
    /// A folder holds sidecars that are also JSON — `*.words.json`,
    /// `*.loudness.json`, a version pointer — and a reader that
    /// trusts a document without checking WHICH file it is about
    /// would happily describe one of those as a song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_file: Option<String>,
    /// The chart file format's version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_format: Option<u32>,
    /// Which build wrote the chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator_version: Option<String>,
    /// Per instrument, per difficulty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub charts: Vec<InstrumentCharts>,
}

/// One instrument's charts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentCharts {
    /// Which instrument.
    pub instrument: Instrument,
    /// Difficulty and its numbers, in the chart's own order.
    pub difficulties: Vec<(Difficulty, ChartStats)>,
}

/// One run of one analyser over this song.
///
/// The point is the version. When a better BPM estimator ships, this
/// is what says "this song was measured by the old one" without
/// re-measuring anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRun {
    /// What was analysed (`audio`, `vocals`, `lyrics`, `chart`).
    pub stage: String,
    /// Which analyser, by name.
    pub analyzer: String,
    /// Its algorithm version.
    pub version: u32,
    /// When it ran.
    pub analyzed_at: Millis,
}

/// How complete one area of a song's metadata is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// Everything this area needs is there.
    Complete,
    /// Some of it is.
    Partial,
    /// None of it is.
    Missing,
    /// It does not apply — an instrumental owes no lyrics.
    NotApplicable,
}

/// The song document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongDoc {
    /// [`SCHEMA_VERSION`] at the time of writing.
    pub schema_version: u32,
    /// Who the song is.
    pub identity: Identity,
    /// What it is about.
    #[serde(default)]
    pub descriptive: Descriptive,
    /// Where it came from.
    pub source: SourceInfo,
    /// Its file.
    pub file: FileInfo,
    /// What the music does.
    #[serde(default)]
    pub musical: Musical,
    /// Measured features, where they are measurable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Features>,
    /// The singing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocals: Option<VocalMeta>,
    /// The words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<LyricsMeta>,
    /// The playable side.
    #[serde(default)]
    pub gameplay: GameplayMeta,
    /// Every analyser that has touched this song, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub analysis: Vec<AnalysisRun>,
    /// When things happened.
    pub lifecycle: Lifecycle,
    /// Field paths the player has edited by hand. A refresh may not
    /// write any of them — see [`crate::may_replace`].
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub overrides: BTreeSet<String>,
}

impl SongDoc {
    /// A new document for a song entering the library.
    #[must_use]
    pub fn new(
        song_id: crate::SongId,
        title: Sourced<String>,
        filename: String,
        kind: SourceKind,
        now: Millis,
    ) -> SongDoc {
        SongDoc {
            schema_version: SCHEMA_VERSION,
            identity: Identity {
                song_id,
                title,
                artists: Vec::new(),
                album: None,
                album_artist: None,
                track_number: None,
                disc_number: None,
                external: ExternalIds::default(),
            },
            descriptive: Descriptive::default(),
            source: SourceInfo {
                kind,
                url: None,
                original_filename: None,
                downloaded_at: None,
                source_created_at: None,
                note: None,
            },
            file: FileInfo {
                filename,
                size_bytes: None,
                sha256: None,
                codec: None,
                sample_rate: None,
                bit_depth: None,
                channels: None,
                bitrate_kbps: None,
                lossy: None,
            },
            musical: Musical::default(),
            features: None,
            vocals: None,
            lyrics: None,
            gameplay: GameplayMeta::default(),
            analysis: Vec::new(),
            lifecycle: Lifecycle::imported(now),
            overrides: BTreeSet::new(),
        }
    }

    /// Whether the player has taken this field into their own hands.
    #[must_use]
    pub fn is_overridden(&self, path: &str) -> bool {
        self.overrides.contains(path)
    }

    /// Record that the player edited this field.
    pub fn mark_override(&mut self, path: &str) {
        self.overrides.insert(path.to_owned());
    }

    /// The song's artists as one display string.
    #[must_use]
    pub fn artist_line(&self) -> Option<String> {
        (!self.identity.artists.is_empty()).then(|| self.identity.artists.join(", "))
    }

    /// Whether an automatic refresh may write `incoming` into the
    /// field at `path`, given what is recorded there now.
    #[must_use]
    pub fn accepts(&self, path: &str, current: Option<MetaSource>, incoming: MetaSource) -> bool {
        crate::may_replace(current, incoming, self.is_overridden(path))
    }
}
