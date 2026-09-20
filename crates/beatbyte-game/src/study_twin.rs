//! The `[GS]` twin for every imported song, made in the
//! background after the import.
//!
//! The player's verdict on the stem-charted study was "by a distance
//! the best", and the ask was that every song carries BOTH — the
//! mix-charted original and its study twin — as two browser entries,
//! played against each other in live sessions. An import therefore
//! queues its folder here; a chore then decodes the song, separates
//! the `other` stem with demucs, and writes the twin through
//! [`beatbyte_chart::study::write_twin`] — the same writer the
//! command line uses. The original folder is never written to.
//!
//! demucs is a Python tool on the player's machine, not a dependency
//! of the game: without it the row in SETTINGS says so, the import is
//! what it always was, and nothing fails.
//!
//! ## It carries the vocal work too
//!
//! Since v0.17 this queue runs a song's whole background job, not
//! just its twin, because both want the same expensive thing. htdemucs
//! computes all four sources whatever it is asked for, so when VOCAL
//! CHARTS is on the job takes ONE four-source run, hands `other` to
//! the twin writer and reads the sung line off `vocals` — instead of
//! two separations of the same song, minutes apart, for one song.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;

use bevy::prelude::*;

use crate::chore::Chore;
use crate::config::Settings;
use crate::states::AppState;

/// The separator's command name.
pub const DEMUCS: &str = "demucs";

/// The model and the stem the study reads.
pub const DEMUCS_MODEL: &str = "htdemucs";

/// One song's background work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongWork {
    /// The song folder.
    pub folder: PathBuf,
    /// Write the `[GS]` twin from the `other` stem.
    pub twin: bool,
    /// Write the vocal chart and keep the karaoke stems.
    pub vocals: bool,
}

/// Song folders whose background work is still to be done, in import
/// order.
#[derive(Resource, Default)]
pub struct StudyQueue {
    /// Folders waiting.
    pub pending: VecDeque<SongWork>,
}

/// Plugin: takes the folders the import queues and makes their twins
/// one at a time, in the background, between songs.
pub struct StudyTwinPlugin;

impl Plugin for StudyTwinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StudyQueue>()
            .add_systems(Update, drive_queue);
    }
}

/// Whether the machine can separate a stem: `demucs --help` runs.
/// A version flag would be the natural probe, but demucs has none.
#[must_use]
pub fn demucs_available() -> bool {
    Command::new(DEMUCS)
        .arg("--help")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The SETTINGS row's second line: what the twin is, and — when the
/// tool is missing — how to get it.
#[must_use]
pub fn row_subtitle() -> String {
    if demucs_available() {
        "a second, stem-charted entry per imported song".to_owned()
    } else {
        format!("needs {DEMUCS} - `pipx install {DEMUCS}`")
    }
}

/// Whether an import should queue its twin. The setting decides; under
/// the autopilot the job runs only when asked for by its own switch,
/// because a separation saturates the machine for two minutes and the
/// autopilot's clock-teleport rule false-fails under exactly that load
/// (recorded in CLAUDE.md). Pure — tested.
#[must_use]
pub fn twin_wanted(setting_on: bool, autopilot: bool, autopilot_twins: bool) -> bool {
    setting_on && (!autopilot || autopilot_twins)
}

/// The VOCAL CHARTS row's second line.
///
/// It names the **cost** rather than the feature, because that is
/// what the player needs before deciding: this is the one setting
/// that spends minutes of machine and tens of megabytes per song.
#[must_use]
pub fn vocal_row_subtitle() -> String {
    if demucs_available() {
        "karaoke: separates each song (minutes, ~80 MB kept)".to_owned()
    } else {
        format!("needs {DEMUCS} - `pipx install {DEMUCS}`")
    }
}

/// Whether an import should queue its vocal chart. The setting
/// decides, and the autopilot needs its own switch for the same
/// reason the twin does: a separation saturates the machine and the
/// autopilot's clock-teleport rule false-fails under exactly that
/// load. Pure — tested.
#[must_use]
pub fn vocals_wanted(setting_on: bool, autopilot: bool, autopilot_vocals: bool) -> bool {
    setting_on && (!autopilot || autopilot_vocals)
}

/// Whether the next job may start now: one chore at a time, never
/// during a song (the separation would fight the game for the CPU
/// and the clock would pay). Pure — tested.
#[must_use]
pub fn may_start(chore_running: bool, in_gameplay: bool, pending: usize) -> bool {
    !chore_running && !in_gameplay && pending > 0
}

/// The demucs invocation for `wav` into `out_dir`: the `other` stem
/// of `DEMUCS_MODEL` on `device`. Pure — tested.
#[must_use]
pub fn demucs_args(wav: &Path, out_dir: &Path, device: &str) -> Vec<String> {
    vec![
        "--two-stems=other".to_owned(),
        "-n".to_owned(),
        DEMUCS_MODEL.to_owned(),
        "-d".to_owned(),
        device.to_owned(),
        "-o".to_owned(),
        out_dir.display().to_string(),
        wav.display().to_string(),
    ]
}

/// Queue a freshly imported song folder for whatever background work
/// the settings and the harness allow.
pub fn queue_twin(queue: &mut StudyQueue, settings: &Settings, folder: PathBuf) {
    let autopilot = std::env::var_os("BEATBYTE_AUTOPILOT").is_some();
    let twins = std::env::var_os("BEATBYTE_AUTOPILOT_TWINS").is_some();
    let vocals_switch = std::env::var_os("BEATBYTE_AUTOPILOT_VOCALS").is_some();
    let work = SongWork {
        twin: twin_wanted(settings.guitar_study_twins, autopilot, twins),
        vocals: vocals_wanted(settings.vocal_charts, autopilot, vocals_switch),
        folder,
    };
    if work.twin || work.vocals {
        info!(
            "song work: queued {} (twin {}, vocals {})",
            work.folder.display(),
            work.twin,
            work.vocals
        );
        queue.pending.push_back(work);
    }
}

fn drive_queue(
    mut queue: ResMut<StudyQueue>,
    mut chore: ResMut<Chore>,
    state: Res<State<AppState>>,
) {
    let in_gameplay = matches!(state.get(), AppState::Gameplay);
    if !may_start(chore.running(), in_gameplay, queue.pending.len()) {
        return;
    }
    let Some(work) = queue.pending.pop_front() else {
        return;
    };
    let name = work.folder.file_name().map_or_else(
        || work.folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let what = if work.vocals {
        "vocals"
    } else {
        "guitar study"
    };
    let line = format!("{what} for {}...", crate::ui::font_safe(&name));
    // ⚠️ The retry keeps the WHOLE work item. The first version of
    // this rebuilt a path out of the song's NAME, so a retry looked
    // for the folder in the working directory and never found it.
    let retry = work.clone();
    if let Err(reason) = chore.start(line, move || do_work(&work)) {
        warn!("song work: {reason}");
        queue.pending.push_front(retry);
    }
}

/// The chart and the audio a song folder points at.
fn song_audio(folder: &Path) -> Result<std::path::PathBuf, String> {
    let names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("cannot read the folder: {error}"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let pointer = std::fs::read_to_string(folder.join(beatbyte_chart::versions::POINTER_FILE)).ok();
    let active = beatbyte_chart::versions::resolve_active(pointer.as_deref(), &names);
    let chart = beatbyte_chart::load_chart_file(&folder.join(&active))
        .map_err(|error| format!("cannot load {active}: {error}"))?;
    beatbyte_chart::resolve_audio_path(folder, &chart.song.audio).map_err(|error| error.to_string())
}

/// One song's background work.
///
/// When the vocal chart is wanted this takes ONE four-source
/// separation and lets the twin ride along on it; when only the twin
/// is wanted it takes the cheaper two-stem run it always did. A
/// missing tool is a line the player reads, not an error.
fn do_work(work: &SongWork) -> Result<String, String> {
    if !demucs_available() {
        return Ok(format!(
            "skipped: {DEMUCS} is not installed - `pipx install {DEMUCS}`"
        ));
    }
    let audio_path = song_audio(&work.folder).map_err(|error| format!("song work: {error}"))?;
    if !work.vocals {
        return make_twin(&work.folder, &audio_path);
    }
    let scratch = std::env::temp_dir()
        .join("beatbyte-song-work")
        .join(scratch_name(&work.folder));
    let mut twin: Option<Result<String, String>> = None;
    let vocal = beatbyte_audio::stems::analyse_song(
        &audio_path,
        &scratch,
        &beatbyte_audio::singing::SingingConfig::default(),
        |other| {
            if work.twin {
                twin = Some(write_twin_from(&work.folder, other));
            }
        },
    );
    let mut lines = Vec::new();
    match twin {
        Some(Ok(line)) => lines.push(line),
        Some(Err(reason)) => warn!("guitar study: {reason}"),
        None => {}
    }
    lines.push(file_vocal_chart(&audio_path, vocal));
    Ok(lines.join("; "))
}

/// Turn a finished separation into `<audio>.vocals.json`, or into the
/// line that says why there is none.
fn file_vocal_chart(audio_path: &Path, work: beatbyte_audio::stems::VocalWork) -> String {
    use beatbyte_audio::stems::StemState;
    match &work.state {
        StemState::Ready => {}
        StemState::Instrumental => return "vocals: instrumental".to_owned(),
        StemState::NoReliableVocals => return "vocals: none clear enough".to_owned(),
        StemState::NeedsSeparator => return "vocals: no separator".to_owned(),
        StemState::Failed { reason, .. } => {
            warn!("vocals: {reason}");
            return "vocals: failed".to_owned();
        }
    }
    let notes: usize = work.phrases.iter().map(|p| p.notes.len()).sum();
    let part = beatbyte_core::vocal::VocalPart {
        id: "lead".to_owned(),
        role: beatbyte_core::vocal::VocalRole::Lead,
        name: None,
        phrases: work.phrases,
    };
    let file = beatbyte_chart::vocals::VocalChartFile::new(
        &work.audio_sha256,
        beatbyte_chart::vocals::VocalProvenance {
            separator: work.separator,
            analyzer: format!("beatbyte-singing {}", env!("CARGO_PKG_VERSION")),
            aligner: None,
        },
        vec![part],
    );
    // A chart the loader would refuse is never written: a wrong
    // target is worse than no target, and half a file is worse still.
    if !file.is_playable() {
        warn!("vocals: the generated chart does not validate; not written");
        return "vocals: rejected".to_owned();
    }
    let path = beatbyte_chart::vocals::vocals_path(audio_path);
    match beatbyte_chart::vocals::save_vocals(&path, &file) {
        Ok(()) => format!("vocals: {notes} notes"),
        Err(error) => {
            warn!("vocals: cannot write {}: {error}", path.display());
            "vocals: not written".to_owned()
        }
    }
}

/// A scratch directory name that cannot escape its parent.
fn scratch_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "song".to_owned())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(64)
        .collect()
}

/// The twin from a stem that is already separated.
fn write_twin_from(folder: &Path, lead: &Path) -> Result<String, String> {
    match beatbyte_chart::study::write_twin(folder, lead, &read)? {
        beatbyte_chart::study::Outcome::Written { title, notes, .. } => {
            let counts: Vec<String> = notes.iter().map(|(d, n)| format!("{d} {n}")).collect();
            info!("guitar study: wrote {title} ({})", counts.join(", "));
            Ok(format!(
                "guitar study added: {}",
                crate::ui::font_safe(&title)
            ))
        }
        beatbyte_chart::study::Outcome::AlreadyThere(_) => {
            Ok("guitar study: already there".to_owned())
        }
        beatbyte_chart::study::Outcome::Refused(reason) => Ok(format!(
            "guitar study skipped: {}",
            crate::ui::font_safe(&reason)
        )),
    }
}

/// The twin alone: the cheaper two-stem run, unchanged from before
/// the vocal work existed.
fn make_twin(folder: &Path, audio_path: &Path) -> Result<String, String> {
    let scratch = std::env::temp_dir()
        .join("beatbyte-guitar-study")
        .join(scratch_name(folder));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|error| format!("guitar study: scratch: {error}"))?;
    // The stem is separated from the DECODED song, so it lies on the
    // timeline the analysis and the chart use.
    let audio = beatbyte_audio::decode_file(audio_path)
        .map_err(|error| format!("guitar study: cannot decode: {error}"))?;
    let wav = scratch.join("mix.wav");
    beatbyte_audio::write_wav_mono16(&wav, &audio)
        .map_err(|error| format!("guitar study: cannot write the WAV: {error}"))?;
    let stems = scratch.join("stems");
    let mut separated = false;
    for device in ["mps", "cpu"] {
        let ok = Command::new(DEMUCS)
            .args(demucs_args(&wav, &stems, device))
            .output()
            .is_ok_and(|out| out.status.success());
        if ok {
            separated = true;
            break;
        }
    }
    if !separated {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(format!(
            "guitar study: {DEMUCS} could not separate the stem"
        ));
    }
    let lead = stems.join(DEMUCS_MODEL).join("mix").join("other.wav");
    let outcome = write_twin_from(folder, &lead);
    let _ = std::fs::remove_dir_all(&scratch);
    outcome
}

fn read(path: &Path) -> Result<beatbyte_chart::redesign::Reading, String> {
    let audio = beatbyte_audio::decode_file(path)
        .map_err(|error| format!("cannot decode `{}`: {error}", path.display()))?;
    let priming = audio.priming();
    let trim = beatbyte_chart::AudioTrim::declared(
        priming.samples,
        priming.timescale,
        audio.sample_rate(),
    );
    let analysis = <beatbyte_audio::SpectralAnalyzer as beatbyte_audio::Analyzer>::analyze(
        &beatbyte_audio::SpectralAnalyzer::default(),
        &audio,
    );
    Ok(beatbyte_chart::redesign::Reading { analysis, trim })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_decides_and_the_autopilot_needs_its_own_switch() {
        assert!(
            twin_wanted(true, false, false),
            "a player's import gets its twin"
        );
        assert!(!twin_wanted(false, false, false), "off means off");
        assert!(
            !twin_wanted(true, true, false),
            "the harness does not separate stems uninvited"
        );
        assert!(twin_wanted(true, true, true), "unless its switch says so");
        assert!(
            !twin_wanted(false, true, true),
            "and never against the setting"
        );
    }

    #[test]
    fn the_vocal_work_needs_its_own_setting_and_its_own_harness_switch() {
        assert!(
            vocals_wanted(true, false, false),
            "a player's import gets it"
        );
        assert!(!vocals_wanted(false, false, false), "off means off");
        assert!(
            !vocals_wanted(true, true, false),
            "the harness does not separate stems uninvited"
        );
        assert!(vocals_wanted(true, true, true), "unless its switch says so");
        assert!(
            !vocals_wanted(false, true, true),
            "and never against the setting"
        );
    }

    #[test]
    fn a_queued_song_carries_what_was_asked_for_and_nothing_else() {
        let mut queue = StudyQueue::default();
        let mut settings = Settings {
            guitar_study_twins: true,
            vocal_charts: false,
            ..Settings::default()
        };
        queue_twin(&mut queue, &settings, PathBuf::from("/songs/a"));
        assert_eq!(queue.pending.len(), 1);
        let work = queue.pending.front().expect("queued");
        assert!(work.twin && !work.vocals);
        assert_eq!(work.folder, PathBuf::from("/songs/a"));

        // Both off: nothing is queued at all, so a chore never starts
        // for a song with no work in it.
        settings.guitar_study_twins = false;
        queue_twin(&mut queue, &settings, PathBuf::from("/songs/b"));
        assert_eq!(queue.pending.len(), 1, "an empty job was queued");

        // Vocals alone are enough to queue.
        settings.vocal_charts = true;
        queue_twin(&mut queue, &settings, PathBuf::from("/songs/c"));
        let work = queue.pending.back().expect("queued");
        assert!(!work.twin && work.vocals);
    }

    #[test]
    fn a_scratch_name_cannot_escape_its_parent() {
        // The job builds a temporary directory out of the song's own
        // folder name, which came from a file the player dropped in.
        assert!(!scratch_name(Path::new("/songs/../../etc")).contains(".."));
        assert!(!scratch_name(Path::new("/songs/a b/c")).contains(['/', '\\']));
        assert_eq!(
            scratch_name(Path::new("/songs/Toto - Africa")),
            "Toto---Africa"
        );
        assert!(scratch_name(Path::new("/x")).len() <= 64);
    }

    #[test]
    fn a_job_waits_for_a_free_runner_and_a_quiet_stage() {
        assert!(may_start(false, false, 1));
        assert!(!may_start(true, false, 1), "one chore at a time");
        assert!(!may_start(false, true, 1), "never during a song");
        assert!(!may_start(false, false, 0), "nothing to do");
    }

    #[test]
    fn the_separation_asks_for_the_other_stem_of_the_model() {
        let args = demucs_args(Path::new("/s/mix.wav"), Path::new("/s/stems"), "mps");
        assert_eq!(args[0], "--two-stems=other");
        assert_eq!(&args[1..3], ["-n", DEMUCS_MODEL]);
        assert_eq!(&args[3..5], ["-d", "mps"]);
        assert_eq!(&args[5..7], ["-o", "/s/stems"]);
        assert_eq!(args[7], "/s/mix.wav");
    }
}
