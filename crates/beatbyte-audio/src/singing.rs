//! Turning a separated vocal stem into what a singer is asked to
//! sing: frames of pitch, then notes, then phrases.
//!
//! Everything here is **pure and deterministic** — samples in, a
//! `Vec<VocalPhrase>` out, no clock, no files, no randomness. That is
//! what lets the hard part be tested: a synthetic melody must come
//! back as the notes it was built from, and a song's chart must be
//! the same chart on the next run.
//!
//! ## The stages, and why each one is there
//!
//! 1. **Frame** the stem at 16 kHz and estimate a pitch every
//!    [`SingingConfig::hop_s`]. One estimator, the same one the
//!    microphone uses, so the target and the judgement cannot
//!    disagree about what a note is.
//! 2. **Median-filter** the pitch over a few frames. This kills the
//!    isolated octave jump — one frame that locked onto a harmonic —
//!    without touching vibrato, which at 5–7 Hz is far slower than
//!    the filter is wide. A mean would smear both.
//! 3. **Bridge** short unvoiced gaps. A consonant in the middle of a
//!    word silences the fundamental for 40 ms; splitting a note there
//!    would ask the singer to re-attack in the middle of "little".
//! 4. **Split** where the pitch makes a move it then holds. A note
//!    boundary is a change that *stays* changed — a passing scoop
//!    into a note is not a second note.
//! 5. **Keep the shape** as a sparse contour, by dropping the points
//!    a straight line already explains. A flat note ends up with no
//!    contour at all; a bend keeps the handful of points that are
//!    the bend.
//! 6. **Group** notes into phrases at the silences, and carry the
//!    detector's own clarity through as confidence, so a chart can
//!    say how much of itself it believes.

use beatbyte_core::vocal::{VocalKind, VocalNote, VocalPhrase, VocalPitchPoint};

use crate::pitch::{PitchConfig, PitchDetector};

/// The rate the analysis runs at. 16 kHz carries every vocal
/// fundamental and its first several harmonics, and it is the rate
/// the realtime path uses, which keeps the two estimators identical.
pub const ANALYSIS_RATE: u32 = 16_000;

/// How the stem is turned into notes.
#[derive(Debug, Clone, PartialEq)]
pub struct SingingConfig {
    /// Seconds between pitch estimates.
    pub hop_s: f32,
    /// The window each estimate sees.
    pub window_s: f32,
    /// How the pitch itself is found.
    pub pitch: PitchConfig,
    /// Frames in the median filter. Odd, and short enough that
    /// vibrato passes through.
    pub median_frames: usize,
    /// An unvoiced gap shorter than this is inside a note, not
    /// between two.
    pub bridge_gap_s: f32,
    /// A note shorter than this is a detector artefact.
    pub min_note_s: f32,
    /// A pitch move of this many semitones, held for
    /// [`SingingConfig::hold_frames`], starts a new note.
    pub split_semitones: f32,
    /// How long a move must hold to be a new note rather than a
    /// scoop.
    pub hold_frames: usize,
    /// Silence longer than this ends a phrase.
    pub phrase_gap_s: f32,
    /// A note the detector was less sure of than this is dropped.
    pub min_confidence: f32,
    /// How far a contour point may sit from the straight line
    /// through its neighbours before it is worth keeping, in
    /// semitones.
    pub contour_tolerance_semitones: f32,
    /// The window the octave correction takes its local reference
    /// from, in seconds.
    pub octave_window_s: f32,
    /// How much closer an octave-shifted pitch must land to the local
    /// reference before the shift is believed, in semitones.
    pub octave_gain_semitones: f32,
}

impl Default for SingingConfig {
    fn default() -> SingingConfig {
        SingingConfig {
            hop_s: 0.010,
            window_s: 0.064,
            pitch: PitchConfig {
                sample_rate: ANALYSIS_RATE,
                ..PitchConfig::default()
            },
            median_frames: 5,
            bridge_gap_s: 0.10,
            min_note_s: 0.08,
            split_semitones: 0.7,
            hold_frames: 4,
            phrase_gap_s: 0.8,
            min_confidence: 0.5,
            contour_tolerance_semitones: 0.25,
            octave_window_s: 1.0,
            octave_gain_semitones: 3.0,
        }
    }
}

/// One analysed frame: what the detector said, on the song timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SungFrame {
    /// Song seconds at the window's centre.
    pub time_s: f64,
    /// The pitch, fractional MIDI, when the frame was voiced.
    pub midi: Option<f32>,
    /// The detector's clarity, `0..=1`.
    pub clarity: f32,
}

/// Estimate the pitch across a whole stem.
///
/// `samples` is mono at `sample_rate`; it is resampled to
/// [`ANALYSIS_RATE`] once. Each frame is stamped at its window's
/// **centre**, which is where the estimate actually applies — stamping
/// at the window's start would put every note half a window early.
#[must_use]
pub fn track_pitch(samples: &[f32], sample_rate: u32, config: &SingingConfig) -> Vec<SungFrame> {
    let mono = crate::resample::resample(samples, sample_rate, ANALYSIS_RATE);
    let rate = f64::from(ANALYSIS_RATE);
    let mut detector = PitchDetector::new(config.pitch);
    let window = ((config.window_s * ANALYSIS_RATE as f32) as usize).max(detector.min_window());
    let hop = ((config.hop_s * ANALYSIS_RATE as f32) as usize).max(1);
    if mono.len() < window {
        return Vec::new();
    }
    let mut frames = Vec::with_capacity((mono.len() - window) / hop + 1);
    let mut start = 0usize;
    while start + window <= mono.len() {
        let frame = detector.detect(&mono[start..start + window]);
        frames.push(SungFrame {
            time_s: (start as f64 + window as f64 / 2.0) / rate,
            midi: frame.midi(),
            clarity: frame.clarity,
        });
        start += hop;
    }
    frames
}

/// Replace each voiced frame's pitch with the median of the window
/// around it.
///
/// Only voiced neighbours vote, and a frame with too few of them is
/// left alone: a median taken across a gap would drag the first note
/// after a rest toward the last note before it. Pure — tested.
#[must_use]
pub fn median_filter(frames: &[SungFrame], width: usize) -> Vec<SungFrame> {
    let half = width / 2;
    if half == 0 {
        return frames.to_vec();
    }
    let mut out = frames.to_vec();
    let mut window: Vec<f32> = Vec::with_capacity(width);
    for (index, frame) in frames.iter().enumerate() {
        if frame.midi.is_none() {
            continue;
        }
        window.clear();
        let from = index.saturating_sub(half);
        let to = (index + half + 1).min(frames.len());
        for neighbour in &frames[from..to] {
            if let Some(midi) = neighbour.midi {
                window.push(midi);
            }
        }
        if window.len() < 3 {
            continue;
        }
        window.sort_by(f32::total_cmp);
        out[index].midi = Some(window[window.len() / 2]);
    }
    out
}

/// Fill unvoiced runs shorter than `max_gap` frames by interpolating
/// between the voiced frames either side. Pure — tested.
#[must_use]
pub fn bridge_gaps(frames: &[SungFrame], max_gap: usize) -> Vec<SungFrame> {
    let mut out = frames.to_vec();
    let mut index = 0;
    while index < out.len() {
        if out[index].midi.is_some() {
            index += 1;
            continue;
        }
        let start = index;
        while index < out.len() && out[index].midi.is_none() {
            index += 1;
        }
        let length = index - start;
        // A gap at either end of the stem has nothing to bridge to.
        if length > max_gap || start == 0 || index >= out.len() {
            continue;
        }
        let (Some(before), Some(after)) = (out[start - 1].midi, out[index].midi) else {
            continue;
        };
        for (step, slot) in out[start..index].iter_mut().enumerate() {
            let t = (step + 1) as f32 / (length + 1) as f32;
            slot.midi = Some(before + (after - before) * t);
        }
    }
    out
}

/// Fold frames that are an octave away from what is being sung
/// around them back onto it.
///
/// ## Why this is needed, measured rather than assumed
///
/// A voice's second harmonic is frequently louder than its
/// fundamental, and McLeod's key maximum is a *threshold* on that:
/// take the earliest crest that is nearly as tall as the tallest.
/// Set it forgiving and half-period crests get taken (an octave too
/// high); set it strict and the true crest gets skipped (an octave
/// too low). There is no value that is right for every voice.
///
/// Measured on a real song — Blondie's *Maria*, checked against the
/// stem's own spectrum rather than by ear — the pitch histogram had
/// two peaks exactly twelve semitones apart, and of the notes charted
/// at the upper one, **21 % also had a partial an octave below**:
/// their fundamental was down there and the chart had taken the
/// harmonic.
///
/// The fix is not a better threshold but a second look with context.
/// A singer does not leap an octave and come straight back; a
/// detector does. So each frame is compared with the median of the
/// second around it, and a shift of ±12 or ±24 is applied only when
/// it lands the frame **much** closer to that median — a genuine
/// seven-semitone leap improves by two and is left alone, while a
/// thirteen-semitone jump improves by twelve and is folded. Pure —
/// tested.
#[must_use]
pub fn correct_octaves(frames: &[SungFrame], window_s: f32, gain_semitones: f32) -> Vec<SungFrame> {
    let mut out = frames.to_vec();
    if frames.len() < 3 {
        return out;
    }
    let half = f64::from(window_s) / 2.0;
    let mut neighbourhood: Vec<f32> = Vec::new();
    for index in 0..frames.len() {
        let Some(midi) = frames[index].midi else {
            continue;
        };
        neighbourhood.clear();
        // Walk outwards from the frame rather than scanning the whole
        // list: the window is a second and the list is a whole song.
        for other in frames[..index].iter().rev() {
            if frames[index].time_s - other.time_s > half {
                break;
            }
            if let Some(m) = other.midi {
                neighbourhood.push(m);
            }
        }
        for other in &frames[index + 1..] {
            if other.time_s - frames[index].time_s > half {
                break;
            }
            if let Some(m) = other.midi {
                neighbourhood.push(m);
            }
        }
        // Too little context to judge by. Leaving the frame alone is
        // the conservative choice: an uncorrected frame is one frame,
        // a wrongly corrected one is a wrong target.
        if neighbourhood.len() < 5 {
            continue;
        }
        let reference = median_of(&neighbourhood);
        let here = (midi - reference).abs();
        let mut best = midi;
        let mut best_distance = here;
        for shift in [-24.0f32, -12.0, 12.0, 24.0] {
            let candidate = midi + shift;
            let distance = (candidate - reference).abs();
            if distance < best_distance {
                best_distance = distance;
                best = candidate;
            }
        }
        if here - best_distance >= gain_semitones {
            out[index].midi = Some(best);
        }
    }
    out
}

/// A run of consecutive voiced frames, as indices into the frame
/// list: `[start, end)`.
type Run = (usize, usize);

fn voiced_runs(frames: &[SungFrame]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut index = 0;
    while index < frames.len() {
        if frames[index].midi.is_none() {
            index += 1;
            continue;
        }
        let start = index;
        while index < frames.len() && frames[index].midi.is_some() {
            index += 1;
        }
        runs.push((start, index));
    }
    runs
}

/// Where inside a voiced run the note boundaries are.
///
/// A boundary is a pitch move of at least `split_semitones` away from
/// the current note's centre that then **holds** for `hold_frames`.
/// The hold is the whole point: a scoop into a note passes through a
/// semitone of distance on its way and is not a note of its own.
/// Pure — tested.
#[must_use]
pub fn split_points(pitches: &[f32], split_semitones: f32, hold_frames: usize) -> Vec<usize> {
    let hold = hold_frames.max(1);
    let mut splits = Vec::new();
    let mut anchor = 0usize;
    let mut centre = pitches.first().copied().unwrap_or(0.0);
    let mut index = 1usize;
    while index + hold <= pitches.len() {
        let away = pitches[index..index + hold]
            .iter()
            .all(|p| (p - centre).abs() >= split_semitones);
        if away {
            splits.push(index);
            anchor = index;
            centre = median_of(&pitches[index..index + hold]);
            index += hold;
        } else {
            // The centre follows the note it is in, so a slow drift
            // across a long note does not eventually trip the split.
            centre = median_of(&pitches[anchor..=index]);
            index += 1;
        }
    }
    splits
}

fn median_of(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f32> = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[sorted.len() / 2]
}

/// Reduce a pitch polyline to the points a straight line does not
/// already explain (Ramer–Douglas–Peucker).
///
/// A held note comes out empty — its nominal pitch says everything —
/// and a bend keeps just the points that are the bend. The caller
/// decides what "empty" means; the chart reads it as flat. Pure —
/// tested.
#[must_use]
pub fn simplify(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (x0, y0) = points[first];
        let (x1, y1) = points[last];
        let span = x1 - x0;
        let mut worst = 0.0f32;
        let mut worst_at = first;
        for (offset, &(x, y)) in points[first + 1..last].iter().enumerate() {
            let on_line = if span.abs() < f32::EPSILON {
                y0
            } else {
                y0 + (y1 - y0) * (x - x0) / span
            };
            let distance = (y - on_line).abs();
            if distance > worst {
                worst = distance;
                worst_at = first + 1 + offset;
            }
        }
        if worst > tolerance {
            keep[worst_at] = true;
            stack.push((first, worst_at));
            stack.push((worst_at, last));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&point, keep)| keep.then_some(point))
        .collect()
}

/// The whole pipeline: a vocal stem to the phrases a singer is held
/// to.
#[must_use]
pub fn analyse(samples: &[f32], sample_rate: u32, config: &SingingConfig) -> Vec<VocalPhrase> {
    let raw = track_pitch(samples, sample_rate, config);
    notes_from_frames(&raw, config)
}

/// The pure half of [`analyse`]: frames in, phrases out.
///
/// Separate so the segmentation can be tested on frames written by
/// hand, without synthesising audio for every case.
#[must_use]
pub fn notes_from_frames(raw: &[SungFrame], config: &SingingConfig) -> Vec<VocalPhrase> {
    if raw.is_empty() {
        return Vec::new();
    }
    let smoothed = median_filter(raw, config.median_frames);
    // After the median (so a lone spike is already gone and cannot
    // drag the local reference) and before the bridging (so an
    // interpolated gap is drawn between corrected pitches).
    let smoothed = correct_octaves(
        &smoothed,
        config.octave_window_s,
        config.octave_gain_semitones,
    );
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "a count of frames from positive seconds"
    )]
    let max_gap = (config.bridge_gap_s / config.hop_s.max(1e-4)) as usize;
    let bridged = bridge_gaps(&smoothed, max_gap);

    let mut notes: Vec<VocalNote> = Vec::new();
    for (start, end) in voiced_runs(&bridged) {
        let pitches: Vec<f32> = bridged[start..end].iter().filter_map(|f| f.midi).collect();
        let mut bounds = vec![0usize];
        bounds.extend(split_points(
            &pitches,
            config.split_semitones,
            config.hold_frames,
        ));
        bounds.push(pitches.len());
        for pair in bounds.windows(2) {
            let (from, to) = (start + pair[0], start + pair[1]);
            if let Some(note) = build_note(&bridged[from..to], config) {
                notes.push(note);
            }
        }
    }
    group_into_phrases(notes, config)
}

fn build_note(frames: &[SungFrame], config: &SingingConfig) -> Option<VocalNote> {
    let first = frames.first()?;
    let last = frames.last()?;
    // The frame stamps are window centres, so a note runs from the
    // first centre to one hop past the last: the last frame covers a
    // hop of audio, not an instant.
    let start_s = first.time_s;
    let end_s = last.time_s + f64::from(config.hop_s);
    if end_s - start_s < f64::from(config.min_note_s) {
        return None;
    }
    let pitches: Vec<f32> = frames.iter().filter_map(|f| f.midi).collect();
    if pitches.is_empty() {
        return None;
    }
    let target = median_of(&pitches);
    let confidence = frames.iter().map(|f| f.clarity).sum::<f32>() / frames.len() as f32;
    if confidence < config.min_confidence {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "an offset inside one note; f32 is the contour's stored precision"
    )]
    let polyline: Vec<(f32, f32)> = frames
        .iter()
        .filter_map(|f| f.midi.map(|midi| ((f.time_s - start_s) as f32, midi)))
        .collect();
    let simplified = simplify(&polyline, config.contour_tolerance_semitones);
    // Two points that a straight line explains ARE the straight line;
    // a flat note needs no contour at all, and saying so keeps the
    // common case out of the file.
    let contour = if simplified.len() <= 2
        && simplified
            .iter()
            .all(|&(_, midi)| (midi - target).abs() <= config.contour_tolerance_semitones)
    {
        Vec::new()
    } else {
        simplified
            .into_iter()
            .map(|(offset_s, midi)| VocalPitchPoint {
                offset_s,
                midi,
                confidence,
            })
            .collect()
    };
    Some(VocalNote {
        start_s,
        end_s,
        kind: VocalKind::Pitched,
        target_midi: Some(target),
        contour,
        confidence: confidence.clamp(0.0, 1.0),
        token_range: None,
    })
}

fn group_into_phrases(notes: Vec<VocalNote>, config: &SingingConfig) -> Vec<VocalPhrase> {
    let mut phrases: Vec<VocalPhrase> = Vec::new();
    let mut current: Vec<VocalNote> = Vec::new();
    let gap = f64::from(config.phrase_gap_s);
    for note in notes {
        let breaks = current
            .last()
            .is_some_and(|last| note.start_s - last.end_s > gap);
        if breaks {
            phrases.extend(close_phrase(std::mem::take(&mut current)));
        }
        current.push(note);
    }
    phrases.extend(close_phrase(current));
    phrases
}

fn close_phrase(notes: Vec<VocalNote>) -> Option<VocalPhrase> {
    let start_s = notes.first()?.start_s;
    let end_s = notes.last()?.end_s;
    // Duration-weighted, so one clipped syllable does not drag a long
    // confident line down and a long uncertain one is not rescued by
    // a crisp final note.
    let total: f64 = notes.iter().map(VocalNote::duration_s).sum();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a weighted mean of values in 0..=1"
    )]
    let confidence = if total > 0.0 {
        (notes
            .iter()
            .map(|n| f64::from(n.confidence) * n.duration_s())
            .sum::<f64>()
            / total) as f32
    } else {
        0.0
    };
    Some(VocalPhrase {
        start_s,
        end_s,
        confidence: confidence.clamp(0.0, 1.0),
        tokens: Vec::new(),
        notes,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn frame(time_s: f64, midi: Option<f32>) -> SungFrame {
        SungFrame {
            time_s,
            midi,
            clarity: 0.9,
        }
    }

    fn frames_at(hop: f64, pitches: &[Option<f32>]) -> Vec<SungFrame> {
        pitches
            .iter()
            .enumerate()
            .map(|(i, &midi)| frame(i as f64 * hop, midi))
            .collect()
    }

    #[test]
    fn the_median_kills_a_lone_octave_jump() {
        // One frame that locked onto the second harmonic. A mean
        // would leave a bump behind; the median removes it entirely.
        let mut pitches = vec![Some(60.0); 9];
        pitches[4] = Some(72.0);
        let out = median_filter(&frames_at(0.01, &pitches), 5);
        assert_eq!(out[4].midi, Some(60.0), "the spike survived");
        assert!(out.iter().all(|f| f.midi == Some(60.0)));
    }

    #[test]
    fn the_median_leaves_vibrato_alone() {
        // 6 Hz at +-50 cents, sampled every 10 ms: one cycle is 17
        // frames, so a 5-frame median must pass it through nearly
        // untouched. Smoothing it away would flatten every held note
        // in the game.
        let pitches: Vec<Option<f32>> = (0..60)
            .map(|i| {
                let t = i as f32 * 0.01;
                Some(60.0 + 0.5 * (core::f32::consts::TAU * 6.0 * t).sin())
            })
            .collect();
        let before = frames_at(0.01, &pitches);
        let after = median_filter(&before, 5);
        let spread = |fs: &[SungFrame]| {
            let values: Vec<f32> = fs.iter().filter_map(|f| f.midi).collect();
            values.iter().cloned().fold(f32::MIN, f32::max)
                - values.iter().cloned().fold(f32::MAX, f32::min)
        };
        let kept = spread(&after) / spread(&before);
        assert!(
            kept > 0.85,
            "the filter flattened vibrato to {kept:.2} of it"
        );
    }

    #[test]
    fn the_median_does_not_vote_across_a_rest() {
        // Only voiced neighbours vote. Otherwise the first note after
        // a rest is dragged toward the last note before it.
        let pitches = [
            Some(60.0),
            Some(60.0),
            None,
            None,
            None,
            Some(72.0),
            Some(72.0),
            Some(72.0),
            Some(72.0),
        ];
        let out = median_filter(&frames_at(0.01, &pitches), 5);
        assert_eq!(out[5].midi, Some(72.0), "the rest pulled the new note down");
        assert_eq!(out[2].midi, None, "a rest stays a rest");
    }

    #[test]
    fn a_consonant_is_bridged_and_a_rest_is_not() {
        // 3 frames of silence inside a word: bridged, interpolated.
        let pitches = [
            Some(60.0),
            Some(60.0),
            None,
            None,
            None,
            Some(62.0),
            Some(62.0),
        ];
        let out = bridge_gaps(&frames_at(0.01, &pitches), 5);
        assert!(
            out[2].midi.is_some() && out[4].midi.is_some(),
            "the gap survived"
        );
        let middle = out[3].midi.unwrap();
        assert!((middle - 61.0).abs() < 0.01, "interpolated to {middle}");

        // The same gap, longer than the limit: left alone.
        let out = bridge_gaps(&frames_at(0.01, &pitches), 2);
        assert!(out[2..5].iter().all(|f| f.midi.is_none()));
    }

    #[test]
    fn a_gap_at_either_end_has_nothing_to_bridge_to() {
        let pitches = [None, None, Some(60.0), Some(60.0), None, None];
        let out = bridge_gaps(&frames_at(0.01, &pitches), 5);
        assert_eq!(out[0].midi, None, "invented a pitch before the song");
        assert_eq!(out[5].midi, None, "invented a pitch after it");
    }

    #[test]
    fn a_held_move_splits_and_a_scoop_does_not() {
        // Four frames at 60, then a sustained jump to 64: one split.
        let held: Vec<f32> = [60.0; 10].into_iter().chain([64.0; 10]).collect();
        assert_eq!(split_points(&held, 0.7, 4), vec![10]);

        // A scoop: two frames a long way off on the way INTO the
        // note, then settled. Not a note of its own.
        let scoop: Vec<f32> = vec![
            60.0, 60.0, 60.0, 60.0, 60.0, 62.0, 61.0, 60.2, 60.0, 60.0, 60.0, 60.0,
        ];
        assert_eq!(
            split_points(&scoop, 0.7, 4),
            Vec::<usize>::new(),
            "a scoop became a note"
        );
    }

    #[test]
    fn a_slow_drift_across_a_long_note_is_not_a_split() {
        // A singer sagging a quarter tone over three seconds. The
        // centre follows the note, so the distance never accumulates
        // into a boundary.
        let drift: Vec<f32> = (0..300).map(|i| 60.0 - i as f32 * 0.0017).collect();
        assert_eq!(
            split_points(&drift, 0.7, 4),
            Vec::<usize>::new(),
            "a drift of half a semitone over three seconds split the note"
        );
    }

    #[test]
    fn a_flat_note_keeps_no_contour_and_a_bend_keeps_its_corner() {
        let flat: Vec<(f32, f32)> = (0..20).map(|i| (i as f32 * 0.01, 60.0)).collect();
        assert_eq!(
            simplify(&flat, 0.25).len(),
            2,
            "a straight line is two points"
        );

        // Up two semitones over the second half: the corner must
        // survive, the straight parts must not.
        let bend: Vec<(f32, f32)> = (0..21)
            .map(|i| {
                let t = i as f32 * 0.01;
                (
                    t,
                    if i <= 10 {
                        60.0
                    } else {
                        60.0 + (i - 10) as f32 * 0.2
                    },
                )
            })
            .collect();
        let kept = simplify(&bend, 0.25);
        assert!(
            kept.len() >= 3 && kept.len() <= 6,
            "kept {} points",
            kept.len()
        );
        assert!(
            kept.iter().any(|&(x, _)| (x - 0.10).abs() < 0.021),
            "the corner at 0.10 s was dropped: {kept:?}"
        );
    }

    #[test]
    fn the_pipeline_really_runs_the_median_over_its_frames() {
        // ⚠️ Testing `median_filter` directly proves the function
        // works, not that anything calls it. A mutation probe that
        // bypassed the stage left the suite green — so this goes
        // through `notes_from_frames`, where a surviving spike shows
        // up as a contour on a note that was held flat.
        let mut pitches = vec![Some(60.0f32); 40];
        pitches[18] = Some(72.0);
        pitches[19] = Some(72.0);
        let notes: Vec<_> =
            notes_from_frames(&frames_at(0.01, &pitches), &SingingConfig::default())
                .into_iter()
                .flat_map(|p| p.notes)
                .collect();
        assert_eq!(notes.len(), 1, "a spike became a note: {notes:#?}");
        assert!(
            notes[0].contour.is_empty(),
            "the spike reached the contour: {:?}",
            notes[0].contour
        );
        let span = notes[0].pitch_span().unwrap();
        assert!(
            span.semitones() < 0.5,
            "the note spans {} semitones",
            span.semitones()
        );
    }

    #[test]
    fn the_pipeline_really_bridges_its_gaps() {
        // The same blind spot on the other stage: one word with a
        // consonant in it must come back as ONE note, and it only
        // does if the pipeline bridges.
        let mut pitches = vec![Some(60.0f32); 23];
        for slot in &mut pitches[10..13] {
            *slot = None;
        }
        let notes: Vec<_> =
            notes_from_frames(&frames_at(0.01, &pitches), &SingingConfig::default())
                .into_iter()
                .flat_map(|p| p.notes)
                .collect();
        assert_eq!(
            notes.len(),
            1,
            "the consonant split the word into {} notes",
            notes.len()
        );
        // And a gap too long to be a consonant still separates.
        let mut long = vec![Some(60.0f32); 60];
        for slot in &mut long[20..45] {
            *slot = None;
        }
        let notes: Vec<_> = notes_from_frames(&frames_at(0.01, &long), &SingingConfig::default())
            .into_iter()
            .flat_map(|p| p.notes)
            .collect();
        assert_eq!(notes.len(), 2, "a quarter-second rest is two notes");
    }

    #[test]
    fn a_melody_comes_back_as_the_notes_it_was_built_from() {
        // The end-to-end proof, on audio rather than on frames.
        let rate = 16_000u32;
        let mut samples = Vec::new();
        let melody = [(60.0f32, 0.6f64), (64.0, 0.6), (67.0, 0.6)];
        let mut phase = 0.0f32;
        for &(midi, seconds) in &melody {
            let hz = beatbyte_core::vocal::midi_to_hz(midi);
            let n = (seconds * f64::from(rate)) as usize;
            for _ in 0..n {
                samples.push(0.4 * phase.sin());
                phase += core::f32::consts::TAU * hz / rate as f32;
            }
            // A clear rest between notes.
            samples.extend(std::iter::repeat_n(0.0, (0.3 * f64::from(rate)) as usize));
        }
        let phrases = analyse(&samples, rate, &SingingConfig::default());
        assert_eq!(phrases.len(), 1, "one phrase: {phrases:#?}");
        let notes = &phrases[0].notes;
        assert_eq!(notes.len(), 3, "expected three notes, got {}", notes.len());
        for (note, &(midi, _)) in notes.iter().zip(&melody) {
            let got = note.target_midi.unwrap();
            assert!(
                (got - midi).abs() < 0.15,
                "expected MIDI {midi}, got {got:.2}"
            );
            assert!(
                (note.duration_s() - 0.6).abs() < 0.12,
                "note of {:.2} s should be about 0.6",
                note.duration_s()
            );
            assert!(note.contour.is_empty(), "a held note needs no contour");
            assert!(note.confidence > 0.8, "{}", note.confidence);
        }
        // And the notes land where they were sung.
        assert!(notes[0].start_s < 0.1, "{}", notes[0].start_s);
        assert!((notes[1].start_s - 0.9).abs() < 0.1, "{}", notes[1].start_s);
    }

    #[test]
    fn a_note_lands_where_it_was_sung_and_not_half_a_window_early() {
        // ⚠️ A mutation probe found nothing here: stamping frames at
        // their window's START instead of its centre shifts every
        // note 32 ms early, and every other test's tolerance was wide
        // enough to swallow it. A systematic offset on every note in
        // the game is exactly the defect that reads as "the lyrics
        // feel late" and gets blamed on the aligner.
        let rate = 16_000u32;
        let silence = (0.5 * f64::from(rate)) as usize;
        let hz = beatbyte_core::vocal::midi_to_hz(64.0);
        let mut samples = vec![0.0f32; silence];
        samples.extend(
            (0..(0.8 * f64::from(rate)) as usize)
                .map(|i| 0.4 * (core::f32::consts::TAU * hz * i as f32 / rate as f32).sin()),
        );
        let note = analyse(&samples, rate, &SingingConfig::default())
            .into_iter()
            .flat_map(|p| p.notes)
            .next()
            .expect("one note");
        let off_ms = (note.start_s - 0.5) * 1000.0;
        assert!(
            off_ms.abs() < 25.0,
            "the note starts {off_ms:.0} ms from where it was sung"
        );
    }

    #[test]
    fn a_blip_and_an_unconvincing_run_are_not_notes() {
        // Risk 3 of the plan: a wrong target is worse than no target.
        // Both guards were BLIND to a mutation probe until this test
        // existed — each could have been deleted with the suite green.
        let config = SingingConfig::default();

        // Three frames of pitch is 30 ms. That is a detector
        // artefact, not something anyone sang.
        let mut blip = vec![None; 30];
        for slot in &mut blip[12..15] {
            *slot = Some(60.0);
        }
        let notes: usize = notes_from_frames(&frames_at(0.01, &blip), &config)
            .iter()
            .map(|p| p.notes.len())
            .sum();
        assert_eq!(notes, 0, "a 30 ms blip became a note");

        // Long enough, but the detector was never convinced. A chart
        // built from this would hold the player to a guess.
        let unsure: Vec<SungFrame> = (0..40)
            .map(|i| SungFrame {
                time_s: f64::from(i) * 0.01,
                midi: Some(60.0),
                clarity: 0.3,
            })
            .collect();
        let notes: usize = notes_from_frames(&unsure, &config)
            .iter()
            .map(|p| p.notes.len())
            .sum();
        assert_eq!(notes, 0, "an unconvincing run became a note");

        // And the same run, believed: one note. Without this the test
        // above would pass on a pipeline that produces nothing ever.
        let sure: Vec<SungFrame> = unsure
            .iter()
            .map(|f| SungFrame { clarity: 0.9, ..*f })
            .collect();
        let notes: usize = notes_from_frames(&sure, &config)
            .iter()
            .map(|p| p.notes.len())
            .sum();
        assert_eq!(notes, 1, "the gegenprobe: a good run must still chart");
    }

    #[test]
    fn a_long_silence_starts_a_new_phrase() {
        let rate = 16_000u32;
        let tone = |midi: f32, seconds: f64| -> Vec<f32> {
            let hz = beatbyte_core::vocal::midi_to_hz(midi);
            (0..(seconds * f64::from(rate)) as usize)
                .map(|i| 0.4 * (core::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
                .collect()
        };
        let mut samples = tone(62.0, 0.6);
        samples.extend(std::iter::repeat_n(0.0, (1.5 * f64::from(rate)) as usize));
        samples.extend(tone(69.0, 0.6));
        let phrases = analyse(&samples, rate, &SingingConfig::default());
        assert_eq!(
            phrases.len(),
            2,
            "a bar and a half of silence is a phrase break"
        );
        assert!(phrases[0].notes.len() == 1 && phrases[1].notes.len() == 1);
        assert!(phrases[1].start_s > phrases[0].end_s + 1.0);
    }

    #[test]
    fn an_instrumental_stem_yields_no_notes_at_all() {
        // The state that matters most: a wrong target is worse than
        // no target, so silence and noise must produce nothing rather
        // than something faint.
        let rate = 16_000u32;
        let silence = vec![0.0f32; rate as usize * 3];
        assert!(analyse(&silence, rate, &SingingConfig::default()).is_empty());

        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let noise: Vec<f32> = (0..rate as usize * 3)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ((state >> 40) as f32 / 16_777_216.0) - 0.5
            })
            .collect();
        let phrases = analyse(&noise, rate, &SingingConfig::default());
        let notes: usize = phrases.iter().map(|p| p.notes.len()).sum();
        assert!(notes <= 1, "noise produced {notes} notes: {phrases:#?}");
    }

    #[test]
    fn the_same_stem_analyses_to_the_same_chart_twice() {
        // Determinism is a feature here: a re-import must not silently
        // produce a different chart from the same bytes.
        let rate = 16_000u32;
        let hz = beatbyte_core::vocal::midi_to_hz(64.0);
        let samples: Vec<f32> = (0..rate as usize)
            .map(|i| 0.4 * (core::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
            .collect();
        let config = SingingConfig::default();
        assert_eq!(
            analyse(&samples, rate, &config),
            analyse(&samples, rate, &config)
        );
    }

    #[test]
    fn a_bend_is_kept_as_a_contour_rather_than_flattened() {
        // A slide from C4 up to D4 across one note: the chart has to
        // carry the shape, or the game would ask the singer to hold a
        // pitch nobody sang.
        let rate = 16_000u32;
        let n = (0.9 * f64::from(rate)) as usize;
        let mut phase = 0.0f32;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let midi = 60.0 + 2.0 * t;
                let hz = beatbyte_core::vocal::midi_to_hz(midi);
                let s = 0.4 * phase.sin();
                phase += core::f32::consts::TAU * hz / rate as f32;
                s
            })
            .collect();
        let phrases = analyse(&samples, rate, &SingingConfig::default());
        let note = phrases
            .iter()
            .flat_map(|p| p.notes.iter())
            .max_by(|a, b| a.duration_s().total_cmp(&b.duration_s()))
            .expect("a slide is still a note");
        assert!(
            !note.contour.is_empty(),
            "the slide was flattened to one pitch"
        );
        let low = note.target_at(note.start_s + 0.05).unwrap();
        let high = note.target_at(note.end_s - 0.05).unwrap();
        assert!(high - low > 1.0, "the contour only rises {:.2}", high - low);
    }
}
