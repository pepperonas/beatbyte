//! The music-analysis pipeline: pure stages from samples to a
//! [`SongAnalysis`] (see `docs/audio/analysis.md`).

pub mod envelope;
pub mod melody;
pub mod onset;
pub mod tempo;

use beatbyte_core::music::SongAnalysis;

use crate::decode::AudioData;
use melody::MelodyConfig;
use onset::OnsetConfig;
use tempo::TempoConfig;

/// Configuration for the full analysis pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnalyzerConfig {
    /// Onset detection parameters.
    pub onset: OnsetConfig,
    /// Tempo estimation parameters.
    pub tempo: TempoConfig,
    /// Melody extraction parameters.
    pub melody: MelodyConfig,
}

/// An analysis implementation. The trait keeps the pipeline
/// replaceable (a future analyzer can use a different algorithm or an
/// external model without touching consumers).
pub trait Analyzer {
    /// Analyze decoded audio into musical events.
    fn analyze(&self, audio: &AudioData) -> SongAnalysis;
}

/// The default spectral-flux analyzer.
#[derive(Debug, Clone, Default)]
pub struct SpectralAnalyzer {
    /// Pipeline configuration.
    pub config: AnalyzerConfig,
}

/// Fallback tempo when a song carries no usable periodicity.
pub const FALLBACK_BPM: f64 = 120.0;

impl Analyzer for SpectralAnalyzer {
    fn analyze(&self, audio: &AudioData) -> SongAnalysis {
        // Analysis runs at ~22 kHz: identical musical information for
        // onset/tempo purposes at half the FFT cost.
        let prepared = if audio.sample_rate() >= 32_000 {
            audio.clone().downsample_half()
        } else {
            audio.clone()
        };
        let duration_s = prepared.duration_s();

        let flux = onset::analyze_onsets(&prepared, &self.config.onset);
        let estimate =
            tempo::estimate_tempo(&flux.flux, flux.hop_s, &flux.onsets, &self.config.tempo);

        let (bpm, bpm_confidence, alt_bpm) = match estimate {
            Some(t) => (t.bpm, t.confidence, t.alt_bpm),
            None => (FALLBACK_BPM, 0.0, None),
        };
        let (_, beats) = tempo::fit_beat_grid(&flux.onsets, bpm, duration_s);

        let melody = melody::extract_melody(&prepared, &self.config.melody);

        let energy_window = prepared.sample_rate() as usize / 10; // 100 ms
        let energy_hop = energy_window / 2; // 50 ms
        let energy = envelope::rms_envelope(&prepared, energy_window.max(1), energy_hop.max(1));
        let energy_hop_s = energy_hop as f64 / f64::from(prepared.sample_rate());

        SongAnalysis {
            bpm,
            bpm_confidence,
            alt_bpm,
            beats,
            onsets: flux.onsets,
            energy,
            energy_hop_s,
            duration_s,
            melody,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn full_pipeline_on_a_click_track() {
        // 128 BPM clicks for 20 s at 44.1 kHz (exercises downsampling).
        let period = 60.0 / 128.0;
        let times: Vec<f64> = (0..40).map(|i| 0.5 + i as f64 * period).collect();
        let audio = synth::click_track(&times, 20.0, 44_100);

        let analysis = SpectralAnalyzer::default().analyze(&audio);

        assert!(
            (analysis.bpm - 128.0).abs() < 2.5,
            "expected ~128 BPM, got {}",
            analysis.bpm
        );
        assert!(analysis.bpm_confidence > 0.2);
        assert!(!analysis.beats.is_empty());
        assert!(
            (analysis.onsets.len() as i64 - times.len() as i64).abs() <= 2,
            "expected ~{} onsets, got {}",
            times.len(),
            analysis.onsets.len()
        );
        assert!((analysis.duration_s - 20.0).abs() < 0.1);
        assert!(!analysis.energy.is_empty());
    }

    #[test]
    fn silence_falls_back_gracefully() {
        let audio = crate::decode::AudioData::from_mono(vec![0.0; 44_100 * 4], 44_100);
        let analysis = SpectralAnalyzer::default().analyze(&audio);
        assert_eq!(analysis.bpm, FALLBACK_BPM);
        assert_eq!(analysis.bpm_confidence, 0.0);
        assert!(analysis.onsets.is_empty());
    }
}
