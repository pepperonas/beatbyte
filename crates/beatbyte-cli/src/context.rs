//! `beatbyte-cli context` — the musical context sidecar, for charts
//! that were made before there was one.
//!
//! New charts get theirs from the generator, which has the analysis
//! in its hands. A library made before this existed does not, and the
//! analysis was never persisted, so the only way back to it is to
//! read the audio again.
//!
//! ⚠️ **What that produces is the analysis of today, not the one the
//! generator saw.** The pipeline has changed since some of these
//! charts were made — a meter model arrived, the grid moved. The
//! numbers are still the right dimensions to group misses by, and
//! they are still measured from the song rather than guessed; they
//! are not a reconstruction of a historical run, and a reader should
//! not read them as one.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::{context, versions};

/// `context <chart>` — one chart file and its siblings.
pub fn run_one(chart_path: &Path) -> ExitCode {
    match build(chart_path) {
        Ok(message) => {
            println!("{}: {message}", chart_path.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{}: {message}", chart_path.display());
            ExitCode::from(1)
        }
    }
}

/// `context --all <library>` — every song folder under a directory.
pub fn run_all(dir: &Path) -> ExitCode {
    let mut folders: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(error) => {
            eprintln!("cannot list `{}`: {error}", dir.display());
            return ExitCode::from(2);
        }
    };
    folders.sort();
    let (mut done, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for folder in folders {
        let Some(chart) = any_chart(&folder) else {
            skipped += 1;
            continue;
        };
        match build(&chart) {
            Ok(message) => {
                println!("{}: {message}", folder.display());
                done += 1;
            }
            Err(message) => {
                eprintln!("{}: {message}", folder.display());
                failed += 1;
            }
        }
    }
    println!("\n{done} song(s) written, {skipped} without a chart, {failed} failed");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Any chart in a folder, for finding the song's audio.
fn any_chart(folder: &Path) -> Option<PathBuf> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".json") && !name.ends_with(".context.json"))
        .collect();
    names.sort();
    let chart = names
        .iter()
        .find(|name| versions::is_version_file(name))
        .or_else(|| names.iter().find(|name| name.ends_with(".chart.json")))
        .or_else(|| names.iter().find(|name| *name == "chart.json"))?;
    Some(folder.join(chart))
}

/// Read the song once, then write a sidecar for **every** chart
/// version in the folder.
///
/// Every version, not just the active one: a session recorded against
/// a superseded version still names it, and its evidence is only
/// joinable while that version's context exists.
fn build(chart_path: &Path) -> Result<String, String> {
    let folder = chart_path
        .parent()
        .ok_or_else(|| "a chart needs a folder".to_owned())?;
    let chart = beatbyte_chart::load_chart_file(chart_path).map_err(|error| error.to_string())?;
    let audio_path = folder.join(&chart.song.audio);
    let audio = decode_file(&audio_path)
        .map_err(|error| format!("cannot decode `{}`: {error}", audio_path.display()))?;
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    crate::meter(&mut analysis, &audio);

    let mut written = 0usize;
    let mut notes = 0usize;
    for path in charts_in(folder, chart_path) {
        let Ok(file) = beatbyte_chart::load_chart_file(&path) else {
            continue;
        };
        let built = context::context_for(&file, &analysis);
        notes += built.len();
        match context::save_context(&path, &built) {
            Ok(_) => written += 1,
            Err(error) => {
                return Err(format!("cannot write beside `{}`: {error}", path.display()));
            }
        }
    }
    Ok(format!("{written} version(s), {notes} note context(s)"))
}

/// Every chart file in the folder: the versions, plus whatever the
/// caller named if it is not one of them.
fn charts_in(folder: &Path, named: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .is_some_and(|name| versions::is_version_file(&name))
        })
        .collect();
    out.sort();
    if !out.iter().any(|path| path == named) {
        out.push(named.to_path_buf());
    }
    out
}

/// Load every sidecar in a library, for the store's importer.
///
/// Returns `(chart_hash, difficulty index, contexts)` per chart
/// version that has one.
#[must_use]
pub fn load_library(dir: &Path) -> Vec<(String, u8, Vec<beatbyte_chart::NoteContext>)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        let Ok(files) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut charts: Vec<PathBuf> = files
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .is_some_and(|name| name.ends_with(".json") && !name.ends_with(".context.json"))
            })
            .collect();
        charts.sort();
        for path in charts {
            let Ok(chart) = beatbyte_chart::load_chart_file(&path) else {
                continue;
            };
            let Some(sidecar) = context::load_context(&path, &chart) else {
                continue;
            };
            for (index, difficulty) in beatbyte_core::Difficulty::ALL.iter().enumerate() {
                let Some(packed) = sidecar.tracks.get(difficulty.id()) else {
                    continue;
                };
                out.push((
                    sidecar.chart_hash.clone(),
                    u8::try_from(index).unwrap_or(0),
                    packed
                        .iter()
                        .map(|entry| beatbyte_chart::NoteContext::from(*entry))
                        .collect(),
                ));
            }
        }
    }
    out
}

/// A chart the caller named that is not itself a version file still
/// gets a sidecar — `chart.json` in a legacy folder, for instance.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_chart_is_included_even_when_it_is_not_a_version() {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-cli-context-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(dir.join("chart.v1.json"), "{}").expect("writes");
        std::fs::write(dir.join("chart.v2.json"), "{}").expect("writes");
        std::fs::write(dir.join("song.chart.json"), "{}").expect("writes");

        let named = dir.join("song.chart.json");
        let found = charts_in(&dir, &named);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.contains(&named));
        assert!(found.contains(&dir.join("chart.v1.json")));

        // …and naming a version does not add it twice.
        let found = charts_in(&dir, &dir.join("chart.v2.json"));
        assert_eq!(found.len(), 2, "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_without_a_chart_is_skipped_rather_than_failing() {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-cli-context-empty-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        assert_eq!(any_chart(&dir), None);
        // A sidecar alone is not a chart.
        std::fs::write(dir.join("chart.v1.context.json"), "{}").expect("writes");
        assert_eq!(any_chart(&dir), None);
        std::fs::write(dir.join("chart.v1.json"), "{}").expect("writes");
        assert_eq!(any_chart(&dir), Some(dir.join("chart.v1.json")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
