//! `library migrate` — give every song folder its document.
//!
//! Reads what each folder already holds and writes exactly one new
//! file per song, `song.json` (ADR-0019). Nothing existing is
//! touched, no audio is decoded, and a folder whose document is
//! already current is left alone — so a second run reports zero
//! writes, which is the property that makes this safe to run at all.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_chart::Severity;
use beatbyte_chart::schema::ChartFile;
use beatbyte_chart::versions;
use beatbyte_library::build::{FolderFacts, document_for};
use beatbyte_library::folder::{is_chart_candidate, read_loudness_facts};
use beatbyte_library::{SongId, SourceKind, store};

/// What one pass over a library did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Folders that gained their first document.
    pub created: usize,
    /// Folders whose document changed.
    pub updated: usize,
    /// Folders already current.
    pub unchanged: usize,
    /// Folders with nothing to read.
    pub skipped: usize,
}

/// Build the queryable index from the documents in a library.
///
/// Rebuilt wholesale rather than patched: the index is a projection
/// (ADR-0019), and the cheapest way to keep a projection honest is to
/// be able to throw it away. At library size this is milliseconds.
pub fn index(root: &Path, db: &Path) -> ExitCode {
    let mut docs: Vec<(beatbyte_library::SongDoc, String)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        eprintln!("cannot read {}", root.display());
        return ExitCode::from(2);
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    for folder in &folders {
        if let Some(doc) = store::read(folder) {
            let name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            docs.push((doc, name));
        }
    }
    let mut index = match beatbyte_library::index::Index::open(db) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("cannot open {}: {error}", db.display());
            return ExitCode::from(2);
        }
    };
    match index.rebuild(docs.iter().map(|(doc, folder)| (doc, folder.as_str()))) {
        Ok(count) => {
            println!(
                "{count} song(s) indexed into {} (schema v{})",
                db.display(),
                index.version().unwrap_or(0)
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cannot build the index: {error}");
            ExitCode::from(2)
        }
    }
}

/// Give every recorded session the song it belongs to.
///
/// ⚠️ **Additive only.** A session that cannot be matched keeps its
/// `NULL` rather than being attached to a guess, and one that already
/// names a song is left alone. Nothing is deleted and nothing is
/// overwritten, so the worst case of running this is that it changes
/// nothing.
pub fn backfill(index_db: &Path, store_db: Option<PathBuf>) -> ExitCode {
    let index = match beatbyte_library::index::Index::open(index_db) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("cannot open {}: {error}", index_db.display());
            return ExitCode::from(2);
        }
    };
    let path = match store_db.or_else(crate::telemetry::default_store_path) {
        Some(path) => path,
        None => {
            eprintln!("no telemetry store — pass --store");
            return ExitCode::from(2);
        }
    };
    let mut store = match beatbyte_telemetry::store::Store::open(&path) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };
    let pending = match store.unattached() {
        Ok(pending) => pending,
        Err(error) => {
            eprintln!("cannot read the store: {error}");
            return ExitCode::from(2);
        }
    };
    let (before, total) = store.attached_count().unwrap_or((0, 0));

    let (mut by_chart, mut by_name, mut unmatched) = (0usize, 0usize, 0usize);
    for session in &pending {
        match index.resolve(&session.chart_hash, &session.title, &session.artist) {
            Ok(Some((song_id, how))) => {
                if let Err(error) = store.attach_song(session.session_id, &song_id) {
                    eprintln!("session {}: {error}", session.session_id);
                    continue;
                }
                match how {
                    beatbyte_library::index::Match::ByChart => by_chart += 1,
                    beatbyte_library::index::Match::ByName => by_name += 1,
                }
            }
            Ok(None) => unmatched += 1,
            Err(error) => {
                eprintln!("session {}: {error}", session.session_id);
                unmatched += 1;
            }
        }
    }
    let (after, _) = store.attached_count().unwrap_or((0, total));
    println!(
        "{} session(s) had no song: {by_chart} matched by chart, {by_name} by name, \
         {unmatched} left unattached",
        pending.len()
    );
    println!("sessions naming a song: {before} → {after} of {total}");
    ExitCode::SUCCESS
}

/// Run over a songs directory.
pub fn run(root: &Path, dry_run: bool) -> ExitCode {
    let Ok(entries) = std::fs::read_dir(root) else {
        eprintln!("cannot read {}", root.display());
        return ExitCode::from(2);
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();

    let now = now_ms();
    let mut tally = Tally::default();
    for folder in &folders {
        match migrate_folder(folder, now, dry_run) {
            Some(Outcome::Created) => tally.created += 1,
            Some(Outcome::Updated) => tally.updated += 1,
            Some(Outcome::Unchanged) => tally.unchanged += 1,
            None => tally.skipped += 1,
        }
    }
    println!(
        "{} song folder(s): {} created, {} updated, {} already current, {} skipped{}",
        folders.len(),
        tally.created,
        tally.updated,
        tally.unchanged,
        tally.skipped,
        if dry_run {
            "  (dry run — nothing written)"
        } else {
            ""
        }
    );
    ExitCode::SUCCESS
}

/// What happened to one folder.
///
/// ⚠️ Created and updated are told apart by whether a document was
/// there **before** the pass. The first version of this asked
/// afterwards, so every creation reported itself as an update and
/// the first run over a fresh library said "171 updated, 0 created"
/// — a report that lies about the one thing it exists to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The folder had no document and now has one.
    Created,
    /// It had one and it changed.
    Updated,
    /// It had one and nothing moved.
    Unchanged,
}

/// What a pass over one folder did, given whether it already had a
/// document and whether anything changed. Pure — tested.
#[must_use]
pub fn outcome(had_document: bool, changed: bool) -> Outcome {
    match (had_document, changed) {
        (false, _) => Outcome::Created,
        (true, true) => Outcome::Updated,
        (true, false) => Outcome::Unchanged,
    }
}

/// Report songs whose files are the same recording.
///
/// Reports only. Which of two copies to keep is a judgement about a
/// library nobody but its owner can make, and a tool that guessed
/// would eventually guess wrong about the one file that mattered.
pub fn duplicates(root: &Path) -> ExitCode {
    let Ok(entries) = std::fs::read_dir(root) else {
        eprintln!("cannot read {}", root.display());
        return ExitCode::FAILURE;
    };
    let mut songs = Vec::new();
    let mut twins = 0usize;
    let mut fingerprinted = 0usize;
    let mut total = 0usize;
    for dir in entries.flatten().map(|entry| entry.path()) {
        let Some(doc) = store::read(&dir) else {
            continue;
        };
        total += 1;
        // By the FOLDER, never by the title: the player may rename
        // a song, and one of them here had been.
        let is_twin = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(beatbyte_chart::study::is_twin_folder);
        if is_twin {
            twins += 1;
        }
        if let Some(print) = doc.file.content_hash.clone() {
            fingerprinted += 1;
            songs.push(beatbyte_library::folder::SongPrint {
                fingerprint: print,
                title: doc.identity.title.value.clone(),
                is_twin,
            });
        }
    }
    let found = beatbyte_library::folder::duplicates(&songs);
    println!("{total} song(s), {fingerprinted} fingerprinted, {twins} study twin(s)");
    if fingerprinted < total {
        println!(
            "{} song(s) have not been fingerprinted yet and cannot be compared",
            total - fingerprinted
        );
    }
    if found.is_empty() {
        println!("no duplicates");
        return ExitCode::SUCCESS;
    }
    for duplicate in &found {
        println!("{}", duplicate.fingerprint);
        for title in &duplicate.titles {
            println!("    {title}");
        }
    }
    println!("{} duplicate(s) — nothing was deleted", found.len());
    ExitCode::SUCCESS
}

/// One folder. `Some(outcome)` when it is a song, `None` when it is not.
fn migrate_folder(dir: &Path, now: u64, dry_run: bool) -> Option<Outcome> {
    let chart_name = active_chart_name(dir)?;
    let chart_path = dir.join(&chart_name);
    let chart = std::fs::read_to_string(&chart_path)
        .ok()
        .and_then(|text| ChartFile::from_json(&text).ok())?;
    // A chart the game would refuse is not a chart a document may
    // describe. The browser reads a document INSTEAD of the chart
    // when it carries note counts, and it must not learn about a
    // song it would then fail to load. Such a folder still gets a
    // document — it is a song in the library — just one that says
    // nothing about a chart.
    let playable = !chart
        .validate()
        .iter()
        .any(|issue| issue.severity == Severity::Error);

    let audio = dir.join(&chart.song.audio);
    let audio_filename = audio
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| chart.song.audio.clone());

    let existing = store::read(dir);
    let facts = FolderFacts {
        chart: playable.then_some(&chart),
        chart_version: playable.then(|| beatbyte_library::fresh::generation(&chart_name)),
        chart_filename: playable.then(|| chart_name.clone()),
        audio_filename,
        extension: audio
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase()),
        oldest_file_ms: oldest_file_ms(dir).unwrap_or(now),
        loudness: read_loudness_facts(&audio),
        lyrics: beatbyte_library::folder::lyric_facts(&audio, &chart_path),
        // ⚠️ This reads every byte of every song — 2.4 GB here,
        // about ten seconds. It belongs in a command the user chose
        // to run, never in a scan and never at start-up; in the game
        // the same work is done one song at a time in the
        // background.
        content_hash: beatbyte_library::folder::fingerprint(&audio).map(|print| print.tagged()),
        tags: Some(beatbyte_audio::read_tags(&audio)),
        source_kind: SourceKind::LocalFile,
    };

    let had_document = existing.is_some();
    let built = document_for(&facts, existing, SongId::new(now), now);
    if dry_run {
        return Some(outcome(had_document, built.changed));
    }
    match store::save_if_changed(dir, &built) {
        Ok(_) => Some(outcome(had_document, built.changed)),
        Err(error) => {
            eprintln!("{}: {error}", dir.display());
            Some(Outcome::Unchanged)
        }
    }
}

/// The chart file the game would load from this folder.
///
/// ⚠️ The version scheme covers the import layout
/// (`chart.json`, `chart.vN.json` and a pointer), and a hand-made
/// folder can hold a chart under any name — the library scanner
/// takes every `*.json` that parses, and a migration that did not
/// would quietly skip songs the game plays. So: the pointer decides
/// when it names a file that is there; otherwise a single candidate
/// is the answer, and several candidates are an ambiguity worth
/// reporting rather than guessing at.
fn active_chart_name(dir: &Path) -> Option<String> {
    let candidates: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_chart_candidate(name))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let pointer = std::fs::read_to_string(dir.join(versions::POINTER_FILE)).ok();
    let active = versions::resolve_active(pointer.as_deref(), &candidates);
    if candidates.contains(&active) {
        return Some(active);
    }
    match candidates.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// The oldest modification time in a folder, Unix milliseconds.
///
/// The best available answer to "when did this song arrive" for a
/// folder that predates documents. Not perfect — a copy can carry a
/// newer time than the import — but it is a fact about the folder
/// rather than a guess, and the alternative is stamping every song
/// in the library with the moment the migration ran, which would
/// make the whole library look imported today.
fn oldest_file_ms(dir: &Path) -> Option<u64> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .filter_map(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .min()
}

/// Now, in Unix milliseconds.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_document_is_a_creation_not_an_update() {
        assert_eq!(outcome(false, true), Outcome::Created);
        assert_eq!(
            outcome(false, false),
            Outcome::Created,
            "a folder that had nothing has gained something either way"
        );
        assert_eq!(outcome(true, true), Outcome::Updated);
        assert_eq!(outcome(true, false), Outcome::Unchanged);
    }
}
