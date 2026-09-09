//! `redesign` on the command line.
//!
//! The redesign itself lives in [`beatbyte_chart::redesign`], which
//! the game runs too. Here is what a command line adds: reading the
//! recording, printing, and an exit code.

use std::path::Path;
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::redesign::{Reading, redesign_folder};

/// Decode and analyse one recording, with the meter folded in where
/// the build has a model for it.
fn read(audio_path: &Path) -> Result<Reading, String> {
    let audio = decode_file(audio_path)
        .map_err(|error| format!("cannot decode `{}`: {error}", audio_path.display()))?;
    let priming = audio.priming();
    let trim = beatbyte_chart::AudioTrim::declared(
        priming.samples,
        priming.timescale,
        audio.sample_rate(),
    );
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    crate::meter(&mut analysis, &audio);
    Ok(Reading { analysis, trim })
}

/// Print what the redesign has to say.
fn say(line: String) {
    eprintln!("{line}");
}

/// One folder, the way both entry points want it.
fn one(folder: &Path) -> Result<String, String> {
    redesign_folder(folder, &read, &say)
}

/// `redesign` on one chart path.
pub fn run_redesign(chart_path: &Path) -> ExitCode {
    let Some(folder) = chart_path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        eprintln!("`{}` has no parent folder", chart_path.display());
        return ExitCode::from(2);
    };
    match one(folder) {
        Ok(message) => {
            println!("{}: {message}", folder.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{}: {message}", folder.display());
            ExitCode::from(1)
        }
    }
}

/// `redesign --all` over a directory of song folders.
pub fn run_redesign_all(dir: &Path) -> ExitCode {
    let mut folders: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(error) => {
            eprintln!("cannot list `{}`: {error}", dir.display());
            return ExitCode::from(2);
        }
    };
    folders.sort();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for folder in folders {
        match one(&folder) {
            Ok(message) => {
                println!("{}: {message}", folder.display());
                written += 1;
            }
            Err(message) if message.contains("legacy layout") => {
                println!("{}: {message}", folder.display());
                skipped += 1;
            }
            Err(message) => {
                eprintln!("{}: {message}", folder.display());
                failed += 1;
            }
        }
    }
    println!("redesigned {written}, skipped {skipped}, failed {failed}");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
