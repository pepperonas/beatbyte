//! `beatbyte-cli study`: the `[Guitar Study]` twin of a song folder.
//!
//! The writer lives in [`beatbyte_chart::study`] so the game's import
//! and this command share one rule; this file only decodes and
//! analyzes, and turns the outcome into an exit code.

use std::path::Path;
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::redesign::Reading;
use beatbyte_chart::study::{Outcome, write_twin};

/// Decode a file and analyze it; the timeline it is on travels with
/// the analysis.
pub fn read(path: &Path) -> Result<Reading, String> {
    let audio =
        decode_file(path).map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
    let priming = audio.priming();
    let trim = beatbyte_chart::AudioTrim::declared(
        priming.samples,
        priming.timescale,
        audio.sample_rate(),
    );
    eprintln!(
        "reading {}...",
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned()
        )
    );
    let analysis = SpectralAnalyzer::default().analyze(&audio);
    Ok(Reading { analysis, trim })
}

/// Build the twin of `song_folder` from the instrument stem at
/// `lead`. Exit 0 when written or already present, 2 on a bad input,
/// 3 when the source carries too little tonal evidence for a playable
/// four-difficulty chart (the original is then the only version, and
/// the report says so).
pub fn run(song_folder: &Path, lead: &Path) -> ExitCode {
    match write_twin(song_folder, lead, &read) {
        Ok(Outcome::Written {
            folder,
            title,
            notes,
        }) => {
            let counts: Vec<String> = notes.iter().map(|(d, n)| format!("{d} {n}")).collect();
            println!(
                "wrote {} — {title} notes: {}",
                folder.display(),
                counts.join(", ")
            );
            ExitCode::SUCCESS
        }
        Ok(Outcome::AlreadyThere(folder)) => {
            println!("already there: {}", folder.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::Refused(reason)) => {
            eprintln!("{reason} — the original stays the only version");
            ExitCode::from(3)
        }
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::from(2)
        }
    }
}
