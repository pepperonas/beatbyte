//! RMS energy envelope — drives sustain generation and stage effects.

use crate::decode::AudioData;

/// Compute a normalized RMS envelope: one value per `hop` samples,
/// each over a `window`-sample span, scaled so the loudest point is 1.0.
#[must_use]
pub fn rms_envelope(audio: &AudioData, window: usize, hop: usize) -> Vec<f32> {
    let samples = audio.samples();
    if samples.is_empty() || window == 0 || hop == 0 || samples.len() < window {
        return Vec::new();
    }
    let frames = (samples.len() - window) / hop + 1;
    let mut envelope = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame * hop;
        let sum: f32 = samples[start..start + window].iter().map(|s| s * s).sum();
        envelope.push((sum / window as f32).sqrt());
    }
    let max = envelope.iter().copied().fold(0.0f32, f32::max);
    if max > 1e-9 {
        for value in &mut envelope {
            *value /= max;
        }
    }
    envelope
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_tracks_loudness() {
        // 1 s silence, 1 s tone, 1 s silence at 1 kHz rate.
        let mut samples = vec![0.0f32; 3000];
        for (i, sample) in samples[1000..2000].iter_mut().enumerate() {
            *sample = (i as f32 * 0.5).sin() * 0.8;
        }
        let audio = AudioData::from_mono(samples, 1000);
        let envelope = rms_envelope(&audio, 100, 100);

        assert!(envelope[2] < 0.01, "silence should be quiet");
        assert!(envelope[15] > 0.9, "tone should be loud");
        assert!(envelope[27] < 0.01, "tail should be quiet");
    }

    #[test]
    fn degenerate_input_yields_empty_envelope() {
        let audio = AudioData::from_mono(vec![], 1000);
        assert!(rms_envelope(&audio, 100, 100).is_empty());
        let audio = AudioData::from_mono(vec![0.0; 10], 1000);
        assert!(rms_envelope(&audio, 100, 100).is_empty());
    }
}
