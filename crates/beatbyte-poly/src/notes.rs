//! From the model's two answers to notes — Basic Pitch's polyphonic
//! note tracking, written out.
//!
//! The model gives, per frame and per piano key, how likely a note is
//! SOUNDING there (`note`) and how likely one STARTS there (`onset`).
//! A note is taken from every onset peak strong enough, followed
//! forward while its key keeps sounding (with a short tolerance for a
//! dip), and its energy is then used up so no second note is read
//! from the same sound — on its key and on the two beside it, where a
//! slightly sharp or flat string also rings.
//!
//! ⚠️ One step of the reference is left out on purpose: its "melodia
//! trick", which afterwards adds notes that never had an onset, read
//! from whatever energy is left. What BeatByte asks this for is which
//! notes were **struck together** — a chord is a strike — and a note
//! with no onset is exactly the thing that answer must not contain.
//!
//! Pure — tested.

use crate::frames::{MIDI_OFFSET, PITCHES, frame_time};

/// An onset peak weaker than this starts no note (the reference's
/// default).
pub const ONSET_THRESHOLD: f32 = 0.5;
/// A key sounding weaker than this counts as silent.
pub const FRAME_THRESHOLD: f32 = 0.3;
/// A note must last more than this many frames (127.7 ms, the
/// reference's default, at 86 frames a second).
pub const MIN_NOTE_FRAMES: usize = 11;
/// How many silent frames in a row end a note.
pub const ENERGY_TOLERANCE: usize = 11;
/// How far back the inferred onsets look.
const DIFF_FRAMES: usize = 2;

/// One transcribed note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolyNote {
    /// When it starts, seconds.
    pub start_s: f64,
    /// When it ends, seconds.
    pub end_s: f64,
    /// Its pitch, MIDI.
    pub midi: u8,
    /// Its mean sounding likelihood, `0`–`1`.
    pub amplitude: f32,
}

/// The model's answers for a whole song, stitched: `frames` rows of
/// [`PITCHES`] values each, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Posteriors {
    /// Frames in the timeline.
    pub frames: usize,
    /// How likely each key is sounding.
    pub note: Vec<f32>,
    /// How likely a note starts on each key.
    pub onset: Vec<f32>,
}

impl Posteriors {
    fn at(values: &[f32], frame: usize, pitch: usize) -> f32 {
        values[frame * PITCHES + pitch]
    }
}

/// The onsets, strengthened by where the sounding likelihood RISES:
/// a note the onset head missed but that clearly starts is still a
/// start. The rise is the smaller of the one- and two-frame
/// differences, scaled to the onsets' own range, and each value is
/// the larger of the two readings.
#[must_use]
pub fn inferred_onsets(posteriors: &Posteriors) -> Vec<f32> {
    let (frames, note) = (posteriors.frames, &posteriors.note);
    let mut rise = vec![0.0f32; frames * PITCHES];
    for frame in DIFF_FRAMES..frames {
        for pitch in 0..PITCHES {
            let here = Posteriors::at(note, frame, pitch);
            let smallest = (1..=DIFF_FRAMES)
                .map(|back| here - Posteriors::at(note, frame - back, pitch))
                .fold(f32::INFINITY, f32::min);
            rise[frame * PITCHES + pitch] = smallest.max(0.0);
        }
    }
    let onset_max = posteriors.onset.iter().copied().fold(0.0f32, f32::max);
    let rise_max = rise.iter().copied().fold(0.0f32, f32::max);
    posteriors
        .onset
        .iter()
        .zip(&rise)
        .map(|(onset, r)| {
            let scaled = if rise_max > 0.0 {
                onset_max * r / rise_max
            } else {
                0.0
            };
            onset.max(scaled)
        })
        .collect()
}

/// Every note the posteriors hold, by the reference's rule without its
/// melodia trick (see the module notes). Deterministic: onsets are
/// taken latest first, and at one frame the highest key first, as the
/// reference does.
#[must_use]
pub fn notes(posteriors: &Posteriors) -> Vec<PolyNote> {
    let frames = posteriors.frames;
    if frames < 3 {
        return Vec::new();
    }
    let onsets = inferred_onsets(posteriors);
    // Peaks in time: strictly above both neighbours, strong enough.
    let mut starts: Vec<(usize, usize)> = Vec::new();
    for frame in 1..frames - 1 {
        for pitch in 0..PITCHES {
            let here = Posteriors::at(&onsets, frame, pitch);
            if here >= ONSET_THRESHOLD
                && here > Posteriors::at(&onsets, frame - 1, pitch)
                && here > Posteriors::at(&onsets, frame + 1, pitch)
            {
                starts.push((frame, pitch));
            }
        }
    }
    let mut remaining = posteriors.note.clone();
    let mut out = Vec::new();
    for &(start, pitch) in starts.iter().rev() {
        if start >= frames - 1 {
            continue;
        }
        let mut end = start + 1;
        let mut quiet = 0usize;
        while end < frames - 1 && quiet < ENERGY_TOLERANCE {
            if Posteriors::at(&remaining, end, pitch) < FRAME_THRESHOLD {
                quiet += 1;
            } else {
                quiet = 0;
            }
            end += 1;
        }
        end -= quiet;
        if end - start <= MIN_NOTE_FRAMES {
            continue;
        }
        for frame in start..end {
            for neighbour in pitch.saturating_sub(1)..=(pitch + 1).min(PITCHES - 1) {
                remaining[frame * PITCHES + neighbour] = 0.0;
            }
        }
        let amplitude = (start..end)
            .map(|frame| Posteriors::at(&posteriors.note, frame, pitch))
            .sum::<f32>()
            / (end - start) as f32;
        out.push(PolyNote {
            start_s: frame_time(start),
            end_s: frame_time(end),
            midi: MIDI_OFFSET + pitch as u8,
            amplitude,
        });
    }
    out.sort_by(|a, b| a.start_s.total_cmp(&b.start_s).then(a.midi.cmp(&b.midi)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Posteriors with nothing in them, `frames` long.
    fn silence(frames: usize) -> Posteriors {
        Posteriors {
            frames,
            note: vec![0.0; frames * PITCHES],
            onset: vec![0.0; frames * PITCHES],
        }
    }

    /// A key sounding over `from..to`, with an onset peak at `from`.
    fn strike(p: &mut Posteriors, pitch: usize, from: usize, to: usize) {
        for frame in from..to {
            p.note[frame * PITCHES + pitch] = 0.8;
        }
        p.onset[from * PITCHES + pitch] = 0.9;
    }

    #[test]
    fn a_struck_key_becomes_one_note_with_its_times() {
        let mut p = silence(200);
        strike(&mut p, 48, 20, 80); // MIDI 69, A4
        let found = notes(&p);
        assert_eq!(found.len(), 1, "{found:?}");
        let note = found[0];
        assert_eq!(note.midi, 69);
        assert!((note.start_s - frame_time(20)).abs() < 1e-12);
        assert!((note.end_s - frame_time(80)).abs() < 1e-12);
        assert!((note.amplitude - 0.8).abs() < 1e-6);
    }

    /// Three keys struck at once are three notes starting together —
    /// the thing the chord ingredient reads.
    #[test]
    fn a_chord_is_three_notes_that_start_together() {
        let mut p = silence(200);
        for pitch in [28, 35, 40] {
            strike(&mut p, pitch, 50, 120);
        }
        let found = notes(&p);
        assert_eq!(
            found.iter().map(|n| n.midi).collect::<Vec<_>>(),
            vec![49, 56, 61]
        );
        assert!(
            found
                .iter()
                .all(|n| (n.start_s - frame_time(50)).abs() < 1e-12)
        );
    }

    #[test]
    fn a_short_blip_is_not_a_note() {
        let mut p = silence(200);
        strike(&mut p, 40, 20, 20 + MIN_NOTE_FRAMES);
        assert!(notes(&p).is_empty());
        let mut p = silence(200);
        strike(&mut p, 40, 20, 22 + MIN_NOTE_FRAMES);
        assert_eq!(notes(&p).len(), 1);
    }

    #[test]
    fn a_weak_onset_starts_nothing() {
        let mut p = silence(200);
        strike(&mut p, 40, 20, 80);
        p.onset[20 * PITCHES + 40] = ONSET_THRESHOLD - 0.01;
        // The inferred onset (the rise) rescales to the onsets' own
        // maximum, which is now under the threshold too.
        assert!(notes(&p).is_empty());
    }

    /// A dip shorter than the tolerance does not end a note; a longer
    /// silence does. ⚠️ The dip here does not return in a leap: a key
    /// that falls silent and comes back strongly IS a new strike to
    /// the inferred onsets (as in the reference), which is a different
    /// question from the tolerance.
    #[test]
    fn a_short_dip_is_bridged_and_a_long_one_ends_the_note() {
        let mut p = silence(300);
        strike(&mut p, 40, 20, 60);
        for frame in 60..65 {
            p.note[frame * PITCHES + 40] = FRAME_THRESHOLD - 0.05;
        }
        for frame in 65..100 {
            p.note[frame * PITCHES + 40] = FRAME_THRESHOLD + 0.05;
        }
        let found = notes(&p);
        assert_eq!(found.len(), 1, "a five-frame dip split the note: {found:?}");
        assert!((found[0].end_s - frame_time(100)).abs() < 1e-12);

        let mut p = silence(300);
        strike(&mut p, 40, 20, 60);
        for frame in 60 + ENERGY_TOLERANCE + 5..120 {
            p.note[frame * PITCHES + 40] = FRAME_THRESHOLD + 0.05;
        }
        let found = notes(&p);
        assert!((found[0].end_s - frame_time(60)).abs() < 1e-12);
    }

    /// ⚠️ Its energy is used up — on the key AND beside it — so a
    /// second onset inside a note that is already taken reads nothing
    /// new, and neither does a neighbour ringing along.
    #[test]
    fn a_taken_sound_is_not_read_twice() {
        let mut p = silence(300);
        strike(&mut p, 40, 20, 150);
        // A later onset on the same sustained key, and the key beside
        // it ringing with the same shape.
        p.onset[30 * PITCHES + 40] = 0.95;
        for frame in 20..150 {
            p.note[frame * PITCHES + 41] = 0.8;
        }
        p.onset[20 * PITCHES + 41] = 0.9;
        let found = notes(&p);
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// A start the onset head missed is inferred from the rise of the
    /// sounding likelihood.
    #[test]
    fn a_rise_without_an_onset_still_starts_a_note() {
        let mut p = silence(200);
        strike(&mut p, 30, 20, 80); // gives the onsets a scale
        for frame in 100..160 {
            p.note[frame * PITCHES + 50] = 0.9;
        }
        let found = notes(&p);
        assert!(found.iter().any(|n| n.midi == 71), "{found:?}");
    }
}
