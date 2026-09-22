//! Matching a song against a music catalogue.
//!
//! Two rules, and they are the same two wherever a catalogue is
//! asked anything: **is this entry the same recording** (by length,
//! because a remix and its original share every word of their name),
//! and **what should we even ask for** (because an imported file
//! carries the uploader's furniture in its title).
//!
//! They live here rather than beside one caller because there are
//! three: the lyrics lookup, the song search, and the metadata
//! catalogue. Three copies of a length tolerance is three different
//! tolerances a year from now.

/// How far a catalogue entry's length may be from the song's before
/// its stamps are for a different edit.
///
/// ⚠️ Measured, not guessed. A library song — an 8:37 remix — was
/// handed the 4-minute original's lyrics, because the catalogue has
/// the words under that name and nothing checked the length. Every
/// stamp was then wrong, and the fallback crammed the whole sheet
/// into the first 45 % of the track. lrclib matches its own
/// `duration` parameter within two seconds; this is the tolerance
/// for the search fallback, wide enough for a fade difference and
/// far too narrow for another edit.
pub const DURATION_TOLERANCE_S: f64 = 12.0;

/// …and how far as a SHARE of the song's own length.
///
/// A fixed number cannot do this job alone, and the library shows
/// both ends of why. Our rip of *The Bad Touch* is 245 s while every
/// one of the catalogue's twenty entries sits near 260 — fifteen
/// seconds, six per cent, plainly the same recording ripped with a
/// different tail. The Annie remix that started this rule is 517 s
/// against the original's 239 — two hundred and seventy-eight
/// seconds, more than half, and its words are genuinely a different
/// sheet. One absolute threshold either lets the remix through or
/// turns away the rip.
pub const DURATION_TOLERANCE_SHARE: f64 = 0.08;

/// Whether a catalogue entry is this recording, by length.
///
/// Pure — tested against the two cases above, which are the reason
/// it takes the larger of an absolute and a relative allowance.
#[must_use]
pub fn duration_fits(ours_s: f64, theirs_s: f64) -> bool {
    if !ours_s.is_finite() || !theirs_s.is_finite() || ours_s <= 0.0 || theirs_s <= 0.0 {
        return false;
    }
    let allowance = DURATION_TOLERANCE_S.max(ours_s * DURATION_TOLERANCE_SHARE);
    (ours_s - theirs_s).abs() <= allowance
}

/// The artist and title to ask a catalogue with, cleaned of what a
/// download put there.
///
/// Imported files carry the uploader's furniture: `- OFFICIAL VIDEO`,
/// `(Official Music Video)`, `[HD]`, a `- Topic` channel as the
/// artist. lrclib's `get` matches the names closely, so a title with
/// any of that attached can only ever miss — three songs in this
/// library never had a chance.
///
/// ⚠️ What it must NOT strip is a real subtitle. `Two of Hearts -
/// Skatebård Remix` is a different recording from `Two of Hearts`,
/// and asking for the wrong one is the mistake this whole area of
/// the code exists to prevent. So only known furniture goes, never a
/// generic `- something`. Pure — tested.
#[must_use]
pub fn clean_query(artist: &str, title: &str) -> (String, String) {
    const FURNITURE: [&str; 12] = [
        "official video",
        "official music video",
        "official audio",
        "official lyric video",
        "official visualizer",
        "music video",
        "lyric video",
        "audio only",
        "hd",
        "hq",
        "4k",
        "remastered audio",
    ];
    // Fullwidth punctuation comes in from filename-safe renaming.
    let unwiden = |text: &str| text.replace('，', ",").replace('：', ":");
    let strip_furniture = |text: &str| {
        let mut out = text.to_owned();
        loop {
            let lower = out.to_lowercase();
            let cut = FURNITURE.iter().find_map(|word| {
                // Only where it is bracketed or trails after a dash:
                // the words alone can be part of a real title.
                for (open, close) in [('(', ')'), ('[', ']')] {
                    let needle = format!("{open}{word}{close}");
                    if let Some(at) = lower.find(&needle) {
                        return Some((at, at + needle.len()));
                    }
                }
                let tail = format!("- {word}");
                lower
                    .rfind(&tail)
                    .filter(|at| at + tail.len() == lower.len())
                    .map(|at| (at, lower.len()))
            });
            let Some((from, to)) = cut else { break };
            out.replace_range(from..to, "");
            out = out
                .trim()
                .trim_end_matches(['-', '–', '|'])
                .trim()
                .to_owned();
        }
        out
    };
    let artist = unwiden(artist);
    let artist = artist
        .trim()
        .trim_end_matches("- Topic")
        .trim_end_matches("- topic")
        .trim();
    (strip_furniture(artist), strip_furniture(&unwiden(title)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalogue_entry_of_another_length_is_another_edit() {
        // The rule, as a rule: within the allowance the entry is
        // this recording, outside it the stamps belong to a
        // different edit and are worse than none. Both ends are from
        // the library, and one absolute number cannot hold them
        // both.
        assert!(duration_fits(360.0, 361.0), "a fade's difference");
        assert!(duration_fits(360.0, 348.1));
        assert!(
            duration_fits(245.0, 260.5),
            "our rip of The Bad Touch against the catalogue's twenty \
             entries: 15 s on 245 is the same recording"
        );
        assert!(
            duration_fits(285.0, 305.0),
            "Whiskey In The Jar, 20 s on 285"
        );
        assert!(
            !duration_fits(517.0, 239.0),
            "an 8:37 remix is not a 4:00 original"
        );
        assert!(
            !duration_fits(379.0, 234.0),
            "The Power Of Love's long version is not the single"
        );
        assert!(
            !duration_fits(428.0, 238.0),
            "nor a 7-minute mix a 4-minute one"
        );
        // A short song gets the absolute allowance, not a share of
        // almost nothing.
        assert!(duration_fits(60.0, 70.0));
        assert!(!duration_fits(60.0, 80.0));
        // Nothing to compare is not a match.
        assert!(!duration_fits(f64::NAN, 200.0));
        assert!(!duration_fits(200.0, 0.0));
    }

    #[test]
    fn a_downloads_furniture_comes_off_the_query_but_a_subtitle_does_not() {
        // Three songs in this library could never be found: the
        // uploader's words were part of the title, and lrclib's `get`
        // matches names closely.
        assert_eq!(
            clean_query("MANOWAR", "Warriors Of The World United - OFFICIAL VIDEO"),
            (
                "MANOWAR".to_owned(),
                "Warriors Of The World United".to_owned()
            )
        );
        assert_eq!(
            clean_query("Aerosmith - Topic", "Dream On (Official Music Video)"),
            ("Aerosmith".to_owned(), "Dream On".to_owned())
        );
        assert_eq!(
            clean_query("Ede， Deckert", "Immer"),
            ("Ede, Deckert".to_owned(), "Immer".to_owned()),
            "fullwidth punctuation comes from filename-safe renaming"
        );
        assert_eq!(clean_query("A", "B [HD]"), ("A".to_owned(), "B".to_owned()));

        // ...and what must survive, because asking for the wrong
        // recording is the mistake this whole area exists to prevent.
        assert_eq!(
            clean_query("Annie", "Two of Hearts - Skatebård Remix"),
            (
                "Annie".to_owned(),
                "Two of Hearts - Skatebård Remix".to_owned()
            )
        );
        assert_eq!(
            clean_query("Manu Chao", "Bongo Bong - Je ne t'aime plus"),
            (
                "Manu Chao".to_owned(),
                "Bongo Bong - Je ne t'aime plus".to_owned()
            )
        );
        // A title that IS the furniture word keeps it: only a
        // bracketed or trailing occurrence is furniture.
        assert_eq!(
            clean_query("Sigur Rós", "Video"),
            ("Sigur Rós".to_owned(), "Video".to_owned())
        );
    }
}
