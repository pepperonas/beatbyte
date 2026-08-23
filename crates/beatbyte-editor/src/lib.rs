//! # beatbyte-editor
//!
//! The engine-free heart of the chart editor: invertible edit
//! operations and an [`EditorSession`] with undo/redo and dirtiness
//! tracking. The in-game editor screen (in `beatbyte-game`) is a thin
//! presentation over this crate — every editing rule here is
//! unit-tested without an engine, mirroring the core/session split
//! (ADR-0002).

pub mod ops;
pub mod session;

pub use ops::{EDIT_EPSILON_S, EditError, EditOp};
pub use session::EditorSession;

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
