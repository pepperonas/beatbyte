//! `beatbyte-cli telemetry` — reading the gameplay store (ADR-0018).
//!
//! Everything here is a read, an export or a one-time import. Nothing
//! writes a chart, a score or a setting: analytics produce evidence
//! for a person to act on, and that line is the point of the whole
//! design.
//!
//! It also holds the loader the older readers (`review`, `dossier`)
//! now go through, so that the one place which decides *where a
//! session comes from* is this one.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_telemetry::{Store, analytics, export, legacy};

use crate::review;

/// Where the store lives by default — beside `scores.json`, the same
/// place the game opens.
#[must_use]
pub fn default_store_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("telemetry.db"))
}

/// The older per-session files.
#[must_use]
pub fn default_jsonl_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("beatbyte").join("telemetry"))
}

/// Every recorded session for one song and difficulty.
///
/// The rule, in one place: an explicitly named directory means "read
/// those files"; otherwise the store is the source, and the old
/// directory is the fallback for a machine that has never opened one.
/// Both paths hand back the same shape, so the readers built on the
/// JSONL format did not have to change their logic to move.
pub fn sessions_for(
    title: &str,
    artist: &str,
    difficulty: &str,
    telemetry_dir: Option<&Path>,
) -> Result<(Vec<review::Session>, String), String> {
    if let Some(dir) = telemetry_dir {
        return Ok((
            from_dir(dir, title, artist, difficulty)?,
            dir.display().to_string(),
        ));
    }
    if let Some(path) = default_store_path()
        && path.is_file()
    {
        let store = Store::open(&path).map_err(|error| error.to_string())?;
        return Ok((
            from_store(&store, title, artist, difficulty)?,
            path.display().to_string(),
        ));
    }
    let Some(dir) = default_jsonl_dir() else {
        return Err("no data directory on this platform; pass --telemetry-dir".to_owned());
    };
    Ok((
        from_dir(&dir, title, artist, difficulty)?,
        dir.display().to_string(),
    ))
}

/// Sessions out of the store, in the shape the readers expect.
fn from_store(
    store: &Store,
    title: &str,
    artist: &str,
    difficulty: &str,
) -> Result<Vec<review::Session>, String> {
    let wanted = difficulty.to_lowercase();
    let mut out = Vec::new();
    // Every session of this song, whatever chart version it was
    // played on — the review reports the stale ones rather than
    // hiding them, so the filter must not drop them here.
    let stored = store
        .sessions(usize::MAX)
        .map_err(|error| error.to_string())?;
    for session in stored {
        if session.row.title != title || session.row.artist != artist {
            continue;
        }
        let events = store
            .events(session.id)
            .map_err(|error| error.to_string())?;
        let notes = store.notes(session.id).map_err(|error| error.to_string())?;
        let (header, lines) = legacy::to_legacy(&session, &events, &notes);
        if header.difficulty == wanted {
            out.push(review::Session { header, lines });
        }
    }
    Ok(out)
}

/// Sessions out of a directory of JSONL files.
fn from_dir(
    dir: &Path,
    title: &str,
    artist: &str,
    difficulty: &str,
) -> Result<Vec<review::Session>, String> {
    let wanted = difficulty.to_lowercase();
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read `{}`: {error}", dir.display()))?;
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "jsonl")
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((header, lines)) = beatbyte_core::telemetry::parse_session(&content) else {
            continue;
        };
        if header.title == title && header.artist == artist && header.difficulty == wanted {
            out.push(review::Session { header, lines });
        }
    }
    Ok(out)
}

/// Open the store, or say why not.
fn open(path: Option<PathBuf>) -> Result<(Store, PathBuf), String> {
    let path = path
        .or_else(default_store_path)
        .ok_or_else(|| "no data directory on this platform; pass --store".to_owned())?;
    let store = Store::open(&path).map_err(|error| error.to_string())?;
    Ok((store, path))
}

/// `telemetry status`.
pub fn run_status(path: Option<PathBuf>) -> ExitCode {
    let (store, path) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    println!("store: {}", path.display());
    match store.version() {
        Ok(version) => println!("schema: v{version}"),
        Err(error) => println!("schema: unreadable ({error})"),
    }
    match export::overview(&store) {
        Ok(text) => print!("{text}"),
        Err(error) => return fail(&error.to_string()),
    }
    match analytics::incomplete_sessions(&store) {
        Ok(rows) if rows.is_empty() => println!("no session lost an event"),
        Ok(rows) => {
            println!("sessions with a hole in them:");
            for (uid, dropped) in rows.iter().take(20) {
                println!("  {uid}  {dropped} event(s) dropped");
            }
        }
        Err(error) => return fail(&error.to_string()),
    }
    ExitCode::SUCCESS
}

/// `telemetry import`.
pub fn run_import(path: Option<PathBuf>, dir: Option<PathBuf>) -> ExitCode {
    let (mut store, path) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let Some(dir) = dir.or_else(default_jsonl_dir) else {
        return fail("no telemetry directory on this platform; pass --dir");
    };
    println!("importing {} into {}", dir.display(), path.display());
    match legacy::import_dir(&mut store, &dir) {
        Ok(report) => {
            println!(
                "{} file(s): {} imported ({} observations), {} already there, {} unreadable",
                report.seen,
                report.imported,
                report.events,
                report.already_there,
                report.unreadable
            );
            if report.unreadable > 0 {
                println!("(a file without a session header is skipped, not fatal)");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// `telemetry list`.
pub fn run_list(path: Option<PathBuf>, limit: usize) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match store.sessions(limit) {
        Ok(sessions) => {
            for session in sessions {
                println!(
                    "{:>5}  {}  {} — {} <{}>  {} {} note(s){}",
                    session.id,
                    session.row.uid,
                    session.row.title,
                    session.row.artist,
                    session.row.difficulty,
                    session.completion.label(),
                    session.row.notes_total,
                    if session.complete {
                        String::new()
                    } else {
                        format!("  [INCOMPLETE: {} dropped]", session.dropped)
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// `telemetry show`.
pub fn run_show(path: Option<PathBuf>, session: i64) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match export::session_report(&store, session) {
        Ok(text) if text.is_empty() => {
            eprintln!("no session {session} in the store");
            ExitCode::from(2)
        }
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// What an export writes.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportWhat {
    /// One row per session.
    Sessions,
    /// One row per judged note, with the session context joined on.
    Dataset,
    /// One session, as JSON.
    Session,
}

/// `telemetry export`.
pub fn run_export(
    path: Option<PathBuf>,
    what: ExportWhat,
    session: Option<i64>,
    limit: usize,
    out: Option<PathBuf>,
) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let rendered = match what {
        ExportWhat::Sessions => export::sessions_csv(&store, limit),
        ExportWhat::Dataset => export::dataset_csv(&store, limit),
        ExportWhat::Session => {
            let Some(session) = session else {
                return fail("`--what session` needs `--session <id>`");
            };
            export::session_json(&store, session)
        }
    };
    let text = match rendered {
        Ok(text) => text,
        Err(error) => return fail(&error.to_string()),
    };
    match out {
        Some(path) => match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => {
                println!("wrote {}", path.display());
                ExitCode::SUCCESS
            }
            Err(error) => fail(&format!("cannot write `{}`: {error}", path.display())),
        },
        None => {
            print!("{text}");
            ExitCode::SUCCESS
        }
    }
}

/// `telemetry problems`.
pub fn run_problems(
    path: Option<PathBuf>,
    chart_hash: &str,
    difficulty: u8,
    min_samples: u32,
    max_hit_rate: f64,
    limit: usize,
) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match analytics::problem_notes(
        &store,
        chart_hash,
        difficulty,
        min_samples,
        max_hit_rate,
        limit,
    ) {
        Ok(notes) if notes.is_empty() => {
            println!(
                "no note of {chart_hash} at difficulty {difficulty} is missed more than \
                 {:.0}% of the time with at least {min_samples} plays behind it",
                (1.0 - max_hit_rate) * 100.0
            );
            ExitCode::SUCCESS
        }
        Ok(notes) => {
            println!("note   plays  hit    median   spread   over  confidence");
            for note in notes {
                println!(
                    "{:>5}  {:>5}  {:>4.0}%  {:>+6.1}  {:>6.1}  {:>4}  {:>6.0}%",
                    note.note_index,
                    note.samples,
                    note.hit_rate * 100.0,
                    note.median_offset_ms,
                    note.offset_spread_ms,
                    note.overstrums,
                    note.confidence() * 100.0
                );
            }
            println!(
                "\n(a single miss means nothing; `confidence` is a saturating curve on the \
                 sample count, not a statistical interval)"
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// `telemetry generators`.
pub fn run_generators(path: Option<PathBuf>, genre: Option<String>, difficulty: u8) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match analytics::generator_comparison(&store, genre.as_deref(), difficulty) {
        Ok(rows) if rows.is_empty() => {
            println!("nothing recorded for that genre at difficulty {difficulty}");
            ExitCode::SUCCESS
        }
        Ok(rows) => {
            println!("generator             sessions  judged  miss    |off|   in finished runs");
            for row in rows {
                println!(
                    "{:<20}  {:>8}  {:>6}  {:>4.1}%  {:>5.1}ms  {:>6.0}%",
                    row.generator.as_deref().unwrap_or("(the import's own)"),
                    row.sessions,
                    row.judged,
                    row.miss_rate * 100.0,
                    row.mean_abs_ms,
                    row.completion_rate * 100.0
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// `telemetry calibration`.
pub fn run_calibration(path: Option<PathBuf>, player: Option<u64>, min_hits: u32) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match analytics::calibration_bias(&store, player, min_hits) {
        Ok(rows) if rows.is_empty() => {
            println!("not enough hits yet ({min_hits} needed per device and offset)");
            ExitCode::SUCCESS
        }
        Ok(rows) => {
            println!("device     offset     hits  median   would centre at");
            for row in rows {
                println!(
                    "{:<9}  {:>+6.1}ms  {:>5}  {:>+5.1}ms  {:>+8.1}ms",
                    row.device.label(),
                    row.offset_ms.unwrap_or(0.0),
                    row.hits,
                    row.median_offset_ms,
                    row.suggested_offset_ms()
                );
            }
            println!(
                "\n(a suggestion, never applied: calibration is not changed from a handful \
                 of events)"
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

/// `telemetry input`.
pub fn run_input(path: Option<PathBuf>) -> ExitCode {
    let (store, _) = match open(path) {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    match analytics::orphan_strums(&store) {
        Ok(rows) if rows.is_empty() => {
            println!(
                "no strum recorded — the action stream needs the TELEMETRY setting at \
                 ACTIONS or above"
            );
            ExitCode::SUCCESS
        }
        Ok(rows) => {
            println!("device     strums  reached the engine and did nothing");
            for row in rows {
                let share = if row.strums > 0 {
                    f64::from(row.orphan_strums) / f64::from(row.strums) * 100.0
                } else {
                    0.0
                };
                println!(
                    "{:<9}  {:>6}  {:>6} ({share:.1}%)",
                    row.device.label(),
                    row.strums,
                    row.orphan_strums
                );
            }
            println!(
                "\n(a high share is a controller, a mapping or an engine problem — not a \
                 player one)"
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::telemetry::{NoteLine, SessionHeader, render_session};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-cli-telemetry-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        dir
    }

    fn a_file(dir: &Path, difficulty: &str) {
        let header = SessionHeader {
            schema: beatbyte_core::telemetry::SCHEMA_VERSION,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            difficulty: difficulty.to_owned(),
            chart_hash: "abc".to_owned(),
            generator: "0.14.0".to_owned(),
            started_ms: 1_700_000_000_000,
            player: 0,
            autopilot: false,
            notes_total: 2,
        };
        let lines = vec![
            NoteLine::Hit {
                i: 0,
                j: "great".to_owned(),
                off_ms: 12.0,
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
        ];
        std::fs::write(
            dir.join(format!("1700000000000-{difficulty}.jsonl")),
            render_session(&header, &lines),
        )
        .expect("writes");
    }

    #[test]
    fn a_named_directory_is_read_as_files() {
        let dir = scratch("dir");
        a_file(&dir, "medium");
        a_file(&dir, "hard");
        let (sessions, source) =
            sessions_for("Maria", "Blondie", "medium", Some(&dir)).expect("loads");
        assert_eq!(sessions.len(), 1, "the other difficulty is not this one");
        assert_eq!(sessions[0].lines.len(), 2);
        assert!(source.contains("beatbyte-cli-telemetry-dir"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_song_nobody_played_loads_as_nothing_rather_than_as_an_error() {
        let dir = scratch("empty");
        a_file(&dir, "medium");
        let (sessions, _) =
            sessions_for("Some Other Song", "Nobody", "medium", Some(&dir)).expect("loads");
        assert!(sessions.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Imported and then read back through the store: the readers
    /// built on the file format must see what they saw before.
    #[test]
    fn the_store_hands_back_what_the_files_held() {
        let dir = scratch("roundtrip");
        a_file(&dir, "medium");
        let (from_files, _) =
            sessions_for("Maria", "Blondie", "medium", Some(&dir)).expect("loads");

        let mut store = Store::open_in_memory().expect("a store");
        legacy::import_dir(&mut store, &dir).expect("imports");
        let from_database = from_store(&store, "Maria", "Blondie", "medium").expect("loads");

        assert_eq!(from_database.len(), 1);
        assert_eq!(
            from_database[0].header, from_files[0].header,
            "the header survives the import"
        );
        assert_eq!(
            from_database[0].lines, from_files[0].lines,
            "and so does every observation"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_is_an_error_a_person_can_read() {
        let error = sessions_for("Maria", "Blondie", "medium", Some(Path::new("/nope")))
            .expect_err("a directory that is not there");
        assert!(error.contains("/nope"), "{error}");
    }
}
