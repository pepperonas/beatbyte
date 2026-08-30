//! `beatbyte-cli review` — layer 2 of adaptive charting (ADR-0011).
//!
//! Reads the telemetry sessions for one (song, difficulty, chart
//! version), joins them with the chart, and answers *where* a chart
//! fails or bores rather than just how well it went. When enough
//! evidence has accumulated it emits generation directives — the
//! machine-readable half of "what should a redesign change".
//!
//! Everything in here is pure over parsed data; the only IO lives in
//! `main.rs`. Thresholds are sized for a household, not a population:
//! a single bad run changes nothing.

use std::collections::BTreeMap;

use beatbyte_core::Track;
use beatbyte_core::telemetry::{NoteLine, SessionHeader, judged_events};
use serde::{Deserialize, Serialize};

/// Everything the analysis is allowed to conclude from, in one place.
#[derive(Debug, Clone)]
pub struct Thresholds {
    /// Sessions of the same chart version before any directive.
    pub min_sessions: usize,
    /// Section accuracy below this is a problem.
    pub low_accuracy: f64,
    /// Timing spread (std dev, ms) above this is sloppy even when the
    /// notes land.
    pub sloppy_stddev_ms: f64,
    /// Sustain-drop share at or above this is a problem.
    pub dropped_sustain_rate: f64,
    /// Whole-chart accuracy at or above this, with tight timing, is
    /// the boredom signal.
    pub mastered_accuracy: f64,
    /// Timing spread below this counts as tight.
    pub mastered_stddev_ms: f64,
    /// Bars per report section.
    pub bars_per_section: u32,
    /// Judged samples a section needs before it may be called a
    /// problem — three events seen once each is an anecdote.
    pub min_section_samples: usize,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds {
            min_sessions: 3,
            low_accuracy: 0.75,
            sloppy_stddev_ms: 45.0,
            dropped_sustain_rate: 0.5,
            mastered_accuracy: 0.995,
            mastered_stddev_ms: 15.0,
            bars_per_section: 4,
            min_section_samples: 6,
        }
    }
}

/// One parsed session, already filtered to the song under review.
#[derive(Debug, Clone)]
pub struct Session {
    /// Its header.
    pub header: SessionHeader,
    /// Its observations.
    pub lines: Vec<NoteLine>,
}

/// What one section of the chart looks like across all sessions.
#[derive(Debug, Clone, Serialize)]
pub struct SectionReport {
    /// First bar of the section (0-based).
    pub bar_start: u32,
    /// One past the last bar.
    pub bar_end: u32,
    /// Song-time range in seconds.
    pub time_s: (f64, f64),
    /// Judged samples (hits + misses) across sessions.
    pub judged: usize,
    /// Hits / judged.
    pub accuracy: f64,
    /// Mean signed offset in ms (negative = early).
    pub mean_off_ms: f64,
    /// Offset spread in ms.
    pub stddev_ms: f64,
    /// Sustain endings seen / of them dropped.
    pub sustains: (usize, usize),
    /// Overstrums localized to this section.
    pub overstrums: usize,
}

/// The machine-readable half of "what should a redesign change".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Directive {
    /// Song title.
    pub title: String,
    /// Song artist.
    pub artist: String,
    /// Difficulty under review.
    pub difficulty: String,
    /// The chart version this evidence binds to.
    pub chart_hash: String,
    /// Bar range `[start, end)`; the whole chart when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bars: Option<(u32, u32)>,
    /// What is wrong (or too right).
    pub problem: String,
    /// The numbers behind the diagnosis.
    pub evidence: Evidence,
    /// What a redesign should try.
    pub recommend: Vec<String>,
    /// What it must not break.
    pub constraints: Vec<String>,
}

/// The numbers a directive stands on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// Sessions of this chart version that fed the diagnosis.
    pub sessions: usize,
    /// Accuracy over the diagnosed range.
    pub accuracy: f64,
    /// Timing spread over the diagnosed range, ms.
    pub stddev_ms: f64,
    /// Dropped / total sustain endings in the range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_sustains: Option<(usize, usize)>,
}

/// The full outcome of a review.
#[derive(Debug, Clone)]
pub struct Review {
    /// Sessions that matched the current chart version.
    pub sessions_used: usize,
    /// Sessions excluded because they were played on another version
    /// of this chart — reported, never silently dropped.
    pub stale_sessions: usize,
    /// Sessions excluded because the autopilot played them.
    pub autopilot_sessions: usize,
    /// Per-section aggregation, ascending by bar.
    pub sections: Vec<SectionReport>,
    /// Directives, when the evidence clears the thresholds.
    pub directives: Vec<Directive>,
}

/// Seconds per bar, from the chart's tempo (4/4 assumed, as
/// everywhere else in the generator).
fn bar_length_s(bpm: f64) -> f64 {
    240.0 / bpm.clamp(20.0, 400.0)
}

/// Which section an event time falls into.
fn section_of(time_s: f64, offset_s: f64, bpm: f64, bars_per_section: u32) -> u32 {
    let bar = ((time_s - offset_s) / bar_length_s(bpm)).max(0.0) as u32;
    bar / bars_per_section.max(1)
}

/// Population standard deviation; 0 for fewer than two samples.
fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance =
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / values.len() as f64;
    variance.sqrt()
}

/// Aggregation scratch for one section.
#[derive(Default)]
struct SectionScratch {
    hits: usize,
    misses: usize,
    offsets: Vec<f64>,
    sustains_seen: usize,
    sustains_dropped: usize,
    overstrums: usize,
}

/// Run the review over sessions that all belong to one (song,
/// difficulty). The caller has already read the files; this decides.
#[must_use]
pub fn review(
    track: &Track,
    bpm: f64,
    offset_s: f64,
    current_hash: &str,
    sessions: &[Session],
    include_autopilot: bool,
    thresholds: &Thresholds,
) -> Review {
    let mut stale = 0usize;
    let mut piloted = 0usize;
    let mut used: Vec<&Session> = Vec::new();
    for session in sessions {
        if session.header.chart_hash != current_hash {
            stale += 1;
        } else if session.header.autopilot && !include_autopilot {
            piloted += 1;
        } else {
            used.push(session);
        }
    }

    // Event index -> (time, is_sustain), from the same track the game
    // played. Telemetry indexes EVENTS (chords merged), so the chart's
    // note list would be the wrong denominator.
    let events = track.events();

    let mut scratch: BTreeMap<u32, SectionScratch> = BTreeMap::new();
    let mut all_offsets: Vec<f64> = Vec::new();
    let mut total_hits = 0usize;
    let mut total_misses = 0usize;
    let section_for = |index: usize| -> Option<u32> {
        events
            .get(index)
            .map(|event| section_of(event.time_s, offset_s, bpm, thresholds.bars_per_section))
    };
    for session in &used {
        for line in &session.lines {
            match line {
                NoteLine::Hit { i, off_ms, .. } => {
                    if let Some(section) = section_for(*i) {
                        let slot = scratch.entry(section).or_default();
                        slot.hits += 1;
                        slot.offsets.push(*off_ms);
                        all_offsets.push(*off_ms);
                        total_hits += 1;
                    }
                }
                NoteLine::Miss { i, .. } => {
                    if let Some(section) = section_for(*i) {
                        scratch.entry(section).or_default().misses += 1;
                        total_misses += 1;
                    }
                }
                NoteLine::Sustain { s, done } => {
                    if let Some(section) = section_for(*s) {
                        let slot = scratch.entry(section).or_default();
                        slot.sustains_seen += 1;
                        if !done {
                            slot.sustains_dropped += 1;
                        }
                    }
                }
                NoteLine::Overstrum { near, .. } => {
                    if let Some(section) = near.and_then(section_for) {
                        scratch.entry(section).or_default().overstrums += 1;
                    }
                }
            }
        }
    }

    let bars_per = thresholds.bars_per_section.max(1);
    let bar_s = bar_length_s(bpm);
    let sections: Vec<SectionReport> = scratch
        .iter()
        .map(|(section, slot)| {
            let judged = slot.hits + slot.misses;
            let start_bar = section * bars_per;
            SectionReport {
                bar_start: start_bar,
                bar_end: start_bar + bars_per,
                time_s: (
                    f64::from(start_bar).mul_add(bar_s, offset_s),
                    f64::from(start_bar + bars_per).mul_add(bar_s, offset_s),
                ),
                judged,
                accuracy: if judged == 0 {
                    0.0
                } else {
                    slot.hits as f64 / judged as f64
                },
                mean_off_ms: if slot.offsets.is_empty() {
                    0.0
                } else {
                    slot.offsets.iter().sum::<f64>() / slot.offsets.len() as f64
                },
                stddev_ms: stddev(&slot.offsets),
                sustains: (slot.sustains_seen, slot.sustains_dropped),
                overstrums: slot.overstrums,
            }
        })
        .collect();

    let mut directives = Vec::new();
    if used.len() >= thresholds.min_sessions {
        let meta = used[0];
        let make = |bars: Option<(u32, u32)>,
                    problem: &str,
                    evidence: Evidence,
                    recommend: &[&str]| Directive {
            title: meta.header.title.clone(),
            artist: meta.header.artist.clone(),
            difficulty: meta.header.difficulty.clone(),
            chart_hash: current_hash.to_owned(),
            bars,
            problem: problem.to_owned(),
            evidence,
            recommend: recommend.iter().map(|r| (*r).to_owned()).collect(),
            constraints: vec![
                "preserve_musical_identity".to_owned(),
                "stay_playable".to_owned(),
            ],
        };
        for section in &sections {
            if section.judged < thresholds.min_section_samples {
                continue;
            }
            let evidence = Evidence {
                sessions: used.len(),
                accuracy: section.accuracy,
                stddev_ms: section.stddev_ms,
                dropped_sustains: (section.sustains.0 > 0).then_some(section.sustains),
            };
            let bars = Some((section.bar_start, section.bar_end));
            if section.accuracy < thresholds.low_accuracy {
                directives.push(make(
                    bars,
                    "low_accuracy",
                    evidence,
                    &["reduce_density", "simplify_movement"],
                ));
            } else if section.sustains.0 >= 2
                && section.sustains.1 as f64 / section.sustains.0 as f64
                    >= thresholds.dropped_sustain_rate
            {
                directives.push(make(
                    bars,
                    "dropped_sustains",
                    evidence,
                    &["revisit_sustains"],
                ));
            } else if section.stddev_ms > thresholds.sloppy_stddev_ms {
                directives.push(make(
                    bars,
                    "sloppy_timing",
                    evidence,
                    &["steady_the_rhythm", "simplify_movement"],
                ));
            }
        }
        // The boredom signal: only when NOTHING else is wrong — a
        // chart with a failing section is not mastered, whatever the
        // average says.
        let judged_all = total_hits + total_misses;
        if directives.is_empty() && judged_all >= thresholds.min_section_samples {
            let accuracy = total_hits as f64 / judged_all as f64;
            let spread = stddev(&all_offsets);
            if accuracy >= thresholds.mastered_accuracy && spread <= thresholds.mastered_stddev_ms {
                directives.push(make(
                    None,
                    "trivially_mastered",
                    Evidence {
                        sessions: used.len(),
                        accuracy,
                        stddev_ms: spread,
                        dropped_sustains: None,
                    },
                    &["raise_challenge"],
                ));
            }
        }
    }

    Review {
        sessions_used: used.len(),
        stale_sessions: stale,
        autopilot_sessions: piloted,
        sections,
        directives,
    }
}

/// Whether a session played its chart to the end.
#[must_use]
pub fn is_complete(session: &Session) -> bool {
    judged_events(&session.lines) >= session.header.notes_total && session.header.notes_total > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::telemetry::SCHEMA_VERSION;
    use beatbyte_core::{Lane, LaneSet, NoteEvent, TempoMap, Track};

    const BPM: f64 = 120.0; // bar = 2 s; 4-bar section = 8 s

    fn track_with_events(times: &[f64]) -> Track {
        let events: Vec<NoteEvent> = times
            .iter()
            .map(|t| NoteEvent::tap(*t, LaneSet::single(Lane::from_index(0).expect("lane"))))
            .collect();
        Track::new(
            beatbyte_core::Difficulty::Medium,
            TempoMap::constant(BPM, 0.0),
            events,
            Vec::new(),
        )
        .expect("a valid test track")
    }

    fn header(hash: &str, autopilot: bool) -> SessionHeader {
        SessionHeader {
            schema: SCHEMA_VERSION,
            title: "T".to_owned(),
            artist: "A".to_owned(),
            difficulty: "medium".to_owned(),
            chart_hash: hash.to_owned(),
            generator: "0".to_owned(),
            started_ms: 0,
            player: 0,
            autopilot,
            notes_total: 4,
        }
    }

    fn hit(i: usize, off_ms: f64) -> NoteLine {
        NoteLine::Hit {
            i,
            j: "perfect".to_owned(),
            off_ms,
        }
    }

    fn miss(i: usize) -> NoteLine {
        NoteLine::Miss {
            i,
            j: "miss".to_owned(),
        }
    }

    /// Three identical sessions on the given lines.
    fn sessions(lines: &[NoteLine]) -> Vec<Session> {
        (0..3)
            .map(|_| Session {
                header: header("h", false),
                lines: lines.to_vec(),
            })
            .collect()
    }

    #[test]
    fn events_land_in_their_bars_sections() {
        // 120 BPM: a bar is 2 s, a 4-bar section 8 s. 7.9 s is still
        // section 0; 8.1 s is section 1.
        assert_eq!(section_of(7.9, 0.0, BPM, 4), 0);
        assert_eq!(section_of(8.1, 0.0, BPM, 4), 1);
        // The chart offset shifts the grid, not the notes.
        assert_eq!(section_of(8.1, 1.0, BPM, 4), 0);
    }

    #[test]
    fn a_failing_section_produces_a_directive_with_its_bars() {
        // Section 0 (0-8 s) is fine; section 1 (8-16 s) fails.
        let track = track_with_events(&[1.0, 2.0, 9.0, 10.0]);
        let lines = vec![hit(0, 2.0), hit(1, -2.0), miss(2), miss(3)];
        let out = review(
            &track,
            BPM,
            0.0,
            "h",
            &sessions(&lines),
            false,
            &Thresholds::default(),
        );
        assert_eq!(out.directives.len(), 1, "exactly the failing section");
        let directive = &out.directives[0];
        assert_eq!(directive.problem, "low_accuracy");
        assert_eq!(directive.bars, Some((4, 8)));
        assert!(directive.evidence.accuracy < 0.01);
        assert_eq!(directive.evidence.sessions, 3);
    }

    #[test]
    fn below_the_session_threshold_nothing_is_concluded() {
        // Two sessions of pure misses: strong-looking, still an
        // anecdote. The report exists; directives do not.
        let track = track_with_events(&[1.0, 2.0, 3.0, 4.0]);
        let lines = vec![miss(0), miss(1), miss(2), miss(3)];
        let two: Vec<Session> = sessions(&lines).into_iter().take(2).collect();
        let out = review(&track, BPM, 0.0, "h", &two, false, &Thresholds::default());
        assert_eq!(out.sessions_used, 2);
        assert!(!out.sections.is_empty(), "the report still describes them");
        assert!(out.directives.is_empty(), "two sessions prove nothing");
    }

    #[test]
    fn sessions_of_another_chart_version_feed_nothing() {
        // The hash-binding payoff: evidence from an older version of
        // the chart would judge notes that no longer exist.
        let track = track_with_events(&[1.0, 2.0, 3.0, 4.0]);
        let lines = vec![miss(0), miss(1), miss(2), miss(3)];
        let mut mixed = sessions(&lines);
        for session in &mut mixed {
            session.header.chart_hash = "OLD".to_owned();
        }
        let out = review(&track, BPM, 0.0, "h", &mixed, false, &Thresholds::default());
        assert_eq!(out.sessions_used, 0);
        assert_eq!(
            out.stale_sessions, 3,
            "stale sessions are counted, not hidden"
        );
        assert!(out.directives.is_empty());
        assert!(out.sections.is_empty());
    }

    #[test]
    fn autopilot_sessions_are_excluded_by_default() {
        // A perfect player in the data concludes every chart is too
        // easy. The flag exists so the pipeline can still be tested.
        let track = track_with_events(&[1.0, 2.0, 3.0, 4.0]);
        let lines = vec![hit(0, 0.0), hit(1, 0.0), hit(2, 0.0), hit(3, 0.0)];
        let mut piloted = sessions(&lines);
        for session in &mut piloted {
            session.header.autopilot = true;
        }
        let out = review(
            &track,
            BPM,
            0.0,
            "h",
            &piloted,
            false,
            &Thresholds::default(),
        );
        assert_eq!(out.sessions_used, 0);
        assert_eq!(out.autopilot_sessions, 3);
        let included = review(
            &track,
            BPM,
            0.0,
            "h",
            &piloted,
            true,
            &Thresholds::default(),
        );
        assert_eq!(included.sessions_used, 3);
    }

    #[test]
    fn mastery_is_only_called_when_nothing_else_is_wrong() {
        // Perfect play with tight timing -> raise_challenge...
        let track = track_with_events(&[1.0, 2.0, 3.0, 4.0]);
        let perfect = vec![hit(0, 1.0), hit(1, -1.0), hit(2, 2.0), hit(3, 0.0)];
        let out = review(
            &track,
            BPM,
            0.0,
            "h",
            &sessions(&perfect),
            false,
            &Thresholds::default(),
        );
        assert_eq!(out.directives.len(), 1);
        assert_eq!(out.directives[0].problem, "trivially_mastered");
        assert_eq!(
            out.directives[0].bars, None,
            "mastery is a whole-chart call"
        );

        // ...but a problem elsewhere silences the call. The probing
        // case is NOT a section of misses — that drags the average
        // accuracy below the mastery bar and never exercises the
        // veto (the first version of this test did exactly that and
        // stayed green with the veto deleted). It is PERFECT accuracy
        // with dropped sustains: every note lands, the holds do not,
        // and only the veto keeps "raise the challenge" from being
        // recommended for a chart whose holds are failing.
        let mut dropped = vec![hit(0, 0.0), hit(1, 0.0), hit(2, 0.0), hit(3, 0.0)];
        dropped.push(NoteLine::Sustain { s: 0, done: false });
        dropped.push(NoteLine::Sustain { s: 1, done: false });
        let out2 = review(
            &track,
            BPM,
            0.0,
            "h",
            &sessions(&dropped),
            false,
            &Thresholds::default(),
        );
        assert!(
            out2.directives
                .iter()
                .any(|d| d.problem == "dropped_sustains"),
            "the sustain problem itself must be reported"
        );
        assert!(
            out2.directives
                .iter()
                .all(|d| d.problem != "trivially_mastered"),
            "a chart with failing holds must not be called mastered"
        );
    }

    #[test]
    fn dropped_sustains_are_their_own_problem() {
        let track = track_with_events(&[1.0, 2.0, 3.0, 4.0]);
        let mut lines = vec![hit(0, 1.0), hit(1, -1.0), hit(2, 1.0), hit(3, 0.0)];
        lines.push(NoteLine::Sustain { s: 0, done: false });
        lines.push(NoteLine::Sustain { s: 1, done: false });
        let out = review(
            &track,
            BPM,
            0.0,
            "h",
            &sessions(&lines),
            false,
            &Thresholds::default(),
        );
        assert_eq!(out.directives.len(), 1);
        assert_eq!(out.directives[0].problem, "dropped_sustains");
        assert_eq!(out.directives[0].recommend, vec!["revisit_sustains"]);
    }

    #[test]
    fn a_directive_round_trips_as_json() {
        let directive = Directive {
            title: "T".to_owned(),
            artist: "A".to_owned(),
            difficulty: "medium".to_owned(),
            chart_hash: "h".to_owned(),
            bars: Some((4, 8)),
            problem: "low_accuracy".to_owned(),
            evidence: Evidence {
                sessions: 3,
                accuracy: 0.5,
                stddev_ms: 12.0,
                dropped_sustains: Some((2, 1)),
            },
            recommend: vec!["reduce_density".to_owned()],
            constraints: vec!["stay_playable".to_owned()],
        };
        let text = serde_json::to_string(&directive).expect("serializes");
        let back: Directive = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, directive);
    }

    #[test]
    fn completion_uses_the_headers_total() {
        let complete = Session {
            header: header("h", false),
            lines: vec![hit(0, 0.0), hit(1, 0.0), miss(2), miss(3)],
        };
        assert!(is_complete(&complete));
        let abandoned = Session {
            header: header("h", false),
            lines: vec![hit(0, 0.0)],
        };
        assert!(!is_complete(&abandoned));
    }
}
