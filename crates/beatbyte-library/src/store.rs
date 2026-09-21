//! Reading and writing the document in a song's folder.
//!
//! The only part of this crate that touches a disk, and it touches
//! exactly one file per song: `song.json`. A migration that also
//! rewrote charts, sidecars or audio would be a migration nobody
//! could safely run twice.
//!
//! Writes are atomic — a temporary file beside the target, then a
//! rename — because the alternative is a power cut leaving half a
//! document where a whole one used to be, and this file is the only
//! copy of the player's own edits.

use std::io;
use std::path::{Path, PathBuf};

use crate::build::Built;
use crate::doc::{DOC_FILE, SongDoc};

/// Where a song folder's document lives.
#[must_use]
pub fn path(dir: &Path) -> PathBuf {
    dir.join(DOC_FILE)
}

/// Read a folder's document, if it has one that parses.
///
/// A document that cannot be parsed is reported as absent rather
/// than as an error: the caller's next move is to build a fresh one
/// from the folder, which is a better outcome than refusing to scan
/// a library because one file is damaged.
#[must_use]
pub fn read(dir: &Path) -> Option<SongDoc> {
    let text = std::fs::read_to_string(path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write the document when — and only when — something changed.
///
/// Returns whether it wrote. The `changed` flag comes from
/// [`crate::build::document_for`], which compares the whole document
/// rather than trusting the caller to notice.
pub fn save_if_changed(dir: &Path, built: &Built) -> io::Result<bool> {
    if !built.changed {
        return Ok(false);
    }
    save(dir, &built.doc)?;
    Ok(true)
}

/// Write the document, atomically.
pub fn save(dir: &Path, doc: &SongDoc) -> io::Result<()> {
    let target = path(dir);
    let mut text = serde_json::to_string_pretty(doc)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    text.push('\n');
    let temporary = target.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{Built, FolderFacts, LyricFacts, document_for};
    use crate::{SongId, SourceKind};

    fn folder() -> (tempdir::TempDir, FolderFacts<'static>) {
        let dir = tempdir::TempDir::new();
        let facts = FolderFacts {
            chart: None,
            chart_version: None,
            audio_filename: "maria.m4a".to_owned(),
            extension: Some("m4a".to_owned()),
            oldest_file_ms: 1_700_000_000_000,
            loudness: None,
            lyrics: LyricFacts::default(),
            source_kind: SourceKind::LocalFile,
        };
        (dir, facts)
    }

    /// A directory that removes itself, so the tests need no crate.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> TempDir {
                let unique = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let counter =
                    std::hash::BuildHasher::hash_one(&std::hash::RandomState::new(), unique);
                let path = std::env::temp_dir().join(format!("bb-lib-{unique}-{counter}"));
                std::fs::create_dir_all(&path).expect("a temporary directory");
                TempDir(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_document_round_trips_through_a_real_folder() {
        let (dir, facts) = folder();
        assert_eq!(read(dir.path()), None, "an empty folder has no document");

        let built = document_for(&facts, None, SongId::from_parts(1, 1), 5_000);
        assert!(save_if_changed(dir.path(), &built).expect("writes"));
        assert_eq!(read(dir.path()).as_ref(), Some(&built.doc));
    }

    #[test]
    fn an_unchanged_pass_does_not_touch_the_file() {
        let (dir, facts) = folder();
        let built = document_for(&facts, None, SongId::from_parts(1, 1), 5_000);
        save_if_changed(dir.path(), &built).expect("writes");
        let before = std::fs::read(path(dir.path())).expect("reads back");

        let again = Built {
            doc: built.doc.clone(),
            changed: false,
        };
        assert!(
            !save_if_changed(dir.path(), &again).expect("does nothing"),
            "an unchanged song must not be rewritten"
        );
        assert_eq!(
            std::fs::read(path(dir.path())).expect("still there"),
            before,
            "not one byte"
        );
    }

    #[test]
    fn a_damaged_document_reads_as_absent_rather_than_as_a_failure() {
        // One bad file must not stop a library from being scanned;
        // the caller's next move is to rebuild it from the folder.
        let (dir, _) = folder();
        std::fs::write(path(dir.path()), "{ this is not json").expect("writes junk");
        assert_eq!(read(dir.path()), None);
    }

    #[test]
    fn writing_leaves_no_temporary_behind() {
        let (dir, facts) = folder();
        let built = document_for(&facts, None, SongId::from_parts(1, 1), 5_000);
        save_if_changed(dir.path(), &built).expect("writes");
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("reads the folder")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![DOC_FILE.to_owned()],
            "the atomic write's temporary file must be gone"
        );
    }
}
