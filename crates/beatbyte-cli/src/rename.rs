//! `beatbyte-cli rename-folders <library>` — give every song folder
//! the name this build would give it (`artist--title--bg01`, the
//! user's scheme of 2026-10-12; see
//! [`beatbyte_chart::twin::song_folder_base`]).
//!
//! A dry run by default: it prints what it would do. `--apply` renames
//! and appends every rename to a map file (`old<TAB>new`, one per
//! line, written as it goes), so a mirror elsewhere — the off-site
//! backup — can be renamed the same way instead of uploaded again.
//!
//! What it leaves alone: `[GS]`/`[CL]` twins (named after the folder
//! they were made from), folders it cannot read, and any rename whose
//! target name is already taken. Nothing inside a folder changes:
//! scores, telemetry and the active chart version do not depend on
//! the folder's name.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;

use beatbyte_chart::twin::{self, Kind};
use beatbyte_chart::versions;

/// What a folder says about itself.
#[derive(Debug, Clone)]
pub struct Folder {
    /// Its current name.
    pub name: String,
    /// The base chart's title, as written (prefix and all).
    pub title: String,
    /// The base chart's artist.
    pub artist: String,
}

/// What happens to one folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Rename `from` to `to`.
    Rename {
        /// Current name.
        from: String,
        /// New name.
        to: String,
    },
    /// The name is already right, or this kind of folder keeps its name.
    Keep(String),
    /// Left alone, and why.
    Skip {
        /// The folder.
        name: String,
        /// Why.
        why: String,
    },
}

/// The name this build gives `folder`, or `None` for a folder that
/// keeps its name (a study or classic twin). Pure — tested.
#[must_use]
pub fn target_name(folder: &Folder) -> Option<String> {
    if matches!(
        twin::kind_of_folder(&folder.name),
        Some(Kind::Study | Kind::Classic)
    ) {
        return None;
    }
    let from_name = Kind::NUMBERED
        .into_iter()
        .find_map(|kind| twin::numbered_number_of_folder(kind, &folder.name).map(|n| (kind, n)));
    let from_title = twin::split_numbered_title(&folder.title).map(|(kind, n, _)| (kind, n));
    let bare = twin::split_numbered_title(&folder.title).map_or(folder.title.as_str(), |t| t.2);
    let base = twin::song_folder_base(&folder.artist, bare);
    // The FOLDER decides what a twin is (twin.rs); the title only
    // fills in for a folder that does not say.
    Some(match from_name.or(from_title) {
        Some((kind, number)) => twin::numbered_folder_name(kind, &base, number),
        None => base,
    })
}

/// The steps for a whole library, in name order. A target that is
/// already a folder's name, or that an earlier folder takes, is a
/// skip — never an overwrite. Pure — tested.
#[must_use]
pub fn plan(folders: &[Folder]) -> Vec<Step> {
    let mut sorted: Vec<&Folder> = folders.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let existing: BTreeSet<&str> = sorted.iter().map(|f| f.name.as_str()).collect();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    sorted
        .into_iter()
        .map(|folder| match target_name(folder) {
            None => Step::Keep(folder.name.clone()),
            Some(to) if to == folder.name => {
                taken.insert(to);
                Step::Keep(folder.name.clone())
            }
            Some(to) => {
                // Two songs that spell to the same name: the second
                // gets `-2` before its version (the user's rule).
                let Some(to) = (1..=99).map(|n| with_counter(&to, n)).find(|c| {
                    c == &folder.name || !(existing.contains(c.as_str()) || taken.contains(c))
                }) else {
                    return Step::Skip {
                        name: folder.name.clone(),
                        why: format!("`{to}` and its numbered variants are taken"),
                    };
                };
                if to == folder.name {
                    taken.insert(to);
                    return Step::Keep(folder.name.clone());
                }
                taken.insert(to.clone());
                Step::Rename {
                    from: folder.name.clone(),
                    to,
                }
            }
        })
        .collect()
}

/// `name` with counter `n` in front of its version: `a--b--bg01` →
/// `a--b-2--bg01`, `a--b` → `a--b-2`; 1 is the name itself.
fn with_counter(name: &str, n: u32) -> String {
    if n == 1 {
        return name.to_owned();
    }
    let numbered = Kind::NUMBERED
        .into_iter()
        .any(|kind| twin::numbered_number_of_folder(kind, name).is_some());
    match name.rsplit_once("--") {
        Some((base, version)) if numbered => format!("{base}-{n}--{version}"),
        _ => format!("{name}-{n}"),
    }
}

/// `name` in composed form (NFC), when that differs from how it was
/// listed. ⚠️ macOS lists a FAT32 volume's names DECOMPOSED (`o` +
/// combining diaeresis) and then finds them only COMPOSED: renaming
/// by the listed name fails with "no such file" for every name with
/// an umlaut or accent — 211 of 8 571 folders on the real library
/// (2026-10-12). Pure — tested.
fn composed(name: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization as _;
    let nfc: String = name.nfc().collect();
    (nfc != name).then_some(nfc)
}

fn read_folder(dir: &Path) -> Result<Folder, String> {
    #[derive(serde::Deserialize)]
    struct Head {
        song: HeadSong,
    }
    #[derive(serde::Deserialize)]
    struct HeadSong {
        title: String,
        #[serde(default)]
        artist: String,
    }
    let text = std::fs::read_to_string(dir.join(versions::BASE_CHART))
        .map_err(|e| format!("no readable {}: {e}", versions::BASE_CHART))?;
    let head: Head = serde_json::from_str(&text).map_err(|e| format!("unreadable chart: {e}"))?;
    Ok(Folder {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        title: head.song.title,
        artist: head.song.artist,
    })
}

/// Run it: a dry run unless `apply`; `map` receives every rename.
pub fn run(library: &Path, apply: bool, map: Option<&Path>) -> ExitCode {
    let Ok(entries) = std::fs::read_dir(library) else {
        eprintln!("cannot list `{}`", library.display());
        return ExitCode::from(2);
    };
    let mut folders = Vec::new();
    let mut steps = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !path.is_dir() {
            continue;
        }
        match read_folder(&path) {
            Ok(folder) => folders.push(folder),
            Err(why) => steps.push(Step::Skip { name, why }),
        }
    }
    steps.extend(plan(&folders));
    let renames: Vec<(&str, &str)> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Rename { from, to } => Some((from.as_str(), to.as_str())),
            _ => None,
        })
        .collect();
    let kept = steps.iter().filter(|s| matches!(s, Step::Keep(_))).count();
    for step in &steps {
        if let Step::Skip { name, why } = step {
            println!("skip    {name}: {why}");
        }
    }
    for (from, to) in renames.iter().take(if apply { 0 } else { usize::MAX }) {
        println!("{from}\n     -> {to}");
    }
    let skipped = steps.len() - renames.len() - kept;
    println!(
        "{} to rename, {kept} keep their name, {skipped} skipped{}",
        renames.len(),
        if apply {
            ""
        } else {
            " (dry run: --apply renames)"
        }
    );
    if !apply {
        return ExitCode::SUCCESS;
    }
    let mut map_file = match map.map(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
    }) {
        Some(Err(e)) => {
            eprintln!("cannot open the map file: {e}");
            return ExitCode::from(2);
        }
        Some(Ok(f)) => Some(f),
        None => None,
    };
    let mut failed = 0usize;
    for (from, to) in &renames {
        let target = library.join(to);
        if target.exists() {
            eprintln!("skip    {from}: `{to}` appeared meanwhile");
            failed += 1;
            continue;
        }
        let renamed =
            std::fs::rename(library.join(from), &target).or_else(|e| match composed(from) {
                Some(nfc) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::rename(library.join(nfc), &target)
                }
                _ => Err(e),
            });
        if let Err(e) = renamed {
            eprintln!("failed  {from}: {e}");
            failed += 1;
            continue;
        }
        if let Some(f) = map_file.as_mut()
            && writeln!(f, "{from}\t{to}").is_err()
        {
            eprintln!("cannot write the map file — stopping after {from}");
            return ExitCode::from(1);
        }
    }
    println!("renamed {}, failed {failed}", renames.len() - failed);
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, artist: &str, title: &str) -> Folder {
        Folder {
            name: name.to_owned(),
            title: title.to_owned(),
            artist: artist.to_owned(),
        }
    }

    #[test]
    fn the_libraries_real_names_become_the_users_scheme() {
        let cases = [
            (
                folder(
                    "bridge-01-009-sound-system---dreamscape",
                    "009 Sound System",
                    "[BG-01] Dreamscape",
                ),
                "009-sound-system--dreamscape--bg01",
            ),
            (
                folder(
                    "bridge-01-nena---99-luftballons--1983---2--m4a",
                    "Nena",
                    "[BG-01] 99 Luftballons",
                ),
                "nena--99-luftballons--bg01",
            ),
            (
                folder(
                    "bridge-01-beyoncé---bodyguard",
                    "Beyoncé",
                    "[BG-01] Bodyguard",
                ),
                "beyonce--bodyguard--bg01",
            ),
            (
                folder(
                    "gamerip-01-alice-cooper--wavegroup----school-s-out",
                    "Alice Cooper",
                    "[GR-01] School's Out",
                ),
                "alice-cooper--schools-out--gr01",
            ),
            (
                folder(
                    "gamerip-03-ozzy---crazy-train",
                    "Ozzy Osbourne",
                    "[GR-03] Crazy Train",
                ),
                "ozzy-osbourne--crazy-train--gr03",
            ),
            (
                folder("never-gonna-m4a", "Rick Astley", "Never Gonna Give You Up"),
                "rick-astley--never-gonna-give-you-up",
            ),
        ];
        for (f, want) in cases {
            assert_eq!(target_name(&f).as_deref(), Some(want), "{}", f.name);
        }
    }

    /// The folder decides the kind and number; a title that disagrees
    /// (a renamed song) does not move a version into another series.
    #[test]
    fn the_folder_not_the_title_says_which_version() {
        let f = folder("gamerip-02-x", "A", "[BG-05] Song");
        assert_eq!(target_name(&f).as_deref(), Some("a--song--gr02"));
    }

    #[test]
    fn study_and_classic_twins_keep_their_names() {
        assert_eq!(target_name(&folder("guitar-study-x", "A", "[GS] B")), None);
        assert_eq!(target_name(&folder("classic-x", "A", "[CL] B")), None);
    }

    /// Two folders that spell to one name: the second gets `-2`
    /// before its version — never an overwrite, never a skip.
    #[test]
    fn a_taken_name_gets_a_counter_never_an_overwrite() {
        let steps = plan(&[
            folder("bridge-01-a---b", "A", "[BG-01] B"),
            folder("a--b--bg01", "A", "[BG-01] B"),
            folder(
                "gamerip-01-x",
                "Elton John",
                "[GR-01] Saturday Night's Alright (For Fighting)",
            ),
            folder(
                "gamerip-01-y",
                "Elton John",
                "[GR-01] Saturday Night's Alright for Fighting",
            ),
            folder("own", "C", "D"),
            folder("own2", "C", "D"),
        ]);
        let rename = |from: &str, to: &str| Step::Rename {
            from: from.into(),
            to: to.into(),
        };
        assert!(steps.contains(&Step::Keep("a--b--bg01".into())));
        assert!(steps.contains(&rename("bridge-01-a---b", "a--b-2--bg01")));
        assert!(steps.contains(&rename(
            "gamerip-01-x",
            "elton-john--saturday-nights-alright-for-fighting--gr01"
        )));
        assert!(steps.contains(&rename(
            "gamerip-01-y",
            "elton-john--saturday-nights-alright-for-fighting-2--gr01"
        )));
        assert!(steps.contains(&rename("own", "c--d")));
        assert!(steps.contains(&rename("own2", "c--d-2")));
        assert!(!steps.iter().any(|s| matches!(s, Step::Skip { .. })));
    }

    #[test]
    fn a_decomposed_name_is_retried_composed() {
        let listed = "bridge-01-mo\u{0308}rhead---ace-of-spades";
        assert_eq!(
            composed(listed).as_deref(),
            Some("bridge-01-m\u{00f6}rhead---ace-of-spades")
        );
        assert_eq!(composed("bridge-01-a---b"), None);
        assert_eq!(composed("bridge-01-m\u{00f6}rhead"), None);
    }

    /// A second run finds nothing to do.
    #[test]
    fn renaming_twice_is_a_no_op() {
        let first = plan(&[folder("bridge-01-a---b", "A", "[BG-01] B")]);
        let Step::Rename { to, .. } = &first[0] else {
            panic!("{first:?}")
        };
        let second = plan(&[folder(to, "A", "[BG-01] B")]);
        assert_eq!(second, vec![Step::Keep(to.clone())]);
    }
}
