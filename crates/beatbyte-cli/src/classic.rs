//! `classic`: apply one classic ingredient to a song folder.
//!
//! The ingredient is written as a NEW version beside the active one,
//! with the active one as its parent, and the pointer is moved. That
//! is what makes the blind test (`T` in the browser) usable: it plays
//! the active version against its parent, so after this the two sides
//! of the test differ by exactly this ingredient and nothing else.
//!
//! ⚠️ Nothing is written while the game is running — a chart file
//! swapped under a live browser turns every Enter on that song into
//! "cannot load" until a rescan. The caller is told, not guessed at.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_chart::context;
use beatbyte_chart::schema::Provenance;
use beatbyte_chart::{ChartFile, chart_hash, classic, load_chart_file, save_chart_file, versions};
use beatbyte_core::Difficulty;

/// The window the blind test plays, so the counts reported here are
/// counts of what the player will actually meet.
const WINDOW_S: f64 = 30.0;

/// The difficulty the window is chosen for. Hard is what these runs
/// are played on.
const WINDOW_DIFFICULTY: Difficulty = Difficulty::Hard;

/// Where the originals go before anything is written.
const BACKUP_DIR: &str = "local/classic-backup";

/// Who a classic version says wrote it.
const DESIGNER: &str = "classic";

/// Which ingredient this version carries.
const HOPO_DIRECTIVE: &str = "classic-hopo";

/// Apply the HOPO ingredient to one song folder.
pub fn run(folder: &Path, dry_run: bool) -> ExitCode {
    match one(folder, dry_run) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("{}: {reason}", folder.display());
            ExitCode::from(1)
        }
    }
}

/// Apply it to every song folder under `dir`.
pub fn run_all(dir: &Path, dry_run: bool) -> ExitCode {
    let mut folders: Vec<PathBuf> = match std::fs::read_dir(dir) {
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
    let (mut done, mut failed) = (0usize, 0usize);
    for folder in folders {
        match one(&folder, dry_run) {
            Ok(report) => {
                println!("{report}");
                done += 1;
            }
            Err(reason) => {
                eprintln!("{}: {reason}", folder.display());
                failed += 1;
            }
        }
    }
    println!("{done} folder(s) read, {failed} failed");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// How many notes inside `window` carry a different flag.
///
/// Pure — tested. The two charts must be the same notes in the same
/// order, which is what the ingredient guarantees.
#[must_use]
pub fn changed_in_window(
    before: &ChartFile,
    after: &ChartFile,
    difficulty: Difficulty,
    window: (f64, f64),
) -> usize {
    let notes = |chart: &ChartFile| {
        chart
            .charts
            .iter()
            .find(|c| c.difficulty == difficulty)
            .map(|c| c.notes.clone())
            .unwrap_or_default()
    };
    let (old, new) = (notes(before), notes(after));
    old.iter()
        .zip(&new)
        .filter(|(a, b)| a.hopo != b.hopo)
        .filter(|(a, _)| a.time >= window.0 && a.time < window.1)
        .count()
}

fn one(folder: &Path, dry_run: bool) -> Result<String, String> {
    if !dry_run && game_is_running() {
        return Err(
            "BeatByte is running — a chart swapped under a live browser breaks every \
             Enter on that song until a rescan. Quit the game first."
                .to_owned(),
        );
    }
    let names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("cannot list: {error}"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if !names.iter().any(|n| n == versions::BASE_CHART) {
        return Err(format!("no `{}` — not a song folder", versions::BASE_CHART));
    }
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active_path = folder.join(&active_name);
    let before = load_chart_file(&active_path)
        .map_err(|error| format!("cannot load `{active_name}`: {error}"))?;

    let mut after = before.clone();
    let changed = classic::apply_hopo_rule(&mut after);
    // ⚠️ The new version says where it came from. Copied unchanged,
    // the twin's own provenance would have this file claiming to be
    // a guitar study of the ORIGINAL song's chart — it is a classic
    // pass over the twin's active version, and the parent hash is
    // what a later reader walks back along.
    after.provenance = Some(Provenance {
        parent_hash: chart_hash(&before),
        designer: DESIGNER.to_owned(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0)),
        directive: Some(HOPO_DIRECTIVE.to_owned()),
    });
    let window = before.preview_window(WINDOW_DIFFICULTY, WINDOW_S);
    let in_window = changed_in_window(&before, &after, WINDOW_DIFFICULTY, window);

    let title = &before.song.title;
    let per_difficulty: Vec<String> = changed
        .iter()
        .map(|(difficulty, n)| format!("{} {n}", difficulty.id()))
        .collect();
    let total: usize = changed.iter().map(|(_, n)| *n).sum();
    let head = format!(
        "{title} [{active_name}] {} BPM — {} flag(s): {}; {in_window} in the test window \
         {:.1}–{:.1}s ({})",
        before.song.bpm,
        total,
        per_difficulty.join(", "),
        window.0,
        window.1,
        WINDOW_DIFFICULTY.id()
    );
    if dry_run {
        return Ok(format!("{head} — dry run, nothing written"));
    }
    if total == 0 {
        return Ok(format!("{head} — nothing to change, nothing written"));
    }

    // The original, kept outside the library before anything moves.
    back_up(&active_path, folder)?;

    let next_name = versions::next_version_name(&names);
    save_chart_file(&folder.join(&next_name), &after)
        .map_err(|error| format!("cannot write `{next_name}`: {error}"))?;
    std::fs::write(
        folder.join(versions::POINTER_FILE),
        format!("{{\"active\": \"{next_name}\"}}\n"),
    )
    .map_err(|error| format!("cannot write the pointer: {error}"))?;
    // The analysis sidecar travels with the version. A failure here
    // costs a dimension in a later reading and must not cost the
    // write that already happened.
    let carried = match carry_context(&active_path, &before, &folder.join(&next_name), &after) {
        Ok(true) => " with its analysis sidecar",
        Ok(false) => " (the parent had no current sidecar)",
        Err(reason) => {
            eprintln!("{}: sidecar not carried: {reason}", folder.display());
            " (sidecar not carried — see above)"
        }
    };
    Ok(format!(
        "{head} — wrote `{next_name}` (parent `{active_name}`){carried} and made it active"
    ))
}

/// Carry the parent version's analysis sidecar over to the new one.
///
/// ⚠️ A file copy would be ignored, not wrong-but-useful: a sidecar
/// names the chart it describes by content hash, and one whose hash
/// does not match is discarded as describing a chart that no longer
/// exists. So the hash is rewritten — which is only honest because
/// this ingredient changes FLAGS and nothing else, so the track
/// events are the same events in the same order and one packed entry
/// still describes one of them.
///
/// ⚠️ Verified rather than assumed: the entry count per difficulty is
/// checked against the new chart's own tracks first. A sidecar that
/// does not line up is worse than none — every later reading would
/// attribute one note's analysis to another.
///
/// `Ok(false)` means the parent had no current sidecar; there was
/// nothing to carry and that is not a failure.
///
/// # Errors
/// When the entries do not line up, or the write fails.
fn carry_context(
    parent_path: &Path,
    parent: &ChartFile,
    next_path: &Path,
    next: &ChartFile,
) -> Result<bool, String> {
    let Some(mut sidecar) = context::load_context(parent_path, parent) else {
        return Ok(false);
    };
    for definition in &next.charts {
        let id = definition.difficulty.id();
        let events = next
            .to_track(definition.difficulty)
            .map(|track| track.events().len())
            .map_err(|error| format!("{id}: cannot read the new track: {error}"))?;
        let entries = sidecar.tracks.get(id).map_or(0, Vec::len);
        if entries != events {
            return Err(format!(
                "{id}: the parent's sidecar describes {entries} events, the new version has \
                 {events} — refusing to write one that does not line up"
            ));
        }
    }
    sidecar.chart_hash = beatbyte_chart::chart_hash(next);
    debug_assert!(sidecar.is_current_for(next));
    context::save_context(next_path, &sidecar)
        .map(|_| true)
        .map_err(|error| format!("cannot write the sidecar: {error}"))
}

/// Copy the file about to be superseded into `local/`.
///
/// ⚠️ Keyed by the FOLDER as well as the file name: every song folder
/// has a `chart.json`, and a backup keyed by base name alone loses
/// the second one it meets — this project has lost an original that
/// way once already.
fn back_up(chart: &Path, folder: &Path) -> Result<(), String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let key = folder
        .file_name()
        .map_or_else(|| "song".to_owned(), |n| n.to_string_lossy().into_owned());
    let name = chart.file_name().map_or_else(
        || "chart.json".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let dir = PathBuf::from(BACKUP_DIR).join(format!("{stamp}-{key}"));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot make the backup dir: {error}"))?;
    std::fs::copy(chart, dir.join(&name))
        .map_err(|error| format!("cannot back up `{name}`: {error}"))?;
    // The pointer too: restoring means putting both back.
    let pointer = chart.with_file_name(versions::POINTER_FILE);
    if pointer.is_file() {
        let _ = std::fs::copy(&pointer, dir.join(versions::POINTER_FILE));
    }
    Ok(())
}

/// Whether the game is running right now.
fn game_is_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "beatbyte"])
        .output()
        .is_ok_and(|out| !out.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::context::ChartContext;
    use beatbyte_chart::schema::ChartNote;

    fn chart(flags: &[(f64, bool)]) -> ChartFile {
        let notes: Vec<ChartNote> = flags
            .iter()
            .enumerate()
            .map(|(i, (time, hopo))| ChartNote {
                time: *time,
                lane: (i % 5) as u8,
                len: 0.0,
                hopo: *hopo,
            })
            .collect();
        ChartFile::from_json(&format!(
            r#"{{"format_version":1,
                "song":{{"title":"T","artist":"A","audio":"a.m4a","bpm":120.0,
                         "duration_s":100.0,"offset_s":0.0}},
                "charts":[{{"difficulty":"hard","lanes":5,"notes":{},"phrases":[]}}]}}"#,
            serde_json::to_string(&notes).expect("notes serialize")
        ))
        .expect("the fixture parses")
    }

    /// ⚠️ The number the reference songs are chosen by. A song whose
    /// flags all change OUTSIDE the thirty seconds the blind test
    /// plays would be tested on a passage where nothing differs —
    /// the player would hear two identical charts and rightly call
    /// it a tie.
    #[test]
    fn only_the_flags_inside_the_test_window_are_counted() {
        let before = chart(&[(1.0, false), (20.0, false), (40.0, false)]);
        let after = chart(&[(1.0, true), (20.0, true), (40.0, true)]);
        assert_eq!(
            changed_in_window(&before, &after, Difficulty::Hard, (0.0, 30.0)),
            2,
            "a change outside the window was counted"
        );
        // The window is half-open: its end belongs to the next one.
        assert_eq!(
            changed_in_window(&before, &after, Difficulty::Hard, (20.0, 40.0)),
            1
        );
        // Unchanged flags are not counted however many there are.
        assert_eq!(
            changed_in_window(&before, &before, Difficulty::Hard, (0.0, 100.0)),
            0
        );
    }

    /// ⚠️ A sidecar names the chart it describes by content hash, so
    /// a straight copy would be DISCARDED — "this describes a chart
    /// that no longer exists" — and the new version would silently
    /// have no analysis behind it. Rewriting the hash is honest here
    /// only because the ingredient changes flags and nothing else.
    #[test]
    fn the_parents_sidecar_is_carried_over_and_still_describes_the_new_version() {
        let dir = std::env::temp_dir().join(format!(
            "bb-classic-ctx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        // 120 BPM: a beat is 0.5 s, so the threshold is 0.177 s.
        // ⚠️ 0.2 s was the first try and changed nothing — 0.4 of a
        // beat is over it. The guard below caught the fixture.
        let parent = chart(&[(0.0, false), (0.15, false), (1.0, false)]);
        let mut next = parent.clone();
        beatbyte_chart::classic::apply_hopo_rule(&mut next);
        assert_ne!(
            beatbyte_chart::chart_hash(&parent),
            beatbyte_chart::chart_hash(&next),
            "the fixture changed no flag, so it proves nothing"
        );

        let parent_path = dir.join("chart.json");
        let next_path = dir.join("chart.v2.json");
        beatbyte_chart::save_chart_file(&parent_path, &parent).expect("the parent");
        beatbyte_chart::save_chart_file(&next_path, &next).expect("the new version");

        // A sidecar for the parent: one entry per track event.
        let events = parent
            .to_track(Difficulty::Hard)
            .expect("a track")
            .events()
            .len();
        let mut tracks = std::collections::BTreeMap::new();
        tracks.insert(Difficulty::Hard.id().to_owned(), vec![[7u8; 6]; events]);
        let sidecar = ChartContext {
            format: beatbyte_chart::context::CONTEXT_FORMAT.to_owned(),
            chart_hash: beatbyte_chart::chart_hash(&parent),
            tracks,
        };
        context::save_context(&parent_path, &sidecar).expect("the parent's sidecar");

        assert_eq!(
            carry_context(&parent_path, &parent, &next_path, &next),
            Ok(true)
        );
        // The point: the reader accepts it for the NEW chart.
        let read = context::load_context(&next_path, &next)
            .expect("the carried sidecar must describe the new version");
        assert_eq!(read.len(), events, "the entries did not travel");

        // And a sidecar that does not line up is refused rather than
        // written: every later reading would blame the wrong note.
        let mut wrong = sidecar.clone();
        wrong
            .tracks
            .get_mut(Difficulty::Hard.id())
            .expect("the track")
            .pop();
        context::save_context(&parent_path, &wrong).expect("a short sidecar");
        let outcome = carry_context(&parent_path, &parent, &next_path, &next);
        assert!(
            matches!(&outcome, Err(reason) if reason.contains("does not line up")),
            "a mismatched sidecar was accepted: {outcome:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A difficulty the chart does not carry counts nothing rather
    /// than panicking.
    #[test]
    fn a_missing_difficulty_counts_nothing() {
        let before = chart(&[(1.0, false)]);
        let after = chart(&[(1.0, true)]);
        assert_eq!(
            changed_in_window(&before, &after, Difficulty::Expert, (0.0, 30.0)),
            0
        );
    }
}
