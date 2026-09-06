//! Repeated sections, from the song's own self-similarity.
//!
//! A human charter charts a chorus once and pastes it; a generator
//! that reads each chorus afresh charts the same music two ways, and
//! that is the most audible "generated" tell (plan
//! `docs/plans/ai-song-graph-upgrade.md`, C4). This stage finds the
//! pairs of beat spans that are the same music: every beat gets a
//! feature vector (pitch-class chroma and a coarse spectral envelope,
//! both centred on the song's own mean), the diagonals of the
//! self-similarity matrix are scanned for long runs of high
//! similarity, and the longest, most similar, non-overlapping runs
//! are the repeats — aligned to bars where the grid knows them. Pure
//! and deterministic: same samples and beats, same repeats.
//!
//! What it does NOT do: name sections (verse, chorus), or find
//! boundaries between different music. A repeat is a claim that two
//! spans are the same music, nothing more, and the generator only
//! acts on that claim.

use beatbyte_core::music::Repeat;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::decode::AudioData;

/// Parameters of the repeat finder.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StructureConfig {
    /// STFT window in samples (Hann).
    pub window: usize,
    /// STFT hop in samples.
    pub hop: usize,
    /// The shortest repeat worth acting on, in beats (8 bars of 4).
    pub min_repeat_beats: usize,
    /// Moving-average width over a diagonal, in beats, before the
    /// floor is applied — one bad beat must not cut a chorus in two.
    pub smooth_beats: usize,
    /// Smoothed similarity a diagonal must hold to count as a repeat.
    pub similarity_floor: f32,
    /// How many beats a span's start may move to land on a bar line.
    pub bar_snap_beats: usize,
}

impl Default for StructureConfig {
    fn default() -> Self {
        StructureConfig {
            window: 4096,
            hop: 1024,
            min_repeat_beats: 32,
            smooth_beats: 4,
            similarity_floor: 0.6,
            bar_snap_beats: 3,
        }
    }
}

/// Pitch classes in a chroma vector.
const CHROMA: usize = 12;
/// Log-spaced bands of the spectral envelope.
const BANDS: usize = 20;
/// The envelope's frequency range, hertz.
const BAND_LOW_HZ: f64 = 60.0;
const BAND_HIGH_HZ: f64 = 8000.0;
/// Bins outside this range say nothing about pitch class.
const CHROMA_LOW_HZ: f64 = 60.0;
const CHROMA_HIGH_HZ: f64 = 4000.0;
/// A beat whose RMS is under this (≈ −60 dBFS) is silence: it gets
/// no features and matches nothing — a fade-out or a digital tail
/// would otherwise be a perfect repeat of itself (seen on a rip
/// with 115 s of silence: "2:49–3:45 repeats at 3:45–4:42, 1.00").
const SILENCE_RMS: f32 = 0.001;

/// Find the repeated spans of a song. `beats` are the grid's beat
/// times (ascending), `downbeats` its bar starts (may be empty).
#[must_use]
pub fn find_repeats(
    audio: &AudioData,
    beats: &[f64],
    downbeats: &[f64],
    config: &StructureConfig,
) -> Vec<Repeat> {
    let features = beat_features(audio, beats, config);
    let bars = bar_indices(beats, downbeats);
    repeats_from_features(&features, &bars, config)
}

/// The pure half: repeats from per-beat feature vectors and the bar
/// starts as beat indices.
#[must_use]
pub fn repeats_from_features(
    features: &[Vec<f32>],
    bars: &[usize],
    config: &StructureConfig,
) -> Vec<Repeat> {
    let n = features.len();
    let min_len = config.min_repeat_beats.max(2);
    if n < 2 * min_len {
        return Vec::new();
    }
    let mut candidates: Vec<Repeat> = Vec::new();
    for lag in min_len..=(n - min_len) {
        let diagonal: Vec<f32> = (0..n - lag)
            .map(|i| dot(&features[i], &features[i + lag]))
            .collect();
        let smoothed = smooth(&diagonal, config.smooth_beats);
        for (start, len) in grow(
            runs_at_or_above(&smoothed, config.similarity_floor),
            &diagonal,
            config.similarity_floor,
        ) {
            let Some((start, len)) = snap_to_bars(start, len, bars, config.bar_snap_beats) else {
                continue;
            };
            // Two occurrences must not share a beat.
            let len = len.min(lag);
            if len < min_len {
                continue;
            }
            let mean = diagonal[start..start + len].iter().sum::<f32>() / len as f32;
            candidates.push(Repeat {
                first_beat: start,
                second_beat: start + lag,
                beats: len,
                similarity: mean,
            });
        }
    }
    pick(candidates, n)
}

/// One feature vector per beat interval `[beats[i], beats[i+1])`:
/// chroma (12) and a log spectral envelope (20), each dimension
/// centred and scaled over the song, the vector then unit-length so
/// a dot product is a cosine. The last beat gets the frames to the
/// song's end. An interval with no frame is all zeros.
#[must_use]
pub fn beat_features(audio: &AudioData, beats: &[f64], config: &StructureConfig) -> Vec<Vec<f32>> {
    if beats.is_empty() {
        return Vec::new();
    }
    let frames = frame_features(audio, config);
    let hop_s = config.hop as f64 / f64::from(audio.sample_rate().max(1));
    let centre_offset_s = config.window as f64 / 2.0 / f64::from(audio.sample_rate().max(1));
    let dims = CHROMA + BANDS;
    let mut per_beat = vec![vec![0.0f32; dims]; beats.len()];
    let mut counts = vec![0usize; beats.len()];
    for (f, frame) in frames.iter().enumerate() {
        let t = f as f64 * hop_s + centre_offset_s;
        if t < beats[0] {
            continue;
        }
        let i = beats.partition_point(|b| *b <= t).saturating_sub(1);
        for (slot, value) in per_beat[i].iter_mut().zip(frame) {
            *slot += value;
        }
        counts[i] += 1;
    }
    for (vector, &count) in per_beat.iter_mut().zip(&counts) {
        if count > 0 {
            for v in vector.iter_mut() {
                *v /= count as f32;
            }
        }
    }
    let silent: Vec<bool> = (0..beats.len())
        .map(|i| beat_rms(audio, beats, i) < SILENCE_RMS)
        .collect();
    // Centre and scale every dimension over the song's SOUNDING
    // beats, so a cosine measures how two beats differ from the
    // song's average rather than how much both sound like the song
    // — and a silent tail does not drag the average away from the
    // music.
    let sounding = silent.iter().filter(|q| !**q).count().max(1) as f32;
    for d in 0..dims {
        let mean = per_beat
            .iter()
            .zip(&silent)
            .filter(|(_, q)| !**q)
            .map(|(v, _)| v[d])
            .sum::<f32>()
            / sounding;
        let var = per_beat
            .iter()
            .zip(&silent)
            .filter(|(_, q)| !**q)
            .map(|(v, _)| (v[d] - mean).powi(2))
            .sum::<f32>()
            / sounding;
        let scale = 1.0 / var.sqrt().max(1e-6);
        for v in &mut per_beat {
            v[d] = (v[d] - mean) * scale;
        }
    }
    for (v, &quiet) in per_beat.iter_mut().zip(&silent) {
        if quiet {
            v.iter_mut().for_each(|x| *x = 0.0);
        } else {
            normalise(v);
        }
    }
    per_beat
}

/// RMS of the samples in beat interval `i` (to the song's end for
/// the last beat); 0 when the interval holds no sample.
fn beat_rms(audio: &AudioData, beats: &[f64], i: usize) -> f32 {
    let rate = f64::from(audio.sample_rate().max(1));
    let samples = audio.samples();
    let from = ((beats[i].max(0.0) * rate) as usize).min(samples.len());
    let to = beats.get(i + 1).map_or(samples.len(), |b| {
        ((b.max(0.0) * rate) as usize).min(samples.len())
    });
    if to <= from {
        return 0.0;
    }
    let sum: f32 = samples[from..to].iter().map(|x| x * x).sum();
    (sum / (to - from) as f32).sqrt()
}

/// Per STFT frame: chroma (12) then log band energies (20).
fn frame_features(audio: &AudioData, config: &StructureConfig) -> Vec<Vec<f32>> {
    let samples = audio.samples();
    let window = config.window.max(16);
    let hop = config.hop.max(1);
    let rate = f64::from(audio.sample_rate().max(1));
    if samples.len() < window {
        return Vec::new();
    }
    let count = (samples.len() - window) / hop + 1;
    let bin_hz = rate / window as f64;
    // Bin → pitch class and bin → band, once.
    let bins = window / 2 + 1;
    let pitch_class: Vec<Option<usize>> = (0..bins)
        .map(|k| {
            let hz = k as f64 * bin_hz;
            (CHROMA_LOW_HZ..=CHROMA_HIGH_HZ).contains(&hz).then(|| {
                let semis = 12.0 * (hz / 440.0).log2();
                (semis.round() as i64).rem_euclid(12) as usize
            })
        })
        .collect();
    let band: Vec<Option<usize>> = (0..bins)
        .map(|k| {
            let hz = k as f64 * bin_hz;
            (BAND_LOW_HZ..=BAND_HIGH_HZ).contains(&hz).then(|| {
                let x = (hz / BAND_LOW_HZ).ln() / (BAND_HIGH_HZ / BAND_LOW_HZ).ln();
                ((x * BANDS as f64) as usize).min(BANDS - 1)
            })
        })
        .collect();
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(window);
    let hann: Vec<f32> = (0..window)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / window as f32).cos())
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
        let mut features = vec![0.0f32; CHROMA + BANDS];
        for (k, c) in output.iter().enumerate() {
            let magnitude = c.norm();
            if let Some(pc) = pitch_class[k] {
                features[pc] += magnitude;
            }
            if let Some(b) = band[k] {
                features[CHROMA + b] += magnitude * magnitude;
            }
        }
        // Chroma as a shape (unit length), envelope as log energy.
        normalise(&mut features[..CHROMA]);
        for e in &mut features[CHROMA..] {
            *e = (*e + 1e-9).ln();
        }
        frames.push(features);
    }
    frames
}

/// Bar starts as indices into `beats` (each downbeat's nearest beat),
/// ascending and unique; empty when no downbeats are known.
#[must_use]
pub fn bar_indices(beats: &[f64], downbeats: &[f64]) -> Vec<usize> {
    if beats.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<usize> = downbeats
        .iter()
        .map(|d| {
            let i = beats.partition_point(|b| b < d);
            match (i.checked_sub(1), beats.get(i)) {
                (Some(before), Some(after)) => {
                    if d - beats[before] <= after - d {
                        before
                    } else {
                        i
                    }
                }
                (Some(before), None) => before,
                _ => 0,
            }
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalise(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in v {
            *x /= norm;
        }
    }
}

/// Centred moving average of width `width` (odd widths centre
/// exactly; even ones lean one sample early), edges shrinking.
#[must_use]
pub fn smooth(values: &[f32], width: usize) -> Vec<f32> {
    let width = width.max(1);
    let half = width / 2;
    (0..values.len())
        .map(|i| {
            let from = i.saturating_sub(half);
            let to = (i + width - half).min(values.len());
            values[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect()
}

/// Maximal runs `(start, len)` where `values[i] >= floor`.
#[must_use]
pub fn runs_at_or_above(values: &[f32], floor: f32) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &v) in values.iter().enumerate() {
        match (v >= floor, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, i - s));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, values.len() - s));
    }
    runs
}

/// Widen each run to the beats whose RAW value clears the floor: the
/// moving average pulls a run's ends inward by half its width, and
/// a chorus is exactly as long as it is.
#[must_use]
pub fn grow(runs: Vec<(usize, usize)>, raw: &[f32], floor: f32) -> Vec<(usize, usize)> {
    runs.into_iter()
        .map(|(mut start, len)| {
            let mut end = start + len;
            while start > 0 && raw[start - 1] >= floor {
                start -= 1;
            }
            while end < raw.len() && raw[end] >= floor {
                end += 1;
            }
            (start, end - start)
        })
        .collect()
}

/// Move a span onto bar lines: the start forward to the next bar
/// start (within `snap` beats), the end back to the last bar start
/// inside the span. Without bars, whole groups of four beats from
/// beat 0. `None` when nothing is left.
#[must_use]
pub fn snap_to_bars(
    start: usize,
    len: usize,
    bars: &[usize],
    snap: usize,
) -> Option<(usize, usize)> {
    let end = start + len;
    if bars.is_empty() {
        let s = start.div_ceil(4) * 4;
        let e = end / 4 * 4;
        return (e > s && s - start <= snap).then_some((s, e - s));
    }
    let s = *bars.iter().find(|&&b| b >= start)?;
    if s - start > snap {
        return None;
    }
    let e = *bars.iter().rev().find(|&&b| b <= end && b > s)?;
    Some((s, e - s))
}

/// The best non-overlapping repeats: longest × most similar first,
/// a candidate dropped when either of its spans touches a beat an
/// accepted repeat already covers.
fn pick(mut candidates: Vec<Repeat>, n: usize) -> Vec<Repeat> {
    candidates.sort_by(|a, b| {
        let score = |r: &Repeat| r.similarity * r.beats as f32;
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.first_beat.cmp(&b.first_beat))
            .then(a.second_beat.cmp(&b.second_beat))
    });
    let mut used = vec![false; n];
    let mut out = Vec::new();
    for r in candidates {
        let spans = [
            r.first_beat..r.first_beat + r.beats,
            r.second_beat..r.second_beat + r.beats,
        ];
        if spans
            .iter()
            .any(|s| s.end > n || used[s.clone()].iter().any(|u| *u))
        {
            continue;
        }
        for s in spans {
            for u in &mut used[s] {
                *u = true;
            }
        }
        out.push(r);
    }
    out.sort_by_key(|r| (r.first_beat, r.second_beat));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::synth::add_burst;

    const RATE: u32 = 22_050;
    const BEAT_S: f64 = 0.5;

    /// A section of `bars` bars starting at `start_s`: one riff of
    /// `tones` (hertz, one per eighth, cycling) over a click on every
    /// beat.
    fn section(mix: &mut [f32], start_s: f64, bars: usize, tones: &[f64], gain: f32) {
        for bar in 0..bars {
            for beat in 0..4 {
                let t = start_s + (bar * 4 + beat) as f64 * BEAT_S;
                add_burst(mix, RATE, t, 1500.0, 0.02, 0.6);
                for eighth in 0..2 {
                    let slot = (bar * 8 + beat * 2 + eighth) % tones.len();
                    add_burst(
                        mix,
                        RATE,
                        t + eighth as f64 * BEAT_S / 2.0,
                        tones[slot],
                        0.22,
                        gain,
                    );
                }
            }
        }
    }

    /// A B A C: the chorus (A) twice with a verse between and a coda.
    fn song() -> (AudioData, Vec<f64>) {
        let bars = [8usize, 8, 8, 4];
        let total: usize = bars.iter().sum();
        let mut mix =
            vec![0.0f32; ((total as f64 * 4.0 * BEAT_S + 1.0) * f64::from(RATE)) as usize];
        let a = [220.0, 277.2, 329.6, 440.0, 329.6, 277.2];
        let b = [523.3, 659.3, 784.0, 659.3];
        let c = [110.0, 130.8, 146.8, 164.8, 196.0];
        let mut t = 0.0;
        for (i, &n) in bars.iter().enumerate() {
            let (tones, gain): (&[f64], f32) = match i {
                0 | 2 => (&a, 0.5),
                1 => (&b, 0.35),
                _ => (&c, 0.45),
            };
            section(&mut mix, t, n, tones, gain);
            t += n as f64 * 4.0 * BEAT_S;
        }
        let beats: Vec<f64> = (0..total * 4).map(|i| i as f64 * BEAT_S).collect();
        (AudioData::from_mono(mix, RATE), beats)
    }

    #[test]
    fn the_repeated_chorus_is_found_where_it_is_and_nothing_else_is() {
        let (audio, beats) = song();
        let downbeats: Vec<f64> = beats.iter().copied().step_by(4).collect();
        let config = StructureConfig::default();
        let repeats = find_repeats(&audio, &beats, &downbeats, &config);
        assert_eq!(repeats.len(), 1, "{repeats:?}");
        let r = &repeats[0];
        assert_eq!((r.first_beat, r.second_beat), (0, 64), "{r:?}");
        assert_eq!(r.beats, 32, "{r:?}");
        assert!(r.similarity > 0.8, "{r:?}");
        // Deterministic.
        assert_eq!(find_repeats(&audio, &beats, &downbeats, &config), repeats);
        // Without bars, the same spans (they start on multiples of 4).
        assert_eq!(find_repeats(&audio, &beats, &[], &config), repeats);
    }

    #[test]
    fn silence_does_not_repeat_itself() {
        // The song plus a minute of digital silence with beats laid
        // across it, the way a rip with a silent tail arrives.
        let (audio, beats) = song();
        let mut samples = audio.samples().to_vec();
        samples.extend(vec![0.0f32; 60 * RATE as usize]);
        let long = AudioData::from_mono(samples, RATE);
        let total_beats = (long.duration_s() / BEAT_S) as usize;
        let long_beats: Vec<f64> = (0..total_beats).map(|i| i as f64 * BEAT_S).collect();
        let config = StructureConfig::default();
        let repeats = find_repeats(&long, &long_beats, &[], &config);
        assert_eq!(repeats.len(), 1, "{repeats:?}");
        assert_eq!((repeats[0].first_beat, repeats[0].second_beat), (0, 64));
        let _ = beats;
    }

    #[test]
    fn a_song_without_a_repeat_has_none_and_so_does_one_without_beats() {
        let (audio, beats) = song();
        // Cut the second chorus away: A B C only.
        let end = ((16 * 4) as f64 * BEAT_S * f64::from(RATE)) as usize;
        let short = AudioData::from_mono(audio.samples()[..end].to_vec(), RATE);
        let short_beats: Vec<f64> = beats
            .iter()
            .copied()
            .filter(|b| *b < 16.0 * 4.0 * BEAT_S)
            .collect();
        let config = StructureConfig::default();
        assert_eq!(
            find_repeats(&short, &short_beats, &[], &config),
            Vec::<Repeat>::new()
        );
        assert_eq!(
            find_repeats(&audio, &[], &[], &config),
            Vec::<Repeat>::new()
        );
        assert!(beat_features(&audio, &[], &config).is_empty());
    }

    #[test]
    fn the_pure_pieces_do_what_they_say() {
        assert_eq!(
            smooth(&[0.0, 1.0, 0.0, 1.0], 3),
            vec![0.5, 1.0 / 3.0, 2.0 / 3.0, 0.5]
        );
        assert_eq!(
            runs_at_or_above(&[0.0, 1.0, 1.0, 0.0, 1.0], 0.5),
            vec![(1, 2), (4, 1)]
        );
        assert_eq!(runs_at_or_above(&[], 0.5), Vec::<(usize, usize)>::new());
        // Bars: start forward to a bar line, end back to one.
        assert_eq!(
            snap_to_bars(2, 30, &[0, 4, 8, 12, 16, 20, 24, 28, 32], 3),
            Some((4, 28))
        );
        assert_eq!(
            grow(vec![(2, 3)], &[0.7, 0.2, 0.9, 0.9, 0.9, 0.8, 0.1], 0.5),
            vec![(2, 4)]
        );
        assert_eq!(grow(vec![(1, 2)], &[0.9, 0.9, 0.9, 0.9], 0.5), vec![(0, 4)]);
        assert_eq!(
            snap_to_bars(2, 30, &[0, 4, 8], 1),
            None,
            "too far from a bar"
        );
        assert_eq!(snap_to_bars(5, 3, &[0, 4, 8], 3), None, "no bar inside");
        assert_eq!(snap_to_bars(1, 10, &[], 3), Some((4, 4)));
        assert_eq!(
            bar_indices(&[0.0, 0.5, 1.0, 1.5, 2.0], &[0.01, 2.04, 2.04]),
            vec![0, 4]
        );
        assert_eq!(bar_indices(&[], &[1.0]), Vec::<usize>::new());
        // Picking: the better-scoring pair wins, overlaps are dropped.
        let picked = pick(
            vec![
                Repeat {
                    first_beat: 0,
                    second_beat: 64,
                    beats: 32,
                    similarity: 0.9,
                },
                Repeat {
                    first_beat: 8,
                    second_beat: 72,
                    beats: 32,
                    similarity: 0.95,
                },
                // Overlaps the winner's second span (72..104): dropped.
                Repeat {
                    first_beat: 100,
                    second_beat: 140,
                    beats: 32,
                    similarity: 0.7,
                },
                Repeat {
                    first_beat: 110,
                    second_beat: 145,
                    beats: 32,
                    similarity: 0.7,
                },
            ],
            180,
        );
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].first_beat, 8);
        assert_eq!(picked[1].first_beat, 110);
    }
}
