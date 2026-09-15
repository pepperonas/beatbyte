//! `beatbyte-cli study`: the `[Guitar Study]` twin of a song folder.
//!
//! The study charts one instrument source (a separated stem) instead
//! of the whole mix — fewer, melodically grounded notes — and it was
//! the version the player picked "by a distance" over the mix-charted
//! one. This command builds that twin for a song WITHOUT touching the
//! original: a new folder beside it, the same audio and sidecars, one
//! `chart.json` whose title carries the prefix, so both appear in the
//! browser and can be played against each other in a live session.
//! The pointer, the versions and the telemetry of the original stay
//! exactly what they were.
//!
//! The one-song experiment this grew out of is
//! `examples/guitar_study.rs`; the timeline rules are its rules: the
//! stem must lie on the DECODED song timeline (priming skipped), and
//! the written chart declares the original's `audio_trim` so playback
//! skips the same frames the analysis did.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_audio::{Analyzer, SpectralAnalyzer, decode_file};
use beatbyte_chart::schema::Provenance;
use beatbyte_chart::{
    AudioTrim, ChartFile, GenerateMeta, Severity, chart_hash, generate::generate_lead_study,
    load_chart_file, resolve_audio_path, save_chart_file, versions,
};

/// The title prefix the browser shows the twin under.
pub const TITLE_PREFIX: &str = "[Guitar Study] ";

/// Who the provenance names.
pub const DESIGNER: &str = "lead-study";

/// The folder the twin lives in, derived from the original's folder
/// name so a second run finds it and a browser sort keeps the pair
/// apart from the hand-made experiments. Pure — tested.
#[must_use]
pub fn study_folder_name(song_folder_name: &str) -> String {
    format!("guitar-study-{song_folder_name}")
}

/// The twin's title: prefixed once, never twice. Pure — tested.
#[must_use]
pub fn study_title(title: &str) -> String {
    if title.starts_with(TITLE_PREFIX) {
        title.to_owned()
    } else {
        format!("{TITLE_PREFIX}{title}")
    }
}

/// Whether a file in the song folder is a chart or the pointer — the
/// things the twin must NOT copy (it gets exactly one chart of its
/// own); everything else (audio, lyrics, loudness) comes along.
/// Pure — tested.
#[must_use]
pub fn is_chart_file(name: &str) -> bool {
    name == versions::BASE_CHART
        || name == versions::POINTER_FILE
        || versions::is_version_file(name)
}

fn fail(message: impl std::fmt::Display, code: u8) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(code)
}

fn errors_of(chart: &ChartFile) -> Vec<String> {
    chart
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect()
}

/// Build the twin of `song_folder` from the instrument stem at
/// `lead`. Exit 0 when written or already present, 2 on a bad input,
/// 3 when the source carries too little tonal evidence for a playable
/// four-difficulty chart (the original is then the only version, and
/// the report says so).
pub fn run(song_folder: &Path, lead: &Path) -> ExitCode {
    let Some(out) = study_folder_for(song_folder) else {
        return fail("the song folder needs a name and a parent", 2);
    };
    if out.exists() {
        println!("already there: {}", out.display());
        return ExitCode::SUCCESS;
    }
    let names: Vec<String> = match std::fs::read_dir(song_folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(error) => return fail(format!("cannot read {}: {error}", song_folder.display()), 2),
    };
    let pointer = std::fs::read_to_string(song_folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active = match load_chart_file(&song_folder.join(&active_name)) {
        Ok(chart) => chart,
        Err(error) => return fail(format!("cannot load {active_name}: {error}"), 2),
    };
    let problems = errors_of(&active);
    if !problems.is_empty() {
        return fail(
            format!("{active_name} is invalid: {}", problems.join("; ")),
            2,
        );
    }
    let parent_hash = chart_hash(&active);
    let audio_path = match resolve_audio_path(song_folder, &active.song.audio) {
        Ok(path) => path,
        Err(error) => return fail(format!("cannot resolve the audio: {error}"), 2),
    };
    let audio = match decode_file(&audio_path) {
        Ok(audio) => audio,
        Err(error) => {
            return fail(
                format!("cannot decode {}: {error}", audio_path.display()),
                2,
            );
        }
    };
    let source = match decode_file(lead) {
        Ok(audio) => audio,
        Err(error) => {
            return fail(
                format!("cannot decode the stem {}: {error}", lead.display()),
                2,
            );
        }
    };
    // The stem has to be the decoded song's timeline, frame for frame;
    // a length check cannot prove alignment, but a different length
    // disproves it.
    if (source.duration_s() - audio.duration_s()).abs() > 0.025 {
        return fail(
            format!(
                "the stem is {:.3}s, the song {:.3}s — prepare the stem from `beatbyte-cli decode`",
                source.duration_s(),
                audio.duration_s()
            ),
            2,
        );
    }
    let priming = audio.priming();
    let trim = AudioTrim::declared(priming.samples, priming.timescale, audio.sample_rate());

    eprintln!("reading the mix...");
    let mut mix = SpectralAnalyzer::default().analyze(&audio);
    // The accepted grid, not a fresh tempo reading: the study changes
    // WHICH notes, never where the beat is.
    mix.bpm = active.song.bpm;
    if let Some(grid) = &active.grid {
        mix.beats = grid.beats.clone();
        mix.downbeats = grid.downbeats.clone();
    } else {
        let step = 60.0 / active.song.bpm;
        mix.beats = (0..)
            .map(|i| active.song.offset_s + f64::from(i) * step)
            .take_while(|t| *t < audio.duration_s())
            .filter(|t| *t >= 0.0)
            .collect();
        mix.downbeats.clear();
    }
    mix.repeats.clear();
    eprintln!("reading the stem...");
    let mut lead_analysis = SpectralAnalyzer::default().analyze(&source);
    lead_analysis.bpm = mix.bpm;
    lead_analysis.beats = mix.beats.clone();
    lead_analysis.downbeats = mix.downbeats.clone();
    lead_analysis.duration_s = mix.duration_s;
    lead_analysis.repeats.clear();

    let meta = GenerateMeta {
        title: study_title(&active.song.title),
        artist: active.song.artist.clone(),
        audio: active.song.audio.clone(),
    };
    let mut chart = generate_lead_study(&lead_analysis, &meta);
    if chart.charts.iter().any(|c| c.notes.is_empty()) {
        return fail(
            format!(
                "{}: the stem carries too little tonal evidence for a playable four-difficulty chart — the original stays the only version",
                active.song.title
            ),
            3,
        );
    }
    chart.audio_trim = Some(trim);
    chart.provenance = Some(Provenance {
        parent_hash: parent_hash.clone(),
        designer: DESIGNER.to_owned(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        directive: Some("guitar-study".to_owned()),
    });
    let problems = errors_of(&chart);
    if !problems.is_empty() {
        return fail(
            format!("the study chart is invalid: {}", problems.join("; ")),
            2,
        );
    }

    // The only write boundary: a NEW folder. Audio and sidecars first,
    // the chart LAST, so a library scan that happens mid-write never
    // sees a chart without its audio.
    if let Err(error) = std::fs::create_dir(&out) {
        return fail(format!("cannot create {}: {error}", out.display()), 2);
    }
    for name in names.iter().filter(|n| !is_chart_file(n)) {
        if let Err(error) = std::fs::copy(song_folder.join(name), out.join(name)) {
            return fail(format!("cannot copy {name}: {error}"), 2);
        }
    }
    if let Err(error) = save_chart_file(&out.join(versions::BASE_CHART), &chart) {
        return fail(format!("cannot write the study chart: {error}"), 2);
    }
    let counts: Vec<String> = chart
        .charts
        .iter()
        .map(|c| format!("{} {}", c.difficulty.id(), c.notes.len()))
        .collect();
    println!(
        "wrote {} — {} (parent {}) notes: {}",
        out.display(),
        chart.song.title,
        &parent_hash[..8.min(parent_hash.len())],
        counts.join(", ")
    );
    ExitCode::SUCCESS
}

/// Where the twin of `song_folder` would live.
#[must_use]
pub fn study_folder_for(song_folder: &Path) -> Option<PathBuf> {
    let name = song_folder.file_name()?.to_string_lossy().into_owned();
    Some(song_folder.parent()?.join(study_folder_name(&name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twin_folder_is_derived_from_the_original() {
        assert_eq!(
            study_folder_name("toto---africa-m4a"),
            "guitar-study-toto---africa-m4a"
        );
        assert_eq!(
            study_folder_for(Path::new("songs/imported/toto---africa-m4a")),
            Some(PathBuf::from(
                "songs/imported/guitar-study-toto---africa-m4a"
            ))
        );
    }

    #[test]
    fn the_title_is_prefixed_exactly_once() {
        assert_eq!(study_title("Africa"), "[Guitar Study] Africa");
        // A twin of a twin would read "[Guitar Study] [Guitar Study] …".
        assert_eq!(
            study_title("[Guitar Study] Africa"),
            "[Guitar Study] Africa"
        );
    }

    #[test]
    fn charts_and_the_pointer_stay_behind_everything_else_comes_along() {
        for chart in ["chart.json", "chart.v7.json", "chart-active.json"] {
            assert!(is_chart_file(chart), "{chart} must not be copied");
        }
        for asset in [
            "Toto - Africa.m4a",
            "Toto - Africa.lrc",
            "Toto - Africa.words.json",
            "Toto - Africa.loudness.json",
            "chart.v7.json.bak",
        ] {
            assert!(!is_chart_file(asset), "{asset} belongs to the twin too");
        }
    }
}
