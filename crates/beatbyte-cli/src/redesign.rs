//! `redesign` — the difficulty-redesign rollout
//! (docs/difficulty-redesign-plan.md, P5).
//!
//! Regenerates **hard + expert** from a fresh, deterministic analysis
//! of the audio and writes the result as the folder's next sibling
//! version. Easy and medium are carried note-for-note from the ACTIVE
//! version — the ear-approved reading never regenerates. Nothing
//! existing is overwritten; per-song revert stays one pointer away.

use std::path::Path;
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::{
    ChartFile, GenerateMeta, Provenance, Severity, chart_hash, generate_chart, versions,
};
use beatbyte_core::Difficulty;

/// The two difficulties the rollout regenerates.
const REDESIGNED: [Difficulty; 2] = [Difficulty::Hard, Difficulty::Expert];

/// Tempo drift between the active chart and the fresh analysis above
/// which the redesign refuses: mixing readings from two different
/// beat grids is not a redesign, it is a collision.
const BPM_TOLERANCE: f64 = 0.1;

/// The active version's song block and easy/medium, the fresh
/// generation's hard/expert, provenance binding the result to its
/// parent. Pure — the whole decision, no filesystem.
pub fn merged_redesign(
    active: &ChartFile,
    fresh: &ChartFile,
    created_ms: u64,
) -> Result<ChartFile, String> {
    if (active.song.bpm - fresh.song.bpm).abs() > BPM_TOLERANCE {
        return Err(format!(
            "the fresh analysis reads {:.2} BPM where the active chart says {:.2} — \
             two different beat grids cannot merge",
            fresh.song.bpm, active.song.bpm
        ));
    }
    let mut merged = active.clone();
    for difficulty in REDESIGNED {
        let Some(new_chart) = fresh.chart_for(difficulty) else {
            return Err(format!(
                "the fresh generation carries no {difficulty} chart"
            ));
        };
        let Some(slot) = merged
            .charts
            .iter_mut()
            .find(|c| c.difficulty == difficulty)
        else {
            return Err(format!("the active version carries no {difficulty} chart"));
        };
        *slot = new_chart.clone();
    }
    merged.provenance = Some(Provenance {
        parent_hash: chart_hash(active),
        designer: "design-session".to_owned(),
        created_ms,
        directive: Some("difficulty-redesign".to_owned()),
    });
    Ok(merged)
}

/// One song folder: resolve the active version, regenerate, merge,
/// validate, write the next sibling, move the pointer. Every failure
/// is a message, never a partial write.
fn redesign_folder(folder: &Path) -> Result<String, String> {
    let names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("cannot list `{}`: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if !names.iter().any(|n| n == versions::BASE_CHART) {
        return Err(format!(
            "no `{}` — legacy layout, skipped",
            versions::BASE_CHART
        ));
    }
    let pointer = std::fs::read_to_string(folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active_path = folder.join(&active_name);
    let text = std::fs::read_to_string(&active_path)
        .map_err(|error| format!("cannot read `{}`: {error}", active_path.display()))?;
    let active =
        ChartFile::from_json(&text).map_err(|error| format!("`{active_name}`: {error}"))?;

    let audio_path = folder.join(&active.song.audio);
    let audio = decode_file(&audio_path)
        .map_err(|error| format!("cannot decode `{}`: {error}", audio_path.display()))?;
    let analysis = SpectralAnalyzer::default().analyze(&audio);
    let fresh = generate_chart(
        &analysis,
        &GenerateMeta {
            title: active.song.title.clone(),
            artist: active.song.artist.clone(),
            audio: active.song.audio.clone(),
        },
    );

    let created_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    let merged = merged_redesign(&active, &fresh, created_ms)?;
    if chart_hash(&merged) == chart_hash(&active) {
        return Ok(
            "already current (hard + expert match the generator) — nothing written".to_owned(),
        );
    }
    let errors: Vec<String> = merged
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect();
    if !errors.is_empty() {
        return Err(format!(
            "the merged chart failed validation — this is a bug: {}",
            errors.join("; ")
        ));
    }

    let next_name = versions::next_version_name(&names);
    let next_path = folder.join(&next_name);
    beatbyte_chart::save_chart_file(&next_path, &merged)
        .map_err(|error| format!("cannot write `{}`: {error}", next_path.display()))?;
    std::fs::write(
        folder.join(versions::POINTER_FILE),
        format!("{{\"active\": \"{next_name}\"}}\n"),
    )
    .map_err(|error| format!("cannot write the pointer: {error}"))?;

    let counts = |chart: &ChartFile, d: Difficulty| chart.chart_for(d).map_or(0, |c| c.notes.len());
    Ok(format!(
        "{next_name} (parent {active_name}) — hard {} → {}, expert {} → {}",
        counts(&active, Difficulty::Hard),
        counts(&merged, Difficulty::Hard),
        counts(&active, Difficulty::Expert),
        counts(&merged, Difficulty::Expert),
    ))
}

/// `redesign` on one chart path.
pub fn run_redesign(chart_path: &Path) -> ExitCode {
    let Some(folder) = chart_path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        eprintln!("`{}` has no parent folder", chart_path.display());
        return ExitCode::from(2);
    };
    match redesign_folder(folder) {
        Ok(message) => {
            println!("{}: {message}", folder.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{}: {message}", folder.display());
            ExitCode::from(1)
        }
    }
}

/// `redesign --all` over a directory of song folders.
pub fn run_redesign_all(dir: &Path) -> ExitCode {
    let mut folders: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(error) => {
            eprintln!("cannot list `{}`: {error}", dir.display());
            return ExitCode::from(2);
        }
    };
    folders.sort();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for folder in folders {
        match redesign_folder(&folder) {
            Ok(message) => {
                println!("{}: {message}", folder.display());
                written += 1;
            }
            Err(message) if message.contains("legacy layout") => {
                println!("{}: {message}", folder.display());
                skipped += 1;
            }
            Err(message) => {
                eprintln!("{}: {message}", folder.display());
                failed += 1;
            }
        }
    }
    println!("redesigned {written}, skipped {skipped}, failed {failed}");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::{ChartDef, ChartNote, SongMeta};

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    fn chart_file(bpm: f64, lane_for: impl Fn(Difficulty) -> u8) -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "T".into(),
                artist: "A".into(),
                audio: "t.wav".into(),
                bpm,
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(10.0),
                genre: None,
            },
            charts: Difficulty::ALL
                .iter()
                .map(|d| ChartDef {
                    difficulty: *d,
                    lanes: 5,
                    notes: vec![note(1.0, lane_for(*d)), note(2.0, lane_for(*d))],
                    phrases: vec![],
                })
                .collect(),
            provenance: None,
        }
    }

    #[test]
    fn easy_and_medium_are_carried_hard_and_expert_are_fresh() {
        let active = chart_file(120.0, |_| 0);
        let fresh = chart_file(120.0, |_| 3);
        let merged = merged_redesign(&active, &fresh, 7).expect("the merge must succeed");
        for difficulty in [Difficulty::Easy, Difficulty::Medium] {
            assert_eq!(
                merged
                    .chart_for(difficulty)
                    .expect("difficulty present")
                    .notes,
                active
                    .chart_for(difficulty)
                    .expect("difficulty present")
                    .notes,
                "{difficulty} must come from the active version untouched"
            );
        }
        for difficulty in REDESIGNED {
            assert_eq!(
                merged
                    .chart_for(difficulty)
                    .expect("difficulty present")
                    .notes,
                fresh
                    .chart_for(difficulty)
                    .expect("difficulty present")
                    .notes,
                "{difficulty} must come from the fresh generation"
            );
        }
    }

    #[test]
    fn provenance_binds_the_result_to_its_parent() {
        let active = chart_file(120.0, |_| 0);
        let fresh = chart_file(120.0, |_| 3);
        let merged = merged_redesign(&active, &fresh, 7).expect("the merge must succeed");
        let provenance = merged.provenance.expect("a redesign leaves a paper trail");
        assert_eq!(provenance.parent_hash, chart_hash(&active));
        assert_eq!(provenance.created_ms, 7);
        assert_eq!(provenance.directive.as_deref(), Some("difficulty-redesign"));
    }

    #[test]
    fn diverging_beat_grids_refuse_to_merge() {
        let active = chart_file(120.0, |_| 0);
        let fresh = chart_file(121.5, |_| 3);
        let error = merged_redesign(&active, &fresh, 7).expect_err("the merge must refuse");
        assert!(error.contains("beat grids"), "{error}");
        // ...while measurement noise passes.
        let close = chart_file(120.05, |_| 3);
        assert!(merged_redesign(&active, &close, 7).is_ok());
    }

    #[test]
    fn a_fresh_generation_missing_a_difficulty_is_an_error() {
        let active = chart_file(120.0, |_| 0);
        let mut fresh = chart_file(120.0, |_| 3);
        fresh.charts.retain(|c| c.difficulty != Difficulty::Expert);
        assert!(merged_redesign(&active, &fresh, 7).is_err());
    }

    #[test]
    fn an_unchanged_redesign_hashes_like_its_parent() {
        // Same hard + expert as the active version: the caller uses
        // hash equality to skip the write, and provenance must not
        // defeat it (chart_hash deliberately strips it).
        let active = chart_file(120.0, |_| 0);
        let fresh = chart_file(120.0, |_| 0);
        let merged = merged_redesign(&active, &fresh, 7).expect("the merge must succeed");
        assert_eq!(chart_hash(&merged), chart_hash(&active));
    }
}
