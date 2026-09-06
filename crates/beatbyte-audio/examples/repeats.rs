//! Print the repeated sections the structure stage finds in a song,
//! on the analyzer's own beat grid.
//!
//! ```text
//! cargo run --release -p beatbyte-audio --example repeats -- <audio> [<audio>…]
//! ```

use beatbyte_audio::analysis::structure::{StructureConfig, find_repeats};
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};

fn clock(t: f64) -> String {
    format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0)
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(audio) = beatbyte_audio::decode_file(std::path::Path::new(&path)) else {
            eprintln!("cannot decode {path}");
            continue;
        };
        let analysis = SpectralAnalyzer::default().analyze(&audio);
        let prepared = if audio.sample_rate() >= 32_000 {
            audio.downsample_half()
        } else {
            audio
        };
        let started = std::time::Instant::now();
        let repeats = find_repeats(
            &prepared,
            &analysis.beats,
            &analysis.downbeats,
            &StructureConfig::default(),
        );
        let covered: usize = repeats.iter().map(|r| 2 * r.beats).sum();
        println!(
            "{path}: {} beats, {} repeats covering {:.0} % ({:.1} s)",
            analysis.beats.len(),
            repeats.len(),
            100.0 * covered as f64 / analysis.beats.len().max(1) as f64,
            started.elapsed().as_secs_f64()
        );
        for r in &repeats {
            let t = |i: usize| {
                analysis
                    .beats
                    .get(i)
                    .copied()
                    .unwrap_or(analysis.duration_s)
            };
            println!(
                "  {} – {}  ==  {} – {}   ({} beats, {:.2})",
                clock(t(r.first_beat)),
                clock(t(r.first_beat + r.beats)),
                clock(t(r.second_beat)),
                clock(t(r.second_beat + r.beats)),
                r.beats,
                r.similarity
            );
        }
    }
}
