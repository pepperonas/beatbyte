//! The BeatByte command line: song analysis, chart generation and chart
//! validation tooling.
//!
//! The subcommand surface is defined here from the start so scripts can
//! rely on it; the implementations land with Milestones 3–4 (audio
//! analysis, chart generation).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "beatbyte-cli",
    version,
    about = "BeatByte song analysis and chart tooling",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a song: BPM, beat positions, onsets.
    Analyze {
        /// Path to the audio file (ogg/wav/flac/mp3).
        song: PathBuf,
    },
    /// Generate a BeatByte chart from a song.
    Generate {
        /// Path to the audio file (ogg/wav/flac/mp3).
        song: PathBuf,
    },
    /// Validate a chart file.
    Validate {
        /// Path to the chart JSON file.
        chart: PathBuf,
    },
    /// Summarize the contents of a chart file.
    Inspect {
        /// Path to the chart JSON file.
        chart: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Analyze { song } => not_yet("analyze", &song),
        Command::Generate { song } => not_yet("generate", &song),
        Command::Validate { chart } => not_yet("validate", &chart),
        Command::Inspect { chart } => not_yet("inspect", &chart),
    }
}

/// Honest placeholder until the audio/chart milestones land: report
/// clearly instead of pretending, and exit non-zero so scripts notice.
fn not_yet(command: &str, path: &std::path::Path) -> ExitCode {
    eprintln!(
        "beatbyte-cli: `{command}` is not implemented yet (target: {}).",
        path.display()
    );
    eprintln!("This command arrives with the audio analysis and chart generation milestones.");
    ExitCode::from(2)
}
