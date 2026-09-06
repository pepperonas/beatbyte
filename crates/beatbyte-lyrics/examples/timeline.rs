//! `cargo run --release -p beatbyte-lyrics --example timeline -- <song> <stem>…`
//!
//! Is a separator's output on the song's timeline? The stems are
//! summed back into a mix (a two-stem separation returns `vocals` +
//! `no_vocals` = its input, so the sum IS the tool's decode of the
//! song), both are brought to the aligner's 16 kHz through the same
//! resampler the aligner uses, and the lag that best explains the
//! song from the sum is measured over the loudest twenty seconds by
//! direct cross-correlation, ±150 ms.
//!
//! Prints the lag in milliseconds and the correlation peak's height
//! relative to the next-best lag outside ±5 ms. A lag of one
//! container priming (23 ms for FFmpeg's AAC at 44.1 kHz, 48 ms for
//! Apple's; 21 ms at 48 kHz) says the tool decoded the container
//! without skipping it — the L0 lesson, and the reason `beatbyte-cli
//! decode` exists. Not a model call: no ML feature needed beyond the
//! crate's own.

use beatbyte_lyrics::emissions::SAMPLE_RATE;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: timeline <song> <stem> [<stem>…]");
        std::process::exit(2);
    }
    let decode = |path: &str| -> Vec<f32> {
        let audio = beatbyte_audio::decode_file(std::path::Path::new(path)).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2);
        });
        beatbyte_audio::resample::resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE)
    };
    let song = decode(&args[0]);
    let mut sum: Vec<f32> = Vec::new();
    for stem in &args[1..] {
        let s = decode(stem);
        if sum.is_empty() {
            sum = s;
        } else {
            for (a, b) in sum.iter_mut().zip(&s) {
                *a += *b;
            }
        }
    }
    let rate = SAMPLE_RATE as usize;
    let window = 20 * rate;
    let max_lag = (rate * 150) / 1000; // ±150 ms
    let usable = song.len().min(sum.len());
    if usable < window + 2 * max_lag + 1 {
        eprintln!("too short to measure");
        std::process::exit(2);
    }
    // The loudest twenty seconds of the song, in whole seconds, kept
    // clear of the edges by the lag range.
    let mut best_start = max_lag;
    let mut best_energy = -1.0f64;
    let mut start = max_lag;
    while start + window + max_lag <= usable {
        let energy: f64 = song[start..start + window]
            .iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum();
        if energy > best_energy {
            best_energy = energy;
            best_start = start;
        }
        start += rate;
    }
    let seg = &song[best_start..best_start + window];
    let mut scores: Vec<(i64, f64)> = Vec::with_capacity(2 * max_lag + 1);
    for lag in -(max_lag as i64)..=(max_lag as i64) {
        // song[t] against sum[t - lag]: a positive lag means the sum
        // (the tool's decode) runs EARLY by that much — it carries
        // audio the song's decode skipped.
        let from = (best_start as i64 - lag) as usize;
        let other = &sum[from..from + window];
        let dot: f64 = seg
            .iter()
            .zip(other)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        scores.push((lag, dot));
    }
    let (peak_lag, peak) = scores
        .iter()
        .copied()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("scores");
    let guard = (rate * 5) / 1000; // ±5 ms
    let runner_up = scores
        .iter()
        .filter(|(lag, _)| (lag - peak_lag).unsigned_abs() as usize > guard)
        .map(|(_, v)| *v)
        .fold(f64::NEG_INFINITY, f64::max);
    println!(
        "lag {:+.2} ms ({peak_lag:+} samples at {rate} Hz) over {:.0}–{:.0} s; peak/runner-up {:.2}",
        peak_lag as f64 * 1000.0 / rate as f64,
        best_start as f64 / rate as f64,
        (best_start + window) as f64 / rate as f64,
        if runner_up > 0.0 {
            peak / runner_up
        } else {
            f64::INFINITY
        }
    );
}
