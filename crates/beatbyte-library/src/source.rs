//! Where a value came from, and who may replace it.
//!
//! Not every metadata field is contested. A file's size has one
//! answer and nobody argues about it; a song's genre has as many
//! answers as there are opinions, and BeatByte will collect several
//! of them over a song's life — the file's own tag at import, the
//! analysis later, a catalogue after that, and the player's own hand
//! at any point.
//!
//! So [`Sourced`] wraps **only the contested fields**. Wrapping every
//! property would turn a file size into a three-line object for no
//! gain and make the document unreadable.

use serde::{Deserialize, Serialize};

/// Who said so.
///
/// The order of the variants is the order of authority, and
/// [`MetaSource::rank`] is what a refresh consults. It is a total
/// order on purpose: a rule that needs a table per field is a rule
/// nobody can predict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaSource {
    /// Guessed from something that was never meant as metadata — a
    /// file name, a folder name. The weakest claim there is.
    Inferred,
    /// BeatByte's own analysis of the audio.
    Analyzed,
    /// Whatever the place the song came from said about it: a video
    /// title, a channel name, a download's own fields.
    Source,
    /// A tag inside the audio file.
    Embedded,
    /// A catalogue: MusicBrainz and its kind.
    External,
    /// The player, by hand. Nothing outranks this.
    User,
}

impl MetaSource {
    /// How much authority this source carries, higher is stronger.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            MetaSource::Inferred => 0,
            MetaSource::Analyzed => 1,
            MetaSource::Source => 2,
            MetaSource::Embedded => 3,
            MetaSource::External => 4,
            MetaSource::User => 5,
        }
    }

    /// Whether this source estimates rather than states.
    ///
    /// Only an estimate may carry a confidence: a tag that says
    /// `2018` is not 82 % sure of anything, and inventing a number
    /// for it would make the field meaningless where it matters.
    #[must_use]
    pub fn estimates(self) -> bool {
        matches!(
            self,
            MetaSource::Analyzed | MetaSource::Inferred | MetaSource::External
        )
    }
}

/// A value together with who said it, and how sure they were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sourced<T> {
    /// The value itself.
    pub value: T,
    /// Who said so.
    pub source: MetaSource,
    /// How sure, `0.0`–`1.0` — and **`None` unless the source really
    /// estimates**. See [`MetaSource::estimates`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl<T> Sourced<T> {
    /// A value somebody simply stated: a tag, a catalogue, a person.
    /// Carries no confidence, because there is none to carry.
    #[must_use]
    pub fn stated(value: T, source: MetaSource) -> Sourced<T> {
        Sourced {
            value,
            source,
            confidence: None,
        }
    }

    /// A value BeatByte worked out, with how sure it is. A confidence
    /// outside `0.0..=1.0` — or one that is not a number — is dropped
    /// rather than stored: a nonsense number reads as a real one.
    #[must_use]
    pub fn analyzed(value: T, confidence: f32) -> Sourced<T> {
        Sourced::estimated(value, MetaSource::Analyzed, confidence)
    }

    /// A value from a source that ESTIMATES, with how sure it is.
    ///
    /// The rule is structural rather than remembered: a source that
    /// states a fact cannot carry a confidence through here at all.
    ///
    /// ⚠️ A catalogue counts as estimating, and that is a considered
    /// change from the first version of this rule. MusicBrainz
    /// states a fact about ITS recording; what is uncertain is that
    /// its recording is ours — asked for David Bowie's "Heroes" it
    /// returns eight, **all scored 100**, from 0 to 393 seconds. The
    /// uncertainty belongs on the field that came out of that
    /// choice, and hiding it would make a guess read as a fact.
    #[must_use]
    pub fn estimated(value: T, source: MetaSource, confidence: f32) -> Sourced<T> {
        Sourced {
            value,
            source,
            confidence: (source.estimates() && (0.0..=1.0).contains(&confidence))
                .then_some(confidence),
        }
    }

    /// The value the player typed. Always wins a refresh; the caller
    /// must also record the field in [`SongDoc::overrides`].
    ///
    /// [`SongDoc::overrides`]: crate::SongDoc::overrides
    #[must_use]
    pub fn by_user(value: T) -> Sourced<T> {
        Sourced {
            value,
            source: MetaSource::User,
            confidence: None,
        }
    }
}

/// Whether an automatic refresh may write `incoming` over `current`.
///
/// Three rules, in this order:
///
/// 1. **A field the player has edited is closed.** `overridden` comes
///    from [`SongDoc::overrides`], and no source — not even a better
///    one — reopens it. Only the player can, by editing again.
/// 2. **An empty field takes anything.** There is nothing to lose.
/// 3. **Otherwise the stronger source wins**, and an equally strong
///    one also writes: a second answer from the same catalogue is a
///    newer answer, not a worse one.
///
/// Pure — tested.
///
/// [`SongDoc::overrides`]: crate::SongDoc::overrides
#[must_use]
pub fn may_replace(current: Option<MetaSource>, incoming: MetaSource, overridden: bool) -> bool {
    if overridden {
        return false;
    }
    match current {
        None => true,
        Some(have) => incoming.rank() >= have.rank(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_players_hand_outranks_every_machine() {
        for source in [
            MetaSource::Inferred,
            MetaSource::Analyzed,
            MetaSource::Source,
            MetaSource::Embedded,
            MetaSource::External,
        ] {
            assert!(
                source.rank() < MetaSource::User.rank(),
                "{source:?} must not outrank the player"
            );
        }
    }

    #[test]
    fn a_refresh_may_not_reopen_a_field_the_player_edited() {
        // The example from the commission: the analyser says House,
        // the player says Deep House, a refresh runs. Deep House
        // stays — even though the catalogue outranks the analyser.
        assert!(!may_replace(
            Some(MetaSource::User),
            MetaSource::External,
            true
        ));
        assert!(
            !may_replace(Some(MetaSource::Analyzed), MetaSource::External, true),
            "the override flag closes the field whatever is recorded in it"
        );
    }

    #[test]
    fn an_empty_field_takes_the_first_thing_offered() {
        assert!(may_replace(None, MetaSource::Inferred, false));
        assert!(
            !may_replace(None, MetaSource::External, true),
            "…unless the player has said they want it empty"
        );
    }

    #[test]
    fn the_stronger_source_wins_and_an_equal_one_refreshes() {
        assert!(may_replace(
            Some(MetaSource::Analyzed),
            MetaSource::Embedded,
            false
        ));
        assert!(
            !may_replace(Some(MetaSource::Embedded), MetaSource::Analyzed, false),
            "our own guess must not overwrite the file's own tag"
        );
        assert!(
            may_replace(Some(MetaSource::External), MetaSource::External, false),
            "a second answer from the same catalogue is a newer answer"
        );
    }

    #[test]
    fn only_an_estimate_carries_a_confidence() {
        // A value simply STATED never carries one, whoever states
        // it: a tag that says 2018 is not 82 % sure of anything.
        assert_eq!(
            Sourced::stated(2018u32, MetaSource::External).confidence,
            None
        );
        assert_eq!(
            Sourced::stated(2018u32, MetaSource::Embedded).confidence,
            None
        );
        assert_eq!(
            Sourced::analyzed("Deep House".to_owned(), 0.82).confidence,
            Some(0.82)
        );
        // ⚠️ And the rule is structural: a source that does not
        // estimate cannot smuggle a confidence through `estimated`
        // either.
        assert_eq!(
            Sourced::estimated(2018u32, MetaSource::Embedded, 0.82).confidence,
            None
        );
        assert!(MetaSource::Analyzed.estimates() && MetaSource::Inferred.estimates());
        assert!(!MetaSource::Embedded.estimates() && !MetaSource::User.estimates());
        assert!(!MetaSource::Source.estimates());
    }

    #[test]
    fn a_catalogue_match_is_an_estimate_and_may_say_how_sure_it_is() {
        // ⚠️ A considered change from the first version of this rule.
        // MusicBrainz states a fact about ITS recording; what is
        // uncertain is that its recording is ours — asked for David
        // Bowie's "Heroes" it returns eight, all scored 100, running
        // from 0 to 393 seconds. That uncertainty belongs on the
        // field the choice produced, and hiding it would make a
        // guess read as a fact.
        assert!(MetaSource::External.estimates());
        let year = Sourced::estimated(1977u32, MetaSource::External, 0.93);
        assert_eq!(year.confidence, Some(0.93));
        assert_eq!(year.source, MetaSource::External);
        // …and the player still outranks it.
        assert!(!may_replace(
            Some(MetaSource::User),
            MetaSource::External,
            false
        ));
    }

    #[test]
    fn a_nonsense_confidence_is_dropped_rather_than_stored() {
        // A number outside the range reads exactly as authoritative
        // as a real one, which is the whole danger.
        assert_eq!(Sourced::analyzed(1u8, 1.7).confidence, None);
        assert_eq!(Sourced::analyzed(1u8, -0.2).confidence, None);
        assert_eq!(Sourced::analyzed(1u8, f32::NAN).confidence, None);
        assert_eq!(Sourced::analyzed(1u8, 0.0).confidence, Some(0.0));
        assert_eq!(Sourced::analyzed(1u8, 1.0).confidence, Some(1.0));
    }
}
