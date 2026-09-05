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

#[cfg(feature = "ml")]
mod align;
mod dossier;
mod history;
#[cfg(feature = "ml")]
mod models;
mod redesign;
mod review;

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
        /// Dump the full analysis as JSON to this path.
        #[arg(long)]
        json: Option<PathBuf>,
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
    /// Review a chart against recorded play sessions (ADR-0011).
    Review {
        /// Path to the chart JSON file the sessions were played on.
        chart: PathBuf,
        /// Difficulty to review (defaults to medium, the tuning
        /// anchor).
        #[arg(long, default_value = "medium")]
        difficulty: String,
        /// Telemetry directory (defaults to the game's own).
        #[arg(long)]
        telemetry_dir: Option<PathBuf>,
        /// Sessions of the current chart version required before any
        /// directive is emitted.
        #[arg(long, default_value_t = 3)]
        min_sessions: usize,
        /// Include autopilot sessions (excluded by default — a
        /// perfect player makes every chart look too easy).
        #[arg(long)]
        include_autopilot: bool,
        /// Write the directives as JSON to this path.
        #[arg(long)]
        directives: Option<PathBuf>,
    },
    /// Write a design dossier: chart + analysis + evidence in one
    /// file (ADR-0011).
    Dossier {
        /// Path to the song's chart (the ACTIVE version is resolved
        /// from the folder's pointer, so this can always be
        /// `chart.json`).
        chart: PathBuf,
        /// Difficulty under design.
        #[arg(long, default_value = "medium")]
        difficulty: String,
        /// Telemetry directory (defaults to the game's own).
        #[arg(long)]
        telemetry_dir: Option<PathBuf>,
        /// Sessions required before directives are included.
        #[arg(long, default_value_t = 3)]
        min_sessions: usize,
        /// Include autopilot sessions in the evidence.
        #[arg(long)]
        include_autopilot: bool,
        /// Output path (defaults to `dossier-<difficulty>.json` next
        /// to the chart).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Regenerate hard + expert as a new sibling version, keeping
    /// easy + medium from the active version (the difficulty
    /// redesign rollout; docs/difficulty-redesign-plan.md).
    Redesign {
        /// Path to the song's chart (the folder's ACTIVE version is
        /// resolved from the pointer) — or, with `--all`, the
        /// directory of song folders to roll over.
        chart: PathBuf,
        /// Treat the path as a directory of song folders.
        #[arg(long)]
        all: bool,
    },
    /// Set a song's genre (display metadata; hash-neutral, so
    /// recorded sessions survive).
    SetGenre {
        /// Path to the song's chart; the genre is written into every
        /// version in the folder, so switching versions keeps it.
        chart: PathBuf,
        /// The genre, 1-48 characters.
        genre: String,
    },
    /// Export the play history (every track this installation
    /// played) for reporting or analysis.
    History {
        /// Output format: `csv` for reporting, `json` for analysis.
        #[arg(long, default_value = "csv")]
        format: String,
        /// Write here instead of standard output.
        #[arg(long)]
        out: Option<PathBuf>,
        /// History file (defaults to the game's own).
        #[arg(long)]
        file: Option<PathBuf>,
        /// Keep only runs started at or after this unix millisecond
        /// stamp.
        #[arg(long)]
        from_ms: Option<u64>,
        /// Keep only runs started before this unix millisecond stamp
        /// (half-open, so two periods cannot report the same run).
        #[arg(long)]
        until_ms: Option<u64>,
        /// Drop runs shorter than this many seconds.
        #[arg(long, default_value_t = 0.0)]
        min_seconds: f64,
        /// Drop runs that used practice speed or a section loop.
        #[arg(long)]
        exclude_practice: bool,
        /// Drop autopilot runs (test runs, not performances).
        #[arg(long)]
        exclude_autopilot: bool,
        /// Keep only runs that reached the end of the song.
        #[arg(long)]
        completed_only: bool,
    },
    /// Render the built-in songs and generate their charts.
    Demo {
        /// Directory to write the songs' WAV + chart files into.
        #[arg(long, default_value = "songs/builtin")]
        out_dir: PathBuf,
    },
    /// Local ML models: list, install, verify, remove (built with
    /// `--features ml`). `install` is the one command here that
    /// reaches the network — once, to the URL this build pins.
    #[cfg(feature = "ml")]
    Models {
        #[command(subcommand)]
        action: ModelsAction,
    },
    /// Word- and letter-level timing for known lyrics, force-aligned
    /// against the song's own audio (built with `--features ml`;
    /// needs `models install wav2vec2-base-960h`). Writes
    /// `<audio stem>.words.json` beside the audio.
    #[cfg(feature = "ml")]
    Align {
        /// The song (wav/ogg/flac/mp3/m4a).
        audio: PathBuf,
        /// The lyrics: an `.lrc` (stamps are stripped and, if present,
        /// compared against) or plain text.
        lyrics: PathBuf,
        /// Where to write the alignment instead of beside the audio.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Skip the confidence gate: write what the aligner produced,
        /// with no word marked estimated and no line-level fallback.
        #[arg(long)]
        raw: bool,
    },
}

/// What to do with the local models.
#[cfg(feature = "ml")]
#[derive(Subcommand)]
enum ModelsAction {
    /// Every model this build knows, and whether it is installed.
    List,
    /// Download and verify a model.
    Install {
        /// The model's id (see `list`).
        id: String,
    },
    /// Re-hash an installed model against the registry.
    Verify {
        /// The model's id.
        id: String,
    },
    /// Delete an installed model.
    Remove {
        /// The model's id.
        id: String,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Analyze { song, json } => analyze(&song, json.as_deref()),
        Command::Generate {
            song,
            title,
            artist,
            out,
        } => generate(&song, title, &artist, out),
        Command::Validate { chart } => validate(&chart),
        Command::Inspect { chart } => inspect(&chart),
        Command::Review {
            chart,
            difficulty,
            telemetry_dir,
            min_sessions,
            include_autopilot,
            directives,
        } => run_review(
            &chart,
            &difficulty,
            telemetry_dir,
            min_sessions,
            include_autopilot,
            directives.as_deref(),
        ),
        Command::Dossier {
            chart,
            difficulty,
            telemetry_dir,
            min_sessions,
            include_autopilot,
            out,
        } => run_dossier(
            &chart,
            &difficulty,
            telemetry_dir,
            min_sessions,
            include_autopilot,
            out,
        ),
        Command::Redesign { chart, all } => {
            if all {
                redesign::run_redesign_all(&chart)
            } else {
                redesign::run_redesign(&chart)
            }
        }
        Command::SetGenre { chart, genre } => set_genre(&chart, &genre),
        Command::History {
            format,
            out,
            file,
            from_ms,
            until_ms,
            min_seconds,
            exclude_practice,
            exclude_autopilot,
            completed_only,
        } => export_history(
            &format,
            out.as_deref(),
            file.as_deref(),
            history::Filter {
                from_ms,
                until_ms,
                min_seconds,
                exclude_practice,
                exclude_autopilot,
                completed_only,
            },
        ),
        Command::Demo { out_dir } => demo(&out_dir),
        #[cfg(feature = "ml")]
        Command::Align {
            audio,
            lyrics,
            out,
            raw,
        } => align::run(&audio, &lyrics, out, raw),
        #[cfg(feature = "ml")]
        Command::Models { action } => match action {
            ModelsAction::List => models::list(),
            ModelsAction::Install { id } => models::install(&id),
            ModelsAction::Verify { id } => models::verify(&id),
            ModelsAction::Remove { id } => models::remove(&id),
        },
    }
}

/// Export the play history.
///
/// The default file is the game's own, in the platform data
/// directory beside `scores.json` — the same rule the game writes
/// by, so the two never have to agree twice.
fn export_history(
    format: &str,
    out: Option<&Path>,
    file: Option<&Path>,
    filter: history::Filter,
) -> ExitCode {
    let path = match file {
        Some(path) => path.to_path_buf(),
        None => match dirs::data_dir() {
            Some(dir) => dir.join("beatbyte").join("history.jsonl"),
            None => {
                eprintln!("no data directory on this platform - pass --file");
                return ExitCode::from(2);
            }
        },
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read the history at {}: {error}", path.display());
            eprintln!("(the file appears once a track has been played)");
            return ExitCode::from(2);
        }
    };
    let all = beatbyte_core::history::parse_log(&text);
    let kept = history::select(&all, filter);
    let rendered = match format {
        "csv" => beatbyte_core::history::to_csv(&kept),
        "json" => match history::to_json(&kept) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("cannot render JSON: {error}");
                return ExitCode::from(2);
            }
        },
        other => {
            eprintln!("unknown format `{other}` - use csv or json");
            return ExitCode::from(2);
        }
    };
    if let Some(out) = out {
        if let Err(error) = std::fs::write(out, rendered) {
            eprintln!("cannot write {}: {error}", out.display());
            return ExitCode::from(2);
        }
        // The counts go to stderr, so a piped export stays clean.
        eprintln!(
            "{} of {} runs written to {}",
            kept.len(),
            all.len(),
            out.display()
        );
    } else {
        print!("{rendered}");
    }
    ExitCode::SUCCESS
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

fn analyze(song: &Path, json: Option<&Path>) -> ExitCode {
    let (analysis, _) = match run_analysis(song) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    if let Some(path) = json {
        match serde_json::to_string(&analysis) {
            Ok(text) => {
                if let Err(error) = std::fs::write(path, text) {
                    eprintln!("cannot write `{}`: {error}", path.display());
                    return ExitCode::FAILURE;
                }
                println!("analysis JSON written to `{}`", path.display());
            }
            Err(error) => {
                eprintln!("cannot serialize analysis: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

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
    let held: Vec<f64> = analysis
        .melody
        .iter()
        .map(beatbyte_core::MelodyNote::len_s)
        .collect();
    let long = held.iter().filter(|l| **l >= 0.45).count();
    println!(
        "  melody notes  {:>8}   ({long} held >=0.45 s)",
        analysis.melody.len()
    );
    if let Some(first) = analysis.beats.first() {
        println!("  first beat    {first:>8.3} s");
    }
    ExitCode::SUCCESS
}

fn generate(song: &Path, title: Option<String>, artist: &str, out: Option<PathBuf>) -> ExitCode {
    let (analysis, trim) = match run_analysis(song) {
        Ok(pair) => pair,
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

    let mut chart = generate_chart(&analysis, &meta);
    chart.audio_trim = Some(trim);
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
/// Decode and analyze; the chart writer also needs the decode's
/// timeline marker, so both come back.
fn run_analysis(
    song: &Path,
) -> Result<(beatbyte_core::SongAnalysis, beatbyte_chart::AudioTrim), ExitCode> {
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
    let priming = audio.priming();
    let trim = beatbyte_chart::AudioTrim::declared(
        priming.samples,
        priming.timescale,
        audio.sample_rate(),
    );
    Ok((SpectralAnalyzer::default().analyze(&audio), trim))
}

fn load(chart_path: &Path) -> Result<ChartFile, ExitCode> {
    beatbyte_chart::load_chart_file(chart_path).map_err(|error| {
        eprintln!("{error}");
        ExitCode::from(2)
    })
}

/// `review`: join the telemetry with the chart and say where it
/// struggles or bores. IO here, all judgment in `review.rs`.
#[allow(clippy::too_many_lines)] // one report, printed in one place
fn run_review(
    chart_path: &Path,
    difficulty: &str,
    telemetry_dir: Option<PathBuf>,
    min_sessions: usize,
    include_autopilot: bool,
    directives_out: Option<&Path>,
) -> ExitCode {
    use beatbyte_core::telemetry::parse_session;

    let text = match std::fs::read_to_string(chart_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", chart_path.display());
            return ExitCode::from(2);
        }
    };
    let chart: ChartFile = match serde_json::from_str(&text) {
        Ok(chart) => chart,
        Err(error) => {
            eprintln!("`{}` is not a chart: {error}", chart_path.display());
            return ExitCode::from(1);
        }
    };
    let Some(parsed_difficulty) = parse_difficulty(difficulty) else {
        eprintln!("unknown difficulty `{difficulty}` (easy/medium/hard/expert)");
        return ExitCode::from(2);
    };
    let track = match chart.to_track(parsed_difficulty) {
        Ok(track) => track,
        Err(error) => {
            eprintln!("chart has no playable {difficulty} track: {error}");
            return ExitCode::from(1);
        }
    };
    let dir =
        telemetry_dir.or_else(|| dirs::data_dir().map(|d| d.join("beatbyte").join("telemetry")));
    let Some(dir) = dir else {
        eprintln!("no telemetry directory on this platform; pass --telemetry-dir");
        return ExitCode::from(2);
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", dir.display());
            eprintln!("(no sessions recorded yet? play the song first)");
            return ExitCode::from(2);
        }
    };

    // Every parseable session for this song + difficulty, whatever
    // chart version it was played on — the review reports the stale
    // ones rather than hiding them.
    let wanted_difficulty = difficulty.to_lowercase();
    let mut sessions = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "jsonl") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((header, lines)) = parse_session(&content) else {
            continue;
        };
        if header.title == chart.song.title
            && header.artist == chart.song.artist
            && header.difficulty == wanted_difficulty
        {
            sessions.push(review::Session { header, lines });
        }
    }

    let thresholds = review::Thresholds {
        min_sessions,
        ..review::Thresholds::default()
    };
    let current_hash = beatbyte_chart::chart_hash(&chart);
    let outcome = review::review(
        &track,
        chart.song.bpm,
        chart.song.offset_s,
        &current_hash,
        &sessions,
        include_autopilot,
        &thresholds,
    );

    println!(
        "review: \"{}\" — {} <{}>  chart {}",
        chart.song.title, chart.song.artist, wanted_difficulty, current_hash
    );
    let complete = sessions.iter().filter(|s| review::is_complete(s)).count();
    println!(
        "sessions: {} used ({} complete), {} stale (other chart version), {} autopilot excluded",
        outcome.sessions_used, complete, outcome.stale_sessions, outcome.autopilot_sessions
    );
    if !outcome.fun_ratings.is_empty() {
        let sum: u32 = outcome.fun_ratings.iter().map(|r| u32::from(*r)).sum();
        println!(
            "fun: {:.1}/5 over {} rating(s)",
            f64::from(sum) / outcome.fun_ratings.len() as f64,
            outcome.fun_ratings.len()
        );
    }
    if !outcome.versus.is_empty() {
        let better = outcome.versus.iter().filter(|v| *v == "better").count();
        println!(
            "versus parent version: {} better / {} worse",
            better,
            outcome.versus.len() - better
        );
    }
    if outcome.sections.is_empty() {
        println!("no observations for this chart version yet.");
        return ExitCode::SUCCESS;
    }
    println!();
    println!("bars      time          judged   acc     mean     spread  sustains  overstrums");
    for section in &outcome.sections {
        println!(
            "{:>3}-{:<3}  {:>6.1}-{:<6.1}s  {:>5}  {:>5.1}%  {:>+6.1}ms {:>6.1}ms  {:>3}/{:<3}  {:>4}",
            section.bar_start,
            section.bar_end,
            section.time_s.0,
            section.time_s.1,
            section.judged,
            section.accuracy * 100.0,
            section.mean_off_ms,
            section.stddev_ms,
            section.sustains.1,
            section.sustains.0,
            section.overstrums,
        );
    }
    println!();
    if outcome.directives.is_empty() {
        println!(
            "no directives ({} sessions of this version; {} required).",
            outcome.sessions_used, thresholds.min_sessions
        );
    } else {
        for directive in &outcome.directives {
            let place = directive.bars.map_or_else(
                || "whole chart".to_owned(),
                |(a, b)| format!("bars {a}-{b}"),
            );
            println!(
                "directive: {} at {} — recommend {:?} (acc {:.1}%, spread {:.1} ms, {} sessions)",
                directive.problem,
                place,
                directive.recommend,
                directive.evidence.accuracy * 100.0,
                directive.evidence.stddev_ms,
                directive.evidence.sessions,
            );
        }
        if let Some(out) = directives_out {
            match serde_json::to_string_pretty(&outcome.directives) {
                Ok(json) => {
                    if let Err(error) = std::fs::write(out, json) {
                        eprintln!("cannot write `{}`: {error}", out.display());
                        return ExitCode::from(2);
                    }
                    println!("directives written to {}", out.display());
                }
                Err(error) => eprintln!("cannot serialize directives: {error}"),
            }
        }
    }
    ExitCode::SUCCESS
}

/// Parse a difficulty name the way the headers spell it.
fn parse_difficulty(name: &str) -> Option<beatbyte_core::Difficulty> {
    use beatbyte_core::Difficulty;
    match name.to_lowercase().as_str() {
        "easy" => Some(Difficulty::Easy),
        "medium" => Some(Difficulty::Medium),
        "hard" => Some(Difficulty::Hard),
        "expert" => Some(Difficulty::Expert),
        _ => None,
    }
}

/// `dossier`: everything a design session needs, in one file.
fn run_dossier(
    chart_path: &Path,
    difficulty: &str,
    telemetry_dir: Option<PathBuf>,
    min_sessions: usize,
    include_autopilot: bool,
    out: Option<PathBuf>,
) -> ExitCode {
    use beatbyte_chart::versions;
    use beatbyte_core::telemetry::parse_session;

    let Some(folder) = chart_path.parent().map(Path::to_path_buf) else {
        eprintln!("`{}` has no parent folder", chart_path.display());
        return ExitCode::from(2);
    };
    // Resolve the ACTIVE version — designing against a superseded
    // chart would attach the wrong parent to the provenance.
    let names: Vec<String> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active_path = folder.join(&active_name);
    let text = match std::fs::read_to_string(&active_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", active_path.display());
            return ExitCode::from(2);
        }
    };
    let chart: ChartFile = match serde_json::from_str(&text) {
        Ok(chart) => chart,
        Err(error) => {
            eprintln!("`{}` is not a chart: {error}", active_path.display());
            return ExitCode::from(1);
        }
    };
    let Some(parsed_difficulty) = parse_difficulty(difficulty) else {
        eprintln!("unknown difficulty `{difficulty}` (easy/medium/hard/expert)");
        return ExitCode::from(2);
    };
    let Ok(track) = chart.to_track(parsed_difficulty) else {
        eprintln!("chart has no playable {difficulty} track");
        return ExitCode::from(1);
    };

    // The audio sits next to the chart; the analysis is recomputed so
    // the dossier reads the song, not a cached opinion of it.
    let audio_path = folder.join(&chart.song.audio);
    let audio = match decode_file(&audio_path) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("cannot decode `{}`: {error}", audio_path.display());
            return ExitCode::from(1);
        }
    };
    let analysis = SpectralAnalyzer::default().analyze(&audio);

    // Evidence, straight from the telemetry — same code path as
    // `review`, so the two cannot disagree.
    let dir =
        telemetry_dir.or_else(|| dirs::data_dir().map(|d| d.join("beatbyte").join("telemetry")));
    let mut sessions = Vec::new();
    if let Some(dir) = dir
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let wanted = difficulty.to_lowercase();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((header, lines)) = parse_session(&content) else {
                continue;
            };
            if header.title == chart.song.title
                && header.artist == chart.song.artist
                && header.difficulty == wanted
            {
                sessions.push(review::Session { header, lines });
            }
        }
    }
    let current_hash = beatbyte_chart::chart_hash(&chart);
    let outcome = review::review(
        &track,
        chart.song.bpm,
        chart.song.offset_s,
        &current_hash,
        &sessions,
        include_autopilot,
        &dossier::dossier_thresholds(min_sessions),
    );

    let next_version = versions::next_version_name(&names);
    let built = dossier::assemble(
        chart,
        &analysis,
        parsed_difficulty,
        outcome.directives,
        next_version,
    );
    let out_path = out.unwrap_or_else(|| folder.join(format!("dossier-{}.json", built.difficulty)));
    match serde_json::to_string_pretty(&built) {
        Ok(json) => {
            if let Err(error) = std::fs::write(&out_path, json) {
                eprintln!("cannot write `{}`: {error}", out_path.display());
                return ExitCode::from(2);
            }
        }
        Err(error) => {
            eprintln!("cannot serialize dossier: {error}");
            return ExitCode::from(2);
        }
    }
    println!(
        "dossier: \"{}\" <{}>  active {}  {} bars, {} melody notes, {} directive(s)",
        built.chart.song.title,
        built.difficulty,
        active_name,
        built.bars.len(),
        built.melody.len(),
        built.directives.len(),
    );
    println!("written to {}", out_path.display());
    println!(
        "next version: {}  parent: {}",
        built.write.next_version_file, built.write.parent_hash
    );
    ExitCode::SUCCESS
}

/// `set-genre`: stamp the genre into every chart version of a song.
fn set_genre(chart_path: &Path, genre: &str) -> ExitCode {
    use beatbyte_chart::versions;
    let Some(folder) = chart_path.parent().map(Path::to_path_buf) else {
        eprintln!("`{}` has no parent folder", chart_path.display());
        return ExitCode::from(2);
    };
    let trimmed = genre.trim();
    if trimmed.is_empty() || trimmed.len() > 48 {
        eprintln!("genre must be 1-48 characters");
        return ExitCode::from(2);
    }
    let mut touched = 0usize;
    let Ok(entries) = std::fs::read_dir(&folder) else {
        eprintln!("cannot list `{}`", folder.display());
        return ExitCode::from(2);
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != versions::BASE_CHART && !versions::is_version_file(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut chart) = serde_json::from_str::<ChartFile>(&text) else {
            continue;
        };
        chart.song.genre = Some(trimmed.to_owned());
        match serde_json::to_string(&chart) {
            Ok(json) => {
                if std::fs::write(&path, json).is_ok() {
                    touched += 1;
                    println!("{name}: genre = {trimmed}");
                }
            }
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    if touched == 0 {
        eprintln!("no chart files found in `{}`", folder.display());
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
