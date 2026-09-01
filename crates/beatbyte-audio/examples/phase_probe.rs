//! Finding 1 of the Phase 1 baseline: measure the beat grid's
//! phase error against the demo song's known grid.
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
fn main() {
    let audio = beatbyte_audio::demo::render_demo_song();
    let a = SpectralAnalyzer::default().analyze(&audio);
    let beat = 60.0 / beatbyte_audio::demo::DEMO_BPM;
    println!("Wahrheit: Beat alle {:.4} s ab 0.000", beat);
    println!(
        "erkannt:  BPM {:.2}, erste Beats {:?}",
        a.bpm,
        a.beats
            .iter()
            .take(4)
            .map(|b| (b * 1000.0).round() / 1000.0)
            .collect::<Vec<_>>()
    );
    let phase = a.beats.first().copied().unwrap_or(0.0);
    let off = (phase / beat - (phase / beat).round()) * beat;
    println!(
        "Phasenversatz gegen das Raster: {:+.1} ms  (halber Beat = {:.1} ms)",
        off * 1000.0,
        beat * 500.0
    );
}
