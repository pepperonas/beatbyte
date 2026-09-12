//! Reproducible one-song experiment, never a library rollout.
//!
//! Usage: `cargo run -p beatbyte-cli --example guitar_study -- <active-chart>
//! <lead-wav> <new-output-directory>`. The source must start at decoded song
//! sample zero and span the full song. A duration check cannot prove alignment;
//! prepare it from `beatbyte-cli decode`, never from a different recording.

use std::{error::Error, path::PathBuf};

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::{
    AudioTrim, ChartFile, GenerateMeta, Severity, chart_hash, generate::generate_lead_study,
    generate_chart,
};
use clap::Parser;
use serde_json::json;

#[derive(Parser)]
struct Args {
    /// Exact active chart to compare (resolve any version pointer first).
    chart: PathBuf,
    /// Full-length, aligned instrument WAV; `other` is not guitar isolation.
    lead: PathBuf,
    /// New directory only; existing charts and pointers are never overwritten.
    out: PathBuf,
}

fn valid(chart: &ChartFile) -> Result<(), Box<dyn Error>> {
    let errors: Vec<_> = chart
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; ").into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args.out.exists() {
        return Err("output directory already exists; choose a new study folder".into());
    }
    let mut active = beatbyte_chart::load_chart_file(&args.chart)?;
    valid(&active)?;
    let folder = args.chart.parent().ok_or("chart has no folder")?;
    let audio_path = beatbyte_chart::resolve_audio_path(folder, &active.song.audio)?;
    let audio = decode_file(&audio_path)?;
    let source = decode_file(&args.lead)?;
    if (source.duration_s() - audio.duration_s()).abs() > 0.025 {
        return Err(
            "instrument source duration differs by more than 25 ms; use the decoded song timeline"
                .into(),
        );
    }
    let p = audio.priming();
    active.retime(AudioTrim::declared(
        p.samples,
        p.timescale,
        audio.sample_rate(),
    ));
    eprintln!("reading full mix...");
    let mut mix = SpectralAnalyzer::default().analyze(&audio);
    // Lock all variants to the accepted grid: compare source/mapping, not tempo.
    mix.bpm = active.song.bpm;
    if let Some(grid) = &active.grid {
        mix.beats = grid.beats.clone();
        mix.downbeats = grid.downbeats.clone();
    } else {
        let step = 60.0 / active.song.bpm;
        mix.beats = (0..)
            .map(|i| active.song.offset_s + i as f64 * step)
            .take_while(|t| *t < audio.duration_s())
            .filter(|t| *t >= 0.0)
            .collect();
        mix.downbeats.clear();
    }
    // Repeat indices belong to the old analyzer grid and must not be reused.
    mix.repeats.clear();
    eprintln!("reading instrument source...");
    let mut lead = SpectralAnalyzer::default().analyze(&source);
    lead.bpm = mix.bpm;
    lead.beats = mix.beats.clone();
    lead.downbeats = mix.downbeats.clone();
    lead.duration_s = mix.duration_s;
    lead.repeats.clear();
    let meta = GenerateMeta {
        title: active.song.title.clone(),
        artist: active.song.artist.clone(),
        audio: "mix.wav".into(),
    };
    let variants = [
        ("mix-default", generate_chart(&mix, &meta)),
        ("mix-study", generate_lead_study(&mix, &meta)),
        ("lead-default", generate_chart(&lead, &meta)),
        ("lead-study", generate_lead_study(&lead, &meta)),
    ];
    if variants[3].1.charts.iter().any(|c| c.notes.is_empty()) {
        return Err(
            "source has too little tonal evidence for a playable four-difficulty pilot".into(),
        );
    }
    for (_, chart) in &variants {
        valid(chart)?;
    }
    // Creating a NEW folder is the only write boundary, after validation.
    std::fs::create_dir(&args.out)?;
    beatbyte_audio::write_wav_mono16(&args.out.join("mix.wav"), &audio)?;
    beatbyte_chart::save_chart_file(&args.out.join("original.json"), &active)?;
    std::fs::write(
        args.out.join("mix-analysis.json"),
        serde_json::to_vec_pretty(&mix)?,
    )?;
    std::fs::write(
        args.out.join("lead-analysis.json"),
        serde_json::to_vec_pretty(&lead)?,
    )?;
    let mut rows = Vec::new();
    for (label, mut chart) in variants {
        chart.audio_trim = Some(AudioTrim::declared(0, 1, audio.sample_rate()));
        for def in &chart.charts {
            let repeated = def
                .notes
                .windows(2)
                .filter(|p| p[0].lane == p[1].lane && p[1].time - p[0].time < 0.18)
                .count();
            let row = json!({"variant": label, "difficulty": def.difficulty.id(), "notes": def.notes.len(),
                "sustains": def.notes.iter().filter(|n| n.len > 0.0).count(), "hopos": def.notes.iter().filter(|n| n.hopo).count(),
                "fast_same_fret_pairs": repeated});
            println!("{row}");
            rows.push(row);
        }
        beatbyte_chart::save_chart_file(&args.out.join(format!("{label}.json")), &chart)?;
    }
    std::fs::write(
        args.out.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "parent_hash": chart_hash(&active), "chart": args.chart, "source": args.lead,
            "generator_version": beatbyte_chart::VERSION,
            "warning": "Experimental mono contour; source identity, pitch correctness and guitar feel require listening. No polyphonic chord transcription. No rollout.",
            "mix_melody_notes": mix.melody.len(), "lead_melody_notes": lead.melody.len(), "variants": rows
        }))?,
    )?;
    Ok(())
}
