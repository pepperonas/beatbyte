//! Reading a catalogue's answer, and choosing from it.
//!
//! Pure: a string of JSON in, a decision out. Nothing here knows
//! that a network exists.

use serde::Deserialize;

/// One recording as the catalogue describes it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Recording {
    /// The catalogue's own identifier.
    pub id: String,
    /// How well IT thinks the query matched, 0–100.
    ///
    /// ⚠️ Nearly useless on its own: a search for a well-known song
    /// returns a page of hundreds, all scored 100.
    #[serde(default)]
    pub score: u32,
    /// The recording's title.
    pub title: String,
    /// Its length in milliseconds, when the catalogue knows one.
    #[serde(default)]
    pub length: Option<u64>,
    /// What the catalogue says this recording is — "live, 1997…",
    /// "demo", "instrumental". Present only when it needs saying.
    #[serde(default)]
    pub disambiguation: Option<String>,
    /// Who it is credited to.
    #[serde(default, rename = "artist-credit")]
    pub artists: Vec<ArtistCredit>,
    /// The releases it appears on.
    #[serde(default)]
    pub releases: Vec<Release>,
}

/// One name in a recording's credit.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArtistCredit {
    /// The name as credited.
    pub name: String,
}

/// One release a recording appears on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Release {
    /// Its title — the album, usually.
    pub title: String,
    /// Its release date, `YYYY` or `YYYY-MM-DD`. Often empty.
    #[serde(default)]
    pub date: Option<String>,
    /// What kind of release it is.
    #[serde(default, rename = "release-group")]
    pub group: Option<ReleaseGroup>,
}

/// What kind of thing a release is.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseGroup {
    /// Album, Single, EP, Broadcast…
    #[serde(default, rename = "primary-type")]
    pub primary: Option<String>,
    /// Compilation, Live, Soundtrack, DJ-mix…
    #[serde(default, rename = "secondary-types")]
    pub secondary: Vec<String>,
}

/// What is left of a match's confidence when it came from the
/// fallback tier: a labelled recording, reached because nothing
/// unlabelled fitted.
pub const FALLBACK_CONFIDENCE: f32 = 0.4;

/// The recording that was chosen, and how sure that is.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// The recording.
    pub recording: Recording,
    /// How well it fits, 0–1: 1 when its length is exactly the
    /// song's, falling to 0 at the edge of what the length rule
    /// allows. It is a MEASURE of the only evidence that actually
    /// discriminated, not a restatement of the catalogue's score.
    pub confidence: f32,
}

impl Recording {
    /// Its length in seconds, when it has one.
    #[must_use]
    pub fn length_s(&self) -> Option<f64> {
        self.length
            .filter(|ms| *ms > 0)
            .map(|ms| ms as f64 / 1000.0)
    }

    /// The credited artist, all names joined as the catalogue joins
    /// them.
    #[must_use]
    pub fn artist(&self) -> String {
        self.artists
            .iter()
            .map(|credit| credit.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Whether the catalogue itself says this is not the plain
    /// studio recording.
    #[must_use]
    pub fn is_a_variant(&self) -> bool {
        const WORDS: [&str; 6] = ["live", "demo", "remix", "karaoke", "instrumental", "edit"];
        self.disambiguation.as_deref().is_some_and(|text| {
            let text = text.to_lowercase();
            WORDS.iter().any(|word| text.contains(word))
        })
    }
}

/// Read a search response.
///
/// A response that cannot be parsed yields nothing rather than an
/// error: the caller's next move is the same either way, and a
/// catalogue changing a field it does not document must not stop a
/// library from being enriched.
#[must_use]
pub fn parse(body: &str) -> Vec<Recording> {
    #[derive(Deserialize)]
    struct Response {
        #[serde(default)]
        recordings: Vec<Recording>,
    }
    serde_json::from_str::<Response>(body)
        .map(|response| response.recordings)
        .unwrap_or_default()
}

/// Choose the recording that is this song, if any is.
///
/// ⚠️ **Without our own length, nothing is chosen.** The catalogue's
/// score does not distinguish an album version from a live take, so
/// with no length there is no evidence — and a guess written into a
/// document is worse than a gap, because it looks like knowledge.
#[must_use]
pub fn pick(ours_s: Option<f64>, candidates: &[Recording]) -> Option<Match> {
    let ours = ours_s.filter(|value| value.is_finite() && *value > 0.0)?;
    let allowance = beatbyte_chart::catalogue::DURATION_TOLERANCE_S
        .max(ours * beatbyte_chart::catalogue::DURATION_TOLERANCE_SHARE);
    // ⚠️ A TIER, not a penalty. Asked for Bruce Springsteen's "Born
    // to Run", the catalogue's first twelve answers are twelve live
    // takes and the studio recording is not among them; over a
    // hundred, ten of 2338 carry no label at all and one of those is
    // the 4:31 everybody means. A soft demotion still picks a live
    // take whose length happens to sit closer. So: the unlabelled
    // ones are considered first, and a labelled one is reached only
    // when none of them fits — a song really can BE the live take,
    // and then it is the only thing there is.
    let closest = |only_plain: bool| {
        let mut best: Option<(f64, &Recording)> = None;
        for candidate in candidates {
            if only_plain && candidate.is_a_variant() {
                continue;
            }
            let Some(theirs) = candidate.length_s() else {
                continue;
            };
            if !beatbyte_chart::catalogue::duration_fits(ours, theirs) {
                continue;
            }
            let distance = (ours - theirs).abs();
            // Only as a tie-break, and only among entries the length
            // already accepted.
            if best.is_none_or(|(held, holder)| {
                (distance, u32::MAX - candidate.score) < (held, u32::MAX - holder.score)
            }) {
                best = Some((distance, candidate));
            }
        }
        best
    };
    let (plain, (distance, recording)) = match closest(true) {
        Some(found) => (true, found),
        None => (false, closest(false)?),
    };
    // A fallback pick is a labelled recording — a live take, a demo,
    // a remix — reached because nothing unlabelled fitted. Its
    // length may be very close and it can still be the wrong thing,
    // so the confidence says which tier it came from rather than
    // only how close it was.
    let closeness = (1.0 - (distance / allowance)).clamp(0.0, 1.0) as f32;
    let confidence = if plain {
        closeness
    } else {
        closeness * FALLBACK_CONFIDENCE
    };
    Some(Match {
        recording: recording.clone(),
        confidence,
    })
}

/// Choose the release to call this recording's album.
///
/// ⚠️ Not `releases[0]`. The single edit of "Heroes" lists a DJ
/// compilation first, undated — recording that as the album would be
/// worse than recording nothing. A release must be dated to be
/// considered at all, a compilation or a DJ mix loses to anything
/// else, and among equals the EARLIEST wins, because that is the one
/// the song came out on.
#[must_use]
pub fn pick_release(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|release| release_year(release).is_some())
        .min_by_key(|release| {
            let secondary = release
                .group
                .as_ref()
                .map(|group| group.secondary.as_slice())
                .unwrap_or_default();
            let repackaged = secondary.iter().any(|kind| {
                matches!(
                    kind.to_lowercase().as_str(),
                    "compilation" | "dj-mix" | "live" | "mixtape/street" | "remix"
                )
            });
            (repackaged, release_year(release).unwrap_or(u16::MAX))
        })
}

/// The year a release states, when it states one that could be a
/// year.
#[must_use]
pub fn release_year(release: &Release) -> Option<u16> {
    release
        .date
        .as_deref()
        .map(str::trim)
        .filter(|date| !date.is_empty())
        .and_then(|date| date.get(..4))
        .and_then(|head| head.parse::<i64>().ok())
        .and_then(|year| (1860..=2200).contains(&year).then_some(year as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real response, recorded — eight recordings, every one of
    /// them scored 100.
    fn heroes() -> Vec<Recording> {
        parse(include_str!("../tests/fixtures/bowie-heroes.json"))
    }

    #[test]
    fn a_real_response_reads_as_what_it_is() {
        let found = heroes();
        assert_eq!(found.len(), 8);
        assert!(
            found.iter().all(|r| r.score == 100),
            "the score is the same for all of them, which is the point"
        );
        assert_eq!(found[0].artist(), "David Bowie");
    }

    #[test]
    fn the_songs_own_length_picks_the_edit_the_score_cannot() {
        // Our file is 3:29. Among eight equally scored candidates
        // running from 0 to 393 seconds, exactly one is that edit.
        let chosen = pick(Some(209.0), &heroes()).expect("a match");
        assert_eq!(chosen.recording.length_s(), Some(210.973));
        assert!(chosen.confidence > 0.8, "{}", chosen.confidence);
    }

    #[test]
    fn a_song_of_another_length_matches_nothing_rather_than_the_nearest() {
        // 2:30 is no edit in this response: the nearest is 211 s,
        // sixty-one seconds away, and the rule refuses it. Refusing
        // is the whole job.
        assert_eq!(pick(Some(150.0), &heroes()), None);
    }

    #[test]
    fn a_thin_match_says_it_is_thin() {
        // At 5:00 the 307-second live take really IS inside the
        // allowance — eight per cent of five minutes is twenty-four
        // seconds. The rule accepts it, and the confidence says how
        // little that is worth: a caller can refuse on the number
        // without the rule having to pretend it found nothing.
        let chosen = pick(Some(300.0), &heroes()).expect("a match inside the allowance");
        assert_eq!(chosen.recording.length_s(), Some(307.0));
        assert!(chosen.recording.is_a_variant(), "it is a live take");
        assert!(
            chosen.confidence < 0.3,
            "reached only because nothing unlabelled fitted, and it says so: {}",
            chosen.confidence
        );
    }

    #[test]
    fn without_our_own_length_nothing_is_chosen() {
        // ⚠️ The score cannot tell an album version from a live take,
        // so with no length there is no evidence at all — and a guess
        // in a document looks exactly like knowledge.
        assert_eq!(pick(None, &heroes()), None);
        assert_eq!(pick(Some(0.0), &heroes()), None);
        assert_eq!(pick(Some(f64::NAN), &heroes()), None);
    }

    #[test]
    fn a_live_take_loses_to_a_studio_one_of_the_same_length() {
        let studio = Recording {
            id: "a".to_owned(),
            score: 90,
            title: "Heroes".to_owned(),
            length: Some(209_000),
            disambiguation: None,
            artists: vec![],
            releases: vec![],
        };
        let live = Recording {
            id: "b".to_owned(),
            score: 100,
            disambiguation: Some("live, 1990-08-05: Milton Keynes".to_owned()),
            ..studio.clone()
        };
        let chosen = pick(Some(209.0), &[live.clone(), studio.clone()]).expect("a match");
        assert_eq!(
            chosen.recording.id, "a",
            "the higher score is not the point"
        );
        // …but a song that really IS the live take still finds it,
        // because length is what decides and nothing else is there.
        assert_eq!(
            pick(Some(209.0), &[live]).expect("a match").recording.id,
            "b"
        );
    }

    #[test]
    fn the_album_is_not_whatever_release_came_first_in_the_list() {
        // The single edit's first listed release is an undated DJ
        // compilation. Recording that as the album would be worse
        // than recording nothing.
        let chosen = pick(Some(209.0), &heroes()).expect("a match");
        let release = pick_release(&chosen.recording.releases);
        assert!(
            release.is_none_or(|release| !release.title.contains("Mastermix")),
            "{release:?}"
        );
    }

    #[test]
    fn a_dated_original_beats_a_dated_compilation() {
        let compilation = Release {
            title: "Greatest Hits".to_owned(),
            date: Some("1990".to_owned()),
            group: Some(ReleaseGroup {
                primary: Some("Album".to_owned()),
                secondary: vec!["Compilation".to_owned()],
            }),
        };
        let original = Release {
            title: "\u{201c}Heroes\u{201d}".to_owned(),
            date: Some("1977-10-14".to_owned()),
            group: Some(ReleaseGroup {
                primary: Some("Album".to_owned()),
                secondary: vec![],
            }),
        };
        let undated = Release {
            title: "Some Reissue".to_owned(),
            date: Some(String::new()),
            group: None,
        };
        let releases = vec![undated, compilation, original.clone()];
        assert_eq!(pick_release(&releases), Some(&original));
        assert_eq!(release_year(&original), Some(1977));
    }

    #[test]
    fn an_unlabelled_recording_wins_even_when_a_live_take_is_closer() {
        // ⚠️ The case a soft penalty got wrong. Asked for "Born to
        // Run", the catalogue's first twelve answers are twelve live
        // takes; the 4:31 everybody means is one of ten unlabelled
        // recordings among 2338. A live take whose length happens to
        // sit nearer must not beat it.
        let live = Recording {
            id: "live".to_owned(),
            score: 100,
            title: "Born to Run".to_owned(),
            length: Some(266_000),
            disambiguation: Some("live, 1975-10-18: The Roxy".to_owned()),
            artists: vec![],
            releases: vec![],
        };
        let studio = Recording {
            id: "studio".to_owned(),
            length: Some(271_000),
            disambiguation: None,
            ..live.clone()
        };
        let chosen = pick(Some(265.0), &[live.clone(), studio]).expect("a match");
        assert_eq!(
            chosen.recording.id, "studio",
            "one second nearer is not the point"
        );

        // …and with nothing unlabelled that fits, the live take is
        // what there is — at a confidence that says so.
        let only_live = pick(Some(265.0), &[live]).expect("a match");
        assert_eq!(only_live.recording.id, "live");
        assert!(
            only_live.confidence <= FALLBACK_CONFIDENCE,
            "{}",
            only_live.confidence
        );
    }

    #[test]
    fn a_response_that_cannot_be_read_yields_nothing_rather_than_an_error() {
        assert!(parse("{ not json").is_empty());
        assert!(parse("{}").is_empty());
    }
}
