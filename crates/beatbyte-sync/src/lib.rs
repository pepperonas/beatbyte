//! # beatbyte-sync
//!
//! Two devices, one career (ADR-0021): the merge rules, as pure
//! functions from two snapshots to one. No I/O, no transport, no
//! clock — the command line reads the files, calls these, and writes
//! the results while the game is closed.
//!
//! One module per kind of data, each with its own rule and never a
//! blanket "last writer wins":
//!
//! - [`players`] — collisions from the old per-device counter and one
//!   person under two ids, resolved into a remap per side that every
//!   other rule applies first;
//! - [`history`] — the union of the play logs, each run once, the
//!   richer copy of a run both have;
//! - [`scores`] — the better result per song and difficulty;
//! - [`achievements`] — the union, the earliest date;
//! - [`settings`] — shared keys by newest change, device keys never;
//! - [`telemetry`] — which sessions to insert or replace, by `uid`;
//! - [`library`] — song folders file by file, by content, with
//!   tombstones for deletions and two versions kept where both devices
//!   made one.
//!
//! Every rule is order-independent (both devices end on the same
//! state) and idempotent (a second sync changes nothing); the tests
//! check both.

pub mod achievements;
pub mod history;
pub mod library;
pub mod players;
pub mod scores;
pub mod settings;
pub mod telemetry;
