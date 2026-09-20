//! # beatbyte-telemetry
//!
//! The gameplay blackbox: what the chart expected, what the player
//! did, how the engine read it and how it was judged — recorded as a
//! versioned, queryable event history rather than as a log file.
//!
//! This is layer 1 of adaptive charting (ADR-0011) grown up. The
//! reasoning behind the shape, and the alternatives that were
//! weighed, are in ADR-0018.
//!
//! ## What it is for
//!
//! Evidence. Whether a generated chart is too hard in a particular
//! bar, whether a controller drops inputs, whether a player has a
//! constant calibration bias, whether generator v18 actually beat
//! v17. It is **not** a debug log, and nothing here changes a chart,
//! a score or a setting: analytics produce evidence, and changing an
//! algorithm stays a deliberate, versioned act.
//!
//! ## The four domains, kept apart
//!
//! | Domain | Lives in | This crate |
//! |---|---|---|
//! | Song data | the audio and its sidecars | referenced |
//! | Analysis / chart data | `chart.vN.json`, its context sidecar | referenced by hash |
//! | **Gameplay telemetry** | this store | **owned** |
//! | Derived analytics | [`analytics`], `history.jsonl` | computed, never authoritative |
//!
//! An event stores *which* note, never *what kind of song moment* it
//! was: that is a join away, and duplicating it per event would cost
//! the entire storage budget for data already on disk.
//!
//! ## What it deliberately does not store
//!
//! Anything derivable from what it does store: combo, score,
//! accuracy, per-note counts, "early / late" (the sign of
//! `delta_us`), sustain *starts* (a hit on a note that has a tail),
//! phrase completions (the hits inside the phrase's span). The one
//! documented exception is the note-shape flags — see
//! [`model::Flags`].
//!
//! ## Privacy
//!
//! Local, and only local. Nothing in this crate opens a socket; there
//! is no upload, no identifier beyond the local roster's own number,
//! and no microphone audio — a sung note is stored as a cent error
//! and a rating, never as a sample. The physical input layer is
//! opt-in ([`model::Detail::Diagnostic`]) and even then records only
//! the actions BeatByte itself is bound to; keys the game is not
//! listening for never reach it.
//!
//! ## Durability, stated honestly
//!
//! The store runs in WAL mode with `synchronous = NORMAL`: a crash of
//! the game loses at most the events since the last batch commit, and
//! a session that was never closed is visible as one — `ended_ms` is
//! null. A power cut can still cost the tail of the write-ahead log.
//! No stronger promise is made here, because none can be kept.
//!
//! ## Module map
//!
//! - [`model`] — the vocabulary and its stable integer encodings
//! - [`schema`] — the tables and the migration runner
//! - [`store`] — the synchronous database
//! - [`writer`] — the queue and worker thread the game records through
//! - [`analytics`] — the questions the store was shaped to answer
//! - [`mod@bench`] — filling it with a lifetime of playing and timing that
//! - [`legacy`] — importing the older per-session JSONL files
//! - [`export`] — handing a session or a dataset to another tool

pub mod analytics;
pub mod bench;
pub mod export;
pub mod legacy;
pub mod model;
pub mod schema;
pub mod store;
pub mod writer;

pub use model::{
    Action, Completion, Detail, Event, EventType, Flags, InputDevice, Outcome, Provenance, Rating,
    SessionRow,
};
pub use schema::schema_version;
pub use store::{PlayerNote, SessionId, Store, StoredSession};
pub use writer::{Telemetry, WriterStats};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What can go wrong on the way to or from the store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database said no.
    #[error("telemetry store: {0}")]
    Store(#[from] rusqlite::Error),
    /// The file system said no.
    #[error("telemetry store: {0}")]
    Io(#[from] std::io::Error),
    /// A legacy file could not be read as one.
    #[error("telemetry import: {0}")]
    Import(String),
}

/// The result type every fallible call in this crate returns.
pub type Result<T> = std::result::Result<T, Error>;

/// Unix milliseconds now, or `0` if the clock is before the epoch.
///
/// Telemetry never panics, and a machine whose clock is wrong is a
/// machine with wrong timestamps, not a machine that crashes.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A locally unique id for one run.
///
/// Not an RFC-4122 UUID and it does not pretend to be one: there is
/// no dependency here that makes one, and what a session id has to be
/// is *unique on this machine and stable once written* — so that an
/// export, an import and a re-import all name the same run. It is the
/// start time, the slot and a scramble of the process id and the
/// nanosecond clock, as 32 hex characters.
#[must_use]
pub fn session_uid(started_ms: u64, slot: u8) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let mut seed = started_ms
        ^ (u64::from(nanos) << 21)
        ^ (u64::from(std::process::id()) << 43)
        ^ u64::from(slot);
    // splitmix64: one round is plenty to spread a low-entropy seed
    // across the whole word, and it is four lines of arithmetic.
    seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = seed;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    format!("{started_ms:016x}{mixed:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_workspace_scheme() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn a_session_id_is_thirty_two_hex_characters_and_names_its_start() {
        let uid = session_uid(1_700_000_000_000, 0);
        assert_eq!(uid.len(), 32);
        assert!(uid.chars().all(|c| c.is_ascii_hexdigit()), "{uid}");
        // The first half IS the start time, so a row's id can be read
        // back to the moment it names without a lookup.
        assert_eq!(
            u64::from_str_radix(&uid[..16], 16),
            Ok(1_700_000_000_000),
            "the start time is readable in the id: {uid}"
        );
    }

    #[test]
    fn two_players_in_one_run_do_not_share_an_id() {
        // Same millisecond, same process — the slot has to separate
        // them, or the second player's session is refused by the
        // unique index and their evidence is simply lost.
        let one = session_uid(1_700_000_000_000, 0);
        let two = session_uid(1_700_000_000_000, 1);
        assert_ne!(one, two);
    }
}
