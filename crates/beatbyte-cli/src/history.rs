//! Exporting the play history — the reading side of
//! [`beatbyte_core::history`].
//!
//! Two formats because the two stated purposes ask different
//! questions. A reporting body wants one row per performance with
//! the work, the date and how long it ran, in something a
//! spreadsheet opens: CSV. Analysis wants everything, including the
//! fields a report has no use for: JSON.
//!
//! The log itself keeps every run, including practice, autopilot and
//! abandoned ones — deciding which of those count is the export's
//! job, not the recorder's, and the filters below are how that
//! decision is made.

use beatbyte_core::history::PlayEntry;

/// What to filter the log down to before writing it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Filter {
    /// Keep only runs that started at or after this unix stamp.
    pub from_ms: Option<u64>,
    /// Keep only runs that started before this unix stamp.
    pub until_ms: Option<u64>,
    /// Drop runs shorter than this many seconds.
    pub min_seconds: f64,
    /// Drop runs where practice speed or a loop was used.
    pub exclude_practice: bool,
    /// Drop autopilot runs (test runs, not performances).
    pub exclude_autopilot: bool,
    /// Keep only runs that reached the end of the song.
    pub completed_only: bool,
}

/// Apply a filter. Pure — tested.
#[must_use]
pub fn select(entries: &[PlayEntry], filter: Filter) -> Vec<PlayEntry> {
    entries
        .iter()
        .filter(|entry| {
            filter.from_ms.is_none_or(|from| entry.started_ms >= from)
                && filter.until_ms.is_none_or(|until| entry.started_ms < until)
                && entry.played_s >= filter.min_seconds
                && !(filter.exclude_practice && entry.practice)
                && !(filter.exclude_autopilot && entry.autopilot)
                && !(filter.completed_only && !entry.completed)
        })
        .cloned()
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

/// Render the history as JSON — the analysis format: every field,
/// nothing flattened.
///
/// # Errors
/// When the entries cannot be serialized.
pub fn to_json(entries: &[PlayEntry]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, started_ms: u64, played_s: f64) -> PlayEntry {
        PlayEntry {
            title: title.to_owned(),
            artist: "The Null Pointers".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms,
            played_s,
            track_s: Some(65.0),
            completed: true,
            players: 1,
            practice: false,
            autopilot: false,
            score: 12_345,
            accuracy: 0.934_25,
            source: "file".to_owned(),
        }
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

    #[test]
    fn a_title_with_a_comma_stays_one_column() {
        // Titles come from file names and tags. A report that splits
        // a title across two columns is worse than no report.
        assert_eq!(csv_field("Plain"), "Plain");
        assert_eq!(csv_field("Hello, World"), "\"Hello, World\"");
        assert_eq!(csv_field("She said \"hi\""), "\"She said \"\"hi\"\"\"");
        let rows = to_csv(&[entry("Comma, Song", 0, 10.0)]);
        let line = rows.lines().nth(1).expect("one row");
        // Counted the way a reader counts them - commas inside
        // quotes are text, not separators. (Counting raw commas is
        // what the first version of this test did, and it failed on
        // correct output.)
        let fields = split_csv(line);
        assert_eq!(fields.len(), CSV_HEADER.split(',').count());
        assert_eq!(fields[1], "Comma, Song", "the title stayed one field");
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
    fn the_csv_carries_what_a_report_asks_for() {
        let csv = to_csv(&[entry("Synthetic", 1_756_684_800_000, 64.5)]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(CSV_HEADER));
        let row = lines.next().expect("one row");
        // The work, when it was performed, and for how long.
        assert!(row.starts_with("2025-09-01T00:00:00Z,Synthetic,The Null Pointers,64.5"));
    }

    #[test]
    fn the_filters_answer_the_questions_a_report_has() {
        let entries = vec![
            entry("Short", 100, 2.0),
            PlayEntry {
                practice: true,
                ..entry("Practised", 200, 60.0)
            },
            PlayEntry {
                autopilot: true,
                ..entry("Robot", 300, 60.0)
            },
            PlayEntry {
                completed: false,
                ..entry("Abandoned", 400, 30.0)
            },
            entry("Proper", 500, 60.0),
        ];
        // Everything is kept by default: the log records, the export
        // decides.
        assert_eq!(select(&entries, Filter::default()).len(), 5);
        let strict = Filter {
            min_seconds: 30.0,
            exclude_practice: true,
            exclude_autopilot: true,
            completed_only: true,
            ..Filter::default()
        };
        let kept = select(&entries, strict);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "Proper");
        // Windows are half-open, so two neighbouring periods cannot
        // report the same performance twice.
        let window = Filter {
            from_ms: Some(200),
            until_ms: Some(500),
            ..Filter::default()
        };
        let windowed = select(&entries, window);
        let titles: Vec<&str> = windowed.iter().map(|entry| entry.title.as_str()).collect();
        assert_eq!(titles, ["Practised", "Robot", "Abandoned"]);
    }

    #[test]
    fn the_json_export_keeps_every_field() {
        // The analysis format must not lose what the CSV leaves out.
        let json = to_json(&[entry("Synthetic", 1, 2.0)]).expect("serializes");
        for field in [
            "title",
            "artist",
            "difficulty",
            "started_ms",
            "played_s",
            "track_s",
            "completed",
            "players",
            "practice",
            "autopilot",
            "score",
            "accuracy",
            "source",
        ] {
            assert!(json.contains(field), "the JSON export dropped {field}");
        }
    }
}
