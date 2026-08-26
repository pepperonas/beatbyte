//! Whether a chart can actually be played, measured.
//!
//! A transcription can be musically correct and still be a bad chart.
//! The generator therefore has to optimise two things at once, and
//! this module supplies the second one: concrete numbers for the
//! properties that make a chart playable or miserable, plus the
//! burst limit the generator enforces while building.
//!
//! Everything here is a pure function over a chart — no audio, no
//! randomness, same chart in, same numbers out.

use beatbyte_core::Difficulty;

use crate::schema::ChartNote;

/// Measured playability of one difficulty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playability {
    /// Notes per second across the whole chart.
    pub density: f64,
    /// The busiest one-second window, in notes.
    pub peak_burst: usize,
    /// Mean lane distance between consecutive note positions.
    pub lane_motion: f64,
    /// Largest lane distance between consecutive note positions.
    pub max_jump: u8,
    /// Share of note positions that are chords.
    pub chord_share: f64,
    /// Share of notes carrying a sustain.
    pub sustain_share: f64,
    /// Share of consecutive pairs that reverse direction — the
    /// zig-zag that makes a chart feel arbitrary.
    pub direction_changes: f64,
    /// Overall 0.0–1.0, higher is more comfortable.
    pub score: f64,
}

/// The comfortable ceiling on notes per second for a difficulty, and
/// the hard burst cap the generator enforces.
///
/// These are hand limits, not musical ones: a player has five fingers
/// and one strumming hand. Expert charts in the genre routinely run
/// 8–10 notes a second in short bursts and settle far lower on
/// average, which is the shape encoded here.
#[must_use]
pub const fn comfortable_density(difficulty: Difficulty) -> f64 {
    match difficulty {
        Difficulty::Easy => 1.5,
        Difficulty::Medium => 3.0,
        Difficulty::Hard => 5.0,
        Difficulty::Expert => 7.5,
    }
}

/// The most notes a difficulty may put inside any one second.
#[must_use]
pub const fn burst_limit(difficulty: Difficulty) -> usize {
    match difficulty {
        Difficulty::Easy => 4,
        Difficulty::Medium => 7,
        Difficulty::Hard => 11,
        Difficulty::Expert => 16,
    }
}

/// Measure a difficulty's notes. `duration_s` is the song length.
#[must_use]
pub fn evaluate(notes: &[ChartNote], difficulty: Difficulty, duration_s: f64) -> Playability {
    if notes.is_empty() || duration_s <= 0.0 {
        return Playability {
            density: 0.0,
            peak_burst: 0,
            lane_motion: 0.0,
            max_jump: 0,
            chord_share: 0.0,
            sustain_share: 0.0,
            direction_changes: 0.0,
            score: 1.0,
        };
    }

    // Note POSITIONS, not notes: a three-note chord is one hand
    // movement, and counting it as three would report motion that
    // nobody makes.
    let mut positions: Vec<(f64, u8, usize)> = Vec::new();
    for note in notes {
        match positions.last_mut() {
            Some((time, lane, size)) if (note.time - *time).abs() < 1e-6 => {
                *lane = (*lane).min(note.lane);
                *size += 1;
            }
            _ => positions.push((note.time, note.lane, 1)),
        }
    }

    let density = notes.len() as f64 / duration_s;
    let peak_burst = busiest_second(notes);

    let mut motion_sum = 0.0;
    let mut max_jump = 0u8;
    let mut directions: Vec<i32> = Vec::new();
    for pair in positions.windows(2) {
        let jump = pair[1].1.abs_diff(pair[0].1);
        motion_sum += f64::from(jump);
        max_jump = max_jump.max(jump);
        if jump > 0 {
            directions.push(i32::from(pair[1].1 > pair[0].1) * 2 - 1);
        }
    }
    let steps = (positions.len().saturating_sub(1)).max(1);
    let lane_motion = motion_sum / steps as f64;
    let reversals = directions
        .windows(2)
        .filter(|pair| pair[0] != pair[1])
        .count();
    let direction_changes = if directions.len() < 2 {
        0.0
    } else {
        reversals as f64 / (directions.len() - 1) as f64
    };

    let chord_share =
        positions.iter().filter(|(_, _, size)| *size > 1).count() as f64 / positions.len() as f64;
    let sustain_share = notes.iter().filter(|n| n.len > 0.0).count() as f64 / notes.len() as f64;

    // Each term is 1.0 while comfortable and falls off past it.
    //
    // The overall score is half the mean and half the WORST term: a
    // chart is about as playable as the thing most wrong with it, but
    // one rough passage should not zero an otherwise good chart. A
    // plain mean is too forgiving — a chart thirteen times too dense
    // still scored 0.65 on it, because four of the five terms were
    // fine.
    let terms = [
        ratio_penalty(density, comfortable_density(difficulty)),
        ratio_penalty(peak_burst as f64, burst_limit(difficulty) as f64),
        ratio_penalty(lane_motion, 1.6),
        ratio_penalty(f64::from(max_jump), 3.0),
        ratio_penalty(direction_changes, 0.65),
    ];
    let mean = terms.iter().sum::<f64>() / terms.len() as f64;
    let worst = terms.iter().copied().fold(1.0f64, f64::min);
    let score = 0.5f64.mul_add(mean, 0.5 * worst);

    Playability {
        density,
        peak_burst,
        lane_motion,
        max_jump,
        chord_share,
        sustain_share,
        direction_changes,
        score,
    }
}

/// 1.0 while `value` is within `comfortable`, decaying beyond it.
fn ratio_penalty(value: f64, comfortable: f64) -> f64 {
    if comfortable <= 0.0 {
        return 1.0;
    }
    let ratio = value / comfortable;
    if ratio <= 1.0 {
        1.0
    } else {
        (1.0 / ratio).clamp(0.0, 1.0)
    }
}

/// The most notes inside any one-second window.
fn busiest_second(notes: &[ChartNote]) -> usize {
    let mut best = 0usize;
    let mut start = 0usize;
    for end in 0..notes.len() {
        while notes[end].time - notes[start].time > 1.0 {
            start += 1;
        }
        best = best.max(end - start + 1);
    }
    best
}

/// Indices of the notes to drop so that no one-second window exceeds
/// the difficulty's burst limit.
///
/// A transcription can be locally denser than hands allow — a drum
/// fill, a tremolo, a chord shredded into single notes. The generator
/// removes the WEAKEST notes in an overfull window rather than
/// truncating the window, so the passage keeps its strongest hits and
/// stays recognisable.
///
/// `strength` runs parallel to `notes`. Ties break on the later note
/// so the result never depends on sort stability.
#[must_use]
pub fn burst_overflow(notes: &[ChartNote], strength: &[f32], limit: usize) -> Vec<usize> {
    let mut dropped = vec![false; notes.len()];
    if limit == 0 || notes.is_empty() {
        return Vec::new();
    }
    for anchor in 0..notes.len() {
        loop {
            // Everything still alive inside the window starting here.
            let alive: Vec<usize> = (anchor..notes.len())
                .take_while(|i| notes[*i].time - notes[anchor].time <= 1.0)
                .filter(|i| !dropped[*i])
                .collect();
            if alive.len() <= limit || dropped[anchor] {
                break;
            }
            // Drop the weakest — and never the window's own anchor,
            // which is what keeps a passage's start intact.
            let Some(&weakest) = alive.iter().filter(|i| **i != anchor).min_by(|a, b| {
                let (sa, sb) = (
                    strength.get(**a).copied().unwrap_or(0.0),
                    strength.get(**b).copied().unwrap_or(0.0),
                );
                sa.total_cmp(&sb).then(b.cmp(a))
            }) else {
                break;
            };
            dropped[weakest] = true;
        }
    }
    dropped
        .iter()
        .enumerate()
        .filter(|(_, gone)| **gone)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn note(time: f64, lane: u8) -> ChartNote {
        ChartNote {
            time,
            lane,
            len: 0.0,
            hopo: false,
        }
    }

    #[test]
    fn a_comfortable_chart_scores_high() {
        let notes: Vec<ChartNote> = (0..20)
            .map(|i| note(f64::from(i) * 0.5, (i % 3) as u8))
            .collect();
        let report = evaluate(&notes, Difficulty::Medium, 10.0);
        assert!(report.score > 0.9, "{report:?}");
        assert!((report.density - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_wall_of_notes_scores_low() {
        let notes: Vec<ChartNote> = (0..200)
            .map(|i| note(f64::from(i) * 0.05, (i % 5) as u8))
            .collect();
        let report = evaluate(&notes, Difficulty::Easy, 10.0);
        assert!(report.score < 0.5, "{report:?}");
        assert!(report.peak_burst >= 20);
    }

    #[test]
    fn chords_count_as_one_hand_movement() {
        // Three notes at the same instant, then one far away. If
        // chords counted as separate positions this would report
        // motion inside the chord that no hand makes.
        let notes = vec![note(0.0, 0), note(0.0, 1), note(0.0, 2), note(1.0, 2)];
        let report = evaluate(&notes, Difficulty::Hard, 2.0);
        assert!((report.lane_motion - 2.0).abs() < 1e-9);
        assert!((report.chord_share - 0.5).abs() < 1e-9);
    }

    #[test]
    fn zigzag_is_measured() {
        let steady: Vec<ChartNote> = (0..10).map(|i| note(f64::from(i) * 0.5, i % 5)).collect();
        let jitter: Vec<ChartNote> = (0..10)
            .map(|i| note(f64::from(i) * 0.5, if i % 2 == 0 { 0 } else { 4 }))
            .collect();
        let steady = evaluate(&steady, Difficulty::Hard, 5.0);
        let jitter = evaluate(&jitter, Difficulty::Hard, 5.0);
        assert!(
            jitter.direction_changes > steady.direction_changes,
            "alternating lanes must read as more zig-zag: {jitter:?} vs {steady:?}"
        );
    }

    #[test]
    fn the_burst_limiter_keeps_the_strongest_and_the_anchor() {
        // Twelve notes inside one second, limit four.
        let notes: Vec<ChartNote> = (0..12).map(|i| note(f64::from(i) * 0.08, 0)).collect();
        let strength: Vec<f32> = (0..12).map(|i| i as f32 / 12.0).collect();
        let dropped = burst_overflow(&notes, &strength, 4);
        assert!(!dropped.is_empty(), "an overfull window must lose notes");
        assert!(!dropped.contains(&0), "the anchor must survive");
        let kept: Vec<usize> = (0..12).filter(|i| !dropped.contains(i)).collect();
        assert!(kept.len() <= 5, "still too dense: {kept:?}");
        // What survives must be the strong end, not the first four.
        let weakest_kept = kept
            .iter()
            .skip(1)
            .map(|i| strength[*i])
            .fold(1.0, f32::min);
        assert!(
            weakest_kept > 0.2,
            "the limiter kept weak notes over strong ones: {kept:?}"
        );
    }

    #[test]
    fn the_burst_limiter_leaves_a_playable_chart_alone() {
        let notes: Vec<ChartNote> = (0..20)
            .map(|i| note(f64::from(i) * 0.5, (i % 3) as u8))
            .collect();
        let strength = vec![0.5f32; notes.len()];
        assert!(burst_overflow(&notes, &strength, 7).is_empty());
    }

    #[test]
    fn limits_rise_with_difficulty() {
        let ladder = [
            Difficulty::Easy,
            Difficulty::Medium,
            Difficulty::Hard,
            Difficulty::Expert,
        ];
        for pair in ladder.windows(2) {
            assert!(comfortable_density(pair[0]) < comfortable_density(pair[1]));
            assert!(burst_limit(pair[0]) < burst_limit(pair[1]));
        }
    }
}
