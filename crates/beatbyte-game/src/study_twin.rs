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
            .add_systems(Update, (backfill, drive_queue).chain());
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

/// Songs in the library that still want vocal work, in browser order.
///
/// Reads only the sidecars — the manifest and the chart beside each
/// audio file — so a library of two hundred songs costs two hundred
/// tiny reads and no separator runs at all. That is the whole point
/// of [`beatbyte_audio::stems::wants_separation`]: the expensive
/// question is answered once and written down. Pure but for the file
/// stats — tested through that function.
#[must_use]
pub fn songs_wanting_vocals(
    library: &crate::library::SongLibrary,
    separator_available: bool,
) -> Vec<PathBuf> {
    if !separator_available {
        return Vec::new();
    }
    let mut folders = Vec::new();
    for entry in &library.entries {
        let crate::library::SongSource::File { audio_path, .. } = &entry.source else {
            continue;
        };
        // A chart that is already current needs nothing, and asking
        // costs one read rather than a hash of the whole song.
        if beatbyte_chart::vocals::vocals_path(audio_path).is_file() {
            continue;
        }
        let manifest = beatbyte_audio::stems::read_manifest(audio_path);
        // A settled answer — instrumental, no usable vocals, a hard
        // failure — is never paid for again. `wants_separation` is
        // asked WITHOUT the audio's hash, which would mean reading
        // every song off the disk: a stale manifest simply retries,
        // which is the safe direction.
        let stale_is_fine = manifest.as_ref().is_some_and(|m| m.state.is_settled());
        if stale_is_fine {
            continue;
        }
        let wanted = manifest
            .as_ref()
            .is_none_or(|m| m.state.retry_when(separator_available));
        if let Some(folder) = audio_path.parent().filter(|_| wanted) {
            folders.push(folder.to_path_buf());
        }
    }
    folders
}

/// Put the library's outstanding vocal work on the queue, once.
///
/// It runs between songs like everything else here, and it says out
/// loud what it is about to spend: a library of two hundred songs is
/// hours of a saturated machine and gigabytes of stems, and a player
/// who turned the setting on deserves to see the size of what they
/// asked for rather than discover it as a full disk.
fn backfill(
    library: Option<Res<crate::library::SongLibrary>>,
    settings: Res<Settings>,
    mut queue: ResMut<StudyQueue>,
    state: Res<State<AppState>>,
    mut swept: Local<bool>,
) {
    if *swept || !settings.vocal_charts || matches!(state.get(), AppState::Gameplay) {
        return;
    }
    let Some(library) = library else {
        return;
    };
    if library.entries.is_empty() {
        return;
    }
    *swept = true;
    let autopilot = std::env::var_os("BEATBYTE_AUTOPILOT").is_some();
    let vocals_switch = std::env::var_os("BEATBYTE_AUTOPILOT_VOCALS").is_some();
    if !vocals_wanted(settings.vocal_charts, autopilot, vocals_switch) {
        return;
    }
    let folders = songs_wanting_vocals(&library, demucs_available());
    if folders.is_empty() {
        return;
    }
    info!(
        "vocals: {} song(s) still to analyse - roughly {} minute(s) of work and {} GB of stems,          one at a time between songs",
        folders.len(),
        folders.len(),
        folders.len() * 80 / 1024
    );
    for folder in folders {
        queue.pending.push_back(SongWork {
            folder,
            twin: false,
            vocals: true,
        });
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
        Ok(()) => {
            share_with_twin(audio_path, &path);
            format!("vocals: {notes} notes")
        }
        Err(error) => {
            warn!("vocals: cannot write {}: {error}", path.display());
            "vocals: not written".to_owned()
        }
    }
}

/// Put the same vocal chart beside the song's `[GS]` twin.
///
/// The twin keeps its own COPY of the audio, so the sidecar beside
/// the original is invisible from the twin's folder and selecting the
/// study entry would silently get no vocals. The copy is a byte copy
/// — the chart's `audio_sha256` matches it exactly — so the same file
/// is simply valid in both places, and it is 400 KB against the
/// hundred megabytes the audio already costs twice.
///
/// Best effort: a twin that is not there, or a folder that cannot be
/// written, is not a reason to fail a chart that was written.
fn share_with_twin(audio_path: &Path, written: &Path) {
    let Some(folder) = audio_path.parent() else {
        return;
    };
    let Some(twin) = beatbyte_chart::study::twin_folder_for(folder) else {
        return;
    };
    let Some(name) = audio_path.file_name() else {
        return;
    };
    let twin_audio = twin.join(name);
    if !twin_audio.is_file() {
        return;
    }
    let target = beatbyte_chart::vocals::vocals_path(&twin_audio);
    match std::fs::copy(written, &target) {
        Ok(_) => info!("vocals: also placed beside the study twin"),
        Err(error) => warn!("vocals: cannot reach the twin: {error}"),
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

    fn entry(audio: &Path) -> crate::library::SongEntry {
        crate::library::SongEntry {
            title: "T".to_owned(),
            artist: "A".to_owned(),
            bpm: 120.0,
            duration_s: None,
            difficulties: Vec::new(),
            note_counts: Vec::new(),
            genre: None,
            preview_start_s: None,
            source: crate::library::SongSource::File {
                chart_path: audio.with_extension("json"),
                audio_path: audio.to_path_buf(),
            },
            polish: crate::library::Polish::default(),
            loudness: None,
            has_lyrics: false,
        }
    }

    #[test]
    fn the_backfill_asks_for_the_work_nobody_has_done_and_nothing_else() {
        use beatbyte_audio::stems::{StemManifest, StemState, write_manifest};
        let dir = std::env::temp_dir().join(format!("bb-backfill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let make = |name: &str| {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).expect("a scratch folder");
            let audio = folder.join("song.wav");
            std::fs::write(&audio, b"not really audio").expect("a file");
            audio
        };
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        // Never touched: wants the work.
        let fresh = make("fresh");
        // Already charted: wants nothing, and asking must not cost a
        // hash of the whole song.
        let charted = make("charted");
        std::fs::write(beatbyte_chart::vocals::vocals_path(&charted), "{}").expect("a sidecar");
        // A settled answer: instrumental. Never paid for again — this
        // is the whole reason the state is written down.
        let instrumental = make("instrumental");
        write_manifest(
            &instrumental,
            &StemManifest::new(hash, "demucs", StemState::Instrumental),
        )
        .expect("a manifest");
        // A hard failure: equally settled.
        let broken = make("broken");
        write_manifest(
            &broken,
            &StemManifest::new(
                hash,
                "demucs",
                StemState::Failed {
                    reason: "cannot decode".to_owned(),
                    retryable: false,
                },
            ),
        )
        .expect("a manifest");
        // The separator arrived since last time.
        let waiting = make("waiting");
        write_manifest(
            &waiting,
            &StemManifest::new(hash, "demucs", StemState::NeedsSeparator),
        )
        .expect("a manifest");

        let library = crate::library::SongLibrary {
            entries: vec![
                entry(&fresh),
                entry(&charted),
                entry(&instrumental),
                entry(&broken),
                entry(&waiting),
            ],
        };
        let wanted = songs_wanting_vocals(&library, true);
        let names: Vec<String> = wanted
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(names, vec!["fresh", "waiting"], "{names:?}");

        // Without a separator there is nothing to queue at all.
        assert!(songs_wanting_vocals(&library, false).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
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
