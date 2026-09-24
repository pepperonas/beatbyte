//! `beatbyte-cli poly <folder>` — hear which notes a song strikes
//! together, and write it down beside the audio (behind `ml`).
//!
//! The one job here that needs the model: the classic `chords`
//! ingredient reads what this writes and never runs a model itself.
//! Basic Pitch hears best one instrument at a time, so the song is
//! separated first and the guitar-ish `other` stem is transcribed —
//! the mix, where bass, keys and a voice would all read as chord
//! tones, only when asked for with `--mix`.
//!
//! A separation costs minutes of a saturated machine, so a song that
//! already has its sidecar is skipped unless `--force`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use beatbyte_audio::decode::{AudioData, decode_file, decode_file_channels};
use beatbyte_chart::classic::chords::evidence_at;
use beatbyte_chart::poly::{POLY_FORMAT, PolyFile, PolyNote, poly_path, save_poly};
use beatbyte_chart::{load_chart_file, versions};
use beatbyte_core::Difficulty;
use beatbyte_ml::{ModelStore, Runtime};

use crate::loudness::audio_of;

/// What the command was asked to do.
pub struct Args {
    /// Every song folder under the path.
    pub all: bool,
    /// Transcribe again where a sidecar exists.
    pub force: bool,
    /// Transcribe the mix instead of the separated `other` stem.
    pub mix: bool,
}

/// Run over one folder, or every song folder beneath it.
pub fn run(path: &Path, args: &Args) -> ExitCode {
    let Some(store) = ModelStore::default_location() else {
        eprintln!("no config directory on this platform; models cannot be stored");
        return ExitCode::from(2);
    };
    if !beatbyte_poly::installed(&store) {
        eprintln!(
            "`basic-pitch` is not installed — `beatbyte-cli models install basic-pitch` \
             (0.2 MB, Apache-2.0)"
        );
        return ExitCode::from(2);
    }
    if !args.mix && !beatbyte_audio::separate::available() {
        eprintln!(
            "{} is not installed — `pipx install {}`, or `--mix` to transcribe the mix \
             (bass, keys and voice will read as chord tones there)",
            beatbyte_audio::separate::DEMUCS,
            beatbyte_audio::separate::DEMUCS
        );
        return ExitCode::from(2);
    }
    let folders = if args.all {
        let mut found: Vec<PathBuf> = std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    } else {
        vec![path.to_path_buf()]
    };
    let runtime = Runtime::new();
    let mut failed = 0usize;
    for folder in &folders {
        match one(folder, args, &runtime, &store) {
            Ok(line) => println!("{line}"),
            Err(reason) => {
                eprintln!("{}: {reason}", folder.display());
                failed += 1;
            }
        }
    }
    if folders.len() > 1 {
        println!("{} folder(s), {failed} failed", folders.len());
    }
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn one(
    folder: &Path,
    args: &Args,
    runtime: &Runtime,
    store: &ModelStore,
) -> Result<String, String> {
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let audio_path = audio_of(folder)?;
    let out = poly_path(&audio_path);
    if out.is_file() && !args.force {
        return Ok(format!("{name}: already has `{}`", file_name(&out)));
    }
    let started = std::time::Instant::now();
    let (audio, source) = if args.mix {
        let audio = decode_file(&audio_path).map_err(|error| format!("cannot decode: {error}"))?;
        (audio, "mix".to_owned())
    } else {
        separated(&audio_path, &name)?
    };
    let transcription = beatbyte_poly::transcribe(
        runtime,
        store,
        &audio,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
    .map_err(|error| format!("cannot transcribe: {error}"))?;
    let file = PolyFile {
        format: POLY_FORMAT.to_owned(),
        model: transcription.model.to_owned(),
        model_sha256: transcription.sha256.to_owned(),
        source,
        notes: transcription
            .notes
            .iter()
            .map(|n| PolyNote {
                // A millisecond is finer than the model's 12 ms frames.
                start_s: (n.start_s * 1000.0).round() / 1000.0,
                end_s: (n.end_s * 1000.0).round() / 1000.0,
                midi: n.midi,
                amplitude: (n.amplitude * 1000.0).round() / 1000.0,
            })
            .collect(),
    };
    if let Some(problem) = file.problem() {
        return Err(format!("the transcription is not sane: {problem}"));
    }
    save_poly(&out, &file).map_err(|error| error.to_string())?;
    Ok(format!(
        "{name}: {} notes from the {} in {:.0} s → `{}`{}",
        file.notes.len(),
        file.source,
        started.elapsed().as_secs_f64(),
        file_name(&out),
        evidence_line(folder, &file)
    ))
}

/// The `other` stem of a separation of the decoded song — decoded, so
/// it lies on the timeline the charts use.
fn separated(audio_path: &Path, name: &str) -> Result<(AudioData, String), String> {
    let channels =
        decode_file_channels(audio_path).map_err(|error| format!("cannot decode: {error}"))?;
    let scratch = std::env::temp_dir().join("beatbyte-poly").join(name);
    let separation = beatbyte_audio::separate::separate(&channels, &scratch, Some("other"))
        .map_err(|error| format!("cannot separate: {error}"));
    let stem = separation.and_then(|s| {
        decode_file(&s.source("other")).map_err(|error| format!("cannot read the stem: {error}"))
    });
    let _ = std::fs::remove_dir_all(&scratch);
    Ok((
        stem?,
        format!("other stem ({})", beatbyte_audio::separate::DEMUCS_MODEL),
    ))
}

/// How much of the active chart's Expert the evidence reaches — the
/// number that says whether the chords ingredient will have anything
/// to do here.
fn evidence_line(folder: &Path, poly: &PolyFile) -> String {
    let Ok(names) = beatbyte_chart::twin::names_in(folder) else {
        return String::new();
    };
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active = versions::resolve_active(pointer.as_deref(), &names);
    let Ok(chart) = load_chart_file(&folder.join(active)) else {
        return String::new();
    };
    let Some(expert) = chart
        .charts
        .iter()
        .find(|c| c.difficulty == Difficulty::Expert)
    else {
        return String::new();
    };
    let events = beatbyte_chart::classic::ladder::event_times(&expert.notes);
    let chords = events
        .iter()
        .filter(|t| evidence_at(poly, **t).is_some())
        .count();
    format!(
        "; chord evidence at {chords} of {} Expert events ({:.0} %)",
        events.len(),
        100.0 * chords as f64 / events.len().max(1) as f64
    )
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
