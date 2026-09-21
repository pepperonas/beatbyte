//! The one name a song keeps.
//!
//! Everything else about a song can change and does: the file gets
//! renamed, the folder moves, the tags get fixed, the cover is
//! replaced, the chart is redesigned twice, the audio is replaced
//! with a better rip. A [`SongId`] survives all of it, because it is
//! **derived from nothing** — it is given once, at import, and after
//! that it is simply carried.
//!
//! ## Why not one of the identities that already exist
//!
//! | Candidate | Fails on |
//! |---|---|
//! | folder or file name | a rename, a move — and it is a path, which the commission rules out |
//! | `chart_hash` | every redesign; it identifies a CHART, which is the point of it |
//! | `sha256` of the audio | replacing the rip, normalising the loudness, re-encoding |
//! | title + artist | a typo fix — and it is what `scores.json` uses today, which is why renaming a song loses its records |
//!
//! Those are all good identities *of other things*, and they stay:
//! the hash identifies bytes, the chart hash identifies a chart. They
//! are attributes here, and two of them are what duplicate detection
//! is built on. None of them is the song.
//!
//! ## Shape
//!
//! `bb_` then the import time, then entropy, both base-36. The time
//! prefix means ids sort into import order, which makes a directory
//! of them readable and a range query on "added around then" cheap;
//! the entropy means two songs imported in the same millisecond do
//! not collide.

use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

/// Digits of the time part. `36^9` milliseconds is the year 3990.
const TIME_DIGITS: usize = 9;

/// Digits of the entropy part: a whole `u64`.
const ENTROPY_DIGITS: usize = 13;

/// The prefix, so an id is recognisable in a log or a file name.
const PREFIX: &str = "bb_";

/// A song's permanent internal identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SongId(String);

impl SongId {
    /// A fresh id for a song being imported now.
    ///
    /// The entropy comes from `RandomState`, which std seeds from the
    /// operating system — no crate, no global state, and no need for
    /// the caller to carry a generator around.
    #[must_use]
    pub fn new(now_ms: u64) -> SongId {
        SongId::from_parts(now_ms, RandomState::new().hash_one(now_ms))
    }

    /// The pure half, so the format can be pinned without a clock or
    /// an entropy source.
    #[must_use]
    pub fn from_parts(now_ms: u64, entropy: u64) -> SongId {
        let mut text = String::with_capacity(PREFIX.len() + TIME_DIGITS + ENTROPY_DIGITS);
        text.push_str(PREFIX);
        push_base36(&mut text, now_ms, TIME_DIGITS);
        push_base36(&mut text, entropy, ENTROPY_DIGITS);
        SongId(text)
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Read an id back, rejecting anything that is not one.
    ///
    /// Strict on purpose: an id is a key, and a key that can be
    /// "almost right" is a key that silently splits a song's history
    /// in two.
    #[must_use]
    pub fn parse(text: &str) -> Option<SongId> {
        let body = text.strip_prefix(PREFIX)?;
        if body.len() != TIME_DIGITS + ENTROPY_DIGITS {
            return None;
        }
        body.bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
            .then(|| SongId(text.to_owned()))
    }
}

impl std::fmt::Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Append `value` in base 36, zero-padded to `width`.
fn push_base36(out: &mut String, value: u64, width: usize) {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buffer = [b'0'; ENTROPY_DIGITS];
    let mut value = value;
    for slot in buffer[..width].iter_mut().rev() {
        *slot = DIGITS[usize::try_from(value % 36).unwrap_or(0)];
        value /= 36;
    }
    out.push_str(std::str::from_utf8(&buffer[..width]).unwrap_or("0"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_fixed_width_and_recognisable() {
        let id = SongId::from_parts(1_726_000_000_000, 0);
        assert!(id.as_str().starts_with(PREFIX));
        assert_eq!(
            id.as_str().len(),
            PREFIX.len() + TIME_DIGITS + ENTROPY_DIGITS
        );
        assert_eq!(SongId::parse(id.as_str()), Some(id));
    }

    #[test]
    fn ids_sort_into_import_order() {
        // The time prefix is the point: a listing of ids reads as a
        // history, and "imported around then" is a range scan.
        let early = SongId::from_parts(1_000_000_000_000, u64::MAX);
        let late = SongId::from_parts(1_000_000_000_001, 0);
        assert!(
            early < late,
            "a later import must sort after an earlier one even with \
             the luckiest entropy: {early} vs {late}"
        );
    }

    #[test]
    fn two_songs_in_one_millisecond_do_not_collide() {
        let a = SongId::from_parts(1_726_000_000_000, 1);
        let b = SongId::from_parts(1_726_000_000_000, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn a_fresh_id_is_not_the_same_as_the_next_one() {
        // The entropy really is entropy — a constant would make every
        // song imported in one millisecond the same song.
        let now = 1_726_000_000_000;
        let ids: std::collections::BTreeSet<SongId> = (0..32).map(|_| SongId::new(now)).collect();
        assert!(
            ids.len() > 1,
            "32 ids from one millisecond collapsed into one"
        );
    }

    #[test]
    fn almost_an_id_is_not_an_id() {
        assert_eq!(SongId::parse(""), None);
        assert_eq!(SongId::parse("bb_"), None);
        assert_eq!(SongId::parse("bb_tooshort"), None);
        assert_eq!(
            SongId::parse("xx_00000000000000000000ab"),
            None,
            "the prefix is part of the key"
        );
        assert_eq!(
            SongId::parse("bb_0000000000000000000AB_"),
            None,
            "an id is lowercase base36 and nothing else"
        );
        let real = SongId::from_parts(7, 7);
        assert_eq!(SongId::parse(real.as_str()).as_ref(), Some(&real));
    }

    #[test]
    fn the_whole_range_of_a_u64_fits_in_the_entropy_part() {
        let id = SongId::from_parts(0, u64::MAX);
        assert_eq!(SongId::parse(id.as_str()), Some(id.clone()));
        assert_ne!(
            id,
            SongId::from_parts(0, u64::MAX - 1),
            "the top of the range must not wrap onto its neighbour"
        );
    }
}
