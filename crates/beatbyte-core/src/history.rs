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

/// A CSV field, quoted when it has to be.
///
/// Titles come from file names and tags: they contain commas,
/// quotes and the occasional newline, and a report that splits a
/// title across two columns is worse than no report.
#[must_use]
pub fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// The date-time a row carries: UTC, ISO 8601, seconds resolution.
///
/// Computed here rather than pulled from a date crate — the workspace
/// has none, and this is the civil-calendar arithmetic from
/// Howard Hinnant's `civil_from_days`, which is exact for every day
/// this program can be handed. Pure — tested against known stamps.
#[must_use]
pub fn iso_utc(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Shift the epoch to 0000-03-01 so leap days land at the end.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The CSV header, and the columns every row follows.
pub const CSV_HEADER: &str = "started_utc,title,artist,seconds_played,track_seconds,completed,difficulty,players,practice,autopilot,source,score,accuracy";

/// Render the history as CSV — the reporting format. Pure — tested.
#[must_use]
pub fn to_csv(entries: &[PlayEntry]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for entry in entries {
        out.push_str(&format!(
            "{},{},{},{:.1},{},{},{},{},{},{},{},{},{:.4}\n",
            iso_utc(entry.started_ms),
            csv_field(&entry.title),
            csv_field(&entry.artist),
            entry.played_s,
            entry
                .track_s
                .map_or_else(String::new, |seconds| format!("{seconds:.1}")),
            entry.completed,
            csv_field(&entry.difficulty),
            entry.players,
            entry.practice,
            entry.autopilot,
            csv_field(&entry.source),
            entry.score,
            entry.accuracy,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(title: &str, started_ms: u64, played_s: f64) -> PlayEntry {
        PlayEntry {
            title: title.to_owned(),
            started_ms,
            played_s,
            ..entry()
        }
    }

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
    fn a_title_with_a_comma_stays_one_column() {
        // Titles come from file names and tags. A report that splits
        // a title across two columns is worse than no report.
        assert_eq!(csv_field("Plain"), "Plain");
        assert_eq!(csv_field("Hello, World"), "\"Hello, World\"");
        assert_eq!(csv_field("She said \"hi\""), "\"She said \"\"hi\"\"\"");
        let rows = to_csv(&[titled("Comma, Song", 0, 10.0)]);
        let line = rows.lines().nth(1).expect("one row");
        // Counted the way a reader counts them - commas inside
        // quotes are text, not separators. (Counting raw commas is
        // what the first version of this test did, and it failed on
        // correct output.)
        let fields = split_csv(line);
        assert_eq!(fields.len(), CSV_HEADER.split(',').count());
        assert_eq!(fields[1], "Comma, Song", "the title stayed one field");
    }

    #[test]
    fn the_csv_carries_what_a_report_asks_for() {
        let csv = to_csv(&[titled("Synthetic", 1_756_684_800_000, 64.5)]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(CSV_HEADER));
        let row = lines.next().expect("one row");
        // The work, when it was performed, and for how long.
        assert!(row.starts_with("2025-09-01T00:00:00Z,Synthetic,The Null Pointers,64.5"));
    }

    #[test]
    fn the_timestamp_matches_known_dates() {
        // Fixed points, including a leap day and the epoch itself -
        // the calendar arithmetic is hand-written, so it gets
        // checked against dates that can be looked up.
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_000), "1970-01-01T00:00:01Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_756_684_800_000), "2025-09-01T00:00:00Z");
        assert_eq!(iso_utc(1_735_689_599_000), "2024-12-31T23:59:59Z");
    }

    /// Split one CSV row the way a spreadsheet does: quoted fields
    /// keep their commas, doubled quotes are one quote.
    fn split_csv(line: &str) -> Vec<String> {
        let (mut fields, mut current, mut quoted) = (Vec::new(), String::new(), false);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    current.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => fields.push(std::mem::take(&mut current)),
                other => current.push(other),
            }
        }
        fields.push(current);
        fields
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
