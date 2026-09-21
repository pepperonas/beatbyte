//! Turning what a file says into what is true.
//!
//! Tags are written by everyone and everything, and a great many of
//! them say nothing in a way that looks like something: an empty
//! string, a lone dash, the word `Unknown`. Stored as-is they become
//! a genre called "Unknown" in the browser's filter, a release year
//! of 0, and an artist nobody can search for.
//!
//! The rule for the whole crate: **absent is `None`, never a
//! placeholder.**

/// The exact strings that mean "nothing was filled in".
///
/// Deliberately short, and matched only against the WHOLE trimmed
/// value, case-insensitively. `Unknown Mortal Orchestra` is a band;
/// `Untitled` is a real title for a real track; `Nada` is a word in
/// Spanish. A longer list would start eating real data, and a field
/// that silently loses a real artist is worse than one that keeps a
/// useless word.
const NOTHING: &[&str] = &[
    "",
    "-",
    "--",
    "n/a",
    "unknown",
    "unknown artist",
    "unknown album",
];

/// A tag value as it should be stored: trimmed, and `None` when it
/// carries no information. Pure — tested.
#[must_use]
pub fn text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let folded = trimmed.to_ascii_lowercase();
    if NOTHING.contains(&folded.as_str()) {
        return None;
    }
    Some(trimmed.to_owned())
}

/// A release year as it should be stored.
///
/// `0` is not a year, and neither is a tag that holds a whole date.
/// The range is wide enough for a remastered wax cylinder and narrow
/// enough to catch a corrupted field.
#[must_use]
pub fn year(raw: i64) -> Option<u16> {
    (1860..=2200)
        .contains(&raw)
        .then(|| u16::try_from(raw).ok())
        .flatten()
}

/// A list of genres from one tag, which may carry several.
///
/// Separators seen in the wild: `;`, `/`, and the comma. Each part
/// goes through [`text`], so `Electronic; ; Deep House` yields two
/// genres and not three.
#[must_use]
pub fn genres(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split([';', '/', ',']) {
        if let Some(clean) = text(part)
            && !out.iter().any(|have| have.eq_ignore_ascii_case(&clean))
        {
            out.push(clean);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_none_rather_than_a_word() {
        assert_eq!(text(""), None);
        assert_eq!(text("   "), None);
        assert_eq!(text("Unknown"), None);
        assert_eq!(text("unknown artist"), None);
        assert_eq!(text("-"), None);
        assert_eq!(text("N/A"), None);
    }

    #[test]
    fn a_real_name_that_looks_like_a_placeholder_survives() {
        // The counter-case is the whole reason the list is matched
        // whole and kept short.
        assert_eq!(
            text("Unknown Mortal Orchestra").as_deref(),
            Some("Unknown Mortal Orchestra")
        );
        assert_eq!(text("Untitled").as_deref(), Some("Untitled"));
        assert_eq!(text("  The Knife  ").as_deref(), Some("The Knife"));
    }

    #[test]
    fn a_year_of_zero_is_not_a_year() {
        assert_eq!(year(0), None);
        assert_eq!(year(-1), None);
        assert_eq!(year(20_180), None);
        assert_eq!(year(2018), Some(2018));
        assert_eq!(year(1888), Some(1888));
    }

    #[test]
    fn one_tag_can_hold_several_genres_without_holding_blanks() {
        assert_eq!(
            genres("Electronic; Deep House / Nu Disco"),
            vec!["Electronic", "Deep House", "Nu Disco"]
        );
        assert_eq!(
            genres("Electronic; ; Deep House"),
            vec!["Electronic", "Deep House"]
        );
        assert_eq!(
            genres("House, house"),
            vec!["House"],
            "the same genre twice in one tag is one genre"
        );
        assert!(genres("Unknown").is_empty());
        assert!(genres("").is_empty());
    }
}
