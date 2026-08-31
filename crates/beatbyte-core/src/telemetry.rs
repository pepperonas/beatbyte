//! The session-telemetry schema — layer 1 of adaptive charting
//! (ADR-0011).
//!
//! This is the one implementation of the format: the game writes it,
//! the CLI reads it, and neither may grow a private copy (the
//! mechanics reference's shared-library rule). It lives in core
//! because it serializes core's own session vocabulary — judgments
//! and offsets — and core is the crate both sides already share.
//!
//! A session file is JSONL: one [`SessionHeader`] line, then one
//! [`NoteLine`] per observation. Readers skip lines they do not
//! understand; a missing header rejects the file, because
//! observations that bind to nothing are worse than no file.

use serde::{Deserialize, Serialize};

use crate::timing::Judgment;

/// Version of the on-disk schema. Bump when a line's meaning changes;
/// adding a new line kind or an optional field does not require it
/// (readers skip what they do not understand).
pub const SCHEMA_VERSION: u32 = 1;

/// The first line of a session file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionHeader {
    /// [`SCHEMA_VERSION`] at the time of writing.
    pub schema: u32,
    /// Song title — its own field, NOT joined with the artist into
    /// one string. The score board's `title|artist` key is a known
    /// collision (roadmap C5); a new schema must not copy it.
    pub title: String,
    /// Song artist.
    pub artist: String,
    /// Difficulty, lowercase display name.
    pub difficulty: String,
    /// Content hash of the exact chart that was played.
    pub chart_hash: String,
    /// The game version that recorded this.
    pub generator: String,
    /// Unix milliseconds when the session began.
    pub started_ms: u64,
    /// Player index (solo = 0).
    pub player: usize,
    /// Whether the autopilot was driving. Autopilot plays perfectly;
    /// evidence readers must be able to exclude it, or every chart
    /// looks too easy.
    pub autopilot: bool,
    /// Note events in the played track. Completion is derived, not
    /// stored: a session is complete when every event was judged —
    /// storing a second flag would just invite disagreement.
    pub notes_total: usize,
}

/// One recorded observation. Serialized untagged: the field sets are
/// disjoint, so each line stays as small as the spec sketches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NoteLine {
    /// A judged hit: event index, judgment, signed offset in ms.
    Hit {
        /// Index into the track's events.
        i: usize,
        /// Judgment label (`perfect` / `great` / `good`).
        j: String,
        /// Signed `hit - note` offset in milliseconds.
        off_ms: f64,
    },
    /// A missed note event.
    Miss {
        /// Index into the track's events.
        i: usize,
        /// Always `"miss"` — kept so a session file greps uniformly
        /// by judgment.
        j: String,
    },
    /// A sustain ended: played out (`done: true`) or dropped early.
    /// Dropped holds are the evidence that separates "too hard" from
    /// "too easy" — a judgment line alone cannot show them.
    Sustain {
        /// Index into the track's events.
        s: usize,
        /// Whether it was held to (or into the grace period of) its
        /// end.
        done: bool,
    },
    /// An overstrum. Counted apart from note accuracy on purpose: it
    /// breaks the streak but is not a missed note.
    Overstrum {
        /// Always `1`; the field is what makes the line parseable.
        o: u32,
        /// The most recently judged event index when it happened —
        /// the session engine does not position an overstrum, so this
        /// is how analytics localize one to a passage. Optional:
        /// absent before the first note, and absent in files written
        /// before the field existed.
        #[serde(skip_serializing_if = "Option::is_none")]
        near: Option<usize>,
    },
    /// The player's one-key fun rating from the results screen
    /// (ADR-0011 A5) — the smallest honest human signal, and the one
    /// thing telemetry cannot derive. Appended after the session is
    /// written; when several appear, the LAST one is the player's
    /// word (they changed their mind).
    Fun {
        /// 1 (no fun) … 5 (loved it).
        fun: u8,
    },
    /// The pairwise verdict on a designed chart version: did THIS
    /// version feel better or worse than the one it was derived
    /// from? Only offered when the played chart carries provenance.
    /// Like [`NoteLine::Fun`], the last line wins.
    Versus {
        /// `"better"` or `"worse"` — this version against its parent.
        versus: String,
        /// [`crate::telemetry`]-external: the parent version's chart
        /// hash (from the played chart's provenance), so the verdict
        /// names BOTH sides even if the pointer moves later.
        parent: String,
    },
}

/// The stable label a judgment is recorded under.
#[must_use]
pub fn judgment_label(judgment: Judgment) -> &'static str {
    match judgment {
        Judgment::Perfect => "perfect",
        Judgment::Great => "great",
        Judgment::Good => "good",
        Judgment::Miss => "miss",
    }
}

/// Render a full session as JSONL.
#[must_use]
pub fn render_session(header: &SessionHeader, lines: &[NoteLine]) -> String {
    let mut out = String::new();
    if let Ok(head) = serde_json::to_string(header) {
        out.push_str(&head);
        out.push('\n');
    }
    for line in lines {
        if let Ok(text) = serde_json::to_string(line) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

/// Parse a session file. Unknown or malformed note lines are skipped
/// — a reader from schema v1 must survive a file that a later version
/// wrote — but a missing or unreadable header is a `None`.
#[must_use]
pub fn parse_session(text: &str) -> Option<(SessionHeader, Vec<NoteLine>)> {
    let mut lines = text.lines();
    let header: SessionHeader = serde_json::from_str(lines.next()?).ok()?;
    let notes = lines
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Some((header, notes))
}

/// How many note events these lines judged (hits + misses). A session
/// is complete when this equals the header's `notes_total` — derived,
/// never stored, so the two cannot disagree.
#[must_use]
pub fn judged_events(lines: &[NoteLine]) -> usize {
    lines
        .iter()
        .filter(|line| matches!(line, NoteLine::Hit { .. } | NoteLine::Miss { .. }))
        .count()
}

/// The most recently judged event index in these lines — what an
/// overstrum's `near` field is filled from.
#[must_use]
pub fn nearest_judged_index(lines: &[NoteLine]) -> Option<usize> {
    lines.iter().rev().find_map(|line| match line {
        NoteLine::Hit { i, .. } | NoteLine::Miss { i, .. } => Some(*i),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> SessionHeader {
        SessionHeader {
            schema: SCHEMA_VERSION,
            title: "Maria".to_owned(),
            artist: "Blondie".to_owned(),
            difficulty: "medium".to_owned(),
            chart_hash: "abc".to_owned(),
            generator: "0.0.0".to_owned(),
            started_ms: 1_756_500_000_000,
            player: 0,
            autopilot: false,
            notes_total: 3,
        }
    }

    #[test]
    fn a_session_round_trips_through_its_file_form() {
        let lines = vec![
            NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: -12.3,
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
            NoteLine::Sustain { s: 0, done: false },
            NoteLine::Overstrum {
                o: 1,
                near: Some(1),
            },
        ];
        let text = render_session(&header(), &lines);
        let (parsed_header, parsed_lines) =
            parse_session(&text).expect("a rendered session parses");
        assert_eq!(parsed_header, header());
        assert_eq!(parsed_lines, lines);
    }

    #[test]
    fn every_line_kind_survives_untagged_serde() {
        // Untagged serde picks the FIRST variant whose fields match,
        // so a hit without its offset would quietly become a miss.
        // Each kind is proven to come back as itself.
        for line in [
            NoteLine::Hit {
                i: 7,
                j: "great".to_owned(),
                off_ms: 4.0,
            },
            NoteLine::Miss {
                i: 8,
                j: "miss".to_owned(),
            },
            NoteLine::Sustain { s: 7, done: true },
            NoteLine::Overstrum { o: 1, near: None },
            NoteLine::Overstrum {
                o: 1,
                near: Some(9),
            },
            NoteLine::Fun { fun: 4 },
            NoteLine::Versus {
                versus: "better".to_owned(),
                parent: "abcd1234abcd1234".to_owned(),
            },
        ] {
            let text = serde_json::to_string(&line).expect("serializes");
            let back: NoteLine = serde_json::from_str(&text).expect("parses");
            assert_eq!(back, line, "{text} came back as something else");
        }
    }

    #[test]
    fn an_overstrum_without_near_still_parses() {
        // Files written before the field existed carry {"o":1} — the
        // schema promise is that optional additions never orphan old
        // files.
        let line: NoteLine = serde_json::from_str("{\"o\":1}").expect("parses");
        assert_eq!(line, NoteLine::Overstrum { o: 1, near: None });
    }

    #[test]
    fn a_reader_skips_lines_it_does_not_understand() {
        let mut text = render_session(
            &header(),
            &[NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: 1.0,
            }],
        );
        text.push_str("{\"future_kind\": {\"x\": 1}}\n");
        text.push_str("not json at all\n");
        let (_, lines) = parse_session(&text).expect("still parses");
        assert_eq!(lines.len(), 1, "unknown lines should be skipped, not kept");
    }

    #[test]
    fn a_file_without_a_header_is_rejected() {
        assert!(parse_session("").is_none());
        assert!(parse_session("{\"i\":0,\"j\":\"miss\"}\n").is_none());
    }

    #[test]
    fn completion_is_judged_events_against_the_total() {
        let lines = vec![
            NoteLine::Hit {
                i: 0,
                j: "perfect".to_owned(),
                off_ms: 0.0,
            },
            NoteLine::Sustain { s: 0, done: true },
            NoteLine::Overstrum {
                o: 1,
                near: Some(0),
            },
            NoteLine::Miss {
                i: 1,
                j: "miss".to_owned(),
            },
        ];
        // Sustains and overstrums are observations about HOW events
        // were played; only hits and misses say THAT one was judged.
        assert_eq!(judged_events(&lines), 2);
    }

    #[test]
    fn an_overstrum_is_localized_by_the_last_judged_event() {
        let mut lines = vec![NoteLine::Hit {
            i: 5,
            j: "perfect".to_owned(),
            off_ms: 0.0,
        }];
        assert_eq!(nearest_judged_index(&lines), Some(5));
        // A sustain ending later must not shadow the judged index.
        lines.push(NoteLine::Sustain { s: 5, done: true });
        assert_eq!(nearest_judged_index(&lines), Some(5));
        // And before any note there is nothing to point at.
        assert_eq!(nearest_judged_index(&[]), None);
    }

    #[test]
    fn every_judgment_has_a_stable_label() {
        assert_eq!(judgment_label(Judgment::Perfect), "perfect");
        assert_eq!(judgment_label(Judgment::Great), "great");
        assert_eq!(judgment_label(Judgment::Good), "good");
        assert_eq!(judgment_label(Judgment::Miss), "miss");
    }
}
