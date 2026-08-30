//! Chart version files — layer 4 of adaptive charting (ADR-0011).
//!
//! A redesigned chart is a **sibling** of the one it replaces, never
//! an overwrite: `chart.json` is the original, `chart.v2.json`,
//! `chart.v3.json`, … are versions, and a tiny pointer file
//! (`chart-active.json`) names the one the game loads. No pointer —
//! or a broken one — means the original: a bad byte in a pointer must
//! never make a song vanish from the library.
//!
//! The scheme deliberately covers ONE versioned chart per folder,
//! which is the import layout (`songs/imported/<song>/chart.json`).
//! Hand-managed folders holding several charts side by side (the
//! builtin songs) simply have no versions.
//!
//! Everything here is pure over names and strings; whoever owns the
//! files does the IO.

use serde::{Deserialize, Serialize};

/// The pointer file's name.
pub const POINTER_FILE: &str = "chart-active.json";

/// The base chart's name.
pub const BASE_CHART: &str = "chart.json";

/// The pointer file's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivePointer {
    /// The file name of the active chart version.
    pub active: String,
}

/// Whether `name` is a chart version file (`chart.v<N>.json`).
#[must_use]
pub fn is_version_file(name: &str) -> bool {
    version_number(name).is_some()
}

/// The `N` of `chart.v<N>.json`, if `name` is one.
#[must_use]
pub fn version_number(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("chart.v")?.strip_suffix(".json")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The name the next version should be written under, given the file
/// names already in the folder. Starts at `chart.v2.json` — the
/// original IS version one.
#[must_use]
pub fn next_version_name(existing: &[String]) -> String {
    let highest = existing
        .iter()
        .filter_map(|name| version_number(name))
        .max()
        .unwrap_or(1);
    format!("chart.v{}.json", highest + 1)
}

/// Whether a pointer target is a name this scheme could have written.
///
/// The pointer is UNTRUSTED INPUT like every chart file: a target of
/// `../../elsewhere.json` must not escape the folder, so only the two
/// spellable forms pass — the base chart, or a version file. Anything
/// with a path separator fails the version pattern by construction.
#[must_use]
pub fn is_valid_target(name: &str) -> bool {
    name == BASE_CHART || is_version_file(name)
}

/// Which file in this folder is the active chart.
///
/// `pointer_text` is the pointer file's content, if one exists;
/// `files` are the names present in the folder. Every failure mode —
/// no pointer, unparseable pointer, a target that is not a spellable
/// name, a target that does not exist — falls back to the base chart,
/// because the recoverable error is "you see the original" and the
/// unrecoverable one is "your song is gone".
#[must_use]
pub fn resolve_active(pointer_text: Option<&str>, files: &[String]) -> String {
    let fallback = BASE_CHART.to_owned();
    let Some(text) = pointer_text else {
        return fallback;
    };
    let Ok(pointer) = serde_json::from_str::<ActivePointer>(text) else {
        return fallback;
    };
    if is_valid_target(&pointer.active) && files.contains(&pointer.active) {
        pointer.active
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn version_files_are_recognized_and_nothing_else_is() {
        assert!(is_version_file("chart.v2.json"));
        assert!(is_version_file("chart.v10.json"));
        // The base chart is version one, not a version FILE.
        assert!(!is_version_file("chart.json"));
        assert!(!is_version_file("chart-active.json"));
        // Near-misses stay near-misses.
        assert!(!is_version_file("chart.v.json"));
        assert!(!is_version_file("chart.v2a.json"));
        assert!(!is_version_file("chart.v2"));
        assert!(!is_version_file("other.v2.json"));
    }

    #[test]
    fn the_next_version_continues_from_the_highest() {
        assert_eq!(next_version_name(&names(&["chart.json"])), "chart.v2.json");
        assert_eq!(
            next_version_name(&names(&["chart.json", "chart.v2.json", "chart.v7.json"])),
            "chart.v8.json"
        );
        // Unrelated files do not count.
        assert_eq!(
            next_version_name(&names(&["chart.json", "song.m4a", "chart-active.json"])),
            "chart.v2.json"
        );
    }

    #[test]
    fn the_pointer_selects_an_existing_version() {
        let files = names(&["chart.json", "chart.v2.json", "chart-active.json"]);
        let pointer = "{\"active\":\"chart.v2.json\"}";
        assert_eq!(resolve_active(Some(pointer), &files), "chart.v2.json");
    }

    #[test]
    fn every_broken_pointer_falls_back_to_the_original() {
        // The recoverable failure is "you see the original"; a song
        // must never vanish because one byte in a pointer went wrong.
        let files = names(&["chart.json", "chart.v2.json"]);
        for text in [
            None,                                   // no pointer at all
            Some("not json"),                       // unparseable
            Some("{\"active\":\"chart.v9.json\"}"), // target missing
            Some("{\"wrong_field\":\"chart.v2.json\"}"),
        ] {
            assert_eq!(resolve_active(text, &files), "chart.json", "for {text:?}");
        }
    }

    #[test]
    fn a_pointer_cannot_escape_its_folder() {
        // The pointer is untrusted input like every chart file. A
        // target with a path in it is not a name this scheme writes,
        // so it must not be followed — even if a matching entry were
        // somehow present in the listing.
        let files = names(&["chart.json", "../../evil.json", "/tmp/evil.json"]);
        for target in ["../../evil.json", "/tmp/evil.json", "chart.v2.json/../x"] {
            let pointer = format!("{{\"active\":\"{target}\"}}");
            assert_eq!(
                resolve_active(Some(&pointer), &files),
                "chart.json",
                "{target} must not be followed"
            );
        }
    }
}
