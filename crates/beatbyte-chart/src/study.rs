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
    let (chart, names) = match prepare(
        song_folder,
        lead,
        read,
        &twin_title,
        DESIGNER,
        "guitar-study".to_owned(),
    )? {
        Prepared::Chart { chart, names } => (*chart, names),
        Prepared::Refused(reason) => return Ok(Outcome::Refused(reason)),
    };

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
        notes: note_counts(&chart),
    })
}

fn note_counts(chart: &ChartFile) -> Vec<(String, usize)> {
    chart
        .charts
        .iter()
        .map(|c| (c.difficulty.id().to_owned(), c.notes.len()))
        .collect()
}

/// A study chart ready to be written, or why there is none.
enum Prepared {
    /// The chart and the names of the files in the song folder.
    Chart {
        chart: Box<ChartFile>,
        names: Vec<String>,
    },
    /// The stem carries too little to chart.
    Refused(String),
}

/// The folder's file names and its active chart, validated.
fn active_of(song_folder: &Path) -> Result<(Vec<String>, String, ChartFile), String> {
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
    Ok((names, active_name, active))
}

/// Chart the stem at `lead` on the grid of the folder's active chart.
fn prepare(
    song_folder: &Path,
    lead: &Path,
    read: &dyn Fn(&Path) -> Result<Reading, String>,
    title: &dyn Fn(&str) -> String,
    designer: &str,
    directive: String,
) -> Result<Prepared, String> {
    let (names, _, active) = active_of(song_folder)?;
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
        title: title(&active.song.title),
        artist: active.song.artist.clone(),
        audio: active.song.audio.clone(),
    };
    let mut chart = generate_lead_study(&lead_analysis, &meta);
    if chart.charts.iter().any(|c| c.notes.is_empty()) {
        return Ok(Prepared::Refused(format!(
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
        designer: designer.to_owned(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        directive: Some(directive),
    });
    let problems = errors_of(&chart);
    if !problems.is_empty() {
        return Err(format!(
            "the study chart is invalid: {}",
            problems.join("; ")
        ));
    }

    Ok(Prepared::Chart {
        chart: Box::new(chart),
        names,
    })
}

/// Who an in-place upgrade's provenance names.
pub const UPGRADE_DESIGNER: &str = "import-upgrade";

/// What an in-place upgrade came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Upgrade {
    /// Written as revision `name`, which now plays.
    Written {
        /// The revision file's name (`chart.v2.json`, …).
        name: String,
        /// `(difficulty id, notes)` in chart order.
        notes: Vec<(String, usize)>,
        /// Chords were asked for with no polyphony evidence beside the
        /// audio, so none were written.
        chords_without_evidence: bool,
    },
    /// The chart that plays is already an upgrade; nothing written.
    AlreadyDone,
    /// Not upgraded, and why: a hand-edited chart is the player's and is
    /// never replaced; a stem with too little in it charts nothing.
    Kept(String),
}

/// Improve an imported song's chart **in its own folder** (ADR-0025):
/// chart the separated instrument stem at `lead` on the accepted grid,
/// apply the classic rules of `recipe` (chords from the polyphony
/// sidecar when one sits beside the audio), write it as the folder's
/// next revision and point the folder at it. The chart that played
/// before stays as a revision — one pointer away, as a redesign is.
///
/// This replaces the `[GS]` twin for a player's import: one entry per
/// version, with the better chart, rather than a second entry beside a
/// worse one.
///
/// # Errors
/// When the folder or its active chart cannot be read, the stem does
/// not fit the song, or the revision cannot be written.
pub fn write_upgrade(
    song_folder: &Path,
    lead: &Path,
    read: &dyn Fn(&Path) -> Result<Reading, String>,
    recipe: crate::classic::Recipe,
) -> Result<Upgrade, String> {
    let (_, active_name, active) = active_of(song_folder)?;
    if versions::is_hand_edited(&active) {
        return Ok(Upgrade::Kept(format!(
            "{active_name} was edited by hand and stays the chart that plays"
        )));
    }
    if active
        .provenance
        .as_ref()
        .is_some_and(|p| p.designer == UPGRADE_DESIGNER)
    {
        return Ok(Upgrade::AlreadyDone);
    }
    let directive = format!("guitar-study+classic:{}", recipe.directive());
    let (mut chart, names) = match prepare(
        song_folder,
        lead,
        read,
        &str::to_owned,
        UPGRADE_DESIGNER,
        directive,
    )? {
        Prepared::Chart { chart, names } => (*chart, names),
        Prepared::Refused(reason) => return Ok(Upgrade::Kept(reason)),
    };
    let poly = crate::classic::poly_beside(song_folder, &chart)?;
    let changes = crate::classic::apply_with(&mut chart, recipe, poly.as_ref());
    let problems = errors_of(&chart);
    if !problems.is_empty() {
        return Err(format!(
            "the upgraded chart is invalid: {}",
            problems.join("; ")
        ));
    }
    let name = versions::next_version_name(&names);
    save_chart_file(&song_folder.join(&name), &chart)
        .map_err(|error| format!("cannot write {name}: {error}"))?;
    crate::io::activate_revision(song_folder, &name)
        .map_err(|error| format!("cannot point the song at {name}: {error}"))?;
    Ok(Upgrade::Written {
        name,
        notes: note_counts(&chart),
        chords_without_evidence: changes.chords_without_evidence,
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

    /// A reading with a tune in it: onsets on the beat and a melody of
    /// changing pitches over a minute — enough for every difficulty.
    fn tuneful() -> beatbyte_core::SongAnalysis {
        use beatbyte_core::music::{MelodyNote, Onset};
        let beat = 0.5;
        let onsets = (0..240)
            .map(|i| Onset {
                time_s: 1.0 + f64::from(i) * beat / 2.0,
                strength: if i % 2 == 0 { 0.9 } else { 0.4 },
                brightness: (i % 16) as f32 / 16.0,
            })
            .collect();
        let melody = (0..220)
            .map(|i| MelodyNote {
                time_s: 1.0 + f64::from(i) * 0.25,
                end_s: 1.2 + f64::from(i) * 0.25,
                midi: 55.0 + ((i * 7) % 19) as f32,
                strength: 0.9,
            })
            .collect();
        beatbyte_core::SongAnalysis {
            repeats: Vec::new(),
            downbeats: Vec::new(),
            bpm: 120.0,
            bpm_confidence: 0.8,
            alt_bpm: None,
            beats: (0..136).map(|i| 1.0 + f64::from(i) * beat).collect(),
            onsets,
            energy: vec![0.8; 1400],
            energy_hop_s: 0.05,
            duration_s: 68.0,
            melody,
        }
    }

    /// A song folder with a generated chart and its (empty) audio file.
    fn imported(dir: &Path) {
        let chart = crate::generate::generate_chart(
            &tuneful(),
            &GenerateMeta {
                title: "Africa".into(),
                artist: "Toto".into(),
                audio: "mix.wav".into(),
            },
        );
        save_chart_file(&dir.join(versions::BASE_CHART), &chart).expect("chart");
        std::fs::write(dir.join("mix.wav"), b"").expect("audio");
    }

    fn reader(_: &Path) -> Result<Reading, String> {
        Ok(Reading {
            analysis: tuneful(),
            trim: crate::schema::AudioTrim::declared(0, 0, 44_100),
        })
    }

    /// The upgrade lands in the song's OWN folder as the next revision,
    /// keeps the song's title (no tag), names itself in its provenance,
    /// and plays from now on — the chart that played before stays as a
    /// revision. A second run writes nothing.
    #[test]
    fn an_import_is_upgraded_in_its_own_folder() {
        let scratch = Scratch::new("upgrade");
        let song = scratch.0.join("toto---africa");
        std::fs::create_dir_all(&song).expect("song");
        imported(&song);
        let before = std::fs::read(song.join(versions::BASE_CHART)).expect("before");
        let outcome = write_upgrade(
            &song,
            &song.join("other.wav"),
            &reader,
            crate::classic::Recipe::all(),
        )
        .expect("upgrade");
        let Upgrade::Written {
            name,
            chords_without_evidence,
            ..
        } = outcome
        else {
            panic!("not written: {outcome:?}");
        };
        assert_eq!(name, "chart.v2.json");
        assert!(
            chords_without_evidence,
            "no polyphony sidecar: no chords guessed"
        );
        let pointer = std::fs::read_to_string(song.join(versions::POINTER_FILE)).expect("ptr");
        assert!(pointer.contains("chart.v2.json"), "{pointer}");
        assert_eq!(
            std::fs::read(song.join(versions::BASE_CHART)).expect("after"),
            before,
            "the chart that played before is untouched"
        );
        let upgraded = load_chart_file(&song.join(&name)).expect("v2");
        assert_eq!(upgraded.song.title, "Africa", "no [GS] in an upgrade");
        assert_eq!(
            upgraded.provenance.as_ref().map(|p| p.designer.as_str()),
            Some(UPGRADE_DESIGNER)
        );
        assert!(
            !twin_folder_for(&song).expect("twin").exists(),
            "no twin folder"
        );
        assert_eq!(
            write_upgrade(
                &song,
                &song.join("other.wav"),
                &reader,
                crate::classic::Recipe::all()
            )
            .expect("again"),
            Upgrade::AlreadyDone
        );
    }

    /// A chart the player edited by hand is theirs: never replaced, and
    /// nothing is analysed or written to find that out.
    #[test]
    fn a_hand_edited_chart_is_never_upgraded() {
        let scratch = Scratch::new("upgrade-hand");
        let song = scratch.0.join("song");
        std::fs::create_dir_all(&song).expect("song");
        imported(&song);
        let path = song.join(versions::BASE_CHART);
        let mut chart = load_chart_file(&path).expect("chart");
        chart.provenance = Some(Provenance {
            parent_hash: chart_hash(&chart),
            designer: versions::EDITOR_DESIGNER.to_owned(),
            created_ms: 0,
            directive: None,
        });
        save_chart_file(&path, &chart).expect("save");
        let never = |_: &Path| -> Result<Reading, String> { panic!("read a hand-edited song") };
        let outcome = write_upgrade(
            &song,
            &song.join("o.wav"),
            &never,
            crate::classic::Recipe::all(),
        )
        .expect("kept");
        assert!(matches!(outcome, Upgrade::Kept(_)), "{outcome:?}");
        assert!(!song.join("chart.v2.json").exists());
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
