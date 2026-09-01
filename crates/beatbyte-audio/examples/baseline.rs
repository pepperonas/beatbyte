//! Phase 1 baseline: run the current pipeline over every case with
//! known ground truth and print the table.
use beatbyte_audio::analysis::beats::GridMode;
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, GroundTruth};

fn row(class: &str, name: &str, property: &str, before: &eval::Scores, after: &eval::Scores) {
    println!(
        "| {class} | {name} | {property} | {:.3} | **{:.3}** | {:.3} | {:.3} | {:.3} | {:.3} | {:.1} | {:.1} |",
        before.beat_f,
        after.beat_f,
        before.cmlt,
        after.cmlt,
        before.amlt,
        after.amlt,
        before.notes_per_s_median,
        after.notes_per_s_median
    );
}

fn main() {
    println!(
        "| Klasse | Fall | Eig. | F starr | F verfolgt | CMLt starr | CMLt verf. | AMLt starr | AMLt verf. | N/s starr | N/s verf. |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let rigid = SpectralAnalyzer::default();
    let mut tracked = SpectralAnalyzer::default();
    tracked.config.grid.mode = GridMode::Tracked;
    tracked.config.grid.low_band_weight = 1.0;

    // rock/ — the built-in songs: rendered from a known BPM, so the
    // grid is exact by construction.
    for (audio, bpm, name) in [
        (
            beatbyte_audio::demo::render_demo_song(),
            beatbyte_audio::demo::DEMO_BPM,
            "circuit-breaker",
        ),
        (
            beatbyte_audio::demo::render_groove_song(),
            beatbyte_audio::demo::GROOVE_BPM,
            "solder-groove",
        ),
    ] {
        let duration = audio.duration_s();
        let beats = (duration / (60.0 / bpm)) as usize;
        let truth = GroundTruth::steady(bpm, 0.0, beats);
        row(
            "rock",
            name,
            "-",
            &eval::evaluate(&rigid.analyze(&audio), &truth),
            &eval::evaluate(&tracked.analyze(&audio), &truth),
        );
    }

    // house-sample/ — the synthetic problem class.
    for case in eval::synthetic::house_sample_class() {
        row(
            "house-sample",
            case.name,
            case.property,
            &eval::evaluate(&rigid.analyze(&case.audio), &case.truth),
            &eval::evaluate(&tracked.analyze(&case.audio), &case.truth),
        );
    }
}
