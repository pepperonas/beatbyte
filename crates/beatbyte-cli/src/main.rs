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
mod chart_check;
mod classic;
mod context;
mod dossier;
mod history;
mod library;
mod loudness;
#[cfg(feature = "ml")]
mod lyrics_eval;
#[cfg(feature = "ml")]
mod models;
mod players;
mod redesign;
mod review;
mod study;
mod telemetry;
mod vocals;

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

/// The store's own subcommands.
#[derive(Subcommand)]
enum TelemetryCommand {
    /// What is in the store, and whether any of it has a hole in it.
    Status {
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Import the older per-session JSONL files. Idempotent: a second
    /// run imports nothing.
    Import {
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
        /// The directory of `*.jsonl` files (defaults to the game's).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// List sessions, newest first.
    List {
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
        /// How many.
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Print one session the way a person reads one.
    Show {
        /// The session id, as `list` prints it.
        session: i64,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Hand the store to another tool.
    Export {
        /// What to write.
        #[arg(long, value_enum, default_value_t = telemetry::ExportWhat::Sessions)]
        what: telemetry::ExportWhat,
        /// The session, for `--what session`.
        #[arg(long)]
        session: Option<i64>,
        /// How many rows at most.
        #[arg(long, default_value_t = 100_000)]
        limit: usize,
        /// Write here instead of to standard output.
        #[arg(long)]
        out: Option<PathBuf>,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Notes of one chart version that are missed far more than the
    /// rest — candidates, never verdicts.
    Problems {
        /// The chart's content hash, as a session row names it.
        chart_hash: String,
        /// Difficulty index (0 easy … 3 expert).
        #[arg(long, default_value_t = 1)]
        difficulty: u8,
        /// Plays a note needs before it is reported at all.
        #[arg(long, default_value_t = 5)]
        min_samples: u32,
        /// Report notes hit at most this often (0.0–1.0).
        #[arg(long, default_value_t = 0.7)]
        max_hit_rate: f64,
        /// How many to list.
        #[arg(long, default_value_t = 25)]
        limit: usize,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Compare generator versions on comparable material.
    Generators {
        /// Restrict to one genre (the comparison measures the songs
        /// otherwise, not the generator).
        #[arg(long)]
        genre: Option<String>,
        /// Difficulty index (0 easy … 3 expert).
        #[arg(long, default_value_t = 1)]
        difficulty: u8,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Whether a player lands consistently early or late.
    Calibration {
        /// Restrict to one roster player.
        #[arg(long)]
        player: Option<u64>,
        /// Hits needed per device and offset before anything is said.
        #[arg(long, default_value_t = 200)]
        min_hits: u32,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Strums that reached the engine and produced nothing.
    Input {
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Load a library's context sidecars into the store.
    Context {
        /// The songs directory.
        library: PathBuf,
        /// Import every chart version, not only the ones a session
        /// can join to. Most of a library has never been played, and
        /// a context nothing joins to costs space and answers
        /// nothing.
        #[arg(long)]
        all: bool,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// What the missed notes have in common musically (needs
    /// `telemetry context` first).
    Music {
        /// Notes a group needs before it is reported.
        #[arg(long, default_value_t = 50)]
        min_judged: u32,
        /// The database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Fill a throwaway store with a lifetime of playing and time it.
    Bench {
        /// How many sessions.
        #[arg(long, default_value_t = 1000)]
        sessions: usize,
        /// Note events per session (the library's median is 328).
        #[arg(long, default_value_t = 330)]
        notes: usize,
        /// How many distinct charts they are spread over.
        #[arg(long, default_value_t = 100)]
        charts: usize,
        /// Where to build it (a temporary file by default — never the
        /// game's own store).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Leave the database behind instead of deleting it.
        #[arg(long)]
        keep: bool,
    },
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
    /// The song with a click on every chart note, so a chart's
    /// RHYTHM can be judged by ear — plus the share of notes that
    /// sit on the grid and on an audible attack. `--secs` cuts a
    /// slice (from the chart's preview anchor unless `--from` says
    /// otherwise), which is what makes two variants comparable in
    /// half a minute.
    /// Build the `[GS]` twin of a song folder from an
    /// instrument stem: a new folder beside the original with the same
    /// audio and one stem-charted `chart.json`, so both play in the
    /// browser. The original is never touched.
    Study {
        /// The song folder (holds the audio, the charts and the pointer).
        folder: PathBuf,
        /// The instrument stem, full length, on the DECODED song
        /// timeline (`beatbyte-cli decode` first, then separate).
        #[arg(long)]
        lead: PathBuf,
    },
    ChartCheck {
        /// Path to the audio file (wav/ogg/flac/mp3/m4a).
        song: PathBuf,
        /// Path to the chart JSON file.
        chart: PathBuf,
        /// Difficulty to check (defaults to medium, the tuning
        /// anchor).
        #[arg(long, default_value = "medium")]
        difficulty: String,
        /// Window start in seconds.
        #[arg(long)]
        from: Option<f64>,
        /// Window length in seconds (default: to the end of the song).
        #[arg(long)]
        secs: Option<f64>,
        /// Where to write the click track (defaults to
        /// `<chart>.chart-check.wav`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Read the gameplay telemetry store (ADR-0018).
    Telemetry {
        #[command(subcommand)]
        what: TelemetryCommand,
    },
    /// Give every song folder the document it should carry
    /// (ADR-0019): identity, provenance and what is already known,
    /// read from the files that are already there. Writes exactly
    /// one new file per song and touches nothing else; a second run
    /// writes nothing.
    Library {
        /// The songs directory.
        root: PathBuf,
        /// Report what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Instead of writing documents, build the queryable index
        /// from the documents that are already there. The index is a
        /// projection: deleting it loses nothing.
        #[arg(long)]
        index: Option<PathBuf>,
        /// Give every recorded session the song it belongs to, using
        /// the index at this path. Additive: an unmatched session
        /// keeps its gap rather than gaining a guess.
        #[arg(long)]
        backfill: Option<PathBuf>,
        /// The telemetry database (defaults to the game's own).
        #[arg(long)]
        store: Option<PathBuf>,
        /// Report songs whose files are the same recording. Reports
        /// only — nothing here deletes anything. A study twin shares
        /// its song's audio on purpose and is not a duplicate.
        #[arg(long)]
        duplicates: bool,
        /// Ask MusicBrainz about the songs whose records are thin.
        /// The only thing in this tool that talks to a network, at
        /// one request a second, and it never overwrites what the
        /// file said or what you edited. Needs `--features
        /// catalogue`.
        #[arg(long)]
        catalogue: bool,
    },
    /// Write the musical context sidecar beside a chart: what the
    /// analysis says at each note, so a miss recorded later can be
    /// asked what the song was doing there (ADR-0018).
    Context {
        /// A chart file, or a songs directory with `--all`.
        path: PathBuf,
        /// Walk every song folder under `path`.
        #[arg(long)]
        all: bool,
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
    /// Apply the classic ingredients — the rules the early guitar
    /// games played by — as a NEW version whose parent is the one
    /// active now (or, with `--twin`, as a `[CL]` twin folder).
    /// `hopo`: hammer-ons tempo-relative and exclusive, so straight
    /// eighths are strummed and triplets hammered. `strum`: a strum
    /// under the wrong fret waits 60 ms for the fret.
    ///
    /// The blind test (`T` in the browser) then plays the two
    /// against each other, differing by this and nothing else.
    Classic {
        /// A song folder — or, with `--all`, a directory of them.
        folder: PathBuf,
        /// Treat the path as a directory of song folders.
        #[arg(long)]
        all: bool,
        /// Only count what would change, per difficulty and inside
        /// the window the blind test plays. Writes nothing.
        #[arg(long)]
        dry_run: bool,
        /// Write a `[CL]` TWIN folder beside the song instead of a
        /// new version inside it: both charts then stand in the
        /// library at once and can be chosen in the browser. Nothing
        /// in the song's own folder is touched.
        #[arg(long)]
        twin: bool,
        /// Which ingredients, comma-separated (`hopo`, `strum`, or
        /// `all`). Without it: the ones that have passed a blind test.
        /// To blind-test one more, apply it alone as a new version on
        /// a `[CL]` twin and press `T` on it in the browser.
        #[arg(long, value_name = "INGREDIENTS")]
        with: Option<String>,
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
    /// Measure a song's loudness and audio quality the way the game
    /// levels it (EBU R128 integrated loudness, true peak, loudness
    /// range; bandwidth, clipping, bitrate) and, with `--write`, put
    /// the sidecar beside the audio so the game applies the gain.
    Loudness {
        /// A chart, a song folder, an audio file — or, with `--all`,
        /// a directory of song folders.
        path: PathBuf,
        /// Treat the path as a directory of song folders.
        #[arg(long)]
        all: bool,
        /// Write `<audio>.loudness.json` beside each audio file.
        #[arg(long)]
        write: bool,
    },
    /// Make a song's vocal chart: separate the stems, read the sung
    /// line off the vocal one and write `<audio>.vocals.json` beside
    /// the song.
    ///
    /// Separation costs minutes of a saturated machine, so a song
    /// that already has a current chart — or a settled answer such as
    /// "instrumental" — is skipped unless `--force` says otherwise.
    /// Needs a local `demucs`; without one nothing is analysed and
    /// the songs stay exactly as playable as they were.
    Vocals {
        /// A song folder, a chart file, or an audio file.
        path: PathBuf,
        /// Treat the path as a library and work every song folder in
        /// it.
        #[arg(long)]
        all: bool,
        /// Analyse even where a current chart or a settled state
        /// already exists.
        #[arg(long)]
        force: bool,
        /// Report what is there; run nothing.
        #[arg(long)]
        status: bool,
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
    /// The local roster: who plays on this machine.
    ///
    /// Adding the FIRST player credits them with every run the log
    /// already holds that nobody is on — the log predates the roster,
    /// and those runs belong to somebody. A copy of the log is kept
    /// beside it first.
    Players {
        /// Add a player under this name instead of listing them.
        #[arg(long)]
        add: Option<String>,
    },
    /// One player's statistics — the same numbers the game plots.
    Stats {
        /// Whose. Defaults to whoever is selected in the game.
        #[arg(long)]
        player: Option<String>,
    },
    /// One player's achievements — the same evaluation the game runs.
    Awards {
        /// Whose. Defaults to whoever is selected in the game.
        #[arg(long)]
        player: Option<String>,
        /// Also list what has not been earned yet.
        #[arg(long)]
        locked: bool,
    },
    /// Render the built-in songs and generate their charts.
    Demo {
        /// Directory to write the songs' WAV + chart files into.
        #[arg(long, default_value = "songs/builtin")]
        out_dir: PathBuf,
    },
    /// Decode a song exactly as the game hears it — mono, 16-bit, the
    /// container's encoder priming skipped so sample 0 is the
    /// master's — and write it as WAV. The reference for any external
    /// tool that must stay on the game's timeline (a vocal separator,
    /// a DAW), and the way to measure whether it did.
    Decode {
        /// The song (wav/ogg/flac/mp3/m4a).
        audio: PathBuf,
        /// Where to write the WAV (default: `<audio stem>.decoded.wav`
        /// beside the audio).
        #[arg(long)]
        out: Option<PathBuf>,
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
        /// Listen to this file instead of the song: a vocal stem an
        /// external separator wrote from the song, on the song's own
        /// timeline (`beatbyte-cli decode` is the reference for it).
        /// The song itself still supplies the hash and the length the
        /// result is checked against.
        #[arg(long)]
        vocals: Option<PathBuf>,
        /// With `--vocals`: what produced the stem, recorded as the
        /// result's provenance (e.g. `demucs:htdemucs`).
        #[arg(long, default_value = "external")]
        separator: String,
        /// Write only when the new alignment outranks the one already
        /// beside the audio (verdict first, then how much of the song
        /// the model heard); otherwise keep the file and say so.
        #[arg(long)]
        keep_better: bool,
        /// The plain forced alignment: do not confine the words to the
        /// source's line stamps. A diagnostic — the game always
        /// anchors — for when the stamps themselves are in question.
        #[arg(long)]
        no_anchors: bool,
    },
    /// A song with a click on every aligned word, so an alignment can
    /// be judged by ear: writes `<audio stem>.check.wav` and prints
    /// the word sheet (built with `--features ml`).
    #[cfg(feature = "ml")]
    LyricsCheck {
        /// The song.
        audio: PathBuf,
        /// Its alignment (default: `<audio stem>.words.json`).
        #[arg(long)]
        words: Option<PathBuf>,
        /// Where to write the check track.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Measure the aligner against word-level ground truth (the
    /// JamendoLyrics MultiLang layout): AAE, PCO@0.1, PCO@0.3 per
    /// song, per language and over all; `--out` writes the JSON
    /// report the regression test reads (built with `--features ml`).
    #[cfg(feature = "ml")]
    LyricsEval {
        /// The corpus root (default: `BEATBYTE_LYRICS_CORPUS`).
        #[arg(long)]
        corpus: Option<PathBuf>,
        /// Where to write the JSON report.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Evaluate at most this many songs.
        #[arg(long)]
        limit: Option<usize>,
        /// Only songs of this language (as the corpus names it).
        #[arg(long)]
        language: Option<String>,
        /// Measure the raw aligner instead of the gated pipeline.
        #[arg(long)]
        raw: bool,
        /// Hand the corpus's line annotations to the aligner as a
        /// source's line stamps — the game's real case, which the
        /// corpus itself does not present.
        #[arg(long)]
        anchors: bool,
        /// With `--anchors`: wobble each stamp by up to this many
        /// seconds, so the measurement is about a real source rather
        /// than about ground truth handed in through the back door.
        #[arg(long)]
        jitter: Option<f64>,
        /// With `--anchors`: put every stamp on another master by
        /// this many seconds.
        #[arg(long)]
        shift: Option<f64>,
        /// With `--anchors`: how far outside its own line a word may
        /// still land (default 4 s).
        #[arg(long)]
        tolerance: Option<f64>,
        /// Listen to vocal stems instead of the mixes: for each song
        /// `<dir>/<song>/vocals.wav` (the layout a separator writes
        /// per input). A song without a stem is skipped, and said.
        #[arg(long)]
        vocals_dir: Option<PathBuf>,
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
        Command::Study { folder, lead } => study::run(&folder, &lead),
        Command::ChartCheck {
            song,
            chart,
            difficulty,
            from,
            secs,
            out,
        } => match parse_difficulty(&difficulty) {
            Some(difficulty) => chart_check::run(&song, &chart, difficulty, from, secs, out),
            None => {
                eprintln!("unknown difficulty `{difficulty}`");
                ExitCode::from(2)
            }
        },
        Command::Library {
            root,
            dry_run,
            index,
            backfill,
            store,
            duplicates,
            catalogue,
        } => match (index, backfill, duplicates, catalogue) {
            #[cfg(feature = "catalogue")]
            (_, _, _, true) => library::catalogue(&root, dry_run),
            #[cfg(not(feature = "catalogue"))]
            (_, _, _, true) => {
                eprintln!(
                    "this build has no catalogue: rebuild with `--features catalogue`.\n\
                     It is off by default because it is the only part that talks to a \
                     network, and a song is fully playable without it."
                );
                ExitCode::FAILURE
            }
            (_, _, true, _) => library::duplicates(&root),
            (_, Some(db), _, _) => library::backfill(&db, store),
            (Some(db), None, _, _) => library::index(&root, &db),
            (None, None, _, _) => library::run(&root, dry_run),
        },
        Command::Context { path, all } => {
            if all {
                context::run_all(&path)
            } else {
                context::run_one(&path)
            }
        }
        Command::Telemetry { what } => match what {
            TelemetryCommand::Status { store } => telemetry::run_status(store),
            TelemetryCommand::Import { store, dir } => telemetry::run_import(store, dir),
            TelemetryCommand::List { store, limit } => telemetry::run_list(store, limit),
            TelemetryCommand::Show { session, store } => telemetry::run_show(store, session),
            TelemetryCommand::Export {
                what,
                session,
                limit,
                out,
                store,
            } => telemetry::run_export(store, what, session, limit, out),
            TelemetryCommand::Problems {
                chart_hash,
                difficulty,
                min_samples,
                max_hit_rate,
                limit,
                store,
            } => telemetry::run_problems(
                store,
                &chart_hash,
                difficulty,
                min_samples,
                max_hit_rate,
                limit,
            ),
            TelemetryCommand::Generators {
                genre,
                difficulty,
                store,
            } => telemetry::run_generators(store, genre, difficulty),
            TelemetryCommand::Calibration {
                player,
                min_hits,
                store,
            } => telemetry::run_calibration(store, player, min_hits),
            TelemetryCommand::Input { store } => telemetry::run_input(store),
            TelemetryCommand::Context {
                library,
                all,
                store,
            } => telemetry::run_context(store, &library, all),
            TelemetryCommand::Music { min_judged, store } => {
                telemetry::run_music(store, min_judged)
            }
            TelemetryCommand::Bench {
                sessions,
                notes,
                charts,
                store,
                keep,
            } => telemetry::run_bench(store, sessions, notes, charts, keep),
        },
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
        Command::Classic {
            folder,
            all,
            dry_run,
            twin,
            with,
        } => {
            let recipe = match with.as_deref().map(beatbyte_chart::classic::Recipe::parse) {
                None => beatbyte_chart::classic::Recipe::default(),
                Some(Ok(recipe)) => recipe,
                Some(Err(reason)) => {
                    eprintln!("--with: {reason}");
                    return ExitCode::from(2);
                }
            };
            match (twin, all) {
                (true, true) => classic::run_twin_all(&folder, dry_run, recipe),
                (true, false) => classic::run_twin(&folder, dry_run, recipe),
                (false, true) => classic::run_all(&folder, dry_run, recipe),
                (false, false) => classic::run(&folder, dry_run, recipe),
            }
        }
        Command::Redesign { chart, all } => {
            if all {
                redesign::run_redesign_all(&chart)
            } else {
                redesign::run_redesign(&chart)
            }
        }
        Command::Loudness { path, all, write } => {
            if all {
                loudness::run_all(&path, write)
            } else {
                loudness::run(&path, write)
            }
        }
        Command::Vocals {
            path,
            all,
            force,
            status,
        } => vocals::run(&path, &vocals::Args { all, force, status }),
        Command::SetGenre { chart, genre } => set_genre(&chart, &genre),
        Command::Players { add } => players::run(add.as_deref()),
        Command::Stats { player } => players::stats(player.as_deref()),
        Command::Awards { player, locked } => players::awards(player.as_deref(), locked),
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
        Command::Decode { audio, out } => decode(&audio, out),
        #[cfg(feature = "ml")]
        Command::Align {
            audio,
            lyrics,
            out,
            raw,
            vocals,
            separator,
            keep_better,
            no_anchors,
        } => align::run(
            &audio,
            &lyrics,
            align::Args {
                out,
                raw,
                vocals,
                separator,
                keep_better,
                no_anchors,
            },
        ),
        #[cfg(feature = "ml")]
        Command::LyricsEval {
            corpus,
            out,
            limit,
            language,
            raw,
            anchors,
            jitter,
            shift,
            tolerance,
            vocals_dir,
        } => lyrics_eval::run(
            corpus, out, limit, language, raw, anchors, jitter, shift, tolerance, vocals_dir,
        ),
        #[cfg(feature = "ml")]
        Command::LyricsCheck { audio, words, out } => lyrics_eval::check(&audio, words, out),
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
    println!(
        "  downbeats     {:>8}   {}",
        analysis.downbeats.len(),
        if analysis.downbeats.is_empty() {
            "(no meter model: bars counted in fours from the first beat)"
        } else {
            "(from the local meter model)"
        }
    );
    println!("  onsets        {:>8}", analysis.onsets.len());
    let covered: usize = analysis.repeats.iter().map(|r| 2 * r.beats).sum();
    println!(
        "  repeats       {:>8}   ({:.0} % of the beats are the same music twice)",
        analysis.repeats.len(),
        100.0 * covered as f64 / analysis.beats.len().max(1) as f64
    );
    for r in &analysis.repeats {
        let at = |i: usize| {
            analysis
                .beats
                .get(i)
                .copied()
                .unwrap_or(analysis.duration_s)
        };
        println!(
            "                {:>7.1} s – {:>6.1} s  =  {:>6.1} s – {:>6.1} s   ({} beats, {:.2})",
            at(r.first_beat),
            at(r.first_beat + r.beats),
            at(r.second_beat),
            at(r.second_beat + r.beats),
            r.beats,
            r.similarity
        );
    }
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
    // What the analysis said at each note, kept beside the chart so
    // that a miss recorded a year from now can still be asked what
    // the song was doing there (ADR-0018 §20).
    match beatbyte_chart::context::save_context(
        &out_path,
        &beatbyte_chart::context_for(&chart, &analysis),
    ) {
        Ok(path) => println!("Context   `{}`", path.display()),
        Err(error) => eprintln!("context sidecar not written: {error}"),
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
    if let Some(folder) = chart_path.parent()
        && let Ok(audio) = beatbyte_chart::resolve_audio_path(folder, &chart.song.audio)
        && let Some(report) = beatbyte_audio::loudness::read_report(&audio)
    {
        let m = &report.measurement;
        println!(
            "  loudness    {} LUFS, {:.1} dBTP, gain {:+.1} dB{}; audio {}{}",
            m.integrated_lufs
                .map_or("n/a".to_owned(), |l| format!("{l:.1}")),
            m.true_peak_dbtp,
            report.gain_db(),
            if report.peak_limited() {
                " (peak-limited)"
            } else {
                ""
            },
            report.quality.verdict.label(),
            report
                .quality
                .issues
                .first()
                .map_or(String::new(), |i| format!(" — {}", i.what))
        );
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
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    meter(&mut analysis, &audio);
    Ok((analysis, trim))
}

/// Fold the local beat/downbeat model into an analysis when this
/// build carries the runtime AND the user has installed the model
/// pair (`models install beat-this-mel` + `beat-this`); otherwise the
/// analysis stays the analyzer's. Says on stderr what it did.
#[cfg(feature = "ml")]
pub(crate) fn meter(analysis: &mut beatbyte_core::SongAnalysis, audio: &beatbyte_audio::AudioData) {
    match beatbyte_meter::refine(analysis, audio, beatbyte_meter::DEFAULT_POLICY) {
        Ok(Some(applied)) => eprintln!("meter: {}", applied.summary()),
        Ok(None) => {}
        Err(error) => eprintln!("meter: {error}; charting without downbeats"),
    }
}

/// Without the `ml` feature there is no model to fold in.
#[cfg(not(feature = "ml"))]
pub(crate) fn meter(
    _analysis: &mut beatbyte_core::SongAnalysis,
    _audio: &beatbyte_audio::AudioData,
) {
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
    // Where a session comes from is decided in one place
    // (`telemetry::sessions_for`): the store when there is one, the
    // old files when there is not, and exactly the named directory
    // when one is named.
    let wanted_difficulty = difficulty.to_lowercase();
    let (sessions, source) = match telemetry::sessions_for(
        &chart.song.title,
        &chart.song.artist,
        &wanted_difficulty,
        telemetry_dir.as_deref(),
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("(no sessions recorded yet? play the song first)");
            return ExitCode::from(2);
        }
    };
    println!("evidence: {source}");

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
    if !outcome.comments.is_empty() {
        println!("what the player said:");
        for comment in &outcome.comments {
            println!("  \u{201c}{comment}\u{201d}");
        }
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
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    meter(&mut analysis, &audio);

    // Evidence, straight from the telemetry — same loader as
    // `review`, so the two cannot disagree about what was played or
    // about where it was read from. A dossier is still worth writing
    // for a song nobody has played, so a missing source is empty
    // evidence rather than an error.
    let sessions = telemetry::sessions_for(
        &chart.song.title,
        &chart.song.artist,
        &difficulty.to_lowercase(),
        telemetry_dir.as_deref(),
    )
    .map(|(sessions, _)| sessions)
    .unwrap_or_default();
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

/// `decode`: the song as the game hears it, as a WAV.
fn decode(audio_path: &Path, out: Option<PathBuf>) -> ExitCode {
    let audio = match decode_file(audio_path) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    let out = out.unwrap_or_else(|| audio_path.with_extension("decoded.wav"));
    if let Err(error) = beatbyte_audio::decode::write_wav_mono16(&out, &audio) {
        eprintln!("cannot write `{}`: {error}", out.display());
        return ExitCode::from(1);
    }
    let priming = audio.priming();
    println!(
        "wrote {} — {:.3} s mono at {} Hz, {} priming sample(s) skipped",
        out.display(),
        audio.duration_s(),
        audio.sample_rate(),
        priming.samples
    );
    ExitCode::SUCCESS
}
