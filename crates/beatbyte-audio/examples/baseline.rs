//! Phase 1 baseline: run the current pipeline over every case with
//! known ground truth and print the table.
use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, GroundTruth};

fn row(class: &str, name: &str, property: &str, s: &eval::Scores, truth_bpm: f64) {
    let octave = if eval::is_octave_error(s.bpm, truth_bpm) {
        " OKTAVFEHLER"
    } else {
        ""
    };
    println!(
        "| {class} | {name} | {property} | {:.3} | {:.3} | {:.3} | {:.0} | {:.1} | {:.1} | {:.1}{octave} |",
        s.beat_f,
        s.cmlt,
        s.amlt,
        s.downbeat_accuracy,
        s.notes_per_s_median,
        s.notes_per_s_p95,
        s.bpm
    );
}

fn main() {
    println!("| Klasse | Fall | Eig. | Beat-F | CMLt | AMLt | DB | N/s med | N/s p95 | BPM |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let analyzer = SpectralAnalyzer::default();

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
        let scores = eval::evaluate(&analyzer.analyze(&audio), &truth);
        row("rock", name, "-", &scores, bpm);
    }

    // house-sample/ — the synthetic problem class.
    for case in eval::synthetic::house_sample_class() {
        let scores = eval::evaluate(&analyzer.analyze(&case.audio), &case.truth);
        row(
            "house-sample",
            case.name,
            case.property,
            &scores,
            case.truth.bpm,
        );
    }
}
