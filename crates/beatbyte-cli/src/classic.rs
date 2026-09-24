//! `classic`: apply classic ingredients to a song folder.
//!
//! Which ingredients is the `--with` list ([`classic::Recipe::parse`]);
//! without it, the recipe that has been through a blind test.
//!
//! The ingredients are written as a NEW version beside the active one,
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
use beatbyte_chart::{
    ChartFile, chart_hash, classic, load_chart_file, save_chart_file, twin, versions,
};
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

/// What a run that would write is told while the game is up.
const GAME_RUNNING: &str = "BeatByte is running — a chart swapped under a live browser breaks every \
     Enter on that song until a rescan. Quit the game first.";

/// Apply the recipe to one song folder.
pub fn run(folder: &Path, dry_run: bool, recipe: classic::Recipe) -> ExitCode {
    match one(folder, dry_run, recipe) {
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
pub fn run_all(dir: &Path, dry_run: bool, recipe: classic::Recipe) -> ExitCode {
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
        match one(&folder, dry_run, recipe) {
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

/// Write the `[CL]` twin of one song folder.
pub fn run_twin(folder: &Path, dry_run: bool, recipe: classic::Recipe) -> ExitCode {
    match one_twin(folder, dry_run, recipe) {
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

/// Write the `[CL]` twin of every song folder under `dir`.
///
/// ⚠️ A twin folder is itself a song folder, and a `[CL]` twin of a
/// `[CL]` twin would be a copy — the recipe changes nothing the
/// second time, so `write_twin` refuses it anyway, but walking them
/// is wasted work and a confusing report. Study twins are NOT
/// skipped: the classic rules applied to a study chart is the
/// combination this library is mostly played on.
pub fn run_twin_all(dir: &Path, dry_run: bool, recipe: classic::Recipe) -> ExitCode {
    let mut folders: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_none_or(|n| twin::kind_of_folder(n) != Some(classic::KIND))
            })
            .collect(),
        Err(error) => {
            eprintln!("cannot list `{}`: {error}", dir.display());
            return ExitCode::from(2);
        }
    };
    folders.sort();
    let (mut yes, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for folder in folders {
        match one_twin(&folder, dry_run, recipe) {
            Ok(report) => {
                println!("{report}");
                // ⚠️ Counted on the OUTCOME, not on a word in the
                // report: the first version looked for "wrote", which
                // a dry run never says, so a complete dry run
                // summed itself up as "0 twin(s) written".
                if report.contains("no twin") || report.contains("already there") {
                    skipped += 1;
                } else {
                    yes += 1;
                }
            }
            Err(reason) => {
                eprintln!("{}: {reason}", folder.display());
                failed += 1;
            }
        }
    }
    let verb = if dry_run {
        "would be written"
    } else {
        "written"
    };
    println!("{yes} twin(s) {verb}, {skipped} skipped, {failed} failed");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// One folder's twin, reported the way the version path reports.
fn one_twin(folder: &Path, dry_run: bool, recipe: classic::Recipe) -> Result<String, String> {
    if !dry_run && game_is_running() {
        return Err(GAME_RUNNING.to_owned());
    }
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    if dry_run {
        let Some((before, after, window, rules)) = preview(folder, recipe)? else {
            return Ok(format!("{name}: no twin — legacy layout"));
        };
        let changed = changed_in_window(&before, &after, WINDOW_DIFFICULTY, window);
        let per_count: Vec<(String, usize)> = after
            .charts
            .iter()
            .map(|def| {
                (
                    def.difficulty.id().to_owned(),
                    changed_in_window(&before, &after, def.difficulty, (0.0, f64::MAX)),
                )
            })
            .collect();
        // ⚠️ The dry run has to answer what the real run would, and
        // the real run REFUSES a chart the recipe does not change —
        // a twin identical to its source is a second browser entry
        // playing the same notes. Without this the four folders that
        // already carry the ingredient were reported as "would
        // write … 0 in the window".
        if !rules && per_count.iter().all(|(_, n)| *n == 0) {
            return Ok(format!(
                "{name}: no twin — the chart already plays by these rules"
            ));
        }
        let per: Vec<String> = per_count
            .iter()
            .map(|(id, n)| format!("{id} {n}"))
            .collect();
        return Ok(format!(
            "{name}: would write `{}` [{}] — {} in the window, {}{}",
            twin::folder_for(folder, classic::KIND)
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "?".to_owned()),
            recipe.directive(),
            changed,
            per.join(", "),
            rule_note(rules),
        ));
    }
    match classic::write_twin(folder, recipe)? {
        classic::Outcome::Written {
            folder: out,
            title,
            changed,
            rules,
            sidecar,
        } => {
            let per: Vec<String> = changed.iter().map(|(id, n)| format!("{id} {n}")).collect();
            let note = if sidecar {
                " with its analysis sidecar"
            } else {
                " (no current sidecar to carry)"
            };
            Ok(format!(
                "{name}: wrote `{}` as \"{title}\" [{}]{note} — notes changed {}{}",
                out.file_name().map_or_else(
                    || out.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                ),
                recipe.directive(),
                per.join(", "),
                rule_note(rules),
            ))
        }
        classic::Outcome::AlreadyThere(out) => Ok(format!(
            "{name}: `{}` is already there",
            out.file_name().map_or_else(
                || out.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        )),
        classic::Outcome::Refused(reason) => Ok(format!("{name}: no twin — {reason}")),
    }
}

/// The active chart, the same chart with the recipe applied, the
/// window the blind test plays, and whether the judgment rules
/// changed. `None` for a folder with no chart to start from.
type Preview = Option<(ChartFile, ChartFile, (f64, f64), bool)>;

/// What a report says about a rule the recipe added — a rule changes
/// no note, so the counts alone would call such a run a no-op.
fn rule_note(rules: bool) -> String {
    if rules {
        format!("; rule: strum grace {} ms", classic::STRUM_GRACE_MS)
    } else {
        String::new()
    }
}

/// The active chart, the same chart with the recipe applied, and the
/// window the blind test plays — everything a dry run reports from,
/// and nothing written.
fn preview(folder: &Path, recipe: classic::Recipe) -> Result<Preview, String> {
    let names = twin::names_in(folder)?;
    if !names.iter().any(|n| n == versions::BASE_CHART) {
        return Ok(None);
    }
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let before = load_chart_file(&folder.join(&active_name))
        .map_err(|error| format!("cannot load {active_name}: {error}"))?;
    let mut after = before.clone();
    let changes = classic::apply(&mut after, recipe);
    let window = before.preview_window(WINDOW_DIFFICULTY, WINDOW_S);
    Ok(Some((before, after, window, changes.rules)))
}

/// How many notes inside `window` differ: removed, added, or on the
/// same moment and fret with another flag or length.
///
/// Pure — tested. ⚠️ It compares NOTES by moment and fret, not by
/// position in the list: an ingredient that removes notes shifts
/// every index after the first removal, and a positional comparison
/// would call the whole rest of the song changed.
#[must_use]
pub fn changed_in_window(
    before: &ChartFile,
    after: &ChartFile,
    difficulty: Difficulty,
    window: (f64, f64),
) -> usize {
    use std::collections::BTreeMap;
    // Keyed to the microsecond: a float round-trip moves a time by
    // far less, and two notes a microsecond apart are one chord to
    // the engine anyway.
    let notes = |chart: &ChartFile| -> BTreeMap<(i64, u8), (bool, i64)> {
        chart
            .charts
            .iter()
            .find(|c| c.difficulty == difficulty)
            .map(|c| {
                c.notes
                    .iter()
                    .filter(|n| n.time >= window.0 && n.time < window.1)
                    .map(|n| {
                        (
                            ((n.time * 1e6).round() as i64, n.lane),
                            (n.hopo, (n.len * 1e6).round() as i64),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (old, new) = (notes(before), notes(after));
    let changed_or_removed = old.iter().filter(|(k, v)| new.get(k) != Some(v)).count();
    let added = new.keys().filter(|k| !old.contains_key(k)).count();
    changed_or_removed + added
}

fn one(folder: &Path, dry_run: bool, recipe: classic::Recipe) -> Result<String, String> {
    if !dry_run && game_is_running() {
        return Err(GAME_RUNNING.to_owned());
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

    if recipe.names().is_empty() {
        return Err("no ingredients: a new version would be a copy".to_owned());
    }
    let mut after = before.clone();
    let changes = classic::apply(&mut after, recipe);
    let changed = &changes.notes;
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
        directive: Some(recipe.directive()),
    });
    let window = before.preview_window(WINDOW_DIFFICULTY, WINDOW_S);
    let in_window = changed_in_window(&before, &after, WINDOW_DIFFICULTY, window);

    let title = &before.song.title;
    let per_difficulty: Vec<String> = changed
        .iter()
        .map(|(difficulty, n)| format!("{} {n}", difficulty.id()))
        .collect();
    let total = changes.total_notes();
    let head = format!(
        "{title} [{active_name}] {} BPM, {} — {} note(s) changed: {}; {in_window} in the test \
         window {:.1}–{:.1}s ({}){}",
        before.song.bpm,
        recipe.directive(),
        total,
        per_difficulty.join(", "),
        window.0,
        window.1,
        WINDOW_DIFFICULTY.id(),
        rule_note(changes.rules),
    );
    if dry_run {
        return Ok(format!("{head} — dry run, nothing written"));
    }
    if changes.is_empty() {
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
    let carried = match context::carry(&active_path, &before, &folder.join(&next_name), &after) {
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

    /// ⚠️ An ingredient that REMOVES notes shifts every index after
    /// the first removal. A positional comparison would call the whole
    /// rest of the song changed; this one counts the removed note and
    /// nothing else.
    #[test]
    fn a_removed_note_is_one_change_not_the_rest_of_the_song() {
        let before = chart(&[(1.0, false), (2.0, false), (3.0, false), (4.0, false)]);
        let mut after = before.clone();
        after.charts[0].notes.remove(1);
        assert_eq!(
            changed_in_window(&before, &after, Difficulty::Hard, (0.0, 30.0)),
            1
        );
        // And an added one is one as well.
        assert_eq!(
            changed_in_window(&after, &before, Difficulty::Hard, (0.0, 30.0)),
            1
        );
        // A note moved to another fret is its removal and its arrival.
        let mut moved = before.clone();
        moved.charts[0].notes[2].lane = 4;
        assert_eq!(
            changed_in_window(&before, &moved, Difficulty::Hard, (0.0, 30.0)),
            2
        );
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
