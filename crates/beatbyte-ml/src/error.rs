//! What can go wrong, named so the caller can say it to the player.

use std::path::PathBuf;

use thiserror::Error;

/// Errors from the model store and the runtime.
#[derive(Debug, Error)]
pub enum MlError {
    /// The model has not been downloaded.
    #[error("model `{id}` is not installed")]
    NotInstalled {
        /// The model's registry id.
        id: String,
    },
    /// The file on disk does not match the registry's hash — a partial
    /// download, disk corruption, or a file someone swapped in.
    #[error(
        "model `{id}` on disk does not match its registered hash (expected {expected}, found {actual})"
    )]
    Damaged {
        /// The model's registry id.
        id: String,
        /// The registered SHA-256, lowercase hex.
        expected: String,
        /// The file's SHA-256, lowercase hex.
        actual: String,
    },
    /// The download could not be started or completed.
    #[error("cannot download `{url}`: {reason}")]
    Download {
        /// The URL that was asked.
        url: String,
        /// What went wrong, in the transport's words.
        reason: String,
    },
    /// The server sent more than the registered size — nothing is
    /// trusted past that point.
    #[error("download of `{id}` exceeded its registered size of {expected} bytes")]
    TooLarge {
        /// The model's registry id.
        id: String,
        /// The size the registry promised.
        expected: u64,
    },
    /// The user cancelled the download; nothing was kept.
    #[error("download of `{id}` cancelled")]
    Cancelled {
        /// The model's registry id.
        id: String,
    },
    /// A file operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The model file did not load — not an ONNX graph the runtime
    /// understands, or one that uses an operator it does not have.
    #[error("model `{id}` cannot be loaded: {reason}")]
    Model {
        /// The model's registry id.
        id: String,
        /// The runtime's words.
        reason: String,
    },
    /// Inference failed — wrong input shape, a missing node, an
    /// operator that rejected its inputs.
    #[error("model `{id}` failed to run: {reason}")]
    Run {
        /// The model's registry id.
        id: String,
        /// The runtime's words.
        reason: String,
    },
}
