//! When things happened to a song, and what may move a timestamp.
//!
//! Every time here is **Unix milliseconds UTC**, the same unit the
//! play log and the telemetry store already use. A wall-clock string
//! with a local offset is a timestamp that changes meaning when the
//! player travels, and half of what these fields are for is comparing
//! them across months.
//!
//! The semantics matter more than the fields. Two of them are easy to
//! get wrong in a way nobody notices for a year:
//!
//! - **`imported_at` is written once and never again.** A metadata
//!   refresh three months later must not make a song look new, or
//!   "songs added per month" quietly becomes "songs refreshed per
//!   month".
//! - **`updated_at` moves only when a value actually changed.**
//!   Reading a song, scanning the library, or writing the same value
//!   back are not changes. Otherwise "recently updated" means
//!   "recently looked at", which is no information at all.
//!
//! What is deliberately NOT here: `play_count`, `first_played_at`,
//! `last_played_at`. Playing is an event, and events live in the
//! telemetry store; deriving those three from sessions keeps one
//! answer instead of two that can disagree.

use serde::{Deserialize, Serialize};

/// Unix milliseconds, UTC.
pub type Millis = u64;

/// When each part of a song's life last happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lifecycle {
    /// When this song first entered the library. **Immutable.**
    pub imported_at: Millis,
    /// When any stored value last changed. See the module doc: not
    /// when the song was last read.
    pub updated_at: Millis,
    /// When descriptive or identity metadata last changed. Moves with
    /// a tag refresh, not with an analysis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_updated_at: Option<Millis>,
    /// When the audio was last analysed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_analyzed_at: Option<Millis>,
    /// When the active chart was generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_generated_at: Option<Millis>,
    /// When the lyrics or their alignment last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_updated_at: Option<Millis>,
    /// When the vocal chart was last computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocals_analyzed_at: Option<Millis>,
}

impl Lifecycle {
    /// A song entering the library for the first time.
    #[must_use]
    pub fn imported(now: Millis) -> Lifecycle {
        Lifecycle {
            imported_at: now,
            updated_at: now,
            ..Lifecycle::default()
        }
    }

    /// Record that something really changed.
    ///
    /// Takes `changed` rather than being called only on change, so
    /// that the caller may write `touch(now, wrote_anything)` at the
    /// end of a refresh and cannot forget the case where it wrote
    /// nothing. Returns whether the clock moved.
    pub fn touch(&mut self, now: Millis, changed: bool) -> bool {
        if changed {
            self.updated_at = now;
        }
        changed
    }

    /// Record that descriptive or identity metadata changed.
    pub fn touch_metadata(&mut self, now: Millis, changed: bool) -> bool {
        if changed {
            self.metadata_updated_at = Some(now);
            self.updated_at = now;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_import_moment_survives_every_later_refresh() {
        let mut life = Lifecycle::imported(1_000);
        for later in [2_000, 3_000, 4_000] {
            life.touch_metadata(later, true);
            assert_eq!(
                life.imported_at, 1_000,
                "a refresh must never make a song look newly added"
            );
        }
        assert_eq!(life.updated_at, 4_000);
        assert_eq!(life.metadata_updated_at, Some(4_000));
    }

    #[test]
    fn reading_a_song_does_not_update_it() {
        let mut life = Lifecycle::imported(1_000);
        assert!(!life.touch(9_999, false), "nothing changed");
        assert_eq!(
            life.updated_at, 1_000,
            "\"recently updated\" must not degrade into \"recently looked at\""
        );
        assert_eq!(life.metadata_updated_at, None);
    }

    #[test]
    fn an_analysis_is_not_a_metadata_change() {
        let mut life = Lifecycle::imported(1_000);
        life.audio_analyzed_at = Some(2_000);
        life.touch(2_000, true);
        assert_eq!(
            life.metadata_updated_at, None,
            "analysing the audio says nothing about the song's title"
        );
        assert_eq!(life.updated_at, 2_000);
    }

    #[test]
    fn the_stages_are_independent_of_one_another() {
        let mut life = Lifecycle::imported(1_000);
        life.chart_generated_at = Some(2_000);
        life.lyrics_updated_at = Some(3_000);
        life.vocals_analyzed_at = Some(4_000);
        assert_eq!(life.audio_analyzed_at, None, "each stage stands alone");
        assert_eq!(life.chart_generated_at, Some(2_000));
    }
}
