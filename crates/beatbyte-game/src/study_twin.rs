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

/// Song folders whose twin is still to be made, in import order.
#[derive(Resource, Default)]
pub struct StudyQueue {
    /// Folders waiting.
    pub pending: VecDeque<PathBuf>,
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

/// Queue a freshly imported song folder for its twin, if the setting
/// and the harness allow it.
pub fn queue_twin(queue: &mut StudyQueue, settings: &Settings, folder: PathBuf) {
    let autopilot = std::env::var_os("BEATBYTE_AUTOPILOT").is_some();
    let twins = std::env::var_os("BEATBYTE_AUTOPILOT_TWINS").is_some();
    if twin_wanted(settings.guitar_study_twins, autopilot, twins) {
        info!("guitar study: queued {}", folder.display());
        queue.pending.push_back(folder);
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
    let Some(folder) = queue.pending.pop_front() else {
        return;
    };
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let line = format!("guitar study for {}...", crate::ui::font_safe(&name));
    if let Err(reason) = chore.start(line, move || make_twin(&folder)) {
        warn!("guitar study: {reason}");
        queue.pending.push_front(folder_from_name(&name));
    }
}

fn folder_from_name(name: &str) -> PathBuf {
    PathBuf::from(name)
}

/// The job: decode → separate → write. Returns the line the player
/// sees; a missing tool is a line, not an error.
fn make_twin(folder: &Path) -> Result<String, String> {
    if !demucs_available() {
        return Ok(format!(
            "guitar study skipped: {DEMUCS} is not installed - `pipx install {DEMUCS}`"
        ));
    }
    let names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|error| format!("guitar study: cannot read the folder: {error}"))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let pointer = std::fs::read_to_string(folder.join(beatbyte_chart::versions::POINTER_FILE)).ok();
    let active = beatbyte_chart::versions::resolve_active(pointer.as_deref(), &names);
    let chart = beatbyte_chart::load_chart_file(&folder.join(&active))
        .map_err(|error| format!("guitar study: cannot load {active}: {error}"))?;
    let audio_path = beatbyte_chart::resolve_audio_path(folder, &chart.song.audio)
        .map_err(|error| format!("guitar study: {error}"))?;
    let scratch = std::env::temp_dir().join("beatbyte-guitar-study").join(
        folder
            .file_name()
            .map_or_else(|| "song".to_owned(), |n| n.to_string_lossy().into_owned()),
    );
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|error| format!("guitar study: scratch: {error}"))?;
    // The stem is separated from the DECODED song, so it lies on the
    // timeline the analysis and the chart use.
    let audio = beatbyte_audio::decode_file(&audio_path)
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
    let outcome = beatbyte_chart::study::write_twin(folder, &lead, &read);
    let _ = std::fs::remove_dir_all(&scratch);
    match outcome? {
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
