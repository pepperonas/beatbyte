//! Evaluation harness: synthetic scenes with known ground truth, and
//! the metrics that turn "the charts feel better" into numbers.
//!
//! The rule this module exists to enforce: **no transcription change
//! lands without a before/after measurement.** Ear judgement decides
//! what to aim for; this decides whether a change actually got there.
//!
//! - [`instruments`] — deterministic synthesis primitives
//! - [`scenes`] — the eight scenes (A–H) and their ground truth
//! - [`metrics`] — matching, precision/recall, pitch, sustain, tempo
//!
//! Everything is deterministic: fixed-seed noise, sorted iteration, no
//! wall-clock. The same build always produces the same report.

pub mod instruments;
pub mod metrics;
pub mod scenes;

use beatbyte_core::music::SongAnalysis;

use crate::analysis::{Analyzer, SpectralAnalyzer};
use metrics::{Detected, MatchReport, TempoReport};
use scenes::Scene;

/// The measured quality of one scene under the current pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneReport {
    /// Which scene this is.
    pub name: &'static str,
    /// What the scene is designed to prove.
    pub about: &'static str,
    /// Tempo detection against the scene's true tempo.
    pub tempo: TempoReport,
    /// Raw onset detection judged against the chartable line.
    pub onsets: MatchReport,
    /// The melody stage judged against the chartable line.
    pub melody: MatchReport,
    /// Melody events that land inside a held tone (1.0 = one spurious
    /// event per sustain).
    pub sustain_fragmentation: f64,
    /// Share of melody events sitting on drums/vocals only.
    pub contamination: f64,
    /// How many melody events the stage produced at all.
    pub melody_events: usize,
}

/// Analyze one scene with the current pipeline and measure it.
#[must_use]
pub fn evaluate(scene: &Scene) -> SceneReport {
    let analysis = SpectralAnalyzer::default().analyze(&scene.audio);
    report_for(scene, &analysis)
}

/// Measure an already-computed analysis against a scene. Kept
/// separate so a caller that needs the analysis anyway (the CLI, a
/// A/B comparison) does not pay for it twice.
#[must_use]
pub fn report_for(scene: &Scene, analysis: &SongAnalysis) -> SceneReport {
    let onset_events: Vec<Detected> = analysis
        .onsets
        .iter()
        .map(|onset| Detected {
            time_s: onset.time_s,
            end_s: onset.time_s,
            midi: None,
        })
        .collect();
    let melody_events: Vec<Detected> = analysis
        .melody
        .iter()
        .map(|note| Detected {
            time_s: note.time_s,
            end_s: note.end_s,
            midi: Some(note.midi),
        })
        .collect();

    SceneReport {
        name: scene.name,
        about: scene.about,
        tempo: metrics::tempo_report(scene, analysis.bpm),
        onsets: metrics::match_events(scene, &onset_events, metrics::TOLERANCE_S),
        melody: metrics::match_events(scene, &melody_events, metrics::TOLERANCE_S),
        sustain_fragmentation: metrics::sustain_fragmentation(scene, &melody_events),
        contamination: metrics::contamination(scene, &melody_events, metrics::TOLERANCE_S),
        melody_events: analysis.melody.len(),
    }
}

/// Run every scene. Ordered, deterministic, and safe to print.
#[must_use]
pub fn evaluate_all() -> Vec<SceneReport> {
    scenes::all().iter().map(evaluate).collect()
}

/// Render reports as a fixed-width table for the CLI and for commit
/// messages — the numbers are meant to be pasted into a diff.
#[must_use]
pub fn format_table(reports: &[SceneReport]) -> String {
    use core::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<22} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "scene", "bpm", "on-F1", "mel-F1", "pitch", "time", "sus", "frag", "contam"
    );
    for report in reports {
        let pitch = report
            .melody
            .pitch_accuracy
            .map_or_else(|| "  n/a".to_owned(), |v| format!("{:>5.0}%", v * 100.0));
        let sustain = report
            .melody
            .sustain_error_s
            .map_or_else(|| "  n/a".to_owned(), |v| format!("{v:>5.2}"));
        let _ = writeln!(
            out,
            "{:<22} {:>7.1} {:>6.2} {:>6.2} {} {:>5.0}ms {} {:>6.2} {:>5.0}%",
            report.name,
            report.tempo.bpm,
            report.onsets.f1(),
            report.melody.f1(),
            pitch,
            report.melody.timing_error_s * 1000.0,
            sustain,
            report.sustain_fragmentation,
            report.contamination * 100.0,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_harness_runs_every_scene_and_is_stable() {
        let first = evaluate_all();
        assert_eq!(first.len(), scenes::all().len());
        let second = evaluate_all();
        assert_eq!(first, second, "evaluation must be deterministic");
    }

    /// Not an assertion — a report. Run with `--nocapture` to see the
    /// current quality of the whole pipeline on every scene.
    #[test]
    fn print_the_report() {
        let reports = evaluate_all();
        println!("\n{}", format_table(&reports));
        for report in &reports {
            println!(
                "{:<22} onsets P{:.2} R{:.2} | melody P{:.2} R{:.2} events {} | {}",
                report.name,
                report.onsets.precision(),
                report.onsets.recall(),
                report.melody.precision(),
                report.melody.recall(),
                report.melody_events,
                report.about,
            );
        }
    }

    #[test]
    fn tempo_is_right_on_every_scene() {
        // Every scene, no exceptions. This started as seven of eight
        // with f_sustains carved out; the carve-out is gone because
        // the defect behind it is.
        for report in evaluate_all() {
            let scene = scenes::all()
                .into_iter()
                .find(|s| s.name == report.name)
                .expect("scene exists");
            assert!(
                report.tempo.error_bpm <= scene.bpm * 0.03,
                "{}: {:.1} BPM against a true {:.1}",
                report.name,
                report.tempo.bpm,
                scene.bpm
            );
            assert!(
                !report.tempo.octave_error,
                "{}: octave error at {:.1} BPM",
                report.name, report.tempo.bpm
            );
        }
    }

    #[test]
    fn the_table_names_every_scene() {
        let reports = evaluate_all();
        let table = format_table(&reports);
        for report in &reports {
            assert!(table.contains(report.name), "{} missing", report.name);
        }
    }
}
