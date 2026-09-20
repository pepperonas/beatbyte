//! `beatbyte-cli vocals …` — make (or inspect) a song's vocal chart.
//!
//! The command line runs the same job the game's background chore
//! runs, through the same `beatbyte_audio::stems::analyse_song`, so
//! the two cannot drift into producing different charts from the same
//! song. What lives here is the part that belongs to a terminal:
//! choosing what to work on, printing a table, and deciding what an
//! exit code should say.
//!
//! Separation costs minutes of a saturated machine per song, so the
//! default is to do nothing that has already been done — the stem
//! manifest's state is consulted first and `--force` is the only way
//! past it.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::singing::SingingConfig;
use beatbyte_audio::stems::{
    self, StemKind, StemState, VocalWork, analyse_existing_stems, analyse_song, audio_sha256,
    read_manifest, wants_separation,
};
use beatbyte_chart::vocals::{
    VocalChartFile, VocalProvenance, load_vocals, save_vocals, vocals_path,
};
use beatbyte_core::vocal::{VocalPart, VocalRole};

use crate::loudness::audio_of;

/// What the command was asked to do.
pub struct Args {
    /// Work on every song folder under the path.
    pub all: bool,
    /// Analyse even where a current chart or a settled state exists.
    pub force: bool,
    /// Report what is there without running anything.
    pub status: bool,
}

/// The build's name in a chart's provenance.
fn analyzer() -> String {
    format!("beatbyte-singing {}", env!("CARGO_PKG_VERSION"))
}

/// Run over one path, or every song folder beneath it.
pub fn run(path: &Path, args: &Args) -> ExitCode {
    let songs = if args.all {
        folders(path)
    } else {
        vec![path.to_path_buf()]
    };
    if songs.is_empty() {
        eprintln!("no song folders under `{}`", path.display());
        return ExitCode::FAILURE;
    }
    if !args.status && !stems::separator_available() {
        // Not an error: it is a state, and saying so beats a wall of
        // identical failures.
        println!(
            "{} is not installed - `pipx install {}`",
            beatbyte_audio::separate::DEMUCS,
            beatbyte_audio::separate::DEMUCS
        );
        println!("nothing was analysed; the songs are unchanged and still playable.");
        return ExitCode::FAILURE;
    }
    let mut failed = 0usize;
    for song in &songs {
        match one(song, args) {
            Ok(line) => println!("{line}"),
            Err(reason) => {
                eprintln!("{}: {reason}", name_of(song));
                failed += 1;
            }
        }
    }
    if songs.len() > 1 {
        println!("\n{} song(s), {failed} could not be read", songs.len());
    }
    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn one(song: &Path, args: &Args) -> Result<String, String> {
    let audio = audio_of(song)?;
    let hash = audio_sha256(&audio).map_err(|error| format!("cannot hash the audio: {error}"))?;
    let manifest = read_manifest(&audio);
    let existing = load_vocals(&vocals_path(&audio)).ok();
    let name = name_of(song);

    if args.status {
        return Ok(status_line(
            &name,
            manifest.as_ref(),
            existing.as_ref(),
            &hash,
        ));
    }

    let current = existing.as_ref().is_some_and(|f| f.is_current_for(&hash));
    if current && !args.force {
        let notes: usize = existing
            .as_ref()
            .map(|f| f.parts.iter().map(VocalPart::note_count).sum())
            .unwrap_or_default();
        return Ok(format!("{name}: already charted ({notes} notes)"));
    }
    let settled = manifest
        .as_ref()
        .is_some_and(|m| m.is_current_for(&hash) && m.state.is_settled() && !m.state.is_ready());
    if settled && !args.force {
        let state = manifest
            .map(|m| m.state.summary().to_owned())
            .unwrap_or_default();
        return Ok(format!("{name}: {state} (settled; --force to re-run)"));
    }

    // Separating again is minutes; re-reading kept stems is seconds.
    // A better segmentation should re-chart a library over a coffee.
    if let Some(work) = analyse_existing_stems(&audio, &SingingConfig::default()) {
        return file_it(&audio, &name, work).map(|line| format!("{line} [kept stems]"));
    }
    let scratch = std::env::temp_dir()
        .join("beatbyte-vocals")
        .join(sanitised(&name));
    let work = analyse_song(&audio, &scratch, &SingingConfig::default(), |_other| {});
    file_it(&audio, &name, work)
}

fn file_it(audio: &Path, name: &str, work: VocalWork) -> Result<String, String> {
    match &work.state {
        StemState::Ready => {}
        StemState::Instrumental => return Ok(format!("{name}: instrumental, nothing to sing")),
        StemState::NoReliableVocals => {
            return Ok(format!(
                "{name}: vocals found but none clear enough to chart"
            ));
        }
        StemState::NeedsSeparator => return Err("the separator vanished mid-run".to_owned()),
        StemState::Failed { reason, retryable } => {
            let again = if *retryable { ", retryable" } else { "" };
            return Err(format!("{reason}{again}"));
        }
    }
    let notes: usize = work.phrases.iter().map(|p| p.notes.len()).sum();
    let part = VocalPart {
        id: "lead".to_owned(),
        role: VocalRole::Lead,
        name: None,
        phrases: work.phrases,
    };
    let range = part.pitch_range();
    let file = VocalChartFile::new(
        &work.audio_sha256,
        VocalProvenance {
            separator: work.separator,
            analyzer: analyzer(),
            aligner: None,
        },
        vec![part],
    );
    // A chart that does not validate is never written: a bad target
    // is worse than no target, and the game would have to refuse it
    // at load time anyway.
    let problems: Vec<String> = file
        .validate()
        .iter()
        .filter(|i| i.severity == beatbyte_chart::Severity::Error)
        .map(std::string::ToString::to_string)
        .collect();
    if !problems.is_empty() {
        return Err(format!(
            "the chart does not validate: {}",
            problems.join("; ")
        ));
    }
    save_vocals(&vocals_path(audio), &file).map_err(|error| error.to_string())?;
    let span = range.map_or_else(
        || "no range".to_owned(),
        |r| format!("{:.0}-{:.0} MIDI", r.low_midi, r.high_midi),
    );
    Ok(format!(
        "{name}: {notes} notes in {} phrases, {span}",
        file.parts.first().map_or(0, |p| p.phrases.len())
    ))
}

fn status_line(
    name: &str,
    manifest: Option<&stems::StemManifest>,
    chart: Option<&VocalChartFile>,
    hash: &str,
) -> String {
    let stem_state = manifest.map_or_else(
        || "not separated".to_owned(),
        |m| {
            if m.is_current_for(hash) {
                m.state.summary().to_owned()
            } else {
                format!("{} (stale)", m.state.summary())
            }
        },
    );
    let chart_state = chart.map_or_else(
        || "none".to_owned(),
        |c| {
            let notes: usize = c.parts.iter().map(VocalPart::note_count).sum();
            if c.is_current_for(hash) {
                format!("{notes} notes")
            } else {
                format!("{notes} notes (stale)")
            }
        },
    );
    let separator = stems::separator_available();
    let queued = wants_separation(manifest, hash, true, separator);
    let kept = manifest.map_or(0, |m| {
        u64::from(m.stem(StemKind::Vocals).is_some())
            + u64::from(m.stem(StemKind::Instrumental).is_some())
    });
    format!(
        "{name}: stems {stem_state}, {kept} kept, chart {chart_state}{}",
        if queued { ", queued" } else { "" }
    )
}

fn folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(beatbyte_chart::versions::BASE_CHART).is_file())
        .collect();
    found.sort();
    found
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A scratch directory name that cannot escape its parent.
fn sanitised(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(64)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_name_cannot_escape_its_parent() {
        // The property, not a dash count: nothing that could build a
        // path survives, and the tail is still recognisable.
        assert_eq!(sanitised("../../etc"), "------etc");
        assert!(!sanitised("../../etc").contains('.'));
        assert_eq!(sanitised("Böhse Onkelz - Mexico"), "B-hse-Onkelz---Mexico");
        assert!(sanitised(&"x".repeat(200)).len() <= 64);
        assert!(!sanitised("a/b\\c").contains(['/', '\\']));
    }

    #[test]
    fn the_status_line_says_what_is_there_without_running_anything() {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let line = status_line("Song", None, None, hash);
        assert!(line.contains("stems not separated"), "{line}");
        assert!(line.contains("chart none"), "{line}");

        let settled = stems::StemManifest::new(hash, "demucs", StemState::Instrumental);
        let line = status_line("Song", Some(&settled), None, hash);
        assert!(line.contains("instrumental"), "{line}");
        assert!(
            !line.contains("queued"),
            "a settled answer is not queued: {line}"
        );

        // A manifest for different audio is stale whatever it says.
        let line = status_line("Song", Some(&settled), None, &"a".repeat(64));
        assert!(line.contains("stale"), "{line}");
    }
}
