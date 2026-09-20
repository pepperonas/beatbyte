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
use sha2::{Digest, Sha256};

use crate::decode::{Channels, decode_file, decode_file_channels};
use crate::separate::{SOURCES, SeparateError, Separation, separate};

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

// ---------------------------------------------------------------
// Making the stems
// ---------------------------------------------------------------

/// The level a 50 ms frame of the vocal stem must reach to count as
/// singing.
pub const VOCAL_GATE_DBFS: f32 = -45.0;

/// Below this share of the song, the "vocal" stem is separator
/// bleed rather than a singer, and the song is recorded as
/// [`StemState::Instrumental`].
pub const INSTRUMENTAL_BELOW: f32 = 0.01;

/// SHA-256 of a file, lowercase hex — what ties a sidecar to the
/// exact bytes it was computed from.
///
/// Read in chunks: a song is tens of megabytes and this runs on the
/// import path.
pub fn audio_sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use core::fmt::Write;
        // `write!` into a String cannot fail; the digest is 32 bytes.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// The share of a signal that is above [`VOCAL_GATE_DBFS`], measured
/// in 50 ms frames.
///
/// This is the one question the separator can answer on its own:
/// *is anyone singing at all*. Whether what they sang can be charted
/// is the analyser's question, and it gets its own state. Pure —
/// tested.
#[must_use]
pub fn vocal_presence(samples: &[f32], sample_rate: u32) -> f32 {
    let frame = (sample_rate.max(1) as usize / 20).max(1);
    if samples.len() < frame {
        return 0.0;
    }
    let mut loud = 0usize;
    let mut total = 0usize;
    for chunk in samples.chunks(frame) {
        if chunk.len() < frame {
            break;
        }
        total += 1;
        if crate::pitch::level(chunk).0 > VOCAL_GATE_DBFS {
            loud += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    loud as f32 / total as f32
}

/// Run the separator over a song, on BeatByte's own decode.
///
/// Errors come back as the [`StemState`] they mean, because every one
/// of them is a fact about this song or this machine that is worth
/// writing down rather than retrying for ever.
pub fn separate_song(audio_path: &Path, scratch: &Path) -> Result<Separation, StemState> {
    let audio = decode_file_channels(audio_path).map_err(|error| StemState::Failed {
        reason: format!("cannot decode the song: {error}"),
        retryable: false,
    })?;
    separate(&audio, scratch, None).map_err(|error| match error {
        SeparateError::NotInstalled => StemState::NeedsSeparator,
        // A run that died and a stem that is not there could both be
        // a full disk or a killed process; both are worth one retry.
        other => StemState::Failed {
            reason: other.to_string(),
            retryable: true,
        },
    })
}

/// Keep the two stems vocal play needs, beside the song, and write
/// the manifest that describes them.
///
/// The separator's `other` stem is deliberately **not** kept: the
/// Guitar Study twin reads it out of the same run before the scratch
/// is cleaned, so one separation serves both and the library pays for
/// two files rather than four.
pub fn persist(
    audio_path: &Path,
    audio_sha256: &str,
    separation: &Separation,
) -> Result<StemManifest, std::io::Error> {
    let separator = format!(
        "demucs {} ({})",
        crate::separate::DEMUCS_MODEL,
        separation.device
    );
    let vocals = decode_file_channels(&separation.source("vocals"))
        .map_err(|error| std::io::Error::other(format!("reading the vocal stem: {error}")))?;

    let presence = vocal_presence(&vocals.mono(), vocals.sample_rate);
    if presence < INSTRUMENTAL_BELOW {
        // Nothing to keep and nothing to analyse. Writing this down
        // is the point: the next library scan reads the answer
        // instead of paying for the separator again.
        let manifest = StemManifest::new(audio_sha256, &separator, StemState::Instrumental);
        write_manifest(audio_path, &manifest)?;
        return Ok(manifest);
    }

    let dir = stems_dir(audio_path);
    std::fs::create_dir_all(&dir)?;
    let mut manifest = StemManifest::new(audio_sha256, &separator, StemState::Ready);

    let vocals_path = dir.join(StemKind::Vocals.file_name());
    crate::decode::write_wav16(&vocals_path, &vocals)?;
    manifest
        .stems
        .push(stem_file(StemKind::Vocals, &vocals, &vocals_path)?);

    let backing = sum_sources(separation, &["drums", "bass", "other"])
        .map_err(|error| std::io::Error::other(format!("summing the backing: {error}")))?;
    let backing_path = dir.join(StemKind::Instrumental.file_name());
    crate::decode::write_wav16(&backing_path, &backing)?;
    manifest
        .stems
        .push(stem_file(StemKind::Instrumental, &backing, &backing_path)?);

    write_manifest(audio_path, &manifest)?;
    Ok(manifest)
}

fn stem_file(kind: StemKind, audio: &Channels, path: &Path) -> std::io::Result<StemFile> {
    Ok(StemFile {
        kind,
        file: kind.file_name().to_owned(),
        sample_rate: audio.sample_rate,
        channels: u32::try_from(audio.channels).unwrap_or(1),
        duration_s: audio.duration_s(),
        bytes: std::fs::metadata(path)?.len(),
    })
}

/// Add several of the separator's sources together, sample for
/// sample.
///
/// The sources come from one run so they agree on rate and width, but
/// the shortest one decides the length rather than an index running
/// off the end — a truncated stem is a bad sum, not a panic.
fn sum_sources(separation: &Separation, names: &[&str]) -> Result<Channels, String> {
    let mut total: Option<Channels> = None;
    for name in names {
        let part = decode_file_channels(&separation.source(name))
            .map_err(|error| format!("reading `{name}`: {error}"))?;
        match &mut total {
            None => total = Some(part),
            Some(sum) => {
                if part.sample_rate != sum.sample_rate || part.channels != sum.channels {
                    return Err(format!(
                        "`{name}` is {} Hz / {} ch against {} Hz / {} ch",
                        part.sample_rate, part.channels, sum.sample_rate, sum.channels
                    ));
                }
                let len = sum.interleaved.len().min(part.interleaved.len());
                sum.interleaved.truncate(len);
                for (into, from) in sum.interleaved.iter_mut().zip(&part.interleaved[..len]) {
                    *into += *from;
                }
            }
        }
    }
    let mut sum = total.ok_or_else(|| "nothing to sum".to_owned())?;
    // The parts were separated from one mix, so their sum is that mix
    // and cannot normally clip; a model that overshoots by a hair
    // still must not wrap round to the opposite rail.
    for sample in &mut sum.interleaved {
        *sample = sample.clamp(-1.0, 1.0);
    }
    Ok(sum)
}

/// Whether this machine can separate a song at all.
///
/// Re-exported here so a caller deciding what vocal work to queue
/// does not have to know which tool does the separating.
#[must_use]
pub fn separator_available() -> bool {
    crate::separate::available()
}

/// Everything one separation run produced, for the caller to file.
#[derive(Debug, Clone, PartialEq)]
pub struct VocalWork {
    /// What happened, in the form that gets written down.
    pub state: StemState,
    /// SHA-256 of the audio this ran on.
    pub audio_sha256: String,
    /// The separator and device, for the chart's provenance.
    pub separator: String,
    /// The phrases the singer is held to. Empty unless the state is
    /// [`StemState::Ready`].
    pub phrases: Vec<beatbyte_core::vocal::VocalPhrase>,
}

/// Read the vocal line off stems that are already on disk.
///
/// `None` when there is nothing current to read. This is what makes a
/// better analysis cheap: improving the segmentation should re-chart
/// a library in seconds, not re-separate it in hours, and the stems
/// are the expensive half.
#[must_use]
pub fn analyse_existing_stems(
    audio_path: &Path,
    config: &crate::singing::SingingConfig,
) -> Option<VocalWork> {
    let hash = audio_sha256(audio_path).ok()?;
    let manifest = read_manifest(audio_path)?;
    if !manifest.is_current_for(&hash) || !manifest.state.is_ready() {
        return None;
    }
    if !manifest.files_present(audio_path) {
        return None;
    }
    let (samples, rate) = decode_stem_at(&manifest, audio_path, StemKind::Vocals)?;
    let phrases = crate::singing::analyse(&samples, rate, config);
    let state = if phrases.is_empty() {
        StemState::NoReliableVocals
    } else {
        StemState::Ready
    };
    Some(VocalWork {
        state,
        audio_sha256: hash,
        separator: manifest.separator,
        phrases,
    })
}

/// Separate a song, keep its stems and read the vocal line off them.
///
/// The one path the command line and the game both take, so they
/// cannot drift into producing different charts from the same song.
/// Every failure is a [`StemState`] rather than an error: each one is
/// a fact worth writing down, and a song whose vocals cannot be made
/// must still be playable on guitar.
///
/// `with_other` is handed the separator's `other` stem before the
/// scratch is cleaned — that is how the Guitar Study twin rides along
/// on the same run instead of paying for a second one. It is called
/// only when the separation itself succeeded.
///
/// `scratch` is removed before returning, whatever happened.
pub fn analyse_song(
    audio_path: &Path,
    scratch: &Path,
    config: &crate::singing::SingingConfig,
    with_other: impl FnOnce(&Path),
) -> VocalWork {
    let audio_sha256 = audio_sha256(audio_path).unwrap_or_default();
    let unknown = |state: StemState| VocalWork {
        state,
        audio_sha256: audio_sha256.clone(),
        separator: format!("demucs {}", crate::separate::DEMUCS_MODEL),
        phrases: Vec::new(),
    };
    if audio_sha256.is_empty() {
        return unknown(StemState::Failed {
            reason: "cannot read the audio file".to_owned(),
            retryable: false,
        });
    }
    let separation = match separate_song(audio_path, scratch) {
        Ok(separation) => separation,
        Err(state) => {
            let _ = std::fs::remove_dir_all(scratch);
            let work = unknown(state);
            let manifest =
                StemManifest::new(&work.audio_sha256, &work.separator, work.state.clone());
            let _ = write_manifest(audio_path, &manifest);
            return work;
        }
    };
    with_other(&separation.source("other"));

    let manifest = persist(audio_path, &audio_sha256, &separation);
    let _ = std::fs::remove_dir_all(scratch);
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            // Writing failed: a full disk or a read-only folder, both
            // worth one retry.
            let work = unknown(StemState::Failed {
                reason: format!("cannot keep the stems: {error}"),
                retryable: true,
            });
            let m = StemManifest::new(&work.audio_sha256, &work.separator, work.state.clone());
            let _ = write_manifest(audio_path, &m);
            return work;
        }
    };
    let separator = manifest.separator.clone();
    if !manifest.state.is_ready() {
        // `Instrumental`: nothing was kept and there is nothing to
        // read. The manifest already says so.
        return VocalWork {
            state: manifest.state,
            audio_sha256,
            separator,
            phrases: Vec::new(),
        };
    }

    let Some(stem) = decode_stem_at(&manifest, audio_path, StemKind::Vocals) else {
        return VocalWork {
            state: StemState::Failed {
                reason: "the kept vocal stem cannot be decoded".to_owned(),
                retryable: true,
            },
            audio_sha256,
            separator,
            phrases: Vec::new(),
        };
    };
    let phrases = crate::singing::analyse(&stem.0, stem.1, config);
    if phrases.is_empty() {
        // There WAS singing in the stem — presence let it through —
        // but nothing survived the confidence and length guards. That
        // is its own answer: a wrong target is worse than no target.
        let mut settled = StemManifest::new(&audio_sha256, &separator, StemState::NoReliableVocals);
        settled.stems = manifest.stems;
        let _ = write_manifest(audio_path, &settled);
        return VocalWork {
            state: StemState::NoReliableVocals,
            audio_sha256,
            separator,
            phrases: Vec::new(),
        };
    }
    VocalWork {
        state: StemState::Ready,
        audio_sha256,
        separator,
        phrases,
    }
}

/// A stem decoded to mono, with the rate it decoded at.
fn decode_stem_at(
    manifest: &StemManifest,
    audio_path: &Path,
    kind: StemKind,
) -> Option<(Vec<f32>, u32)> {
    let path = manifest.stem_path(audio_path, kind)?;
    let audio = decode_file(&path).ok()?;
    let rate = audio.sample_rate();
    Some((audio.samples().to_vec(), rate))
}

/// Decode a stem for analysis: mono, at the stem's own rate.
pub fn decode_stem(manifest: &StemManifest, audio_path: &Path, kind: StemKind) -> Option<Vec<f32>> {
    let path = manifest.stem_path(audio_path, kind)?;
    decode_file(&path)
        .ok()
        .map(|audio| audio.samples().to_vec())
}

/// Every source name a full run produces — re-exported so a caller
/// that wants the Guitar Study stem out of a shared run does not
/// have to know the separator's vocabulary.
#[must_use]
pub fn source_names() -> &'static [&'static str] {
    SOURCES
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
    fn presence_tells_a_singer_from_separator_bleed() {
        let rate = 16_000u32;
        let tone = |seconds: f32, amplitude: f32| -> Vec<f32> {
            (0..(seconds * rate as f32) as usize)
                .map(|i| {
                    amplitude * (core::f32::consts::TAU * 220.0 * i as f32 / rate as f32).sin()
                })
                .collect()
        };
        // Silence is nobody.
        assert_eq!(vocal_presence(&vec![0.0; rate as usize * 4], rate), 0.0);
        // A loud stem throughout is a singer.
        assert!(vocal_presence(&tone(4.0, 0.3), rate) > 0.99);
        // Bleed: audible in the meter, far under the gate.
        let bleed = tone(4.0, 0.001);
        assert!(
            vocal_presence(&bleed, rate) < INSTRUMENTAL_BELOW,
            "bleed read as {}",
            vocal_presence(&bleed, rate)
        );
        // One short line in a long instrumental still counts as
        // singing — the separator's job is "is anyone there", not
        // "is there enough to chart".
        let mut sparse = vec![0.0f32; rate as usize * 60];
        sparse[..rate as usize * 3].copy_from_slice(&tone(3.0, 0.3));
        let share = vocal_presence(&sparse, rate);
        assert!(
            share > INSTRUMENTAL_BELOW,
            "three seconds of singing in a minute read as instrumental ({share})"
        );
        // Too short to measure is not a claim.
        assert_eq!(vocal_presence(&[0.5; 10], rate), 0.0);
    }

    #[test]
    fn the_audio_hash_is_the_usual_sha256_of_the_bytes() {
        let dir = std::env::temp_dir().join(format!("bb-hash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.bin");
        std::fs::write(&path, b"").unwrap();
        // The empty digest, which is a value anyone can check.
        assert_eq!(
            audio_sha256(&path).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Bigger than one read buffer, so the chunking is exercised.
        std::fs::write(&path, vec![7u8; 200_000]).unwrap();
        let big = audio_sha256(&path).unwrap();
        assert_eq!(big.len(), 64);
        std::fs::write(&path, vec![7u8; 200_001]).unwrap();
        assert_ne!(audio_sha256(&path).unwrap(), big, "one byte changes it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_backing_is_the_sum_of_the_sources_and_cannot_wrap() {
        let dir = std::env::temp_dir().join(format!("bb-sum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sep = Separation {
            dir: dir.clone(),
            device: "cpu".to_owned(),
        };
        let write = |name: &str, value: f32, frames: usize| {
            let channels = Channels {
                interleaved: vec![value; frames * 2],
                channels: 2,
                sample_rate: 44_100,
                truncated: false,
            };
            crate::decode::write_wav16(&sep.source(name), &channels).unwrap();
        };
        write("drums", 0.2, 1000);
        write("bass", 0.3, 1000);
        write("other", 0.1, 1000);
        let sum = sum_sources(&sep, &["drums", "bass", "other"]).unwrap();
        assert_eq!(sum.channels, 2);
        assert_eq!(sum.sample_rate, 44_100);
        // 0.2 + 0.3 + 0.1, through 16-bit quantisation.
        assert!(
            (sum.interleaved[0] - 0.6).abs() < 1e-3,
            "{}",
            sum.interleaved[0]
        );

        // A shorter source truncates rather than running off the end.
        write("bass", 0.3, 400);
        let short = sum_sources(&sep, &["drums", "bass", "other"]).unwrap();
        assert_eq!(short.frames(), 400);

        // Sources that disagree about their format are refused, not
        // interleaved into noise.
        let odd = Channels {
            interleaved: vec![0.1; 1000],
            channels: 1,
            sample_rate: 44_100,
            truncated: false,
        };
        crate::decode::write_wav16(&sep.source("bass"), &odd).unwrap();
        assert!(sum_sources(&sep, &["drums", "bass"]).is_err());

        // And a sum that would overshoot is clamped, never wrapped.
        write("drums", 0.9, 100);
        write("bass", 0.9, 100);
        let hot = sum_sources(&sep, &["drums", "bass"]).unwrap();
        assert!(
            hot.interleaved.iter().all(|s| (-1.0..=1.0).contains(s)),
            "a clipped sum must not wrap to the opposite rail"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
