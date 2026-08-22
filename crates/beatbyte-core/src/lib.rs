//! # beatbyte-core
//!
//! The engine-free domain model of BeatByte: lanes, notes, timing,
//! judgment, scoring and gameplay rules.
//!
//! This crate deliberately has **no** dependency on Bevy, audio backends
//! or any I/O. Every gameplay rule defined here is unit-testable with
//! plain values, which is what makes deterministic rhythm-game timing
//! possible (see ADR-0002).

pub mod lane;

pub use lane::Lane;

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_workspace_scheme() {
        // Semantic versioning: MAJOR.MINOR.PATCH
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "version component `{part}` must be numeric"
            );
        }
    }
}
