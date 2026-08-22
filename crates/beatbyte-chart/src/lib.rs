//! # beatbyte-chart
//!
//! The versioned BeatByte chart file format: schema, serialization and
//! validation. Charts are untrusted input — everything loaded through
//! this crate is validated before it reaches gameplay.
//!
//! The format is JSON with an explicit `format_version` field; see
//! `docs/chart-format/` in the repository for the specification.
//!
//! Implemented in Milestone 2.

/// The chart format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn format_version_is_one() {
        assert_eq!(super::FORMAT_VERSION, 1);
    }
}
