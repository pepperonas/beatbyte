//! Song-level features, measured in one pass over the audio.
//!
//! **Only what can actually be measured.** Every number here has an
//! algorithm, a range and a meaning, written down in
//! `docs/audio/features.md`. The commission also asked for
//! danceability and valence; there is no model here that determines
//! either, so they are absent rather than guessed — a number that
//! looks like an answer is worse than a missing one.
//!
//! One pass, because decoding the audio is the expensive part and
//! everything here falls out of the same short-time spectrum: band
//! shares, brightness, the onset rate, how periodic the song is, and
//! the chroma that the key estimate reads.

use realfft::RealFftPlanner;

/// The version of this measurement, recorded with every result.
///
/// The point of writing it down: when a better estimator ships, the
/// documents say which songs were measured by the old one, and
/// nothing has to be re-measured to find out.
pub const VERSION: u32 = 1;

/// Window length, samples. ~93 ms at 44.1 kHz — long enough to
/// resolve a bass note's pitch class, short enough that a chord
/// change is not smeared across the whole chroma.
const WINDOW: usize = 4096;
/// Hop, samples. Half the window: the usual overlap.
const HOP: usize = 2048;
/// Where bass stops, hertz. Below this is the kick, the bass guitar
/// and the low body of a mix.
pub const BASS_TO_HZ: f32 = 250.0;
/// Where the middle stops, hertz. Above it is air, cymbals and
/// consonants.
pub const MID_TO_HZ: f32 = 4_000.0;
/// The lowest pitch the chroma reads, as MIDI: G3, 196 Hz.
///
/// Not lower. A semitone at 196 Hz is 11.4 Hz and an FFT bin here is
/// 10.8 Hz, so this is about where a semitone stops being narrower
/// than the grid that has to resolve it. Below it, two pitch classes
/// share a bin and the answer is arithmetic rather than music.
const CHROMA_FROM_MIDI: i32 = 55;
/// The highest: C8, 4186 Hz. Above that a pitch class is mostly
/// cymbals.
const CHROMA_TO_MIDI: i32 = 96;

/// What one pass over a song measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongFeatures {
    /// Mean loudness of a frame against the loudest frame, `0.0`–
    /// `1.0`. A song that is loud most of the time sits high; one
    /// with a quiet verse and a loud chorus sits lower.
    pub energy: f32,
    /// Share of the spectrum's weight below [`BASS_TO_HZ`].
    pub bass: f32,
    /// …between the two splits.
    pub mid: f32,
    /// …above [`MID_TO_HZ`]. The three sum to 1.
    pub high: f32,
    /// Mean spectral centroid, hertz — where the weight sits, which
    /// is what "bright" means when said about a mix.
    pub centroid_hz: f32,
    /// Onsets per second, from peaks in the spectral flux.
    pub onset_density: f32,
    /// How strongly a single period stands out in the flux, `0.0`–
    /// `1.0`: the best autocorrelation peak in the range a song's
    /// beat can occupy. A metronome approaches 1, a free-time piece
    /// sits near 0.
    pub beat_strength: f32,
    /// Weight per pitch class, C first, normalised to sum to 1.
    pub chroma: [f32; 12],
}

/// Measure a decoded song. `None` when it is too short to window.
#[must_use]
pub fn measure(samples: &[f32], rate: u32) -> Option<SongFeatures> {
    if rate == 0 || samples.len() < WINDOW + HOP {
        return None;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| {
            let x = i as f32 / WINDOW as f32;
            0.5 - 0.5 * (2.0 * core::f32::consts::PI * x).cos()
        })
        .collect();
    let bins = spectrum.len();
    let bin_hz = rate as f32 / WINDOW as f32;

    // ⚠️ One window per PITCH, and the MEAN magnitude inside it —
    // never a sum over every bin binned by rounding. FFT bins are
    // linearly spaced and pitch classes are not, so a sum gives each
    // class a share of the spectrum that depends on the grid rather
    // than on the music: white noise came out with a 1.9× spread
    // across the twelve, and 135 of this library's 171 songs landed
    // in four keys. A mean over each semitone window is flat for
    // noise, which is the only honest answer for noise.
    let pitch_windows: Vec<(usize, usize, usize)> = (CHROMA_FROM_MIDI..=CHROMA_TO_MIDI)
        .filter_map(|midi| {
            let centre = 440.0 * 2f32.powf((midi - 69) as f32 / 12.0);
            let low = centre * 2f32.powf(-0.5 / 12.0);
            let high = centre * 2f32.powf(0.5 / 12.0);
            let from = (low / bin_hz).ceil().max(1.0) as usize;
            let to = ((high / bin_hz).floor() as usize).min(bins - 1);
            (from <= to).then_some((midi.rem_euclid(12) as usize, from, to))
        })
        .collect();
    let mut magnitudes = vec![0.0f32; bins];
    let mut band = [0.0f64; 3];
    let mut centroid_weight = 0.0f64;
    let mut centroid_sum = 0.0f64;
    let mut chroma = [0.0f64; 12];
    let mut frame_rms: Vec<f32> = Vec::new();
    let mut flux: Vec<f32> = Vec::new();
    let mut previous = vec![0.0f32; bins];

    let frames = (samples.len() - WINDOW) / HOP + 1;
    for frame in 0..frames {
        let start = frame * HOP;
        let window = &samples[start..start + WINDOW];
        let mean_square: f64 = window.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        frame_rms.push((mean_square / WINDOW as f64).sqrt() as f32);
        for (slot, (sample, taper)) in input.iter_mut().zip(window.iter().zip(&hann)) {
            *slot = sample * taper;
        }
        if fft.process(&mut input, &mut spectrum).is_err() {
            return None;
        }
        let mut rise = 0.0f32;
        for (index, value) in spectrum.iter().enumerate().skip(1) {
            let magnitude = value.norm();
            let hz = index as f32 * bin_hz;
            // Spectral flux: only what GREW. A note starting is a
            // rise; a note ending is not an onset.
            rise += (magnitude - previous[index]).max(0.0);
            previous[index] = magnitude;
            let weight = f64::from(magnitude);
            if hz < BASS_TO_HZ {
                band[0] += weight;
            } else if hz < MID_TO_HZ {
                band[1] += weight;
            } else {
                band[2] += weight;
            }
            centroid_sum += weight * f64::from(hz);
            centroid_weight += weight;
            magnitudes[index] = magnitude;
        }
        for (class, from, to) in &pitch_windows {
            let span = to - from + 1;
            let sum: f64 = magnitudes[*from..=*to].iter().map(|m| f64::from(*m)).sum();
            chroma[*class] += sum / span as f64;
        }
        flux.push(rise);
    }

    let total: f64 = band.iter().sum();
    let share = |value: f64| {
        if total > 0.0 {
            (value / total) as f32
        } else {
            0.0
        }
    };
    let hop_s = HOP as f32 / rate as f32;
    let chroma_total: f64 = chroma.iter().sum();
    let mut chroma_out = [0.0f32; 12];
    if chroma_total > 0.0 {
        for (slot, value) in chroma_out.iter_mut().zip(chroma) {
            *slot = (value / chroma_total) as f32;
        }
    }
    Some(SongFeatures {
        energy: mean_against_peak(&frame_rms),
        bass: share(band[0]),
        mid: share(band[1]),
        high: share(band[2]),
        centroid_hz: if centroid_weight > 0.0 {
            (centroid_sum / centroid_weight) as f32
        } else {
            0.0
        },
        onset_density: onset_rate(&flux, hop_s),
        beat_strength: periodicity(&flux, hop_s),
        chroma: chroma_out,
    })
}

/// The mean frame against the loudest frame.
///
/// Peak-relative on purpose: absolute RMS says how loud the master
/// is, which the loudness sidecar already measures properly. What
/// this says is how much of the song is spent near its own ceiling.
fn mean_against_peak(frames: &[f32]) -> f32 {
    let peak = frames.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 || frames.is_empty() {
        return 0.0;
    }
    let mean = frames.iter().map(|v| f64::from(*v)).sum::<f64>() / frames.len() as f64;
    ((mean / f64::from(peak)) as f32).clamp(0.0, 1.0)
}

/// Peaks per second in the flux.
///
/// A peak is a local maximum that stands above the song's own median
/// by a margin scaled to its own spread — an absolute threshold
/// works on one mix and on no other.
///
/// ⚠️ And a **floor**, which the first version did not have. A
/// purely relative threshold has nothing to hold on to when there
/// are no onsets at all: on a held tone the spread is numerical
/// ripple, and the rate came back higher than a steady pulse's. The
/// floor is a share of the loudest rise in the song, so a song with
/// no rises has no onsets rather than a great many tiny ones.
fn onset_rate(flux: &[f32], hop_s: f32) -> f32 {
    if flux.len() < 3 || hop_s <= 0.0 {
        return 0.0;
    }
    let mut sorted: Vec<f32> = flux.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    let high = sorted[sorted.len() * 9 / 10];
    let loudest = sorted[sorted.len() - 1];
    let threshold = (median + (high - median) * 0.5).max(loudest * 0.05);
    let peaks = flux
        .windows(3)
        .filter(|w| w[1] > w[0] && w[1] >= w[2] && w[1] > threshold)
        .count();
    peaks as f32 / (flux.len() as f32 * hop_s)
}

/// How strongly one period stands out in the flux.
///
/// Autocorrelation over the lags a beat can occupy — 60 to 200 BPM,
/// the same range the tempo stage allows — normalised by the lag-0
/// energy, so the answer is "how much of this song's movement
/// repeats at one period" rather than "how loud it is".
fn periodicity(flux: &[f32], hop_s: f32) -> f32 {
    if flux.len() < 8 || hop_s <= 0.0 {
        return 0.0;
    }
    let mean = flux.iter().map(|v| f64::from(*v)).sum::<f64>() / flux.len() as f64;
    let centred: Vec<f64> = flux.iter().map(|v| f64::from(*v) - mean).collect();
    let zero: f64 = centred.iter().map(|v| v * v).sum();
    if zero <= 0.0 {
        return 0.0;
    }
    let lag_for = |bpm: f64| ((60.0 / bpm) / f64::from(hop_s)).round() as usize;
    let (fastest, slowest) = (lag_for(200.0).max(1), lag_for(60.0));
    let mut best = 0.0f64;
    for lag in fastest..=slowest.min(centred.len() / 2) {
        let sum: f64 = centred
            .iter()
            .zip(centred.iter().skip(lag))
            .map(|(a, b)| a * b)
            .sum();
        best = best.max(sum / zero);
    }
    (best as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, seconds: f32, rate: u32) -> Vec<f32> {
        let count = (seconds * rate as f32) as usize;
        (0..count)
            .map(|i| {
                let t = i as f32 / rate as f32;
                (2.0 * core::f32::consts::PI * hz * t).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn a_song_too_short_to_window_measures_nothing() {
        // Absent, not zero: "no measurement" and "measured zero" are
        // different facts and only one of them is true here.
        assert!(measure(&tone(440.0, 0.05, 44_100), 44_100).is_none());
        assert!(measure(&tone(440.0, 5.0, 0), 0).is_none());
    }

    #[test]
    fn a_bass_tone_and_a_bright_tone_land_in_their_own_bands() {
        let low = measure(&tone(80.0, 3.0, 44_100), 44_100).expect("measured");
        assert!(low.bass > 0.8, "80 Hz is bass: {:?}", low.bass);
        assert!(low.centroid_hz < 400.0, "{}", low.centroid_hz);

        let high = measure(&tone(8_000.0, 3.0, 44_100), 44_100).expect("measured");
        assert!(high.high > 0.8, "8 kHz is treble: {:?}", high.high);
        assert!(high.centroid_hz > 6_000.0, "{}", high.centroid_hz);

        // The three shares are shares: they account for everything.
        for song in [low, high] {
            let sum = song.bass + song.mid + song.high;
            assert!((sum - 1.0).abs() < 1e-3, "{sum}");
        }
    }

    #[test]
    fn a_tone_at_a_is_a_in_the_chroma() {
        // A440 is pitch class 9. If this is off by one the key
        // estimate is off by a semitone for every song.
        let song = measure(&tone(440.0, 3.0, 44_100), 44_100).expect("measured");
        let loudest = song
            .chroma
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(class, _)| class);
        assert_eq!(loudest, Some(9));
        let sum: f32 = song.chroma.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3, "{sum}");
    }

    #[test]
    fn a_steady_pulse_is_more_periodic_than_a_held_note() {
        let rate = 44_100;
        // A click every half second: 120 BPM.
        let mut pulse = vec![0.0f32; rate as usize * 4];
        for beat in 0..8 {
            let at = beat * rate as usize / 2;
            for i in 0..200 {
                if at + i < pulse.len() {
                    pulse[at + i] = 0.8 * (1.0 - i as f32 / 200.0);
                }
            }
        }
        let beaty = measure(&pulse, rate).expect("measured");
        let held = measure(&tone(440.0, 4.0, rate), rate).expect("measured");
        assert!(
            beaty.beat_strength > held.beat_strength,
            "pulse {} vs held {}",
            beaty.beat_strength,
            held.beat_strength
        );
        assert!(beaty.onset_density > held.onset_density);
    }

    #[test]
    fn a_song_that_is_loud_throughout_has_more_energy_than_one_that_is_not() {
        let rate = 44_100;
        let steady = tone(440.0, 4.0, rate);
        let mut patchy = steady.clone();
        // Silence the second half.
        for sample in patchy.iter_mut().skip(steady.len() / 2) {
            *sample = 0.0;
        }
        let a = measure(&steady, rate).expect("measured").energy;
        let b = measure(&patchy, rate).expect("measured").energy;
        assert!(a > b, "steady {a} should beat patchy {b}");
        assert!((0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b));
    }
}

/// What key a chroma profile looks like, and how sure that is.
///
/// Krumhansl–Schmuckler: correlate the song's pitch-class weights
/// against the two profiles listeners produced in the 1982 probe-tone
/// experiments, once for each of the twelve tonics, and take the best
/// of the twenty-four.
///
/// ⚠️ It is an estimate and it says so. A song that modulates, one
/// built on a mode that is neither major nor minor, and one whose
/// bass carries the weight all report *something* — which is why the
/// answer comes with a margin, and why the caller is expected to
/// refuse a thin one rather than write it down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEstimate {
    /// Pitch class of the tonic, `0` = C, rising in semitones.
    pub tonic: u8,
    /// Whether the major or the minor profile fitted better.
    pub major: bool,
    /// How far the winner's correlation stood above the runner-up's,
    /// `0.0`–`1.0`. Near zero means the profiles fitted about
    /// equally well — including the case where they all fitted
    /// badly, which is the honest answer for a great many songs.
    pub margin: f32,
}

/// Krumhansl–Schmuckler major profile (1982 probe-tone ratings).
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// …and the minor one.
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Estimate the key from a chroma profile.
///
/// `None` when the chroma says nothing — silence, or a signal with
/// no pitched content at all.
#[must_use]
pub fn estimate_key(chroma: &[f32; 12]) -> Option<KeyEstimate> {
    if chroma.iter().sum::<f32>() <= 0.0 {
        return None;
    }
    let observed: Vec<f64> = chroma.iter().map(|v| f64::from(*v)).collect();
    let mut best: Option<(f64, u8, bool)> = None;
    let mut second = f64::NEG_INFINITY;
    for tonic in 0..12u8 {
        for major in [true, false] {
            let profile = if major { &MAJOR } else { &MINOR };
            let rotated: Vec<f64> = (0..12)
                .map(|i| profile[(i + 12 - tonic as usize) % 12])
                .collect();
            let r = correlation(&observed, &rotated);
            match best {
                Some((top, _, _)) if r <= top => second = second.max(r),
                Some((top, _, _)) => {
                    second = second.max(top);
                    best = Some((r, tonic, major));
                }
                None => best = Some((r, tonic, major)),
            }
        }
    }
    let (top, tonic, major) = best?;
    if !top.is_finite() {
        return None;
    }
    // ⚠️ The straight DIFFERENCE of two correlations, not their
    // ratio. A ratio is scale-free in exactly the wrong way: on a
    // flat chroma the two best fits are 0.020 and 0.017, both
    // useless, and `(top - second) / top` calls that a margin of
    // 0.15. White noise was read as A minor that way. Both terms are
    // correlations in [-1, 1], so their difference is already
    // comparable across songs — and it is small when nothing fits,
    // which is the answer that was missing.
    let margin = if second.is_finite() {
        (top - second).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(KeyEstimate {
        tonic,
        major,
        margin: margin as f32,
    })
}

/// Pearson correlation of two equal-length series.
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let mut top = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - mean_a, y - mean_b);
        top += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    if var_a <= 0.0 || var_b <= 0.0 {
        return f64::NEG_INFINITY;
    }
    top / (var_a * var_b).sqrt()
}

#[cfg(test)]
mod key_tests {
    use super::*;

    /// A chroma shaped like the profile of one key.
    fn profile_of(tonic: usize, major: bool) -> [f32; 12] {
        let source = if major { &MAJOR } else { &MINOR };
        let mut chroma = [0.0f32; 12];
        for (i, slot) in chroma.iter_mut().enumerate() {
            *slot = source[(i + 12 - tonic) % 12] as f32;
        }
        chroma
    }

    #[test]
    fn a_chroma_shaped_like_a_key_is_read_as_that_key() {
        for tonic in 0..12usize {
            for major in [true, false] {
                let found = estimate_key(&profile_of(tonic, major)).expect("an estimate");
                assert_eq!(
                    (found.tonic as usize, found.major),
                    (tonic, major),
                    "tonic {tonic} major {major}"
                );
                assert!(found.margin > 0.0);
            }
        }
    }

    #[test]
    fn silence_has_no_key() {
        // Not C major with a small margin: no key at all. A number
        // that looks like an answer is worse than a missing one.
        assert_eq!(estimate_key(&[0.0; 12]), None);
    }

    #[test]
    fn a_flat_chroma_gives_no_usable_answer() {
        // Every pitch class equally loud fits every profile equally
        // badly. Whatever comes out must not pretend to be sure.
        let found = estimate_key(&[1.0 / 12.0; 12]);
        assert!(
            found.is_none_or(|key| key.margin < 0.05),
            "{found:?} claimed to know"
        );
    }

    #[test]
    fn a_major_key_and_its_relative_minor_are_hard_to_tell_apart() {
        // C major and A minor share every note. The estimate is
        // allowed to pick either — what it may NOT do is claim a
        // wide margin, because the difference really is thin.
        let mut chroma = profile_of(0, true);
        for (slot, minor) in chroma.iter_mut().zip(profile_of(9, false)) {
            *slot = (*slot + minor) / 2.0;
        }
        let found = estimate_key(&chroma).expect("an estimate");
        assert!(
            found.margin < 0.25,
            "a thin difference must read as thin: {found:?}"
        );
    }
}

#[cfg(test)]
mod bias_tests {
    use super::*;

    /// Deterministic white noise — no pitch, no key.
    fn noise(seconds: f32, rate: u32) -> Vec<f32> {
        let count = (seconds * rate as f32) as usize;
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        (0..count)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / 8_388_608.0) - 1.0
            })
            .collect()
    }

    #[test]
    fn noise_has_no_key() {
        // ⚠️ THE counter-check for this whole feature. White noise
        // has no pitch class at all, so a chroma taken from it must
        // be flat and the estimate must not be sure of anything. The
        // first version summed every FFT bin, and because bins are
        // linearly spaced while pitch classes are not, broadband
        // content fell into a FIXED pattern — a key read off the
        // geometry of the transform rather than off the music.
        let song = measure(&noise(4.0, 44_100), 44_100).expect("measured");
        let flat = 1.0 / 12.0;
        let worst = song
            .chroma
            .iter()
            .map(|v| (v - flat).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < flat * 0.35,
            "noise produced a shaped chroma: {:?}",
            song.chroma
        );
        let found = estimate_key(&song.chroma);
        assert!(
            found.is_none_or(|key| key.margin < 0.08),
            "noise was read as a key: {found:?}"
        );
    }
}
