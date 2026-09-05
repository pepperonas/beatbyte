//! Where models live on disk, and how they get there.
//!
//! One folder per model under the store root (`<app data>/beatbyte/
//! models/<id>/<file>`). A download is streamed into a `.part` file
//! beside its final name, capped at the registered size, hashed as it
//! arrives, and renamed into place only when both the size and the
//! SHA-256 match the registry. Anything else — a short read, a wrong
//! hash, a cancel, a server that keeps sending — leaves no file behind
//! but the one that was there before.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::error::MlError;
use crate::hash::{Hashing, sha256_file};
use crate::registry::ModelSpec;

/// How long to wait for the connection and for each read.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The read chunk; progress is reported per chunk.
const CHUNK: usize = 1 << 16;

/// Where one model stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Not on disk.
    Missing,
    /// On disk and matching its registered hash.
    Installed,
    /// On disk but not matching — a partial or tampered file. The
    /// store will not load it; installing again replaces it.
    Damaged {
        /// The file's actual SHA-256.
        actual: String,
    },
}

/// How far a download has come, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes received so far.
    pub done: u64,
    /// The registered size — the total the download will reach.
    pub total: u64,
}

/// The on-disk home of installed models.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    /// A store rooted at `root` (created on first install).
    #[must_use]
    pub fn at(root: PathBuf) -> ModelStore {
        ModelStore { root }
    }

    /// The store beside the game's settings: `<config dir>/beatbyte/models`.
    /// `None` on a platform without a config directory.
    #[must_use]
    pub fn default_location() -> Option<ModelStore> {
        dirs::config_dir().map(|dir| ModelStore::at(dir.join("beatbyte").join("models")))
    }

    /// The store's root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a model's file is (or would be).
    #[must_use]
    pub fn path(&self, spec: &ModelSpec) -> PathBuf {
        self.root.join(spec.id).join(spec.file)
    }

    /// Where a model stands: missing, installed, or damaged. Hashes the
    /// file when it exists — hundreds of megabytes, so not per frame.
    #[must_use]
    pub fn status(&self, spec: &ModelSpec) -> Status {
        let path = self.path(spec);
        if !path.is_file() {
            return Status::Missing;
        }
        match sha256_file(&path) {
            Ok(actual) if actual == spec.sha256 => Status::Installed,
            Ok(actual) => Status::Damaged { actual },
            Err(error) => Status::Damaged {
                actual: format!("unreadable: {error}"),
            },
        }
    }

    /// The model's path if it is installed and intact.
    pub fn verify(&self, spec: &ModelSpec) -> Result<PathBuf, MlError> {
        match self.status(spec) {
            Status::Installed => Ok(self.path(spec)),
            Status::Missing => Err(MlError::NotInstalled {
                id: spec.id.to_owned(),
            }),
            Status::Damaged { actual } => Err(MlError::Damaged {
                id: spec.id.to_owned(),
                expected: spec.sha256.to_owned(),
                actual,
            }),
        }
    }

    /// Fetch a model — the one thing in this crate that touches the
    /// network, and only ever on the user's explicit action.
    ///
    /// Blocking; call it off the frame thread. `progress` is called
    /// per chunk; `cancel` is checked per chunk. Returns the installed
    /// file's path.
    pub fn install(
        &self,
        spec: &ModelSpec,
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<PathBuf, MlError> {
        let final_path = self.path(spec);
        let dir = final_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone());
        std::fs::create_dir_all(&dir).map_err(|source| MlError::Io {
            path: dir.clone(),
            source,
        })?;
        let part_path = final_path.with_extension(format!(
            "{}.part",
            final_path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));

        if let Err(error) = self.download_to(spec, &part_path, progress, cancel) {
            let _ = std::fs::remove_file(&part_path);
            return Err(error);
        }
        std::fs::rename(&part_path, &final_path).map_err(|source| MlError::Io {
            path: final_path.clone(),
            source,
        })?;
        Ok(final_path)
    }

    /// The streaming half of [`ModelStore::install`]: everything that
    /// has to be right before a byte is trusted.
    fn download_to(
        &self,
        spec: &ModelSpec,
        part_path: &Path,
        progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<(), MlError> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(TIMEOUT)
            .timeout_read(TIMEOUT)
            .build();
        let response = agent
            .get(spec.url)
            .call()
            .map_err(|error| MlError::Download {
                url: spec.url.to_owned(),
                reason: error.to_string(),
            })?;
        let mut body = response.into_reader();
        let mut file = std::fs::File::create(part_path).map_err(|source| MlError::Io {
            path: part_path.to_path_buf(),
            source,
        })?;
        let mut hashing = Hashing::new();
        let mut done = 0u64;
        let mut buffer = vec![0u8; CHUNK];
        progress(Progress {
            done: 0,
            total: spec.bytes,
        });
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MlError::Cancelled {
                    id: spec.id.to_owned(),
                });
            }
            let read = body.read(&mut buffer).map_err(|error| MlError::Download {
                url: spec.url.to_owned(),
                reason: error.to_string(),
            })?;
            if read == 0 {
                break;
            }
            done += read as u64;
            // A server that keeps sending past the registered size is
            // not sending the registered file. Stop before writing.
            if done > spec.bytes {
                return Err(MlError::TooLarge {
                    id: spec.id.to_owned(),
                    expected: spec.bytes,
                });
            }
            hashing.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|source| MlError::Io {
                    path: part_path.to_path_buf(),
                    source,
                })?;
            progress(Progress {
                done,
                total: spec.bytes,
            });
        }
        file.flush().map_err(|source| MlError::Io {
            path: part_path.to_path_buf(),
            source,
        })?;
        drop(file);
        let actual = hashing.finish();
        if done != spec.bytes || actual != spec.sha256 {
            return Err(MlError::Damaged {
                id: spec.id.to_owned(),
                expected: spec.sha256.to_owned(),
                actual: if done == spec.bytes {
                    actual
                } else {
                    format!("{actual} ({done} of {} bytes)", spec.bytes)
                },
            });
        }
        Ok(())
    }

    /// Delete a model's folder. Not an error if there is nothing.
    pub fn remove(&self, spec: &ModelSpec) -> Result<(), MlError> {
        let dir = self.root.join(spec.id);
        if !dir.exists() {
            return Ok(());
        }
        std::fs::remove_dir_all(&dir).map_err(|source| MlError::Io { path: dir, source })
    }
}
