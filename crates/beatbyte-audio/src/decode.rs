//! Decoding user-provided music into analyzable sample buffers.
//!
//! Playback streams straight from disk (see [`crate::playback`]); this
//! module fully decodes into memory only for *analysis*, which needs
//! random access. Untrusted input rules apply: decode length is capped
//! and decoder failures are errors, never panics.

use std::fs::{self, File};
use std::path::Path;

use rodio::{Decoder, Source};
use thiserror::Error;

use crate::priming::{Priming, container_priming};

/// Analysis decodes at most this many seconds of audio (memory guard;
/// a 20-minute mono track at 48 kHz is ~220 MB of f64-free f32 data).
pub const MAX_ANALYSIS_SECONDS: f64 = 1_200.0;

/// Errors decoding an audio file.
#[derive(Debug, Error)]
pub enum DecodeError {
    /// The file could not be opened.
    #[error("cannot open `{path}`: {source}")]
    Open {
        /// The file involved.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The audio format was not recognized or is unsupported.
    #[error("cannot decode `{path}`: {source}")]
    Decode {
        /// The file involved.
        path: String,
        /// The underlying decoder error.
        #[source]
        source: rodio::decoder::DecoderError,
    },
    /// The file decoded to zero samples.
    #[error("`{path}` contains no audio")]
    Empty {
        /// The file involved.
        path: String,
    },
}

/// Decoded mono audio ready for analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    samples: Vec<f32>,
    sample_rate: u32,
    /// Whether decoding stopped at [`MAX_ANALYSIS_SECONDS`].
    truncated: bool,
    /// The encoder priming the container declared and this decode
    /// SKIPPED, so sample 0 is the master's sample 0. Zero for
    /// containers that declare none.
    priming: Priming,
}

impl AudioData {
    /// Wrap an existing mono buffer (used by tests and synthesis).
    #[must_use]
    pub fn from_mono(samples: Vec<f32>, sample_rate: u32) -> AudioData {
        AudioData {
            samples,
            sample_rate: sample_rate.max(1),
            truncated: false,
            priming: Priming::default(),
        }
    }

    /// The encoder priming skipped before sample 0 — what a chart
    /// generated from this decode records as its
    /// timeline (`audio_trim`), so a chart from before the skip can be
    /// told apart from one after it.
    #[must_use]
    pub fn priming(&self) -> Priming {
        self.priming
    }

    /// The mono samples in −1.0..=1.0.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Samples per second.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whether decoding hit the analysis length cap.
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Duration of the decoded audio in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    /// Halve the sample rate with a half-band FIR low-pass (anti-alias)
    /// — analysis quality is unaffected below ~10 kHz and memory/FFT
    /// cost halve. Returns `self` unchanged if the rate is already low.
    #[must_use]
    pub fn downsample_half(self) -> AudioData {
        if self.sample_rate < 32_000 || self.samples.len() < 64 {
            return self;
        }
        let taps = half_band_taps::<31>();
        let half = taps.len() / 2;
        let n_out = self.samples.len() / 2;
        let mut out = Vec::with_capacity(n_out);
        for i in 0..n_out {
            let center = i * 2;
            let mut acc = 0.0f32;
            for (k, tap) in taps.iter().enumerate() {
                let idx = center as isize + k as isize - half as isize;
                if idx >= 0 && (idx as usize) < self.samples.len() {
                    acc += tap * self.samples[idx as usize];
                }
            }
            out.push(acc);
        }
        AudioData {
            samples: out,
            sample_rate: self.sample_rate / 2,
            truncated: self.truncated,
            priming: self.priming,
        }
    }
}

/// Windowed-sinc half-band low-pass taps (cutoff at ¼ of the input
/// rate, i.e. the new Nyquist), Hann-windowed, unity DC gain.
fn half_band_taps<const N: usize>() -> [f32; N] {
    let mut taps = [0.0f32; N];
    let half = (N / 2) as isize;
    let mut sum = 0.0f32;
    for (k, tap) in taps.iter_mut().enumerate() {
        let n = k as isize - half;
        let x = n as f32;
        // sinc at cutoff 0.25 (normalized to the input rate).
        let sinc = if n == 0 {
            0.5
        } else {
            (0.5 * core::f32::consts::PI * x).sin() / (core::f32::consts::PI * x)
        };
        let window = 0.5 + 0.5 * (core::f32::consts::PI * x / (half as f32 + 1.0)).cos();
        *tap = sinc * window;
        sum += *tap;
    }
    // Normalize to unity gain at DC.
    for tap in &mut taps {
        *tap /= sum;
    }
    taps
}

/// Encode mono audio as a 16-bit PCM WAV in memory (the header is
/// trivial and saves a dependency). Used for demo material on disk and
/// for procedurally generated SFX handed to the engine's audio assets.
#[must_use]
pub fn wav_bytes_mono16(audio: &AudioData) -> Vec<u8> {
    let data_len = (audio.samples().len() * 2) as u32;
    let rate = audio.sample_rate();
    let byte_rate = rate * 2;

    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for &sample in audio.samples() {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Write mono audio as a 16-bit PCM WAV file.
pub fn write_wav_mono16(path: &Path, audio: &AudioData) -> std::io::Result<()> {
    fs::write(path, wav_bytes_mono16(audio))
}

/// The frames a container's declared priming amounts to at the
/// decoder's rate. The declaration is in the track's own timescale,
/// which for MP4 audio is the sample rate; when a file says otherwise
/// the count is rescaled rather than trusted blindly. Pure — tested.
#[must_use]
pub fn priming_frames(priming: Priming, sample_rate: u32) -> u64 {
    if priming.timescale == 0 || priming.samples == 0 {
        return 0;
    }
    if priming.timescale == sample_rate {
        return u64::from(priming.samples);
    }
    u64::from(priming.samples) * u64::from(sample_rate) / u64::from(priming.timescale)
}

/// The sample rate a file decodes at, from its header alone — no
/// samples are pulled. `None` when the file cannot be opened or is
/// not audio.
#[must_use]
pub fn probe_sample_rate(path: &Path) -> Option<u32> {
    let file = File::open(path).ok()?;
    let decoder = Decoder::try_from(file).ok()?;
    Some(decoder.sample_rate().get())
}

/// Decode an audio file to mono for analysis. Multi-channel input is
/// downmixed by averaging; decoding stops at [`MAX_ANALYSIS_SECONDS`].
///
/// The container's declared encoder priming is skipped, so the first
/// sample is the master's first sample — the same skip the playback
/// path applies, and it has to be the same: analysis alone skipping
/// would put every chart a frame ahead of the audio it is played
/// against.
pub fn decode_file(path: &Path) -> Result<AudioData, DecodeError> {
    let display = path.display().to_string();
    let file = File::open(path).map_err(|source| DecodeError::Open {
        path: display.clone(),
        source,
    })?;
    let decoder = Decoder::try_from(file).map_err(|source| DecodeError::Decode {
        path: display.clone(),
        source,
    })?;

    let sample_rate = decoder.sample_rate().get();
    let channels = u32::from(decoder.channels().get()).max(1);
    let max_mono_samples = (MAX_ANALYSIS_SECONDS * f64::from(sample_rate)) as usize;
    let priming = container_priming(path);
    let skip = priming_frames(priming, sample_rate) * u64::from(channels);

    let mut samples = Vec::new();
    let mut truncated = false;
    let mut frame_acc = 0.0f32;
    let mut frame_fill = 0u32;
    for sample in decoder.skip(usize::try_from(skip).unwrap_or(usize::MAX)) {
        frame_acc += sample;
        frame_fill += 1;
        if frame_fill == channels {
            samples.push(frame_acc / channels as f32);
            frame_acc = 0.0;
            frame_fill = 0;
            if samples.len() >= max_mono_samples {
                truncated = true;
                break;
            }
        }
    }

    if samples.is_empty() {
        return Err(DecodeError::Empty { path: display });
    }
    Ok(AudioData {
        samples,
        sample_rate,
        truncated,
        priming,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, channels: u16, sample_rate: u32, frames: &[f32]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &sample in frames {
            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("beatbyte-audio-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn decodes_mono_wav() {
        let path = scratch("mono.wav");
        let sine: Vec<f32> = (0..4410).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        write_wav(&path, 1, 44_100, &sine);

        let audio = decode_file(&path).unwrap();
        assert_eq!(audio.sample_rate(), 44_100);
        assert_eq!(audio.samples().len(), 4410);
        assert!((audio.duration_s() - 0.1).abs() < 1e-3);
        assert!(!audio.truncated());
        // Content survives the 16-bit round trip approximately.
        assert!((audio.samples()[100] - sine[100]).abs() < 0.001);
    }

    #[test]
    fn stereo_downmixes_to_mono() {
        let path = scratch("stereo.wav");
        // L = +0.5, R = -0.5 → mono ≈ 0.
        let frames: Vec<f32> = (0..2000)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        write_wav(&path, 2, 44_100, &frames);

        let audio = decode_file(&path).unwrap();
        assert_eq!(audio.samples().len(), 1000);
        for &sample in audio.samples() {
            assert!(sample.abs() < 0.001, "downmix should cancel: {sample}");
        }
    }

    #[test]
    fn garbage_file_is_a_decode_error() {
        let path = scratch("garbage.wav");
        std::fs::write(&path, b"this is not audio at all").unwrap();
        assert!(matches!(
            decode_file(&path),
            Err(DecodeError::Decode { .. })
        ));
    }

    #[test]
    fn missing_file_is_an_open_error() {
        assert!(matches!(
            decode_file(Path::new("/definitely/not/here.ogg")),
            Err(DecodeError::Open { .. })
        ));
    }

    #[test]
    fn downsampling_halves_rate_and_preserves_low_frequencies() {
        // 440 Hz sine at 44.1 kHz.
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..rate)
            .map(|i| (2.0 * core::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.8)
            .collect();
        let audio = AudioData::from_mono(samples, rate).downsample_half();
        assert_eq!(audio.sample_rate(), 22_050);

        // RMS of a sine is amplitude/√2; the low-pass must keep it.
        let rms = (audio.samples().iter().map(|s| s * s).sum::<f32>()
            / audio.samples().len() as f32)
            .sqrt();
        let expected = 0.8 / 2.0f32.sqrt();
        assert!(
            (rms - expected).abs() < 0.02,
            "440 Hz content should survive: rms={rms}, expected≈{expected}"
        );
    }

    #[test]
    fn downsampling_kills_frequencies_above_the_new_nyquist() {
        // 15 kHz sine at 44.1 kHz — above the new 11.025 kHz Nyquist.
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..rate)
            .map(|i| (2.0 * core::f32::consts::PI * 15_000.0 * i as f32 / rate as f32).sin())
            .collect();
        let audio = AudioData::from_mono(samples, rate).downsample_half();
        let rms = (audio.samples().iter().map(|s| s * s).sum::<f32>()
            / audio.samples().len() as f32)
            .sqrt();
        assert!(
            rms < 0.15,
            "aliasing content should be attenuated: rms={rms}"
        );
    }

    #[test]
    fn own_wav_writer_round_trips_through_the_decoder() {
        let path = scratch("own-writer.wav");
        let sine: Vec<f32> = (0..8820).map(|i| (i as f32 * 0.05).sin() * 0.6).collect();
        let original = AudioData::from_mono(sine, 44_100);
        write_wav_mono16(&path, &original).unwrap();

        let decoded = decode_file(&path).unwrap();
        assert_eq!(decoded.sample_rate(), 44_100);
        assert_eq!(decoded.samples().len(), original.samples().len());
        for (a, b) in decoded.samples().iter().zip(original.samples()) {
            assert!((a - b).abs() < 0.001, "{a} vs {b}");
        }
    }

    #[test]
    fn low_rates_are_left_alone() {
        let audio = AudioData::from_mono(vec![0.5; 22_050], 22_050);
        let same = audio.clone().downsample_half();
        assert_eq!(same, audio);
    }
}

/// The genre tag of an audio file, if it carries one.
///
/// A metadata-only probe: the container is opened and its tags read,
/// no audio is decoded — cheap enough to run during a library scan.
/// Every failure mode is `None`; a missing tag must never make a
/// song fail to load.
#[must_use]
pub fn read_genre(path: &Path) -> Option<String> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTagKey};
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let stream = MediaSourceStream::new(
        Box::new(file),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let pick = |meta: &symphonia::core::meta::MetadataRevision| -> Option<String> {
        meta.tags()
            .iter()
            .find(|tag| tag.std_key == Some(StandardTagKey::Genre))
            .map(|tag| tag.value.to_string())
            .filter(|value| !value.trim().is_empty())
    };
    // Tags can live in the probe metadata (ID3 before the container)
    // or in the container itself (MP4 atoms, Vorbis comments).
    if let Some(meta) = probed.metadata.get()
        && let Some(current) = meta.current()
        && let Some(genre) = pick(current)
    {
        return Some(genre);
    }
    probed
        .format
        .metadata()
        .current()
        .and_then(pick)
        .map(|genre| genre.trim().to_owned())
}

#[cfg(test)]
mod genre_tests {
    use super::*;

    #[test]
    fn an_untagged_file_yields_none_not_an_error() {
        // The bundled fixture carries no genre tag; a missing tag is
        // an absence, never a failure.
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.ogg");
        assert_eq!(read_genre(&fixture), None);
    }

    #[test]
    fn a_missing_file_yields_none() {
        assert_eq!(read_genre(std::path::Path::new("/no/such/file.m4a")), None);
    }
}
