//! Tracking a beat sequence that follows the music.
//!
//! ## Why this exists
//!
//! The pipeline's original grid is one period and one phase, laid
//! rigidly across the whole song ([`super::tempo::fit_beat_grid`]).
//! On a three-minute rock song that is fine. On a 6–8 minute DJ track
//! it cannot work, and `docs/audio-eval-baseline.md` measures why:
//! against Rekordbox's own grids, the tempo estimate is accurate to
//! 0.02–0.25 %, and **every single track still drifts out of the
//! ±70 ms tolerance** — the worst by a factor of 18, because a
//! relative tempo error accumulates linearly while the tolerance does
//! not. Holding 70 ms over eight minutes needs 0.014 % accuracy. That
//! is not a number an estimator reaches; it is a shape that does not
//! fit the material.
//!
//! A tracked grid does not accumulate: each beat only has to sit
//! roughly one period after the previous one, so the sequence
//! re-anchors on real onsets continuously and a small period error
//! stays a small error forever.
//!
//! ## The method
//!
//! Dynamic programming over the onset envelope, after Ellis (2007),
//! *Beat Tracking by Dynamic Programming*. Every frame asks which
//! earlier frame is its best predecessor, paying a penalty for
//! landing at anything other than one period back; the best chain is
//! then read out backwards. It is a global optimum over the whole
//! song rather than a greedy walk, which is what keeps a busy fill
//! from throwing the sequence off.
//!
//! Deterministic and pure: same envelope in, same beats out.

use serde::{Deserialize, Serialize};

/// How the beat grid is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GridMode {
    /// One period, one phase, laid across the song. The original
    /// behaviour, kept because it is the cheaper answer and because a
    /// caller that wants the pre-2026-09 grids can ask for them.
    ConstantTempo,
    /// A tracked sequence that follows the music ([`track`]).
    ///
    /// The default since the measurement said so: on every case in
    /// the repository that has ground truth — the two rock songs,
    /// the four synthetic cases and seven real house tracks — this
    /// mode is better or identical, and never worse.
    #[default]
    Tracked,
}

/// Configuration for beat tracking.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridConfig {
    /// Which grid to produce.
    pub mode: GridMode,
    /// How hard the tracker insists on the target period. Higher is
    /// stiffer; librosa's long-standing default is 100, and the same
    /// value is used here rather than a fresh guess.
    pub tightness: f64,
    /// Weight of the kick channel against the broadband channel when
    /// building the tracking envelope. 1.0 is kick only, 0.0 is the
    /// broadband curve the rest of the pipeline uses.
    pub low_band_weight: f32,
}

impl Default for GridConfig {
    fn default() -> Self {
        GridConfig {
            mode: GridMode::default(),
            tightness: 100.0,
            low_band_weight: 1.0,
        }
    }
}

/// Blend the broadband and kick envelopes into the curve the tracker
/// follows.
///
/// The blend is kept as a knob because the right weight was measured,
/// not reasoned out. My first guess was 0.75, on the argument that a
/// breakdown with no kick would leave a kick-only tracker with
/// nothing to hold on to. The sweep over the real corpus says
/// otherwise, and monotonically: mean beat F-measure runs 0.530 /
/// 0.588 / 0.733 / **0.840** as the weight goes 0.0 / 0.5 / 0.75 /
/// 1.0. The argument was wrong because dynamic programming does not
/// need onsets to cross a gap — with nothing to reward, the chain
/// simply continues at the target period and picks the music back up
/// on the far side. Pure — tested.
#[must_use]
pub fn tracking_envelope(broadband: &[f32], low: &[f32], low_weight: f32) -> Vec<f32> {
    let weight = low_weight.clamp(0.0, 1.0);
    if low.len() != broadband.len() {
        // A caller that hands over mismatched channels gets the one
        // that is certainly present rather than a panic or a silently
        // truncated song.
        return broadband.to_vec();
    }
    broadband
        .iter()
        .zip(low)
        .map(|(&wide, &deep)| wide * (1.0 - weight) + deep * weight)
        .collect()
}

/// Track a beat sequence through `strength` at roughly `bpm`.
///
/// Returns beat times in seconds. Empty when the envelope is too
/// short to hold two beats — a caller must fall back rather than
/// receive a fabricated grid.
#[must_use]
pub fn track(
    strength: &[f32],
    hop_s: f64,
    frame_offset_s: f64,
    bpm: f64,
    config: &GridConfig,
) -> Vec<f64> {
    let period = period_frames(bpm, hop_s);
    if period < 2.0 || strength.len() < 4 {
        return Vec::new();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (back_far, back_near) = (
        (period * 2.0).round() as usize,
        (period * 0.5).round() as usize,
    );
    if back_near == 0 || back_far >= strength.len() {
        return Vec::new();
    }

    // Local score of the best chain ending at each frame, and the
    // predecessor that produced it.
    let mut score: Vec<f64> = vec![0.0; strength.len()];
    let mut back: Vec<Option<usize>> = vec![None; strength.len()];

    for t in 0..strength.len() {
        let here = f64::from(strength[t]);
        let mut best = f64::NEG_INFINITY;
        let mut best_from = None;
        let first = t.saturating_sub(back_far);
        let last = t.checked_sub(back_near);
        if let Some(last) = last {
            for (offset, &reached) in score[first..=last].iter().enumerate() {
                let candidate = first + offset;
                // Ellis's transition cost: a squared log penalty, so
                // being 10 % early costs the same as being 10 % late
                // and a half-period jump is punished sharply.
                let gap = (t - candidate) as f64;
                let penalty = -config.tightness * (gap / period).ln().powi(2);
                let total = reached + penalty;
                if total > best {
                    best = total;
                    best_from = Some(candidate);
                }
            }
        }
        if best_from.is_some() {
            score[t] = here + best;
            back[t] = best_from;
        } else {
            // No legal predecessor yet: this frame can only start a
            // chain.
            score[t] = here;
        }
    }

    let Some(end) = best_ending(&score) else {
        return Vec::new();
    };
    let mut frames = vec![end];
    let mut cursor = end;
    while let Some(previous) = back[cursor] {
        frames.push(previous);
        cursor = previous;
    }
    frames.reverse();
    if frames.len() < 2 {
        return Vec::new();
    }
    frames
        .into_iter()
        .map(|frame| frame as f64 * hop_s + frame_offset_s)
        .collect()
}

/// Where to start reading the chain backwards.
///
/// The plain argmax of the cumulative score, which is where the
/// longest well-supported chain ends. Pure — tested.
fn best_ending(score: &[f64]) -> Option<usize> {
    score
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_finite())
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
}

/// The target period in frames.
fn period_frames(bpm: f64, hop_s: f64) -> f64 {
    if bpm <= 0.0 || hop_s <= 0.0 {
        return 0.0;
    }
    (60.0 / bpm) / hop_s
}

/// Continue a tracked sequence out to both ends of the song.
///
/// The tracker only produces beats where the music supports them, so
/// a quiet intro or a long outro comes back bare. Chart generation
/// quantises against this grid, so a note in the outro would have
/// nothing to snap to. The edges are therefore continued at the
/// sequence's own **local** period — the nearest real interval, not
/// the global average, so extrapolating cannot reintroduce the very
/// drift this module exists to remove. Pure — tested.
#[must_use]
pub fn extend_to_span(beats: &[f64], duration_s: f64) -> Vec<f64> {
    if beats.len() < 2 || duration_s <= 0.0 {
        return beats.to_vec();
    }
    let mut out = Vec::with_capacity(beats.len() + 16);

    let head = beats[1] - beats[0];
    if head > 0.0 {
        let mut t = beats[0] - head;
        let mut backwards = Vec::new();
        while t >= 0.0 {
            backwards.push(t);
            t -= head;
        }
        backwards.reverse();
        out.extend(backwards);
    }

    out.extend_from_slice(beats);

    let tail = beats[beats.len() - 1] - beats[beats.len() - 2];
    if tail > 0.0 {
        let mut t = beats[beats.len() - 1] + tail;
        while t < duration_s {
            out.push(t);
            t += tail;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An envelope with a spike every `period` frames, starting at
    /// `phase` — a beat track with the answer known in advance.
    fn pulses(frames: usize, period: f64, phase: usize) -> Vec<f32> {
        let mut envelope = vec![0.02f32; frames];
        let mut index = phase as f64;
        while (index as usize) < frames {
            envelope[index as usize] = 1.0;
            index += period;
        }
        envelope
    }

    const HOP: f64 = 0.0116;

    #[test]
    fn a_steady_pulse_is_tracked_onto_its_own_spikes() {
        // 120 BPM at an 11.6 ms hop is 43.1 frames per beat.
        let period = (60.0 / 120.0) / HOP;
        let envelope = pulses(2000, period, 20);
        let beats = track(&envelope, HOP, 0.0, 120.0, &GridConfig::default());
        assert!(beats.len() > 30, "got {} beats", beats.len());
        // Every beat must sit on a spike, not between them.
        for &beat in &beats {
            let frame = beat / HOP;
            let offset = (frame - 20.0) / period;
            let error = (offset - offset.round()).abs() * period;
            assert!(error < 1.0, "beat at frame {frame} is {error} frames off");
        }
    }

    #[test]
    fn a_drifting_pulse_is_followed_where_a_rigid_grid_could_not() {
        // THE reason this module exists. The envelope runs 0.3 %
        // faster than the tempo the tracker is told, which over
        // 2000 frames is ~7 frames — ten times the tolerance a rigid
        // grid would have to hold from a single phase.
        let told = (60.0 / 120.0) / HOP;
        let actual = told * 0.997;
        let envelope = pulses(2000, actual, 20);
        let beats = track(&envelope, HOP, 0.0, 120.0, &GridConfig::default());
        assert!(beats.len() > 30);

        // The LAST beat is the one a rigid grid gets wrong, so that is
        // the one worth checking.
        let last = beats[beats.len() - 1] / HOP;
        let offset = (last - 20.0) / actual;
        let error = (offset - offset.round()).abs() * actual;
        assert!(error < 1.0, "the tail drifted {error} frames off");

        // And prove the contrast: a rigid grid at the told period,
        // phase-locked to the first spike, misses the same spike by
        // several frames.
        let rigid = 20.0 + told * ((last - 20.0) / told).round();
        assert!(
            (rigid - last).abs() > 3.0,
            "the rigid grid should be visibly worse here, was {} frames off",
            (rigid - last).abs()
        );
    }

    #[test]
    fn a_kick_channel_breaks_an_offbeat_tie_a_broadband_one_cannot() {
        // The measured defect: on four-to-the-floor, offbeat hats are
        // as strong in the broadband curve as the kick. Build exactly
        // that — kicks on the beat, LOUDER hats between them.
        let period = (60.0 / 120.0) / HOP;
        let mut broadband = vec![0.02f32; 2000];
        let mut low = vec![0.02f32; 2000];
        let mut index = 20.0f64;
        while (index as usize) < 1990 {
            let on = index as usize;
            let off = (index + period / 2.0) as usize;
            broadband[on] = 0.8;
            low[on] = 1.0;
            if off < 2000 {
                // The hat is louder broadband and absent from the kick
                // band — the whole point of the split.
                broadband[off] = 1.0;
                low[off] = 0.0;
            }
            index += period;
        }

        let on_beat = |beats: &[f64]| -> f64 {
            let hits = beats
                .iter()
                .filter(|&&b| {
                    let offset = (b / HOP - 20.0) / period;
                    (offset - offset.round()).abs() * period < 2.0
                })
                .count();
            hits as f64 / beats.len().max(1) as f64
        };

        let wide = tracking_envelope(&broadband, &low, 0.0);
        let deep = tracking_envelope(&broadband, &low, 0.75);
        let wide_beats = track(&wide, HOP, 0.0, 120.0, &GridConfig::default());
        let deep_beats = track(&deep, HOP, 0.0, 120.0, &GridConfig::default());

        assert!(
            on_beat(&deep_beats) > 0.9,
            "the kick channel should land on the beat, got {}",
            on_beat(&deep_beats)
        );
        assert!(
            on_beat(&deep_beats) > on_beat(&wide_beats),
            "the kick channel must beat the broadband one here \
             ({} vs {}), or the split buys nothing",
            on_beat(&deep_beats),
            on_beat(&wide_beats)
        );
    }

    #[test]
    fn the_envelope_blend_is_a_real_blend() {
        let wide = vec![1.0, 0.0];
        let deep = vec![0.0, 1.0];
        assert_eq!(tracking_envelope(&wide, &deep, 0.0), vec![1.0, 0.0]);
        assert_eq!(tracking_envelope(&wide, &deep, 1.0), vec![0.0, 1.0]);
        assert_eq!(tracking_envelope(&wide, &deep, 0.5), vec![0.5, 0.5]);
        // Out-of-range weights are clamped, not trusted.
        assert_eq!(tracking_envelope(&wide, &deep, 5.0), vec![0.0, 1.0]);
        // Mismatched channels fall back to the one that is there.
        assert_eq!(tracking_envelope(&wide, &[0.0], 1.0), vec![1.0, 0.0]);
    }

    #[test]
    fn nonsense_input_yields_no_grid_rather_than_a_fabricated_one() {
        let config = GridConfig::default();
        assert!(track(&[], HOP, 0.0, 120.0, &config).is_empty());
        assert!(track(&[1.0; 100], 0.0, 0.0, 120.0, &config).is_empty());
        assert!(track(&[1.0; 100], HOP, 0.0, 0.0, &config).is_empty());
        // A period longer than the song cannot be tracked.
        assert!(track(&[1.0; 20], HOP, 0.0, 1.0, &config).is_empty());
    }

    #[test]
    fn the_edges_are_continued_at_the_local_period() {
        // Beats from 5.0 to 6.0 in a 10 s song: the head and tail both
        // need filling, and the filling must use the neighbouring
        // interval rather than an average.
        let beats = vec![5.0, 5.5, 6.0];
        let full = extend_to_span(&beats, 10.0);
        // The invariant is not "a beat at 0.0 and one at 10.0" — a
        // grid cannot place a beat past the end. It is that neither
        // edge leaves a gap wider than one beat, which is what a note
        // needs in order to have something to snap to.
        assert!(full[0] < 0.5, "the head reaches the start, got {}", full[0]);
        assert!(
            10.0 - full[full.len() - 1] <= 0.5,
            "the tail leaves a gap of {}",
            10.0 - full[full.len() - 1]
        );
        // The original beats survive untouched — extrapolation may
        // never move a beat the tracker actually found.
        for original in &beats {
            assert!(
                full.iter().any(|t| (t - original).abs() < 1e-9),
                "{original} was lost"
            );
        }
        // Ascending throughout.
        assert!(full.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn extending_a_grid_that_cannot_be_extended_changes_nothing() {
        assert_eq!(extend_to_span(&[1.0], 10.0), vec![1.0]);
        assert!(extend_to_span(&[], 10.0).is_empty());
        assert_eq!(extend_to_span(&[1.0, 2.0], 0.0), vec![1.0, 2.0]);
    }
}
