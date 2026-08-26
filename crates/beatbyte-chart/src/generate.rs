//! Automatic chart generation: musical events → playable charts.
//!
//! The generator consumes a [`SongAnalysis`] (produced by
//! `beatbyte-audio`, but any source works — the tests build them by
//! hand) and emits one chart per difficulty. It is **deterministic**:
//! the same analysis always yields the same charts. There is no
//! randomness — variety comes from a hash of the note's own time.
//!
//! Design priorities, in order: correct timing → beat alignment →
//! musical consistency → playable patterns → sensible difficulty →
//! readability. Not every audio event becomes a note; difficulty
//! profiles quantize, filter and thin the onset stream, then assign
//! lanes by spectral brightness with jump limiting.

use beatbyte_core::Difficulty;
use beatbyte_core::lane::LANE_COUNT;
use beatbyte_core::music::SongAnalysis;

use crate::schema::{ChartDef, ChartFile, ChartNote, ChartPhrase, SongMeta};
use crate::{FORMAT_VERSION, validate::BPM_RANGE};

/// Per-difficulty generation parameters. Data, not code: tuning a
/// difficulty never touches generator logic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DifficultyProfile {
    /// The difficulty this profile generates.
    pub difficulty: Difficulty,
    /// Beat subdivisions notes may land on (1 = beats, 2 = eighths,
    /// 4 = sixteenths).
    pub grid_division: u32,
    /// Minimum spacing between kept notes in seconds.
    pub min_spacing_s: f64,
    /// Onsets weaker than this (0–1, relative) are ignored.
    pub strength_floor: f32,
    /// Number of lanes used, anchored at lane 0 (3 = lanes 0–2).
    pub lanes_used: u8,
    /// Onset strength at which a note becomes a chord (>1.0 = never).
    pub chord_threshold: f32,
    /// Maximum chord size.
    pub max_chord_size: u8,
    /// Whether fast single-note runs become HOPOs.
    pub hopos: bool,
    /// Single notes this close to their predecessor become HOPOs.
    pub hopo_max_gap_s: f64,
    /// Whether sustains are generated.
    pub sustains: bool,
    /// Minimum gap to the next note for a sustain to fit.
    pub sustain_min_gap_s: f64,
}

impl DifficultyProfile {
    /// The default profile for a difficulty.
    #[must_use]
    pub fn for_difficulty(difficulty: Difficulty) -> DifficultyProfile {
        match difficulty {
            Difficulty::Easy => DifficultyProfile {
                difficulty,
                grid_division: 1,
                min_spacing_s: 0.45,
                strength_floor: 0.35,
                lanes_used: 3,
                chord_threshold: 2.0,
                max_chord_size: 1,
                hopos: false,
                hopo_max_gap_s: 0.0,
                sustains: true,
                sustain_min_gap_s: 0.9,
            },
            Difficulty::Medium => DifficultyProfile {
                difficulty,
                grid_division: 2,
                min_spacing_s: 0.28,
                strength_floor: 0.22,
                lanes_used: 4,
                chord_threshold: 0.85,
                max_chord_size: 2,
                hopos: false,
                hopo_max_gap_s: 0.0,
                sustains: true,
                sustain_min_gap_s: 0.6,
            },
            Difficulty::Hard => DifficultyProfile {
                difficulty,
                grid_division: 4,
                min_spacing_s: 0.16,
                strength_floor: 0.14,
                lanes_used: 5,
                chord_threshold: 0.75,
                max_chord_size: 2,
                hopos: true,
                hopo_max_gap_s: 0.20,
                sustains: true,
                sustain_min_gap_s: 0.5,
            },
            Difficulty::Expert => DifficultyProfile {
                difficulty,
                grid_division: 4,
                min_spacing_s: 0.10,
                strength_floor: 0.07,
                lanes_used: 5,
                chord_threshold: 0.62,
                max_chord_size: 3,
                hopos: true,
                hopo_max_gap_s: 0.22,
                sustains: true,
                sustain_min_gap_s: 0.45,
            },
        }
    }
}

/// Metadata for the song being charted.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateMeta {
    /// Song title.
    pub title: String,
    /// Artist name.
    pub artist: String,
    /// Relative audio path to embed in the chart.
    pub audio: String,
}

/// Generate a complete chart file (all four difficulties) from an
/// analysis.
#[must_use]
pub fn generate_chart(analysis: &SongAnalysis, meta: &GenerateMeta) -> ChartFile {
    let bpm = analysis.bpm.clamp(*BPM_RANGE.start(), *BPM_RANGE.end());
    let offset_s = analysis.beats.first().copied().unwrap_or(0.0);

    let charts = Difficulty::ALL
        .iter()
        .map(|&difficulty| {
            generate_difficulty(
                analysis,
                &DifficultyProfile::for_difficulty(difficulty),
                offset_s,
            )
        })
        .collect();

    ChartFile {
        format_version: FORMAT_VERSION,
        song: SongMeta {
            title: meta.title.clone(),
            artist: meta.artist.clone(),
            audio: meta.audio.clone(),
            bpm,
            offset_s: offset_s.clamp(-59.0, 59.0),
            preview_start_s: preview_start(analysis),
            duration_s: Some(analysis.duration_s),
        },
        charts,
    }
}

/// Generate one difficulty's chart.
#[must_use]
pub fn generate_difficulty(
    analysis: &SongAnalysis,
    profile: &DifficultyProfile,
    grid_origin_s: f64,
) -> ChartDef {
    let kept = select_onsets(analysis, profile, grid_origin_s);
    let notes = place_notes(analysis, profile, &kept);
    let phrases = place_phrases(analysis, &notes);
    ChartDef {
        difficulty: profile.difficulty,
        lanes: LANE_COUNT as u8,
        notes,
        phrases,
    }
}

/// A selected, quantized onset ready for note placement.
#[derive(Debug, Clone, Copy)]
struct Selected {
    time_s: f64,
    strength: f32,
    brightness: f32,
}

/// Quantize, filter and thin the onset stream.
fn select_onsets(
    analysis: &SongAnalysis,
    profile: &DifficultyProfile,
    grid_origin_s: f64,
) -> Vec<Selected> {
    let beat = analysis.beat_interval_s();
    let step = beat / f64::from(profile.grid_division.max(1));
    // Snap to the grid only when close; otherwise trust the onset (the
    // grid may be imperfect, the music never is).
    let snap_window = (step * 0.3).min(0.07);

    let mut kept: Vec<Selected> = Vec::new();
    for onset in &analysis.onsets {
        if onset.strength < profile.strength_floor {
            continue;
        }
        let time_s = quantize(onset.time_s, grid_origin_s, step, snap_window);
        if time_s < 0.0 || time_s > analysis.duration_s {
            continue;
        }
        if let Some(last) = kept.last()
            && time_s - last.time_s < profile.min_spacing_s
        {
            continue;
        }
        kept.push(Selected {
            time_s,
            strength: onset.strength,
            brightness: onset.brightness,
        });
    }
    kept
}

/// Snap a time to the nearest grid point when within the snap window.
fn quantize(time_s: f64, origin_s: f64, step: f64, snap_window: f64) -> f64 {
    if step <= 0.0 {
        return time_s;
    }
    let position = (time_s - origin_s) / step;
    let snapped = origin_s + position.round() * step;
    if (snapped - time_s).abs() <= snap_window {
        snapped
    } else {
        time_s
    }
}

/// Assign lanes, chords, HOPOs and sustains.
fn place_notes(
    analysis: &SongAnalysis,
    profile: &DifficultyProfile,
    kept: &[Selected],
) -> Vec<ChartNote> {
    let lanes_used = i32::from(profile.lanes_used.clamp(1, LANE_COUNT as u8));
    let mut notes: Vec<ChartNote> = Vec::new();
    let mut previous_lane: Option<i32> = None;

    for (i, selected) in kept.iter().enumerate() {
        // Brightness maps to a base lane; a per-note deterministic
        // wiggle keeps identical-brightness runs from freezing on one
        // lane; jump limiting keeps patterns playable.
        let base = (f64::from(selected.brightness) * f64::from(lanes_used)).floor() as i32;
        let wiggle = (note_hash(selected.time_s) % 3) as i32 - 1; // −1, 0, +1
        let mut lane = (base + wiggle).clamp(0, lanes_used - 1);
        if let Some(prev) = previous_lane {
            lane = lane.clamp(prev - 2, prev + 2).clamp(0, lanes_used - 1);
            // Fast runs feel better when they move.
            let gap = selected.time_s - notes.last().map_or(f64::NEG_INFINITY, |n| n.time);
            if lane == prev && gap < profile.min_spacing_s * 2.0 {
                lane = if prev + 1 < lanes_used {
                    prev + 1
                } else {
                    prev - 1
                }
                .max(0);
            }
        }

        // Sustain: the ENERGY decides (a held tone keeps ringing and
        // nothing new strikes); the gap to the next note only bounds
        // the length. The old rule required near-silence after the
        // note (gap >= 0.8-1.2 s) — dense live mixes never have that,
        // and a 428-second live track came out with 3 sustains.
        let next_time = kept.get(i + 1).map_or(analysis.duration_s, |n| n.time_s);
        let gap_to_next = next_time - selected.time_s;
        let mut len = 0.0;
        if profile.sustains && gap_to_next >= profile.sustain_min_gap_s {
            let mut candidate = (gap_to_next - 0.25).min(analysis.beat_interval_s() * 4.0);
            // A strong fresh onset inside the window ends the held
            // tone — cut the sustain just before it. "Strong" is an
            // ABSOLUTE bar: measured relative to the held note, a
            // live mix's reverb/crowd rumble cut almost everything
            // (Rick's medium chart dropped 53 -> 37 sustains).
            let cutoff = 0.5;
            for onset in &analysis.onsets {
                if onset.time_s <= selected.time_s + 0.05 {
                    continue;
                }
                if onset.time_s >= selected.time_s + candidate {
                    break;
                }
                if onset.strength >= cutoff {
                    candidate = onset.time_s - selected.time_s - 0.1;
                    break;
                }
            }
            if candidate > 0.3 && energy_carries(analysis, selected.time_s, candidate) {
                len = candidate;
            }
        }

        // HOPO: fast single-note runs that change lanes.
        let gap_to_prev = notes
            .last()
            .map_or(f64::INFINITY, |n| selected.time_s - n.time);
        let hopo =
            profile.hopos && gap_to_prev <= profile.hopo_max_gap_s && previous_lane != Some(lane);

        // Chord: strong, well-spaced hits widen.
        let chord_size = if selected.strength >= profile.chord_threshold
            && gap_to_prev >= profile.min_spacing_s * 2.0
            && len == 0.0
        {
            let extra = 1 + (note_hash(selected.time_s + 0.5) % 2) as u8;
            (1 + extra).min(profile.max_chord_size)
        } else {
            1
        };

        for c in 0..i32::from(chord_size) {
            let chord_lane = if lane + c < lanes_used {
                lane + c
            } else {
                lane - c
            };
            notes.push(ChartNote {
                time: selected.time_s,
                lane: chord_lane.clamp(0, lanes_used - 1) as u8,
                len,
                hopo: hopo && chord_size == 1,
            });
        }
        previous_lane = Some(lane);
    }

    dedupe_same_lane(&mut notes);
    notes
}

/// Whether the energy envelope stays alive over the sustain span.
fn energy_carries(analysis: &SongAnalysis, start_s: f64, length_s: f64) -> bool {
    let reference = analysis.energy_at(start_s).max(0.05);
    for i in 1..=4 {
        let probe = start_s + length_s * f64::from(i) / 4.0;
        if analysis.energy_at(probe) < reference * 0.4 {
            return false;
        }
    }
    true
}

/// Special phrases: recurring two-bar windows in note-dense regions.
fn place_phrases(analysis: &SongAnalysis, notes: &[ChartNote]) -> Vec<ChartPhrase> {
    let bar = analysis.beat_interval_s() * 4.0;
    if bar <= 0.0 || notes.is_empty() {
        return Vec::new();
    }
    let mut phrases = Vec::new();
    // A phrase candidate every 8 bars, lasting 2 bars.
    let stride = bar * 8.0;
    let mut start = stride; // never in the very first bars
    while start + bar * 2.0 < analysis.duration_s {
        let end = start + bar * 2.0;
        let count = notes
            .iter()
            .filter(|n| n.time >= start && n.time <= end)
            .count();
        if count >= 4 {
            phrases.push(ChartPhrase { start, end });
        }
        start += stride;
    }
    phrases
}

/// The song-browser preview: the loudest 10-second window start.
fn preview_start(analysis: &SongAnalysis) -> Option<f64> {
    if analysis.energy.is_empty() || analysis.energy_hop_s <= 0.0 {
        return None;
    }
    let window = (10.0 / analysis.energy_hop_s) as usize;
    if analysis.energy.len() <= window {
        return Some(0.0);
    }
    let mut best_start = 0usize;
    let mut best_sum = f32::NEG_INFINITY;
    let mut sum: f32 = analysis.energy[..window].iter().sum();
    let mut current = 0usize;
    loop {
        if sum > best_sum {
            best_sum = sum;
            best_start = current;
        }
        if current + window >= analysis.energy.len() {
            break;
        }
        sum += analysis.energy[current + window] - analysis.energy[current];
        current += 1;
    }
    Some(best_start as f64 * analysis.energy_hop_s)
}

/// Remove accidental same-lane duplicates at the same instant
/// (possible when chord widening folds back).
fn dedupe_same_lane(notes: &mut Vec<ChartNote>) {
    notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.lane.cmp(&b.lane)));
    notes.dedup_by(|a, b| (a.time - b.time).abs() < 1e-9 && a.lane == b.lane);
}

/// Deterministic per-note hash (splitmix-style) — variety without
/// randomness.
fn note_hash(time_s: f64) -> u64 {
    let mut x = (time_s * 1_000.0).round() as i64 as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::validate::Severity;
    use beatbyte_core::music::Onset;

    /// A synthetic 120 BPM analysis: onsets on every eighth note for
    /// 60 s (strength alternating strong beats / weak offbeats,
    /// brightness sweeping low→high per bar), followed by a
    /// sixteenth-note run — the terrain auto-HOPO rules exist for.
    fn analysis() -> SongAnalysis {
        let mut onsets = Vec::new();
        let beat = 0.5;
        for i in 0..240 {
            let time_s = 1.0 + i as f64 * beat / 2.0;
            let on_beat = i % 2 == 0;
            onsets.push(Onset {
                time_s,
                strength: if on_beat { 0.9 } else { 0.3 },
                brightness: (i % 16) as f32 / 16.0,
            });
        }
        // Sixteenth-note run: 62.0 s .. 64.0 s.
        for k in 0..16 {
            onsets.push(Onset {
                time_s: 62.0 + k as f64 * beat / 4.0,
                strength: 0.5,
                brightness: 0.2 + 0.05 * (k % 8) as f32,
            });
        }
        let beats: Vec<f64> = (0..136).map(|i| 1.0 + i as f64 * beat).collect();
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.8,
            alt_bpm: None,
            beats,
            onsets,
            energy: vec![0.8; 1400],
            energy_hop_s: 0.05,
            duration_s: 68.0,
        }
    }

    fn meta() -> GenerateMeta {
        GenerateMeta {
            title: "Synth Test".into(),
            artist: "BeatByte".into(),
            audio: "synth.ogg".into(),
        }
    }

    #[test]
    fn generated_chart_is_valid() {
        let chart = generate_chart(&analysis(), &meta());
        let errors: Vec<_> = chart
            .validate()
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(chart.charts.len(), 4);
        // And it converts into playable tracks.
        assert_eq!(chart.to_tracks().unwrap().len(), 4);
    }

    #[test]
    fn difficulty_orders_note_density() {
        let chart = generate_chart(&analysis(), &meta());
        let count = |d: Difficulty| chart.chart_for(d).unwrap().notes.len();
        assert!(
            count(Difficulty::Easy) < count(Difficulty::Medium),
            "easy {} < medium {}",
            count(Difficulty::Easy),
            count(Difficulty::Medium)
        );
        assert!(
            count(Difficulty::Medium) < count(Difficulty::Hard),
            "medium {} < hard {}",
            count(Difficulty::Medium),
            count(Difficulty::Hard)
        );
        assert!(count(Difficulty::Hard) <= count(Difficulty::Expert));
    }

    #[test]
    fn easy_stays_on_low_lanes() {
        let chart = generate_chart(&analysis(), &meta());
        for note in &chart.chart_for(Difficulty::Easy).unwrap().notes {
            assert!(note.lane < 3, "easy must use lanes 0–2, got {}", note.lane);
        }
    }

    #[test]
    fn expert_uses_the_full_neck() {
        let chart = generate_chart(&analysis(), &meta());
        let lanes: std::collections::HashSet<u8> = chart
            .chart_for(Difficulty::Expert)
            .unwrap()
            .notes
            .iter()
            .map(|n| n.lane)
            .collect();
        assert!(
            lanes.len() >= 4,
            "expert should spread over lanes: {lanes:?}"
        );
    }

    #[test]
    fn min_spacing_is_respected() {
        let chart = generate_chart(&analysis(), &meta());
        for difficulty in Difficulty::ALL {
            let profile = DifficultyProfile::for_difficulty(difficulty);
            let notes = &chart.chart_for(difficulty).unwrap().notes;
            let mut times: Vec<f64> = notes.iter().map(|n| n.time).collect();
            times.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            for pair in times.windows(2) {
                assert!(
                    pair[1] - pair[0] >= profile.min_spacing_s - 1e-9,
                    "{difficulty}: spacing {} < {}",
                    pair[1] - pair[0],
                    profile.min_spacing_s
                );
            }
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_chart(&analysis(), &meta());
        let b = generate_chart(&analysis(), &meta());
        assert_eq!(a, b);
    }

    #[test]
    fn strong_onsets_become_chords_on_expert() {
        let chart = generate_chart(&analysis(), &meta());
        let notes = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        let mut by_time: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for note in notes {
            *by_time
                .entry((note.time * 1000.0).round() as i64)
                .or_default() += 1;
        }
        assert!(
            by_time.values().any(|&count| count > 1),
            "expert should contain chords"
        );
        // But easy must not.
        let easy = &chart.chart_for(Difficulty::Easy).unwrap().notes;
        let mut easy_by_time: std::collections::HashMap<i64, usize> =
            std::collections::HashMap::new();
        for note in easy {
            *easy_by_time
                .entry((note.time * 1000.0).round() as i64)
                .or_default() += 1;
        }
        assert!(easy_by_time.values().all(|&count| count == 1));
    }

    #[test]
    fn fast_runs_become_hopos_on_expert_only() {
        let chart = generate_chart(&analysis(), &meta());
        let expert_hopos = chart
            .chart_for(Difficulty::Expert)
            .unwrap()
            .notes
            .iter()
            .filter(|n| n.hopo)
            .count();
        assert!(expert_hopos > 0, "expert should contain HOPOs");
        let easy_hopos = chart
            .chart_for(Difficulty::Easy)
            .unwrap()
            .notes
            .iter()
            .filter(|n| n.hopo)
            .count();
        assert_eq!(easy_hopos, 0);
    }

    #[test]
    fn a_strong_fresh_onset_truncates_the_sustain() {
        // A held tone ends when something new strikes: an onset at or
        // above the ABSOLUTE 0.5 bar inside the sustain window cuts
        // the tail just before it (weaker ones — reverb, crowd — do
        // not; that relative-measure bug once cost a live track most
        // of its sustains).
        let mk = |breaker_strength: f32| {
            let onsets = vec![
                Onset {
                    time_s: 1.0,
                    strength: 0.9,
                    brightness: 0.2,
                },
                Onset {
                    time_s: 1.8,
                    strength: breaker_strength,
                    brightness: 0.5,
                },
                Onset {
                    time_s: 4.5,
                    strength: 0.9,
                    brightness: 0.2,
                },
            ];
            let a = SongAnalysis {
                bpm: 120.0,
                bpm_confidence: 0.8,
                alt_bpm: None,
                beats: (0..20).map(|i| f64::from(i) * 0.5).collect(),
                onsets,
                energy: vec![0.8; 200],
                energy_hop_s: 0.05,
                duration_s: 10.0,
            };
            let file = generate_chart(&a, &meta());
            let medium = file
                .charts
                .iter()
                .find(|c| c.difficulty == Difficulty::Medium)
                .unwrap();
            medium
                .notes
                .iter()
                .find(|n| (n.time - 1.0).abs() < 0.1)
                .map(|n| n.len)
                .unwrap_or(0.0)
        };
        let cut = mk(0.6);
        let uncut = mk(0.2);
        assert!(
            cut > 0.0 && cut <= 0.75,
            "strong onset at +0.8s must cap the sustain near 0.7s: {cut}"
        );
        assert!(
            uncut > cut,
            "a weak onset must NOT truncate: {uncut} vs {cut}"
        );
    }

    #[test]
    fn sustains_appear_where_energy_carries() {
        // Sparse strong onsets with big gaps and constant energy.
        let onsets: Vec<Onset> = (0..20)
            .map(|i| Onset {
                time_s: 1.0 + i as f64 * 2.0,
                strength: 0.9,
                brightness: 0.4,
            })
            .collect();
        let a = SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.8,
            alt_bpm: None,
            beats: (0..80).map(|i| i as f64 * 0.5).collect(),
            onsets,
            energy: vec![0.8; 900],
            energy_hop_s: 0.05,
            duration_s: 45.0,
        };
        let chart = generate_chart(&a, &meta());
        let sustained = chart
            .chart_for(Difficulty::Expert)
            .unwrap()
            .notes
            .iter()
            .filter(|n| n.len > 0.0)
            .count();
        assert!(sustained > 10, "expected sustains, got {sustained}");
    }

    #[test]
    fn phrases_are_placed_in_dense_regions() {
        let chart = generate_chart(&analysis(), &meta());
        let phrases = &chart.chart_for(Difficulty::Expert).unwrap().phrases;
        assert!(!phrases.is_empty(), "a 65 s dense song should have phrases");
        for pair in phrases.windows(2) {
            assert!(pair[1].start > pair[0].end, "phrases must not overlap");
        }
    }

    #[test]
    fn empty_analysis_produces_valid_empty_charts() {
        let a = SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.0,
            alt_bpm: None,
            beats: vec![],
            onsets: vec![],
            energy: vec![],
            energy_hop_s: 0.05,
            duration_s: 30.0,
        };
        let chart = generate_chart(&a, &meta());
        let errors: Vec<_> = chart
            .validate()
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn quantize_snaps_only_within_window() {
        assert!((quantize(1.02, 0.0, 0.5, 0.07) - 1.0).abs() < 1e-9);
        assert!((quantize(1.2, 0.0, 0.5, 0.07) - 1.2).abs() < 1e-9);
    }
}
