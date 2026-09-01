//! Which tracker settings actually help? Sweep them over the real
//! corpus rather than guessing from one track.
//!
//! ```text
//! cargo run -p beatbyte-audio --example sweep_real -- <anlz-root> <audio-root>
//! ```

use beatbyte_audio::analysis::beats::GridMode;
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, corpus};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(anlz_root), Some(audio_root)) = (args.next(), args.next()) else {
        eprintln!("usage: sweep_real <anlz-root> <audio-root>");
        return;
    };
    let tracks = corpus::pair(&anlz_root, &audio_root, corpus::Profile::loop_house());
    // Decode once; the grid mode is the only thing that varies.
    let decoded: Vec<_> = tracks
        .iter()
        .filter_map(|t| {
            beatbyte_audio::decode_file(&t.audio)
                .ok()
                .map(|audio| (t, audio))
        })
        .collect();

    println!("| Kick-Gewicht | Steifigkeit | Beat-F Mittel | Beat-F Median | schlechtester |");
    println!("|---|---|---|---|---|");
    for weight in [0.0f32, 0.5, 0.75, 1.0] {
        for tightness in [100.0f64, 400.0] {
            let mut analyzer = SpectralAnalyzer::default();
            analyzer.config.grid.mode = GridMode::Tracked;
            analyzer.config.grid.low_band_weight = weight;
            analyzer.config.grid.tightness = tightness;
            let mut scores: Vec<f64> = decoded
                .iter()
                .map(|(track, audio)| eval::evaluate(&analyzer.analyze(audio), &track.truth).beat_f)
                .collect();
            let mean = scores.iter().sum::<f64>() / scores.len().max(1) as f64;
            scores.sort_by(f64::total_cmp);
            println!(
                "| {weight:.2} | {tightness:.0} | {mean:.3} | {:.3} | {:.3} |",
                scores[scores.len() / 2],
                scores.first().copied().unwrap_or(0.0)
            );
        }
    }
}
