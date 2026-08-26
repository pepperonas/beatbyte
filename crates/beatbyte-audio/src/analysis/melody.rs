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

use beatbyte_core::music::{MelodyNote, Onset};
use realfft::RealFftPlanner;

use crate::decode::AudioData;

/// Configuration for melody extraction.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// DP penalty per semitone of pitch jump between frames — the
    /// temporal-continuity term. Swept (0.035 / 0.09 / 0.18): 0.09
    /// makes the drums-and-bass scene perfect (F1 0.92 → 1.00) and
    /// going further changes nothing, so continuity is worth exactly
    /// this much and no more.
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
    /// Extra weight given to pitches that start on a detected attack
    /// (0 disables the preference entirely).
    pub struck_boost: f32,
    /// How long that preference lasts after the attack, in seconds.
    pub struck_hold_s: f64,
    /// Minimum onset strength (relative to its local peak) for an
    /// attack to be allowed to END a note.
    ///
    /// Left at zero after a measured sweep (0.0 / 0.4 / 0.6): raising
    /// it was expected to protect held notes in a dense mix, and it
    /// barely does (10 → 33 long notes on a real track) while costing
    /// real accuracy on the voice and syncopation scenes (F1 0.50 →
    /// 0.33 and 0.91 → 0.86). The knob stays because the hypothesis
    /// is reasonable and someone will want to retest it on other
    /// material — but it is off, and the numbers say why.
    pub split_min_strength: f32,
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
            jump_penalty: 0.09,
            switch_penalty: 0.09,
            min_note_s: 0.09,
            bridge_gap_s: 0.12,
            sustained_ratio_floor: 0.35,
            lonely_min_s: 0.22,
            neighbor_window_s: 0.6,
            struck_boost: 0.6,
            struck_hold_s: 0.35,
            split_min_strength: 0.0,
        }
    }
}

/// Extract the lead melody from decoded audio. Returns notes
/// ascending by start time; empty when nothing tonal stands out.
#[must_use]
pub fn extract_melody(
    audio: &AudioData,
    config: &MelodyConfig,
    onsets: &[Onset],
) -> Vec<MelodyNote> {
    let spectrogram = stft(audio, config.window, config.hop);
    if spectrogram.frames.is_empty() {
        return Vec::new();
    }
    let harmonic = harmonic_part(
        &spectrogram.frames,
        config.hpss_time_halfwidth,
        config.hpss_freq_halfwidth,
    );
    let mut salience = salience_map(&harmonic, spectrogram.bin_hz, config);
    let attacks = attack_frames(
        onsets,
        spectrogram.hop_s,
        spectrogram.frame_offset_s,
        salience.len(),
        config.split_min_strength,
    );
    favour_struck_voices(&mut salience, &attacks, spectrogram.hop_s, config);
    let track = track_contour(&salience, config);
    segment_notes(
        &track,
        spectrogram.hop_s,
        spectrogram.frame_offset_s,
        &attacks,
        config,
    )
}

/// Frames at which something was struck, as a lookup by frame index.
///
/// A repeated note at the same pitch is invisible to a pitch tracker —
/// the contour simply continues — so without this a plucked scale
/// merges into single long events. What separates them is the attack,
/// and that is exactly what the onset stage measures.
fn attack_frames(
    onsets: &[Onset],
    hop_s: f64,
    frame_offset_s: f64,
    frames: usize,
    min_strength: f32,
) -> Vec<bool> {
    let mut marks = vec![false; frames];
    if hop_s <= 0.0 {
        return marks;
    }
    for onset in onsets {
        if onset.strength < min_strength {
            continue;
        }
        let frame = ((onset.time_s - frame_offset_s) / hop_s).round();
        if frame >= 0.0 && (frame as usize) < frames {
            marks[frame as usize] = true;
        }
    }
    marks
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
            for h in 1..=8u32 {
                let exact = f64::from(h) * f0 / bin_hz;
                // Interpolate at the EXACT bin. Taking the maximum of
                // three neighbouring bins destroys resolution exactly
                // where a guitar lives: at 82 Hz with 10.8 Hz bins,
                // three adjacent semitones share a bin, so a ±1-bin
                // maximum makes them literally indistinguishable
                // (measured: 3 % pitch accuracy on a low riff).
                let Some(magnitude) = interpolate(frame, exact) else {
                    break;
                };
                sum += weight * magnitude;
                weight *= 0.85;
            }
            *slot = sum * register_weight(midi) as f32;
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
    suppress_sub_octaves(&mut map);
    map
}

/// How much of a candidate's salience survives when the octave above
/// it is nearly as salient. See [`suppress_sub_octaves`].
const SUB_OCTAVE_KEEP: f32 = 0.35;

/// Threshold at which the octave above is considered "just as
/// salient", meaning the lower candidate is its shadow.
const SUB_OCTAVE_RATIO: f32 = 0.75;

/// Remove sub-octave shadows from the salience map.
///
/// Harmonic summation cannot tell a note from the pitch an octave
/// below it: every even harmonic of the lower candidate lands exactly
/// on a partial of the real note, so F0/2 collects roughly half the
/// evidence for free. The signature that distinguishes them is
/// asymmetric — a real note's octave-up candidate only catches the
/// weaker even partials (~30 %), while a shadow's octave-up candidate
/// catches everything (~100 %). Comparing each candidate with the one
/// twelve semitones above it therefore separates them cleanly.
///
/// Comparisons read from a snapshot so the suppression cannot
/// cascade down an octave chain.
fn suppress_sub_octaves(map: &mut [Vec<f32>]) {
    for frame in map.iter_mut() {
        let original = frame.clone();
        for (s, value) in frame.iter_mut().enumerate() {
            let Some(&above) = original.get(s + 12) else {
                continue;
            };
            if *value > 0.0 && above > SUB_OCTAVE_RATIO * original[s] {
                *value *= SUB_OCTAVE_KEEP;
            }
        }
    }
}

/// How far a segment's pitch may wander from the note it started on
/// before it counts as a different note. One semitone would break on
/// vibrato that crosses a semitone boundary; three would merge a
/// stepwise melody.
const ANCHOR_DRIFT: i32 = 3;

/// How much the tracked pitch's own salience must rise at an onset
/// for that onset to count as a re-articulation of this voice rather
/// than something else being struck nearby.
///
/// Measured sweep (1.25 / 1.6 / 2.2): raising it buys very few extra
/// long notes on a real pop track (36 → 47 held tones) and costs real
/// transcription accuracy on the riff and syncopation scenes
/// (F1 0.96 → 0.81 and 0.98 → 0.86). Accuracy wins.
const REARTICULATION_RISE: f32 = 1.25;

/// Magnitude at a fractional bin, linearly interpolated. `None` once
/// the harmonic runs past the spectrum.
fn interpolate(frame: &[f32], bin: f64) -> Option<f32> {
    if bin < 0.0 {
        return None;
    }
    let low = bin.floor();
    let index = low as usize;
    let upper = frame.get(index + 1)?;
    let lower = *frame.get(index)?;
    let fraction = (bin - low) as f32;
    Some(lower.mul_add(1.0 - fraction, upper * fraction))
}

/// Preference for the register a guitar chart lives in.
///
/// A narrow Gaussian centred on MIDI 66 was fitted to one pop song to
/// stop its bassline winning the tracker — and it then penalised the
/// guitar's own low E (MIDI 40) by a factor of four, which is the
/// opposite of what a guitar game needs. The honest shape is flat
/// across the neck and rolls off outside it: the bass guitar's range
/// below the low E stays suppressed, everything on the fretboard is
/// treated equally.
fn register_weight(midi: i32) -> f64 {
    const LOW: f64 = 40.0; // E2, the guitar's low E
    const HIGH: f64 = 84.0; // C6, past the top of most necks
    let midi = f64::from(midi);
    let distance = if midi < LOW {
        LOW - midi
    } else if midi > HIGH {
        midi - HIGH
    } else {
        return 1.0;
    };
    (-0.5 * (distance / 7.0).powi(2)).exp()
}

/// Boost pitches that were STRUCK over pitches that faded in.
///
/// A plucked string and a voice are separated by physics, not by
/// loudness: the string's energy at its pitch appears within a few
/// milliseconds of an attack, while a sung note swells into place and
/// its start rarely coincides with anything percussive. Tracking by
/// salience alone therefore follows whichever voice is loudest — a
/// scene with a loud voice over a quieter riff had the tracker
/// following the singer almost exclusively.
///
/// For every frame the onset stage called an attack, any pitch whose
/// own salience rises there is boosted for the length of a note. This
/// is deliberately a preference and not a filter: a guitar line that
/// happens to enter softly is still tracked, just not favoured.
fn favour_struck_voices(
    salience: &mut [Vec<f32>],
    attacks: &[bool],
    hop_s: f64,
    config: &MelodyConfig,
) {
    if config.struck_boost <= 0.0 || hop_s <= 0.0 || salience.len() < 3 {
        return;
    }
    let hold = ((config.struck_hold_s / hop_s).round() as usize).max(1);
    let look_back = ((0.035 / hop_s).round() as usize).max(1);
    let states = salience.first().map_or(0, Vec::len);
    // Collect first, apply second: a boost must never feed the test
    // for the boost of a later frame.
    let mut boosts: Vec<(usize, usize)> = Vec::new();
    for (t, is_attack) in attacks.iter().enumerate() {
        if !*is_attack || t < look_back {
            continue;
        }
        // Only the pitch that rose MOST at this attack. A strike
        // lifts a whole family of candidates — harmonics, the
        // sub-octave, near neighbours — and boosting all of them
        // rewards the wrong one about as often as the right one
        // (measured: it improved the two distractor scenes and hurt
        // every clean one).
        let rises: Vec<f32> = (0..states)
            .map(|s| {
                let before = salience[t - look_back][s];
                let now = salience[t][s];
                if now <= 0.0 || now <= before * STRUCK_RISE {
                    0.0
                } else {
                    now - before
                }
            })
            .collect();
        let best = rises
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, rise)| *rise > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        // …and anything that rose nearly as much, because a chord
        // strikes several pitches at once and picking one of a triad
        // arbitrarily is worse than picking none.
        if let Some((_, top)) = best {
            for (s, rise) in rises.iter().enumerate() {
                if *rise >= top * STRUCK_PEERS {
                    boosts.push((t, s));
                }
            }
        }
    }
    for (t, s) in boosts {
        for frame in salience.iter_mut().skip(t).take(hold) {
            frame[s] *= 1.0 + config.struck_boost;
        }
    }
}

/// How sharply a pitch's salience must rise at an attack to count as
/// having been struck rather than merely present.
const STRUCK_RISE: f32 = 1.4;

/// How close to the strongest rise at an attack another pitch must
/// come to count as struck by the same event — the difference between
/// a chord and a harmonic sitting on the coat-tails of one note.
const STRUCK_PEERS: f32 = 0.7;

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
    attacks: &[bool],
    config: &MelodyConfig,
) -> Vec<MelodyNote> {
    let bridge_frames = (config.bridge_gap_s / hop_s).round() as usize;
    let mut notes: Vec<MelodyNote> = Vec::new();
    let mut start: Option<usize> = None;
    let mut pitch = 0i32;
    let mut anchor = 0i32;
    let mut previous_salience = 0.0f32;
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
                    // Two bounds, because either alone is wrong.
                    // Against the RUNNING pitch, so a scoop or
                    // vibrato does not chop a held tone into
                    // fragments; and against the segment's ANCHOR, so
                    // a stepwise line cannot drift a semitone at a
                    // time into one long smear (measured on a plain
                    // scale: four notes merged into a single 1.8 s
                    // event).
                    // An attack ends the previous note even when the
                    // pitch does not move — but only if it belongs to
                    // THIS voice. A drum hit over a held guitar note
                    // is an onset and must not split it; a genuine
                    // re-articulation makes the tracked pitch's own
                    // salience jump. And a note's own attack cannot
                    // split it, hence the minimum length.
                    Some(begin)
                        if (midi - pitch).abs() <= 1
                            && (midi - anchor).abs() <= ANCHOR_DRIFT
                            && !(attacks.get(t).copied().unwrap_or(false)
                                && (t - begin) as f64 * hop_s >= config.min_note_s
                                && salience > previous_salience * REARTICULATION_RISE) =>
                    {
                        previous_salience = salience;
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
                        anchor = midi;
                        previous_salience = salience;
                        midis.push(midi);
                        saliences.push(salience);
                        gap = 0;
                    }
                    None => {
                        start = Some(t);
                        pitch = midi;
                        anchor = midi;
                        previous_salience = salience;
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
    use crate::analysis::onset::{OnsetConfig, analyze_onsets};
    use crate::synth;

    /// The onsets the melody stage is given in the real pipeline.
    /// Segmentation depends on them, so the tests must exercise the
    /// same path rather than a convenient empty slice.
    fn onsets_of(audio: &AudioData) -> Vec<Onset> {
        analyze_onsets(audio, &OnsetConfig::default()).onsets
    }

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
        let notes = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
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
        let notes = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
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
        let notes = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
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
        let notes = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
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
        let a = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
        let b = extract_melody(&audio, &MelodyConfig::default(), &onsets_of(&audio));
        assert_eq!(a, b);
    }
}
