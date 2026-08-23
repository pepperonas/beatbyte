//! The BeatByte command line: song analysis, chart generation and chart
//! validation tooling.
//!
//! Exit codes: `0` success · `1` the input failed (invalid chart,
//! undecodable song) · `2` operational error (missing file, bad usage).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::{ChartFile, GenerateMeta, Severity, generate_chart};
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
    /// Analyze a song: BPM, beat grid, onsets, energy.
    Analyze {
        /// Path to the audio file (wav/ogg/flac/mp3/m4a).
        song: PathBuf,
    },
    /// Generate a BeatByte chart (all four difficulties) from a song.
    Generate {
        /// Path to the audio file (wav/ogg/flac/mp3/m4a).
        song: PathBuf,
        /// Song title (defaults to the file name).
        #[arg(long)]
        title: Option<String>,
        /// Artist name.
        #[arg(long, default_value = "Unknown")]
        artist: String,
        /// Output chart path (defaults to `<song>.chart.json`).
        #[arg(long)]
        out: Option<PathBuf>,
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
    /// Render the built-in songs and generate their charts.
    Demo {
        /// Directory to write the songs' WAV + chart files into.
        #[arg(long, default_value = "songs/builtin")]
        out_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Analyze { song } => analyze(&song),
        Command::Generate {
            song,
            title,
            artist,
            out,
        } => generate(&song, title, &artist, out),
        Command::Validate { chart } => validate(&chart),
        Command::Inspect { chart } => inspect(&chart),
        Command::Demo { out_dir } => demo(&out_dir),
    }
}

fn demo(out_dir: &Path) -> ExitCode {
    use beatbyte_audio::demo;

    if let Err(error) = std::fs::create_dir_all(out_dir) {
        eprintln!("cannot create `{}`: {error}", out_dir.display());
        return ExitCode::from(2);
    }
    type Render = fn() -> beatbyte_audio::decode::AudioData;
    let songs: [(Render, &str, &str, &str); 2] = [
        (
            demo::render_demo_song,
            demo::DEMO_TITLE,
            demo::DEMO_ARTIST,
            "circuit-breaker",
        ),
        (
            demo::render_groove_song,
            demo::GROOVE_TITLE,
            demo::GROOVE_ARTIST,
            "solder-groove",
        ),
    ];
    for (render, title, artist, stem) in songs {
        eprintln!("rendering \"{title}\" by {artist}…");
        let audio = render();
        let wav_path = out_dir.join(format!("{stem}.wav"));
        if let Err(error) = beatbyte_audio::write_wav_mono16(&wav_path, &audio) {
            eprintln!("cannot write `{}`: {error}", wav_path.display());
            return ExitCode::from(2);
        }
        println!(
            "Wrote `{}` ({:.0} s)",
            wav_path.display(),
            audio.duration_s()
        );
        let code = generate(
            &wav_path,
            Some(title.to_owned()),
            artist,
            Some(out_dir.join(format!("{stem}.chart.json"))),
        );
        if code != ExitCode::SUCCESS {
            return code;
        }
    }
    ExitCode::SUCCESS
}

fn analyze(song: &Path) -> ExitCode {
    let analysis = match run_analysis(song) {
        Ok(analysis) => analysis,
        Err(code) => return code,
    };

    println!("Analysis of `{}`", song.display());
    println!("  duration      {:>8.1} s", analysis.duration_s);
    println!(
        "  bpm           {:>8.1}   (confidence {:.0}%)",
        analysis.bpm,
        analysis.bpm_confidence * 100.0
    );
    if let Some(alt) = analysis.alt_bpm {
        println!("  alt bpm       {alt:>8.1}   (the other plausible octave)");
    }
    println!("  beats         {:>8}", analysis.beats.len());
    println!("  onsets        {:>8}", analysis.onsets.len());
    if let Some(first) = analysis.beats.first() {
        println!("  first beat    {first:>8.3} s");
    }
    ExitCode::SUCCESS
}

fn generate(song: &Path, title: Option<String>, artist: &str, out: Option<PathBuf>) -> ExitCode {
    let analysis = match run_analysis(song) {
        Ok(analysis) => analysis,
        Err(code) => return code,
    };

    let stem = song
        .file_stem()
        .map_or_else(|| "song".to_owned(), |s| s.to_string_lossy().into_owned());
    let audio_name = song
        .file_name()
        .map_or_else(|| "audio".to_owned(), |s| s.to_string_lossy().into_owned());
    let meta = GenerateMeta {
        title: title.unwrap_or_else(|| stem.clone()),
        artist: artist.to_owned(),
        audio: audio_name,
    };

    let chart = generate_chart(&analysis, &meta);
    let issues = chart.validate();
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if errors > 0 {
        eprintln!("generated chart failed its own validation — this is a bug:");
        for issue in &issues {
            eprintln!("  {issue}");
        }
        return ExitCode::from(1);
    }

    let out_path = out.unwrap_or_else(|| song.with_file_name(format!("{stem}.chart.json")));
    if let Err(error) = beatbyte_chart::save_chart_file(&out_path, &chart) {
        eprintln!("cannot write chart: {error}");
        return ExitCode::from(2);
    }

    println!(
        "Generated `{}` — {:.1} BPM, {:.0} s",
        out_path.display(),
        chart.song.bpm,
        analysis.duration_s
    );
    for def in &chart.charts {
        println!(
            "  {:<8} {:>5} notes, {:>2} phrases",
            def.difficulty.id(),
            def.notes.len(),
            def.phrases.len()
        );
    }
    ExitCode::SUCCESS
}

fn validate(chart_path: &Path) -> ExitCode {
    let chart = match load(chart_path) {
        Ok(chart) => chart,
        Err(code) => return code,
    };
    let issues = chart.validate();
    if issues.is_empty() {
        println!("`{}` is valid.", chart_path.display());
        return ExitCode::SUCCESS;
    }
    let mut errors = 0;
    for issue in &issues {
        println!("{issue}");
        if issue.severity == Severity::Error {
            errors += 1;
        }
    }
    if errors > 0 {
        println!("{errors} error(s) — the chart is not playable.");
        ExitCode::from(1)
    } else {
        println!("warnings only — the chart is playable.");
        ExitCode::SUCCESS
    }
}

fn inspect(chart_path: &Path) -> ExitCode {
    let chart = match load(chart_path) {
        Ok(chart) => chart,
        Err(code) => return code,
    };
    println!("`{}`", chart_path.display());
    println!("  format      v{}", chart.format_version);
    println!("  title       {}", chart.song.title);
    println!("  artist      {}", chart.song.artist);
    println!("  audio       {}", chart.song.audio);
    println!("  bpm         {:.1}", chart.song.bpm);
    println!("  offset      {:.3} s", chart.song.offset_s);
    if let Some(duration) = chart.song.duration_s {
        println!("  duration    {duration:.1} s");
    }
    for def in &chart.charts {
        let sustains = def.notes.iter().filter(|n| n.len > 0.0).count();
        let hopos = def.notes.iter().filter(|n| n.hopo).count();
        println!(
            "  {:<8}  {:>5} notes ({sustains} sustains, {hopos} hopos), {} phrases",
            def.difficulty.id(),
            def.notes.len(),
            def.phrases.len()
        );
    }
    ExitCode::SUCCESS
}

/// Decode + analyze, with human-readable failures.
fn run_analysis(song: &Path) -> Result<beatbyte_core::SongAnalysis, ExitCode> {
    let audio = decode_file(song).map_err(|error| {
        eprintln!("{error}");
        ExitCode::from(2)
    })?;
    if audio.truncated() {
        eprintln!(
            "note: `{}` is longer than the analysis cap; only the first part was analyzed",
            song.display()
        );
    }
    eprintln!(
        "analyzing {:.0} s of audio at {} Hz…",
        audio.duration_s(),
        audio.sample_rate()
    );
    Ok(SpectralAnalyzer::default().analyze(&audio))
}

fn load(chart_path: &Path) -> Result<ChartFile, ExitCode> {
    beatbyte_chart::load_chart_file(chart_path).map_err(|error| {
        eprintln!("{error}");
        ExitCode::from(2)
    })
}
