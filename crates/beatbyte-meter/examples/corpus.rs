//! The C1 A/B on real tracks with a DJ's accepted grids: the
//! analyzer's tracked grid against the Beat This! models, and the
//! two ways a model could enter the chart.
//!
//! ```text
//! cargo run --release -p beatbyte-meter --example corpus -- <anlz-root> <audio-root> [--all] [--full-only]
//! ```
//!
//! The models must be installed (`beatbyte-cli models install
//! beat-this-mel`, `… beat-this-small`, `… beat-this`). `--all` widens
//! the corpus from the loop-house profile to every paired track;
//! `--full-only` skips the small model. Paths because the corpus is
//! local and stays local.

use std::path::PathBuf;

use beatbyte_audio::analysis::{Analyzer, SpectralAnalyzer};
use beatbyte_audio::eval::{self, corpus};
use beatbyte_meter::{Policy, Size, merge, track};
use beatbyte_ml::{ModelStore, Runtime};

fn main() {
    let mut all = false;
    let mut full_only = false;
    let paths: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter(|a| match a.as_str() {
            "--all" => {
                all = true;
                false
            }
            "--full-only" => {
                full_only = true;
                false
            }
            _ => true,
        })
        .map(PathBuf::from)
        .collect();
    let (Some(anlz_root), Some(audio_root)) = (paths.first(), paths.get(1)) else {
        eprintln!("usage: corpus <anlz-root> <audio-root> [--all] [--full-only]");
        return;
    };
    let Some(store) = ModelStore::default_location() else {
        eprintln!("no config directory");
        return;
    };
    let runtime = Runtime::new();
    let profile = if all {
        corpus::Profile {
            bpm: (20.0, 400.0),
            min_len_s: 30.0,
        }
    } else {
        corpus::Profile::loop_house()
    };
    let tracks = corpus::pair(anlz_root, audio_root, profile);
    let analyzer = SpectralAnalyzer::default();

    // Columns: F = beat F-measure, C = CMLt, A = AMLt, D = downbeat F.
    // "bars in fours" is what the chart does without a meter; "ours
    // BPM"/"full BPM" are the tracker's and the full model's tempo
    // readings, so a level disagreement shows as a ratio.
    println!(
        "| Track | BPM | ours BPM | full BPM | ours F | ours C | ours A | bars-in-fours D | \
         small F | small C | small A | small D | full F | full C | full A | full D | \
         ours+full-downbeats D | s/song small | s/song full |"
    );
    println!(
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"
    );
    let mut sums = [0.0f64; 13];
    let mut counted = 0usize;
    for track_ in &tracks {
        let Ok(decoded) = beatbyte_audio::decode_file(&track_.audio) else {
            eprintln!("nicht dekodierbar: {}", track_.name);
            continue;
        };
        let ours = analyzer.analyze(&decoded);
        let ours_scores = eval::evaluate(&ours, &track_.truth);
        let mut fours = ours.clone();
        fours.downbeats = ours.beats.iter().copied().step_by(4).collect();
        let fours_scores = eval::evaluate(&fours, &track_.truth);

        let mut row = Vec::new();
        let mut hybrid_d = 0.0;
        let mut full_bpm = 0.0;
        let mut secs = [0.0f64; 2];
        for (i, size) in [Size::Small, Size::Full].into_iter().enumerate() {
            if full_only && size == Size::Small {
                row.push((0.0, 0.0, 0.0, 0.0));
                continue;
            }
            let started = std::time::Instant::now();
            let meter = match track(&runtime, &store, size, &decoded) {
                Ok(m) => m,
                Err(error) => {
                    eprintln!("{}: {size:?}: {error}", track_.name);
                    return;
                }
            };
            secs[i] = started.elapsed().as_secs_f64();
            let mut grid = ours.clone();
            merge(&mut grid, &meter, Policy::Grid);
            let s = eval::evaluate(&grid, &track_.truth);
            row.push((s.beat_f, s.cmlt, s.amlt, s.downbeat_f));
            if size == Size::Full {
                full_bpm = grid.bpm;
                let mut hybrid = ours.clone();
                merge(&mut hybrid, &meter, Policy::Downbeats);
                hybrid_d = eval::evaluate(&hybrid, &track_.truth).downbeat_f;
            }
        }
        let values = [
            ours_scores.beat_f,
            ours_scores.cmlt,
            ours_scores.amlt,
            fours_scores.downbeat_f,
            row[0].0,
            row[0].1,
            row[0].2,
            row[0].3,
            row[1].0,
            row[1].1,
            row[1].2,
            row[1].3,
            hybrid_d,
        ];
        for (sum, v) in sums.iter_mut().zip(values) {
            *sum += v;
        }
        counted += 1;
        eprintln!(
            "{}: small {:.1} s, full {:.1} s",
            track_.name, secs[0], secs[1]
        );
        print!(
            "| {} | {:.1} | {:.2} | {:.2} |",
            track_.name.chars().take(28).collect::<String>(),
            track_.truth.bpm,
            ours.bpm,
            full_bpm
        );
        for v in &values {
            print!(" {v:.3} |");
        }
        println!(" {:.1} | {:.1} |", secs[0], secs[1]);
    }
    if counted > 0 {
        print!("| **mean of {counted}** | | | |");
        for v in &sums {
            print!(" **{:.3}** |", v / counted as f64);
        }
        println!(" | |");
    }
    eprintln!("{counted} tracks measured");
}
