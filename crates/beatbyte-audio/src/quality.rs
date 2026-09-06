//! What a file's audio can and cannot resolve — and whether to warn.
//!
//! A container's bitrate is not evidence: a 320 kbps file made from a
//! 96 kbps source looks fine and sounds like the source. So the checks
//! read the signal: where the spectrum ends (a lossy encoder low-passes,
//! and a cliff of twenty dB inside 500 Hz is a cliff no instrument
//! makes), how many samples sit flat against full scale (clipping),
//! whether the reconstructed waveform passes 0 dBTP, and the DC
//! offset — plus the file's facts (sample rate, channels, bitrate).
//! Every threshold is a named constant; the verdict is a pure function
//! of the numbers, pinned at its boundaries.

use std::path::Path;

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::decode::Channels;

/// Below this the spectrum's end is a defect, not a taste.
pub const BANDWIDTH_POOR_HZ: f64 = 15_000.0;
/// Below this it is worth a note.
pub const BANDWIDTH_FAIR_HZ: f64 = 17_000.0;
/// Lossy bitrate under which a file is poor (kbps).
pub const BITRATE_POOR_KBPS: u32 = 96;
/// Lossy bitrate under which a file is worth a note (kbps).
pub const BITRATE_FAIR_KBPS: u32 = 160;
/// Sample rate under which a file is poor.
pub const RATE_POOR_HZ: u32 = 32_000;
/// Sample rate under which a file is worth a note.
pub const RATE_FAIR_HZ: u32 = 44_100;
/// Share of samples in flat-top runs at full scale that is poor.
pub const CLIPPING_POOR: f64 = 0.001;
/// Share of clipped samples worth a note.
pub const CLIPPING_FAIR: f64 = 0.000_1;
/// DC offset worth a note (fraction of full scale).
pub const DC_FAIR: f64 = 0.02;
/// True peak above which inter-sample overs are worth a flag (dBTP);
/// up to it they are a note.
pub const TRUE_PEAK_FAIR_DBTP: f64 = 1.0;
/// A flat-top run this long at full scale is a clip, not a peak.
const CLIP_RUN: usize = 3;
const CLIP_LEVEL: f32 = 0.999;
/// The spectrum's cliff: this much drop inside this band.
const CLIFF_DB: f64 = 20.0;
const CLIFF_BAND_HZ: f64 = 500.0;
/// Cliffs are looked for above this; below it the music itself has
/// gaps.
const CLIFF_FROM_HZ: f64 = 8_000.0;
const FRAME: usize = 4096;

/// How the file fares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Nothing to say.
    #[default]
    Good,
    /// Worth a note; plays fine.
    Fair,
    /// Audibly limited; the import warns.
    Poor,
}

impl Verdict {
    /// One word for a list.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Verdict::Good => "good",
            Verdict::Fair => "fair",
            Verdict::Poor => "poor",
        }
    }
}

/// One thing the checks found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// How bad.
    pub severity: Verdict,
    /// What, in one line for a person.
    pub what: String,
}

/// The file's facts, the signal's measurements and the verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Quality {
    /// Sample rate, hertz.
    pub sample_rate: u32,
    /// Channels.
    pub channels: usize,
    /// Average container bitrate, kbps (file size over duration);
    /// meaningful for lossy codecs.
    pub bitrate_kbps: Option<u32>,
    /// Whether the container is a lossy codec (by extension).
    pub lossy: bool,
    /// Where the spectrum ends, hertz, when a cliff was found; `None`
    /// means the band is full to the file's own limit.
    pub bandwidth_hz: Option<f64>,
    /// Share of samples inside flat-top runs at full scale.
    pub clipping_share: f64,
    /// Mean of the signal, fraction of full scale.
    pub dc_offset: f64,
    /// The verdict.
    pub verdict: Verdict,
    /// Everything found, worst first.
    pub issues: Vec<Issue>,
}

/// Whether a path names a lossy container.
#[must_use]
pub fn is_lossy(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp3" | "m4a" | "aac" | "ogg" | "oga" | "opus" | "wma" | "mp4")
    )
}

/// Assess a decoded file. `bytes` is the file's size and `lossy`
/// its container's kind ([`is_lossy`]); `true_peak_dbtp` comes from
/// the loudness meter.
#[must_use]
pub fn assess(audio: &Channels, bytes: u64, lossy: bool, true_peak_dbtp: f64) -> Quality {
    let mono = audio.mono();
    let duration_s = audio.duration_s();
    let bitrate_kbps =
        (duration_s > 0.0).then(|| (bytes as f64 * 8.0 / duration_s / 1000.0).round() as u32);
    let facts = Facts {
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        bitrate_kbps,
        lossy,
        bandwidth_hz: bandwidth_cutoff(&mono, audio.sample_rate),
        clipping_share: clipping_share(&audio.interleaved),
        dc_offset: dc_offset(&mono),
        true_peak_dbtp,
    };
    let (verdict, issues) = judge(&facts);
    Quality {
        sample_rate: facts.sample_rate,
        channels: facts.channels,
        bitrate_kbps,
        lossy,
        bandwidth_hz: facts.bandwidth_hz,
        clipping_share: facts.clipping_share,
        dc_offset: facts.dc_offset,
        verdict,
        issues,
    }
}

/// The numbers the verdict is a function of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts {
    /// Sample rate, hertz.
    pub sample_rate: u32,
    /// Channels.
    pub channels: usize,
    /// Container bitrate, kbps.
    pub bitrate_kbps: Option<u32>,
    /// Lossy container.
    pub lossy: bool,
    /// Spectrum cliff, hertz.
    pub bandwidth_hz: Option<f64>,
    /// Clipped share.
    pub clipping_share: f64,
    /// DC offset.
    pub dc_offset: f64,
    /// True peak, dBTP.
    pub true_peak_dbtp: f64,
}

/// The verdict and its reasons, worst first. Pure — pinned at every
/// threshold.
#[must_use]
pub fn judge(f: &Facts) -> (Verdict, Vec<Issue>) {
    let mut issues = Vec::new();
    let mut push = |severity: Verdict, what: String| issues.push(Issue { severity, what });
    if f.sample_rate < RATE_POOR_HZ {
        push(
            Verdict::Poor,
            format!("sample rate {:.1} kHz", f64::from(f.sample_rate) / 1000.0),
        );
    } else if f.sample_rate < RATE_FAIR_HZ {
        push(
            Verdict::Fair,
            format!("sample rate {:.1} kHz", f64::from(f.sample_rate) / 1000.0),
        );
    }
    if f.lossy
        && let Some(kbps) = f.bitrate_kbps
    {
        if kbps < BITRATE_POOR_KBPS {
            push(Verdict::Poor, format!("{kbps} kbps"));
        } else if kbps < BITRATE_FAIR_KBPS {
            push(Verdict::Fair, format!("{kbps} kbps"));
        }
    }
    if let Some(hz) = f.bandwidth_hz {
        let what = format!(
            "spectrum ends at {:.1} kHz (a low-bitrate encode somewhere in its past)",
            hz / 1000.0
        );
        if hz < BANDWIDTH_POOR_HZ {
            push(Verdict::Poor, what);
        } else if hz < BANDWIDTH_FAIR_HZ {
            push(Verdict::Fair, what);
        }
    }
    if f.clipping_share > CLIPPING_POOR {
        push(
            Verdict::Poor,
            format!("clipped ({:.2} % of the samples)", f.clipping_share * 100.0),
        );
    } else if f.clipping_share > CLIPPING_FAIR {
        push(
            Verdict::Fair,
            format!(
                "some clipping ({:.3} % of the samples)",
                f.clipping_share * 100.0
            ),
        );
    }
    // Modern masters routinely reconstruct a little over full scale
    // between samples; that is a note, and normalisation pulls them
    // under the ceiling anyway. A full dB over is worth a flag.
    if f.true_peak_dbtp > TRUE_PEAK_FAIR_DBTP {
        push(
            Verdict::Fair,
            format!(
                "true peak {:+.1} dBTP (inter-sample overs)",
                f.true_peak_dbtp
            ),
        );
    } else if f.true_peak_dbtp > 0.0 {
        push(
            Verdict::Good,
            format!("true peak {:+.1} dBTP", f.true_peak_dbtp),
        );
    }
    if f.dc_offset.abs() > DC_FAIR {
        push(
            Verdict::Fair,
            format!("DC offset {:+.1} %", f.dc_offset * 100.0),
        );
    }
    if f.channels == 1 {
        push(Verdict::Good, "mono".to_owned());
    }
    issues.sort_by_key(|i| std::cmp::Reverse(i.severity));
    let verdict = issues
        .iter()
        .map(|i| i.severity)
        .max()
        .unwrap_or(Verdict::Good);
    (verdict, issues)
}

/// Where the long-term spectrum ends: the lowest frequency above
/// `CLIFF_FROM_HZ` where the level drops by `CLIFF_DB` within
/// `CLIFF_BAND_HZ` and stays down to the file's limit. `None` when
/// no such cliff exists (the band is full).
#[must_use]
pub fn bandwidth_cutoff(mono: &[f32], sample_rate: u32) -> Option<f64> {
    let spectrum = long_term_spectrum_db(mono, sample_rate)?;
    let bin_hz = f64::from(sample_rate) / FRAME as f64;
    let bins = spectrum.len();
    let band = (CLIFF_BAND_HZ / bin_hz).round().max(1.0) as usize;
    let from = (CLIFF_FROM_HZ / bin_hz) as usize;
    let mean = |a: usize, b: usize| -> f64 {
        let b = b.min(bins);
        if b <= a {
            return f64::NEG_INFINITY;
        }
        spectrum[a..b].iter().sum::<f64>() / (b - a) as f64
    };
    let mut k = from;
    while k + 2 * band <= bins {
        let below = mean(k, k + band);
        let above = mean(k + band, k + 2 * band);
        if below - above >= CLIFF_DB {
            // ...and it stays down: everything above the cliff sits
            // under the level just below it.
            let rest = mean(k + band, bins);
            if below - rest >= CLIFF_DB {
                return Some((k + band) as f64 * bin_hz);
            }
        }
        k += band / 4;
        if band < 4 {
            k += 1;
        }
    }
    None
}

/// The long-term average power spectrum in dB (Hann frames of
/// `FRAME`, hop half), smoothed over three bins; `None` under one
/// frame.
fn long_term_spectrum_db(mono: &[f32], sample_rate: u32) -> Option<Vec<f64>> {
    let _ = sample_rate;
    if mono.len() < FRAME {
        return None;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FRAME);
    let hann: Vec<f32> = (0..FRAME)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAME as f32).cos())
        .collect();
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let bins = FRAME / 2 + 1;
    let mut power = vec![0.0f64; bins];
    let mut frames = 0usize;
    let mut start = 0;
    while start + FRAME <= mono.len() {
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = mono[start + i] * hann[i];
        }
        if fft.process(&mut input, &mut output).is_err() {
            break;
        }
        for (p, c) in power.iter_mut().zip(&output) {
            *p += f64::from(c.norm_sqr());
        }
        frames += 1;
        start += FRAME / 2;
    }
    if frames == 0 {
        return None;
    }
    let db: Vec<f64> = power
        .iter()
        .map(|p| 10.0 * (p / frames as f64 + 1e-20).log10())
        .collect();
    Some(
        (0..bins)
            .map(|i| {
                let a = i.saturating_sub(1);
                let b = (i + 2).min(bins);
                db[a..b].iter().sum::<f64>() / (b - a) as f64
            })
            .collect(),
    )
}

/// Share of samples inside runs of at least `CLIP_RUN` consecutive
/// samples at or above `CLIP_LEVEL` of full scale — a waveform
/// flattened against the ceiling, not merely a loud one.
#[must_use]
pub fn clipping_share(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut clipped = 0usize;
    let mut run = 0usize;
    for &x in samples {
        if x.abs() >= CLIP_LEVEL {
            run += 1;
        } else {
            if run >= CLIP_RUN {
                clipped += run;
            }
            run = 0;
        }
    }
    if run >= CLIP_RUN {
        clipped += run;
    }
    clipped as f64 / samples.len() as f64
}

/// The signal's mean.
#[must_use]
pub fn dc_offset(mono: &[f32]) -> f64 {
    if mono.is_empty() {
        return 0.0;
    }
    mono.iter().map(|&x| f64::from(x)).sum::<f64>() / mono.len() as f64
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Deterministic white noise in (−0.5, 0.5).
    fn noise(n: usize) -> Vec<f32> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        (0..n)
            .map(|_| {
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                (z >> 40) as f32 / (1u64 << 24) as f32 - 0.5
            })
            .collect()
    }

    /// Brick-wall low-pass by zeroing FFT bins above `cutoff_hz`,
    /// frame by frame (a crude encoder's low-pass).
    fn lowpass(samples: &[f32], cutoff_hz: f64) -> Vec<f32> {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(FRAME);
        let inverse = planner.plan_fft_inverse(FRAME);
        let bin_hz = f64::from(RATE) / FRAME as f64;
        let keep = (cutoff_hz / bin_hz) as usize;
        let mut out = Vec::with_capacity(samples.len());
        let mut input = forward.make_input_vec();
        let mut spectrum = forward.make_output_vec();
        for chunk in samples.chunks(FRAME) {
            if chunk.len() < FRAME {
                break;
            }
            input.copy_from_slice(chunk);
            forward.process(&mut input, &mut spectrum).unwrap();
            for (k, c) in spectrum.iter_mut().enumerate() {
                if k > keep {
                    *c = realfft::num_complex::Complex::new(0.0, 0.0);
                }
            }
            let mut back = inverse.make_output_vec();
            inverse.process(&mut spectrum, &mut back).unwrap();
            out.extend(back.iter().map(|x| x / FRAME as f32));
        }
        out
    }

    #[test]
    fn a_lossy_low_pass_is_found_where_it_is_and_a_full_band_is_not() {
        let full = noise(RATE as usize * 8);
        assert_eq!(bandwidth_cutoff(&full, RATE), None, "noise is full band");
        for cutoff in [12_000.0, 16_000.0] {
            let cut = lowpass(&full, cutoff);
            let found = bandwidth_cutoff(&cut, RATE).expect("a cliff");
            assert!((found - cutoff).abs() < 400.0, "{cutoff}: found {found}");
        }
        assert_eq!(bandwidth_cutoff(&[0.0; 100], RATE), None, "too short");
    }

    #[test]
    fn clipping_is_flat_tops_not_loudness() {
        let n = RATE as usize;
        let loud: Vec<f32> = (0..n)
            .map(|i| 0.95 * (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin())
            .collect();
        assert_eq!(
            clipping_share(&loud),
            0.0,
            "a loud clean sine is not clipped"
        );
        let clipped: Vec<f32> = loud.iter().map(|x| (x * 1.5).clamp(-1.0, 1.0)).collect();
        let share = clipping_share(&clipped);
        assert!(share > 0.05, "{share}");
        // A lone full-scale sample is a peak, not a clip.
        let mut single = vec![0.0f32; 100];
        single[50] = 1.0;
        assert_eq!(clipping_share(&single), 0.0);
        assert_eq!(clipping_share(&[]), 0.0);
        assert!((dc_offset(&[0.1, 0.1, 0.1]) - 0.1).abs() < 1e-6);
        assert_eq!(dc_offset(&[]), 0.0);
    }

    fn facts() -> Facts {
        Facts {
            sample_rate: 44_100,
            channels: 2,
            bitrate_kbps: Some(256),
            lossy: true,
            bandwidth_hz: None,
            clipping_share: 0.0,
            dc_offset: 0.0,
            true_peak_dbtp: -1.5,
        }
    }

    #[test]
    fn the_verdict_turns_at_every_threshold_and_is_the_worst_issue() {
        assert_eq!(judge(&facts()), (Verdict::Good, vec![]));
        let poor_rate = Facts {
            sample_rate: RATE_POOR_HZ - 1,
            ..facts()
        };
        assert_eq!(judge(&poor_rate).0, Verdict::Poor);
        assert_eq!(
            judge(&Facts {
                sample_rate: RATE_FAIR_HZ - 1,
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        assert_eq!(
            judge(&Facts {
                bitrate_kbps: Some(BITRATE_POOR_KBPS - 1),
                ..facts()
            })
            .0,
            Verdict::Poor
        );
        assert_eq!(
            judge(&Facts {
                bitrate_kbps: Some(BITRATE_FAIR_KBPS - 1),
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        // A lossless container's bitrate says nothing.
        assert_eq!(
            judge(&Facts {
                bitrate_kbps: Some(40),
                lossy: false,
                ..facts()
            })
            .0,
            Verdict::Good
        );
        assert_eq!(
            judge(&Facts {
                bandwidth_hz: Some(BANDWIDTH_POOR_HZ - 1.0),
                ..facts()
            })
            .0,
            Verdict::Poor
        );
        assert_eq!(
            judge(&Facts {
                bandwidth_hz: Some(BANDWIDTH_FAIR_HZ - 1.0),
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        assert_eq!(
            judge(&Facts {
                bandwidth_hz: Some(BANDWIDTH_FAIR_HZ),
                ..facts()
            })
            .0,
            Verdict::Good
        );
        assert_eq!(
            judge(&Facts {
                clipping_share: CLIPPING_POOR * 2.0,
                ..facts()
            })
            .0,
            Verdict::Poor
        );
        assert_eq!(
            judge(&Facts {
                clipping_share: CLIPPING_FAIR * 2.0,
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        let (verdict, issues) = judge(&Facts {
            true_peak_dbtp: 0.3,
            ..facts()
        });
        assert_eq!(verdict, Verdict::Good, "a few tenths over is a note");
        assert!(issues[0].what.contains("+0.3 dBTP"));
        assert_eq!(
            judge(&Facts {
                true_peak_dbtp: TRUE_PEAK_FAIR_DBTP + 0.1,
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        assert_eq!(
            judge(&Facts {
                dc_offset: DC_FAIR * 2.0,
                ..facts()
            })
            .0,
            Verdict::Fair
        );
        // Mono is noted, not judged.
        let (verdict, issues) = judge(&Facts {
            channels: 1,
            ..facts()
        });
        assert_eq!(verdict, Verdict::Good);
        assert_eq!(issues[0].what, "mono");
        // The worst issue leads.
        let (verdict, issues) = judge(&Facts {
            dc_offset: 0.1,
            bandwidth_hz: Some(11_000.0),
            ..facts()
        });
        assert_eq!(verdict, Verdict::Poor);
        assert_eq!(issues[0].severity, Verdict::Poor);
        assert!(issues[0].what.contains("11.0 kHz"), "{}", issues[0].what);
    }

    #[test]
    fn lossy_is_a_matter_of_container() {
        assert!(is_lossy(Path::new("a/song.M4A")));
        assert!(is_lossy(Path::new("song.mp3")));
        assert!(!is_lossy(Path::new("song.wav")));
        assert!(!is_lossy(Path::new("song.flac")));
        assert!(!is_lossy(Path::new("song")));
    }
}
