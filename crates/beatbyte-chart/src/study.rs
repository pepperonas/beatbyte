//! The `[GS]` (Guitar Study) twin of a song folder.
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
use crate::twin;
use crate::{
    ChartFile, GenerateMeta, Severity, chart_hash, load_chart_file, save_chart_file, versions,
};

/// The title prefix the browser shows the twin under.
pub const TITLE_PREFIX: &str = twin::Kind::Study.title_prefix();

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
    format!("{}{song_folder_name}", twin::Kind::Study.folder_prefix())
}

/// Where the twin of `song_folder` lives (or would).
#[must_use]
pub fn twin_folder_for(song_folder: &Path) -> Option<PathBuf> {
    twin::folder_for(song_folder, twin::Kind::Study)
}

/// The twin's title: prefixed once, never twice. Pure — tested.
#[must_use]
pub fn twin_title(title: &str) -> String {
    twin::titled(title, twin::Kind::Study)
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
    if twin::is_finished(&out) {
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
    std::fs::create_dir_all(&out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    twin::copy_assets(song_folder, &out, &names)?;
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

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir().join(format!("bb-twin-{tag}-{unique}"));
            std::fs::create_dir_all(&dir).expect("scratch");
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
    }

    #[test]
    fn the_title_is_prefixed_exactly_once() {
        assert_eq!(twin_title("Africa"), "[GS] Africa");
        // A twin of a twin would read "[GS] [GS] …".
        assert_eq!(twin_title("[GS] Africa"), "[GS] Africa");
    }

    #[test]
    fn a_folder_that_already_has_its_twin_is_left_alone() {
        // The reader must never be asked: a FINISHED twin ends the
        // run before any decoding.
        let scratch = Scratch::new("already");
        let twin = scratch.0.join("guitar-study-song");
        std::fs::create_dir_all(scratch.0.join("song")).expect("a song folder");
        std::fs::create_dir_all(&twin).expect("a twin folder");
        std::fs::write(twin.join(versions::BASE_CHART), b"{}").expect("its chart");
        let outcome = write_twin(&scratch.0.join("song"), Path::new("/nowhere.wav"), &|_| {
            Err("the reader was called".to_owned())
        });
        assert_eq!(outcome, Ok(Outcome::AlreadyThere(twin)));
    }

    /// ⚠️ And the other way: a twin folder WITHOUT a chart must not
    /// end the run. That early return is what made a failed copy
    /// permanent — the folder was there, so every later attempt
    /// stopped at it and the song never got its twin.
    #[test]
    fn a_half_written_twin_does_not_end_the_run() {
        let scratch = Scratch::new("halfway");
        let song = scratch.0.join("song");
        std::fs::create_dir_all(&song).expect("a song folder");
        std::fs::create_dir_all(scratch.0.join("guitar-study-song")).expect("a twin folder");
        let outcome = write_twin(&song, Path::new("/nowhere.wav"), &|_| {
            Err("the reader was called".to_owned())
        });
        // It gets past the folder and fails on the real problem —
        // this song folder has no chart — rather than reporting a
        // twin that is not there.
        assert!(
            matches!(&outcome, Err(reason) if reason.contains("cannot load")),
            "expected the run to continue and fail honestly, got {outcome:?}"
        );
    }
}
