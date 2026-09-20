//! Source separation through a local Demucs installation.
//!
//! One run, three consumers. Before this module the Guitar Study twin
//! asked Demucs for the `other` stem and nothing else; vocal play
//! needs `vocals` for analysis and everything-but-vocals for the
//! karaoke backing. Since htdemucs computes all four sources whatever
//! you ask it for, asking for all four costs the same minutes and
//! saves running it twice.
//!
//! ## Why the tool is not bundled
//!
//! ADR-0014 permits a local vocal stem as pipeline input but does not
//! permit BeatByte to ship a separator while the model weights'
//! licensing is unclear. So Demucs is a tool on the player's machine,
//! not a dependency of the game: without it the vocal state says so
//! (`StemState::NeedsSeparator`), the import is what it always was,
//! and nothing fails.
//!
//! ## Timeline
//!
//! The separator is fed BeatByte's own decode, not the original file
//! — priming already skipped, one canonical axis — so every stem
//! lands on the timeline the charts and the clock use. Feeding it the
//! source file instead would put the stems a frame or two off the
//! charts, which is exactly the class of error nobody notices until
//! the lyrics look late.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::decode::Channels;

/// The separator's command name.
pub const DEMUCS: &str = "demucs";

/// The model the stems come from.
pub const DEMUCS_MODEL: &str = "htdemucs";

/// The devices tried, in order. Apple's GPU first where there is one;
/// the CPU always works and is what a failed `mps` falls back to.
pub const DEVICES: &[&str] = &["mps", "cpu"];

/// The four sources htdemucs produces.
pub const SOURCES: &[&str] = &["vocals", "drums", "bass", "other"];

/// What can go wrong.
#[derive(Debug, Error)]
pub enum SeparateError {
    /// Demucs is not on this machine.
    #[error("{DEMUCS} is not installed - `pipx install {DEMUCS}`")]
    NotInstalled,
    /// Every device was tried and none of them worked.
    #[error("{DEMUCS} could not separate the song (tried {tried})")]
    Run {
        /// The devices that were attempted.
        tried: String,
    },
    /// It reported success but a stem is not there.
    #[error("{DEMUCS} reported success but `{0}` is missing")]
    Missing(String),
    /// Filesystem trouble.
    #[error("{context}: {source}")]
    Io {
        /// What was being attempted.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Whether the machine can separate: `demucs --help` runs.
///
/// A version flag would be the natural probe; Demucs has none.
#[must_use]
pub fn available() -> bool {
    Command::new(DEMUCS)
        .arg("--help")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The invocation for `wav` into `out_dir` on `device`.
///
/// `two_stems` asks Demucs to sum everything else into one file —
/// what the Guitar Study twin used to do on its own. `None` writes
/// all four sources, which is what one shared run wants. Pure —
/// tested.
#[must_use]
pub fn demucs_args(
    wav: &Path,
    out_dir: &Path,
    device: &str,
    two_stems: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::with_capacity(9);
    if let Some(stem) = two_stems {
        args.push(format!("--two-stems={stem}"));
    }
    args.push("-n".to_owned());
    args.push(DEMUCS_MODEL.to_owned());
    args.push("-d".to_owned());
    args.push(device.to_owned());
    args.push("-o".to_owned());
    args.push(out_dir.display().to_string());
    args.push(wav.display().to_string());
    args
}

/// Where Demucs puts a run's stems: `<out>/<model>/<input stem>/`.
/// Pure — tested.
#[must_use]
pub fn output_dir(out_dir: &Path, wav: &Path, model: &str) -> PathBuf {
    let name = wav
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    out_dir.join(model).join(name)
}

/// A finished separation: the directory its stems are in, and the
/// device that managed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Separation {
    /// The directory holding `vocals.wav` and the rest.
    pub dir: PathBuf,
    /// The device that ran it.
    pub device: String,
}

impl Separation {
    /// The path of one source, e.g. `vocals`.
    #[must_use]
    pub fn source(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.wav"))
    }

    /// Whether every source in `SOURCES` is on disk.
    #[must_use]
    pub fn complete(&self) -> bool {
        SOURCES.iter().all(|name| self.source(name).is_file())
    }
}

/// Write `audio` as the separator's input and run it, trying each
/// device in turn.
///
/// `scratch` is emptied first and is the caller's to remove; nothing
/// here writes beside the player's song.
pub fn separate(
    audio: &Channels,
    scratch: &Path,
    two_stems: Option<&str>,
) -> Result<Separation, SeparateError> {
    if !available() {
        return Err(SeparateError::NotInstalled);
    }
    let io = |context: &str| {
        let context = context.to_owned();
        move |source| SeparateError::Io { context, source }
    };
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).map_err(io("creating the scratch directory"))?;
    let wav = scratch.join("mix.wav");
    crate::decode::write_wav16(&wav, audio).map_err(io("writing the separator's input"))?;
    let stems = scratch.join("stems");

    for device in DEVICES {
        let ran = Command::new(DEMUCS)
            .args(demucs_args(&wav, &stems, device, two_stems))
            .output()
            .is_ok_and(|out| out.status.success());
        if !ran {
            continue;
        }
        let separation = Separation {
            dir: output_dir(&stems, &wav, DEMUCS_MODEL),
            device: (*device).to_owned(),
        };
        // A successful exit code is not a file. Demucs has written
        // into a different layout before now, and a missing stem must
        // read as a failure here rather than as an empty vocal chart
        // three stages later.
        let wanted: Vec<&str> = two_stems.map_or_else(|| SOURCES.to_vec(), |stem| vec![stem]);
        if let Some(missing) = wanted
            .iter()
            .find(|name| !separation.source(name).is_file())
        {
            return Err(SeparateError::Missing((*missing).to_owned()));
        }
        return Ok(separation);
    }
    Err(SeparateError::Run {
        tried: DEVICES.join(", "),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_four_stem_invocation_asks_for_the_model_on_the_device() {
        let args = demucs_args(Path::new("/s/mix.wav"), Path::new("/s/stems"), "mps", None);
        assert_eq!(&args[0..2], ["-n", DEMUCS_MODEL]);
        assert_eq!(&args[2..4], ["-d", "mps"]);
        assert_eq!(&args[4..6], ["-o", "/s/stems"]);
        assert_eq!(args[6], "/s/mix.wav");
        assert!(
            !args.iter().any(|a| a.starts_with("--two-stems")),
            "a shared run wants all four sources, not a sum"
        );
    }

    #[test]
    fn the_two_stem_invocation_is_still_available_unchanged() {
        // The Guitar Study twin's original call, preserved so the
        // shared service can serve it without changing its behaviour.
        let args = demucs_args(
            Path::new("/s/mix.wav"),
            Path::new("/s/stems"),
            "cpu",
            Some("other"),
        );
        assert_eq!(args[0], "--two-stems=other");
        assert_eq!(&args[1..3], ["-n", DEMUCS_MODEL]);
        assert_eq!(&args[3..5], ["-d", "cpu"]);
    }

    #[test]
    fn the_output_layout_is_the_models_and_the_inputs_name() {
        assert_eq!(
            output_dir(Path::new("/s/stems"), Path::new("/s/mix.wav"), "htdemucs"),
            PathBuf::from("/s/stems/htdemucs/mix")
        );
    }

    #[test]
    fn a_separation_knows_what_it_is_missing() {
        let dir = std::env::temp_dir().join(format!("bb-sep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Separation {
            dir: dir.clone(),
            device: "cpu".to_owned(),
        };
        assert!(!s.complete());
        assert_eq!(s.source("vocals"), dir.join("vocals.wav"));
        for name in SOURCES {
            std::fs::write(s.source(name), b"x").unwrap();
        }
        assert!(s.complete());
        std::fs::remove_file(s.source("bass")).unwrap();
        assert!(!s.complete(), "one missing source is not a complete run");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_tool_is_a_named_error_rather_than_a_panic() {
        // The path a machine without demucs takes. It must be an
        // error a caller can turn into `NeedsSeparator`, and its
        // message must tell the player how to fix it.
        let text = SeparateError::NotInstalled.to_string();
        assert!(text.contains("pipx install"), "{text}");
        let text = SeparateError::Run {
            tried: "mps, cpu".to_owned(),
        }
        .to_string();
        assert!(text.contains("mps, cpu"), "{text}");
    }
}
