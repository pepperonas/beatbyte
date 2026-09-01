//! The play history's schema: one line per played track.
//!
//! Lives here, not in the game crate, for the reason the telemetry
//! schema does: the game WRITES these files and the CLI READS them,
//! so the format has exactly one definition and neither side can
//! drift from the other.

use serde::{Deserialize, Serialize};

/// One played track.
///
/// Title and artist are separate fields, never joined: the score
/// board's `title|artist` key is a known collision (roadmap C5) and
/// the telemetry schema already refuses to copy it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayEntry {
    /// Song title, as the chart carries it.
    pub title: String,
    /// Song artist, as the chart carries it.
    pub artist: String,
    /// Difficulty played, lowercase display name.
    pub difficulty: String,
    /// Unix milliseconds when the run started.
    pub started_ms: u64,
    /// How long the track actually ran, in wall-clock seconds.
    ///
    /// Wall clock, not song time: at 50 % practice speed a track is
    /// audible for twice its length, and "how long was this
    /// performed" is the question a reporting body asks.
    pub played_s: f64,
    /// The song's own length, when the chart knows it. Together with
    /// `played_s` this says whether the track ran through or was
    /// left early — without the reader having to trust a flag alone.
    pub track_s: Option<f64>,
    /// Whether the run reached the end of the song.
    pub completed: bool,
    /// How many players were on the highway.
    pub players: usize,
    /// Practice speed or a section loop was used at some point.
    pub practice: bool,
    /// The autopilot was driving (test runs, not performances).
    pub autopilot: bool,
    /// Score of player one — the analysis side of the log.
    pub score: u64,
    /// Weighted accuracy of player one, 0.0–1.0.
    pub accuracy: f64,
    /// Where the audio came from: `builtin` or `file`.
    pub source: String,
}

/// Serialize one entry as a log line (no trailing newline). Pure.
///
/// # Errors
/// When the entry cannot be serialized, which for this plain struct
/// means a `serde_json` bug rather than bad input.
pub fn render_entry(entry: &PlayEntry) -> Result<String, serde_json::Error> {
    serde_json::to_string(entry)
}

/// Read a whole log, skipping lines that do not parse.
///
/// A history is appended to over years and read by tools that were
/// written later: one damaged line (a half-written record after a
/// crash, a field from a newer version) must cost that line and
/// nothing else. Pure — tested.
#[must_use]
pub fn parse_log(text: &str) -> Vec<PlayEntry> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> PlayEntry {
        PlayEntry {
            title: "Synthetic Song".to_owned(),
            artist: "The Null Pointers".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms: 1_756_000_000_000,
            played_s: 64.5,
            track_s: Some(65.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 12_345,
            accuracy: 0.93,
            source: "file".to_owned(),
        }
    }

    #[test]
    fn an_entry_round_trips_through_a_log_line() {
        let line = render_entry(&entry()).expect("serializes");
        assert_eq!(parse_log(&line), vec![entry()]);
        // One line per run: a record must never span two lines, or
        // the append-only format falls apart.
        assert!(!line.contains('\n'));
    }

    #[test]
    fn a_damaged_line_costs_only_itself() {
        // Histories are appended to for years and read by tools
        // written later; a half-written record after a crash must
        // not take the rest of the log with it.
        let good = render_entry(&entry()).expect("serializes");
        let log = format!("{good}\nnot json at all\n\n{{\"title\":\"only a title\"}}\n{good}\n");
        assert_eq!(
            parse_log(&log).len(),
            2,
            "the two intact records survive, the broken ones are skipped"
        );
    }

    #[test]
    fn the_flags_a_report_filters_on_are_all_recorded() {
        // The recorder deliberately keeps runs that a report may
        // want to exclude - practice, autopilot, aborted - because
        // dropping them here would be unrecoverable, while filtering
        // them at export is one flag. This pins that each of those
        // facts actually survives into the log.
        let aborted = PlayEntry {
            completed: false,
            practice: true,
            autopilot: true,
            played_s: 3.0,
            ..entry()
        };
        let line = render_entry(&aborted).expect("serializes");
        let back = &parse_log(&line)[0];
        assert!(!back.completed && back.practice && back.autopilot);
        assert!((back.played_s - 3.0).abs() < f64::EPSILON);
    }
}
