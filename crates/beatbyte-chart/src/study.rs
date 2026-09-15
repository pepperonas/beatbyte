//! The `[Guitar Study]` twin of a song folder.
//!
//! The study charts ONE instrument source (a separated stem) instead
//! of the whole mix — fewer, melodically grounded notes — and the
//! player picked it "by a distance" over the mix-charted version. A
//! twin is that chart in a folder of its own beside the original: the
//! same audio and sidecars, one `chart.json` whose title carries the
//! prefix, so both appear in the browser and play against each other
//! in a live session. The original — its versions, its pointer, its
//! telemetry — is never written to.
//!
//! This crate knows nothing of audio: decoding and analysis are
//! injected as a reader, exactly as [`crate::redesign`] takes them,
//! so the command line and the game share one writer.

use std::path::{Path, PathBuf};

use crate::generate::generate_lead_study;
use crate::redesign::Reading;
use crate::schema::Provenance;
use crate::{
    ChartFile, GenerateMeta, Severity, chart_hash, load_chart_file, save_chart_file, versions,
};

/// The title prefix the browser shows the twin under.
pub const TITLE_PREFIX: &str = "[Guitar Study] ";

/// Who the provenance names.
pub const DESIGNER: &str = "lead-study";

/// How far the stem's length may differ from the song's. A length
/// check cannot prove alignment; a different length disproves it.
pub const LENGTH_TOLERANCE_S: f64 = 0.025;

/// What a twin-writing run came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Written: the folder, the title and the note count per difficulty.
    Written {
        /// Where the twin lives.
        folder: PathBuf,
        /// Its title, prefix included.
        title: String,
        /// `(difficulty id, notes)` in chart order.
        notes: Vec<(String, usize)>,
    },
    /// A twin was already there; nothing written.
    AlreadyThere(PathBuf),
    /// The stem carries too little tonal evidence for a playable
    /// four-difficulty chart; the original stays the only version.
    Refused(String),
}

/// The folder the twin lives in, derived from the original's folder
/// name so a second run finds it. Pure — tested.
#[must_use]
pub fn twin_folder_name(song_folder_name: &str) -> String {
    format!("guitar-study-{song_folder_name}")
}

/// Where the twin of `song_folder` lives (or would).
#[must_use]
pub fn twin_folder_for(song_folder: &Path) -> Option<PathBuf> {
    let name = song_folder.file_name()?.to_string_lossy().into_owned();
    Some(song_folder.parent()?.join(twin_folder_name(&name)))
}

/// Whether a folder name is a twin's.
#[must_use]
pub fn is_twin_folder(name: &str) -> bool {
    name.starts_with("guitar-study-")
}

/// The twin's title: prefixed once, never twice. Pure — tested.
#[must_use]
pub fn twin_title(title: &str) -> String {
    if title.starts_with(TITLE_PREFIX) {
        title.to_owned()
    } else {
        format!("{TITLE_PREFIX}{title}")
    }
}

/// The original's title behind a twin's, or `None` for a title that
/// is not a twin's. What the browser keys the pairing on. Pure — tested.
#[must_use]
pub fn base_title(title: &str) -> Option<&str> {
    title.strip_prefix(TITLE_PREFIX)
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

fn errors_of(chart: &ChartFile) -> Vec<String> {
    chart
        .validate()
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.to_string())
        .collect()
}

/// Write the twin of `song_folder` from the instrument stem at `lead`.
///
/// `read` decodes and analyzes a file — the song's audio (resolved
/// from the active chart) and the stem alike. The stem has to be the
/// DECODED song timeline, frame for frame: prepare it from the decoded
/// audio, never from a different decoder's reading of the same file.
pub fn write_twin(
    song_folder: &Path,
    lead: &Path,
    read: &dyn Fn(&Path) -> Result<Reading, String>,
) -> Result<Outcome, String> {
    let out = twin_folder_for(song_folder).ok_or("the song folder needs a name and a parent")?;
    if out.exists() {
        return Ok(Outcome::AlreadyThere(out));
    }
    let names: Vec<String> = std::fs::read_dir(song_folder)
        .map_err(|error| format!("cannot read {}: {error}", song_folder.display()))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let pointer = std::fs::read_to_string(song_folder.join(versions::POINTER_FILE)).ok();
    let active_name = versions::resolve_active(pointer.as_deref(), &names);
    let active = load_chart_file(&song_folder.join(&active_name))
        .map_err(|error| format!("cannot load {active_name}: {error}"))?;
    let problems = errors_of(&active);
    if !problems.is_empty() {
        return Err(format!("{active_name} is invalid: {}", problems.join("; ")));
    }
    let parent_hash = chart_hash(&active);
    let audio_path = crate::resolve_audio_path(song_folder, &active.song.audio)
        .map_err(|error| format!("cannot resolve the audio: {error}"))?;
    let mix_reading = read(&audio_path)?;
    let lead_reading = read(lead)?;
    let (song_s, stem_s) = (
        mix_reading.analysis.duration_s,
        lead_reading.analysis.duration_s,
    );
    if (stem_s - song_s).abs() > LENGTH_TOLERANCE_S {
        return Err(format!(
            "the stem is {stem_s:.3}s, the song {song_s:.3}s — prepare the stem from the decoded song"
        ));
    }

    // The accepted grid, not a fresh tempo reading: the study changes
    // WHICH notes, never where the beat is.
    let mut mix = mix_reading.analysis;
    mix.bpm = active.song.bpm;
    if let Some(grid) = &active.grid {
        mix.beats = grid.beats.clone();
        mix.downbeats = grid.downbeats.clone();
    } else {
        let step = 60.0 / active.song.bpm;
        mix.beats = (0..)
            .map(|i| active.song.offset_s + f64::from(i) * step)
            .take_while(|t| *t < song_s)
            .filter(|t| *t >= 0.0)
            .collect();
        mix.downbeats.clear();
    }
    let mut lead_analysis = lead_reading.analysis;
    lead_analysis.bpm = mix.bpm;
    lead_analysis.beats = mix.beats;
    lead_analysis.downbeats = mix.downbeats;
    lead_analysis.duration_s = mix.duration_s;
    lead_analysis.repeats.clear();

    let meta = GenerateMeta {
        title: twin_title(&active.song.title),
        artist: active.song.artist.clone(),
        audio: active.song.audio.clone(),
    };
    let mut chart = generate_lead_study(&lead_analysis, &meta);
    if chart.charts.iter().any(|c| c.notes.is_empty()) {
        return Ok(Outcome::Refused(format!(
            "{}: the stem carries too little tonal evidence for a playable four-difficulty chart",
            active.song.title
        )));
    }
    // The analysis ran on the decoded timeline; the chart says so with
    // the same declaration the original carries, so playback skips the
    // frames the analysis skipped.
    chart.audio_trim = Some(mix_reading.trim);
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
        return Err(format!(
            "the study chart is invalid: {}",
            problems.join("; ")
        ));
    }

    // The only write boundary: a NEW folder. Audio and sidecars first,
    // the chart LAST, so a library scan that happens mid-write never
    // sees a chart without its audio.
    std::fs::create_dir(&out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    for name in names.iter().filter(|n| !is_chart_file(n)) {
        std::fs::copy(song_folder.join(name), out.join(name))
            .map_err(|error| format!("cannot copy {name}: {error}"))?;
    }
    save_chart_file(&out.join(versions::BASE_CHART), &chart)
        .map_err(|error| format!("cannot write the study chart: {error}"))?;
    Ok(Outcome::Written {
        folder: out,
        title: chart.song.title.clone(),
        notes: chart
            .charts
            .iter()
            .map(|c| (c.difficulty.id().to_owned(), c.notes.len()))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twin_folder_is_derived_from_the_original() {
        assert_eq!(
            twin_folder_name("toto---africa-m4a"),
            "guitar-study-toto---africa-m4a"
        );
        assert_eq!(
            twin_folder_for(Path::new("songs/imported/toto---africa-m4a")),
            Some(PathBuf::from(
                "songs/imported/guitar-study-toto---africa-m4a"
            ))
        );
        assert!(is_twin_folder("guitar-study-toto---africa-m4a"));
        assert!(!is_twin_folder("toto---africa-m4a"));
    }

    #[test]
    fn a_twin_names_its_original_and_an_original_names_nothing() {
        assert_eq!(base_title("[Guitar Study] Africa"), Some("Africa"));
        assert_eq!(base_title("Africa"), None);
        // The prefix must be the title's start, not just somewhere in it.
        assert_eq!(base_title("Africa [Guitar Study] "), None);
    }

    #[test]
    fn the_title_is_prefixed_exactly_once() {
        assert_eq!(twin_title("Africa"), "[Guitar Study] Africa");
        // A twin of a twin would read "[Guitar Study] [Guitar Study] …".
        assert_eq!(twin_title("[Guitar Study] Africa"), "[Guitar Study] Africa");
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

    #[test]
    fn a_folder_that_already_has_its_twin_is_left_alone() {
        // The reader must never be asked: an existing twin ends the run
        // before any decoding.
        let dir = std::env::temp_dir().join(format!("bb-twin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("song")).expect("a temp song folder");
        std::fs::create_dir_all(dir.join("guitar-study-song")).expect("a temp twin folder");
        let outcome = write_twin(&dir.join("song"), Path::new("/nowhere.wav"), &|_| {
            Err("the reader was called".to_owned())
        });
        assert_eq!(
            outcome,
            Ok(Outcome::AlreadyThere(dir.join("guitar-study-song")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
