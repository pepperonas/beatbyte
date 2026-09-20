//! The separated stems of a song, and the record of what happened
//! when BeatByte tried to make them.
//!
//! Separation is expensive — minutes of a saturated machine — so the
//! expensive part is done once and the **answer is written down**,
//! including the answers that are "no". A song with no singing, a
//! song whose vocals came back unusable and a song the separator
//! refuses are all *results*: [`StemState`] records them, and
//! [`StemState::retry_when`] is what stops a library rescan from
//! asking the same expensive question on every start.
//!
//! The manifest sits at `<audio stem>.stems.json` and the audio at
//! `<audio stem>.stems/`, beside the song, under the same convention
//! as `loudness.json` and `words.json`.
//!
//! ## Size
//!
//! Stems are 16-bit WAV at the song's own rate and width. A four
//! minute stereo song costs roughly 40 MB per kept stem, so a library
//! pays for this in gigabytes. That is a deliberate V1 trade — a
//! verified FLAC cache is the named follow-up — and it is why
//! [`StemManifest::bytes`] is recorded: the cost is visible rather
//! than discovered when a disk fills.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The schema this module writes.
pub const STEM_SCHEMA: &str = "beatbyte.stems/1";

/// The separation pipeline's version. It rises when a change would
/// produce different stems from the same audio.
pub const STEM_PIPELINE_VERSION: u32 = 1;

/// Which stem a file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    /// The singing, isolated: what the pitch analyser and the lyric
    /// aligner read, and what an "original vocals" mix plays.
    Vocals,
    /// Everything but the singing: the karaoke backing.
    Instrumental,
    /// The separator's `other` stem — guitars and keys — which the
    /// Guitar Study twin charts.
    Other,
}

impl StemKind {
    /// The file name this stem is stored under.
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            StemKind::Vocals => "vocals.wav",
            StemKind::Instrumental => "instrumental.wav",
            StemKind::Other => "other.wav",
        }
    }
}

/// One stored stem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemFile {
    /// Which stem it is.
    pub kind: StemKind,
    /// Its file name inside the stem directory — a plain name, never
    /// a path.
    pub file: String,
    /// Its sample rate.
    pub sample_rate: u32,
    /// Its channel count.
    pub channels: u32,
    /// Its length in seconds.
    pub duration_s: f64,
    /// Its size on disk.
    pub bytes: u64,
}

/// How far separation got, and whether asking again could help.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StemState {
    /// The stems named in the manifest are on disk and usable.
    Ready,
    /// The song has no singing in it. Not a failure — an answer.
    Instrumental,
    /// There is singing, but not enough of it came back cleanly
    /// enough to hold a player to. Vocal play is offered as unscored
    /// or not at all; normal play is unaffected.
    NoReliableVocals,
    /// The separator is not installed on this machine. The only state
    /// that a change *outside* BeatByte can resolve.
    NeedsSeparator,
    /// It went wrong.
    Failed {
        /// What went wrong, in the words a player can act on.
        reason: String,
        /// Whether trying again could plausibly succeed — a killed
        /// process or a full disk, yes; a file the decoder cannot
        /// read, no.
        retryable: bool,
    },
}

impl StemState {
    /// Whether this is a settled answer about the song: running the
    /// separator again would spend minutes to learn the same thing.
    ///
    /// [`StemState::NeedsSeparator`] is deliberately *not* settled —
    /// it is a statement about the machine, and installing the tool
    /// changes it.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            StemState::Ready
                | StemState::Instrumental
                | StemState::NoReliableVocals
                | StemState::Failed {
                    retryable: false,
                    ..
                }
        )
    }

    /// Whether the background queue should pick this song up again,
    /// given whether a separator is available now.
    #[must_use]
    pub fn retry_when(&self, separator_available: bool) -> bool {
        match self {
            StemState::NeedsSeparator => separator_available,
            StemState::Failed { retryable, .. } => *retryable,
            StemState::Ready | StemState::Instrumental | StemState::NoReliableVocals => false,
        }
    }

    /// Whether the song's vocals can be played.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, StemState::Ready)
    }

    /// One short line for the song browser.
    #[must_use]
    pub fn summary(&self) -> &str {
        match self {
            StemState::Ready => "ready",
            StemState::Instrumental => "instrumental",
            StemState::NoReliableVocals => "no clear vocals",
            StemState::NeedsSeparator => "separator missing",
            StemState::Failed { .. } => "failed",
        }
    }
}

/// What a song's separation produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemManifest {
    /// [`STEM_SCHEMA`].
    pub schema: String,
    /// [`STEM_PIPELINE_VERSION`] at the time of writing.
    pub pipeline_version: u32,
    /// SHA-256 of the audio the stems were made from.
    pub audio_sha256: String,
    /// The separator and model, e.g. `demucs htdemucs`.
    pub separator: String,
    /// How it went.
    #[serde(flatten)]
    pub state: StemState,
    /// The stems on disk. Empty unless [`StemState::Ready`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stems: Vec<StemFile>,
}

impl StemManifest {
    /// A manifest recording `state` for this build's pipeline.
    #[must_use]
    pub fn new(audio_sha256: &str, separator: &str, state: StemState) -> StemManifest {
        StemManifest {
            schema: STEM_SCHEMA.to_owned(),
            pipeline_version: STEM_PIPELINE_VERSION,
            audio_sha256: audio_sha256.to_owned(),
            separator: separator.to_owned(),
            state,
            stems: Vec::new(),
        }
    }

    /// The recorded stem of a kind.
    #[must_use]
    pub fn stem(&self, kind: StemKind) -> Option<&StemFile> {
        self.stems.iter().find(|s| s.kind == kind)
    }

    /// The path of a stem of a kind, given the song's audio path.
    ///
    /// Only a plain file name is accepted from the manifest: a
    /// manifest is a file on disk like any other, and a `file` of
    /// `../../something` must not resolve outside the stem directory.
    #[must_use]
    pub fn stem_path(&self, audio_path: &Path, kind: StemKind) -> Option<PathBuf> {
        let stem = self.stem(kind)?;
        let name = Path::new(&stem.file);
        let plain = name.file_name()? == name.as_os_str();
        plain.then(|| stems_dir(audio_path).join(name))
    }

    /// Total bytes the kept stems occupy.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.stems.iter().map(|s| s.bytes).sum()
    }

    /// Whether this manifest describes the current pipeline run
    /// against `audio_sha256`.
    #[must_use]
    pub fn is_current_for(&self, audio_sha256: &str) -> bool {
        self.schema == STEM_SCHEMA
            && self.pipeline_version == STEM_PIPELINE_VERSION
            && self.audio_sha256 == audio_sha256
    }

    /// Whether every stem the manifest claims is actually on disk.
    ///
    /// A `Ready` manifest whose files someone has deleted must not
    /// read as ready — that is a state the player can create with a
    /// file manager, so it is checked rather than trusted.
    #[must_use]
    pub fn files_present(&self, audio_path: &Path) -> bool {
        self.stems.iter().all(|s| {
            self.stem_path(audio_path, s.kind)
                .is_some_and(|p| p.is_file())
        })
    }
}

/// Where a song's stem manifest lives: `<audio stem>.stems.json`.
#[must_use]
pub fn stems_manifest_path(audio_path: &Path) -> PathBuf {
    audio_path.with_file_name(format!("{}.stems.json", file_stem(audio_path)))
}

/// Where a song's stem audio lives: `<audio stem>.stems/`.
#[must_use]
pub fn stems_dir(audio_path: &Path) -> PathBuf {
    audio_path.with_file_name(format!("{}.stems", file_stem(audio_path)))
}

fn file_stem(audio_path: &Path) -> String {
    audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Read a song's stem manifest, if one is beside the audio and this
/// build understands it.
///
/// A manifest of another schema is `None`, not an error: an older
/// build must not choke on a newer sidecar, and a newer build treats
/// an unreadable one as "not separated yet".
#[must_use]
pub fn read_manifest(audio_path: &Path) -> Option<StemManifest> {
    let text = std::fs::read_to_string(stems_manifest_path(audio_path)).ok()?;
    let manifest: StemManifest = serde_json::from_str(&text).ok()?;
    (manifest.schema == STEM_SCHEMA).then_some(manifest)
}

/// Write a song's stem manifest, atomically.
///
/// Through `<name>.part` and a rename, for the reason every sidecar
/// here is: a manifest truncated by a crash would claim stems that
/// are not there, and the state it records is exactly what stops the
/// work being redone.
pub fn write_manifest(audio_path: &Path, manifest: &StemManifest) -> std::io::Result<()> {
    let path = stems_manifest_path(audio_path);
    let text = serde_json::to_string_pretty(manifest).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("json.part");
    std::fs::write(&part, text)?;
    std::fs::rename(&part, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/// Whether the background queue should separate this song now.
///
/// The one place the decision lives, so a library scan, an import and
/// the command line cannot disagree about it. Pure — tested.
#[must_use]
pub fn wants_separation(
    manifest: Option<&StemManifest>,
    audio_sha256: &str,
    files_present: bool,
    separator_available: bool,
) -> bool {
    match manifest {
        // Never separated: do it, if we can.
        None => separator_available,
        Some(m) if !m.is_current_for(audio_sha256) => separator_available,
        // Ready but the files are gone — the player deleted them, or
        // a disk filled mid-write.
        Some(m) if m.state.is_ready() && !files_present => separator_available,
        Some(m) => m.state.retry_when(separator_available),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn ready() -> StemManifest {
        let mut m = StemManifest::new(HASH, "demucs htdemucs", StemState::Ready);
        m.stems = vec![
            StemFile {
                kind: StemKind::Vocals,
                file: "vocals.wav".to_owned(),
                sample_rate: 44_100,
                channels: 2,
                duration_s: 180.0,
                bytes: 31_752_000,
            },
            StemFile {
                kind: StemKind::Instrumental,
                file: "instrumental.wav".to_owned(),
                sample_rate: 44_100,
                channels: 2,
                duration_s: 180.0,
                bytes: 31_752_000,
            },
        ];
        m
    }

    #[test]
    fn the_sidecars_sit_beside_the_audio() {
        let audio = Path::new("/songs/x/Artist - Title.m4a");
        assert_eq!(
            stems_manifest_path(audio),
            PathBuf::from("/songs/x/Artist - Title.stems.json")
        );
        assert_eq!(
            stems_dir(audio),
            PathBuf::from("/songs/x/Artist - Title.stems")
        );
        assert_eq!(
            ready().stem_path(audio, StemKind::Vocals),
            Some(PathBuf::from("/songs/x/Artist - Title.stems/vocals.wav"))
        );
        assert_eq!(ready().stem_path(audio, StemKind::Other), None, "not kept");
    }

    #[test]
    fn a_manifest_cannot_point_out_of_its_own_directory() {
        // The manifest is a file on disk; a hand-edited or corrupt one
        // must not become a path traversal.
        let mut m = ready();
        m.stems[0].file = "../../../etc/passwd".to_owned();
        assert_eq!(
            m.stem_path(Path::new("/songs/x/a.m4a"), StemKind::Vocals),
            None
        );
        m.stems[0].file = "sub/vocals.wav".to_owned();
        assert_eq!(
            m.stem_path(Path::new("/songs/x/a.m4a"), StemKind::Vocals),
            None
        );
    }

    #[test]
    fn the_settled_answers_are_the_ones_worth_writing_down() {
        assert!(StemState::Ready.is_settled());
        assert!(StemState::Instrumental.is_settled());
        assert!(StemState::NoReliableVocals.is_settled());
        assert!(
            StemState::Failed {
                reason: "not audio".to_owned(),
                retryable: false,
            }
            .is_settled()
        );
        // These two are about the machine and the moment, not the song.
        assert!(!StemState::NeedsSeparator.is_settled());
        assert!(
            !StemState::Failed {
                reason: "killed".to_owned(),
                retryable: true,
            }
            .is_settled()
        );
    }

    #[test]
    fn a_known_answer_is_never_paid_for_twice() {
        // The point of the whole state machine: an instrumental song
        // does not get separated again on every start.
        assert!(!wants_separation(
            Some(&StemManifest::new(HASH, "d", StemState::Instrumental)),
            HASH,
            true,
            true
        ));
        assert!(!wants_separation(
            Some(&StemManifest::new(HASH, "d", StemState::NoReliableVocals)),
            HASH,
            true,
            true
        ));
        assert!(!wants_separation(Some(&ready()), HASH, true, true));
        let hard = StemManifest::new(
            HASH,
            "d",
            StemState::Failed {
                reason: "cannot decode".to_owned(),
                retryable: false,
            },
        );
        assert!(!wants_separation(Some(&hard), HASH, true, true));
    }

    #[test]
    fn the_questions_still_worth_asking_are_asked() {
        // Never tried.
        assert!(wants_separation(None, HASH, false, true));
        assert!(
            !wants_separation(None, HASH, false, false),
            "no tool, no job"
        );

        // The tool arrived since last time.
        let waiting = StemManifest::new(HASH, "d", StemState::NeedsSeparator);
        assert!(wants_separation(Some(&waiting), HASH, false, true));
        assert!(!wants_separation(Some(&waiting), HASH, false, false));

        // A transient failure.
        let soft = StemManifest::new(
            HASH,
            "d",
            StemState::Failed {
                reason: "killed".to_owned(),
                retryable: true,
            },
        );
        assert!(wants_separation(Some(&soft), HASH, false, true));

        // The audio changed under a stale manifest.
        assert!(wants_separation(
            Some(&ready()),
            &"b".repeat(64),
            true,
            true
        ));

        // Ready, but someone deleted the stems.
        assert!(wants_separation(Some(&ready()), HASH, false, true));
    }

    #[test]
    fn the_state_rides_along_in_the_json_and_round_trips() {
        for state in [
            StemState::Ready,
            StemState::Instrumental,
            StemState::NoReliableVocals,
            StemState::NeedsSeparator,
            StemState::Failed {
                reason: "x".to_owned(),
                retryable: true,
            },
        ] {
            let m = StemManifest::new(HASH, "demucs htdemucs", state.clone());
            let json = serde_json::to_string(&m).unwrap();
            let back: StemManifest = serde_json::from_str(&json).unwrap();
            assert_eq!(back, m, "{json}");
        }
        let r = ready();
        assert_eq!(r.bytes(), 63_504_000);
        assert!(r.is_current_for(HASH));
        assert_eq!(r.state.summary(), "ready");
    }

    #[test]
    fn a_manifest_that_claims_missing_files_is_not_ready() {
        let dir = std::env::temp_dir().join(format!("bb-stems-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("song.wav");
        let m = ready();
        assert!(!m.files_present(&audio), "nothing written yet");

        std::fs::create_dir_all(stems_dir(&audio)).unwrap();
        std::fs::write(stems_dir(&audio).join("vocals.wav"), b"x").unwrap();
        assert!(!m.files_present(&audio), "only one of the two");
        std::fs::write(stems_dir(&audio).join("instrumental.wav"), b"x").unwrap();
        assert!(m.files_present(&audio));

        write_manifest(&audio, &m).unwrap();
        assert!(
            !stems_manifest_path(&audio)
                .with_extension("json.part")
                .is_file()
        );
        assert_eq!(read_manifest(&audio), Some(m));

        // A foreign schema reads as "nothing here", not as an error.
        std::fs::write(stems_manifest_path(&audio), r#"{"schema":"other/9"}"#).unwrap();
        assert_eq!(read_manifest(&audio), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
