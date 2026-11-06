//! `relevel`: give a song whose lower levels are copies of the level
//! above real levels again.
//!
//! Some community charts carry one track pasted under several names,
//! or a stub of a few notes below a charted level (59 of 8 630 library
//! charts, 2026-11-06): choosing Easy played the Hard chart. The importer now derives such levels like missing
//! ones ([`beatbyte_chart::bridge::relevel`]); this command does the
//! same for songs imported before it did.
//!
//! The result is a new chart VERSION beside the active one (ADR-0011),
//! never an overwrite: the parent stays on disk, and the sessions
//! recorded against it keep their chart. A folder without copies is
//! left alone and says nothing, so running it over the whole library
//! twice writes nothing the second time.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_chart::schema::Provenance;
use beatbyte_chart::{
    ChartFile, Severity, bridge, chart_hash, context, load_chart_file, save_chart_file, versions,
};

const DESIGNER: &str = "relevel";

const GAME_RUNNING: &str = "BeatByte is running — a chart swapped under a live browser breaks every \
     Enter on that song until a rescan. Quit the game first.";

/// Re-level one song folder, or every song folder under `path` with
/// `all`.
pub fn run(path: &Path, all: bool, dry_run: bool) -> ExitCode {
    if !dry_run && game_is_running() {
        eprintln!("{GAME_RUNNING}");
        return ExitCode::from(1);
    }
    let folders: Vec<PathBuf> = if all {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut folders: Vec<PathBuf> = entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && !hidden(p))
                    .collect();
                folders.sort();
                folders
            }
            Err(error) => {
                eprintln!("cannot list `{}`: {error}", path.display());
                return ExitCode::from(2);
            }
        }
    } else {
        vec![path.to_path_buf()]
    };
    let (mut written, mut failed) = (0usize, 0usize);
    for folder in &folders {
        match one(folder, dry_run, now_ms()) {
            Ok(Some(report)) => {
                println!("{}: {report}", folder.display());
                written += 1;
            }
            Ok(None) => {}
            Err(reason) => {
                eprintln!("{}: {reason}", folder.display());
                failed += 1;
            }
        }
    }
    let verb = if dry_run {
        "would re-level"
    } else {
        "re-levelled"
    };
    println!(
        "{} folder(s) read, {verb} {written}, {failed} failed",
        folders.len()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// macOS writes `._name` companions on foreign volumes; they are not
/// songs.
fn hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// One folder: `Ok(None)` when it has no copied level (or is no song
/// folder at all), `Ok(Some(line))` when it was — or, dry, would be —
/// re-levelled.
fn one(folder: &Path, dry_run: bool, created_ms: u64) -> Result<Option<String>, String> {
    let names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("cannot list: {error}"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if !names.iter().any(|n| n == versions::BASE_CHART) {
        return Ok(None);
    }
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active_path = folder.join(&active_name);
    let before = load_chart_file(&active_path)
        .map_err(|error| format!("cannot load `{active_name}`: {error}"))?;
    if bridge::copied_levels(&before).is_empty() {
        return Ok(None);
    }
    // A version the player edited by hand stays the active one.
    if versions::is_hand_edited(&before) {
        return Err(format!(
            "`{active_name}` has copied levels but was edited by hand — left alone"
        ));
    }
    let mut after = before.clone();
    let replaced = bridge::relevel(&mut after);
    // A song too sparse for the ladder to thin derives its copies
    // back into themselves; a version that changes nothing is noise.
    if replaced.is_empty() || after.charts == before.charts {
        return Ok(None);
    }
    let errors: Vec<String> = after
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect();
    if !errors.is_empty() {
        return Err(format!(
            "the re-levelled chart failed validation — this is a bug: {}",
            errors.join("; ")
        ));
    }
    after.provenance = Some(Provenance {
        parent_hash: chart_hash(&before),
        designer: DESIGNER.to_owned(),
        created_ms,
        directive: Some("copied_levels".to_owned()),
    });
    let summary = summary(&before, &after, &replaced);
    if dry_run {
        return Ok(Some(format!("{summary} — dry run, nothing written")));
    }
    let next_name = versions::next_version_name(&names);
    let next_path = folder.join(&next_name);
    save_chart_file(&next_path, &after)
        .map_err(|error| format!("cannot write `{next_name}`: {error}"))?;
    std::fs::write(
        folder.join(versions::POINTER_FILE),
        versions::pointer_json(&next_name),
    )
    .map_err(|error| format!("cannot write the pointer: {error}"))?;
    // The analysis sidecar travels with the version; losing it costs a
    // dimension in a later reading, never the write.
    if let Err(reason) = context::carry(&active_path, &before, &next_path, &after) {
        eprintln!("{}: sidecar not carried: {reason}", folder.display());
    }
    Ok(Some(format!(
        "{summary} — wrote `{next_name}` (parent `{active_name}`) and made it active"
    )))
}

/// `easy 3837 → 1210, medium 3837 → 2390` for the levels replaced.
fn summary(
    before: &ChartFile,
    after: &ChartFile,
    replaced: &[beatbyte_core::Difficulty],
) -> String {
    let count = |chart: &ChartFile, d| chart.chart_for(d).map_or(0, |c| c.notes.len());
    let parts: Vec<String> = replaced
        .iter()
        .map(|d| format!("{} {} → {}", d.id(), count(before, *d), count(after, *d)))
        .collect();
    format!("{} — {}", before.song.title, parts.join(", "))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// Whether the game is running right now.
fn game_is_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "beatbyte"])
        .output()
        .is_ok_and(|out| !out.stdout.is_empty())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use beatbyte_core::Difficulty;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bb-relevel-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One Expert track of eighths over all frets, pasted into every
    /// level below it.
    fn pasted() -> ChartFile {
        let lanes = (0..64)
            .map(|i| {
                format!(
                    r#"{{"time": {:.3}, "lane": {}}}"#,
                    1.0 + f64::from(i) * 0.25,
                    i % 5
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let charts = ["easy", "medium", "hard", "expert"]
            .iter()
            .map(|d| format!(r#"{{"difficulty": "{d}", "lanes": 5, "notes": [{lanes}]}}"#))
            .collect::<Vec<_>>()
            .join(",");
        ChartFile::from_json(&format!(
            r#"{{"format_version": 1, "song": {{"title": "T", "audio": "song.m4a", "bpm": 120.0}}, "charts": [{charts}]}}"#
        ))
        .unwrap()
    }

    /// The copies become a new active version, the parent stays, and a
    /// second run finds nothing left to do.
    #[test]
    fn copied_levels_become_a_new_version_once() {
        let dir = scratch("once");
        let original = pasted();
        save_chart_file(&dir.join(versions::BASE_CHART), &original).unwrap();

        let dry = one(&dir, true, 7).unwrap();
        assert!(dry.is_some_and(|line| line.contains("dry run")));
        assert!(
            !dir.join("chart.v2.json").exists(),
            "a dry run writes nothing"
        );

        let line = one(&dir, false, 7).unwrap().unwrap();
        assert!(line.contains("chart.v2.json"), "{line}");
        let pointer = std::fs::read_to_string(dir.join(versions::POINTER_FILE)).unwrap();
        assert!(pointer.contains("chart.v2.json"));
        let parent = load_chart_file(&dir.join(versions::BASE_CHART)).unwrap();
        assert_eq!(parent, original, "the parent is never rewritten");
        let fresh = load_chart_file(&dir.join("chart.v2.json")).unwrap();
        assert!(bridge::copied_levels(&fresh).is_empty());
        let count = |d| fresh.chart_for(d).map_or(0, |c| c.notes.len());
        assert!(count(Difficulty::Expert) > count(Difficulty::Hard));
        assert!(count(Difficulty::Hard) > count(Difficulty::Easy));
        let provenance = fresh.provenance.unwrap();
        assert_eq!(provenance.parent_hash, chart_hash(&original));
        assert_eq!(provenance.designer, DESIGNER);

        assert_eq!(
            one(&dir, false, 8).unwrap(),
            None,
            "the second run writes nothing"
        );
        assert!(!dir.join("chart.v3.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A song of one note per level derives its copies back into
    /// themselves: nothing is written, and the second run is as quiet
    /// as the first.
    #[test]
    fn a_copy_that_derives_into_itself_writes_nothing() {
        let dir = scratch("itself");
        let mut chart = pasted();
        for def in &mut chart.charts {
            def.notes.truncate(1);
        }
        save_chart_file(&dir.join(versions::BASE_CHART), &chart).unwrap();
        assert_eq!(one(&dir, false, 7).unwrap(), None);
        assert!(!dir.join(versions::POINTER_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder whose levels are all charted is not touched.
    #[test]
    fn a_charted_song_is_left_alone() {
        let dir = scratch("charted");
        let mut chart = pasted();
        for (def, keep) in chart.charts.iter_mut().zip([10, 20, 40]) {
            def.notes.truncate(keep);
        }
        save_chart_file(&dir.join(versions::BASE_CHART), &chart).unwrap();
        assert_eq!(one(&dir, false, 7).unwrap(), None);
        assert!(!dir.join(versions::POINTER_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
