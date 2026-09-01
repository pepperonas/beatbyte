//! Phase 1 baseline against REAL tracks: Rekordbox's own grids paired
//! with the audio they were analysed from.
//!
//! ```text
//! cargo run -p beatbyte-audio --example baseline_real -- <anlz-root> <audio-root>
//! ```
//!
//! Takes paths because the corpus is local and stays local — no
//! audio and no grid is checked into this repository.

use beatbyte_audio::analysis::beats::GridMode;
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, corpus};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(anlz_root), Some(audio_root)) = (args.next(), args.next()) else {
        eprintln!("usage: baseline_real <anlz-root> <audio-root>");
        return;
    };

    // Both grid modes on the same decode, so the comparison cannot be
    // confounded by anything but the mode itself.
    // "after" is literally what ships; "before" asks for the old grid
    // explicitly, so the comparison is against the shipped default
    // rather than against a hand-built configuration.
    let mut rigid = SpectralAnalyzer::default();
    rigid.config.grid.mode = GridMode::ConstantTempo;
    let tracked = SpectralAnalyzer::default();

    println!(
        "| Fall | BPM-Ref | Beat-F starr | Beat-F verfolgt | CMLt starr | CMLt verfolgt | N/s starr | N/s verfolgt |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    let tracks = corpus::pair(&anlz_root, &audio_root, corpus::Profile::loop_house());
    let (mut sum_rigid, mut sum_tracked, mut counted) = (0.0, 0.0, 0usize);
    for track in &tracks {
        let Ok(decoded) = beatbyte_audio::decode_file(&track.audio) else {
            eprintln!("nicht dekodierbar: {}", track.name);
            continue;
        };
        let before = eval::evaluate(&rigid.analyze(&decoded), &track.truth);
        let started = std::time::Instant::now();
        let after = eval::evaluate(&tracked.analyze(&decoded), &track.truth);
        let elapsed = started.elapsed().as_secs_f64();
        let length = track.truth.beats.last().copied().unwrap_or(0.0);
        eprintln!(
            "{}: {length:.0} s Musik in {elapsed:.1} s analysiert",
            track.name
        );
        sum_rigid += before.beat_f;
        sum_tracked += after.beat_f;
        counted += 1;
        println!(
            "| {} | {:.2} | {:.3} | **{:.3}** | {:.3} | {:.3} | {:.1} | {:.1} |",
            track.name.chars().take(30).collect::<String>(),
            track.truth.bpm,
            before.beat_f,
            after.beat_f,
            before.cmlt,
            after.cmlt,
            before.notes_per_s_median,
            after.notes_per_s_median
        );
    }
    if counted > 0 {
        println!(
            "| **Mittel** | | {:.3} | **{:.3}** | | | | |",
            sum_rigid / counted as f64,
            sum_tracked / counted as f64
        );
    }
    eprintln!("{counted} Tracks gemessen");
}
