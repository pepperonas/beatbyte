//! `cargo run -p beatbyte-lyrics --features … --example voice -- <audio>`
//!
//! Where the acoustic model hears something speech-like, second by
//! second: one line per second with the model's probability of NOT
//! being silent (1 − p(blank)), averaged over that second's frames.
//!
//! With two more arguments — `voice <audio> <from> <to>` — it instead
//! prints what the model GREEDILY hears in that range: the most
//! probable letter per frame, collapsed. That is not a transcript
//! (the model is a character model on a full mix and it mishears
//! freely), but it answers "is this the chorus again?" when a forced
//! alignment has no text left to put there.
//!
//! This is the evidence behind the evaluation's diagnosis (a slide
//! through a long instrumental) and behind the "blank prior" idea in
//! `docs/lyrics/evaluation.md`: the model already knows where the
//! singing is, the aligner just does not use it yet. It is also the
//! honest way to ask of one song "does anybody sing here at all?"
//! without listening.

use std::sync::atomic::AtomicBool;

use beatbyte_lyrics::emissions::{FRAME_S, MODEL, SAMPLE_RATE, compute_with};
use beatbyte_lyrics::transcript::{BLANK, VOCAB};
use beatbyte_ml::{ModelStore, Runtime};

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: voice <audio>");
        std::process::exit(2);
    };
    let audio = match beatbyte_audio::decode_file(std::path::Path::new(&path)) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let store = ModelStore::default_location().expect("a config directory");
    let runtime = Runtime::new();
    let model = runtime
        .load(&store, &MODEL)
        .expect("the model is installed");
    let samples =
        beatbyte_audio::resample::resample(audio.samples(), audio.sample_rate(), SAMPLE_RATE);
    let emissions = compute_with(
        &runtime,
        &model,
        &samples,
        &mut |done, total| eprintln!("listening {done}/{total}"),
        &AtomicBool::new(false),
    )
    .expect("the model runs");
    // `voice <audio> <from> <to>`: what the model hears there.
    let range: Option<(f64, f64)> = match (std::env::args().nth(2), std::env::args().nth(3)) {
        (Some(from), Some(to)) => match (from.parse::<f64>(), to.parse::<f64>()) {
            (Ok(from), Ok(to)) if to > from => Some((from, to)),
            _ => {
                eprintln!("usage: voice <audio> [<from seconds> <to seconds>]");
                std::process::exit(2);
            }
        },
        _ => None,
    };
    if let Some((from, to)) = range {
        let frame = |seconds: f64| (seconds / FRAME_S) as usize;
        let (first, last) = (
            frame(from).min(emissions.frames),
            frame(to).min(emissions.frames),
        );
        let mut heard = String::new();
        let mut previous = usize::MAX;
        for f in first..last {
            let row = emissions.frame(f);
            let best = row
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(index, _)| index);
            // CTC collapse: repeats of the same symbol are one, and
            // the blank separates them.
            if best != previous && best != usize::from(BLANK) {
                heard.push_str(VOCAB[best]);
            }
            previous = best;
        }
        println!("{from:.1}-{to:.1} s: {}", heard.replace('|', " "));
        return;
    }
    let per_second = (1.0 / FRAME_S) as usize;
    for (second, frames) in emissions
        .log_probs
        .chunks(emissions.vocab * per_second)
        .enumerate()
    {
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for frame in frames.chunks(emissions.vocab) {
            sum += 1.0 - f64::from(frame[usize::from(BLANK)]).exp();
            count += 1;
        }
        if count > 0 {
            println!("{second} {:.4}", sum / count as f64);
        }
    }
}
