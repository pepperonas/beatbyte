//! Bridge to the explicitly installed local audio-to-chart runtime.
//!
//! A device registration selects the runtime, never the working directory.
//! Output stays in scratch space until the native parser accepts all four
//! difficulties. A failed AI run never falls back to another generator.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;

struct Jobs {
    closing: bool,
    imports: usize,
    children: Vec<u32>,
}

static JOBS: Mutex<Jobs> = Mutex::new(Jobs {
    closing: false,
    imports: 0,
    children: Vec::new(),
});

/// Keeps shutdown waiting until the import's commit or rollback is finished.
pub(crate) struct ImportGuard;

impl ImportGuard {
    pub(crate) fn start() -> Result<Self, String> {
        let mut jobs = JOBS.lock().map_err(|e| e.to_string())?;
        if jobs.closing {
            return Err("import cancelled: Beatbyte is closing".to_owned());
        }
        jobs.imports += 1;
        Ok(Self)
    }
}

impl Drop for ImportGuard {
    fn drop(&mut self) {
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.imports = jobs.imports.saturating_sub(1);
        }
    }
}

/// Stops Python and Demucs before the app exits, allowing rollback to finish.
pub(crate) fn shutdown() {
    let children = if let Ok(mut jobs) = JOBS.lock() {
        jobs.closing = true;
        jobs.children.clone()
    } else {
        Vec::new()
    };
    for pid in children {
        kill_tree(pid);
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if JOBS
            .lock()
            .is_ok_and(|jobs| jobs.imports == 0 && jobs.children.is_empty())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Progress shared by the worker and the existing import overlay.
#[derive(Clone, Default, bevy::prelude::Resource)]
pub(crate) struct Progress(Arc<Mutex<(String, f32)>>);

impl Progress {
    pub(crate) fn set(&self, label: &str, fraction: f32) {
        if let Ok(mut state) = self.0.lock() {
            *state = (label.to_owned(), fraction.clamp(0.0, 1.0));
        }
    }

    pub(crate) fn get(&self) -> (String, f32) {
        self.0.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[derive(Deserialize)]
struct Registration {
    protocol: u32,
    root: PathBuf,
}

/// An explicitly registered Python runtime and its project directory.
pub(crate) struct Runtime {
    python: PathBuf,
    root: PathBuf,
}

impl Runtime {
    pub(crate) fn registered(data: &Path) -> Result<Option<Self>, String> {
        let path = data.join("song-chart-runtime.json");
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read AI charting registration: {e}")),
        };
        let registration: Registration = serde_json::from_str(&text)
            .map_err(|_| "AI charting registration is invalid; run setup again".to_owned())?;
        if registration.protocol != 1 || !registration.root.is_absolute() {
            return Err("AI charting registration is unsupported; run setup again".to_owned());
        }
        let root = registration.root;
        let python = root.join(if cfg!(windows) {
            ".venv/Scripts/python.exe"
        } else {
            ".venv/bin/python"
        });
        if !python.is_file() || !root.join("bbchart/__main__.py").is_file() {
            return Err("AI charting installation is missing; run setup again".to_owned());
        }
        Ok(Some(Self { python, root }))
    }

    pub(crate) fn generate(
        &self,
        source: &Path,
        title: &str,
        artist: &str,
        data: &Path,
        progress: &Progress,
    ) -> Result<Generated, String> {
        let jobs = data.join("song-chart-jobs");
        std::fs::create_dir_all(&jobs).map_err(|e| e.to_string())?;
        let id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let scratch = Scratch(jobs.join(&id));
        std::fs::create_dir(&scratch.0).map_err(|e| e.to_string())?;
        let output = scratch.0.join("output");
        let state = scratch.0.join("progress.json");
        let log_path = jobs.join(format!("{id}.log"));
        let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut command = Command::new(&self.python);
        command
            .current_dir(&self.root)
            .args(["-m", "bbchart", "game-import"])
            .arg(source)
            .arg("--out")
            .arg(&output)
            .arg("--title")
            .arg(title)
            .arg("--artist")
            .arg(artist)
            .args(["--mode", "ml", "--no-preview", "--progress-file"])
            .arg(&state)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log));
        progress.set("AI: starting chart analysis", 0.0);
        let result = run(&mut command, &state, progress, Duration::from_secs(1800))?;
        if !result {
            return Err(format!(
                "AI charting failed: {} (log: {})",
                log_tail(&log_path),
                log_path.display()
            ));
        }
        let chart = beatbyte_chart::load_chart_file(&output.join("chart.json"))
            .map_err(|e| format!("AI chart rejected: {e}"))?;
        validate_levels(&chart)?;
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output.join("report.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output.join("config.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(Generated {
            chart,
            evidence: serde_json::json!({
                "generator": "beatbyte-song-chart-v1",
                "report": report,
                "config": config,
            }),
        })
    }
}

pub(crate) struct Generated {
    pub(crate) chart: beatbyte_chart::ChartFile,
    pub(crate) evidence: serde_json::Value,
}

fn validate_levels(chart: &beatbyte_chart::ChartFile) -> Result<(), String> {
    use beatbyte_core::Difficulty::{Easy, Expert, Hard, Medium};
    if chart.charts.len() != 4
        || [Easy, Medium, Hard, Expert].iter().any(|d| {
            !chart
                .charts
                .iter()
                .any(|c| c.difficulty == *d && !c.notes.is_empty())
        })
        || chart
            .validate()
            .iter()
            .any(|issue| issue.severity == beatbyte_chart::Severity::Error)
    {
        return Err("AI chart must contain four valid, nonempty difficulties".to_owned());
    }
    Ok(())
}

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        // Demucs is a child of Python. Stop the whole job on timeout,
        // otherwise a failed import could keep the GPU and file lock busy.
        kill_tree(self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.children.retain(|pid| *pid != self.0.id());
        }
    }
}

fn kill_tree(pid: u32) {
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[derive(Deserialize)]
struct Update {
    phase: String,
    fraction: f32,
}

fn phase_label(phase: &str) -> Option<&'static str> {
    match phase {
        "decoding" => Some("AI: reading audio"),
        "beats" => Some("AI: finding beats"),
        "instruments" => Some("AI: separating instruments"),
        "notes" => Some("AI: detecting notes"),
        "patterns" => Some("AI: creating four difficulties"),
        "validating" | "done" => Some("AI: checking chart"),
        _ => None,
    }
}

fn run(
    command: &mut Command,
    state: &Path,
    progress: &Progress,
    timeout: Duration,
) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut worker = {
        let mut jobs = JOBS.lock().map_err(|e| e.to_string())?;
        if jobs.closing {
            return Err("AI charting cancelled: Beatbyte is closing".to_owned());
        }
        let child = command
            .spawn()
            .map_err(|e| format!("AI charting cannot start: {e}"))?;
        jobs.children.push(child.id());
        Worker(child)
    };
    let started = Instant::now();
    loop {
        if let Some(status) = worker.0.try_wait().map_err(|e| e.to_string())? {
            return Ok(status.success());
        }
        if started.elapsed() > timeout {
            return Err("AI charting timed out; the import can be retried".to_owned());
        }
        if let Ok(text) = std::fs::read_to_string(state)
            && text.len() <= 1024
            && let Ok(update) = serde_json::from_str::<Update>(&text)
            && update.fraction.is_finite()
            && let Some(label) = phase_label(&update.phase)
        {
            progress.set(label, update.fraction.min(0.97));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn log_tail(path: &Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else {
        return "see import log".to_owned();
    };
    let _ = file.seek(SeekFrom::End(-2048));
    let mut bytes = Vec::new();
    let _ = file.take(2048).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("see import log")
        .chars()
        .take(180)
        .collect()
}

/// AI output is already instrument-charted; the old upgrade must not replace it.
pub(crate) fn owns_chart(folder: &Path) -> bool {
    std::fs::read(folder.join("chart-generation.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value["generator"] == "beatbyte-song-chart-v1")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch() -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "bb-chart-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Scratch(path)
    }

    #[test]
    fn registration_is_explicit_and_broken_installations_do_not_fall_back() {
        let temp = scratch();
        assert!(Runtime::registered(&temp.0).unwrap().is_none());
        let record = temp.0.join("song-chart-runtime.json");
        for text in [
            "not json".to_owned(),
            serde_json::json!({"protocol":2,"root":temp.0}).to_string(),
            serde_json::json!({"protocol":1,"root":"relative"}).to_string(),
            serde_json::json!({"protocol":1,"root":temp.0}).to_string(),
        ] {
            std::fs::write(&record, text).unwrap();
            assert!(Runtime::registered(&temp.0).is_err());
        }
    }

    #[test]
    fn progress_only_accepts_known_stages_and_chart_ownership_is_explicit() {
        assert_eq!(phase_label("notes"), Some("AI: detecting notes"));
        assert_eq!(phase_label("arbitrary command"), None);
        let temp = scratch();
        assert!(!owns_chart(&temp.0));
        std::fs::write(temp.0.join("chart-generation.json"), "{}").unwrap();
        assert!(!owns_chart(&temp.0));
        std::fs::write(
            temp.0.join("chart-generation.json"),
            r#"{"generator":"beatbyte-song-chart-v1"}"#,
        )
        .unwrap();
        assert!(owns_chart(&temp.0));
    }

    #[test]
    fn a_successful_process_must_supply_four_valid_nonempty_levels() {
        let mut chart = beatbyte_chart::ChartFile::from_json(
            r#"{"format_version":1,"song":{"title":"Test","artist":"Test","audio":"song.wav","bpm":120,"duration_s":10},"charts":[]}"#,
        ).unwrap();
        assert!(validate_levels(&chart).is_err());
        for difficulty in ["easy", "medium", "hard", "expert"] {
            chart.charts.push(
                serde_json::from_value(serde_json::json!({
                    "difficulty":difficulty,"lanes":5,"notes":[{"time":1,"lane":0}]
                }))
                .unwrap(),
            );
        }
        assert!(validate_levels(&chart).is_ok());
        chart.charts[3].notes[0].lane = 8;
        assert!(validate_levels(&chart).is_err());
        chart.charts[3].notes.clear();
        assert!(validate_levels(&chart).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn worker_reports_progress_and_nonzero_exit_and_kills_a_timed_out_job() {
        let temp = scratch();
        let state = temp.0.join("state.json");
        std::fs::write(&state, r#"{"phase":"instruments","fraction":0.35}"#).unwrap();
        let progress = Progress::default();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 0.4; exit 7"]);
        assert!(!run(&mut command, &state, &progress, Duration::from_secs(3)).unwrap());
        assert_eq!(
            progress.get(),
            ("AI: separating instruments".to_owned(), 0.35)
        );
        let mut command = Command::new("/bin/sh");
        let marker = temp.0.join("orphan-ran");
        command
            .args(["-c", "(sleep 0.6; touch \"$1\") & wait", "worker"])
            .arg(&marker);
        let start = Instant::now();
        assert!(run(&mut command, &state, &progress, Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(800));
        assert!(
            !marker.exists(),
            "the child process group must be stopped too"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_kills_workers_and_waits_for_import_rollback() {
        // The shutdown flag is process-wide: use an isolated test runner.
        if std::env::var_os("BBCHART_TEST_SHUTDOWN").is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "song_chart::tests::shutdown_kills_workers_and_waits_for_import_rollback",
                    "--nocapture",
                ])
                .env("BBCHART_TEST_SHUTDOWN", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let temp = scratch();
        let state = temp.0.join("progress.json");
        let handle = std::thread::spawn(move || {
            let _import = ImportGuard::start().unwrap();
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "sleep 30 & wait"]);
            run(
                &mut command,
                &state,
                &Progress::default(),
                Duration::from_secs(60),
            )
        });
        let start = Instant::now();
        while JOBS.lock().unwrap().children.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        shutdown();
        assert!(!handle.join().unwrap().unwrap());
        {
            let jobs = JOBS.lock().unwrap();
            assert_eq!(jobs.imports, 0);
            assert!(jobs.children.is_empty());
        }
        assert!(ImportGuard::start().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn real_subprocess_uses_literal_arguments_and_rejects_incomplete_output() {
        use std::os::unix::fs::PermissionsExt;
        let temp = scratch();
        let python = temp.0.join("fake-python");
        // Only log argv; a successful exit without valid output must fail.
        std::fs::write(&python, "#!/bin/sh\nprintf '%s\\n' \"$@\" > argv\nexit 0\n").unwrap();
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runtime = Runtime {
            python,
            root: temp.0.clone(),
        };
        let title = "spaces ' ; $(never-run)";
        assert!(
            runtime
                .generate(
                    Path::new("/audio with spaces.m4a"),
                    title,
                    "Artist",
                    &temp.0,
                    &Progress::default()
                )
                .is_err()
        );
        let argv = std::fs::read_to_string(temp.0.join("argv")).unwrap();
        assert!(argv.lines().any(|line| line == title));
        assert!(argv.lines().any(|line| line == "/audio with spaces.m4a"));
        assert!(argv.lines().any(|line| line == "game-import"));
        assert!(argv.lines().any(|line| line == "--no-preview"));
    }
}
