//! Entry-point policy for adding and regenerating playable song charts.
//!
//! Legacy algorithms remain available as library code for historical fixtures
//! and research, but the game and authoring CLI must not run them for imports.

/// Only the installed audio-to-chart pipeline may create new game charts.
/// No setting, environment variable or harness switch overrides this policy.
pub const LEGACY_IMPORTS_ENABLED: bool = false;

/// Explanation for an archived import/authoring entry point.
pub const DISABLED_MESSAGE: &str = "This chart import/generator is disabled. Import an audio file with the installed song-chart pipeline instead.";

/// Refuse an archived import before it reads, downloads or writes anything.
///
/// # Errors
/// Always while legacy imports are disabled.
pub fn require_legacy_imports() -> Result<(), String> {
    if LEGACY_IMPORTS_ENABLED {
        Ok(())
    } else {
        Err(DISABLED_MESSAGE.to_owned())
    }
}
