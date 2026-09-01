//! Does one global tempo hold phase over a 6-8 minute track?
//!
//! ```text
//! cargo run -p beatbyte-audio --example drift_real -- <anlz-root> <audio-root>
//! ```

use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::corpus;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(anlz_root), Some(audio_root)) = (args.next(), args.next()) else {
        eprintln!("usage: drift_real <anlz-root> <audio-root>");
        return;
    };

    // ⚠️ The two residual columns are nearest-beat distances and wrap
    // at half a period, so they must not be subtracted to obtain a
    // drift. The last column is derived from the tempo error, which
    // has no wrap — that is the figure the report cites.
    println!(
        "| Track | Länge | BPM-Fehler % | Rest 1. Min ms | Rest letzte Min ms | Drift (aus BPM) ms |"
    );
    println!("|---|---|---|---|---|---|");
    let analyzer = SpectralAnalyzer::default();
    for track in corpus::pair(&anlz_root, &audio_root, corpus::Profile::loop_house()) {
        let Ok(decoded) = beatbyte_audio::decode_file(&track.audio) else {
            continue;
        };
        let analysis = analyzer.analyze(&decoded);
        let last = track.truth.beats.last().copied().unwrap_or(0.0);
        let mean = |from: f64, to: f64| -> f64 {
            let residuals: Vec<f64> = analysis
                .beats
                .iter()
                .copied()
                .filter(|&t| t >= from && t < to)
                .filter_map(|t| corpus::residual(t, &track.truth.beats))
                .collect();
            if residuals.is_empty() {
                f64::NAN
            } else {
                residuals.iter().sum::<f64>() / residuals.len() as f64
            }
        };
        println!(
            "| {} | {last:.0}s | {:+.3} | {:+.0} | {:+.0} | {:.0} |",
            track.name.chars().take(28).collect::<String>(),
            (analysis.bpm - track.truth.bpm) / track.truth.bpm * 100.0,
            mean(0.0, 60.0) * 1000.0,
            mean(last - 60.0, last) * 1000.0,
            corpus::accumulated_drift_s(analysis.bpm, track.truth.bpm, last) * 1000.0
        );
    }
}
