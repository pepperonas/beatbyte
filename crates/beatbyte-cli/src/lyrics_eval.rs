//! `beatbyte-cli lyrics-eval` — the aligner measured against word-level
//! ground truth (plan milestone L5): AAE, PCO@0.1 and PCO@0.3 per
//! song, per language and over all, written as a JSON report that
//! the regression test reads (`BEATBYTE_LYRICS_EVAL_REPORT`).
//!
//! The corpus (JamendoLyrics MultiLang) stays outside the repository;
//! `--corpus` or `BEATBYTE_LYRICS_CORPUS` says where it is. Behind the
//! `ml` feature; needs the aligner model installed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use beatbyte_lyrics::align::{Anchoring, Options};
use beatbyte_lyrics::emissions::MODEL;
use beatbyte_lyrics::eval::{
    Aggregate, EvalOptions, JamendoCorpus, REPORT_SCHEMA, Report, SongScore, StampNoise, aggregate,
    by_language, evaluate_song,
};
use beatbyte_ml::{MlError, ModelStore, Runtime};

/// Run the evaluation and report.
#[allow(clippy::too_many_arguments)] // one flag per measured condition
pub fn run(
    corpus: Option<PathBuf>,
    out: Option<PathBuf>,
    limit: Option<usize>,
    language: Option<String>,
    raw: bool,
    anchors: bool,
    jitter_s: Option<f64>,
    shift_s: Option<f64>,
) -> ExitCode {
    let Some(root) = corpus.or_else(|| std::env::var_os("BEATBYTE_LYRICS_CORPUS").map(Into::into))
    else {
        eprintln!(
            "no corpus: pass --corpus <dir> or set BEATBYTE_LYRICS_CORPUS to a JamendoLyrics \
             MultiLang checkout"
        );
        return ExitCode::from(2);
    };
    let corpus = match JamendoCorpus::load(&root) {
        Ok(corpus) => corpus,
        Err(reason) => {
            eprintln!("{reason}");
            return ExitCode::from(2);
        }
    };
    for (name, why) in &corpus.skipped {
        eprintln!("skipping `{name}`: {why}");
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
    let songs: Vec<_> = corpus
        .songs
        .iter()
        .filter(|s| {
            language
                .as_ref()
                .is_none_or(|l| s.language.eq_ignore_ascii_case(l))
        })
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    if songs.is_empty() {
        eprintln!("no songs to evaluate in `{}`", root.display());
        return ExitCode::from(2);
    }
    let options = EvalOptions {
        gated: !raw,
        align: Options {
            anchoring: anchors.then(Anchoring::default),
        },
        stamps: anchors.then(|| StampNoise {
            shift_s: shift_s.unwrap_or(0.0),
            jitter_s: jitter_s.unwrap_or(0.0),
        }),
    };
    eprintln!(
        "evaluating {} song(s) from `{}` ({}{})…",
        songs.len(),
        root.display(),
        if raw { "raw aligner" } else { "gated" },
        if anchors {
            format!(
                ", line stamps shifted {:+.2} s jittered ±{:.2} s",
                shift_s.unwrap_or(0.0),
                jitter_s.unwrap_or(0.0)
            )
        } else {
            ", no line stamps".to_owned()
        }
    );
    let cancel = AtomicBool::new(false);
    let mut scores: Vec<SongScore> = Vec::new();
    for (index, song) in songs.iter().enumerate() {
        let started = std::time::Instant::now();
        match evaluate_song(song, &runtime, &model, &options, &cancel) {
            Ok(score) => {
                println!(
                    "{:>3}/{} {:<40} {:<8} AAE {:>6.3} s  PCO@0.1 {:>5.1}%  PCO@0.3 {:>5.1}%  \
                     cov {:>5.1}%  est {:>4.1}%  ({:.0?})",
                    index + 1,
                    songs.len(),
                    trim(&score.song, 40),
                    score.language,
                    score.aae_s,
                    score.pco_01 * 100.0,
                    score.pco_03 * 100.0,
                    score.coverage * 100.0,
                    score.estimated_rate * 100.0,
                    started.elapsed()
                );
                scores.push(score);
            }
            Err(reason) => eprintln!("{:>3}/{} {}: {reason}", index + 1, songs.len(), song.name),
        }
    }
    if scores.is_empty() {
        eprintln!("nothing could be evaluated");
        return ExitCode::from(1);
    }
    let by_lang = by_language(&scores);
    let all = aggregate(&scores);
    println!();
    for (lang, agg) in &by_lang {
        print_aggregate(lang, agg);
    }
    print_aggregate("ALL", &all);
    println!(
        "gates (plan §2): AAE < {:.2} s, PCO@0.3 > {:.2}, PCO@0.1 > {:.2} — {}",
        beatbyte_lyrics::eval::GATE_AAE_S,
        beatbyte_lyrics::eval::GATE_PCO_03,
        beatbyte_lyrics::eval::GATE_PCO_01,
        if all.passes_gates() { "PASS" } else { "FAIL" }
    );
    let report = Report {
        schema: REPORT_SCHEMA.to_owned(),
        aligner: format!(
            "{}@sha256:{} {}",
            model.id,
            model.sha256,
            beatbyte_ml::FINGERPRINT
        ),
        gated: !raw,
        songs: scores,
        by_language: by_lang,
        all,
    };
    if let Some(out) = out {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                if let Err(error) = std::fs::write(&out, json) {
                    eprintln!("cannot write `{}`: {error}", out.display());
                    return ExitCode::from(1);
                }
                println!("wrote {}", out.display());
            }
            Err(error) => {
                eprintln!("cannot serialise the report: {error}");
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}

/// `lyrics-check`: the alignment as a click track plus a word sheet.
pub fn check(audio_path: &Path, words: Option<PathBuf>, out: Option<PathBuf>) -> ExitCode {
    let words_path = words.unwrap_or_else(|| beatbyte_chart::lyrics::words_path(audio_path));
    let json = match std::fs::read_to_string(&words_path) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("cannot read `{}`: {error}", words_path.display());
            return ExitCode::from(2);
        }
    };
    let alignment = match beatbyte_lyrics::Alignment::from_json(&json) {
        Ok(alignment) => alignment,
        Err(error) => {
            eprintln!("`{}` is not an alignment: {error}", words_path.display());
            return ExitCode::from(2);
        }
    };
    let audio = match beatbyte_audio::decode_file(audio_path) {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let track = beatbyte_lyrics::check::check_track(&audio, &alignment);
    let out = out.unwrap_or_else(|| audio_path.with_extension("check.wav"));
    if let Err(error) = beatbyte_audio::decode::write_wav_mono16(&out, &track) {
        eprintln!("cannot write `{}`: {error}", out.display());
        return ExitCode::from(1);
    }
    print!("{}", beatbyte_lyrics::check::word_sheet(&alignment));
    println!("wrote {} — a click on every word", out.display());
    ExitCode::SUCCESS
}

fn print_aggregate(label: &str, agg: &Aggregate) {
    println!(
        "{:<8} {:>3} song(s)  AAE mean {:>7.3} s  median {:>6.3} s  lost {:>2}  PCO@0.1 {:>5.1}%  \
         PCO@0.3 {:>5.1}%  cov {:>5.1}%  est {:>4.1}%",
        label,
        agg.songs,
        agg.aae_s,
        agg.aae_median_s,
        agg.derailed,
        agg.pco_01 * 100.0,
        agg.pco_03 * 100.0,
        agg.coverage * 100.0,
        agg.estimated_rate * 100.0
    );
}

fn trim(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(width - 1).collect();
        format!("{cut}…")
    }
}
