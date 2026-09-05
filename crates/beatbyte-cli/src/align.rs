//! `beatbyte-cli align <audio> <lyrics>` — word- and letter-level
//! timing for known lyrics, written as `<audio>.words.json` beside the
//! audio (plan milestones L2 + L3). Behind the `ml` feature; needs the
//! aligner model installed (`models install wav2vec2-base-960h`).
//!
//! The work is [`beatbyte_lyrics::align_file`], the very function the
//! game runs from its menu; this file only prints. The confidence
//! gate runs by default: it marks words the aligner sprinted through
//! or got stuck on, drops lines with too many of them to line level,
//! and judges the source's stamps against the alignment (same master,
//! shifted master, different edit, failed). `--raw` skips it, for the
//! evaluation harness and for looking at what the aligner itself
//! produced.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use beatbyte_lyrics::{JobError, JobStage, Verdict, align_file};

/// Run the alignment and report.
pub fn run(audio_path: &Path, lyrics_path: &Path, out: Option<PathBuf>, raw: bool) -> ExitCode {
    let cancel = AtomicBool::new(false);
    let mut last_stage = None;
    let summary = align_file(
        audio_path,
        lyrics_path,
        out,
        !raw,
        &mut |p| {
            // One line per stage, not one per window.
            if last_stage != Some(p.stage) {
                last_stage = Some(p.stage);
                if matches!(p.stage, JobStage::Aligning(_)) && p.done == 0 {
                    eprintln!("{}…", p.label());
                }
            }
        },
        &cancel,
    );
    let summary = match summary {
        Ok(summary) => summary,
        Err(JobError::NotInstalled { id }) => {
            eprintln!(
                "model `{id}` is not installed — run `beatbyte-cli models install {id}` first"
            );
            return ExitCode::from(2);
        }
        Err(error @ (JobError::Lyrics { .. } | JobError::NoWords { .. } | JobError::NoStore)) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
        Err(error @ JobError::Audio(_)) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    let s = &summary.stats;
    println!("wrote {}", summary.out.display());
    println!(
        "  {} words ({} estimated), mean confidence {:.2}, {} under {:.1}; {} frames in {:.1?}",
        s.words,
        s.estimated,
        s.mean_conf,
        s.uncertain,
        beatbyte_lyrics::align::UNCERTAIN_BELOW,
        s.frames,
        summary.took
    );
    if let Some((lines, median, mad)) = s.source_line_delta {
        println!(
            "  against the source's {lines} line stamps: aligned − source median {median:+.3} s, \
             spread (MAD) {mad:.3} s"
        );
    }
    match summary.gate {
        None => println!("  raw: the confidence gate did not run"),
        Some(report) => {
            let verdict = match report.verdict {
                Verdict::NoReference => "no line stamps to compare against".to_owned(),
                Verdict::SameMaster => "same master as the source".to_owned(),
                Verdict::ShiftedMaster { offset_s } => {
                    format!("same edit, another master: source is {offset_s:+.2} s off")
                }
                Verdict::DifferentEdit => {
                    "a different edit: the source's stamps are not this file's".to_owned()
                }
                Verdict::Failed => {
                    "alignment FAILED — every line falls back to the source's stamps".to_owned()
                }
            };
            println!(
                "  gate: {verdict}; {} words marked estimated, {} lines at line level",
                report.words_estimated, report.lines_fallen_back
            );
        }
    }
    ExitCode::SUCCESS
}
