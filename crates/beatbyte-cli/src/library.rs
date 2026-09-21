//! `library migrate` — give every song folder its document.
//!
//! Reads what each folder already holds and writes exactly one new
//! file per song, `song.json` (ADR-0019). Nothing existing is
//! touched, no audio is decoded, and a folder whose document is
//! already current is left alone — so a second run reports zero
//! writes, which is the property that makes this safe to run at all.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_chart::schema::ChartFile;
use beatbyte_chart::versions;
use beatbyte_library::build::{FolderFacts, LoudnessFacts, LyricFacts, document_for};
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

/// One folder. `Some(outcome)` when it is a song, `None` when it is not.
fn migrate_folder(dir: &Path, now: u64, dry_run: bool) -> Option<Outcome> {
    let chart_name = active_chart_name(dir)?;
    let chart_path = dir.join(&chart_name);
    let chart = std::fs::read_to_string(&chart_path)
        .ok()
        .and_then(|text| ChartFile::from_json(&text).ok())?;

    let audio = dir.join(&chart.song.audio);
    let audio_filename = audio
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| chart.song.audio.clone());
    let stem = audio.file_stem().map(std::ffi::OsStr::to_os_string);

    let existing = store::read(dir);
    let facts = FolderFacts {
        chart: Some(&chart),
        chart_version: versions::version_number(&chart_name).or(Some(1)),
        audio_filename,
        extension: audio
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase()),
        oldest_file_ms: oldest_file_ms(dir).unwrap_or(now),
        loudness: read_loudness(&audio),
        lyrics: LyricFacts {
            has_lrc: stem
                .as_ref()
                .is_some_and(|stem| dir.join(stem).with_extension("lrc").exists()),
            has_words: stem
                .as_ref()
                .is_some_and(|stem| dir.join(stem).with_extension("words.json").exists()),
        },
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

/// Whether a file name could be a chart rather than a sidecar.
///
/// Pure so the rule can be pinned: a sidecar mistaken for a chart
/// would be read, fail to parse and skip a real song.
#[must_use]
pub fn is_chart_candidate(name: &str) -> bool {
    name.ends_with(".json")
        && name != versions::POINTER_FILE
        && name != beatbyte_library::DOC_FILE
        && !name.ends_with(".context.json")
        && !name.ends_with(".loudness.json")
        && !name.ends_with(".words.json")
}

/// What the loudness sidecar says, in the fields a document keeps.
fn read_loudness(audio: &Path) -> Option<LoudnessFacts> {
    let report = beatbyte_audio::loudness::read_report(audio)?;
    Some(LoudnessFacts {
        bytes: Some(report.bytes),
        duration_s: Some(report.measurement.duration_s),
        integrated_lufs: report.measurement.integrated_lufs,
        loudness_range_lu: report.measurement.loudness_range_lu,
        sample_rate: Some(report.quality.sample_rate),
        channels: u16::try_from(report.quality.channels).ok(),
        bitrate_kbps: report.quality.bitrate_kbps,
        lossy: Some(report.quality.lossy),
    })
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

    #[test]
    fn a_sidecar_is_not_mistaken_for_a_chart() {
        assert!(is_chart_candidate("chart.json"));
        assert!(is_chart_candidate("chart.v4.json"));
        assert!(
            is_chart_candidate("girls.chart.json"),
            "a hand-made folder names its chart what it likes, and the \
             game plays it — so the migration must see it too"
        );
        for sidecar in [
            "chart.context.json",
            "chart.v4.context.json",
            "Song.loudness.json",
            "Song.words.json",
            "chart-active.json",
            "song.json",
        ] {
            assert!(!is_chart_candidate(sidecar), "{sidecar} is not a chart");
        }
    }
}
