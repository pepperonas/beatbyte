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
    /// Onsets weaker than this (0–1, relative) are ignored. A hard
    /// floor only — the actual thinning is driven by
    /// [`DifficultyProfile::target_notes_per_beat`].
    pub strength_floor: f32,
    /// Target note density in notes per beat. The derivation picks
    /// the strength floor that lands on this number, so the SAME
    /// difficulty feels the same across songs. Absolute floors made
    /// the curve song-dependent: measured on five real imports, the
    /// easy→medium jump ranged from 1.4x to 3.6x.
    pub target_notes_per_beat: f64,
    /// Number of lanes used, anchored at lane 0 (3 = lanes 0–2).
    pub lanes_used: u8,
    /// Share of the difficulty's kept notes that strike as chords
    /// (0 = never): the song's OWN strongest hits are its accents,
    /// whatever the absolute scale. The strongest quarter of that
    /// share widens to three notes where the cap allows.
    pub chord_share: f32,
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
    /// Longest run of consecutive events under [`BURST_GAP_S`] this
    /// difficulty tolerates; longer streams relax their interior to
    /// every second note (sixteenths become eighths).
    pub max_burst_events: usize,
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
                strength_floor: 0.20,
                target_notes_per_beat: 0.35,
                lanes_used: 3,
                chord_share: 0.0,
                max_chord_size: 1,
                hopos: false,
                hopo_max_gap_s: 0.0,
                sustains: true,
                sustain_min_gap_s: 0.9,
                max_burst_events: 4,
            },
            Difficulty::Medium => DifficultyProfile {
                difficulty,
                grid_division: 2,
                min_spacing_s: 0.28,
                strength_floor: 0.12,
                target_notes_per_beat: 0.70,
                lanes_used: 4,
                chord_share: 0.05,
                max_chord_size: 2,
                hopos: false,
                hopo_max_gap_s: 0.0,
                sustains: true,
                sustain_min_gap_s: 0.6,
                max_burst_events: 8,
            },
            Difficulty::Hard => DifficultyProfile {
                difficulty,
                grid_division: 4,
                min_spacing_s: 0.16,
                strength_floor: 0.08,
                target_notes_per_beat: 1.30,
                lanes_used: 5,
                chord_share: 0.08,
                max_chord_size: 2,
                hopos: true,
                hopo_max_gap_s: 0.26,
                sustains: true,
                sustain_min_gap_s: 0.5,
                max_burst_events: 10,
            },
            Difficulty::Expert => DifficultyProfile {
                difficulty,
                grid_division: 4,
                min_spacing_s: 0.10,
                strength_floor: 0.05,
                target_notes_per_beat: 2.20,
                lanes_used: 5,
                chord_share: 0.12,
                max_chord_size: 3,
                hopos: true,
                hopo_max_gap_s: 0.22,
                sustains: true,
                sustain_min_gap_s: 0.45,
                max_burst_events: 24,
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
            genre: None,
        },
        charts,
        provenance: None,
    }
}

/// Generate one difficulty's chart.
///
/// Every difficulty derives from ONE master (the official-charting
/// workflow): the same musical event keeps the same lane and the
/// same tail on every difficulty — leveling up must feel like the
/// same song with more of it, never a different chart.
#[must_use]
pub fn generate_difficulty(
    analysis: &SongAnalysis,
    profile: &DifficultyProfile,
    grid_origin_s: f64,
) -> ChartDef {
    let master = build_master(analysis, grid_origin_s);
    let notes = derive_notes(analysis, profile, &master, grid_origin_s);
    let phrases = place_phrases(analysis, &notes);
    ChartDef {
        difficulty: profile.difficulty,
        lanes: LANE_COUNT as u8,
        notes,
        phrases,
    }
}

/// One event of the master chart: the single authored truth every
/// difficulty derives from. Lanes live on the full five-lane neck;
/// `held_s` is the tail's natural length before per-difficulty
/// capping (0 = a tap).
#[derive(Debug, Clone, Copy)]
struct MasterNote {
    time_s: f64,
    strength: f32,
    lane: i32,
    held_s: f64,
    pitched: bool,
}

/// Master selection parameters: expert-tight spacing and a
/// near-open strength floor. Difficulties THIN this — they never
/// re-select. (The grid division that used to live here is gone: the
/// quantizer now picks the subdivision per hit, see
/// [`quantize_musical`].)
const MASTER_MIN_SPACING_S: f64 = 0.10;
const MASTER_STRENGTH_FLOOR: f32 = 0.07;

/// Below this gap, two notes on the SAME lane are a machine-gun jack
/// — physically brutal where a trill is easy. Real charts alternate
/// lanes for fast repeated pitches; so does the flow pass.
const JACK_GAP_S: f64 = 0.18;

/// Consecutive events closer than this form a burst (a stream);
/// [`DifficultyProfile::max_burst_events`] caps how long one may run.
/// Sixteenths at 120 BPM sit at exactly 0.125 s — the threshold must
/// clear them, or the whole discipline misses its main customer.
const BURST_GAP_S: f64 = 0.13;

/// Build the master chart: melody-first selection, contour lanes on
/// the full neck, true-length tails.
fn build_master(analysis: &SongAnalysis, grid_origin_s: f64) -> Vec<MasterNote> {
    let kept = select_candidates(analysis, grid_origin_s);
    let beat = analysis.beat_interval_s();
    let lanes = LANE_COUNT as i32;
    let mut contour = ContourMapper::new(lanes);
    let mut master: Vec<MasterNote> = Vec::new();
    let mut previous_lane: Option<i32> = None;
    for selected in &kept {
        let mut lane = if let Some(pitch) = selected.pitch {
            contour.lane(selected.time_s, pitch.midi, beat, analysis)
        } else {
            let base = (f64::from(selected.brightness) * f64::from(lanes)).floor() as i32;
            let wiggle = (note_hash(selected.time_s) % 3) as i32 - 1; // −1, 0, +1
            (base + wiggle).clamp(0, lanes - 1)
        };
        if let Some(prev) = previous_lane {
            lane = lane.clamp(prev - 2, prev + 2).clamp(0, lanes - 1);
            let gap = selected.time_s - master.last().map_or(f64::NEG_INFINITY, |n| n.time_s);
            if selected.pitch.is_none() && lane == prev && gap < MASTER_MIN_SPACING_S * 2.0 {
                lane = if prev + 1 < lanes { prev + 1 } else { prev - 1 }.max(0);
            }
        }
        let held_s = natural_tail(analysis, selected, beat);
        master.push(MasterNote {
            time_s: selected.time_s,
            strength: selected.strength,
            lane,
            held_s,
            pitched: selected.pitch.is_some(),
        });
        previous_lane = Some(lane);
    }
    break_jacks(&mut master);
    master
}

/// Rewrite machine-gun jacks into trills, at the master level so
/// every difficulty inherits ONE consistent reading (a note shared
/// between difficulties keeps its lane, as the pinned invariant
/// demands). A repeated pitch is a step-0 interval for the
/// [`ContourMapper`] — melodically right, physically brutal under
/// [`JACK_GAP_S`]. The walk repairs pairs in time order against the
/// CURRENT lanes, so the no-fast-same-lane property holds
/// inductively; the moved lane prefers the higher neighbor and
/// dodges the next fast note's lane where it can.
fn break_jacks(master: &mut [MasterNote]) {
    let lanes = LANE_COUNT as i32;
    for i in 1..master.len() {
        if master[i].time_s - master[i - 1].time_s >= JACK_GAP_S
            || master[i].lane != master[i - 1].lane
        {
            continue;
        }
        let here = master[i].lane;
        let up = (here + 1 < lanes).then_some(here + 1);
        let down = (here > 0).then_some(here - 1);
        let next_fast_lane = master
            .get(i + 1)
            .and_then(|next| (next.time_s - master[i].time_s < JACK_GAP_S).then_some(next.lane));
        let pick = [up, down]
            .into_iter()
            .flatten()
            .find(|lane| Some(*lane) != next_fast_lane)
            .or(up)
            .or(down);
        if let Some(lane) = pick {
            master[i].lane = lane;
        }
    }
}

/// The natural tail of a master note, before difficulty capping.
/// Pitched: the TRUE held length the melody stage measured (short
/// tones are taps). Unpitched: the energy heuristic — the envelope
/// keeps ringing and no strong fresh onset strikes.
fn natural_tail(analysis: &SongAnalysis, selected: &Selected, beat: f64) -> f64 {
    if let Some(pitch) = selected.pitch {
        let held = pitch.end_s - selected.time_s;
        if held >= (beat * 0.5).max(0.3) {
            return held.min(beat * 8.0);
        }
        return 0.0;
    }
    let mut candidate = beat * 4.0;
    // A strong fresh onset ends the ringing — absolute bar (relative
    // measures once cut a live track's sustains to almost none).
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
        candidate
    } else {
        0.0
    }
}

/// Remap a full-neck master lane onto a difficulty's lane count.
fn remap_lane(master_lane: i32, lanes_used: i32) -> i32 {
    if lanes_used >= LANE_COUNT as i32 {
        return master_lane;
    }
    let scaled =
        f64::from(master_lane) * f64::from(lanes_used - 1) / f64::from(LANE_COUNT as i32 - 1);
    (scaled.round() as i32).clamp(0, lanes_used - 1)
}

/// Keep the master notes at or above `floor` that also clear the
/// minimum spacing. Pure and monotonic in `floor` — which is what
/// makes the search below well defined.
fn thin<'a>(master: &[&'a MasterNote], floor: f32, min_spacing_s: f64) -> Vec<&'a MasterNote> {
    let mut kept: Vec<&'a MasterNote> = Vec::new();
    for note in master.iter().copied() {
        if note.strength < floor {
            continue;
        }
        if let Some(last) = kept.last()
            && note.time_s - last.time_s < min_spacing_s
        {
            continue;
        }
        kept.push(note);
    }
    kept
}

/// Reduce the master down to a difficulty by thinning ONCE PER STEP,
/// hardest first — the official workflow (lower difficulties are
/// reductions of the expert chart, not independent charts).
///
/// The chain is what makes "every easy note also exists on expert" a
/// structural guarantee instead of a lucky coincidence: thinning a
/// set can only remove from it. Deriving each difficulty straight
/// from the master does NOT guarantee it — a note easy keeps can be
/// crowded out on medium by a neighbor easy's wider spacing had
/// rejected (constructed case: notes at 0.00/0.30/0.50 with easy
/// spacing 0.45 and medium 0.28).
fn reduction_chain<'a>(
    master: &'a [MasterNote],
    difficulty: Difficulty,
    analysis: &SongAnalysis,
    grid_origin_s: f64,
) -> Vec<&'a MasterNote> {
    // The song's own high ground, computed once (ADR-0011: the
    // design pattern that won its by-ear A/B, graduated into the
    // generator).
    let hot_bars = crate::escalation::hot_bar_flags(analysis, grid_origin_s);
    let mut current: Vec<&MasterNote> = master.iter().collect();
    // Difficulty::ALL is easy..expert; walk it hardest first. Each
    // step escalates its hot bars toward the density of the level
    // ABOVE it — selecting more of the parent set it thins from, so
    // "medium is a subset of hard" survives by construction. Expert's
    // own level above is the MASTER: it rises toward the
    // transcription's hot-bar density where the song rises, and
    // keeps its anchor everywhere else.
    let mut previous_npb: Option<f64> =
        master_hot_npb(master, &hot_bars, grid_origin_s, analysis.beat_interval_s());
    for &step in Difficulty::ALL.iter().rev() {
        let profile = DifficultyProfile::for_difficulty(step);
        current = thin_to_target(
            &current,
            &profile,
            analysis,
            previous_npb.map(|npb| Escalation {
                hot_bars: &hot_bars,
                hot_notes_per_beat: npb,
                grid_origin_s,
            }),
        );
        current = discipline_bursts(current, profile.max_burst_events);
        previous_npb = Some(profile.target_notes_per_beat);
        if step == difficulty {
            break;
        }
    }
    current
}

/// Streams longer than the difficulty tolerates relax their interior
/// to every second note — sixteenths become eighths — keeping the
/// run's first and last hits where they land. Runs inside the cap
/// pass untouched: the cap is a ceiling, not a mower. This runs
/// INSIDE the reduction chain, so the level below thins from the
/// disciplined set and the nesting invariant survives by
/// construction.
fn discipline_bursts(notes: Vec<&MasterNote>, max_burst_events: usize) -> Vec<&MasterNote> {
    if notes.len() < 2 || max_burst_events < 2 {
        return notes;
    }
    let mut keep = vec![true; notes.len()];
    let mut start = 0usize;
    for i in 1..=notes.len() {
        let joins = i < notes.len() && notes[i].time_s - notes[i - 1].time_s < BURST_GAP_S;
        if !joins {
            // The run spans [start, i).
            if i - start > max_burst_events {
                let mut index = start + 1;
                while index < i - 1 {
                    if (index - start) % 2 == 1 {
                        keep[index] = false;
                    }
                    index += 1;
                }
            }
            start = i;
        }
    }
    notes
        .into_iter()
        .zip(keep)
        .filter_map(|(note, kept)| kept.then_some(note))
        .collect()
}

/// The master's own density inside the hot bars, in notes per beat —
/// the ceiling expert escalates toward (`None` when the song has no
/// high ground or no beat: nothing to escalate is a finding).
fn master_hot_npb(
    master: &[MasterNote],
    hot_bars: &[bool],
    grid_origin_s: f64,
    beat: f64,
) -> Option<f64> {
    if beat <= 0.0 {
        return None;
    }
    let hot_beats = 4.0 * hot_bars.iter().filter(|hot| **hot).count() as f64;
    if hot_beats <= 0.0 {
        return None;
    }
    let hot_notes = master
        .iter()
        .filter(|note| {
            hot_bars
                .get(crate::escalation::bar_of(note.time_s, grid_origin_s, beat))
                .copied()
                .unwrap_or(false)
        })
        .count();
    (hot_notes > 0).then(|| hot_notes as f64 / hot_beats)
}

/// The escalation one thinning step runs under: which bars are hot,
/// and the density they rise toward (the next difficulty's reading).
struct Escalation<'a> {
    hot_bars: &'a [bool],
    hot_notes_per_beat: f64,
    grid_origin_s: f64,
}

/// Thin the master to the difficulty's target density: keep the
/// STRONGEST notes that still respect the minimum spacing, then put
/// them back in time order.
///
/// A strength-threshold search cannot do this reliably — note
/// strengths are a step function, so for many songs NO threshold
/// lands near the target (a bisection then converges on the one that
/// empties the chart; that bug is why this is rank-based). Ranking is
/// also what a human charter does: chart the most important hits
/// first, add smaller ones as the difficulty rises.
fn thin_to_target<'a>(
    master: &[&'a MasterNote],
    profile: &DifficultyProfile,
    analysis: &SongAnalysis,
    escalation: Option<Escalation<'_>>,
) -> Vec<&'a MasterNote> {
    let beat = analysis.beat_interval_s();
    let candidates: Vec<&'a MasterNote> = master
        .iter()
        .copied()
        .filter(|note| note.strength >= profile.strength_floor)
        .collect();
    if beat <= 0.0 || candidates.is_empty() {
        return thin(master, profile.strength_floor, profile.min_spacing_s);
    }
    let beats = analysis.duration_s / beat;

    // Per-region budgets: hot bars rise toward the level above, cold
    // bars keep the difficulty's own anchor density. With no hot bars
    // this degenerates to exactly the uniform budget it replaces.
    let is_hot = |time_s: f64| -> bool {
        escalation.as_ref().is_some_and(|e| {
            e.hot_bars
                .get(crate::escalation::bar_of(time_s, e.grid_origin_s, beat))
                .copied()
                .unwrap_or(false)
        })
    };
    let hot_beats = escalation.as_ref().map_or(0.0, |e| {
        4.0 * e.hot_bars.iter().filter(|f| **f).count() as f64
    });
    let cold_beats = (beats - hot_beats).max(0.0);
    let hot_npb = escalation
        .as_ref()
        .map_or(profile.target_notes_per_beat, |e| {
            e.hot_notes_per_beat.max(profile.target_notes_per_beat)
        });
    let mut cold_budget = (profile.target_notes_per_beat * cold_beats)
        .round()
        .max(1.0) as usize;
    let mut hot_budget = (hot_npb * hot_beats).round() as usize;
    if hot_beats <= 0.0 {
        cold_budget = (profile.target_notes_per_beat * beats).round().max(1.0) as usize;
        hot_budget = 0;
    }

    // Strongest first; ties by time so the order never depends on the
    // input's incidental ordering (determinism is a hard rule here).
    let mut ranked: Vec<&'a MasterNote> = candidates;
    ranked.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.time_s
                    .partial_cmp(&b.time_s)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let mut accepted_times: Vec<f64> = Vec::with_capacity(cold_budget + hot_budget);
    let mut kept: Vec<&'a MasterNote> = Vec::with_capacity(cold_budget + hot_budget);
    let mut cold_used = 0usize;
    let mut hot_used = 0usize;
    for note in ranked {
        let hot = is_hot(note.time_s);
        if hot {
            if hot_used >= hot_budget {
                continue;
            }
        } else if cold_used >= cold_budget {
            continue;
        }
        // Spacing check against the nearest accepted neighbors.
        let position = accepted_times.partition_point(|t| *t < note.time_s);
        let too_close_left = position
            .checked_sub(1)
            .is_some_and(|i| note.time_s - accepted_times[i] < profile.min_spacing_s);
        let too_close_right = accepted_times
            .get(position)
            .is_some_and(|t| t - note.time_s < profile.min_spacing_s);
        if too_close_left || too_close_right {
            continue;
        }
        accepted_times.insert(position, note.time_s);
        kept.push(note);
        if hot {
            hot_used += 1;
        } else {
            cold_used += 1;
        }
    }
    kept.sort_by(|a, b| {
        a.time_s
            .partial_cmp(&b.time_s)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept
}

/// Derive one difficulty from the master: thin by strength floor and
/// spacing, remap lanes, cap tails against the DERIVED neighbor gaps,
/// then apply the difficulty's HOPO and chord rules.
fn derive_notes(
    analysis: &SongAnalysis,
    profile: &DifficultyProfile,
    master: &[MasterNote],
    grid_origin_s: f64,
) -> Vec<ChartNote> {
    let lanes_used = i32::from(profile.lanes_used.clamp(1, LANE_COUNT as u8));
    let kept = reduction_chain(master, profile.difficulty, analysis, grid_origin_s);
    // The song's own accents: chord eligibility is a percentile of
    // the KEPT notes' strengths, never an absolute bar — a quiet
    // master and a loud one carry the same accent rate. The median
    // guard keeps accentless songs (uniform strengths) chord-free.
    let mut sorted_strengths: Vec<f32> = kept.iter().map(|n| n.strength).collect();
    sorted_strengths.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let median_strength = sorted_strengths
        .get(sorted_strengths.len() / 2)
        .copied()
        .unwrap_or(0.0);
    let accent_floor = share_floor(&sorted_strengths, profile.chord_share);
    let triple_floor = share_floor(&sorted_strengths, profile.chord_share * 0.25);
    // Place.
    let mut notes: Vec<ChartNote> = Vec::new();
    let mut previous_lane: Option<i32> = None;
    for (i, note) in kept.iter().enumerate() {
        let lane = remap_lane(note.lane, lanes_used);
        let next_time = kept.get(i + 1).map_or(analysis.duration_s, |n| n.time_s);
        let gap_to_next = next_time - note.time_s;
        let mut len = 0.0;
        if profile.sustains && note.held_s > 0.0 {
            let min_gap_ok = note.pitched || gap_to_next >= profile.sustain_min_gap_s;
            if min_gap_ok {
                let candidate = note.held_s.min(gap_to_next - trailing_gap_s(analysis.bpm));
                if candidate > 0.25 {
                    len = candidate;
                }
            }
        }
        let gap_to_prev = notes.last().map_or(f64::INFINITY, |n| note.time_s - n.time);
        let hopo =
            profile.hopos && gap_to_prev <= profile.hopo_max_gap_s && previous_lane != Some(lane);
        let accented = note.strength >= accent_floor && note.strength > median_strength;
        let chord_size = if accented
            && gap_to_prev >= profile.min_spacing_s * 2.0
            && gap_to_next >= profile.min_spacing_s * 1.5
            && len == 0.0
        {
            let widened: u8 = if note.strength >= triple_floor { 3 } else { 2 };
            widened.min(profile.max_chord_size)
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
                time: note.time_s,
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

/// The strength at the boundary of the top `share` of a
/// descending-sorted strength list (infinity when the share is
/// empty, so nothing qualifies).
fn share_floor(sorted_desc: &[f32], share: f32) -> f32 {
    if share <= 0.0 || sorted_desc.is_empty() {
        return f32::INFINITY;
    }
    let count = ((sorted_desc.len() as f32) * share).ceil() as usize;
    sorted_desc[count.clamp(1, sorted_desc.len()) - 1]
}

/// Pitch information a candidate carries when the melody stage found
/// a tone at its position: the raw material for contour lanes and
/// true-length sustains.
#[derive(Debug, Clone, Copy)]
struct Pitched {
    /// MIDI pitch of the tone.
    midi: f32,
    /// Song time the tone stops being held.
    end_s: f64,
}

/// A selected, quantized candidate ready for note placement.
#[derive(Debug, Clone, Copy)]
struct Selected {
    time_s: f64,
    strength: f32,
    brightness: f32,
    pitch: Option<Pitched>,
}

/// An onset this close to a melody-note start is that note's attack.
const ATTACH_S: f64 = 0.09;

/// Merge the onset stream with the melody notes into one candidate
/// stream: an onset at a melody-note start carries that note's pitch;
/// melody notes with no percussive attack (soft guitar entries) stand
/// alone — a hand charter would place them, so do we.
fn merge_candidates(analysis: &SongAnalysis) -> Vec<Selected> {
    let mut used = vec![false; analysis.melody.len()];
    let mut out: Vec<Selected> = Vec::new();
    for onset in &analysis.onsets {
        let mut pitch = None;
        let mut best = f64::INFINITY;
        for (i, note) in analysis.melody.iter().enumerate() {
            let distance = (note.time_s - onset.time_s).abs();
            let at_start = distance <= ATTACH_S;
            // Also inside the note's opening moments — an onset a
            // hair late still belongs to the attack, not the tail.
            let in_attack = onset.time_s >= note.time_s
                && onset.time_s < note.end_s
                && onset.time_s - note.time_s <= 2.0 * ATTACH_S;
            if (at_start || in_attack) && distance < best {
                best = distance;
                pitch = Some((
                    i,
                    Pitched {
                        midi: note.midi,
                        end_s: note.end_s,
                    },
                ));
            }
        }
        if let Some((i, _)) = pitch {
            used[i] = true;
        }
        out.push(Selected {
            time_s: onset.time_s,
            strength: onset.strength,
            brightness: onset.brightness,
            pitch: pitch.map(|(_, p)| p),
        });
    }
    for (i, note) in analysis.melody.iter().enumerate() {
        if used[i] {
            continue;
        }
        out.push(Selected {
            time_s: note.time_s,
            strength: note.strength,
            brightness: pitch_position(note.midi, analysis),
            pitch: Some(Pitched {
                midi: note.midi,
                end_s: note.end_s,
            }),
        });
    }
    out.sort_by(|a, b| {
        a.time_s
            .partial_cmp(&b.time_s)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Where a pitch sits inside the song's melody range, 0.0–1.0.
fn pitch_position(midi: f32, analysis: &SongAnalysis) -> f32 {
    let (low, high) = melody_range(analysis);
    if (high - low).abs() < f32::EPSILON || high < low {
        return 0.5;
    }
    ((midi - low) / (high - low)).clamp(0.0, 1.0)
}

/// The song's melody pitch range (semitone-safe fallback).
fn melody_range(analysis: &SongAnalysis) -> (f32, f32) {
    let mut low = f32::MAX;
    let mut high = f32::MIN;
    for note in &analysis.melody {
        low = low.min(note.midi);
        high = high.max(note.midi);
    }
    if low > high {
        (60.0, 72.0)
    } else {
        (low, high)
    }
}

/// Quantize, filter and thin the candidate stream at MASTER density.
fn select_candidates(analysis: &SongAnalysis, grid_origin_s: f64) -> Vec<Selected> {
    let beat = analysis.beat_interval_s();

    let mut kept: Vec<Selected> = Vec::new();
    // While a strong melody note is HELD, the lead owns the highway:
    // official charts do not stack drum hits on top of a sustain, so
    // unpitched candidates inside the held span are skipped (capped
    // at four beats — background hums must not silence a whole bar).
    let mut hold_until = f64::NEG_INFINITY;
    for candidate in merge_candidates(analysis) {
        if candidate.strength < MASTER_STRENGTH_FLOOR {
            continue;
        }
        if candidate.pitch.is_none() && candidate.time_s < hold_until {
            continue;
        }
        if let Some(pitch) = candidate.pitch {
            let held = pitch.end_s - candidate.time_s;
            if held >= (beat * 0.5).max(0.3) && candidate.strength >= 0.35 {
                hold_until = (candidate.time_s + held.min(beat * 4.0)).min(pitch.end_s)
                    - trailing_gap_s(analysis.bpm);
            }
        }
        let time_s = quantize_musical(candidate.time_s, grid_origin_s, beat, SNAP_TOLERANCE_S);
        if time_s < 0.0 || time_s > analysis.duration_s {
            continue;
        }
        if let Some(last) = kept.last()
            && time_s - last.time_s < MASTER_MIN_SPACING_S
        {
            // Too close: but a PITCHED candidate may upgrade an
            // unpitched neighbor at the same moment — the pitch
            // is what the lane and sustain want to know.
            let last_index = kept.len() - 1;
            if candidate.pitch.is_some()
                && kept[last_index].pitch.is_none()
                && (time_s - kept[last_index].time_s) <= ATTACH_S
            {
                kept[last_index].pitch = candidate.pitch;
            }
            continue;
        }

        kept.push(Selected {
            time_s,
            strength: candidate.strength,
            brightness: candidate.brightness,
            pitch: candidate.pitch,
        });
    }
    kept
}

/// How far a hit may sit from a subdivision and still be the same
/// musical event.
///
/// An absolute time, deliberately NOT a fraction of the beat. Human
/// micro-timing and onset-detector scatter are both well under 60 ms
/// regardless of tempo; a hit further than that from every
/// subdivision is a different note, not a mistimed one. Expressing it
/// as a fraction of the beat is a trap: a quarter of a beat at 120
/// BPM is a whole sixteenth, so adjacent sixteenths collapse onto the
/// beat and a sixteenth-note run disappears (measured).
const SNAP_TOLERANCE_S: f64 = 0.055;

/// Subdivisions a hit may be snapped to, coarsest first.
///
/// Coarsest-first is the point: a hit that fits both an eighth and a
/// sixteenth belongs on the eighth, because that is what a listener
/// feels.
///
/// **Binary only, deliberately.** A triplet level was tried and
/// removed: at 120 BPM a triplet-eighth grid passes within 42 ms of a
/// genuine sixteenth, so with any tolerance wide enough to be useful
/// it steals straight notes and turns a sixteenth run into a shuffle.
/// The two cannot be told apart one note at a time — that needs a
/// song-level decision about whether the piece swings at all, which
/// is a separate feature and not this function's job.
const SNAP_LEVELS: [f64; 4] = [1.0, 2.0, 4.0, 8.0];

/// Snap a time to the simplest musical subdivision close enough to it.
///
/// The single-level quantizer had a cliff: with a window of 30 % of a
/// sixteenth, every hit between 36 ms and 60 ms off kept its raw time.
/// Measured on two real tracks that was a THIRD of all notes, landing
/// nowhere musical while the other two thirds sat exactly on the grid.
/// A chart split between "on the pulse" and "scattered" is what reads
/// as the beat not being detected, even when the tempo is within
/// 1.5 % of the truth.
fn quantize_musical(time_s: f64, origin_s: f64, beat_s: f64, tolerance: f64) -> f64 {
    if beat_s <= 0.0 {
        return time_s;
    }
    for division in SNAP_LEVELS {
        let step = beat_s / division;
        let position = (time_s - origin_s) / step;
        let snapped = origin_s + position.round() * step;
        // Never wider than half a step, or a level would claim
        // positions belonging to its own neighbours.
        if (snapped - time_s).abs() <= tolerance.min(step * 0.5) {
            return snapped;
        }
    }
    time_s
}

/// Snap a time to the nearest grid point when within the snap window.
///
/// Superseded by [`quantize_musical`] in the pipeline; kept because a
/// single fixed grid is still the right primitive when one is wanted.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "single-level primitive, kept for reuse")
)]
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

/// The Guitar-Hero lane convention: lanes track the riff's RELATIVE
/// pitch contour (green low → orange high), not absolute pitch. A
/// phrase anchors where its pitch sits in the song's melody range;
/// within a phrase, the interval to the previous note decides how
/// far the lane moves (small step = 1 lane, leap = 2–3, capped at
/// the neck's edge — official charts saturate there too).
struct ContourMapper {
    lanes_used: i32,
    last: Option<(f64, f32, i32)>, // (time, midi, lane)
}

impl ContourMapper {
    fn new(lanes_used: i32) -> ContourMapper {
        ContourMapper {
            lanes_used,
            last: None,
        }
    }

    fn lane(&mut self, time_s: f64, midi: f32, beat_s: f64, analysis: &SongAnalysis) -> i32 {
        let lane = match self.last {
            // A rest of 4+ beats ends the phrase: re-anchor.
            Some((last_time, last_midi, last_lane)) if time_s - last_time <= 4.0 * beat_s => {
                let interval = midi - last_midi;
                let step = match interval.abs() {
                    d if d < 0.5 => 0,
                    d if d < 2.5 => 1,
                    d if d < 5.5 => 2,
                    _ => 3,
                };
                let direction = if interval >= 0.0 { 1 } else { -1 };
                (last_lane + direction * step).clamp(0, self.lanes_used - 1)
            }
            _ => {
                let position = f64::from(pitch_position(midi, analysis));
                ((position * f64::from(self.lanes_used)).floor() as i32)
                    .clamp(0, self.lanes_used - 1)
            }
        };
        self.last = Some((time_s, midi, lane));
        lane
    }
}

/// The trailing gap a sustain leaves before the next note, from the
/// CustomSongsCentral charting convention (whole-note fractions:
/// 1/32 below 100 BPM, 1/24 at 100–140, 1/16 above — expressed here
/// in beats: 1/8, 1/6 and 1/4 of a beat).
fn trailing_gap_s(bpm: f64) -> f64 {
    let beat = 60.0 / bpm.max(f64::EPSILON);
    if bpm < 100.0 {
        beat / 8.0
    } else if bpm <= 140.0 {
        beat / 6.0
    } else {
        beat / 4.0
    }
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
    use beatbyte_core::music::{MelodyNote, Onset};

    /// A synthetic 120 BPM analysis: onsets on every eighth note for
    /// 60 s (strength alternating strong beats / weak offbeats,
    /// brightness sweeping low→high per bar), followed by a
    /// sixteenth-note run — the terrain auto-HOPO rules exist for.
    pub(super) fn analysis() -> SongAnalysis {
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
            melody: vec![],
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
        // Tests the RULE directly rather than through a whole chart:
        // end to end BOTH cases are capped by the gap to the next
        // note, so a chart-level fixture cannot tell them apart.
        //
        // A held tone ends when something new strikes, and "strong"
        // is an ABSOLUTE bar — measured relative to the held note, a
        // live mix's reverb and crowd cut almost every sustain.
        let tail = |breaker_strength: f32| {
            let analysis = SongAnalysis {
                bpm: 120.0,
                bpm_confidence: 0.8,
                alt_bpm: None,
                beats: (0..20).map(|i| f64::from(i) * 0.5).collect(),
                onsets: vec![
                    Onset {
                        time_s: 1.0,
                        strength: 0.9,
                        brightness: 0.2,
                    },
                    Onset {
                        time_s: 2.5,
                        strength: breaker_strength,
                        brightness: 0.5,
                    },
                ],
                energy: vec![0.8; 200],
                energy_hop_s: 0.05,
                duration_s: 10.0,
                melody: vec![],
            };
            let held = Selected {
                time_s: 1.0,
                strength: 0.9,
                brightness: 0.2,
                pitch: None,
            };
            natural_tail(&analysis, &held, 0.5)
        };
        let cut = tail(0.6);
        let uncut = tail(0.2);
        assert!(
            (cut - 1.4).abs() < 1e-9,
            "a strong onset at +1.5 s must cut the tail just before it: {cut}"
        );
        assert!(
            uncut > cut,
            "a weak onset must NOT truncate: {uncut} vs {cut}"
        );
    }

    /// A melody-driven analysis: notes with true starts/ends plus
    /// matching onsets (the attack the flux stage would report).
    fn melody_analysis(notes: &[(f64, f64, f32)]) -> SongAnalysis {
        use beatbyte_core::music::MelodyNote;
        let melody: Vec<MelodyNote> = notes
            .iter()
            .map(|&(time_s, end_s, midi)| MelodyNote {
                time_s,
                end_s,
                midi,
                strength: 0.9,
            })
            .collect();
        let onsets: Vec<Onset> = notes
            .iter()
            .map(|&(time_s, _, _)| Onset {
                time_s,
                strength: 0.9,
                brightness: 0.5,
            })
            .collect();
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.8,
            alt_bpm: None,
            beats: (0..40).map(|i| f64::from(i) * 0.5).collect(),
            onsets,
            energy: vec![0.8; 400],
            energy_hop_s: 0.05,
            duration_s: 20.0,
            melody,
        }
    }

    fn medium(analysis: &SongAnalysis) -> Vec<crate::schema::ChartNote> {
        generate_chart(analysis, &meta())
            .charts
            .into_iter()
            .find(|c| c.difficulty == Difficulty::Medium)
            .unwrap()
            .notes
    }

    /// The base (contour) lane per time position — chord widening
    /// adds same-time notes on neighboring lanes by design.
    fn base_lanes(notes: &[crate::schema::ChartNote]) -> Vec<u8> {
        let mut lanes = Vec::new();
        let mut last_time = f64::NEG_INFINITY;
        for note in notes {
            if (note.time - last_time).abs() > 1e-9 {
                lanes.push(note.lane);
                last_time = note.time;
            }
        }
        lanes
    }

    #[test]
    fn melody_lanes_follow_the_pitch_contour() {
        // An ascending scale must climb the neck, a descending one
        // must come back down — the Guitar-Hero convention (green
        // low, orange high) in its relative form.
        let up: Vec<(f64, f64, f32)> = (0..5)
            .map(|i| {
                (
                    1.0 + f64::from(i) * 0.6,
                    1.3 + f64::from(i) * 0.6,
                    55.0 + 2.0 * i as f32,
                )
            })
            .collect();
        let notes = medium(&melody_analysis(&up));
        assert!(notes.len() >= 4, "{notes:?}");
        let lanes = base_lanes(&notes);
        assert!(
            lanes.windows(2).all(|w| w[1] >= w[0]),
            "ascending pitch must never move left: {lanes:?}"
        );
        assert!(
            lanes.last() > lanes.first(),
            "an ascending scale must actually climb: {lanes:?}"
        );

        let down: Vec<(f64, f64, f32)> = (0..5)
            .map(|i| {
                (
                    1.0 + f64::from(i) * 0.6,
                    1.3 + f64::from(i) * 0.6,
                    67.0 - 2.0 * i as f32,
                )
            })
            .collect();
        let notes = medium(&melody_analysis(&down));
        let lanes = base_lanes(&notes);
        assert!(
            lanes.windows(2).all(|w| w[1] <= w[0]),
            "descending pitch must never move right: {lanes:?}"
        );
    }

    #[test]
    fn melody_sustains_use_the_true_held_length() {
        // One tone held 1.2 s, next note 2 s later: the sustain is
        // the REAL held length (not the gap), and a short 0.2 s tone
        // gets no tail at all.
        let notes = medium(&melody_analysis(&[
            (1.0, 2.2, 60.0),
            (3.0, 3.2, 62.0),
            (4.0, 4.2, 64.0),
        ]));
        let held = notes.iter().find(|n| (n.time - 1.0).abs() < 0.1).unwrap();
        assert!(
            (held.len - 1.2).abs() < 0.15,
            "sustain must be the true held length: {}",
            held.len
        );
        let short = notes.iter().find(|n| (n.time - 3.0).abs() < 0.1).unwrap();
        assert!(
            short.len < 0.05,
            "a 0.2 s tone is a tap, not a sustain: {}",
            short.len
        );
    }

    #[test]
    fn melody_sustain_leaves_the_trailing_gap() {
        // A tone held right up to the next note must be trimmed by
        // the tempo-scaled trailing gap (the CSC charting
        // convention) so the player can lift and re-fret.
        let notes = medium(&melody_analysis(&[(1.0, 3.0, 60.0), (3.0, 3.4, 62.0)]));
        let held = notes.iter().find(|n| (n.time - 1.0).abs() < 0.1).unwrap();
        let gap = 2.0 - held.len;
        let expected = trailing_gap_s(120.0);
        assert!(
            (gap - expected).abs() < 0.05,
            "trailing gap {gap} should be ~{expected}"
        );
    }

    #[test]
    fn soft_melody_entries_without_onsets_still_chart() {
        // A tone the flux stage never saw (soft entry) must still
        // become a note — hand charters chart what they HEAR.
        let mut analysis = melody_analysis(&[(1.0, 1.8, 60.0), (3.0, 3.8, 64.0)]);
        analysis.onsets.clear();
        let notes = medium(&analysis);
        assert!(
            notes.iter().any(|n| (n.time - 3.0).abs() < 0.1),
            "the onset-less melody note is missing: {notes:?}"
        );
    }

    #[test]
    fn the_reduction_chain_nests_where_one_shot_thinning_would_not() {
        // The constructed break: with a one-shot derivation from the
        // master, medium accepts the middle note (0.30 clears its
        // 0.28 spacing) which then crowds out the note at 0.50 that
        // easy DID keep (0.50 cleared easy's 0.45 spacing from 0.00).
        let master: Vec<MasterNote> = [(0.00, 1.00), (0.30, 0.95), (0.50, 0.90)]
            .iter()
            .map(|&(time_s, strength)| MasterNote {
                time_s,
                strength,
                lane: 0,
                held_s: 0.0,
                pitched: false,
            })
            .collect();
        let analysis = SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            // 3 s at 120 BPM = 6 beats, so the density targets are
            // easy 2 notes and medium 4 — big enough for easy to
            // reach the third note, which is the whole point.
            beats: (0..6).map(|i| f64::from(i) * 0.5).collect(),
            onsets: vec![],
            energy: vec![0.8; 80],
            energy_hop_s: 0.05,
            duration_s: 3.0,
            melody: vec![],
        };
        let refs: Vec<&MasterNote> = master.iter().collect();
        // One-shot: easy keeps a note medium drops — nesting broken.
        let one_shot = |d: Difficulty| -> Vec<f64> {
            thin_to_target(
                &refs,
                &DifficultyProfile::for_difficulty(d),
                &analysis,
                None,
            )
            .iter()
            .map(|n| n.time_s)
            .collect()
        };
        let broken = one_shot(Difficulty::Easy)
            .iter()
            .any(|t| !one_shot(Difficulty::Medium).contains(t));
        assert!(
            broken,
            "this fixture is supposed to break one-shot thinning;              if it no longer does, the chain test below proves nothing"
        );
        // The chain cannot break it: thinning only ever removes.
        let chained = |d: Difficulty| -> Vec<f64> {
            reduction_chain(&master, d, &analysis, 0.0)
                .iter()
                .map(|n| n.time_s)
                .collect()
        };
        for (lower, higher) in [
            (Difficulty::Easy, Difficulty::Medium),
            (Difficulty::Medium, Difficulty::Hard),
            (Difficulty::Hard, Difficulty::Expert),
        ] {
            let up = chained(higher);
            for time in chained(lower) {
                assert!(
                    up.contains(&time),
                    "{lower:?} note at {time} is missing on {higher:?}"
                );
            }
        }
    }

    #[test]
    fn every_easy_note_also_exists_on_expert() {
        // The derivation contract: lower difficulties are SUBSETS of
        // the master — a note you learned on Easy is still there on
        // Expert, never replaced by a different chart.
        let a = analysis();
        let chart = generate_chart(&a, &meta());
        let times = |d: Difficulty| -> Vec<i64> {
            chart
                .charts
                .iter()
                .find(|c| c.difficulty == d)
                .unwrap()
                .notes
                .iter()
                .map(|n| (n.time * 1000.0).round() as i64)
                .collect()
        };
        let expert = times(Difficulty::Expert);
        for time in times(Difficulty::Easy) {
            assert!(
                expert.contains(&time),
                "easy note at {time} ms missing on expert"
            );
        }
        for time in times(Difficulty::Medium) {
            assert!(
                expert.contains(&time),
                "medium note at {time} ms missing on expert"
            );
        }
    }

    #[test]
    fn shared_notes_keep_their_lane_across_difficulties() {
        // The same musical event must sit on the "same" lane on every
        // difficulty (modulo the lane-count remap) — leveling up is
        // the same song with more notes, not a re-chart.
        let a = melody_analysis(&[
            (1.0, 1.3, 55.0),
            (2.0, 2.3, 59.0),
            (3.0, 3.3, 63.0),
            (4.0, 4.3, 67.0),
            (5.0, 5.3, 71.0),
        ]);
        let chart = generate_chart(&a, &meta());
        let base = |d: Difficulty| -> Vec<(i64, u8)> {
            let notes = &chart
                .charts
                .iter()
                .find(|c| c.difficulty == d)
                .unwrap()
                .notes;
            let mut out = Vec::new();
            let mut last = i64::MIN;
            for n in notes.iter() {
                let t = (n.time * 1000.0).round() as i64;
                if t != last {
                    out.push((t, n.lane));
                    last = t;
                }
            }
            out
        };
        let expert: std::collections::HashMap<i64, u8> =
            base(Difficulty::Expert).into_iter().collect();
        for (time, lane) in base(Difficulty::Medium) {
            let Some(&expert_lane) = expert.get(&time) else {
                continue;
            };
            assert_eq!(
                i32::from(lane),
                remap_lane(i32::from(expert_lane), 4),
                "lane mismatch at {time} ms"
            );
        }
    }

    #[test]
    fn lane_remap_preserves_contour_order() {
        for lanes in [3, 4] {
            let mapped: Vec<i32> = (0..5).map(|l| remap_lane(l, lanes)).collect();
            assert!(
                mapped.windows(2).all(|w| w[1] >= w[0]),
                "remap to {lanes} lanes must not reorder: {mapped:?}"
            );
            assert_eq!(mapped[0], 0);
            assert_eq!(mapped[4], lanes - 1, "the top lane must map to the top");
        }
        // PROPORTIONAL, not clamped-identity: the middle of the neck
        // maps to the middle (a clamp-only "remap" satisfied every
        // assertion above — found by mutation testing).
        assert_eq!(remap_lane(2, 3), 1, "neck middle -> 3-lane middle");
        {}
    }

    /// A melody hammering ONE pitch in sixteenth-note bursts — the
    /// machine-gun shape that used to map straight onto a single
    /// lane. Strength stays below every chord threshold so the
    /// property is about single notes, and the tones are short so
    /// nothing sustains. `groups` bursts of `per_group` sixteenths,
    /// one burst every two bars.
    fn sixteenth_bursts(groups: usize, per_group: usize) -> SongAnalysis {
        let beat = 0.5; // 120 BPM
        let mut melody = Vec::new();
        let mut onsets = Vec::new();
        for group in 0..groups {
            for k in 0..per_group {
                let time_s = 1.0 + group as f64 * 8.0 * beat + k as f64 * beat / 4.0;
                melody.push(MelodyNote {
                    time_s,
                    end_s: time_s + 0.06,
                    midi: 64.0,
                    strength: 0.5,
                });
                onsets.push(Onset {
                    time_s,
                    strength: 0.5,
                    brightness: 0.5,
                });
            }
        }
        let beats: Vec<f64> = (0..118).map(|i| 1.0 + f64::from(i) * beat).collect();
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats,
            onsets,
            energy: vec![0.8; 1200],
            energy_hop_s: 0.05,
            duration_s: 60.0,
            melody,
        }
    }

    /// Short bursts that fit inside every burst cap.
    fn repeated_pitch_analysis() -> SongAnalysis {
        sixteenth_bursts(8, 12)
    }

    #[test]
    fn fast_repeated_pitches_do_not_machine_gun_one_lane() {
        let chart = generate_chart(&repeated_pitch_analysis(), &meta());
        // The fixture must actually produce a fast run somewhere, or
        // every assertion below is vacuous.
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        assert!(
            expert
                .windows(2)
                .any(|p| p[1].time - p[0].time > 0.0 && p[1].time - p[0].time < JACK_GAP_S),
            "fixture must keep a fast run on expert"
        );
        for difficulty in Difficulty::ALL {
            let notes = &chart.chart_for(difficulty).unwrap().notes;
            for pair in notes.windows(2) {
                let gap = pair[1].time - pair[0].time;
                if gap > 0.0 && gap < JACK_GAP_S {
                    assert_ne!(
                        pair[1].lane, pair[0].lane,
                        "{difficulty}: same-lane pair {gap:.3}s apart at {:.2}s",
                        pair[0].time
                    );
                }
            }
        }
    }

    #[test]
    fn slow_repeated_pitches_keep_their_lane() {
        // Quarter-note repeats are a musical statement, not a jack —
        // the flow pass must not touch anything at or above the gap.
        let mut analysis = repeated_pitch_analysis();
        analysis.melody = (0..40)
            .map(|k| {
                let time_s = 1.0 + f64::from(k) * 0.5;
                MelodyNote {
                    time_s,
                    end_s: time_s + 0.06,
                    midi: 64.0,
                    strength: 0.5,
                }
            })
            .collect();
        analysis.onsets = analysis
            .melody
            .iter()
            .map(|m| Onset {
                time_s: m.time_s,
                strength: 0.5,
                brightness: 0.5,
            })
            .collect();
        let chart = generate_chart(&analysis, &meta());
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        assert!(expert.len() > 10, "fixture must keep the repeats");
        for pair in expert.windows(2) {
            assert_eq!(
                pair[1].lane, pair[0].lane,
                "a repeated pitch at quarter notes stays on its lane ({:.2}s)",
                pair[0].time
            );
        }
    }

    /// The longest run of consecutive events under the burst gap.
    fn longest_burst(notes: &[ChartNote]) -> usize {
        let mut longest = 1usize;
        let mut run = 1usize;
        for pair in notes.windows(2) {
            let gap = pair[1].time - pair[0].time;
            if gap > 0.0 && gap < BURST_GAP_S {
                run += 1;
                longest = longest.max(run);
            } else if gap > 0.0 {
                run = 1;
            }
        }
        longest
    }

    #[test]
    fn over_cap_streams_relax_to_eighths() {
        // One unbroken 64-event sixteenth stream — longer than any
        // difficulty tolerates.
        let chart = generate_chart(&sixteenth_bursts(1, 64), &meta());
        for difficulty in Difficulty::ALL {
            let profile = DifficultyProfile::for_difficulty(difficulty);
            let notes = &chart.chart_for(difficulty).unwrap().notes;
            assert!(
                longest_burst(notes) <= profile.max_burst_events,
                "{difficulty}: burst of {} events exceeds the cap of {}",
                longest_burst(notes),
                profile.max_burst_events
            );
        }
        // The stream is relaxed, not deleted: expert still carries a
        // substantial run of eighths where the sixteenths were.
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        assert!(expert.len() >= 30, "expert kept {} notes", expert.len());
    }

    #[test]
    fn streams_inside_the_cap_survive_at_full_speed() {
        // Twelve-event bursts sit under expert's cap and must come
        // through untouched — the cap is a ceiling, not a mower.
        let chart = generate_chart(&repeated_pitch_analysis(), &meta());
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        assert!(
            expert
                .windows(2)
                .any(|p| p[1].time - p[0].time > 0.0 && p[1].time - p[0].time < BURST_GAP_S),
            "sub-cap sixteenth bursts must keep their speed on expert"
        );
    }

    #[test]
    fn hard_grows_hopo_runs_at_its_own_speed() {
        // A stepping melody in eighths — 0.25 s gaps, exactly the
        // terrain hard actually inhabits (its median gaps measured
        // 0.23–0.37 s on the imported library). The old 0.20 s HOPO
        // gap sat below that floor, so 15 of 25 real songs got ZERO
        // hopos on hard.
        let steps = [60.0f32, 62.0, 64.0, 62.0];
        let melody: Vec<MelodyNote> = (0..180)
            .map(|k| {
                let time_s = 1.0 + f64::from(k) * 0.25;
                MelodyNote {
                    time_s,
                    end_s: time_s + 0.06,
                    midi: steps[k as usize % steps.len()],
                    strength: 0.5,
                }
            })
            .collect();
        let onsets: Vec<Onset> = melody
            .iter()
            .map(|m| Onset {
                time_s: m.time_s,
                strength: 0.5,
                brightness: 0.5,
            })
            .collect();
        let beats: Vec<f64> = (0..118).map(|i| 1.0 + f64::from(i) * 0.5).collect();
        let analysis = SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats,
            onsets,
            energy: vec![0.8; 1200],
            energy_hop_s: 0.05,
            duration_s: 60.0,
            melody,
        };
        let chart = generate_chart(&analysis, &meta());
        let hard = &chart.chart_for(Difficulty::Hard).unwrap().notes;
        let hopos = hard.iter().filter(|n| n.hopo).count();
        assert!(
            hopos > 0,
            "hard must grow hopo runs from eighth-note steps (kept {} notes, 0 hopos)",
            hard.len()
        );
    }

    #[test]
    fn expert_escalates_toward_the_transcription_in_hot_bars() {
        // Cold verses in eighths, a high-energy refrain in sixteenth
        // bursts short enough to clear the burst cap. Expert's
        // uniform budget used to thin the refrain like the verses;
        // its level above is the master itself.
        let beat = 0.5;
        let mut melody: Vec<MelodyNote> = Vec::new();
        let hot_bars = 40..64usize; // of 64 bars — at the tail, where a
        // uniform time-ranked budget starves first
        for bar in 0..64usize {
            let bar_start = 1.0 + bar as f64 * 4.0 * beat;
            if hot_bars.contains(&bar) {
                // Fourteen sixteenths, then air — under the burst
                // cap, over the uniform budget.
                for k in 0..14 {
                    let time_s = bar_start + k as f64 * beat / 4.0;
                    melody.push(MelodyNote {
                        time_s,
                        end_s: time_s + 0.06,
                        midi: 64.0,
                        strength: 0.5,
                    });
                }
            } else {
                for k in 0..8 {
                    let time_s = bar_start + k as f64 * beat / 2.0;
                    melody.push(MelodyNote {
                        time_s,
                        end_s: time_s + 0.06,
                        midi: 62.0,
                        strength: 0.5,
                    });
                }
            }
        }
        let onsets: Vec<Onset> = melody
            .iter()
            .map(|m| Onset {
                time_s: m.time_s,
                strength: 0.5,
                brightness: 0.5,
            })
            .collect();
        let beats: Vec<f64> = (0..258).map(|i| 1.0 + f64::from(i) * beat).collect();
        let hot_supply = melody
            .iter()
            .filter(|m| {
                let bar = ((m.time_s - 1.0) / (4.0 * beat)).floor() as usize;
                hot_bars.contains(&bar)
            })
            .count();
        let mut energy = vec![0.2f32; 2640];
        for (i, sample) in energy.iter_mut().enumerate() {
            let time = i as f64 * 0.05;
            let bar = ((time - 1.0) / (4.0 * beat)).floor();
            if (40.0..64.0).contains(&bar) {
                *sample = 0.9;
            }
        }
        let analysis = SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats,
            onsets,
            energy,
            energy_hop_s: 0.05,
            duration_s: 132.0,
            melody,
        };
        let chart = generate_chart(&analysis, &meta());
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        let mut hot_times = std::collections::HashSet::new();
        for note in expert.iter() {
            let bar = ((note.time - 1.0) / (4.0 * beat)).floor() as usize;
            if hot_bars.contains(&bar) {
                hot_times.insert((note.time * 1000.0).round() as i64);
            }
        }
        assert!(
            hot_times.len() * 10 >= hot_supply * 9,
            "expert must rise toward the transcription where the song rises: \
             kept {} of {} refrain events",
            hot_times.len(),
            hot_supply
        );
    }

    /// Unpitched eighth notes with every fourth hit accented, at a
    /// chosen absolute scale — the accents are unmistakable INSIDE
    /// the song, whatever the scale.
    fn accented_analysis(scale: f32) -> SongAnalysis {
        let onsets: Vec<Onset> = (0..120)
            .map(|k| Onset {
                time_s: 1.0 + f64::from(k) * 0.25,
                strength: scale * if k % 4 == 0 { 1.0 } else { 0.45 },
                brightness: 0.3 + 0.02 * (k % 8) as f32,
            })
            .collect();
        let beats: Vec<f64> = (0..118).map(|i| 1.0 + f64::from(i) * 0.5).collect();
        SongAnalysis {
            bpm: 120.0,
            bpm_confidence: 0.9,
            alt_bpm: None,
            beats,
            onsets,
            energy: vec![0.8; 1200],
            energy_hop_s: 0.05,
            duration_s: 60.0,
            melody: vec![],
        }
    }

    /// Times at which two or more notes strike together.
    fn chord_times(notes: &[ChartNote]) -> Vec<f64> {
        let mut times = Vec::new();
        for pair in notes.windows(2) {
            if (pair[1].time - pair[0].time).abs() < 1e-9
                && times
                    .last()
                    .is_none_or(|t: &f64| (t - pair[0].time).abs() > 1e-9)
            {
                times.push(pair[0].time);
            }
        }
        times
    }

    #[test]
    fn chords_mark_the_songs_own_accents() {
        // Absolute scale 0.5: every strength sits below the old
        // absolute chord threshold, but the accents are plain.
        let chart = generate_chart(&accented_analysis(0.5), &meta());
        let expert = &chart.chart_for(Difficulty::Expert).unwrap().notes;
        let chords = chord_times(expert);
        assert!(!chords.is_empty(), "the accents must become chords");
        for time in &chords {
            // Accents land on 1.0 + k where k % 4 == 0 → whole beats.
            let beats = (time - 1.0) / 0.5;
            assert!(
                (beats - beats.round()).abs() < 1e-6 && (beats.round() as i64).rem_euclid(2) == 0,
                "chord at {time:.3}s sits on no accent"
            );
        }
    }

    #[test]
    fn chord_rate_is_song_relative_not_absolute() {
        // The same pattern at two absolute scales: the song's OWN
        // accents decide, so the chords are identical.
        let quiet = generate_chart(&accented_analysis(0.5), &meta());
        let loud = generate_chart(&accented_analysis(0.9), &meta());
        let count = |chart: &ChartFile| {
            chord_times(&chart.chart_for(Difficulty::Expert).unwrap().notes).len()
        };
        assert!(count(&quiet) > 0, "quiet song must still have accents");
        assert_eq!(
            count(&quiet),
            count(&loud),
            "chord rate must not depend on the absolute scale"
        );
    }

    /// Accent every `stride`-th hit of a burst fixture.
    fn accent_every(analysis: &mut SongAnalysis, stride: usize) {
        for (k, onset) in analysis.onsets.iter_mut().enumerate() {
            if k % stride == 0 {
                onset.strength = 0.9;
            }
        }
        for (k, tone) in analysis.melody.iter_mut().enumerate() {
            if k % stride == 0 {
                tone.strength = 0.9;
            }
        }
    }

    #[test]
    fn chords_need_room_to_breathe() {
        // Two shapes, one rule. (a) Accents INSIDE a long sixteenth
        // stream: no setup room, no chord. (b) Accents on the
        // OPENERS of short bursts (air before, sixteenths after):
        // plenty of setup room, no landing room — still no chord. A
        // chord inside a machine-gun run is a physical absurdity in
        // either direction.
        let mut inside = sixteenth_bursts(1, 64);
        accent_every(&mut inside, 8);
        let mut openers = sixteenth_bursts(8, 12);
        accent_every(&mut openers, 12);
        for analysis in [inside, openers] {
            check_chord_breathing_room(&generate_chart(&analysis, &meta()));
        }
    }

    fn check_chord_breathing_room(chart: &ChartFile) {
        for difficulty in [Difficulty::Hard, Difficulty::Expert] {
            let notes = &chart.chart_for(difficulty).unwrap().notes;
            let profile = DifficultyProfile::for_difficulty(difficulty);
            for time in chord_times(notes) {
                let prev_gap = notes
                    .iter()
                    .filter(|n| n.time < time - 1e-9)
                    .map(|n| time - n.time)
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    prev_gap >= profile.min_spacing_s * 2.0 - 1e-9,
                    "{difficulty}: chord at {time:.3}s has only {prev_gap:.3}s of setup room"
                );
                let next_gap = notes
                    .iter()
                    .filter(|n| n.time > time + 1e-9)
                    .map(|n| n.time - time)
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    next_gap >= profile.min_spacing_s * 1.5 - 1e-9,
                    "{difficulty}: chord at {time:.3}s has only {next_gap:.3}s of landing room"
                );
            }
        }
    }

    #[test]
    fn uniform_songs_get_no_chords() {
        // Every hit equally strong = the song has no accents; a
        // percentile that ignores this would chord EVERYTHING.
        let uniform: Vec<Onset> = (0..80)
            .map(|k| Onset {
                time_s: 1.0 + f64::from(k) * 0.5,
                strength: 0.5,
                brightness: 0.4,
            })
            .collect();
        let mut analysis = accented_analysis(0.5);
        analysis.onsets = uniform;
        let chart = generate_chart(&analysis, &meta());
        for difficulty in Difficulty::ALL {
            let notes = &chart.chart_for(difficulty).unwrap().notes;
            assert!(
                chord_times(notes).is_empty(),
                "{difficulty}: a song without accents must not chord"
            );
        }
    }

    #[test]
    fn trailing_gap_scales_with_tempo() {
        // 1/32 whole note below 100 BPM, 1/24 to 140, 1/16 above.
        assert!((trailing_gap_s(90.0) - (60.0 / 90.0) / 8.0).abs() < 1e-9);
        assert!((trailing_gap_s(120.0) - (60.0 / 120.0) / 6.0).abs() < 1e-9);
        assert!((trailing_gap_s(160.0) - (60.0 / 160.0) / 4.0).abs() < 1e-9);
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
            melody: vec![],
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
            melody: vec![],
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
    fn musical_snapping_prefers_the_simplest_subdivision() {
        let beat = 0.5; // 120 BPM
        // Dead on a beat: stays.
        assert!((quantize_musical(1.0, 0.0, beat, 0.055) - 1.0).abs() < 1e-9);
        // 30 ms late off a beat: pulled to the beat, not to an eighth.
        assert!((quantize_musical(1.03, 0.0, beat, 0.055) - 1.0).abs() < 1e-9);
        // A real eighth stays an eighth — it must NOT be swallowed by
        // the beat above it.
        assert!((quantize_musical(1.25, 0.0, beat, 0.055) - 1.25).abs() < 1e-9);
        // A real sixteenth likewise.
        assert!((quantize_musical(1.125, 0.0, beat, 0.055) - 1.125).abs() < 1e-9);
        // Documented limitation: the grid is binary, so a triplet is
        // pulled onto the nearest binary subdivision. Detecting swing
        // needs a song-level decision, not a per-note one.
        let triplet = 1.0 + beat / 3.0;
        let snapped = quantize_musical(triplet, 0.0, beat, 0.055);
        assert!(
            (snapped - triplet).abs() > 1e-9,
            "the binary grid is expected to move a triplet; if this \
             ever passes, swing handling was added and this test \
             should describe the new behaviour"
        );
    }

    #[test]
    fn musical_snapping_closes_the_gap_the_old_quantizer_left() {
        // The exact case measured on real tracks: a hit 45 ms off a
        // sixteenth. The single-level quantizer left it where it was
        // (its window was 36 ms); this one puts it on the grid.
        let beat = 0.48; // 125 BPM
        let sixteenth = beat / 4.0;
        let target = 2.0 + sixteenth * 5.0;
        let hit = target + 0.045;
        assert!(
            (quantize(hit, 2.0, sixteenth, (sixteenth * 0.3).min(0.07)) - hit).abs() < 1e-9,
            "the old quantizer is supposed to leave this alone"
        );
        assert!(
            (quantize_musical(hit, 2.0, beat, 0.055) - target).abs() < 1e-9,
            "the musical quantizer must land it on the sixteenth"
        );
    }

    #[test]
    fn a_run_of_sixteenths_survives_snapping() {
        // Adjacent sixteenths must not collapse onto the beat between
        // them — the failure a beat-relative tolerance caused.
        let beat = 0.5;
        let run: Vec<f64> = (0..8).map(|k| 1.0 + f64::from(k) * beat / 4.0).collect();
        let snapped: Vec<f64> = run
            .iter()
            .map(|t| quantize_musical(*t, 0.0, beat, 0.055))
            .collect();
        // f64 is not Ord; compare adjacent pairs instead.
        for pair in snapped.windows(2) {
            assert!(
                pair[1] - pair[0] > 1e-9,
                "snapping merged two sixteenths: {snapped:?}"
            );
        }
    }

    #[test]
    fn quantize_snaps_only_within_window() {
        assert!((quantize(1.02, 0.0, 0.5, 0.07) - 1.0).abs() < 1e-9);
        assert!((quantize(1.2, 0.0, 0.5, 0.07) - 1.2).abs() < 1e-9);
    }
}

#[cfg(test)]
mod escalation_generation_tests {
    use super::*;
    use crate::escalation::hot_bar_flags;

    /// The flat fixture from `tests`, with one loud 16-bar refrain
    /// (32-64 s) pushed into the energy envelope.
    fn hot_analysis() -> SongAnalysis {
        let mut analysis = super::tests::analysis();
        // energy_hop_s = 0.05 -> samples 640..1280 are 32 s .. 64 s.
        for sample in &mut analysis.energy[640..1280] {
            *sample = 1.0;
        }
        // Lower the floor so the refrain is high ground, not part of
        // a uniformly loud song.
        for sample in &mut analysis.energy[..640] {
            *sample = 0.35;
        }
        analysis
    }

    fn notes_in(chart: &ChartDef, from_s: f64, to_s: f64) -> usize {
        chart
            .notes
            .iter()
            .filter(|n| n.time >= from_s && n.time < to_s)
            .count()
    }

    #[test]
    fn medium_rises_where_the_song_does_and_only_there() {
        // The graduated pattern: refrains rise toward hard's density,
        // verses keep the anchor. Compare the same song with and
        // without its refrain being high ground.
        let flat = super::tests::analysis();
        let hot = hot_analysis();
        assert!(
            hot_bar_flags(&hot, 1.0).iter().any(|f| *f),
            "the fixture's refrain must register as hot"
        );
        let profile = DifficultyProfile::for_difficulty(Difficulty::Medium);
        let flat_chart = generate_difficulty(&flat, &profile, 1.0);
        let hot_chart = generate_difficulty(&hot, &profile, 1.0);
        let flat_refrain = notes_in(&flat_chart, 32.0, 64.0);
        let hot_refrain = notes_in(&hot_chart, 32.0, 64.0);
        assert!(
            hot_refrain > flat_refrain,
            "the refrain must densify: {flat_refrain} -> {hot_refrain}"
        );
        // And the verse must NOT densify - escalation is selective or
        // it is nothing.
        let flat_verse = notes_in(&flat_chart, 1.0, 31.0);
        let hot_verse = notes_in(&hot_chart, 1.0, 31.0);
        assert!(
            hot_verse <= flat_verse + 2,
            "the verse must keep breathing: {flat_verse} -> {hot_verse}"
        );
    }

    #[test]
    fn the_subset_invariant_survives_escalation() {
        // "Leveling up is the same song with more of it": every
        // medium note time must exist in hard. Escalation selects
        // MORE of the parent set - it must never invent notes hard
        // does not have.
        let hot = hot_analysis();
        let medium = generate_difficulty(
            &hot,
            &DifficultyProfile::for_difficulty(Difficulty::Medium),
            1.0,
        );
        let hard = generate_difficulty(
            &hot,
            &DifficultyProfile::for_difficulty(Difficulty::Hard),
            1.0,
        );
        let hard_times: std::collections::BTreeSet<u64> = hard
            .notes
            .iter()
            .map(|n| (n.time * 1000.0).round() as u64)
            .collect();
        for note in &medium.notes {
            let key = (note.time * 1000.0).round() as u64;
            assert!(
                hard_times.contains(&key),
                "medium note at {}s does not exist in hard",
                note.time
            );
        }
    }

    /// A fixture dense enough that EXPERT's budget genuinely thins:
    /// sixteenth-note onsets for the whole minute (~480 candidates
    /// against a budget of ~300). The plain fixture saturates -
    /// every master note survives expert regardless - and a
    /// saturated fixture made the first version of the test below
    /// blind: forcing expert to escalate changed nothing it could
    /// see, and the mutation stayed green.
    fn dense(energy_hot: bool) -> SongAnalysis {
        let mut analysis = super::tests::analysis();
        analysis.onsets = (0..480)
            .map(|i| beatbyte_core::music::Onset {
                time_s: 1.0 + f64::from(i) * 0.125,
                strength: 0.3 + 0.6 * ((i % 7) as f32 / 7.0),
                brightness: (i % 16) as f32 / 16.0,
            })
            .collect();
        if energy_hot {
            for sample in &mut analysis.energy[..640] {
                *sample = 0.35;
            }
            for sample in &mut analysis.energy[640..1280] {
                *sample = 1.0;
            }
        }
        analysis
    }

    #[test]
    fn expert_escalates_toward_the_master_where_the_song_rises() {
        // Expert's level above is the MASTER itself (P4 of the
        // difficulty redesign; before that it was the one difficulty
        // that ignored the song's shape). With high ground it keeps
        // more of the transcription than without — in the hot bars,
        // and only there.
        let profile = DifficultyProfile::for_difficulty(Difficulty::Expert);
        let flat = generate_difficulty(&dense(false), &profile, 1.0);
        let hot = generate_difficulty(&dense(true), &profile, 1.0);
        // The guard against a saturated (and therefore blind)
        // fixture: expert's thinning must actually be dropping notes.
        assert!(
            flat.notes.len() < 460,
            "the dense fixture no longer thins ({}) - the test is blind",
            flat.notes.len()
        );
        assert!(
            hot.notes.len() > flat.notes.len(),
            "high ground must let expert rise ({} hot vs {} flat)",
            hot.notes.len(),
            flat.notes.len()
        );
    }

    #[test]
    fn escalated_generation_is_deterministic() {
        let profile = DifficultyProfile::for_difficulty(Difficulty::Medium);
        let a = generate_difficulty(&hot_analysis(), &profile, 1.0);
        let b = generate_difficulty(&hot_analysis(), &profile, 1.0);
        assert_eq!(a.notes, b.notes);
    }
}
