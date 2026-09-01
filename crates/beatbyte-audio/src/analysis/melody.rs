//! Lead-melody extraction: the missing half of Guitar-Hero-style
//! charting.
//!
//! Official GH/Rock-Band charts are hand-authored against the guitar
//! stem; the conventions the charters follow are what players
//! recognize as "the chart follows the song": lanes track the RIFF's
//! pitch contour (green low → orange high) and long notes are
//! sustains of their REAL held length. To adapt that without stems,
//! this stage separates the tonal layer from the percussive one and
//! transcribes it into [`MelodyNote`]s (pitch + true start/end):
//!
//! 1. **STFT** (93 ms windows, 23 ms hops at the ~22 kHz analysis
//!    rate — pitch needs longer windows than onset flux).
//! 2. **HPSS** (harmonic/percussive separation via median filtering,
//!    Fitzgerald 2010): tones are horizontal ridges in the
//!    spectrogram, hits are vertical spikes; a time-median enhances
//!    the former, a frequency-median the latter, and a Wiener-style
//!    soft mask keeps the harmonic part.
//! 3. **Pitch salience** per frame by harmonic summation over a
//!    semitone grid (A1..E6, 6 harmonics, decaying weights, ±1-bin
//!    mistuning tolerance).
//! 4. **Contour tracking** by dynamic programming over
//!    semitone states + an "unvoiced" state: jump penalties keep the
//!    track from teleporting to accompaniment; flat (noisy) frames
//!    fall to unvoiced on their own.
//! 5. **Segmentation**: stable voiced runs become notes with median
//!    pitch, true end time and normalized strength.
//!
//! Everything is pure functions over buffers — same audio in, same
//! melody out.

use beatbyte_core::music::MelodyNote;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::decode::AudioData;

/// Configuration for melody extraction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MelodyConfig {
    /// STFT window size in samples (power of two).
    pub window: usize,
    /// Hop between frames in samples.
    pub hop: usize,
    /// Median-filter half-width across time (frames) for the
    /// harmonic enhancement.
    pub hpss_time_halfwidth: usize,
    /// Median-filter half-width across frequency (bins) for the
    /// percussive enhancement.
    pub hpss_freq_halfwidth: usize,
    /// Lowest candidate pitch as a MIDI note number (40 = E2, the
    /// guitar's low E at 82.4 Hz).
    pub midi_min: i32,
    /// Highest candidate pitch as a MIDI note number (88 = E6).
    pub midi_max: i32,
    /// DP penalty per semitone of pitch jump between frames.
    pub jump_penalty: f32,
    /// DP penalty for switching voiced <-> unvoiced.
    pub switch_penalty: f32,
    /// Minimum note length in seconds (shorter segments are noise).
    pub min_note_s: f64,
    /// Unvoiced gaps up to this length inside a stable pitch are
    /// bridged (tremolo picking, brief masking by a drum hit).
    pub bridge_gap_s: f64,
    /// A segment is kept only if its median salience is at least this
    /// fraction of its peak. A HELD tone keeps its level (ratio near
    /// 1); a transient's window-smear decays exponentially (ratio
    /// near 0) — this is what keeps drum hits out of the melody.
    pub sustained_ratio_floor: f32,
    /// Notes shorter than this are kept only when another melody
    /// note starts within [`MelodyConfig::neighbor_window_s`] — real
    /// riffs are runs; a solitary sub-quarter-second blip is a drum
    /// hit's window-smear, physically indistinguishable by shape.
    pub lonely_min_s: f64,
    /// Neighborhood used by the loneliness rule, in seconds.
    pub neighbor_window_s: f64,
}

impl Default for MelodyConfig {
    fn default() -> Self {
        MelodyConfig {
            window: 2048,
            hop: 512,
            hpss_time_halfwidth: 4,
            hpss_freq_halfwidth: 4,
            // Low E of a standard-tuned guitar: this IS a guitar
            // game, and everything below is bass/kick territory.
            midi_min: 40,
            midi_max: 88,
            jump_penalty: 0.035,
            switch_penalty: 0.09,
            min_note_s: 0.09,
            bridge_gap_s: 0.12,
            sustained_ratio_floor: 0.35,
            lonely_min_s: 0.22,
            neighbor_window_s: 0.6,
        }
    }
}

/// Extract the lead melody from decoded audio. Returns notes
/// ascending by start time; empty when nothing tonal stands out.
#[must_use]
pub fn extract_melody(audio: &AudioData, config: &MelodyConfig) -> Vec<MelodyNote> {
    let spectrogram = stft(audio, config.window, config.hop);
    if spectrogram.frames.is_empty() {
        return Vec::new();
    }
    let harmonic = harmonic_part(
        &spectrogram.frames,
        config.hpss_time_halfwidth,
        config.hpss_freq_halfwidth,
    );
    let salience = salience_map(&harmonic, spectrogram.bin_hz, config);
    let track = track_contour(&salience, config);
    segment_notes(
        &track,
        spectrogram.hop_s,
        spectrogram.frame_offset_s,
        config,
    )
}

/// Magnitude spectrogram: `frames[t][bin]`.
struct Spectrogram {
    frames: Vec<Vec<f32>>,
    bin_hz: f64,
    hop_s: f64,
    /// Time of the center of frame 0 (the window looks ahead).
    frame_offset_s: f64,
}

/// Hann-windowed magnitude STFT.
fn stft(audio: &AudioData, window: usize, hop: usize) -> Spectrogram {
    let samples = audio.samples();
    let rate = f64::from(audio.sample_rate().max(1));
    let hop_s = hop as f64 / rate;
    let bin_hz = rate / window as f64;
    let frame_offset_s = window as f64 / 2.0 / rate;
    if samples.len() < window {
        return Spectrogram {
            frames: Vec::new(),
            bin_hz,
            hop_s,
            frame_offset_s,
        };
    }
    let count = (samples.len() - window) / hop + 1;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(window);
    let hann: Vec<f32> = (0..window)
        .map(|i| {
            let x = i as f32 / window as f32;
            0.5 - 0.5 * (std::f32::consts::TAU * x).cos()
        })
        .collect();
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut frames = Vec::with_capacity(count);
    for frame in 0..count {
        let start = frame * hop;
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples[start + i] * hann[i];
        }
        if fft.process(&mut input, &mut output).is_err() {
            break;
        }
        frames.push(output.iter().map(|c| c.norm()).collect());
    }
    Spectrogram {
        frames,
        bin_hz,
        hop_s,
        frame_offset_s,
    }
}

/// Median of a small scratch slice (sorts in place).
fn median(scratch: &mut [f32]) -> f32 {
    if scratch.is_empty() {
        return 0.0;
    }
    scratch.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    scratch[scratch.len() / 2]
}

/// HPSS: keep the harmonic (tonal) part of the spectrogram.
/// Time-median per bin = harmonic enhancement; frequency-median per
/// frame = percussive enhancement; soft mask `h²/(h²+p²)`.
fn harmonic_part(
    frames: &[Vec<f32>],
    time_halfwidth: usize,
    freq_halfwidth: usize,
) -> Vec<Vec<f32>> {
    let bins = frames.first().map_or(0, Vec::len);
    let count = frames.len();
    let mut scratch = Vec::new();
    let mut result = vec![vec![0.0f32; bins]; count];
    for t in 0..count {
        let t0 = t.saturating_sub(time_halfwidth);
        let t1 = (t + time_halfwidth + 1).min(count);
        for bin in 0..bins {
            // Harmonic estimate: median across neighboring frames.
            scratch.clear();
            scratch.extend((t0..t1).map(|u| frames[u][bin]));
            let h = median(&mut scratch);
            // Percussive estimate: median across neighboring bins.
            let b0 = bin.saturating_sub(freq_halfwidth);
            let b1 = (bin + freq_halfwidth + 1).min(bins);
            scratch.clear();
            scratch.extend(frames[t][b0..b1].iter().copied());
            let p = median(&mut scratch);
            let mask = (h * h) / (h * h + p * p + f32::EPSILON);
            result[t][bin] = frames[t][bin] * mask;
        }
    }
    result
}

/// Per-frame pitch salience over the semitone grid: harmonic
/// summation with decaying weights and ±1-bin mistuning tolerance.
/// Returned matrix is normalized so the global maximum is 1.0.
fn salience_map(harmonic: &[Vec<f32>], bin_hz: f64, config: &MelodyConfig) -> Vec<Vec<f32>> {
    let states = (config.midi_max - config.midi_min + 1).max(1) as usize;
    let mut map = vec![vec![0.0f32; states]; harmonic.len()];
    let mut global_max = 0.0f32;
    for (frame, row) in harmonic.iter().zip(map.iter_mut()) {
        for (s, slot) in row.iter_mut().enumerate() {
            let midi = config.midi_min + s as i32;
            let f0 = 440.0 * 2f64.powf((f64::from(midi) - 69.0) / 12.0);
            let mut sum = 0.0f32;
            let mut weight = 1.0f32;
            for h in 1..=6u32 {
                let bin = (f64::from(h) * f0 / bin_hz).round() as usize;
                if bin + 1 >= frame.len() {
                    break;
                }
                let peak = frame[bin - 1].max(frame[bin]).max(frame[bin + 1]);
                sum += weight * peak;
                weight *= 0.85;
            }
            // Register weighting (the Melodia idea): the LEAD lives
            // in the mid register; without this the bassline — loud,
            // long, stable — wins the tracker on every dense mix
            // (measured on a real track: pitch histogram peaked at
            // E2–G#2, the bass, not the vocal).
            let register = (-(f64::from(midi) - 66.0).powi(2) / (2.0 * 16.0 * 16.0)).exp();
            *slot = sum * register as f32;
            global_max = global_max.max(sum);
        }
    }
    if global_max > 0.0 {
        for frame in &mut map {
            for value in frame.iter_mut() {
                *value /= global_max;
            }
        }
    }
    map
}

/// One tracked frame: the chosen semitone, or unvoiced.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tracked {
    Unvoiced,
    Voiced { midi: i32, salience: f32 },
}

/// Dynamic-programming contour tracking over semitone states + one
/// unvoiced state. Flat frames (noise, clicks) score higher as
/// unvoiced; jump penalties keep the melody from teleporting.
fn track_contour(salience: &[Vec<f32>], config: &MelodyConfig) -> Vec<Tracked> {
    let count = salience.len();
    let states = salience.first().map_or(0, Vec::len);
    if count == 0 || states == 0 {
        return Vec::new();
    }
    let unvoiced = states; // extra state index
    // Unvoiced reward per frame: beats voiced only when the frame is
    // FLAT (best barely above the mean — noise), never when a real
    // tone towers over the rest.
    let unvoiced_score: Vec<f32> = salience
        .iter()
        .map(|frame| {
            let mean = frame.iter().sum::<f32>() / states as f32;
            // 2.2, not 3: on a dense mix salience spreads over many
            // candidates and a strong mean muted the whole tracker
            // (real track: only 22% voiced coverage at 3.0).
            (2.2 * mean).max(0.015)
        })
        .collect();
    let score_of = |t: usize, s: usize| -> f32 {
        if s == unvoiced {
            unvoiced_score[t]
        } else {
            salience[t][s]
        }
    };
    let transition = |from: usize, to: usize| -> f32 {
        if from == unvoiced || to == unvoiced {
            if from == to {
                0.0
            } else {
                config.switch_penalty
            }
        } else {
            let jump = (from as i32 - to as i32).abs() as f32;
            (config.jump_penalty * jump).min(0.3)
        }
    };
    let total_states = states + 1;
    let mut best = vec![0.0f32; total_states];
    let mut back = vec![vec![0u16; total_states]; count];
    for (s, slot) in best.iter_mut().enumerate() {
        *slot = score_of(0, s);
    }
    for (t, back_row) in back.iter_mut().enumerate().skip(1) {
        let mut next = vec![f32::MIN; total_states];
        for (to, (next_slot, back_slot)) in next.iter_mut().zip(back_row.iter_mut()).enumerate() {
            let mut best_from = 0usize;
            let mut best_value = f32::MIN;
            for (from, from_best) in best.iter().enumerate() {
                let value = from_best - transition(from, to);
                if value > best_value {
                    best_value = value;
                    best_from = from;
                }
            }
            *next_slot = best_value + score_of(t, to);
            *back_slot = best_from as u16;
        }
        best = next;
    }
    // Backtrack.
    let mut state = (0..total_states)
        .max_by(|a, b| {
            best[*a]
                .partial_cmp(&best[*b])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(unvoiced);
    let mut track = vec![Tracked::Unvoiced; count];
    for t in (0..count).rev() {
        track[t] = if state == unvoiced {
            Tracked::Unvoiced
        } else {
            Tracked::Voiced {
                midi: config.midi_min + state as i32,
                salience: salience[t][state],
            }
        };
        if t > 0 {
            state = usize::from(back[t][state]);
        }
    }
    track
}

/// Group the tracked frames into notes: stable pitch runs with true
/// start/end times; brief unvoiced gaps inside a stable pitch are
/// bridged; strengths normalized to the strongest note.
fn segment_notes(
    track: &[Tracked],
    hop_s: f64,
    frame_offset_s: f64,
    config: &MelodyConfig,
) -> Vec<MelodyNote> {
    let bridge_frames = (config.bridge_gap_s / hop_s).round() as usize;
    let mut notes: Vec<MelodyNote> = Vec::new();
    let mut start: Option<usize> = None;
    let mut pitch = 0i32;
    let mut midis: Vec<i32> = Vec::new();
    let mut saliences: Vec<f32> = Vec::new();
    let mut gap = 0usize;
    let time_of = |frame: usize| frame_offset_s + frame as f64 * hop_s;
    let close = |start: &mut Option<usize>,
                 end_frame: usize,
                 midis: &mut Vec<i32>,
                 saliences: &mut Vec<f32>,
                 notes: &mut Vec<MelodyNote>| {
        if let Some(s) = start.take() {
            let time_s = time_of(s);
            let end_s = time_of(end_frame);
            let pitch = {
                let mut sorted = midis.clone();
                sorted.sort_unstable();
                sorted.get(sorted.len() / 2).copied().unwrap_or(0)
            };
            let peak = saliences.iter().fold(0.0f32, |m, v| m.max(*v));
            let mut sorted = saliences.clone();
            let sustained = if peak > 0.0 {
                median(&mut sorted) / peak
            } else {
                0.0
            };
            // Length + shape: long enough AND actually held (a
            // transient's smear decays — a tone does not).
            if end_s - time_s >= config.min_note_s
                && !saliences.is_empty()
                && sustained >= config.sustained_ratio_floor
            {
                notes.push(MelodyNote {
                    time_s,
                    end_s,
                    midi: pitch as f32,
                    strength: saliences.iter().sum::<f32>() / saliences.len() as f32,
                });
            }
            saliences.clear();
            midis.clear();
        }
    };
    for (t, tracked) in track.iter().enumerate() {
        match *tracked {
            Tracked::Voiced { midi, salience } => {
                match start {
                    // Compare against the RUNNING pitch (previous
                    // frame), not the segment's first frame: vocals
                    // scoop and wobble, and the anchored comparison
                    // chopped real held notes into fragments.
                    Some(_) if (midi - pitch).abs() <= 1 => {
                        pitch = midi;
                        midis.push(midi);
                        saliences.push(salience);
                        gap = 0;
                    }
                    Some(_) => {
                        // Pitch moved: close and open a new note.
                        close(
                            &mut start,
                            t.saturating_sub(gap + 1) + 1,
                            &mut midis,
                            &mut saliences,
                            &mut notes,
                        );
                        start = Some(t);
                        pitch = midi;
                        midis.push(midi);
                        saliences.push(salience);
                        gap = 0;
                    }
                    None => {
                        start = Some(t);
                        pitch = midi;
                        midis.push(midi);
                        saliences.push(salience);
                        gap = 0;
                    }
                }
            }
            Tracked::Unvoiced => {
                if start.is_some() {
                    gap += 1;
                    if gap > bridge_frames {
                        close(
                            &mut start,
                            t - gap + 1,
                            &mut midis,
                            &mut saliences,
                            &mut notes,
                        );
                        gap = 0;
                    }
                }
            }
        }
    }
    let end = track.len();
    close(
        &mut start,
        end.saturating_sub(gap),
        &mut midis,
        &mut saliences,
        &mut notes,
    );
    // Loneliness rule: a short blip with no melodic neighbors is a
    // transient, not a riff note.
    let starts: Vec<f64> = notes.iter().map(|n| n.time_s).collect();
    let lonely = |i: usize| -> bool {
        let me = &notes[i];
        if me.len_s() >= config.lonely_min_s {
            return false;
        }
        !starts
            .iter()
            .enumerate()
            .any(|(j, &start)| j != i && (start - me.time_s).abs() <= config.neighbor_window_s)
    };
    let keep: Vec<bool> = (0..notes.len()).map(|i| !lonely(i)).collect();
    let mut index = 0;
    notes.retain(|_| {
        let kept = keep[index];
        index += 1;
        kept
    });
    // Normalize strengths.
    let max = notes.iter().map(|n| n.strength).fold(0.0f32, f32::max);
    if max > 0.0 {
        for note in &mut notes {
            note.strength /= max;
        }
    }
    notes
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::synth;

    const RATE: u32 = 22_050;

    /// A steady (non-decaying) tone, unlike `synth::tone_burst`'s
    /// decaying burst — held notes are what this module measures.
    fn held_tone(spans: &[(f64, f64, f64)], duration_s: f64) -> AudioData {
        let count = (duration_s * f64::from(RATE)) as usize;
        let mut samples = vec![0.0f32; count];
        for &(start, len, hz) in spans {
            let begin = (start * f64::from(RATE)) as usize;
            let end = (((start + len) * f64::from(RATE)) as usize).min(count);
            for (i, slot) in samples[begin..end].iter_mut().enumerate() {
                let t = i as f64 / f64::from(RATE);
                // Fundamental + two harmonics: a guitar-ish tone.
                let v = (std::f64::consts::TAU * hz * t).sin() * 0.5
                    + (std::f64::consts::TAU * 2.0 * hz * t).sin() * 0.2
                    + (std::f64::consts::TAU * 3.0 * hz * t).sin() * 0.1;
                *slot += v as f32;
            }
        }
        AudioData::from_mono(samples, RATE)
    }

    #[test]
    fn a_held_tone_becomes_one_note_with_its_true_length() {
        let audio = held_tone(&[(0.5, 0.8, 440.0)], 2.0);
        let notes = extract_melody(&audio, &MelodyConfig::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        let note = notes[0];
        assert!(
            (note.midi - 69.0).abs() <= 1.0,
            "A4 expected: {}",
            note.midi
        );
        assert!((note.time_s - 0.5).abs() < 0.1, "start: {}", note.time_s);
        assert!(
            (note.len_s() - 0.8).abs() < 0.15,
            "true held length expected: {}",
            note.len_s()
        );
    }

    #[test]
    fn two_pitches_become_two_notes_with_the_right_interval() {
        // A4 then E5 (+7 semitones) back to back.
        let audio = held_tone(&[(0.4, 0.5, 440.0), (0.9, 0.5, 659.26)], 2.0);
        let notes = extract_melody(&audio, &MelodyConfig::default());
        assert_eq!(notes.len(), 2, "{notes:?}");
        let interval = notes[1].midi - notes[0].midi;
        assert!(
            (interval - 7.0).abs() <= 1.0,
            "a fifth expected, got {interval}"
        );
        assert!((notes[1].time_s - 0.9).abs() < 0.1);
    }

    #[test]
    fn clicks_alone_yield_no_melody() {
        let times: Vec<f64> = (0..8).map(|i| 0.25 + f64::from(i) * 0.25).collect();
        let audio = synth::click_track(&times, 2.5, RATE);
        let notes = extract_melody(&audio, &MelodyConfig::default());
        assert!(
            notes.is_empty(),
            "percussion must not fake a melody: {notes:?}"
        );
    }

    #[test]
    fn the_tone_survives_clicks_riding_on_it() {
        // The whole point of HPSS: a drum hit through the note must
        // not cut or destroy the tracked tone.
        let mut audio = held_tone(&[(0.4, 1.0, 440.0)], 2.0);
        let mut samples = audio.samples().to_vec();
        for click in [0.6, 0.9, 1.2] {
            synth::add_burst(&mut samples, RATE, click, 180.0, 0.02, 0.9);
        }
        audio = AudioData::from_mono(samples, RATE);
        let notes = extract_melody(&audio, &MelodyConfig::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!((notes[0].midi - 69.0).abs() <= 1.0);
        assert!(
            (notes[0].len_s() - 1.0).abs() < 0.2,
            "clicks must not shorten the note: {}",
            notes[0].len_s()
        );
    }

    #[test]
    fn extraction_is_deterministic() {
        let audio = held_tone(&[(0.3, 0.6, 523.25)], 1.5);
        let a = extract_melody(&audio, &MelodyConfig::default());
        let b = extract_melody(&audio, &MelodyConfig::default());
        assert_eq!(a, b);
    }
}
