//! Moving the song library to another folder — an external drive, say
//! — without losing a byte.
//!
//! The rules, in the order they protect the player:
//!
//! 1. **Copy, never move.** The source is not touched until the player
//!    confirms a deletion after a verified copy ([`delete_source`]).
//! 2. **Every file is verified.** Each copy is hashed (SHA-256) as it
//!    is read and again from the target after it is written; a
//!    mismatch fails the run. A final pass re-hashes every target file
//!    against the record before the move counts as done.
//! 3. **Passes until nothing changes.** The game keeps running while
//!    the library copies, and playing a song writes into its folder.
//!    A file that changed (size or modification time) since it was
//!    copied is copied again; the copy ends with a pass that copied
//!    nothing.
//! 4. **Resumable.** The record ([`MANIFEST_FILE`], in the target)
//!    names the source and every file copied so far, so a run cut off
//!    by an unplugged cable continues where it stopped. A target that
//!    holds anything else is refused: this never writes into a
//!    stranger's folder.
//! 5. **The target is checked first**: not inside the source (nor the
//!    other way round), writable and readable back, and with room for
//!    the library plus a margin.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The record a move keeps in its target. No `.json` extension on
/// purpose: the library scan reads loose JSON files as charts.
pub const MANIFEST_FILE: &str = ".beatbyte-move";

/// Files the platform drops into folders, never part of a song.
const IGNORED: [&str; 2] = [".DS_Store", MANIFEST_FILE];

/// Most passes before a library that keeps changing is given up on.
pub const MAX_PASSES: usize = 6;

/// Room left free on the target beyond the library itself.
pub const MARGIN_BYTES: u64 = 512 * 1024 * 1024;

/// One file as the record knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Copied {
    /// SHA-256 of its content, hex.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
    /// The source's modification time when it was copied, ns since
    /// the epoch (0 when the platform does not say).
    pub modified_ns: u128,
}

/// The record of a move.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// The library being copied.
    pub source: PathBuf,
    /// Every file copied, by path relative to the library.
    pub files: BTreeMap<String, Copied>,
    /// Set once every file was verified and the source stood still.
    #[serde(default)]
    pub complete: bool,
}

/// How far a copy is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    /// Which pass (1-based).
    pub pass: usize,
    /// Bytes handled in this pass so far.
    pub bytes_done: u64,
    /// Bytes this pass has to look at.
    pub bytes_total: u64,
    /// Files copied in this pass so far.
    pub copied: usize,
}

/// What a finished copy did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moved {
    /// Files in the library.
    pub files: usize,
    /// Their size.
    pub bytes: u64,
    /// Passes it took.
    pub passes: usize,
}

/// One file of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the library, `/`-separated.
    pub rel: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, ns since the epoch (0 if unknown).
    pub modified_ns: u128,
}

/// Every file in the library (sorted), skipping the platform's own.
///
/// # Errors
/// When the library cannot be listed.
pub fn inventory(root: &Path) -> Result<Vec<Entry>, String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<Entry>) -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if IGNORED.contains(&name.as_str()) || name.ends_with(PARTIAL) {
                continue;
            }
            let meta = std::fs::metadata(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            if meta.is_dir() {
                walk(root, &path, out)?;
            } else if meta.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| format!("{} escaped the library", path.display()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push(Entry {
                    rel,
                    size: meta.len(),
                    modified_ns: modified_ns(&meta),
                });
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn modified_ns(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

/// The suffix a file carries while it is being written.
const PARTIAL: &str = ".beatbyte-partial";

/// Free bytes on the volume holding `path` (or its nearest existing
/// ancestor), from `df`; `None` where that cannot be asked.
#[must_use]
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    let output = std::process::Command::new("df")
        .arg("-Pk")
        .arg(existing)
        .output()
        .ok()?;
    parse_df(&String::from_utf8_lossy(&output.stdout))
}

/// The available kilobytes of `df -Pk`'s data line, in bytes. Pure —
/// tested.
#[must_use]
pub fn parse_df(text: &str) -> Option<u64> {
    let line = text.lines().nth(1)?;
    let available: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    available.checked_mul(1024)
}

/// Why `to` cannot take the library at `from`, if it cannot.
/// `free` is the target volume's free space when known. Pure over the
/// paths it is given and the facts passed in — tested.
///
/// # Errors
/// The reason, in words a player can act on.
pub fn check_target(from: &Path, to: &Path, needed: u64, free: Option<u64>) -> Result<(), String> {
    let (from_c, to_c) = (resolve(from), resolve(to));
    if from_c == to_c {
        return Err("that is where the library already is".to_owned());
    }
    if to_c.starts_with(&from_c) {
        return Err("the new place is inside the library itself".to_owned());
    }
    if from_c.starts_with(&to_c) {
        return Err("the library is inside the new place; choose a folder of its own".to_owned());
    }
    if let Some(free) = free {
        let wanted = needed.saturating_add(MARGIN_BYTES);
        if free < wanted {
            return Err(format!(
                "not enough space there: {} needed, {} free",
                human(wanted),
                human(free)
            ));
        }
    }
    Ok(())
}

/// `path` with its nearest existing ancestor resolved (symlinks, and on
/// macOS `/var` → `/private/var`) and the rest appended. A folder that
/// does not exist yet must compare like one that does: resolving only
/// existing paths let a target inside the library pass as outside it.
#[must_use]
pub fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    for ancestor in path.ancestors() {
        if let Ok(real) = std::fs::canonicalize(ancestor) {
            return rest
                .iter()
                .rev()
                .fold(real, |p: PathBuf, part| p.join(part));
        }
        if let Some(name) = ancestor.file_name() {
            rest.push(name.to_os_string());
        }
    }
    path.to_path_buf()
}

/// Write, read back and remove a probe file in `to`.
///
/// # Errors
/// When the folder cannot be created, written or read back.
pub fn probe_writable(to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let probe = to.join(".beatbyte-probe");
    let content = b"beatbyte write probe";
    std::fs::write(&probe, content)
        .map_err(|e| format!("cannot write to {}: {e}", to.display()))?;
    let back = std::fs::read(&probe).map_err(|e| format!("cannot read from {}: {e}", to.display()));
    let _ = std::fs::remove_file(&probe);
    if back? == content {
        Ok(())
    } else {
        Err(format!(
            "{} gave back something else than was written",
            to.display()
        ))
    }
}

/// A size in words. Pure — tested.
#[must_use]
pub fn human(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

/// The record in `to`, if there is one and every path in it is a plain
/// relative path. ⚠️ The record lives on the TARGET — an external drive
/// anyone can write to — so it is untrusted input: one `..` or absolute
/// path in it and it is no record at all, because its paths decide what
/// is deleted.
fn read_manifest(to: &Path) -> Option<Manifest> {
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(to.join(MANIFEST_FILE)).ok()?).ok()?;
    manifest
        .files
        .keys()
        .all(|rel| is_plain_rel(rel))
        .then_some(manifest)
}

/// Whether `rel` names a file strictly inside a folder: `/`-separated
/// names, none empty, `.` or `..`, no drive or root. Pure — tested.
#[must_use]
pub fn is_plain_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains(':')
        && rel
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn write_manifest(to: &Path, manifest: &Manifest) -> Result<(), String> {
    let text = serde_json::to_string(manifest).map_err(|e| e.to_string())?;
    beatbyte_chart::io::write_atomic(&to.join(MANIFEST_FILE), text.as_bytes())
        .map_err(|e| format!("cannot write the move record: {e}"))
}

/// Whether `to` may receive this library: empty (but for the
/// platform's own files), or holding a record of a move FROM this
/// library. Anything else is somebody's folder.
fn may_receive(from: &Path, to: &Path) -> Result<Manifest, String> {
    if let Some(manifest) = read_manifest(to) {
        return if resolve(&manifest.source) == resolve(from) {
            Ok(manifest)
        } else {
            Err(format!(
                "{} holds a move of another library ({})",
                to.display(),
                manifest.source.display()
            ))
        };
    }
    let foreign = std::fs::read_dir(to)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|e| !IGNORED.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .unwrap_or(false);
    if foreign {
        Err(format!(
            "{} is not empty; choose an empty folder for the library",
            to.display()
        ))
    } else {
        Ok(Manifest {
            source: from.to_path_buf(),
            ..Manifest::default()
        })
    }
}

/// Copy `src` to `dst`, hashing what is read; then hash `dst` back.
/// Returns the hash when both agree.
fn copy_verified(src: &Path, dst: &Path) -> Result<String, String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let partial = PathBuf::from(format!("{}{PARTIAL}", dst.display()));
    let mut reader =
        std::fs::File::open(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let mut writer = std::fs::File::create(&partial)
        .map_err(|e| format!("cannot write {}: {e}", partial.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", src.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        writer
            .write_all(&buffer[..n])
            .map_err(|e| format!("cannot write {}: {e}", partial.display()))?;
    }
    writer
        .sync_all()
        .map_err(|e| format!("cannot write {}: {e}", partial.display()))?;
    drop(writer);
    let read = hex(&hasher.finalize());
    let written = hash_file(&partial)?;
    if read != written {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "{} did not arrive intact (checksum differs) — the target may be failing",
            dst.display()
        ));
    }
    std::fs::rename(&partial, dst).map_err(|e| format!("cannot finish {}: {e}", dst.display()))?;
    Ok(read)
}

/// SHA-256 of a file, hex.
///
/// # Errors
/// When the file cannot be read.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn local(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// Copy the library at `from` into `to`, verified, in passes until the
/// source stands still. `progress` hears about every file.
///
/// # Errors
/// When the target is refused, a file cannot be copied or does not
/// arrive intact, or the library will not stand still.
pub fn copy_library(
    from: &Path,
    to: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Moved, String> {
    if !from.is_dir() {
        return Err(format!("the library {} is not there", from.display()));
    }
    let first = inventory(from)?;
    let needed: u64 = first.iter().map(|e| e.size).sum();
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let mut manifest = may_receive(from, to)?;
    let already: u64 = manifest.files.values().map(|c| c.size).sum();
    check_target(from, to, needed.saturating_sub(already), free_space(to))?;
    probe_writable(to)?;
    manifest.complete = false;
    write_manifest(to, &manifest)?;

    for pass in 1..=MAX_PASSES {
        let files = inventory(from)?;
        let bytes_total: u64 = files.iter().map(|e| e.size).sum();
        let mut state = Progress {
            pass,
            bytes_total,
            ..Progress::default()
        };
        // A file gone from the source since an earlier pass goes from
        // the target too — but only one this move put there.
        let present: std::collections::BTreeSet<&str> =
            files.iter().map(|e| e.rel.as_str()).collect();
        let vanished: Vec<String> = manifest
            .files
            .keys()
            .filter(|rel| !present.contains(rel.as_str()))
            .cloned()
            .collect();
        for rel in &vanished {
            let _ = std::fs::remove_file(local(to, rel));
            manifest.files.remove(rel);
        }
        let mut copied_this_pass = !vanished.is_empty();
        for (index, entry) in files.iter().enumerate() {
            let unchanged = manifest.files.get(&entry.rel).is_some_and(|c| {
                c.size == entry.size
                    && c.modified_ns == entry.modified_ns
                    && std::fs::metadata(local(to, &entry.rel)).is_ok_and(|m| m.len() == c.size)
            });
            if !unchanged {
                let sha256 = copy_verified(&local(from, &entry.rel), &local(to, &entry.rel))?;
                manifest.files.insert(
                    entry.rel.clone(),
                    Copied {
                        sha256,
                        size: entry.size,
                        modified_ns: entry.modified_ns,
                    },
                );
                state.copied += 1;
                copied_this_pass = true;
                // The record keeps up, so a cut cable loses at most
                // the last few files' worth of work.
                if state.copied.is_multiple_of(25) || index + 1 == files.len() {
                    write_manifest(to, &manifest)?;
                }
            }
            state.bytes_done += entry.size;
            progress(state);
        }
        write_manifest(to, &manifest)?;
        if !copied_this_pass {
            verify_all(to, &manifest)?;
            manifest.complete = true;
            write_manifest(to, &manifest)?;
            return Ok(Moved {
                files: files.len(),
                bytes: bytes_total,
                passes: pass,
            });
        }
    }
    Err(format!(
        "the library kept changing for {MAX_PASSES} passes; try again when nothing is importing"
    ))
}

/// Re-hash every target file against the record.
fn verify_all(to: &Path, manifest: &Manifest) -> Result<(), String> {
    for (rel, copied) in &manifest.files {
        let path = local(to, rel);
        if hash_file(&path)? != copied.sha256 {
            return Err(format!(
                "{} changed on the target after it was copied — the target may be failing",
                path.display()
            ));
        }
    }
    Ok(())
}

/// What a deletion did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deleted {
    /// Files removed from the old place.
    pub removed: usize,
    /// Files left there because they were not exactly what was copied.
    pub kept: usize,
}

/// Remove the old copy after a complete move: only files whose size
/// and modification time are still what the record says were copied
/// AND whose copy is still in the target; then the folders left
/// empty. Anything else stays and is counted. The record is removed
/// last, when nothing was kept.
///
/// `from` is the old library as the CALLER knows it — never taken from
/// the record, which lives on the target and could name any folder.
/// The record must name exactly `from`.
///
/// # Errors
/// When there is no complete, valid move from `from` recorded in `to`.
pub fn delete_source(from: &Path, to: &Path) -> Result<Deleted, String> {
    let manifest = read_manifest(to).ok_or("no finished move is recorded there")?;
    if !manifest.complete {
        return Err("the move did not finish; nothing is deleted".to_owned());
    }
    if resolve(&manifest.source) != resolve(from) {
        return Err("the record there is of another library; nothing is deleted".to_owned());
    }
    let (from_r, to_r) = (resolve(from), resolve(to));
    if from_r == to_r || from_r.starts_with(&to_r) || to_r.starts_with(&from_r) {
        return Err("the two places overlap; nothing is deleted".to_owned());
    }
    let mut deleted = Deleted {
        removed: 0,
        kept: 0,
    };
    for (rel, copied) in &manifest.files {
        let source = local(from, rel);
        let same = std::fs::metadata(&source)
            .is_ok_and(|m| m.len() == copied.size && modified_ns(&m) == copied.modified_ns);
        // Both read back against the record: a file is deleted only
        // when a true copy of it provably exists. Size alone would let
        // a forged record (it lives on the target) claim copies that
        // are not there.
        let verified = same
            && hash_file(&local(to, rel)).is_ok_and(|h| h == copied.sha256)
            && hash_file(&source).is_ok_and(|h| h == copied.sha256);
        if verified && std::fs::remove_file(&source).is_ok() {
            deleted.removed += 1;
        }
    }
    // What is left — changed since the copy, or never copied — stays
    // where it is, and is counted.
    deleted.kept = inventory(from).map_or(0, |left| left.len());
    remove_empty_dirs(from);
    if deleted.kept == 0 {
        let _ = std::fs::remove_file(to.join(MANIFEST_FILE));
    }
    Ok(deleted)
}

/// Remove every directory under (and including) `dir` that holds
/// nothing but the platform's own files.
fn remove_empty_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.path().is_dir() {
            remove_empty_dirs(&entry.path());
        }
    }
    let only_noise = std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .all(|e| e.file_name() == ".DS_Store")
    });
    if only_noise {
        let _ = std::fs::remove_file(dir.join(".DS_Store"));
        let _ = std::fs::remove_dir(dir);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "beatbyte-relocate-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn library(root: &Path) {
        for (rel, content) in [
            ("song-a/chart.json", "{\"a\":1}"),
            ("song-a/song.m4a", "audio a"),
            ("song-b/chart.json", "{\"b\":2}"),
            ("song-b/sub/notes.txt", "deep"),
        ] {
            let path = local(root, rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        std::fs::write(root.join(".DS_Store"), "noise").unwrap();
    }

    #[test]
    fn a_library_arrives_whole_and_the_old_copy_is_untouched() {
        let dir = Scratch::new("whole");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        let mut reports = Vec::new();
        let moved = copy_library(&from, &to, &mut |p| reports.push(p)).unwrap();
        assert_eq!(moved.files, 4);
        assert_eq!(moved.passes, 2, "one pass copies, the next confirms");
        for entry in inventory(&from).unwrap() {
            assert_eq!(
                hash_file(&local(&from, &entry.rel)).unwrap(),
                hash_file(&local(&to, &entry.rel)).unwrap(),
                "{}",
                entry.rel
            );
        }
        assert!(from.join("song-a/song.m4a").is_file(), "copy, never move");
        assert!(read_manifest(&to).unwrap().complete);
        assert_eq!(reports.last().unwrap().bytes_done, moved.bytes);
    }

    #[test]
    fn a_file_that_changes_during_the_copy_is_copied_again() {
        let dir = Scratch::new("changes");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        let mut touched = false;
        copy_library(&from, &to, &mut |p| {
            // While the first pass runs, a song is played and its
            // folder gains a document and a changed chart.
            if p.pass == 1 && !touched {
                touched = true;
                std::fs::write(from.join("song-a/song.json"), "played").unwrap();
                std::fs::write(from.join("song-b/chart.json"), "{\"b\":3, \"longer\":true}")
                    .unwrap();
            }
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("song-a/song.json")).unwrap(),
            "played"
        );
        assert_eq!(
            std::fs::read_to_string(to.join("song-b/chart.json")).unwrap(),
            "{\"b\":3, \"longer\":true}"
        );
    }

    #[test]
    fn a_cut_off_copy_resumes_and_a_strangers_folder_is_refused() {
        let dir = Scratch::new("resume");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        // A move that was cut off: half the files and an open record.
        std::fs::create_dir_all(to.join("song-a")).unwrap();
        let sha = copy_verified(
            &from.join("song-a/chart.json"),
            &to.join("song-a/chart.json"),
        )
        .unwrap();
        let meta = std::fs::metadata(from.join("song-a/chart.json")).unwrap();
        let mut manifest = Manifest {
            source: from.clone(),
            ..Manifest::default()
        };
        manifest.files.insert(
            "song-a/chart.json".into(),
            Copied {
                sha256: sha,
                size: meta.len(),
                modified_ns: modified_ns(&meta),
            },
        );
        write_manifest(&to, &manifest).unwrap();
        let mut copied = 0;
        copy_library(&from, &to, &mut |p| copied = copied.max(p.copied)).unwrap();
        assert_eq!(copied, 3, "only what was not there yet");

        let stranger = dir.0.join("photos");
        std::fs::create_dir_all(&stranger).unwrap();
        std::fs::write(stranger.join("holiday.jpg"), "jpg").unwrap();
        let error = copy_library(&from, &stranger, &mut |_| {}).unwrap_err();
        assert!(error.contains("not empty"), "{error}");
        assert_eq!(
            std::fs::read_dir(&stranger).unwrap().count(),
            1,
            "nothing written"
        );
    }

    #[test]
    fn targets_inside_out_or_too_small_are_refused() {
        let dir = Scratch::new("checks");
        let from = dir.0.join("lib");
        std::fs::create_dir_all(&from).unwrap();
        assert!(
            check_target(&from, &from, 1, None)
                .unwrap_err()
                .contains("already")
        );
        assert!(
            check_target(&from, &from.join("x"), 1, None)
                .unwrap_err()
                .contains("inside the library")
        );
        assert!(
            check_target(&from.join("x"), &from, 1, None)
                .unwrap_err()
                .contains("inside the new place")
        );
        let far = dir.0.join("far");
        assert!(
            check_target(&from, &far, 1_000, Some(10))
                .unwrap_err()
                .contains("not enough space")
        );
        assert!(check_target(&from, &far, 1_000, Some(10 * 1024 * 1024 * 1024)).is_ok());
        assert!(
            check_target(&from, &far, 1_000, None).is_ok(),
            "unknown space is not a refusal"
        );
    }

    #[test]
    fn deleting_takes_only_what_was_copied_unchanged() {
        let dir = Scratch::new("delete");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        assert!(
            delete_source(&from, &to).is_err(),
            "no move, nothing deleted"
        );
        copy_library(&from, &to, &mut |_| {}).unwrap();
        // After the copy, one file changes and a new one appears.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(from.join("song-b/chart.json"), "{\"edited\":true}").unwrap();
        std::fs::write(from.join("song-c.json"), "new").unwrap();
        let deleted = delete_source(&from, &to).unwrap();
        assert_eq!(deleted.removed, 3);
        assert_eq!(deleted.kept, 2);
        assert!(from.join("song-b/chart.json").is_file());
        assert!(from.join("song-c.json").is_file());
        assert!(!from.join("song-a").exists(), "an emptied folder goes");
        assert!(
            to.join(MANIFEST_FILE).is_file(),
            "kept files keep the record"
        );
        // Every copied file is still in the new place.
        assert!(to.join("song-a/song.m4a").is_file());
    }

    #[test]
    fn an_incomplete_move_deletes_nothing() {
        let dir = Scratch::new("incomplete");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        std::fs::create_dir_all(&to).unwrap();
        write_manifest(
            &to,
            &Manifest {
                source: from.clone(),
                ..Manifest::default()
            },
        )
        .unwrap();
        assert!(
            delete_source(&from, &to)
                .unwrap_err()
                .contains("did not finish")
        );
        assert_eq!(inventory(&from).unwrap().len(), 4);
    }

    #[test]
    fn only_plain_relative_paths_are_paths() {
        assert!(is_plain_rel("song-a/chart.json"));
        for bad in [
            "",
            "/etc/hosts",
            "../x",
            "a/../../x",
            "a//b",
            "./a",
            "C:/x",
            "a\\..\\b",
        ] {
            assert!(!is_plain_rel(bad), "{bad}");
        }
    }

    /// ⚠️ The record lives on the target drive and is untrusted: a
    /// forged one must neither steer the deletion into another folder
    /// nor reach outside the library with `..`.
    #[test]
    fn a_forged_record_deletes_nothing() {
        let dir = Scratch::new("forged");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        let victim = dir.0.join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("precious.txt"), "keep me").unwrap();
        let meta = std::fs::metadata(victim.join("precious.txt")).unwrap();
        let copied = Copied {
            sha256: String::new(),
            size: meta.len(),
            modified_ns: modified_ns(&meta),
        };
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("precious.txt"), "keep me").unwrap();
        // A record naming another folder as its source.
        let mut forged = Manifest {
            source: victim.clone(),
            complete: true,
            ..Manifest::default()
        };
        forged.files.insert("precious.txt".into(), copied.clone());
        write_manifest(&to, &forged).unwrap();
        assert!(
            delete_source(&from, &to)
                .unwrap_err()
                .contains("another library")
        );
        // A record of this library reaching out with `..`.
        let mut escaping = Manifest {
            source: from.clone(),
            complete: true,
            ..Manifest::default()
        };
        escaping
            .files
            .insert("../victim/precious.txt".into(), copied);
        write_manifest(&to, &escaping).unwrap();
        assert!(delete_source(&from, &to).is_err());
        assert!(
            victim.join("precious.txt").is_file(),
            "nothing outside was touched"
        );
        assert_eq!(inventory(&from).unwrap().len(), 4, "nor inside");
        // And a resumed copy refuses a target carrying such a record.
        assert!(copy_library(&from, &to, &mut |_| {}).is_err());
        assert!(victim.join("precious.txt").is_file());
    }

    /// A record of THIS library that claims copies which are not true
    /// ones — right sizes, wrong content — deletes nothing: every file
    /// is read back on both sides before it goes.
    #[test]
    fn a_record_claiming_false_copies_deletes_nothing() {
        let dir = Scratch::new("lying");
        let (from, to) = (dir.0.join("old"), dir.0.join("new"));
        library(&from);
        let mut lying = Manifest {
            source: from.clone(),
            complete: true,
            ..Manifest::default()
        };
        for entry in inventory(&from).unwrap() {
            let target = local(&to, &entry.rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, vec![b'x'; entry.size as usize]).unwrap();
            lying.files.insert(
                entry.rel.clone(),
                Copied {
                    sha256: hash_file(&local(&from, &entry.rel)).unwrap(),
                    size: entry.size,
                    modified_ns: entry.modified_ns,
                },
            );
        }
        write_manifest(&to, &lying).unwrap();
        assert_eq!(delete_source(&from, &to).unwrap().removed, 0);
        assert_eq!(inventory(&from).unwrap().len(), 4);
    }

    #[test]
    fn df_output_reads_as_bytes_and_sizes_read_as_words() {
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                  /dev/disk3s5 482000000 437000000 17000000 97% /System/Volumes/Data\n";
        assert_eq!(parse_df(df), Some(17_000_000 * 1024));
        assert_eq!(parse_df("garbage"), None);
        assert_eq!(human(5 * 1_073_741_824 / 2), "2.5 GB");
        assert_eq!(human(300 * 1_048_576), "300 MB");
    }
}
