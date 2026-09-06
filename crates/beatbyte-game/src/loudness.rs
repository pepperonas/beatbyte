//! Loudness matching in the game: the gain a song plays at, and the
//! browser's quality marker — both read from the sidecar the import
//! (or `beatbyte-cli loudness --write`) put beside the audio. Nothing
//! is measured at play time. Pure helpers, tested; the systems that
//! start music call [`song_gain_for`] right after starting a song.

use std::path::Path;

use beatbyte_audio::loudness::{Report, db_to_linear, read_report};
use beatbyte_audio::quality::Verdict;

use crate::boot::SongAudio;
use crate::config::Settings;

/// What the browser and the gain need from a sidecar, cheap enough to
/// carry on every library entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LoudnessMark {
    /// The gain the game applies, dB.
    pub gain_db: f64,
    /// Whether the peaks, not the target, set the gain.
    pub peak_limited: bool,
    /// The file's quality verdict.
    pub verdict: Verdict,
    /// The worst issue, in one line, when there is one to show.
    pub issue: Option<String>,
}

impl LoudnessMark {
    /// From a report.
    #[must_use]
    pub fn from_report(report: &Report) -> LoudnessMark {
        LoudnessMark {
            gain_db: report.gain_db(),
            peak_limited: report.peak_limited(),
            verdict: report.quality.verdict,
            issue: report
                .quality
                .issues
                .iter()
                .find(|i| i.severity != Verdict::Good)
                .map(|i| i.what.clone()),
        }
    }

    /// Read the sidecar beside an audio file, if any.
    #[must_use]
    pub fn beside(audio_path: &Path) -> Option<LoudnessMark> {
        read_report(audio_path).map(|r| LoudnessMark::from_report(&r))
    }
}

/// The factor a song plays at: its sidecar's gain when loudness
/// matching is on, unity otherwise — and unity for a song without a
/// sidecar or for in-memory audio. Pure — tested.
#[must_use]
pub fn song_gain(mark: Option<&LoudnessMark>, normalize: bool) -> f32 {
    match (normalize, mark) {
        (true, Some(mark)) => db_to_linear(mark.gain_db) as f32,
        _ => 1.0,
    }
}

/// [`song_gain`] for what is about to play, reading the sidecar.
#[must_use]
pub fn song_gain_for(audio: &SongAudio, settings: &Settings) -> f32 {
    match audio {
        SongAudio::File(path) => song_gain(
            LoudnessMark::beside(path).as_ref(),
            settings.normalize_loudness,
        ),
        SongAudio::Memory(_) => 1.0,
    }
}

/// The browser's marker for a song's audio quality: nothing for a
/// good file or an unmeasured one, a short warning otherwise. Pure —
/// tested.
#[must_use]
pub fn quality_marker(mark: Option<&LoudnessMark>) -> String {
    match mark {
        Some(m) if m.verdict == Verdict::Poor => {
            format!("!! {}", m.issue.as_deref().unwrap_or("poor audio"))
        }
        Some(m) if m.verdict == Verdict::Fair => {
            format!("! {}", m.issue.as_deref().unwrap_or("audio"))
        }
        _ => String::new(),
    }
}

/// One line for an import's status: the warning a poor or fair file
/// earns, or nothing.
#[must_use]
pub fn import_warning(report: &Report) -> Option<String> {
    let mark = LoudnessMark::from_report(report);
    match mark.verdict {
        Verdict::Good => None,
        _ => Some(format!(
            "audio {}: {}",
            mark.verdict.label(),
            mark.issue.as_deref().unwrap_or("see the loudness report")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_audio::quality::Issue;

    fn mark(verdict: Verdict, gain_db: f64) -> LoudnessMark {
        LoudnessMark {
            gain_db,
            peak_limited: false,
            verdict,
            issue: (verdict != Verdict::Good).then(|| "spectrum ends at 11.0 kHz".to_owned()),
        }
    }

    #[test]
    fn the_gain_follows_the_sidecar_only_while_matching_is_on() {
        let m = mark(Verdict::Good, -6.0206);
        assert!((song_gain(Some(&m), true) - 0.5).abs() < 1e-4);
        assert!(
            (song_gain(Some(&m), false) - 1.0).abs() < 1e-6,
            "switched off: unity"
        );
        assert!(
            (song_gain(None, true) - 1.0).abs() < 1e-6,
            "no sidecar: unity"
        );
        let settings = Settings::default();
        assert!(settings.normalize_loudness, "on by default");
        let memory = SongAudio::Memory(beatbyte_audio::AudioData::from_mono(vec![0.0; 10], 8000));
        assert!((song_gain_for(&memory, &settings) - 1.0).abs() < 1e-6);
        let nowhere = SongAudio::File(std::path::PathBuf::from("/no/such/song.m4a"));
        assert!((song_gain_for(&nowhere, &settings) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_marker_names_the_worst_issue_and_stays_quiet_for_good_files() {
        assert_eq!(quality_marker(None), "");
        assert_eq!(quality_marker(Some(&mark(Verdict::Good, 0.0))), "");
        assert_eq!(
            quality_marker(Some(&mark(Verdict::Fair, 0.0))),
            "! spectrum ends at 11.0 kHz"
        );
        assert_eq!(
            quality_marker(Some(&mark(Verdict::Poor, 0.0))),
            "!! spectrum ends at 11.0 kHz"
        );
        // From a report: the first non-good issue is the one shown.
        let report = beatbyte_audio::loudness::Report {
            schema: beatbyte_audio::loudness::REPORT_SCHEMA.to_owned(),
            measured_by: "test".to_owned(),
            audio: "x.m4a".to_owned(),
            bytes: 1,
            measurement: beatbyte_audio::loudness::Measurement {
                integrated_lufs: Some(-10.0),
                loudness_range_lu: None,
                true_peak_dbtp: -0.5,
                sample_peak_dbfs: -0.6,
                duration_s: 1.0,
            },
            quality: beatbyte_audio::quality::Quality {
                verdict: Verdict::Poor,
                issues: vec![
                    Issue {
                        severity: Verdict::Poor,
                        what: "clipped (0.30 % of the samples)".to_owned(),
                    },
                    Issue {
                        severity: Verdict::Good,
                        what: "mono".to_owned(),
                    },
                ],
                ..Default::default()
            },
        };
        let m = LoudnessMark::from_report(&report);
        assert_eq!(m.issue.as_deref(), Some("clipped (0.30 % of the samples)"));
        assert!((m.gain_db + 6.0).abs() < 1e-9, "−10 LUFS turns down by 6");
        assert_eq!(
            import_warning(&report).as_deref(),
            Some("audio poor: clipped (0.30 % of the samples)")
        );
        let mut good = report;
        good.quality.verdict = Verdict::Good;
        good.quality.issues.clear();
        assert_eq!(import_warning(&good), None);
    }
}
