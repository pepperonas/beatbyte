//! Phase 1 baseline against REAL tracks: Rekordbox's own grids paired
//! with the audio they were analysed from.
//!
//! ```text
//! cargo run -p beatbyte-audio --example baseline_real -- <anlz-root> <audio-root>
//! ```
//!
//! Takes paths because the corpus is local and stays local — no
//! audio and no grid is checked into this repository.

use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, corpus};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(anlz_root), Some(audio_root)) = (args.next(), args.next()) else {
        eprintln!("usage: baseline_real <anlz-root> <audio-root>");
        return;
    };

    println!("| Fall | BPM-Ref | BPM-ist | Beat-F | CMLt | AMLt | DB | N/s med | N/s p95 |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let analyzer = SpectralAnalyzer::default();
    let tracks = corpus::pair(&anlz_root, &audio_root, corpus::Profile::loop_house());
    for track in &tracks {
        let Ok(decoded) = beatbyte_audio::decode_file(&track.audio) else {
            eprintln!("nicht dekodierbar: {}", track.name);
            continue;
        };
        let scores = eval::evaluate(&analyzer.analyze(&decoded), &track.truth);
        let octave = if eval::is_octave_error(scores.bpm, track.truth.bpm) {
            " OKT!"
        } else {
            ""
        };
        println!(
            "| {} | {:.2} | {:.2}{octave} | {:.3} | {:.3} | {:.3} | {:.0} | {:.1} | {:.1} |",
            track.name.chars().take(34).collect::<String>(),
            track.truth.bpm,
            scores.bpm,
            scores.beat_f,
            scores.cmlt,
            scores.amlt,
            scores.downbeat_accuracy,
            scores.notes_per_s_median,
            scores.notes_per_s_p95
        );
    }
    eprintln!("{} Tracks gemessen", tracks.len());
}
