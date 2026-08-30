//! `beatbyte-cli dossier` — layer 3 of adaptive charting (ADR-0011).
//!
//! One self-contained file per song: everything a design session
//! needs to redesign a chart without touching the game. The musical
//! representation (melody notes, a per-bar structure table), the
//! full active chart, the playability constraints per difficulty,
//! the open directives from the review — and the mechanical facts of
//! writing the result: the next version name and the parent hash the
//! provenance must carry.
//!
//! Pure assembly over parts the caller loaded; IO lives in `main.rs`.

use beatbyte_chart::generate::DifficultyProfile;
use beatbyte_chart::{ChartFile, chart_hash};
use beatbyte_core::{Difficulty, SongAnalysis};
use serde::Serialize;

use crate::review::{Directive, Thresholds};

/// One bar of the song, read for structure: how busy and how loud.
#[derive(Debug, Clone, Serialize)]
pub struct BarSummary {
    /// Bar index, 0-based.
    pub bar: u32,
    /// Song time the bar starts, seconds.
    pub time_s: f64,
    /// Detected onsets in the bar.
    pub onsets: usize,
    /// Mean normalized energy in the bar (0-1).
    pub energy: f32,
    /// Melody notes starting in the bar.
    pub melody_notes: usize,
}

/// A melody note, trimmed to what a designer reads.
#[derive(Debug, Clone, Serialize)]
pub struct MelodyLine {
    /// Start, seconds.
    pub time_s: f64,
    /// Held length, seconds.
    pub held_s: f64,
    /// MIDI pitch (fractional).
    pub midi: f32,
    /// Relative salience 0-1.
    pub strength: f32,
}

/// The playability rules one difficulty is designed under.
#[derive(Debug, Clone, Serialize)]
pub struct ConstraintSummary {
    /// Difficulty these apply to.
    pub difficulty: String,
    /// Lanes in play.
    pub lanes: u8,
    /// Largest chord allowed.
    pub max_chord_size: u8,
    /// Minimum spacing between kept notes, seconds.
    pub min_spacing_s: f64,
    /// Target density the derivation aims for, notes per beat.
    pub target_notes_per_beat: f64,
    /// Whether HOPOs are generated, and up to which gap.
    pub hopo_max_gap_s: f64,
    /// Minimum gap to the next note for a sustain to fit, seconds.
    pub sustain_min_gap_s: f64,
}

/// What the design session must write when it is done.
#[derive(Debug, Clone, Serialize)]
pub struct WriteInstructions {
    /// File name the new version goes under, next to the chart.
    pub next_version_file: String,
    /// The hash the new file's provenance must carry as parent.
    pub parent_hash: String,
    /// The pointer file to update so the new version plays.
    pub pointer_file: String,
    /// The gate that outranks everything: the new version becomes
    /// active only after a by-ear A/B against the current one
    /// (ADR-0009 — metrics are a guard, the ear is the arbiter).
    pub gate: String,
}

/// The dossier: everything in one place.
#[derive(Debug, Clone, Serialize)]
pub struct Dossier {
    /// What this file is, for anyone who finds it.
    pub what: String,
    /// Difficulty under design.
    pub difficulty: String,
    /// The active chart, complete — the base the redesign edits.
    pub chart: ChartFile,
    /// Identity of that chart (what telemetry binds to).
    pub chart_hash: String,
    /// Per-bar structure: onsets, energy, melody density.
    pub bars: Vec<BarSummary>,
    /// The extracted lead melody — the playable truth the master
    /// chart was built from.
    pub melody: Vec<MelodyLine>,
    /// Tempo the analysis measured (may disagree with the chart's).
    pub analysis_bpm: f64,
    /// Confidence of that measurement, 0-1.
    pub analysis_bpm_confidence: f64,
    /// Playability constraints per difficulty.
    pub constraints: Vec<ConstraintSummary>,
    /// Open directives from the review (empty = no evidence yet).
    pub directives: Vec<Directive>,
    /// The mechanics of writing the result.
    pub write: WriteInstructions,
}

/// Assemble the dossier from loaded parts.
#[must_use]
pub fn assemble(
    chart: ChartFile,
    analysis: &SongAnalysis,
    difficulty: Difficulty,
    directives: Vec<Directive>,
    next_version_file: String,
) -> Dossier {
    let hash = chart_hash(&chart);
    let bar_s = 240.0 / chart.song.bpm.clamp(20.0, 400.0);
    let offset = chart.song.offset_s;
    let duration = chart.song.duration_s.unwrap_or(analysis.duration_s);
    let bar_count = (((duration - offset) / bar_s).ceil().max(0.0)) as u32;

    let bars = (0..bar_count)
        .map(|bar| {
            let start = f64::from(bar).mul_add(bar_s, offset);
            let end = start + bar_s;
            let onsets = analysis
                .onsets
                .iter()
                .filter(|o| o.time_s >= start && o.time_s < end)
                .count();
            let melody_notes = analysis
                .melody
                .iter()
                .filter(|m| m.time_s >= start && m.time_s < end)
                .count();
            // Energy samples covering the bar.
            let hop = analysis.energy_hop_s.max(1e-6);
            let from = ((start / hop).floor().max(0.0)) as usize;
            let to = (((end / hop).ceil()).max(0.0)) as usize;
            let window: Vec<f32> = analysis
                .energy
                .iter()
                .skip(from)
                .take(to.saturating_sub(from))
                .copied()
                .collect();
            let energy = if window.is_empty() {
                0.0
            } else {
                window.iter().sum::<f32>() / window.len() as f32
            };
            BarSummary {
                bar,
                time_s: start,
                onsets,
                energy,
                melody_notes,
            }
        })
        .collect();

    let melody = analysis
        .melody
        .iter()
        .map(|note| MelodyLine {
            time_s: note.time_s,
            held_s: (note.end_s - note.time_s).max(0.0),
            midi: note.midi,
            strength: note.strength,
        })
        .collect();

    let constraints = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ]
    .into_iter()
    .map(|d| {
        let profile = DifficultyProfile::for_difficulty(d);
        ConstraintSummary {
            difficulty: d.display_name().to_lowercase(),
            lanes: profile.lanes_used,
            max_chord_size: profile.max_chord_size,
            min_spacing_s: profile.min_spacing_s,
            target_notes_per_beat: profile.target_notes_per_beat,
            hopo_max_gap_s: if profile.hopos {
                profile.hopo_max_gap_s
            } else {
                0.0
            },
            sustain_min_gap_s: profile.sustain_min_gap_s,
        }
    })
    .collect();

    Dossier {
        what: "BeatByte design dossier (ADR-0011): everything a design \
               session needs to write the next chart version"
            .to_owned(),
        difficulty: difficulty.display_name().to_lowercase(),
        chart_hash: hash.clone(),
        bars,
        melody,
        analysis_bpm: analysis.bpm,
        analysis_bpm_confidence: analysis.bpm_confidence,
        constraints,
        directives,
        write: WriteInstructions {
            next_version_file,
            parent_hash: hash,
            pointer_file: beatbyte_chart::versions::POINTER_FILE.to_owned(),
            gate: "the new version becomes active only after a by-ear A/B \
                   against the current one (ADR-0009)"
                .to_owned(),
        },
        chart,
    }
}

/// The review thresholds a dossier run uses — one place, so the
/// dossier and a standalone review cannot quietly disagree.
#[must_use]
pub fn dossier_thresholds(min_sessions: usize) -> Thresholds {
    Thresholds {
        min_sessions,
        ..Thresholds::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::{ChartDef, ChartNote, SongMeta};
    use beatbyte_core::music::{MelodyNote, Onset};

    fn chart() -> ChartFile {
        ChartFile {
            format_version: 1,
            song: SongMeta {
                title: "T".to_owned(),
                artist: "A".to_owned(),
                audio: "t.wav".to_owned(),
                bpm: 120.0, // bar = 2 s
                offset_s: 0.0,
                preview_start_s: None,
                duration_s: Some(8.0), // 4 bars
            },
            charts: vec![ChartDef {
                difficulty: Difficulty::Medium,
                lanes: 5,
                notes: vec![ChartNote {
                    time: 1.0,
                    lane: 0,
                    len: 0.0,
                    hopo: false,
                }],
                phrases: Vec::new(),
            }],
            provenance: None,
        }
    }

    fn analysis() -> SongAnalysis {
        SongAnalysis {
            bpm: 119.5,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats: vec![0.0, 0.5, 1.0],
            onsets: vec![
                Onset {
                    time_s: 0.5,
                    strength: 1.0,
                    brightness: 0.5,
                },
                Onset {
                    time_s: 2.5,
                    strength: 0.5,
                    brightness: 0.5,
                },
                Onset {
                    time_s: 2.9,
                    strength: 0.4,
                    brightness: 0.5,
                },
            ],
            energy: vec![0.0; 80],
            energy_hop_s: 0.1,
            duration_s: 8.0,
            melody: vec![MelodyNote {
                time_s: 2.2,
                end_s: 2.8,
                midi: 64.0,
                strength: 1.0,
            }],
        }
    }

    #[test]
    fn the_bar_table_counts_what_falls_into_each_bar() {
        let dossier = assemble(
            chart(),
            &analysis(),
            Difficulty::Medium,
            Vec::new(),
            "chart.v2.json".to_owned(),
        );
        assert_eq!(dossier.bars.len(), 4, "8 s at 120 BPM is four bars");
        // Bar 0 (0-2 s): one onset, no melody. Bar 1 (2-4 s): two
        // onsets and the melody note.
        assert_eq!(dossier.bars[0].onsets, 1);
        assert_eq!(dossier.bars[0].melody_notes, 0);
        assert_eq!(dossier.bars[1].onsets, 2);
        assert_eq!(dossier.bars[1].melody_notes, 1);
    }

    #[test]
    fn the_write_instructions_bind_to_the_active_chart() {
        // The mechanical core: the provenance's parent MUST be the
        // hash of the chart the redesign started from, or the paper
        // trail lies and telemetry cannot follow the lineage.
        let base = chart();
        let expected = chart_hash(&base);
        let dossier = assemble(
            base,
            &analysis(),
            Difficulty::Medium,
            Vec::new(),
            "chart.v2.json".to_owned(),
        );
        assert_eq!(dossier.write.parent_hash, expected);
        assert_eq!(dossier.chart_hash, expected);
        assert_eq!(dossier.write.next_version_file, "chart.v2.json");
        assert!(dossier.write.gate.contains("by-ear"));
    }

    #[test]
    fn every_difficulty_has_its_constraints() {
        let dossier = assemble(
            chart(),
            &analysis(),
            Difficulty::Medium,
            Vec::new(),
            "chart.v2.json".to_owned(),
        );
        let names: Vec<&str> = dossier
            .constraints
            .iter()
            .map(|c| c.difficulty.as_str())
            .collect();
        assert_eq!(names, vec!["easy", "medium", "hard", "expert"]);
        // The values come from the generator's own profiles, not a
        // copy that could drift.
        let expert = &dossier.constraints[3];
        let profile = DifficultyProfile::for_difficulty(Difficulty::Expert);
        assert_eq!(expert.lanes, profile.lanes_used);
        assert_eq!(expert.max_chord_size, profile.max_chord_size);
    }

    #[test]
    fn melody_notes_carry_their_held_length() {
        let dossier = assemble(
            chart(),
            &analysis(),
            Difficulty::Medium,
            Vec::new(),
            "chart.v2.json".to_owned(),
        );
        assert_eq!(dossier.melody.len(), 1);
        assert!((dossier.melody[0].held_s - 0.6).abs() < 1e-9);
    }
}
