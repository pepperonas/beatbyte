//! Smart lyrics from inside the game (plan §6, milestone L4b): the
//! aligner model's download from the settings screen, and "align
//! this song" from the song browser — both off the main thread, both
//! reporting where they are, both cancellable, and both a **visible
//! state** in every build: a game built without `ml` says so on the
//! row instead of hiding it.
//!
//! Nothing here starts on its own. The download runs when the player
//! confirms the LYRICS MODEL row (the one explicit action the README
//! promises), the alignment when they press `K` on a song that has
//! lyrics beside it. Both use exactly the code `beatbyte-cli` runs
//! ([`beatbyte_lyrics::align_file`], `ModelStore::install`), so the
//! result from the menu is the result from the terminal.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

/// The model's standing, as the settings row shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModelState {
    /// The game was built without `ml`.
    NotInBuild,
    /// Not looked at yet (the check hashes 378 MB; it runs when the
    /// settings screen first asks).
    #[default]
    Unknown,
    /// Being checked.
    Probing,
    /// Not on this machine.
    Missing,
    /// Coming down.
    Downloading {
        /// Bytes so far.
        done: u64,
        /// Bytes in total.
        total: u64,
    },
    /// Present and intact.
    Installed,
    /// Present but not matching its hash (refused; re-download).
    Damaged,
    /// The last download failed (retry).
    Failed(String),
}

/// Where a running alignment is, for the browser's status row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignProgress {
    /// The song's title.
    pub title: String,
    /// The stage label.
    pub label: String,
}

/// What a finished alignment leaves for the status row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlignOutcome {
    /// Written; the message names the verdict.
    Done(String),
    /// Nothing written; the message says why.
    Failed(String),
    /// Nothing written; the player stopped it.
    Cancelled,
}

/// Why an alignment cannot start — each a line for the status row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannotAlign {
    /// Built without `ml`.
    NotInBuild,
    /// A built-in song.
    Builtin,
    /// No `.lrc` beside the song (press `L` first).
    NoLyrics,
    /// The model is not installed (settings).
    NoModel,
    /// The model's standing is still being checked (a hash of the
    /// file, at boot).
    ModelChecking,
    /// One is already running.
    Busy,
}

impl CannotAlign {
    /// The status line.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            CannotAlign::NotInBuild => {
                "this build has no aligner - build with --features ml (or use beatbyte-cli align)"
            }
            CannotAlign::Builtin => "built-in songs ship with their own lyrics",
            CannotAlign::NoLyrics => "no lyrics beside this song yet - press L to look them up",
            CannotAlign::NoModel => "download the lyrics model first (SETTINGS > LYRICS MODEL)",
            CannotAlign::ModelChecking => "still checking the lyrics model - try again in a moment",
            CannotAlign::Busy => "an alignment is already running - K again cancels it",
        }
    }
}

/// Whether an alignment may start. Pure — tested.
pub fn can_align(
    in_build: bool,
    is_file_song: bool,
    has_lyrics_file: bool,
    model: &ModelState,
    running: bool,
) -> Result<(), CannotAlign> {
    if !in_build {
        return Err(CannotAlign::NotInBuild);
    }
    if running {
        return Err(CannotAlign::Busy);
    }
    if !is_file_song {
        return Err(CannotAlign::Builtin);
    }
    if !has_lyrics_file {
        return Err(CannotAlign::NoLyrics);
    }
    match model {
        ModelState::Installed => Ok(()),
        ModelState::Unknown | ModelState::Probing => Err(CannotAlign::ModelChecking),
        _ => Err(CannotAlign::NoModel),
    }
}

/// The widest a settings value may be before it wraps into the
/// label (measured: `ENTER > DOWNLOADS` fits, a 37-character value
/// broke the row). A test pins every model value under it.
pub const VALUE_CHARS: usize = 20;

/// The settings row's value for a model state — short, the next step
/// on it; the explanation lives in [`model_subtitle`]. Pure — tested.
#[must_use]
pub fn model_text(state: &ModelState, size_mb: u64) -> String {
    match state {
        ModelState::NotInBuild => "NOT IN THIS BUILD".to_owned(),
        ModelState::Unknown | ModelState::Probing => "CHECKING...".to_owned(),
        ModelState::Missing => format!("ENTER > GET {size_mb} MB"),
        ModelState::Downloading { done, total } => {
            let percent = if *total == 0 {
                0
            } else {
                (done * 100 / total).min(100)
            };
            format!("{percent}% - ENTER STOPS")
        }
        ModelState::Installed => "INSTALLED".to_owned(),
        ModelState::Damaged => "DAMAGED > ENTER".to_owned(),
        ModelState::Failed(_) => "FAILED > ENTER".to_owned(),
    }
}

/// The widest the line under the list may be — the settings screen's
/// own limit, one source; a failure reason is cut to fit.
use crate::settings_ui::SUBTITLE_CHARS;

/// The line under the list while the LYRICS MODEL row is selected:
/// what the model is and where it comes from, or why the last
/// download failed. Pure — tested.
#[must_use]
pub fn model_subtitle(state: &ModelState) -> String {
    let line = match state {
        ModelState::NotInBuild => "no aligner in this build - see beatbyte-cli align".to_owned(),
        ModelState::Failed(reason) => format!("download failed: {reason}"),
        ModelState::Damaged => "file on disk fails its hash check - refused".to_owned(),
        ModelState::Downloading { .. } => {
            "from this project's release, SHA-256 checked when done".to_owned()
        }
        ModelState::Installed => {
            "wav2vec2 (Apache-2.0) on this machine - K aligns a song".to_owned()
        }
        _ => "English aligner wav2vec2 (Apache-2.0), fetched on ENTER".to_owned(),
    };
    line.chars().take(SUBTITLE_CHARS).collect()
}

/// The `.lrc` an alignment reads: beside the audio, the place the
/// lookup caches it. Pure — tested.
#[must_use]
pub fn lyrics_file_beside(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("lrc")
}

/// The smart-lyrics state: the model, a download, an alignment.
#[derive(Resource, Default)]
pub struct SmartLyrics {
    /// The model's standing.
    pub model: ModelState,
    /// The running alignment's progress, for the status row.
    pub aligning: Option<AlignProgress>,
    #[cfg(feature = "ml")]
    inner: ml::Inner,
}

impl SmartLyrics {
    /// Whether this build carries the aligner.
    #[must_use]
    pub const fn in_build() -> bool {
        cfg!(feature = "ml")
    }

    /// The settings row's value.
    #[must_use]
    pub fn model_text(&self) -> String {
        #[cfg(feature = "ml")]
        let size_mb = beatbyte_lyrics::emissions::MODEL.bytes / 1_000_000;
        #[cfg(not(feature = "ml"))]
        let size_mb = 0;
        model_text(&self.model, size_mb)
    }

    /// The line under the list for the LYRICS MODEL row.
    #[must_use]
    pub fn model_subtitle(&self) -> String {
        model_subtitle(&self.model)
    }

    /// Whether an alignment is running.
    #[must_use]
    pub fn is_aligning(&self) -> bool {
        self.aligning.is_some()
    }

    /// What confirming the LYRICS MODEL row does: start the download
    /// (missing / damaged / failed), cancel it (downloading), nothing
    /// (installed, checking, not in build).
    pub fn confirm_model_row(&mut self) {
        #[cfg(feature = "ml")]
        self.inner.confirm(&mut self.model);
    }

    /// Ask for the model's standing, once (the settings screen calls
    /// this on entry).
    pub fn probe_model(&mut self) {
        #[cfg(feature = "ml")]
        self.inner.probe(&mut self.model);
        #[cfg(not(feature = "ml"))]
        {
            self.model = ModelState::NotInBuild;
        }
    }

    /// Start aligning `audio_path` against the `.lrc` beside it, or
    /// say why not.
    pub fn start_align(
        &mut self,
        title: &str,
        is_file_song: bool,
        audio_path: Option<&Path>,
    ) -> Result<(), CannotAlign> {
        let lyrics = audio_path.map(lyrics_file_beside);
        can_align(
            Self::in_build(),
            is_file_song,
            lyrics.as_ref().is_some_and(|p| p.is_file()),
            &self.model,
            self.is_aligning(),
        )?;
        #[cfg(feature = "ml")]
        if let (Some(audio), Some(lyrics)) = (audio_path, lyrics) {
            self.inner.start_align(audio, &lyrics);
            self.aligning = Some(AlignProgress {
                title: title.to_owned(),
                label: "starting".to_owned(),
            });
        }
        #[cfg(not(feature = "ml"))]
        let _ = title;
        Ok(())
    }

    /// Stop the running alignment (nothing is written).
    pub fn cancel_align(&mut self) {
        #[cfg(feature = "ml")]
        self.inner.cancel_align();
    }

    /// Drain the background threads' messages. Returns a finished
    /// alignment's outcome, once.
    pub fn poll(&mut self) -> Option<AlignOutcome> {
        #[cfg(feature = "ml")]
        {
            self.inner.poll(&mut self.model, &mut self.aligning)
        }
        #[cfg(not(feature = "ml"))]
        {
            None
        }
    }
}

/// Report a finished alignment on the browser's status row, and keep
/// the progress label there while one runs.
pub fn poll_smart_lyrics(
    mut smart: ResMut<SmartLyrics>,
    mut status: ResMut<crate::import::ImportStatus>,
) {
    let was_aligning = smart.aligning.clone();
    if let Some(outcome) = smart.poll() {
        let title = was_aligning.map(|p| p.title).unwrap_or_default();
        status.0 = match outcome {
            AlignOutcome::Done(message) => format!("aligned \"{title}\": {message}"),
            AlignOutcome::Failed(message) => format!("alignment of \"{title}\" failed: {message}"),
            AlignOutcome::Cancelled => format!("alignment of \"{title}\" cancelled"),
        };
    } else if let Some(progress) = &smart.aligning {
        status.0 = format!("aligning \"{}\": {}", progress.title, progress.label);
    }
}

/// Register the resource and the poll.
pub struct SmartLyricsPlugin;

impl Plugin for SmartLyricsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmartLyrics>()
            .add_systems(Startup, probe_at_boot)
            .add_systems(Update, poll_smart_lyrics);
    }
}

/// Ask for the model's standing at boot (a hash of the file, in a
/// thread), so the browser's `K` knows the answer by the time a
/// player presses it. Reads a file that is already there; fetches
/// nothing.
fn probe_at_boot(mut smart: ResMut<SmartLyrics>) {
    smart.probe_model();
}

#[cfg(feature = "ml")]
mod ml {
    //! The threads. Each job owns a channel and a cancel flag; the
    //! main thread only ever `try_recv`s.

    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};

    use beatbyte_lyrics::emissions::MODEL;
    use beatbyte_lyrics::{JobError, JobProgress, Verdict, align_file};
    use beatbyte_ml::{ModelStore, Status};

    use super::{AlignOutcome, AlignProgress, ModelState};

    enum ModelMsg {
        Probed(Status),
        Progress { done: u64, total: u64 },
        Done(Result<(), String>),
    }

    enum AlignMsg {
        Progress(JobProgress),
        Done(Result<String, JobError>),
    }

    /// A receiver behind a mutex: `Receiver` is `Send` but not
    /// `Sync`, and a Bevy resource must be both. Only the main thread
    /// ever locks it, in `poll`.
    type Rx<T> = Option<Mutex<Receiver<T>>>;

    #[derive(Default)]
    pub(super) struct Inner {
        model_rx: Rx<ModelMsg>,
        download_cancel: Option<Arc<AtomicBool>>,
        align_rx: Rx<AlignMsg>,
        align_cancel: Option<Arc<AtomicBool>>,
    }

    /// Everything a receiver holds right now.
    fn drain<T>(rx: &Rx<T>) -> Vec<T> {
        let mut out = Vec::new();
        if let Some(rx) = rx
            && let Ok(rx) = rx.lock()
        {
            while let Ok(msg) = rx.try_recv() {
                out.push(msg);
            }
        }
        out
    }

    impl Inner {
        pub(super) fn probe(&mut self, model: &mut ModelState) {
            if *model != ModelState::Unknown || self.model_rx.is_some() {
                return;
            }
            let Some(store) = ModelStore::default_location() else {
                *model = ModelState::Failed("no config directory".to_owned());
                return;
            };
            *model = ModelState::Probing;
            let (tx, rx) = channel();
            self.model_rx = Some(Mutex::new(rx));
            std::thread::spawn(move || {
                let _ = tx.send(ModelMsg::Probed(store.status(&MODEL)));
            });
        }

        pub(super) fn confirm(&mut self, model: &mut ModelState) {
            match model {
                ModelState::Downloading { .. } => {
                    if let Some(cancel) = &self.download_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                ModelState::Missing | ModelState::Damaged | ModelState::Failed(_) => {
                    self.start_download(model);
                }
                _ => {}
            }
        }

        fn start_download(&mut self, model: &mut ModelState) {
            let Some(store) = ModelStore::default_location() else {
                *model = ModelState::Failed("no config directory".to_owned());
                return;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let (tx, rx) = channel();
            self.model_rx = Some(Mutex::new(rx));
            self.download_cancel = Some(Arc::clone(&cancel));
            *model = ModelState::Downloading {
                done: 0,
                total: MODEL.bytes,
            };
            std::thread::spawn(move || {
                let result = store.install(
                    &MODEL,
                    &mut |p| {
                        let _ = tx.send(ModelMsg::Progress {
                            done: p.done,
                            total: p.total,
                        });
                    },
                    &cancel,
                );
                let _ = tx.send(ModelMsg::Done(
                    result.map(|_| ()).map_err(|e| e.to_string()),
                ));
            });
        }

        pub(super) fn start_align(&mut self, audio: &Path, lyrics: &Path) {
            let cancel = Arc::new(AtomicBool::new(false));
            let (tx, rx) = channel();
            self.align_rx = Some(Mutex::new(rx));
            self.align_cancel = Some(Arc::clone(&cancel));
            let (audio, lyrics) = (audio.to_path_buf(), lyrics.to_path_buf());
            std::thread::spawn(move || {
                let progress_tx: Sender<AlignMsg> = tx.clone();
                let result = align_file(
                    &audio,
                    &lyrics,
                    None,
                    true,
                    &mut |p| {
                        let _ = progress_tx.send(AlignMsg::Progress(p));
                    },
                    &cancel,
                )
                .map(|summary| verdict_line(&summary));
                let _ = tx.send(AlignMsg::Done(result));
            });
        }

        pub(super) fn cancel_align(&mut self) {
            if let Some(cancel) = &self.align_cancel {
                cancel.store(true, Ordering::Relaxed);
            }
        }

        pub(super) fn poll(
            &mut self,
            model: &mut ModelState,
            aligning: &mut Option<AlignProgress>,
        ) -> Option<AlignOutcome> {
            if self.model_rx.is_some() {
                let mut finished = false;
                for msg in drain(&self.model_rx) {
                    match msg {
                        ModelMsg::Probed(status) => {
                            *model = match status {
                                Status::Installed => ModelState::Installed,
                                Status::Missing => ModelState::Missing,
                                Status::Damaged { .. } => ModelState::Damaged,
                            };
                            finished = true;
                        }
                        ModelMsg::Progress { done, total } => {
                            *model = ModelState::Downloading { done, total };
                        }
                        ModelMsg::Done(result) => {
                            *model = match result {
                                Ok(()) => ModelState::Installed,
                                Err(reason) if reason.contains("cancelled") => ModelState::Missing,
                                Err(reason) => ModelState::Failed(reason),
                            };
                            finished = true;
                        }
                    }
                }
                if finished {
                    self.model_rx = None;
                    self.download_cancel = None;
                }
            }
            let mut outcome = None;
            if self.align_rx.is_some() {
                for msg in drain(&self.align_rx) {
                    match msg {
                        AlignMsg::Progress(p) => {
                            if let Some(progress) = aligning.as_mut() {
                                progress.label = p.label();
                            }
                        }
                        AlignMsg::Done(result) => {
                            outcome = Some(match result {
                                Ok(line) => AlignOutcome::Done(line),
                                Err(JobError::Cancelled) => AlignOutcome::Cancelled,
                                Err(error) => AlignOutcome::Failed(error.to_string()),
                            });
                        }
                    }
                }
            }
            if outcome.is_some() {
                self.align_rx = None;
                self.align_cancel = None;
                *aligning = None;
            }
            outcome
        }
    }

    /// The one line a finished alignment leaves on the status row.
    fn verdict_line(summary: &beatbyte_lyrics::Summary) -> String {
        let Some(gate) = &summary.gate else {
            return format!("{} words, raw", summary.stats.words);
        };
        let verdict = match gate.verdict {
            Verdict::NoReference => "no line stamps to compare".to_owned(),
            Verdict::SameMaster => "same master as the source".to_owned(),
            Verdict::ShiftedMaster { offset_s } => format!("source {offset_s:+.2} s off"),
            Verdict::DifferentEdit => "a different edit, aligned times kept".to_owned(),
            Verdict::Failed => "FAILED, line-level fallback".to_owned(),
        };
        format!(
            "{verdict}; {} lines at line level - words.json beside the song",
            gate.lines_fallen_back
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_not_to_align_is_a_line_the_player_can_read() {
        let ok = ModelState::Installed;
        assert_eq!(
            can_align(false, true, true, &ok, false),
            Err(CannotAlign::NotInBuild)
        );
        assert_eq!(
            can_align(true, false, true, &ok, false),
            Err(CannotAlign::Builtin)
        );
        assert_eq!(
            can_align(true, true, false, &ok, false),
            Err(CannotAlign::NoLyrics)
        );
        assert_eq!(
            can_align(true, true, true, &ModelState::Missing, false),
            Err(CannotAlign::NoModel)
        );
        assert_eq!(
            can_align(true, true, true, &ModelState::Probing, false),
            Err(CannotAlign::ModelChecking),
            "a model still being checked is not 'missing'"
        );
        assert_eq!(
            can_align(true, true, true, &ok, true),
            Err(CannotAlign::Busy)
        );
        assert_eq!(can_align(true, true, true, &ok, false), Ok(()));
        for reason in [
            CannotAlign::NotInBuild,
            CannotAlign::Builtin,
            CannotAlign::NoLyrics,
            CannotAlign::NoModel,
            CannotAlign::ModelChecking,
            CannotAlign::Busy,
        ] {
            assert!(reason.message().len() > 10);
        }
    }

    #[test]
    fn the_model_row_names_every_state_and_its_next_step_and_fits() {
        assert_eq!(
            model_text(&ModelState::NotInBuild, 377),
            "NOT IN THIS BUILD"
        );
        assert!(model_text(&ModelState::Missing, 377).contains("377 MB"));
        assert!(model_text(&ModelState::Missing, 377).contains("ENTER"));
        let half = ModelState::Downloading {
            done: 50,
            total: 100,
        };
        assert_eq!(model_text(&half, 377), "50% - ENTER STOPS");
        let zero = ModelState::Downloading { done: 5, total: 0 };
        assert!(
            model_text(&zero, 377).starts_with("0%"),
            "no divide by zero"
        );
        assert_eq!(model_text(&ModelState::Installed, 377), "INSTALLED");
        assert!(model_text(&ModelState::Damaged, 377).contains("ENTER"));
        let failed = ModelState::Failed("connection timed out after 30 s".to_owned());
        assert!(model_text(&failed, 377).contains("ENTER"));
        // The reason is on the subtitle line, where there is room -
        // and every subtitle fits that line.
        assert!(model_subtitle(&failed).contains("timed out"));
        for state in [
            ModelState::NotInBuild,
            ModelState::Unknown,
            ModelState::Missing,
            ModelState::Installed,
            ModelState::Damaged,
            ModelState::Downloading { done: 1, total: 2 },
            ModelState::Failed("x".repeat(200)),
        ] {
            let line = model_subtitle(&state);
            assert!(
                line.chars().count() <= SUBTITLE_CHARS,
                "{line:?} is {} chars",
                line.chars().count()
            );
        }
        assert_eq!(model_text(&ModelState::Unknown, 377), "CHECKING...");
        // Every value fits the column: a 37-character one wrapped
        // into the label and broke the row (seen on the screenshot).
        for state in [
            ModelState::NotInBuild,
            ModelState::Unknown,
            ModelState::Missing,
            half,
            ModelState::Installed,
            ModelState::Damaged,
            failed,
        ] {
            let text = model_text(&state, 9999);
            assert!(
                text.chars().count() <= VALUE_CHARS,
                "{text:?} is {} chars, over {VALUE_CHARS}",
                text.chars().count()
            );
        }
    }

    #[test]
    fn the_lyrics_file_is_the_lookups_cache_beside_the_audio() {
        assert_eq!(
            lyrics_file_beside(Path::new("/s/Artist - Title.m4a")),
            PathBuf::from("/s/Artist - Title.lrc")
        );
    }

    #[test]
    fn a_build_without_ml_says_so_and_refuses_politely() {
        let mut smart = SmartLyrics::default();
        if !SmartLyrics::in_build() {
            smart.probe_model();
            assert_eq!(smart.model_text(), "NOT IN THIS BUILD");
            assert_eq!(
                smart.start_align("x", true, Some(Path::new("/nope.m4a"))),
                Err(CannotAlign::NotInBuild)
            );
            assert!(smart.poll().is_none());
        } else {
            // With the aligner in the build, a song without an `.lrc`
            // is refused for THAT reason, before any model question.
            smart.model = ModelState::Installed;
            assert_eq!(
                smart.start_align("x", true, Some(Path::new("/nope.m4a"))),
                Err(CannotAlign::NoLyrics)
            );
        }
    }
}
