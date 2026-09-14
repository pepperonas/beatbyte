//! `beatbyte-cli chart-check …` — the song with a click on every
//! chart note, so a chart's RHYTHM can be judged by ear in one pass,
//! plus the two numbers that do not need an ear.
//!
//! The question this answers came from a playtest: "the guitar study
//! is much more fun, but the rhythm may not be optimal — or is there
//! a delay?". Playing it is a slow and noisy way to find out, and the
//! telemetry of a run only records what the PLAYER did. A click track
//! asks the chart directly: a click that lands on the music is a note
//! that lands on the music, and a click in a gap is a note nobody can
//! hear coming.
//!
//! IO and printing here; the marking is [`beatbyte_audio::mark`] and
//! the statistics are [`beatbyte_chart::fit`], both pure and tested.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file, mark};
use beatbyte_chart::ChartFile;
use beatbyte_chart::fit::{ATTACK_TOLERANCE_S, measure};
use beatbyte_core::Difficulty;

/// Render and measure one difficulty of one chart against its song.
pub fn run(
    song: &Path,
    chart_path: &Path,
    difficulty: Difficulty,
    from: Option<f64>,
    secs: Option<f64>,
    out: Option<PathBuf>,
) -> ExitCode {
    let text = match std::fs::read_to_string(chart_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", chart_path.display());
            return ExitCode::from(2);
        }
    };
    let chart: ChartFile = match serde_json::from_str(&text) {
        Ok(chart) => chart,
        Err(error) => {
            eprintln!("cannot parse `{}`: {error}", chart_path.display());
            return ExitCode::from(2);
        }
    };
    let Some(def) = chart.charts.iter().find(|c| c.difficulty == difficulty) else {
        eprintln!("`{}` carries no {difficulty:?} chart", chart_path.display());
        return ExitCode::from(2);
    };

    let audio = match decode_file(song) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    // The window: a slice is what makes two variants comparable in
    // half a minute. Without `--from` the chart's own preview anchor
    // is the musically representative moment the generator already
    // picked; without that, the song from its start.
    let start = from
        .or_else(|| secs.and(chart.song.preview_start_s))
        .unwrap_or(0.0)
        .max(0.0);
    let end = secs.map_or(audio.duration_s(), |s| (start + s).min(audio.duration_s()));
    if end <= start {
        eprintln!("the window {start:.1}–{end:.1} s is empty");
        return ExitCode::from(2);
    }

    eprintln!(
        "analyzing {:.0} s of audio at {} Hz…",
        audio.duration_s(),
        audio.sample_rate()
    );
    let mut analysis = SpectralAnalyzer::default().analyze(&audio);
    crate::meter(&mut analysis, &audio);

    let fit = measure(
        &def.notes,
        chart.grid.as_ref(),
        &analysis.onsets,
        start,
        end,
    );

    // The click track carries the window only — a 30 s file is
    // something you listen to twice, a six-minute one is not.
    let rate = audio.sample_rate();
    let first = (start * f64::from(rate)) as usize;
    let last = ((end * f64::from(rate)) as usize).min(audio.samples().len());
    let window_samples = &audio.samples()[first.min(last)..last];
    let times: Vec<f64> = def
        .notes
        .iter()
        .filter(|n| n.time >= start && n.time < end)
        .map(|n| n.time - start)
        .collect();
    let clicked = beatbyte_audio::decode::AudioData::from_mono(
        mark::clicks_over(window_samples, rate, &times),
        rate,
    );
    let out = out.unwrap_or_else(|| chart_path.with_extension("chart-check.wav"));
    if let Err(error) = beatbyte_audio::decode::write_wav_mono16(&out, &clicked) {
        eprintln!("cannot write `{}`: {error}", out.display());
        return ExitCode::from(1);
    }

    println!(
        "Chart check `{}` — {}",
        chart.song.title,
        difficulty_name(difficulty)
    );
    println!("  window        {start:>8.1} – {end:.1} s");
    println!(
        "  notes         {:>8}   ({:.2}/s, {:.0} % sustains)",
        fit.notes,
        fit.notes_per_s,
        fit.sustain_share * 100.0
    );
    match fit.on_grid {
        Some(share) => println!("  on the grid   {:>7.0} %", share * 100.0),
        None => println!("  on the grid          —   (this chart carries no grid)"),
    }
    println!(
        "  with an attack{:>7.0} %   (a detected onset within {:.0} ms)",
        fit.with_attack * 100.0,
        ATTACK_TOLERANCE_S * 1000.0
    );
    match fit.median_offset_s {
        Some(offset) => println!(
            "  median offset {:>+8.1} ms  (positive = charted later than the attack)",
            offset * 1000.0
        ),
        None => println!("  median offset        —   (no note had an onset near it)"),
    }
    println!(
        "wrote {} — a click beside the music is a note beside the music",
        out.display()
    );
    ExitCode::SUCCESS
}

/// The difficulty's name the way the chart format spells it.
fn difficulty_name(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_difficulty_prints_under_its_own_name() {
        // The name is what a person reads in the table and types into
        // `--difficulty`; a mismatch between the two is a trap.
        for difficulty in Difficulty::ALL {
            let name = difficulty_name(difficulty);
            assert_eq!(
                crate::parse_difficulty(name),
                Some(difficulty),
                "`{name}` must parse back to what printed it"
            );
        }
    }
}
