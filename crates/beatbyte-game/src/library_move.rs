//! SETTINGS > LIBRARY: put the song library somewhere else — an
//! external drive, say — and move what is there without losing a byte.
//!
//! The row walks through five steps, each one a line in its own value
//! column: choose a folder (the system's folder dialog on macOS, or a
//! folder dropped onto the window while the row is selected), confirm
//! the move with its size, copy (verified, in passes — see
//! `beatbyte_library::relocate`), switch the game over, and only then
//! offer to delete the old copy. Declining keeps it.
//!
//! The work runs off the frame thread and is collected here wherever
//! the player has gone, like every other long job; the library is
//! rescanned the moment the switch is made.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};

use beatbyte_library::relocate::{self, Moved, Phase, Progress};

/// How long the progress panel keeps a finished move up.
pub const LINGER_S: f32 = 8.0;

/// What the worker last reported, and when its phase began — the
/// rate and the time left are measured per phase, because the verify
/// and the delete read at a different speed than the copy writes.
#[derive(Debug, Clone, Copy, Default)]
struct Live {
    progress: Progress,
    since: Option<Instant>,
}

/// The worker's report callback: store the progress, restart the
/// clock when the phase or the pass changes.
fn report(shared: &Mutex<Live>, progress: Progress) {
    if let Ok(mut live) = shared.lock() {
        let same = live.since.is_some()
            && (live.progress.phase, live.progress.pass) == (progress.phase, progress.pass);
        if !same {
            live.since = Some(Instant::now());
        }
        live.progress = progress;
    }
}

/// Where the move stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Step {
    /// Nothing asked.
    #[default]
    Idle,
    /// The folder dialog is open.
    Choosing,
    /// A folder was chosen; the player is asked to confirm.
    Confirm {
        /// The new place.
        to: PathBuf,
        /// What the library weighs.
        bytes: u64,
        /// How many files it has.
        files: usize,
    },
    /// Copying.
    Copying {
        /// The old place.
        from: PathBuf,
        /// The new place.
        to: PathBuf,
    },
    /// Copied and switched; the old copy is still there.
    Copied {
        /// The old place — as the game knew it, never as the record on
        /// the target says (that record is untrusted).
        from: PathBuf,
        /// The new place.
        to: PathBuf,
        /// What was copied.
        moved: Moved,
    },
    /// Removing the old copy.
    Deleting,
    /// Finished, with what to say.
    Done(String),
    /// Stopped, with why.
    Failed(String),
}

/// What a background job hands back.
enum Outcome {
    Chosen(Option<PathBuf>),
    Copied(Result<Moved, String>),
    Deleted(Result<relocate::Deleted, String>),
}

/// The move, as the settings row sees it.
#[derive(Resource, Default)]
pub struct LibraryMove {
    /// Where it stands.
    pub step: Step,
    task: Option<Task<Outcome>>,
    live: Arc<Mutex<Live>>,
    /// Seconds since the last job finished, while the panel lingers.
    lingering: Option<f32>,
}

impl LibraryMove {
    /// Whether a job runs.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    fn live(&self) -> (Progress, f64) {
        let live = self.live.lock().map(|l| *l).unwrap_or_default();
        let elapsed = live.since.map_or(0.0, |t| t.elapsed().as_secs_f64());
        (live.progress, elapsed)
    }

    /// The row's value line.
    #[must_use]
    pub fn value(&self) -> String {
        let (progress, elapsed) = self.live();
        value_line(&self.step, progress, elapsed, current_root().as_ref())
    }

    /// Whether the shared progress panel shows the move: while it
    /// copies or deletes, and for a moment after.
    #[must_use]
    pub fn panel_showing(&self) -> bool {
        match self.step {
            Step::Copying { .. } | Step::Deleting => true,
            Step::Copied { .. } | Step::Done(_) | Step::Failed(_) => {
                self.lingering.is_some_and(|t| t < LINGER_S)
            }
            _ => false,
        }
    }

    /// The panel's line — what the row says, plus where to answer.
    #[must_use]
    pub fn panel_line(&self) -> String {
        let (progress, elapsed) = self.live();
        panel_text(&self.step, progress, elapsed)
    }

    /// The panel's bar, 0..=1: how far the current phase is.
    #[must_use]
    pub fn panel_bar(&self) -> f32 {
        match self.step {
            Step::Copying { .. } | Step::Deleting => fraction(self.live().0),
            _ => 1.0,
        }
    }

    /// Whether the last job failed (the panel's bar turns red).
    #[must_use]
    pub fn failed(&self) -> bool {
        matches!(self.step, Step::Failed(_))
    }

    /// ENTER on the row: the next step forward.
    pub fn confirm(&mut self) {
        self.lingering = None;
        match self.step.clone() {
            Step::Idle | Step::Done(_) | Step::Failed(_) => self.choose_folder(),
            Step::Confirm { to, .. } => self.start_copy(to),
            Step::Copied { from, to, .. } => self.start_delete(from, to),
            Step::Choosing | Step::Copying { .. } | Step::Deleting => {}
        }
    }

    /// LEFT on the row: back out of a question. Keeping the old copy
    /// is an answer too.
    pub fn decline(&mut self) {
        match &self.step {
            Step::Confirm { .. } => self.step = Step::Idle,
            Step::Copied { .. } => {
                self.step = Step::Done("the old copy is kept".to_owned());
            }
            Step::Done(_) | Step::Failed(_) => self.step = Step::Idle,
            _ => {}
        }
    }

    /// A folder was chosen (by the dialog or by a drop): ask.
    pub fn chosen(&mut self, to: PathBuf) {
        let Some(root) = current_root() else {
            self.step = Step::Failed("no library on this platform".to_owned());
            return;
        };
        if !root.reachable {
            // The library is gone (its drive unplugged): nothing to
            // move, but the player may point the game somewhere else.
            self.step = match beatbyte_library::location::choose(&data_dir(), &to) {
                Ok(()) => Step::Done(format!("the library is now {}", to.display())),
                Err(error) => Step::Failed(format!("cannot record it: {error}")),
            };
            return;
        }
        match relocate::inventory(&root.path) {
            Ok(files) => {
                let bytes = files.iter().map(|f| f.size).sum();
                if let Err(reason) = relocate::check_target(&root.path, &to, bytes, None) {
                    self.step = Step::Failed(reason);
                    return;
                }
                self.step = Step::Confirm {
                    to,
                    bytes,
                    files: files.len(),
                };
            }
            // A default library that was never created has nothing to
            // move: the new place simply becomes the library.
            Err(_) if !root.chosen && !root.path.exists() => {
                self.step = match beatbyte_library::location::choose(&data_dir(), &to) {
                    Ok(()) => Step::Done(format!("the library is now {}", to.display())),
                    Err(error) => Step::Failed(format!("cannot record it: {error}")),
                };
            }
            Err(reason) => self.step = Step::Failed(reason),
        }
    }

    fn choose_folder(&mut self) {
        if !cfg!(target_os = "macos") {
            self.step =
                Step::Failed("drop a folder onto the window while this row is selected".to_owned());
            return;
        }
        self.step = Step::Choosing;
        self.task =
            Some(AsyncComputeTaskPool::get().spawn(async move { Outcome::Chosen(pick_folder()) }));
    }

    fn start_copy(&mut self, to: PathBuf) {
        let Some(root) = current_root() else {
            return;
        };
        let Some(guard) = beatbyte_library::location::MovingGuard::acquire() else {
            self.step = Step::Failed("a move is already running".to_owned());
            return;
        };
        if let Ok(mut live) = self.live.lock() {
            *live = Live::default();
        }
        let live = Arc::clone(&self.live);
        let from = root.path;
        let target = to.clone();
        self.step = Step::Copying {
            from: from.clone(),
            to,
        };
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let _guard = guard;
            let result = relocate::copy_library(&from, &target, &mut |p| report(&live, p))
                .and_then(|moved| {
                    // The switch: only after every file was verified.
                    beatbyte_library::location::choose(&data_dir(), &target)
                        .map(|()| moved)
                        .map_err(|e| format!("copied, but cannot switch over: {e}"))
                });
            Outcome::Copied(result)
        }));
    }

    fn start_delete(&mut self, from: PathBuf, to: PathBuf) {
        self.step = Step::Deleting;
        if let Ok(mut live) = self.live.lock() {
            *live = Live::default();
        }
        let live = Arc::clone(&self.live);
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            Outcome::Deleted(relocate::delete_source(&from, &to, &mut |p| {
                report(&live, p);
            }))
        }));
    }
}

fn data_dir() -> PathBuf {
    crate::library::data_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn current_root() -> Option<beatbyte_library::location::Root> {
    crate::library::library_root()
}

/// The row's value for a step. Pure — tested.
#[must_use]
pub fn value_line(
    step: &Step,
    progress: Progress,
    elapsed_s: f64,
    root: Option<&beatbyte_library::location::Root>,
) -> String {
    let short = |p: &std::path::Path| crate::settings_ui::short_path(&p.display().to_string(), 28);
    match step {
        Step::Idle => match root {
            Some(root) if !root.reachable => format!("{} - NOT REACHABLE", short(&root.path)),
            Some(root) => short(&root.path),
            None => "-".to_owned(),
        },
        Step::Choosing => "CHOOSE A FOLDER IN THE DIALOG...".to_owned(),
        Step::Confirm { to, bytes, files } => format!(
            "MOVE {} ({files} FILES) TO {}? ENTER YES / LEFT NO",
            relocate::human(*bytes),
            short(to)
        ),
        Step::Copying { .. } | Step::Deleting => progress_line(progress, elapsed_s),
        Step::Copied { moved, .. } => format!(
            "MOVED {} - DELETE OLD COPY? ENTER YES / LEFT KEEP",
            relocate::human(moved.bytes)
        ),
        Step::Done(line) | Step::Failed(line) => line.to_uppercase(),
    }
}

/// The shared panel's line for a step. Pure — tested.
#[must_use]
pub fn panel_text(step: &Step, progress: Progress, elapsed_s: f64) -> String {
    match step {
        Step::Copying { .. } | Step::Deleting => {
            format!("LIBRARY: {}", progress_line(progress, elapsed_s))
        }
        Step::Copied { moved, .. } => format!(
            "LIBRARY MOVED ({} FILES, {}) - THE OLD COPY IS STILL THERE: SETTINGS > LIBRARY",
            moved.files,
            relocate::human(moved.bytes)
        ),
        Step::Done(line) | Step::Failed(line) => format!("LIBRARY: {}", line.to_uppercase()),
        _ => String::new(),
    }
}

/// How far the current phase is, 0..=1 — by bytes, by files when the
/// phase has no bytes to count.
#[must_use]
pub fn fraction(progress: Progress) -> f32 {
    let (done, total) = if progress.bytes_total > 0 {
        (progress.bytes_done as f64, progress.bytes_total as f64)
    } else {
        (progress.files_done as f64, progress.files_total as f64)
    };
    if total <= 0.0 {
        0.0
    } else {
        (done / total).clamp(0.0, 1.0) as f32
    }
}

/// Least time in a phase before a rate is believed: the first second
/// is the file system warming up, and a rate from it promises a time
/// left that jumps around.
pub const RATE_AFTER_S: f64 = 3.0;

/// The live line while a job runs: what it does, how far, how fast,
/// how long still. Pure — tested.
#[must_use]
pub fn progress_line(progress: Progress, elapsed_s: f64) -> String {
    let word = match progress.phase {
        Phase::Copying if progress.pass > 1 => {
            format!("CHECKING FOR CHANGES (PASS {})", progress.pass)
        }
        Phase::Copying => "COPYING".to_owned(),
        Phase::Verifying => "VERIFYING".to_owned(),
        Phase::Deleting => "DELETING THE OLD COPY".to_owned(),
    };
    let percent = (fraction(progress) * 100.0).floor() as u32;
    let mut line = format!(
        "{word} {percent}% - {}/{} FILES - {} OF {}",
        progress.files_done,
        progress.files_total,
        relocate::human(progress.bytes_done),
        relocate::human(progress.bytes_total)
    );
    // A later copy pass only compares, at a speed that says nothing
    // about the drive; the other phases read or write every byte.
    let measured = !(progress.phase == Phase::Copying && progress.pass > 1);
    if measured
        && elapsed_s >= RATE_AFTER_S
        && progress.bytes_done > 0
        && progress.bytes_done < progress.bytes_total
    {
        let rate = progress.bytes_done as f64 / elapsed_s;
        let left = (progress.bytes_total - progress.bytes_done) as f64 / rate;
        line.push_str(&format!(
            " - {}/S - {} LEFT",
            relocate::human(rate as u64),
            duration_words(left)
        ));
    }
    line
}

/// A time left as a player reads it: seconds under a minute, whole
/// minutes under an hour (rounded up — "1 MIN" must not linger at
/// zero), then hours and minutes.
#[must_use]
pub fn duration_words(seconds: f64) -> String {
    let s = seconds.max(0.0).ceil() as u64;
    if s < 60 {
        format!("{s} S")
    } else if s < 3600 {
        format!("{} MIN", s.div_ceil(60))
    } else {
        format!("{} H {:02} MIN", s / 3600, (s % 3600) / 60)
    }
}

/// The macOS folder dialog, through the system's own scripting — no
/// dependency, and the dialog the player knows. `None` on cancel.
fn pick_folder() -> Option<PathBuf> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Where should the BeatByte song library live?\")",
        ])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// The plugin: collects the jobs, takes a dropped folder on the row,
/// and says so in the browser when the library is not reachable.
pub struct LibraryMovePlugin;

impl Plugin for LibraryMovePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LibraryMove>()
            .add_systems(Update, poll_move)
            .add_systems(Update, library_move_drill.after(poll_move))
            .add_systems(
                Update,
                take_dropped_folder.run_if(in_state(crate::states::AppState::Settings)),
            )
            .add_systems(
                OnEnter(crate::states::AppState::SongSelect),
                say_when_unreachable,
            );
    }
}

/// Collect a finished job; rescan the library once it moved.
fn poll_move(
    time: Res<Time>,
    mut state: ResMut<LibraryMove>,
    builtins: Option<Res<crate::boot::BuiltinSongs>>,
    library: Option<ResMut<crate::library::SongLibrary>>,
) {
    if let Some(t) = state.lingering.as_mut() {
        *t += time.delta_secs();
    }
    let Some(task) = state.task.as_mut() else {
        return;
    };
    let Some(outcome) = bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    else {
        return;
    };
    state.task = None;
    if !matches!(outcome, Outcome::Chosen(_)) {
        state.lingering = Some(0.0);
    }
    match outcome {
        Outcome::Chosen(Some(to)) => state.chosen(to),
        Outcome::Chosen(None) => state.step = Step::Idle,
        Outcome::Copied(Ok(moved)) => {
            info!(
                "library moved: {} files, {} in {} passes",
                moved.files,
                relocate::human(moved.bytes),
                moved.passes
            );
            let (from, to) = match &state.step {
                Step::Copying { from, to } => (from.clone(), to.clone()),
                _ => (PathBuf::new(), PathBuf::new()),
            };
            state.step = Step::Copied { from, to, moved };
            if let (Some(builtins), Some(mut library)) = (builtins, library) {
                *library = crate::boot::scan_with_builtins(&builtins.0);
            }
        }
        Outcome::Copied(Err(reason)) => {
            warn!("library move failed: {reason}");
            state.step = Step::Failed(reason);
        }
        Outcome::Deleted(Ok(deleted)) => {
            state.step = Step::Done(if deleted.kept == 0 {
                format!("old copy removed ({} files)", deleted.removed)
            } else {
                format!(
                    "old copy removed; {} changed files kept there",
                    deleted.kept
                )
            });
        }
        Outcome::Deleted(Err(reason)) => state.step = Step::Failed(reason),
    }
}

/// A folder dropped while the LIBRARY row is selected is the answer
/// to "where?" — the gesture the watch folder already uses.
fn take_dropped_folder(
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
    cursor: Res<crate::settings_ui::SettingsCursor>,
    mut state: ResMut<LibraryMove>,
) {
    for drop in drops.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        if crate::settings_ui::library_row_selected(&cursor) && path_buf.is_dir() && !state.busy() {
            state.chosen(path_buf.clone());
        }
    }
}

/// The browser says so when the chosen library is not there — the
/// list would otherwise just look emptier than it should.
fn say_when_unreachable(mut status: ResMut<crate::import::ImportStatus>) {
    if let Some(root) = current_root()
        && !root.reachable
    {
        status.0 = format!(
            "the library at {} is not reachable - connect its drive",
            crate::settings_ui::short_path(&root.path.display().to_string(), 40)
        );
    }
}

/// `BEATBYTE_AUTOPILOT_LIBRARY_MOVE=<folder>`: the move, end to end,
/// through the same state machine the settings row drives. Run it with
/// `HOME` pointed at a scratch folder — it moves THAT library, and the
/// real one is never touched. Passes only when the game switched to
/// the new place, every file arrived with the checksum it left with,
/// and the delete left nothing of what was copied behind.
#[derive(Default)]
struct Drill {
    started: bool,
    from: PathBuf,
    expected: Vec<(String, String)>,
    deleting: bool,
    frames: u32,
}

fn library_move_drill(
    mut drill: Local<Drill>,
    state: Option<Res<State<crate::states::AppState>>>,
    mut moving: ResMut<LibraryMove>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(target) = std::env::var("BEATBYTE_AUTOPILOT_LIBRARY_MOVE") else {
        return;
    };
    // Wait for the menu: boot has scanned the library by then.
    if state.is_none_or(|s| *s.get() != crate::states::AppState::MainMenu) && !drill.started {
        return;
    }
    let fail = |exit: &mut MessageWriter<AppExit>, why: String| {
        error!("library move drill: {why}");
        crate::autopilot::deliver(exit, AppExit::error());
    };
    drill.frames += 1;
    if drill.frames > 60 * 600 {
        fail(&mut exit, "did not finish in time".to_owned());
        return;
    }
    if !drill.started {
        drill.started = true;
        let Some(root) = current_root() else {
            fail(&mut exit, "no library".to_owned());
            return;
        };
        drill.from = root.path.clone();
        drill.expected = relocate::inventory(&root.path)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| {
                let path = root.path.join(&e.rel);
                relocate::hash_file(&path).ok().map(|h| (e.rel, h))
            })
            .collect();
        info!(
            "library move drill: {} files from {} to {target}",
            drill.expected.len(),
            root.path.display()
        );
        moving.chosen(PathBuf::from(&target));
        if !matches!(moving.step, Step::Confirm { .. }) {
            fail(
                &mut exit,
                format!("no confirmation asked: {:?}", moving.step),
            );
            return;
        }
        moving.confirm();
        return;
    }
    match moving.step.clone() {
        Step::Copied { .. } if !drill.deleting => {
            let Some(root) = current_root() else {
                return;
            };
            if root.path != std::path::Path::new(&target) || !root.chosen {
                fail(
                    &mut exit,
                    format!("the game did not switch: {}", root.path.display()),
                );
                return;
            }
            for (rel, hash) in &drill.expected {
                let got = relocate::hash_file(&root.path.join(rel)).unwrap_or_default();
                if &got != hash {
                    fail(&mut exit, format!("{rel} did not arrive intact"));
                    return;
                }
            }
            info!(
                "library move drill: {} files verified in the new place",
                drill.expected.len()
            );
            drill.deleting = true;
            moving.confirm();
        }
        Step::Done(line) if drill.deleting => {
            let left = relocate::inventory(&drill.from).map_or(0, |l| l.len());
            if left != 0 {
                fail(
                    &mut exit,
                    format!("{left} files left in the old place: {line}"),
                );
                return;
            }
            info!("library move drill: PASSED ({line})");
            crate::autopilot::deliver(&mut exit, AppExit::Success);
        }
        Step::Failed(why) => fail(&mut exit, why),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_library::location::Root;

    fn root(reachable: bool) -> Root {
        Root {
            path: PathBuf::from("/Volumes/Drive/BeatByte"),
            chosen: true,
            reachable,
        }
    }

    #[test]
    fn every_step_says_what_it_is_and_what_the_keys_do() {
        let none = Progress::default();
        assert_eq!(
            value_line(&Step::Idle, none, 0.0, Some(&root(true))),
            "/Volumes/Drive/BeatByte"
        );
        assert!(value_line(&Step::Idle, none, 0.0, Some(&root(false))).ends_with("NOT REACHABLE"));
        let confirm = Step::Confirm {
            to: PathBuf::from("/Volumes/Drive/BeatByte"),
            bytes: 5 * 1_073_741_824,
            files: 4000,
        };
        let line = value_line(&confirm, none, 0.0, None);
        assert!(
            line.contains("5.0 GB") && line.contains("ENTER YES"),
            "{line}"
        );
        let copying = Progress {
            phase: Phase::Copying,
            pass: 1,
            bytes_done: 25 * 1_048_576,
            bytes_total: 100 * 1_048_576,
            files_done: 3,
            files_total: 12,
            copied: 3,
        };
        let moving = Step::Copying {
            from: PathBuf::new(),
            to: PathBuf::new(),
        };
        assert_eq!(
            value_line(&moving, copying, 0.0, None),
            "COPYING 25% - 3/12 FILES - 25 MB OF 100 MB"
        );
        let second = Progress { pass: 2, ..copying };
        assert!(value_line(&moving, second, 0.0, None).contains("PASS 2"));
        let copied = Step::Copied {
            from: PathBuf::new(),
            to: PathBuf::new(),
            moved: Moved {
                files: 1,
                bytes: 300 * 1_048_576,
                passes: 2,
            },
        };
        assert!(value_line(&copied, none, 0.0, None).contains("DELETE OLD COPY"));
    }

    /// The live line: every phase named, the rate and the time left
    /// only once there is something to measure, and never for a pass
    /// that only compares.
    #[test]
    fn the_live_line_says_how_far_how_fast_and_how_long() {
        let mb = 1_048_576;
        let half = Progress {
            phase: Phase::Copying,
            pass: 1,
            bytes_done: 50 * mb,
            bytes_total: 100 * mb,
            files_done: 6,
            files_total: 12,
            copied: 6,
        };
        // Too early to believe a rate: no speed, no time left.
        assert!(!progress_line(half, RATE_AFTER_S - 0.1).contains("LEFT"));
        // 50 MB in 10 s: 5 MB/s, the other 50 MB in 10 s.
        assert_eq!(
            progress_line(half, 10.0),
            "COPYING 50% - 6/12 FILES - 50 MB OF 100 MB - 5 MB/S - 10 S LEFT"
        );
        // A comparing pass is quick and says nothing about the drive.
        let comparing = Progress { pass: 2, ..half };
        assert!(!progress_line(comparing, 10.0).contains("/S"));
        // Verifying and deleting read every byte: they are measured.
        let verifying = Progress {
            phase: Phase::Verifying,
            ..half
        };
        let line = progress_line(verifying, 10.0);
        assert!(
            line.starts_with("VERIFYING 50%") && line.contains("LEFT"),
            "{line}"
        );
        let deleting = Progress {
            phase: Phase::Deleting,
            ..half
        };
        assert!(progress_line(deleting, 10.0).starts_with("DELETING THE OLD COPY 50%"));
        // Done means no time left to promise.
        let done = Progress {
            bytes_done: 100 * mb,
            files_done: 12,
            ..half
        };
        assert!(!progress_line(done, 10.0).contains("LEFT"));
        // The bar follows the bytes, the files when there are none.
        assert!((fraction(half) - 0.5).abs() < 1e-6);
        let empty = Progress {
            bytes_total: 0,
            bytes_done: 0,
            files_done: 1,
            files_total: 4,
            ..half
        };
        assert!((fraction(empty) - 0.25).abs() < 1e-6);
        assert!(fraction(Progress::default()).abs() < f32::EPSILON);
    }

    #[test]
    fn time_left_reads_as_a_player_reads_it() {
        assert_eq!(duration_words(0.2), "1 S");
        assert_eq!(duration_words(59.0), "59 S");
        assert_eq!(
            duration_words(61.0),
            "2 MIN",
            "rounded up, never 1 MIN at 1:01"
        );
        assert_eq!(duration_words(3599.0), "60 MIN");
        assert_eq!(duration_words(3600.0 + 5.0 * 60.0), "1 H 05 MIN");
        assert_eq!(duration_words(-3.0), "0 S");
    }

    /// The shared panel shows the move while it works and for a moment
    /// after, then lets go; a question being asked is not its business.
    #[test]
    fn the_panel_shows_the_work_and_lets_go() {
        let mut state = LibraryMove {
            step: Step::Copying {
                from: PathBuf::new(),
                to: PathBuf::new(),
            },
            ..LibraryMove::default()
        };
        assert!(state.panel_showing());
        state.step = Step::Copied {
            from: PathBuf::new(),
            to: PathBuf::new(),
            moved: Moved {
                files: 3447,
                bytes: 6 * 1_073_741_824,
                passes: 2,
            },
        };
        state.lingering = Some(0.0);
        assert!(state.panel_showing());
        assert!(
            state.panel_line().contains("SETTINGS > LIBRARY"),
            "{}",
            state.panel_line()
        );
        state.lingering = Some(LINGER_S + 0.1);
        assert!(!state.panel_showing());
        state.step = Step::Confirm {
            to: PathBuf::new(),
            bytes: 1,
            files: 1,
        };
        state.lingering = Some(0.0);
        assert!(
            !state.panel_showing(),
            "a question is asked on the row, not the panel"
        );
        state.step = Step::Failed("the target failed".to_owned());
        assert!(state.panel_showing() && state.failed());
    }

    /// LEFT never deletes and never starts anything: it backs out of
    /// a question, and after a copy it means KEEP.
    #[test]
    fn declining_keeps_everything() {
        let mut state = LibraryMove {
            step: Step::Copied {
                from: PathBuf::from("/old"),
                to: PathBuf::from("/x"),
                moved: Moved {
                    files: 1,
                    bytes: 1,
                    passes: 2,
                },
            },
            ..LibraryMove::default()
        };
        state.decline();
        assert_eq!(state.step, Step::Done("the old copy is kept".to_owned()));
        assert!(!state.busy());
        state.step = Step::Confirm {
            to: PathBuf::from("/x"),
            bytes: 1,
            files: 1,
        };
        state.decline();
        assert_eq!(state.step, Step::Idle);
    }
}
