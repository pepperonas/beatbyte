//! Chart validation: collect *all* problems with useful messages
//! instead of failing on the first.
//!
//! Charts are untrusted input; validation gates version, numeric ranges,
//! note counts, lane indices, the audio path, duplicate notes and phrase
//! structure. Errors make a chart unplayable; warnings are advisory.

use std::collections::HashSet;
use std::path::Component;

use crate::schema::{ChartDef, ChartFile};
use crate::{FORMAT_VERSION, MAX_NOTES_PER_CHART, MAX_SONG_LENGTH_S};

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The chart must not be played.
    Error,
    /// Suspicious but playable.
    Warning,
}

/// One validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// How serious it is.
    pub severity: Severity,
    /// Where in the file (human-readable, e.g. `charts[expert].notes[3]`).
    pub location: String,
    /// What is wrong.
    pub message: String,
}

impl core::fmt::Display for Issue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let tag = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{tag}: {}: {}", self.location, self.message)
    }
}

/// BPM range accepted by the validator.
pub const BPM_RANGE: core::ops::RangeInclusive<f64> = 20.0..=400.0;

/// Maximum absolute chart offset in seconds.
pub const MAX_OFFSET_S: f64 = 60.0;

/// Maximum sustain length in seconds.
pub const MAX_SUSTAIN_S: f64 = 300.0;

impl ChartFile {
    /// Validate the chart, returning every issue found. A chart with no
    /// [`Severity::Error`] issues is playable.
    #[must_use]
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let err = |issues: &mut Vec<Issue>, location: &str, message: String| {
            issues.push(Issue {
                severity: Severity::Error,
                location: location.to_owned(),
                message,
            });
        };
        let warn = |issues: &mut Vec<Issue>, location: &str, message: String| {
            issues.push(Issue {
                severity: Severity::Warning,
                location: location.to_owned(),
                message,
            });
        };

        // Version gate.
        if self.format_version == 0 || self.format_version > FORMAT_VERSION {
            err(
                &mut issues,
                "format_version",
                format!(
                    "unsupported format version {} (this build supports 1–{FORMAT_VERSION})",
                    self.format_version
                ),
            );
        }

        // Song metadata.
        if self.song.title.trim().is_empty() {
            err(&mut issues, "song.title", "title must not be empty".into());
        }
        if !self.song.bpm.is_finite() || !BPM_RANGE.contains(&self.song.bpm) {
            err(
                &mut issues,
                "song.bpm",
                format!(
                    "bpm {} out of range ({}–{})",
                    self.song.bpm,
                    BPM_RANGE.start(),
                    BPM_RANGE.end()
                ),
            );
        }
        if !self.song.offset_s.is_finite() || self.song.offset_s.abs() > MAX_OFFSET_S {
            err(
                &mut issues,
                "song.offset_s",
                format!(
                    "offset {} out of range (±{MAX_OFFSET_S}s)",
                    self.song.offset_s
                ),
            );
        }
        if let Some(reason) = audio_path_problem(&self.song.audio) {
            err(&mut issues, "song.audio", reason);
        }
        for (name, value) in [
            ("song.preview_start_s", self.song.preview_start_s),
            ("song.duration_s", self.song.duration_s),
        ] {
            if let Some(v) = value
                && (!v.is_finite() || !(0.0..=MAX_SONG_LENGTH_S).contains(&v))
            {
                err(&mut issues, name, format!("value {v} out of range"));
            }
        }

        // Charts.
        if self.charts.is_empty() {
            err(
                &mut issues,
                "charts",
                "chart file contains no charts".into(),
            );
        }
        let mut seen = HashSet::new();
        for chart in &self.charts {
            if !seen.insert(chart.difficulty) {
                err(
                    &mut issues,
                    "charts",
                    format!("duplicate difficulty `{}`", chart.difficulty.id()),
                );
            }
            validate_chart_def(chart, &mut issues, &err, &warn);
        }

        issues
    }
}

fn validate_chart_def(
    chart: &ChartDef,
    issues: &mut Vec<Issue>,
    err: &impl Fn(&mut Vec<Issue>, &str, String),
    warn: &impl Fn(&mut Vec<Issue>, &str, String),
) {
    let loc = format!("charts[{}]", chart.difficulty.id());

    if chart.lanes != 5 {
        err(
            issues,
            &loc,
            format!("format v1 requires 5 lanes, found {}", chart.lanes),
        );
    }
    if chart.notes.is_empty() {
        warn(issues, &loc, "chart has no notes".into());
    }
    if chart.notes.len() > MAX_NOTES_PER_CHART {
        err(
            issues,
            &loc,
            format!(
                "{} notes exceed the maximum of {MAX_NOTES_PER_CHART}",
                chart.notes.len()
            ),
        );
        // Don't iterate an absurd list.
        return;
    }

    let mut seen_notes = HashSet::new();
    for (i, note) in chart.notes.iter().enumerate() {
        let nloc = format!("{loc}.notes[{i}]");
        if !note.time.is_finite() || note.time < 0.0 || note.time > MAX_SONG_LENGTH_S {
            err(issues, &nloc, format!("time {} out of range", note.time));
            continue;
        }
        if note.lane >= chart.lanes {
            err(
                issues,
                &nloc,
                format!("lane {} out of range (0–{})", note.lane, chart.lanes - 1),
            );
        }
        if !note.len.is_finite() || note.len < 0.0 || note.len > MAX_SUSTAIN_S {
            err(
                issues,
                &nloc,
                format!("sustain length {} out of range", note.len),
            );
        }
        // Duplicate = same lane at the same millisecond.
        let key = (note.lane, (note.time * 1000.0).round() as i64);
        if !seen_notes.insert(key) {
            err(
                issues,
                &nloc,
                format!("duplicate note on lane {} at {:.3}s", note.lane, note.time),
            );
        }
    }

    for (i, phrase) in chart.phrases.iter().enumerate() {
        let ploc = format!("{loc}.phrases[{i}]");
        if !phrase.start.is_finite()
            || !phrase.end.is_finite()
            || phrase.start < 0.0
            || phrase.end < phrase.start
            || phrase.end > MAX_SONG_LENGTH_S
        {
            err(
                issues,
                &ploc,
                format!("invalid phrase bounds {}–{}", phrase.start, phrase.end),
            );
        }
    }
    let mut sorted: Vec<_> = chart
        .phrases
        .iter()
        .filter(|p| p.start.is_finite() && p.end.is_finite())
        .collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    for pair in sorted.windows(2) {
        if pair[1].start <= pair[0].end {
            err(
                issues,
                &format!("{loc}.phrases"),
                format!("phrases overlap at {:.3}s", pair[1].start),
            );
        }
    }
}

/// Why an audio reference is unacceptable, if it is.
fn audio_path_problem(audio: &str) -> Option<String> {
    if audio.trim().is_empty() {
        return Some("audio path must not be empty".into());
    }
    // Normalize Windows separators before inspecting components.
    let normalized = audio.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    if path.is_absolute() || normalized.starts_with('/') {
        return Some(format!("audio path `{audio}` must be relative"));
    }
    // Windows drive letters (`C:`) are not `Prefix` components on Unix,
    // so catch them (and URL-like schemes) explicitly.
    if normalized.contains(':') {
        return Some(format!("audio path `{audio}` must not contain `:`"));
    }
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Some(format!("audio path `{audio}` must not contain `..`"));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Some(format!("audio path `{audio}` must be relative"));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::schema::{ChartNote, ChartPhrase, SongMeta};
    use beatbyte_core::Difficulty;

    fn valid_chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "Test".into(),
                artist: "Nobody".into(),
                audio: "audio.ogg".into(),
                bpm: 120.0,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: None,
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Expert,
                lanes: 5,
                notes: vec![
                    ChartNote {
                        time: 1.0,
                        lane: 0,
                        len: 0.0,
                        hopo: false,
                    },
                    ChartNote {
                        time: 2.0,
                        lane: 4,
                        len: 1.5,
                        hopo: true,
                    },
                ],
                phrases: vec![ChartPhrase {
                    start: 0.5,
                    end: 1.5,
                }],
            }],
        }
    }

    fn errors(chart: &ChartFile) -> Vec<Issue> {
        chart
            .validate()
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect()
    }

    #[test]
    fn valid_chart_has_no_errors() {
        assert!(errors(&valid_chart()).is_empty());
    }

    #[test]
    fn future_format_version_is_rejected() {
        let mut chart = valid_chart();
        chart.format_version = 999;
        let errs = errors(&chart);
        assert!(errs.iter().any(|i| i.location == "format_version"));
    }

    #[test]
    fn bpm_bounds_are_exact() {
        // The caps are a security boundary for untrusted charts:
        // 20 and 400 are valid, one hair outside is not.
        for (bpm, ok) in [
            (20.0, true),
            (400.0, true),
            (19.999, false),
            (400.001, false),
        ] {
            let mut chart = valid_chart();
            chart.song.bpm = bpm;
            let report = errors(&chart);
            assert_eq!(
                report.is_empty(),
                ok,
                "bpm {bpm} should be valid={ok}: {report:?}"
            );
        }
    }

    #[test]
    fn bad_bpm_is_rejected() {
        for bpm in [0.0, -10.0, 1000.0, f64::NAN, f64::INFINITY] {
            let mut chart = valid_chart();
            chart.song.bpm = bpm;
            assert!(
                errors(&chart).iter().any(|i| i.location == "song.bpm"),
                "bpm {bpm} must be rejected"
            );
        }
    }

    #[test]
    fn path_traversal_in_audio_is_rejected() {
        for audio in [
            "../secret.ogg",
            "a/../../b.ogg",
            "/etc/passwd",
            "..\\windows.ogg",
            "C:\\music.ogg",
            "",
        ] {
            let mut chart = valid_chart();
            chart.song.audio = audio.into();
            assert!(
                errors(&chart).iter().any(|i| i.location == "song.audio"),
                "audio `{audio}` must be rejected"
            );
        }
    }

    #[test]
    fn honest_relative_audio_paths_pass() {
        for audio in ["audio.ogg", "media/audio.flac", "./audio.mp3"] {
            let mut chart = valid_chart();
            chart.song.audio = audio.into();
            assert!(
                !errors(&chart).iter().any(|i| i.location == "song.audio"),
                "audio `{audio}` should pass"
            );
        }
    }

    #[test]
    fn out_of_range_notes_are_rejected() {
        let mut chart = valid_chart();
        chart.charts[0].notes.push(ChartNote {
            time: -1.0,
            lane: 0,
            len: 0.0,
            hopo: false,
        });
        chart.charts[0].notes.push(ChartNote {
            time: 1.0,
            lane: 9,
            len: 0.0,
            hopo: false,
        });
        chart.charts[0].notes.push(ChartNote {
            time: 3.0,
            lane: 0,
            len: -2.0,
            hopo: false,
        });
        let errs = errors(&chart);
        assert_eq!(errs.len(), 3, "{errs:?}");
    }

    #[test]
    fn duplicate_notes_are_rejected() {
        let mut chart = valid_chart();
        chart.charts[0].notes.push(ChartNote {
            time: 1.0,
            lane: 0,
            len: 0.0,
            hopo: false,
        });
        assert!(
            errors(&chart)
                .iter()
                .any(|i| i.message.contains("duplicate note"))
        );
    }

    #[test]
    fn duplicate_difficulties_are_rejected() {
        let mut chart = valid_chart();
        let dup = chart.charts[0].clone();
        chart.charts.push(dup);
        assert!(
            errors(&chart)
                .iter()
                .any(|i| i.message.contains("duplicate difficulty"))
        );
    }

    #[test]
    fn overlapping_phrases_are_rejected() {
        let mut chart = valid_chart();
        chart.charts[0].phrases.push(ChartPhrase {
            start: 1.0,
            end: 2.0,
        });
        assert!(
            errors(&chart)
                .iter()
                .any(|i| i.message.contains("phrases overlap"))
        );
    }

    #[test]
    fn empty_chart_is_a_warning_not_an_error() {
        let mut chart = valid_chart();
        chart.charts[0].notes.clear();
        chart.charts[0].phrases.clear();
        assert!(errors(&chart).is_empty());
        assert!(
            chart
                .validate()
                .iter()
                .any(|i| i.severity == Severity::Warning)
        );
    }

    #[test]
    fn empty_chart_list_is_an_error() {
        let mut chart = valid_chart();
        chart.charts.clear();
        assert!(errors(&chart).iter().any(|i| i.location == "charts"));
    }
}
