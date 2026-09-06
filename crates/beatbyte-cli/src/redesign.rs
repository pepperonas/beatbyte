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
    AudioTrim, ChartFile, GenerateMeta, Provenance, Severity, chart_hash, generate_chart, versions,
};
use beatbyte_core::Difficulty;

/// The two difficulties the rollout regenerates.
const REDESIGNED: [Difficulty; 2] = [Difficulty::Hard, Difficulty::Expert];

/// Tempo ratio between the active chart and the fresh analysis
/// beyond which the redesign refuses: mixing readings from two
/// different beat grids is not a redesign, it is a collision. Two
/// readings of the SAME grid differ by well under this — the
/// tracker's autocorrelation against the meter's median interval
/// (ADR-0015) measured ≤ 0.9 BPM apart on the library — while a
/// different metrical level differs by a third or more, and the
/// carried notes are moved onto the fresh grid note by note anyway.
const BPM_RATIO_TOLERANCE: f64 = 0.05;

/// The active version's song block and easy/medium, the fresh
/// generation's hard/expert, provenance binding the result to its
/// parent. Pure — the whole decision, no filesystem.
pub fn merged_redesign(
    active: &ChartFile,
    fresh: &ChartFile,
    created_ms: u64,
) -> Result<ChartFile, String> {
    if active.song.bpm <= 0.0
        || (fresh.song.bpm / active.song.bpm - 1.0).abs() > BPM_RATIO_TOLERANCE
    {
        return Err(format!(
            "the fresh analysis reads {:.2} BPM where the active chart says {:.2} — \
             two different beat grids cannot merge",
            fresh.song.bpm, active.song.bpm
        ));
    }
    let mut merged = active.clone();
    // The tracked grid is the fresh analysis's, like hard and expert.
    // The carried difficulties were placed on whatever grid their
    // version had — the constant one, before v0.14.30 — and are moved
    // onto the tracked grid by at most the snap tolerance: what they
    // are (lane, tail, which hits) does not change, where a hit sits
    // inside the beat does, by up to 55 ms, toward the audio.
    merged.grid = fresh.grid.clone();
    if let Some(grid) = merged.grid.as_ref() {
        for chart in &mut merged.charts {
            if !REDESIGNED.contains(&chart.difficulty) {
                grid.snap_notes(&mut chart.notes, beatbyte_chart::grid::SNAP_TOLERANCE_S);
            }
        }
    }
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

/// Whether `merged` would change anything the active file does not
/// already say — decided on what a reader would GET, not on what the
/// generator produced. Pure — tested.
///
/// ⚠️ The active chart was loaded from disk, the merged one carries
/// freshly generated numbers, and the reader moves a float by one
/// ULP on load (`serde_json` without `float_roundtrip`; 197 of a
/// chart's lines came back different on one round trip). Compared
/// as generated, the two never hash alike, and this check was dead
/// for as long as it existed: every `redesign --all` wrote a new
/// version of every song, most of them byte-identical to their
/// parent but for the provenance. So the merged chart is sent
/// through the same save → load path first; what survives that is
/// what the game will play.
#[must_use]
pub fn is_current(active: &ChartFile, merged: &ChartFile) -> bool {
    let as_read = merged
        .to_json_pretty()
        .ok()
        .and_then(|json| ChartFile::from_json(&json).ok());
    as_read.is_some_and(|merged| chart_hash(&merged) == chart_hash(active))
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
    let mut active =
        ChartFile::from_json(&text).map_err(|error| format!("`{active_name}`: {error}"))?;

    let audio_path = folder.join(&active.song.audio);
    let audio = decode_file(&audio_path)
        .map_err(|error| format!("cannot decode `{}`: {error}", audio_path.display()))?;
    // One timeline for the merge: the fresh charts come from a decode
    // that skipped the container's priming; an active file from
    // before the skip is moved onto the same axis first.
    let priming = audio.priming();
    let trim = AudioTrim::declared(priming.samples, priming.timescale, audio.sample_rate());
    active.retime(trim);
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    crate::meter(&mut analysis, &audio);
    // How identically the repeated sections play, before and after —
    // the measure behind "charted identically" (C4), per folder.
    if !analysis.repeats.is_empty() {
        let consistency = |chart: &ChartFile| {
            chart
                .charts
                .iter()
                .find(|d| d.difficulty == Difficulty::Expert)
                .and_then(|d| {
                    beatbyte_chart::repeat_consistency(
                        &d.notes,
                        &analysis.beats,
                        &analysis.repeats,
                        analysis.duration_s,
                        0.001,
                    )
                })
        };
        let covered: usize = analysis.repeats.iter().map(|r| 2 * r.beats).sum();
        eprintln!(
            "repeats: {} covering {:.0} % of the beats; expert consistency {} before",
            analysis.repeats.len(),
            100.0 * covered as f64 / analysis.beats.len().max(1) as f64,
            consistency(&active).map_or("n/a".to_owned(), |c| format!("{c:.2}"))
        );
    }
    let mut fresh = generate_chart(
        &analysis,
        &GenerateMeta {
            title: active.song.title.clone(),
            artist: active.song.artist.clone(),
            audio: active.song.audio.clone(),
        },
    );
    fresh.audio_trim = Some(trim);

    let created_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    let merged = merged_redesign(&active, &fresh, created_ms)?;
    if is_current(&active, &merged) {
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
            audio_trim: None,
            grid: None,
        }
    }

    #[test]
    fn a_chart_is_current_when_a_reader_would_get_the_same_file_back() {
        // A time that does not survive a save → load round trip: the
        // reader returns it one ULP off (measured on a real chart,
        // where 197 lines changed). The active file is what a reader
        // got; the fresh generation is the exact number.
        let drifting = 12.584_606_736_510_647_f64;
        let mut fresh = chart_file(120.0, |_| 0);
        fresh.charts[0].notes[0].time = drifting;
        let loaded =
            ChartFile::from_json(&fresh.to_json_pretty().expect("json")).expect("reads back");
        assert_ne!(
            loaded.charts[0].notes[0].time.to_bits(),
            drifting.to_bits(),
            "the fixture must drift, or it proves nothing"
        );
        // As generated, the two never hash alike — the dead check.
        assert_ne!(chart_hash(&fresh), chart_hash(&loaded));
        // As read, they are the same chart.
        assert!(is_current(&loaded, &fresh));
        // And a real change is still seen.
        let mut changed = fresh.clone();
        changed.charts[0].notes[1].lane = 4;
        assert!(!is_current(&loaded, &changed));
    }

    #[test]
    fn the_merge_takes_the_fresh_grid_and_moves_the_carried_notes_onto_it() {
        use beatbyte_chart::grid::BeatGrid;
        // The active version was placed on a constant 0.5 s grid; the
        // fresh analysis tracked a grid that has drifted 40 ms by the
        // fourth beat — inside the snap tolerance, so the beat wins.
        let mut active = chart_file(120.0, |_| 0);
        active.charts[1].notes = vec![note(1.0, 0), note(2.5, 1)]; // medium
        let mut fresh = chart_file(120.0, |_| 3);
        fresh.grid = Some(BeatGrid::from_beats(&[1.0, 1.5, 2.02, 2.54, 3.08]));
        let merged = merged_redesign(&active, &fresh, 7).expect("merges");
        assert_eq!(merged.grid, fresh.grid, "the grid is the analysis's");
        let medium = merged.chart_for(Difficulty::Medium).expect("medium");
        // 1.0 sits on the grid; 2.5 was a beat on the constant grid and
        // is 40 ms off the tracked beat at 2.54 — moved onto it, lane kept.
        assert!((medium.notes[0].time - 1.0).abs() < 1e-9);
        assert!(
            (medium.notes[1].time - 2.54).abs() < 1e-9,
            "{:?}",
            medium.notes[1]
        );
        assert_eq!(medium.notes[1].lane, 1);
        // Hard came from the fresh generation untouched.
        assert_eq!(
            merged.chart_for(Difficulty::Hard).expect("hard").notes,
            fresh.chart_for(Difficulty::Hard).expect("hard").notes
        );
        // A fresh generation without a grid leaves the carried notes
        // where they were.
        let plain = chart_file(120.0, |_| 3);
        let merged = merged_redesign(&active, &plain, 7).expect("merges");
        assert!(merged.grid.is_none());
        assert!(
            (merged.chart_for(Difficulty::Medium).expect("m").notes[1].time - 2.5).abs() < 1e-9
        );
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
        // Another metrical level — half, double, a third off — is a
        // different grid.
        for level in [60.0, 180.0, 240.0, 127.0] {
            let fresh = chart_file(level, |_| 3);
            let error = merged_redesign(&active, &fresh, 7).expect_err("the merge must refuse");
            assert!(error.contains("beat grids"), "{error}");
        }
        // ...while another READING of the same grid passes: the
        // meter's median interval against the tracker's tempo
        // (measured ≤ 0.9 BPM apart), and measurement noise.
        for reading in [120.05, 120.9, 119.1] {
            let close = chart_file(reading, |_| 3);
            assert!(merged_redesign(&active, &close, 7).is_ok(), "{reading}");
        }
        let zero = chart_file(0.0, |_| 0);
        assert!(
            merged_redesign(&zero, &active, 7).is_err(),
            "no tempo is no grid"
        );
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
