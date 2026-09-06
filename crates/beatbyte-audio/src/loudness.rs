//! Loudness the way broadcast and streaming measure it — ITU-R
//! BS.1770-4 / EBU R128 — and the gain that brings a song to the
//! game's level without clipping it.
//!
//! - [`measure`] — integrated loudness (K-weighted, 400 ms blocks,
//!   absolute and relative gate), loudness range (EBU Tech 3342) and
//!   true peak (4× oversampled), on the channels the player plays
//! - [`play_gain_db`] — target minus loudness, held under a true-peak
//!   ceiling: a quiet song rises as far as its peaks allow and no
//!   further (no limiter, by the user's call)
//! - [`Report`] — the sidecar written beside the audio at import and
//!   by `beatbyte-cli loudness`, read by the game when a song starts
//!
//! Pure and deterministic; the filters are derived for the file's own
//! sample rate (the standard tabulates 48 kHz only) the way
//! libebur128 does, and the meter is pinned on the standard's
//! calibration signals.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::decode::Channels;
use crate::quality::Quality;

/// The level every song is brought to (integrated, LUFS).
pub const TARGET_LUFS: f64 = -16.0;
/// True peak a normalised song may not exceed (dBTP).
pub const CEILING_DBTP: f64 = -1.0;
/// The most gain in either direction a song may get (dB); a song
/// that needs more is a measurement error, not a quiet song.
pub const GAIN_LIMIT_DB: f64 = 24.0;
/// Below this integrated loudness the meter reports nothing (a
/// silent file, or one the gate left empty).
pub const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// The relative gate sits this far under the ungated mean.
const RELATIVE_GATE_LU: f64 = 10.0;
/// Momentary block length and hop (BS.1770: 400 ms, 75 % overlap).
const BLOCK_S: f64 = 0.4;
const HOP_S: f64 = 0.1;
/// Short-term block for the loudness range (Tech 3342: 3 s).
const SHORT_S: f64 = 3.0;
/// Loudness range's relative gate (Tech 3342: −20 LU).
const LRA_GATE_LU: f64 = 20.0;
/// The constant that makes a 997 Hz sine read its RMS.
const OFFSET_LU: f64 = -0.691;
/// True-peak oversampling and the interpolator's half width.
const OVERSAMPLE: usize = 4;
const PEAK_TAPS: i64 = 8;

/// What the meter read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// Integrated loudness, LUFS; `None` when the gate left nothing
    /// (silence).
    pub integrated_lufs: Option<f64>,
    /// Loudness range, LU (P95 − P10 of the gated short-term
    /// loudness); `None` under two short-term blocks.
    pub loudness_range_lu: Option<f64>,
    /// True peak, dBTP (4× oversampled).
    pub true_peak_dbtp: f64,
    /// Sample peak, dBFS.
    pub sample_peak_dbfs: f64,
    /// Seconds measured.
    pub duration_s: f64,
}

/// Measure a decoded song.
#[must_use]
pub fn measure(audio: &Channels) -> Measurement {
    let planes: Vec<Vec<f32>> = (0..audio.channels.max(1)).map(|c| audio.plane(c)).collect();
    measure_planes(&planes, audio.sample_rate)
}

/// [`measure`] on planar channels (one `Vec` per channel).
#[must_use]
pub fn measure_planes(planes: &[Vec<f32>], sample_rate: u32) -> Measurement {
    let rate = f64::from(sample_rate.max(1));
    let frames = planes.iter().map(Vec::len).min().unwrap_or(0);
    let duration_s = frames as f64 / rate;
    // Sample and true peak over every channel.
    let mut sample_peak = 0.0f32;
    let mut true_peak = 0.0f32;
    for plane in planes {
        sample_peak = plane.iter().fold(sample_peak, |acc, x| acc.max(x.abs()));
        true_peak = true_peak.max(true_peak_of(plane));
    }
    // K-weighted power per block per channel.
    let weighted: Vec<Vec<f32>> = planes.iter().map(|p| k_weight(p, sample_rate)).collect();
    let block = (BLOCK_S * rate).round() as usize;
    let hop = (HOP_S * rate).round().max(1.0) as usize;
    let block_power = block_powers(&weighted, block.max(1), hop);
    let integrated_lufs = gated_loudness(&block_power);
    let short = (SHORT_S * rate).round() as usize;
    let short_power = block_powers(&weighted, short.max(1), hop);
    let loudness_range_lu = loudness_range(&short_power);
    Measurement {
        integrated_lufs,
        loudness_range_lu,
        true_peak_dbtp: to_db(f64::from(true_peak)),
        sample_peak_dbfs: to_db(f64::from(sample_peak)),
        duration_s,
    }
}

/// The K-weighting: a high shelf (+4 dB above ~1.5 kHz, the head's
/// acoustic effect) then the RLB high-pass at 38 Hz, as two biquads
/// derived for `sample_rate` by bilinear transform of the standard's
/// prototypes (the standard's own coefficients are for 48 kHz).
#[must_use]
pub fn k_weight(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let fs = f64::from(sample_rate.max(1));
    // Stage 1: high shelf.
    let (f0, g, q) = (
        1_681.974_450_955_533,
        3.999_843_853_973_347,
        0.707_175_236_955_419_6,
    );
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    // Stage 2: RLB high-pass.
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let highpass = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    let mut state1 = BiquadState::default();
    let mut state2 = BiquadState::default();
    samples
        .iter()
        .map(|&x| {
            let y = shelf.step(&mut state1, f64::from(x));
            highpass.step(&mut state2, y) as f32
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

#[derive(Debug, Default, Clone, Copy)]
struct BiquadState {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn step(&self, s: &mut BiquadState, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * s.x1 + self.b2 * s.x2 - self.a1 * s.y1 - self.a2 * s.y2;
        s.x2 = s.x1;
        s.x1 = x;
        s.y2 = s.y1;
        s.y1 = y;
        y
    }
}

/// Mean square per block, summed over channels (each weighted 1.0 —
/// stereo and mono are what this game plays): one value per block
/// of `block` frames every `hop` frames.
fn block_powers(weighted: &[Vec<f32>], block: usize, hop: usize) -> Vec<f64> {
    let frames = weighted.iter().map(Vec::len).min().unwrap_or(0);
    if frames < block {
        return Vec::new();
    }
    // Prefix sums of the squared signal per channel: O(1) per block.
    let prefix: Vec<Vec<f64>> = weighted
        .iter()
        .map(|plane| {
            let mut acc = 0.0f64;
            let mut out = Vec::with_capacity(plane.len() + 1);
            out.push(0.0);
            for &x in plane {
                acc += f64::from(x) * f64::from(x);
                out.push(acc);
            }
            out
        })
        .collect();
    (0..=(frames - block))
        .step_by(hop.max(1))
        .map(|start| {
            prefix
                .iter()
                .map(|p| (p[start + block] - p[start]) / block as f64)
                .sum()
        })
        .collect()
}

/// Loudness of a block power.
fn lufs(power: f64) -> f64 {
    OFFSET_LU + 10.0 * power.max(1e-30).log10()
}

/// Integrated loudness with the absolute (−70 LUFS) and relative
/// (−10 LU) gates, per BS.1770-4.
#[must_use]
pub fn gated_loudness(block_power: &[f64]) -> Option<f64> {
    let above_absolute: Vec<f64> = block_power
        .iter()
        .copied()
        .filter(|&p| lufs(p) > ABSOLUTE_GATE_LUFS)
        .collect();
    if above_absolute.is_empty() {
        return None;
    }
    let mean = above_absolute.iter().sum::<f64>() / above_absolute.len() as f64;
    let relative_gate = lufs(mean) - RELATIVE_GATE_LU;
    let kept: Vec<f64> = above_absolute
        .into_iter()
        .filter(|&p| lufs(p) > relative_gate)
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(lufs(kept.iter().sum::<f64>() / kept.len() as f64))
}

/// Loudness range per EBU Tech 3342: short-term loudness, absolute
/// gate −70, relative gate −20 LU, the spread between the 10th and
/// 95th percentile.
#[must_use]
pub fn loudness_range(short_power: &[f64]) -> Option<f64> {
    let above: Vec<f64> = short_power
        .iter()
        .map(|&p| lufs(p))
        .filter(|&l| l > ABSOLUTE_GATE_LUFS)
        .collect();
    if above.len() < 2 {
        return None;
    }
    let mean_power = above
        .iter()
        .map(|l| 10f64.powf((l - OFFSET_LU) / 10.0))
        .sum::<f64>()
        / above.len() as f64;
    let gate = lufs(mean_power) - LRA_GATE_LU;
    let mut kept: Vec<f64> = above.into_iter().filter(|&l| l > gate).collect();
    if kept.len() < 2 {
        return None;
    }
    kept.sort_by(f64::total_cmp);
    let at = |q: f64| kept[((kept.len() - 1) as f64 * q).round() as usize];
    Some(at(0.95) - at(0.10))
}

/// True peak of one channel: the largest magnitude of the signal
/// reconstructed between samples, by 4× windowed-sinc interpolation
/// (the standard asks for at least 4×).
#[must_use]
pub fn true_peak_of(plane: &[f32]) -> f32 {
    let n = plane.len() as i64;
    let mut peak = plane.iter().fold(0.0f32, |acc, x| acc.max(x.abs()));
    // The interpolation taps for the three in-between phases, once.
    let taps: Vec<Vec<f64>> = (1..OVERSAMPLE)
        .map(|phase| {
            let frac = phase as f64 / OVERSAMPLE as f64;
            (-PEAK_TAPS..=PEAK_TAPS)
                .map(|k| {
                    let t = k as f64 - frac;
                    let sinc = if t.abs() < 1e-12 {
                        1.0
                    } else {
                        (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
                    };
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * t / (PEAK_TAPS as f64 + 1.0)).cos();
                    sinc * w
                })
                .collect()
        })
        .collect();
    for i in 0..n {
        // Cheap skip: an interpolated peak needs neighbours near the
        // current peak; a stretch well under it cannot beat it.
        let local = plane[i as usize]
            .abs()
            .max(plane.get(i as usize + 1).map_or(0.0, |x| x.abs()));
        if local < peak * 0.5 {
            continue;
        }
        for tap in &taps {
            let mut acc = 0.0f64;
            for (j, &h) in tap.iter().enumerate() {
                let idx = i + j as i64 - PEAK_TAPS;
                if idx >= 0 && idx < n {
                    acc += f64::from(plane[idx as usize]) * h;
                }
            }
            peak = peak.max(acc.abs() as f32);
        }
    }
    peak
}

/// Linear magnitude to dB (−∞ clamped to −120).
#[must_use]
pub fn to_db(magnitude: f64) -> f64 {
    if magnitude <= 0.0 {
        -120.0
    } else {
        (20.0 * magnitude.log10()).max(-120.0)
    }
}

/// dB to a linear factor.
#[must_use]
pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The gain that brings `integrated_lufs` to `target` without the
/// true peak passing `ceiling`: `min(target − loudness, ceiling −
/// true_peak)`, clamped to ±[`GAIN_LIMIT_DB`]. A song the meter
/// could not read (silence) gets 0 dB. Pure — tested.
#[must_use]
pub fn play_gain_db(
    integrated_lufs: Option<f64>,
    true_peak_dbtp: f64,
    target: f64,
    ceiling: f64,
) -> f64 {
    let Some(loudness) = integrated_lufs else {
        return 0.0;
    };
    if !loudness.is_finite() || !true_peak_dbtp.is_finite() {
        return 0.0;
    }
    let wanted = target - loudness;
    let allowed = ceiling - true_peak_dbtp;
    wanted.min(allowed).clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB)
}

/// Whether the ceiling, not the target, decided the gain: the song
/// plays quieter than the level because its peaks would clip.
#[must_use]
pub fn peak_limited(
    integrated_lufs: Option<f64>,
    true_peak_dbtp: f64,
    target: f64,
    ceiling: f64,
) -> bool {
    integrated_lufs.is_some_and(|l| (target - l) > (ceiling - true_peak_dbtp) + 1e-9)
}

/// The sidecar schema written beside the audio.
pub const REPORT_SCHEMA: &str = "beatbyte.loudness/1";

/// What is written beside the audio: the measurement, the file's
/// facts and the quality verdict, so the game never measures at
/// play time and a person can read why a song plays as it does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// [`REPORT_SCHEMA`].
    pub schema: String,
    /// Who measured (`beatbyte <version>`).
    pub measured_by: String,
    /// The audio file's name (beside this sidecar).
    pub audio: String,
    /// The audio file's size, bytes.
    pub bytes: u64,
    /// The measurement.
    #[serde(flatten)]
    pub measurement: Measurement,
    /// The file's technical quality.
    pub quality: Quality,
}

impl Report {
    /// The gain the game applies, dB.
    #[must_use]
    pub fn gain_db(&self) -> f64 {
        play_gain_db(
            self.measurement.integrated_lufs,
            self.measurement.true_peak_dbtp,
            TARGET_LUFS,
            CEILING_DBTP,
        )
    }

    /// Whether the peaks, not the target, set the gain.
    #[must_use]
    pub fn peak_limited(&self) -> bool {
        peak_limited(
            self.measurement.integrated_lufs,
            self.measurement.true_peak_dbtp,
            TARGET_LUFS,
            CEILING_DBTP,
        )
    }
}

/// Decode, measure and judge a file: everything the sidecar holds.
/// `measured_by` names the build (`beatbyte 0.14.33`).
pub fn measure_file(path: &Path, measured_by: &str) -> Result<Report, crate::decode::DecodeError> {
    let audio = crate::decode::decode_file_channels(path)?;
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let measurement = measure(&audio);
    let quality = crate::quality::assess(
        &audio,
        bytes,
        crate::quality::is_lossy(path),
        measurement.true_peak_dbtp,
    );
    Ok(Report {
        schema: REPORT_SCHEMA.to_owned(),
        measured_by: measured_by.to_owned(),
        audio: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bytes,
        measurement,
        quality,
    })
}

/// Where a song's report lives: `<audio stem>.loudness.json` beside
/// the audio (the `words.json` convention).
#[must_use]
pub fn sidecar_path(audio_path: &Path) -> PathBuf {
    let stem = audio_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    audio_path.with_file_name(format!("{stem}.loudness.json"))
}

/// Read a song's report, if one is beside the audio and readable.
/// A report of another schema is ignored, not an error — an older
/// game must not fail on a newer sidecar, and vice versa.
#[must_use]
pub fn read_report(audio_path: &Path) -> Option<Report> {
    let text = std::fs::read_to_string(sidecar_path(audio_path)).ok()?;
    let report: Report = serde_json::from_str(&text).ok()?;
    (report.schema == REPORT_SCHEMA).then_some(report)
}

/// Write a song's report beside the audio.
pub fn write_report(audio_path: &Path, report: &Report) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(report).map_err(std::io::Error::other)?;
    std::fs::write(sidecar_path(audio_path), text)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f64, seconds: f64, amplitude: f32, phase: f64) -> Vec<f32> {
        (0..(seconds * f64::from(RATE)) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                amplitude * (std::f64::consts::TAU * hz * t + phase).sin() as f32
            })
            .collect()
    }

    #[test]
    fn the_standards_calibration_signals_read_what_the_standard_says() {
        // BS.1770: a 997 Hz sine at −20 dBFS in BOTH channels of a
        // stereo pair reads −20.0 LUFS; in one channel alone, −23.0.
        let tone = sine(997.0, 6.0, db_to_linear(-20.0) as f32, 0.0);
        let stereo = measure_planes(&[tone.clone(), tone.clone()], RATE);
        let l = stereo.integrated_lufs.unwrap();
        assert!((l + 20.0).abs() < 0.1, "stereo {l}");
        let single = measure_planes(&[tone.clone(), vec![0.0; tone.len()]], RATE);
        let l = single.integrated_lufs.unwrap();
        assert!((l + 23.0).abs() < 0.1, "one channel {l}");
        // The peak of a −20 dBFS sine is −20 dBTP, give or take the
        // interpolator (under a tenth of a dB).
        assert!(
            (stereo.true_peak_dbtp + 20.0).abs() < 0.1,
            "{}",
            stereo.true_peak_dbtp
        );
        // The same at 44.1 kHz: the filters are derived per rate.
        let tone44: Vec<f32> = (0..44_100 * 6)
            .map(|i| {
                db_to_linear(-20.0) as f32
                    * (std::f64::consts::TAU * 997.0 * i as f64 / 44_100.0).sin() as f32
            })
            .collect();
        let l = measure_planes(&[tone44.clone(), tone44], 44_100)
            .integrated_lufs
            .unwrap();
        assert!((l + 20.0).abs() < 0.1, "44.1 kHz {l}");
    }

    #[test]
    fn the_k_weighting_lifts_the_highs_and_drops_the_lows() {
        let quiet = db_to_linear(-20.0) as f32;
        let at = |hz: f64| {
            let t = sine(hz, 6.0, quiet, 0.0);
            measure_planes(&[t.clone(), t], RATE)
                .integrated_lufs
                .unwrap()
        };
        let mid = at(997.0);
        assert!(at(8000.0) > mid + 3.0, "8 kHz {} vs {mid}", at(8000.0));
        assert!(at(40.0) < mid - 2.0, "40 Hz {} vs {mid}", at(40.0));
    }

    #[test]
    fn the_gate_ignores_silence_and_the_quiet_tail() {
        // Six seconds of tone then six of silence: the absolute gate
        // drops the silence and the tone's loudness stands.
        let quiet = db_to_linear(-20.0) as f32;
        let mut tone = sine(997.0, 6.0, quiet, 0.0);
        tone.extend(vec![0.0; 6 * RATE as usize]);
        // (The blocks straddling the edge count in part: −20.1.)
        let l = measure_planes(&[tone.clone(), tone], RATE)
            .integrated_lufs
            .unwrap();
        assert!((l + 20.0).abs() < 0.25, "{l}");
        assert!(l > -20.5, "{l}");
        // A −40 dB tail under a −20 dB body is under the relative
        // gate too, so it does not pull the reading down.
        let mut body = sine(997.0, 6.0, quiet, 0.0);
        body.extend(sine(997.0, 6.0, db_to_linear(-40.0) as f32, 0.0));
        let l = measure_planes(&[body.clone(), body], RATE)
            .integrated_lufs
            .unwrap();
        // (The blocks straddling the step count in part: −20.1.)
        assert!((l + 20.0).abs() < 0.25, "{l}");
        // Without the relative gate the tail would pull it to −23.
        assert!(l > -20.5, "{l}");
        // Silence reads nothing.
        let silent = vec![0.0f32; 4 * RATE as usize];
        assert_eq!(
            measure_planes(&[silent.clone(), silent], RATE).integrated_lufs,
            None
        );
        assert_eq!(measure_planes(&[], RATE).integrated_lufs, None);
    }

    #[test]
    fn the_true_peak_sees_between_the_samples() {
        // A sine at a quarter of the rate sampled at 45°: every
        // sample is ±0.707, the waveform peaks at 1.0.
        let t = sine(f64::from(RATE) / 4.0, 1.0, 1.0, std::f64::consts::FRAC_PI_4);
        let sample_peak = t.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!((sample_peak - 0.707).abs() < 0.01, "{sample_peak}");
        let tp = true_peak_of(&t);
        assert!(tp > 0.98 && tp < 1.02, "{tp}");
        assert_eq!(true_peak_of(&[]), 0.0);
    }

    #[test]
    fn the_loudness_range_is_the_spread_of_the_short_term_blocks() {
        // Ten seconds at −30 then ten at −20: two levels, ~10 LU apart.
        let mut s = sine(997.0, 10.0, db_to_linear(-30.0) as f32, 0.0);
        s.extend(sine(997.0, 10.0, db_to_linear(-20.0) as f32, 0.0));
        let m = measure_planes(&[s.clone(), s], RATE);
        let lra = m.loudness_range_lu.unwrap();
        assert!(lra > 8.0 && lra < 10.5, "{lra}");
        // A steady tone has no range.
        let flat = sine(997.0, 10.0, 0.1, 0.0);
        let lra = measure_planes(&[flat.clone(), flat], RATE)
            .loudness_range_lu
            .unwrap();
        assert!(lra < 0.5, "{lra}");
    }

    #[test]
    fn the_gain_reaches_the_target_unless_the_peaks_forbid_it() {
        // −20 LUFS, peak −6 dBTP: +4 dB to the target, room to spare.
        assert!((play_gain_db(Some(-20.0), -6.0, -16.0, -1.0) - 4.0).abs() < 1e-9);
        assert!(!peak_limited(Some(-20.0), -6.0, -16.0, -1.0));
        // −26 LUFS, peak −3 dBTP: wants +10, may have +2.
        assert!((play_gain_db(Some(-26.0), -3.0, -16.0, -1.0) - 2.0).abs() < 1e-9);
        assert!(peak_limited(Some(-26.0), -3.0, -16.0, -1.0));
        // A loud master turns down, ceiling irrelevant.
        assert!((play_gain_db(Some(-8.0), -0.2, -16.0, -1.0) + 8.0).abs() < 1e-9);
        // Nothing measured, nothing changed; absurd values clamped.
        assert_eq!(play_gain_db(None, -1.0, -16.0, -1.0), 0.0);
        assert_eq!(play_gain_db(Some(-80.0), -60.0, -16.0, -1.0), GAIN_LIMIT_DB);
        assert_eq!(play_gain_db(Some(f64::NAN), -1.0, -16.0, -1.0), 0.0);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 1e-4);
        assert!((to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(to_db(0.0), -120.0);
    }

    #[test]
    fn the_sidecar_sits_beside_the_audio_and_round_trips() {
        let path = Path::new("/songs/x/Toto - Africa.m4a");
        assert_eq!(
            sidecar_path(path),
            PathBuf::from("/songs/x/Toto - Africa.loudness.json")
        );
        let dir = std::env::temp_dir().join(format!("beatbyte-loudness-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("song.wav");
        let report = Report {
            schema: REPORT_SCHEMA.to_owned(),
            measured_by: "test".to_owned(),
            audio: "song.wav".to_owned(),
            bytes: 10,
            measurement: Measurement {
                integrated_lufs: Some(-20.0),
                loudness_range_lu: Some(6.0),
                true_peak_dbtp: -3.0,
                sample_peak_dbfs: -3.2,
                duration_s: 30.0,
            },
            quality: Quality::default(),
        };
        assert_eq!(read_report(&audio), None);
        write_report(&audio, &report).unwrap();
        assert_eq!(read_report(&audio), Some(report.clone()));
        assert!(
            (report.gain_db() - 2.0).abs() < 1e-9,
            "wants +4, peaks allow +2"
        );
        assert!(report.peak_limited());
        // Another schema is not read.
        std::fs::write(sidecar_path(&audio), r#"{"schema":"beatbyte.loudness/9"}"#).unwrap();
        assert_eq!(read_report(&audio), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
