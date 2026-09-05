//! # beatbyte-ml
//!
//! The local machine-learning runtime BeatByte's learned features
//! stand on (ADR-0013, plan `docs/plans/ai-song-graph-upgrade.md`):
//!
//! - [`registry`] — the models this build knows: id, file, a URL the
//!   project controls, exact size, SHA-256, licence. Compiled in; no
//!   manifest is ever fetched.
//! - [`store`] — where a model lives on disk once the user asked for
//!   it, and how it gets there: streamed to a `.part` file, verified
//!   against size and hash **before** it is renamed into place.
//! - [`runtime`] — loading a stored model and running it with a
//!   pinned thread count, so the same input gives the same output on
//!   the same platform, every time.
//! - [`hash`] — SHA-256 over bytes and files.
//!
//! **No domain logic.** Nothing in here knows what a waveform or a
//! transcript is; the aligner, the separator and the beat tracker are
//! consumers. **Nothing in here runs unless asked**: the crate is
//! linked into the game and the CLI only behind their `ml` feature,
//! and a download happens only on an explicit user action.
//!
//! Everything a model ever produces should carry [`FINGERPRINT`] and
//! the model's hash, so a cached result never changes silently under
//! the player.

pub mod error;
pub mod hash;
pub mod registry;
pub mod runtime;
pub mod store;

pub use error::MlError;
pub use registry::{ModelSpec, REGISTRY, WAV2VEC2_BASE_960H, spec};
pub use runtime::{Input, Loaded, Output, Runtime, THREADS};
pub use store::{ModelStore, Progress, Status};

/// The runtime's identity, for the provenance of anything it
/// produces: the inference crate and its version. A result computed
/// under a different fingerprint is a different result.
pub const FINGERPRINT: &str = concat!("rten-", "0.26");
