//! Is the grid error a constant offset or an offbeat lock? Measure
//! the first-beat phase residual of every real track.
//!
//! ```text
//! cargo run -p beatbyte-audio --example phase_real -- <anlz-root> <audio-root>
//! ```

use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::corpus;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(anlz_root), Some(audio_root)) = (args.next(), args.next()) else {
        eprintln!("usage: phase_real <anlz-root> <audio-root>");
        return;
    };

    println!("| Track | Periode ms | Versatz ms | in Beats |");
    println!("|---|---|---|---|");
    let analyzer = SpectralAnalyzer::default();
    for track in corpus::pair(&anlz_root, &audio_root, corpus::Profile::loop_house()) {
        let Ok(decoded) = beatbyte_audio::decode_file(&track.audio) else {
            continue;
        };
        let analysis = analyzer.analyze(&decoded);
        let (Some(&detected), Some(&reference)) =
            (analysis.beats.first(), track.truth.beats.first())
        else {
            continue;
        };
        let period = 60.0 / track.truth.bpm;
        // Fold into ±half a period: what matters is which part of the
        // beat the grid sits on, not how many beats in it started.
        let raw = detected - reference;
        let offset = (raw / period - (raw / period).round()) * period;
        println!(
            "| {} | {:.1} | {:+.0} | {:+.3} |",
            track.name.chars().take(30).collect::<String>(),
            period * 1000.0,
            offset * 1000.0,
            offset / period
        );
    }
}
