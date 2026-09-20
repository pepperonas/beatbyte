//! The vocal chart as it is stored: `<audio stem>.vocals.json` beside
//! the player's audio.
//!
//! ## Why it is a sidecar and not part of `chart.json`
//!
//! Three reasons, all of them about not breaking what already works:
//! guitar chart versions are content-hashed and a vocal analysis must
//! not move those hashes; vocal analysis is regenerated on its own
//! schedule (a better model, a better stem) while the guitar chart
//! stands; and a BeatByte that predates vocals simply does not see
//! this file, which is the cheapest possible form of compatibility.
//!
//! The file names the exact audio it was computed on
//! ([`VocalChartFile::audio_sha256`]) and the timeline that audio was
//! decoded onto ([`VocalChartFile::audio_trim`]) — the same two facts
//! the alignment carries, and for the same reason: a chart is only
//! valid against the bytes and the axis it was measured against.

use std::path::{Path, PathBuf};

use beatbyte_core::vocal::{VocalPart, part_problems};
use serde::{Deserialize, Serialize};

use crate::MAX_SONG_LENGTH_S;
use crate::io::ChartIoError;
use crate::schema::AudioTrim;
use crate::validate::{Issue, Severity};

/// The schema this module writes.
pub const VOCAL_SCHEMA: &str = "beatbyte.vocals/1";

/// The analysis pipeline's version. It rises whenever a change would
/// produce a different chart from the same audio, so a stored file
/// can be recognised as stale without being re-analysed to find out.
pub const VOCAL_PIPELINE_VERSION: u32 = 1;

/// Hard cap on a vocal sidecar (untrusted input guard). Contours make
/// these larger than a guitar chart, but not unboundedly so.
pub const MAX_VOCAL_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Most parts one song may carry — a duet plus backing lines is four;
/// anything beyond twelve is a corrupt file.
pub const MAX_VOCAL_PARTS: usize = 12;

/// Where the inputs came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VocalProvenance {
    /// The separator that produced the stem (`demucs htdemucs`), or
    /// `none` when the analyser ran on the mix.
    pub separator: String,
    /// The pitch analyser: a model id and hash, or the name of the
    /// native estimator.
    pub analyzer: String,
    /// The lyric aligner whose words the notes were linked to, when
    /// they were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aligner: Option<String>,
}

impl VocalProvenance {
    /// Provenance for an analysis that ran without a separator and
    /// without lyrics.
    #[must_use]
    pub fn bare(analyzer: &str) -> VocalProvenance {
        VocalProvenance {
            separator: "none".to_owned(),
            analyzer: analyzer.to_owned(),
            aligner: None,
        }
    }
}

/// A whole vocal chart, as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalChartFile {
    /// [`VOCAL_SCHEMA`].
    pub schema: String,
    /// [`VOCAL_PIPELINE_VERSION`] at the time of writing.
    pub pipeline_version: u32,
    /// SHA-256 of the audio file this was computed on.
    pub audio_sha256: String,
    /// The timeline the times are on — the same marker a chart
    /// carries, so a vocal chart and a guitar chart of one song can
    /// be known to share an axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_trim: Option<AudioTrim>,
    /// Where it came from.
    pub provenance: VocalProvenance,
    /// SHA-256 of the alignment the tokens were taken from, when
    /// lyrics were linked. It identifies the snapshot: an alignment
    /// that has since been recomputed no longer matches these tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_sha256: Option<String>,
    /// The voices. One in V1; the shape is plural so a duet does not
    /// need a format change.
    pub parts: Vec<VocalPart>,
}

impl VocalChartFile {
    /// A file with this build's schema and pipeline version.
    #[must_use]
    pub fn new(
        audio_sha256: &str,
        provenance: VocalProvenance,
        parts: Vec<VocalPart>,
    ) -> VocalChartFile {
        VocalChartFile {
            schema: VOCAL_SCHEMA.to_owned(),
            pipeline_version: VOCAL_PIPELINE_VERSION,
            audio_sha256: audio_sha256.to_owned(),
            audio_trim: None,
            provenance,
            lyrics_sha256: None,
            parts,
        }
    }

    /// Parse from JSON. No semantic validation — call
    /// [`VocalChartFile::validate`] on the result.
    pub fn from_json(json: &str) -> Result<VocalChartFile, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialise, indented.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The part a single singer gets: the lead if there is one, else
    /// the first part in the file.
    #[must_use]
    pub fn lead(&self) -> Option<&VocalPart> {
        self.parts
            .iter()
            .find(|p| p.role == beatbyte_core::vocal::VocalRole::Lead)
            .or_else(|| self.parts.first())
    }

    /// A part by its id.
    #[must_use]
    pub fn part(&self, id: &str) -> Option<&VocalPart> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// Whether this file was written by the current pipeline against
    /// `audio_sha256`. A `false` here is what queues a re-analysis —
    /// and the only thing that should, since re-deriving the answer
    /// to find out costs a separator run.
    #[must_use]
    pub fn is_current_for(&self, audio_sha256: &str) -> bool {
        self.schema == VOCAL_SCHEMA
            && self.pipeline_version == VOCAL_PIPELINE_VERSION
            && self.audio_sha256 == audio_sha256
    }

    /// Validate the file, returning every issue found. A file with no
    /// [`Severity::Error`] issue is playable.
    #[must_use]
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let mut err = |location: &str, message: String| {
            issues.push(Issue {
                severity: Severity::Error,
                location: location.to_owned(),
                message,
            });
        };
        if self.schema != VOCAL_SCHEMA {
            err(
                "schema",
                format!("`{}` is not `{VOCAL_SCHEMA}`", self.schema),
            );
        }
        if !is_sha256(&self.audio_sha256) {
            err("audio_sha256", "is not a SHA-256 hex digest".to_owned());
        }
        if let Some(hash) = &self.lyrics_sha256
            && !is_sha256(hash)
        {
            err("lyrics_sha256", "is not a SHA-256 hex digest".to_owned());
        }
        if self.provenance.analyzer.trim().is_empty() {
            err("provenance.analyzer", "is empty".to_owned());
        }
        if self.parts.is_empty() {
            err("parts", "a vocal chart with no parts is not one".to_owned());
        }
        if self.parts.len() > MAX_VOCAL_PARTS {
            err(
                "parts",
                format!("{} parts, more than {MAX_VOCAL_PARTS}", self.parts.len()),
            );
        }
        let mut seen: Vec<&str> = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            if seen.contains(&part.id.as_str()) {
                err("parts", format!("duplicate part id `{}`", part.id));
            }
            seen.push(&part.id);
            for problem in part_problems(part, MAX_SONG_LENGTH_S) {
                err("parts", problem);
            }
        }
        issues
    }

    /// Whether [`VocalChartFile::validate`] found nothing fatal.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        !self
            .validate()
            .iter()
            .any(|i| i.severity == Severity::Error)
    }
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The words a song's alignment places, as vocal tokens.
///
/// Only word-timed lyrics count: a line-timed `.lrc` says when a LINE
/// starts and nothing about which note carries which word, so linking
/// from it would put every word of a line on its first note. An empty
/// result means "there is nothing here to link", which is a state the
/// chart records rather than an error.
#[must_use]
pub fn tokens_from_lyrics(lyrics: &crate::lyrics::Lyrics) -> Vec<beatbyte_core::vocal::VocalToken> {
    if !lyrics.has_word_timing() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    for line in &lyrics.lines {
        for word in &line.words {
            if word.end <= word.start {
                continue;
            }
            tokens.push(beatbyte_core::vocal::VocalToken {
                text: word.text.clone(),
                start_s: word.start,
                end_s: word.end,
                // The alignment's own confidence does not reach this
                // type, and inventing one would be a claim. 1.0 says
                // "the aligner placed it", which is all that is
                // known here; the chart's own confidence is separate.
                confidence: 1.0,
                source_word: None,
            });
        }
    }
    tokens.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    tokens
}

/// Where a song's vocal chart lives: `<audio stem>.vocals.json` beside
/// the audio (the `words.json` convention).
#[must_use]
pub fn vocals_path(audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    audio_path.with_file_name(format!("{stem}.vocals.json"))
}

/// Read the vocal chart beside a song, if there is a readable, valid
/// one.
///
/// A file of another schema, a corrupt file or an invalid one is
/// `None`, not an error: a vocal sidecar is an optional extra, and a
/// song whose vocals cannot be read must still be playable on guitar.
/// Callers that need to know *why* use [`load_vocals`].
#[must_use]
pub fn vocals_beside(audio_path: &Path) -> Option<VocalChartFile> {
    let file = load_vocals(&vocals_path(audio_path)).ok()?;
    file.is_playable().then_some(file)
}

/// Load a vocal chart from an explicit path, saying what went wrong.
pub fn load_vocals(path: &Path) -> Result<VocalChartFile, ChartIoError> {
    let io_err = |source| ChartIoError::Io {
        path: path.to_path_buf(),
        source,
    };
    let size = std::fs::metadata(path).map_err(io_err)?.len();
    if size > MAX_VOCAL_FILE_BYTES {
        return Err(ChartIoError::TooLarge {
            path: path.to_path_buf(),
            size,
            limit: MAX_VOCAL_FILE_BYTES,
        });
    }
    let text = std::fs::read_to_string(path).map_err(io_err)?;
    VocalChartFile::from_json(&text).map_err(|source| ChartIoError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Write a vocal chart, atomically.
///
/// The bytes go to `<path>.part` and are renamed into place, so a
/// crash or a kill mid-write leaves either the previous file or no
/// file — never a half one that would parse as a shorter song. The
/// loader never sees `.part`, because it is not the name it reads.
pub fn save_vocals(path: &Path, file: &VocalChartFile) -> Result<(), ChartIoError> {
    let io_err = |source| ChartIoError::Io {
        path: path.to_path_buf(),
        source,
    };
    let json = file
        .to_json_pretty()
        .map_err(|source| ChartIoError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_err)?;
    }
    let part = part_path(path);
    std::fs::write(&part, json).map_err(io_err)?;
    std::fs::rename(&part, path).map_err(|source| {
        let _ = std::fs::remove_file(&part);
        ChartIoError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// The temporary name [`save_vocals`] writes through.
#[must_use]
pub fn part_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.part"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use beatbyte_core::vocal::{VocalKind, VocalNote, VocalPhrase, VocalRole};

    const HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn part(id: &str, role: VocalRole) -> VocalPart {
        VocalPart {
            id: id.to_owned(),
            role,
            name: None,
            phrases: vec![VocalPhrase {
                start_s: 0.0,
                end_s: 2.0,
                confidence: 0.9,
                tokens: Vec::new(),
                notes: vec![VocalNote {
                    start_s: 0.0,
                    end_s: 1.0,
                    kind: VocalKind::Pitched,
                    target_midi: Some(62.0),
                    contour: Vec::new(),
                    confidence: 0.9,
                    token_range: None,
                }],
            }],
        }
    }

    fn file() -> VocalChartFile {
        VocalChartFile::new(
            HASH,
            VocalProvenance::bare("test"),
            vec![part("lead", VocalRole::Lead)],
        )
    }

    #[test]
    fn a_fresh_file_is_playable_and_round_trips() {
        let f = file();
        assert_eq!(f.validate(), Vec::new());
        assert!(f.is_playable());
        let json = f.to_json_pretty().unwrap();
        assert_eq!(VocalChartFile::from_json(&json).unwrap(), f);
        assert!(!json.contains("audio_trim"), "absent fields stay absent");
    }

    #[test]
    fn the_lead_is_found_by_role_not_by_position() {
        let f = VocalChartFile::new(
            HASH,
            VocalProvenance::bare("test"),
            vec![
                part("harmony", VocalRole::Backing),
                part("main", VocalRole::Lead),
            ],
        );
        assert_eq!(f.lead().map(|p| p.id.as_str()), Some("main"));
        assert_eq!(f.part("harmony").map(|p| p.id.as_str()), Some("harmony"));
        assert!(f.part("nobody").is_none());
        // With no lead at all, the first part is what a single singer gets.
        let backing_only = VocalChartFile::new(
            HASH,
            VocalProvenance::bare("test"),
            vec![part("a", VocalRole::Backing), part("b", VocalRole::Backing)],
        );
        assert_eq!(backing_only.lead().map(|p| p.id.as_str()), Some("a"));
    }

    #[test]
    fn staleness_is_decided_without_re_analysing() {
        let f = file();
        assert!(f.is_current_for(HASH));
        assert!(!f.is_current_for(&"a".repeat(64)), "different audio");
        let mut old = file();
        old.pipeline_version = VOCAL_PIPELINE_VERSION - 1;
        assert!(!old.is_current_for(HASH), "an older pipeline");
        let mut foreign = file();
        foreign.schema = "beatbyte.vocals/99".to_owned();
        assert!(!foreign.is_current_for(HASH));
    }

    #[test]
    fn validation_refuses_the_files_that_would_mislead() {
        let mut wrong_schema = file();
        wrong_schema.schema = "something/else".to_owned();
        assert!(!wrong_schema.is_playable());

        let mut bad_hash = file();
        bad_hash.audio_sha256 = "not a hash".to_owned();
        assert!(
            bad_hash
                .validate()
                .iter()
                .any(|i| i.location == "audio_sha256")
        );

        let mut bad_lyrics = file();
        bad_lyrics.lyrics_sha256 = Some("ZZZ".to_owned());
        assert!(
            bad_lyrics
                .validate()
                .iter()
                .any(|i| i.location == "lyrics_sha256")
        );

        let empty = VocalChartFile::new(HASH, VocalProvenance::bare("t"), Vec::new());
        assert!(!empty.is_playable(), "no parts is not a vocal chart");

        let dupes = VocalChartFile::new(
            HASH,
            VocalProvenance::bare("t"),
            vec![
                part("lead", VocalRole::Lead),
                part("lead", VocalRole::Backing),
            ],
        );
        assert!(
            dupes
                .validate()
                .iter()
                .any(|i| i.message.contains("duplicate"))
        );

        let mut nameless = file();
        nameless.provenance.analyzer = "  ".to_owned();
        assert!(
            nameless
                .validate()
                .iter()
                .any(|i| i.location == "provenance.analyzer")
        );

        // And the part-level rules from core are reported through here.
        let mut broken = file();
        broken.parts[0].phrases[0].notes[0].end_s = 0.0;
        assert!(!broken.is_playable());
    }

    #[test]
    fn only_word_timed_lyrics_become_tokens() {
        use crate::lyrics::{LyricLine, LyricWord, Lyrics};
        // A line-timed file says when a LINE starts and nothing about
        // which note carries which word; linking from it would put a
        // whole line on its first note.
        let line_timed = Lyrics {
            lines: vec![LyricLine {
                start: 1.0,
                end: 3.0,
                text: "hello world".to_owned(),
                words: Vec::new(),
            }],
        };
        assert!(tokens_from_lyrics(&line_timed).is_empty());

        let word_timed = Lyrics {
            lines: vec![LyricLine {
                start: 1.0,
                end: 3.0,
                text: "hello world".to_owned(),
                words: vec![
                    LyricWord {
                        text: "world".to_owned(),
                        start: 2.0,
                        end: 2.8,
                        chars: Vec::new(),
                    },
                    LyricWord {
                        text: "hello".to_owned(),
                        start: 1.0,
                        end: 1.8,
                        chars: Vec::new(),
                    },
                    // A zero-length word places nothing and is left out.
                    LyricWord {
                        text: "".to_owned(),
                        start: 2.9,
                        end: 2.9,
                        chars: Vec::new(),
                    },
                ],
            }],
        };
        let tokens = tokens_from_lyrics(&word_timed);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].text, "hello", "they come back in song order");
        assert_eq!(tokens[1].text, "world");
    }

    #[test]
    fn the_sidecar_sits_beside_the_audio_under_the_house_convention() {
        assert_eq!(
            vocals_path(Path::new("/songs/x/Artist - Title.m4a")),
            PathBuf::from("/songs/x/Artist - Title.vocals.json")
        );
        // A dotted name keeps everything but the last extension.
        assert_eq!(
            vocals_path(Path::new("a.b.wav")),
            PathBuf::from("a.b.vocals.json")
        );
        assert_eq!(
            part_path(Path::new("/x/a.vocals.json")),
            PathBuf::from("/x/a.vocals.json.part")
        );
    }

    #[test]
    fn a_half_written_file_is_never_loaded() {
        let dir = std::env::temp_dir().join(format!("bb-vocals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let audio = dir.join("song.wav");
        let target = vocals_path(&audio);

        // An interrupted write leaves only the `.part`, which the
        // reader does not look at.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(part_path(&target), "{\"schema\":\"trunc").unwrap();
        assert!(
            vocals_beside(&audio).is_none(),
            "the part file is not the file"
        );

        save_vocals(&target, &file()).unwrap();
        assert!(target.is_file());
        assert!(
            !part_path(&target).is_file(),
            "the part is renamed, not left"
        );
        assert_eq!(vocals_beside(&audio).map(|f| f.parts.len()), Some(1));

        // A corrupt file at the real name reads as "no vocals", not as
        // an error that stops the song loading.
        std::fs::write(&target, "{ not json").unwrap();
        assert!(vocals_beside(&audio).is_none());
        assert!(
            load_vocals(&target).is_err(),
            "but the explicit loader says why"
        );

        // An invalid-but-parsing file is equally not playable.
        let mut invalid = file();
        invalid.parts.clear();
        std::fs::write(&target, invalid.to_json_pretty().unwrap()).unwrap();
        assert!(vocals_beside(&audio).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_sidecar_is_refused_rather_than_read() {
        let dir = std::env::temp_dir().join(format!("bb-vocals-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.vocals.json");
        // Cheaply: a sparse-ish file just over the cap.
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(MAX_VOCAL_FILE_BYTES + 1).unwrap();
        drop(f);
        assert!(matches!(
            load_vocals(&path),
            Err(ChartIoError::TooLarge { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
