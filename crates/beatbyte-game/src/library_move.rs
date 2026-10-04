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

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};

use beatbyte_library::relocate::{self, Moved, Progress};

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
        /// The new place.
        to: PathBuf,
    },
    /// Copied and switched; the old copy is still there.
    Copied {
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
    progress: Arc<Mutex<Progress>>,
}

impl LibraryMove {
    /// Whether a job runs.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }

    /// The row's value line. Pure over the step — tested.
    #[must_use]
    pub fn value(&self) -> String {
        let progress = self.progress.lock().map(|p| *p).unwrap_or_default();
        value_line(&self.step, progress, current_root().as_ref())
    }

    /// ENTER on the row: the next step forward.
    pub fn confirm(&mut self) {
        match self.step.clone() {
            Step::Idle | Step::Done(_) | Step::Failed(_) => self.choose_folder(),
            Step::Confirm { to, .. } => self.start_copy(to),
            Step::Copied { to, .. } => self.start_delete(to),
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
        if let Ok(mut progress) = self.progress.lock() {
            *progress = Progress::default();
        }
        let progress = Arc::clone(&self.progress);
        let from = root.path;
        let target = to.clone();
        self.step = Step::Copying { to };
        self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let _guard = guard;
            let result = relocate::copy_library(&from, &target, &mut |p| {
                if let Ok(mut shared) = progress.lock() {
                    *shared = p;
                }
            })
            .and_then(|moved| {
                // The switch: only after every file was verified.
                beatbyte_library::location::choose(&data_dir(), &target)
                    .map(|()| moved)
                    .map_err(|e| format!("copied, but cannot switch over: {e}"))
            });
            Outcome::Copied(result)
        }));
    }

    fn start_delete(&mut self, to: PathBuf) {
        self.step = Step::Deleting;
        self.task = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { Outcome::Deleted(relocate::delete_source(&to)) }),
        );
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
        Step::Copying { .. } => {
            let percent = progress
                .bytes_done
                .saturating_mul(100)
                .checked_div(progress.bytes_total)
                .map_or(0, |p| p.min(100));
            if progress.pass <= 1 {
                format!("COPYING {percent}%")
            } else {
                format!("CHECKING FOR CHANGES {percent}% (PASS {})", progress.pass)
            }
        }
        Step::Copied { moved, .. } => format!(
            "MOVED {} - DELETE OLD COPY? ENTER YES / LEFT KEEP",
            relocate::human(moved.bytes)
        ),
        Step::Deleting => "DELETING THE OLD COPY...".to_owned(),
        Step::Done(line) | Step::Failed(line) => line.to_uppercase(),
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
    mut state: ResMut<LibraryMove>,
    builtins: Option<Res<crate::boot::BuiltinSongs>>,
    library: Option<ResMut<crate::library::SongLibrary>>,
) {
    let Some(task) = state.task.as_mut() else {
        return;
    };
    let Some(outcome) = bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    else {
        return;
    };
    state.task = None;
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
            let to = match &state.step {
                Step::Copying { to } => to.clone(),
                _ => PathBuf::new(),
            };
            state.step = Step::Copied { to, moved };
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
        exit.write(AppExit::error());
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
            exit.write(AppExit::Success);
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
            value_line(&Step::Idle, none, Some(&root(true))),
            "/Volumes/Drive/BeatByte"
        );
        assert!(value_line(&Step::Idle, none, Some(&root(false))).ends_with("NOT REACHABLE"));
        let confirm = Step::Confirm {
            to: PathBuf::from("/Volumes/Drive/BeatByte"),
            bytes: 5 * 1_073_741_824,
            files: 4000,
        };
        let line = value_line(&confirm, none, None);
        assert!(
            line.contains("5.0 GB") && line.contains("ENTER YES"),
            "{line}"
        );
        let copying = Progress {
            pass: 1,
            bytes_done: 25,
            bytes_total: 100,
            copied: 3,
        };
        assert_eq!(
            value_line(&Step::Copying { to: PathBuf::new() }, copying, None),
            "COPYING 25%"
        );
        let second = Progress { pass: 2, ..copying };
        assert!(value_line(&Step::Copying { to: PathBuf::new() }, second, None).contains("PASS 2"));
        let copied = Step::Copied {
            to: PathBuf::new(),
            moved: Moved {
                files: 1,
                bytes: 300 * 1_048_576,
                passes: 2,
            },
        };
        assert!(value_line(&copied, none, None).contains("DELETE OLD COPY"));
    }

    /// LEFT never deletes and never starts anything: it backs out of
    /// a question, and after a copy it means KEEP.
    #[test]
    fn declining_keeps_everything() {
        let mut state = LibraryMove {
            step: Step::Copied {
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
