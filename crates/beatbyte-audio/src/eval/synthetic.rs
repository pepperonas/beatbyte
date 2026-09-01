//! Synthetic tracks that reproduce the material properties which
//! break the current pipeline — with ground truth by construction.
//!
//! These do not replace real music, and the report says so. What
//! they give is a regression floor that exists TODAY, without
//! waiting for an annotated corpus: each case isolates one property
//! from `docs/audio-pipeline-ist.md` so a fix can be attributed to
//! the thing it fixed rather than to the corpus average.
//!
//! `// ASSUMPTION:` marks every number taken from the description of
//! the material rather than measured from a real file.

use crate::decode::AudioData;
use crate::eval::GroundTruth;
use crate::synth;

/// One synthetic case: audio plus the grid it was built from.
pub struct Case {
    /// Short name for the report table.
    pub name: &'static str,
    /// Which material property it isolates (a–g).
    pub property: &'static str,
    /// The rendered audio.
    pub audio: AudioData,
    /// Exact ground truth — these times ARE where the hits were put.
    pub truth: GroundTruth,
}

/// Analysis sample rate for the cases. Matches what a decoded file
/// is downsampled to, so the harness measures the real path.
const RATE: u32 = 44_100;
/// Bars per case. Long enough for autocorrelation to see the period
/// several times, short enough that the suite stays quick.
const BARS: usize = 32;

/// Beat times for a steady 4/4 at `bpm`, starting at `first`.
fn beats(bpm: f64, first: f64, count: usize) -> Vec<f64> {
    let period = 60.0 / bpm;
    (0..count).map(|i| first + i as f64 * period).collect()
}

/// **Property f** — a flat four-to-the-floor: every beat identical,
/// no accent, no fill. The tempo-octave and downbeat trap.
#[must_use]
pub fn flat_four_on_the_floor(bpm: f64) -> Case {
    let count = BARS * 4;
    let times = beats(bpm, 1.0, count);
    let duration = times.last().copied().unwrap_or(0.0) + 2.0;
    let mut samples = vec![0.0f32; (duration * f64::from(RATE)) as usize];
    for &t in &times {
        // ASSUMPTION: a house kick reads as a short low thud; 60 Hz
        // for 90 ms is the shape, not a measurement of any record.
        synth::add_burst(&mut samples, RATE, t, 60.0, 0.09, 0.9);
    }
    Case {
        name: "flat-4x4",
        property: "f",
        audio: AudioData::from_mono(samples, RATE),
        truth: GroundTruth::steady(bpm, 1.0, count),
    }
}

/// **Property a** — two overlaid timing rasters: a quantised machine
/// layer and a sampled live layer that lags. The bimodal-residual
/// case.
#[must_use]
pub fn two_rasters(bpm: f64, lag_s: f64) -> Case {
    let count = BARS * 4;
    let times = beats(bpm, 1.0, count);
    let duration = times.last().copied().unwrap_or(0.0) + 2.0;
    let mut samples = vec![0.0f32; (duration * f64::from(RATE)) as usize];
    for (index, &t) in times.iter().enumerate() {
        // The programmed layer sits exactly on the grid.
        synth::add_burst(&mut samples, RATE, t, 60.0, 0.09, 0.9);
        // ASSUMPTION: the sampled layer lags by a roughly constant
        // amount with a little jitter — the description gives
        // ±10–40 ms, not a distribution. A deterministic wobble
        // keeps the case reproducible.
        let wobble = ((index as f64) * 0.7).sin() * 0.006;
        synth::add_burst(&mut samples, RATE, t + lag_s + wobble, 220.0, 0.12, 0.55);
    }
    Case {
        name: "two-rasters",
        property: "a",
        audio: AudioData::from_mono(samples, RATE),
        // The GRID is the programmed layer: that is what a DJ would
        // beatmatch to, and what the chart must land on.
        truth: GroundTruth::steady(bpm, 1.0, count),
    }
}

/// **Property b** — soft transients: slow attacks under a sustained
/// bed, the shape tape and bus compression leave behind.
#[must_use]
pub fn soft_transients(bpm: f64) -> Case {
    let count = BARS * 4;
    let times = beats(bpm, 1.0, count);
    let duration = times.last().copied().unwrap_or(0.0) + 2.0;
    let mut samples = vec![0.0f32; (duration * f64::from(RATE)) as usize];
    // A sustained loop body the hits have to cut through.
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / f64::from(RATE);
        *slot += (0.18 * (2.0 * std::f64::consts::PI * 110.0 * t).sin()) as f32;
    }
    for &t in &times {
        // ASSUMPTION: "soft" is modelled as a long, quiet burst —
        // the flux step is small but the event is real.
        synth::add_burst(&mut samples, RATE, t, 90.0, 0.25, 0.30);
    }
    Case {
        name: "soft-transients",
        property: "b",
        audio: AudioData::from_mono(samples, RATE),
        truth: GroundTruth::steady(bpm, 1.0, count),
    }
}

/// **Property d** — a filter sweep over the loop: the perceptually
/// largest event, which produces no onset and lifts the flux
/// baseline while it runs.
#[must_use]
pub fn filter_sweep(bpm: f64) -> Case {
    let count = BARS * 4;
    let times = beats(bpm, 1.0, count);
    let duration = times.last().copied().unwrap_or(0.0) + 2.0;
    let mut samples = vec![0.0f32; (duration * f64::from(RATE)) as usize];
    let bar = 4.0 * 60.0 / bpm;
    // ASSUMPTION: the sweep runs 16 bars and opens a bright partial
    // in — a real filter is a resonant lowpass, this is its audible
    // consequence, not its transfer function.
    let sweep_start = 8.0 * bar;
    let sweep_end = 24.0 * bar;
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / f64::from(RATE);
        let open = ((t - sweep_start) / (sweep_end - sweep_start)).clamp(0.0, 1.0);
        let body = 0.15 * (2.0 * std::f64::consts::PI * 110.0 * t).sin();
        let bright = 0.15 * open * (2.0 * std::f64::consts::PI * 2_400.0 * t).sin();
        *slot += (body + bright) as f32;
    }
    for &t in &times {
        synth::add_burst(&mut samples, RATE, t, 60.0, 0.09, 0.8);
    }
    let mut truth = GroundTruth::steady(bpm, 1.0, count);
    // The moment the filter finishes opening IS the drop — the
    // boundary a chart should mark.
    truth.boundaries = vec![sweep_start, sweep_end];
    Case {
        name: "filter-sweep",
        property: "d",
        audio: AudioData::from_mono(samples, RATE),
        truth,
    }
}

/// The whole synthetic class, at the reference tempo.
///
/// 125 BPM is the reference track's tempo (*Anoa*), and it sits
/// right where the octave trap is: 62.5 and 250 are both inside the
/// pipeline's 60–200 search window.
#[must_use]
pub fn house_sample_class() -> Vec<Case> {
    vec![
        flat_four_on_the_floor(125.0),
        two_rasters(125.0, 0.022),
        soft_transients(125.0),
        filter_sweep(125.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_case_carries_a_grid_that_matches_its_audio() {
        for case in house_sample_class() {
            assert!(!case.truth.beats.is_empty(), "{}", case.name);
            assert!(
                (case.truth.bpm - 125.0).abs() < 1e-9,
                "{} drifted off the reference tempo",
                case.name
            );
            // The audio must actually be long enough to hold the
            // grid it claims — a truth that runs past the file would
            // make every recall score a lie.
            let last = case.truth.beats.last().copied().unwrap_or(0.0);
            assert!(
                case.audio.duration_s() >= last,
                "{}: grid ends at {last}, audio at {}",
                case.name,
                case.audio.duration_s()
            );
        }
    }

    #[test]
    fn the_two_raster_case_really_carries_two_layers() {
        // The fixture must be verified against the AUDIO, not
        // against the pipeline: the pipeline's answer is the thing
        // under measurement.
        //
        // (The first version of this test asked the onset stage and
        // failed at 128 onsets for 128 beats — which is not a broken
        // fixture but the defect itself: `min_gap_s = 0.05` in
        // onset.rs discards the second layer 22 ms later. That is
        // property (a) from docs/audio-pipeline-ist.md reproducing
        // on demand, and it belongs in the baseline table, not in a
        // red test.)
        let case = two_rasters(125.0, 0.022);
        let samples = case.audio.samples();
        let rate = f64::from(case.audio.sample_rate());
        // Count energy peaks directly: two bursts per beat means two
        // rises within the first 60 ms after each grid position.
        let energy_at = |t: f64| -> f32 {
            let start = (t * rate) as usize;
            let end = ((t + 0.01) * rate) as usize;
            samples
                .get(start..end.min(samples.len()))
                .map_or(0.0, |w| w.iter().map(|v| v.abs()).fold(0.0, f32::max))
        };
        let beat = case.truth.beats[8];
        assert!(energy_at(beat) > 0.1, "the programmed layer is there");
        assert!(
            energy_at(beat + 0.022) > 0.1,
            "the sampled layer 22 ms later is there too"
        );
        assert!(
            energy_at(beat + 0.2) < energy_at(beat),
            "and the gap between beats is quieter than the hits"
        );
    }

    #[test]
    fn the_sweep_case_marks_the_drop_as_a_boundary() {
        let case = filter_sweep(125.0);
        assert_eq!(case.truth.boundaries.len(), 2, "sweep start and end");
        assert!(case.truth.boundaries[1] > case.truth.boundaries[0]);
    }
}
