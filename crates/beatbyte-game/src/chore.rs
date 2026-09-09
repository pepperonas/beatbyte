//! One long job at a time, run off the main thread and reported
//! wherever the player is.
//!
//! The song search grew this shape first: a task in the pool, a line
//! of prose, a bar on the import overlay, and a poll registered
//! globally so leaving the browser cannot strand it. A chart redesign
//! wants exactly the same thing, and so will the next job of its kind
//! — so the shape lives here rather than being copied.
//!
//! What it is NOT: a queue. One chore runs at a time on purpose. The
//! jobs it carries decode and re-analyse whole recordings, and a
//! browser where holding a key starts a dozen of those is a browser
//! that eats the machine.

use bevy::prelude::*;

/// How long the overlay keeps a finished chore up.
pub const LINGER_S: f32 = 6.0;

/// How long a chore is assumed to take, for the bar's creep.
///
/// A redesign of a four-minute song runs about this long on this
/// machine. It is an expectation, not a promise: the bar creeps
/// toward the end without arriving, exactly as the search's does, so
/// a job that takes twice as long still reads as working rather than
/// as finished-but-stuck.
pub const EXPECTED_S: f32 = 45.0;

/// The one running job.
#[derive(Resource, Default)]
pub struct Chore {
    /// The task, while there is one.
    task: Option<bevy::tasks::Task<Result<String, String>>>,
    /// What the overlay shows.
    pub line: String,
    /// How long it has been running.
    pub elapsed_s: f32,
    /// Whether the finished job succeeded.
    pub ok: bool,
    /// Seconds since it finished.
    pub since_finished: f32,
    /// Whether anything has run this session.
    pub ran: bool,
    /// Set for one frame after a job succeeds, so a caller can act on
    /// it — rescanning the library, say.
    pub just_finished: bool,
}

impl Chore {
    /// Whether a job is in flight.
    #[must_use]
    pub fn running(&self) -> bool {
        self.task.is_some()
    }

    /// Whether the overlay has anything to show.
    #[must_use]
    pub fn showing(&self) -> bool {
        self.ran && (self.running() || self.since_finished < LINGER_S)
    }

    /// Where the bar stands.
    #[must_use]
    pub fn bar(&self) -> f32 {
        if self.running() {
            creeping(self.elapsed_s)
        } else {
            1.0
        }
    }

    /// Start one. Refused, and says so, while another runs.
    ///
    /// # Errors
    /// When a chore is already in flight.
    pub fn start<F>(&mut self, line: impl Into<String>, job: F) -> Result<(), &'static str>
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if self.running() {
            return Err("one job at a time — this one is still running");
        }
        self.line = line.into();
        self.elapsed_s = 0.0;
        self.since_finished = 0.0;
        self.ran = true;
        self.ok = false;
        self.just_finished = false;
        self.task = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { job() }));
        Ok(())
    }
}

/// Where the bar stands after `elapsed` seconds of an unmeasured job.
///
/// Asymptotic: it never reaches the end, because a bar that arrives
/// while the work goes on is a lie and a bar that stands still reads
/// as a hang. Pure — tested.
#[must_use]
pub fn creeping(elapsed_s: f32) -> f32 {
    1.0 - (-elapsed_s / EXPECTED_S).exp()
}

/// The chore runner: polled wherever the player is.
pub struct ChorePlugin;

impl Plugin for ChorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chore>().add_systems(Update, poll_chore);
    }
}

/// Advance the clock, collect the result, and say so.
pub fn poll_chore(
    time: Res<Time>,
    mut chore: ResMut<Chore>,
    mut status: ResMut<crate::import::ImportStatus>,
    builtins: Option<Res<crate::boot::BuiltinSongs>>,
    library: Option<ResMut<crate::library::SongLibrary>>,
) {
    let delta = time.delta_secs();
    // The flag lives for exactly one frame.
    chore.just_finished = false;
    if !chore.running() {
        if chore.ran {
            chore.since_finished += delta;
        }
        return;
    }
    chore.elapsed_s += delta;
    let Some(task) = chore.task.as_mut() else {
        return;
    };
    let Some(result) = bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(task))
    else {
        return;
    };
    let ok = result.is_ok();
    let line = match result {
        Ok(line) => line,
        Err(reason) => reason,
    };
    status.0.clone_from(&line);
    chore.line = line;
    chore.ok = ok;
    chore.since_finished = 0.0;
    chore.just_finished = ok;
    chore.task = None;
    // A job that changed a song on disk has to reach the browser, or
    // the player is looking at what was there before it ran — the
    // same lesson the song search learned.
    if ok && let (Some(builtins), Some(mut library)) = (builtins, library) {
        *library = crate::boot::scan_with_builtins(&builtins.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_creeps_toward_the_end_without_arriving() {
        assert!(creeping(0.0).abs() < 1e-6, "it starts at nothing");
        assert!(creeping(1.0) > 0.0, "and moves at once");
        assert!(creeping(10.0) < creeping(30.0), "forward while it runs");
        assert!(creeping(EXPECTED_S * 4.0) < 1.0, "but never arrives");
        // A job running well past its expectation still reads as
        // working rather than as finished.
        assert!(creeping(EXPECTED_S * 4.0) > 0.9);
    }

    #[test]
    fn a_finished_chore_lingers_and_then_goes() {
        let mut chore = Chore::default();
        assert!(!chore.showing(), "nothing before the first job");
        chore.ran = true;
        chore.since_finished = 0.0;
        assert!(chore.showing(), "a fresh result is shown");
        chore.since_finished = LINGER_S + 0.1;
        assert!(!chore.showing(), "and then it goes");
        // Not running: the bar is full, which is what a finished job
        // should leave behind.
        assert!((chore.bar() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn one_job_at_a_time_and_the_second_is_told_why() {
        // A real pool, so this exercises `start` rather than a
        // stand-in for it: the browser is a place where a held key
        // could otherwise start a dozen full re-analyses.
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let mut chore = Chore::default();
        let (block, wait) = std::sync::mpsc::channel::<()>();
        assert!(
            chore
                .start("first", move || {
                    let _ = wait.recv();
                    Ok("done".to_owned())
                })
                .is_ok(),
            "the first job starts"
        );
        assert!(chore.running());
        assert!(chore.line.contains("first"), "and says what it is");
        let refused = chore.start("second", || Ok("never".to_owned()));
        let Err(reason) = refused else {
            panic!("the second job must be refused");
        };
        assert!(
            reason.contains("one job at a time"),
            "and told why, in words a status line can carry: {reason}"
        );
        assert!(chore.line.contains("first"), "the refusal changed nothing");
        drop(block);
    }
}
