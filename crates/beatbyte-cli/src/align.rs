//! `beatbyte-cli align <audio> <lyrics>` — word- and letter-level
//! timing for known lyrics, written as `<audio>.words.json` beside the
//! audio (plan milestone L2). Behind the `ml` feature; needs the
//! aligner model installed (`models install wav2vec2-base-960h`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_lyrics::emissions::MODEL;
use beatbyte_lyrics::{Transcript, align};
use beatbyte_ml::{MlError, ModelStore, Runtime};

/// Where the alignment goes when `--out` is not given.
#[must_use]
pub fn default_output(audio: &Path) -> PathBuf {
    let stem = audio
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "song".to_owned());
    audio.with_file_name(format!("{stem}.words.json"))
}

/// Run the alignment and report.
pub fn run(audio_path: &Path, lyrics_path: &Path, out: Option<PathBuf>) -> ExitCode {
    let lyrics = match std::fs::read_to_string(lyrics_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", lyrics_path.display());
            return ExitCode::from(2);
        }
    };
    let transcript = Transcript::parse(&lyrics);
    if transcript.alignable_words() == 0 {
        eprintln!(
            "`{}` holds no words the model has letters for",
            lyrics_path.display()
        );
        return ExitCode::from(2);
    }
    let Some(store) = ModelStore::default_location() else {
        eprintln!("no config directory on this platform; models cannot be stored");
        return ExitCode::from(2);
    };
    let runtime = Runtime::new();
    let model = match runtime.load(&store, &MODEL) {
        Ok(model) => model,
        Err(MlError::NotInstalled { id }) => {
            eprintln!(
                "model `{id}` is not installed — run `beatbyte-cli models install {id}` first"
            );
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    let audio = match beatbyte_audio::decode_file(audio_path) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let audio_sha256 = match beatbyte_ml::hash::sha256_file(audio_path) {
        Ok(hash) => hash,
        Err(error) => {
            eprintln!("cannot hash `{}`: {error}", audio_path.display());
            return ExitCode::from(2);
        }
    };
    let text_source = format!(
        "file:{}",
        lyrics_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    eprintln!(
        "aligning {} words over {:.0} s of audio…",
        transcript.alignable_words(),
        audio.duration_s()
    );
    let started = std::time::Instant::now();
    let outcome = match align(
        &audio,
        &audio_sha256,
        &transcript,
        &text_source,
        &runtime,
        &model,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    let took = started.elapsed();
    let out_path = out.unwrap_or_else(|| default_output(audio_path));
    let json = match outcome.alignment.to_json() {
        Ok(json) => json,
        Err(error) => {
            eprintln!("cannot serialise: {error}");
            return ExitCode::from(1);
        }
    };
    if let Err(error) = std::fs::write(&out_path, json) {
        eprintln!("cannot write `{}`: {error}", out_path.display());
        return ExitCode::from(1);
    }
    let s = &outcome.stats;
    println!("wrote {}", out_path.display());
    println!(
        "  {} words ({} estimated), mean confidence {:.2}, {} under {:.1}; {} frames in {:.1?}",
        s.words,
        s.estimated,
        s.mean_conf,
        s.uncertain,
        beatbyte_lyrics::align::UNCERTAIN_BELOW,
        s.frames,
        took
    );
    if let Some((lines, median, mad)) = s.source_line_delta {
        println!(
            "  against the source's {lines} line stamps: aligned − source median {median:+.3} s, \
             spread (MAD) {mad:.3} s"
        );
    }
    ExitCode::SUCCESS
}
