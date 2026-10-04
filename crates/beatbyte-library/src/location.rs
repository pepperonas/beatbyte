//! Where this device keeps its song library.
//!
//! By default the library is `<data>/beatbyte/songs/imported`. A player
//! may put it anywhere else — an external drive, say — and that choice
//! is recorded in [`LOCATION_FILE`] beside the game's other data. It is
//! a DEVICE setting (ADR-0021): a path on one Mac means nothing on the
//! other, so it lives in its own file and never in the shared settings
//! that sync carries.
//!
//! Everything that writes into the library or reads it as a whole —
//! the game's scan and import, the Bridge import, the CLI's sync —
//! asks [`library_root`] here, so the four of them cannot drift apart
//! the way four hard-coded joins could.
//!
//! ⚠️ **A chosen library that is not there is not an empty library.**
//! An unplugged drive must not make the game import into the internal
//! disk (the songs would then be split over two places) and must not
//! let sync read every song as deleted. [`Root::reachable`] says
//! which, and the callers refuse rather than fall back.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// The file, in `<data>/beatbyte/`, that names a chosen library.
pub const LOCATION_FILE: &str = "library-location.json";

/// What [`LOCATION_FILE`] holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// The folder the song folders live in.
    pub root: PathBuf,
}

/// The library this device uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// The folder the song folders live in.
    pub path: PathBuf,
    /// Whether the player chose it (else it is the default).
    pub chosen: bool,
    /// Whether it is there to be read and written.
    pub reachable: bool,
}

/// The default library under `data` (`<data>/beatbyte`).
#[must_use]
pub fn default_root(data: &Path) -> PathBuf {
    data.join("songs").join("imported")
}

/// The chosen library, if one is recorded and readable.
#[must_use]
pub fn chosen_root(data: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data.join(LOCATION_FILE)).ok()?;
    let location: Location = serde_json::from_str(&text).ok()?;
    location.root.is_absolute().then_some(location.root)
}

/// The library this device uses: the chosen one when there is one,
/// whether or not it is reachable right now, else the default (which
/// counts as reachable — it is created on first import).
#[must_use]
pub fn library_root(data: &Path) -> Root {
    match chosen_root(data) {
        Some(path) => Root {
            reachable: path.is_dir(),
            path,
            chosen: true,
        },
        None => Root {
            path: default_root(data),
            chosen: false,
            reachable: true,
        },
    }
}

/// Record `root` as this device's library. Writing the default path
/// removes the record instead, so the device is back on the default.
///
/// # Errors
/// When the record cannot be written.
pub fn choose(data: &Path, root: &Path) -> std::io::Result<()> {
    if root == default_root(data) {
        return match std::fs::remove_file(data.join(LOCATION_FILE)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string_pretty(&Location {
        root: root.to_path_buf(),
    })
    .map_err(std::io::Error::other)?;
    std::fs::create_dir_all(data)?;
    beatbyte_chart::io::write_atomic(&data.join(LOCATION_FILE), text.as_bytes())
}

/// Set while a library move copies: writers into the library refuse
/// until it is over, so nothing lands in the copy's source after its
/// last pass.
static MOVING: AtomicBool = AtomicBool::new(false);

/// Whether a library move is running in this process.
#[must_use]
pub fn moving() -> bool {
    MOVING.load(Ordering::SeqCst)
}

/// Marks a move as running for as long as it lives.
#[derive(Debug)]
pub struct MovingGuard(());

impl MovingGuard {
    /// Mark a move as running, or `None` when one already is.
    #[must_use]
    pub fn acquire() -> Option<MovingGuard> {
        MOVING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then_some(MovingGuard(()))
    }
}

impl Drop for MovingGuard {
    fn drop(&mut self) {
        MOVING.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beatbyte-location-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn without_a_choice_the_default_is_used_and_reachable() {
        let data = scratch("default");
        let root = library_root(&data);
        assert_eq!(root.path, data.join("songs/imported"));
        assert!(!root.chosen);
        assert!(root.reachable);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_chosen_library_that_is_not_there_is_unreachable_not_replaced() {
        let data = scratch("missing");
        let drive = data.join("unplugged/BeatByte");
        choose(&data, &drive).unwrap();
        let root = library_root(&data);
        assert_eq!(root.path, drive, "never a silent fallback to the default");
        assert!(root.chosen);
        assert!(!root.reachable);
        std::fs::create_dir_all(&drive).unwrap();
        assert!(library_root(&data).reachable);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn choosing_the_default_again_removes_the_record() {
        let data = scratch("back");
        choose(&data, &data.join("x")).unwrap();
        assert!(data.join(LOCATION_FILE).is_file());
        choose(&data, &default_root(&data)).unwrap();
        assert!(!data.join(LOCATION_FILE).exists());
        assert!(!library_root(&data).chosen);
        // A broken or relative record reads as no choice.
        std::fs::write(data.join(LOCATION_FILE), r#"{"root":"relative/path"}"#).unwrap();
        assert!(!library_root(&data).chosen);
        std::fs::write(data.join(LOCATION_FILE), "garbage").unwrap();
        assert!(!library_root(&data).chosen);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn one_move_at_a_time() {
        let first = MovingGuard::acquire().unwrap();
        assert!(moving());
        assert!(MovingGuard::acquire().is_none());
        drop(first);
        assert!(!moving());
    }
}
