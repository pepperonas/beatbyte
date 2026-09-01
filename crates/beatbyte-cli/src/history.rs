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
