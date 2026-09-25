//! The performance bench: `BEATBYTE_BENCH=<scenario>`.
//!
//! Built on the autopilot, not beside it: the autopilot chooses the
//! song (`BEATBYTE_AUTOPILOT_SONG` / `…_DIFFICULTY`), plays it
//! perfectly and delivers its verdict as always; this module only
//! watches the frames go by and, when the song's playing phase ends,
//! writes what it saw to `BEATBYTE_BENCH_OUT` (default
//! `bench-result.json`). One run is one file; `tools/bench.sh`
//! repeats runs, adds the machine and the commit, and
//! `tools/bench-compare.py` sets two results side by side.
//!
//! # What is measured, and what is not
//!
//! - **Frame time** is the real (wall) delta between frames — under
//!   `BEATBYTE_UNCAPPED=1`, the cost of a frame rather than the
//!   display's pacing. Only frames in the playing phase, and only
//!   from [`WARMUP_S`] of song time on, count: the first seconds carry
//!   the song's own load and are measured separately as
//!   `song_start_ms`.
//! - **`cpu_main_ms`** is the main world's schedule, `First` to
//!   `Last`. With pipelined rendering the render world runs on its own
//!   thread in parallel, so this is the simulation's share, not the
//!   frame's.
//! - **`gpu_ms`** is the sum of the top-level passes' GPU time from
//!   Bevy's `RenderDiagnosticsPlugin`, added only in a bench run.
//!   Where the device offers no timestamp queries it stays empty, and
//!   the file says so (`null`) rather than inventing a zero.
//! - **Counts** (entities, notes on screen, particles) are sampled
//!   every [`COUNT_EVERY`] frames — counting is a walk over the world
//!   and must not become the thing being measured.
//! - **`song_start_ms`**: from entering gameplay to the first frame in
//!   which the song clock runs.
//!
//! No allocation per frame: the sample vectors are reserved for an
//! hour of frames at start, and a run longer than that stops adding
//! (and says so) rather than growing.

use std::time::Instant;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::audio_sys::GameClock;
use crate::states::{AppState, GamePhase};

/// Song seconds dropped at the start of every run.
pub const WARMUP_S: f64 = 5.0;
/// Frames between two counts.
pub const COUNT_EVERY: u32 = 30;
/// A frame over this missed a 60 Hz display.
pub const BUDGET_60_MS: f32 = 1000.0 / 60.0;
/// Room for an hour at 240 fps.
const CAPACITY: usize = 240 * 3600;

/// Forward jumps of the song clock the autopilot saw. In a normal
/// harness run one fails the run; in a bench run it is counted here
/// and reported, because it is exactly what a bench is looking for.
pub static CLOCK_JUMPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether a bench run was asked to measure full screen at the
/// display's native resolution (`BEATBYTE_BENCH_FULLSCREEN`).
#[must_use]
pub fn fullscreen() -> bool {
    active() && std::env::var_os("BEATBYTE_BENCH_FULLSCREEN").is_some()
}

/// Whether this run is a bench run.
#[must_use]
pub fn active() -> bool {
    std::env::var_os("BEATBYTE_BENCH").is_some()
}

/// The numbers of one distribution. Pure — tested.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Stats {
    /// Samples.
    pub n: usize,
    /// Mean.
    pub avg: f32,
    /// Median.
    pub median: f32,
    /// 95th percentile.
    pub p95: f32,
    /// 99th percentile.
    pub p99: f32,
    /// The worst sample.
    pub max: f32,
}

/// The distribution of `samples` (which it sorts). `None` when empty.
/// Percentiles by nearest rank: p99 of 100 samples is the 99th, not
/// an interpolation between two frames that happened.
#[must_use]
pub fn stats(samples: &mut [f32]) -> Option<Stats> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(f32::total_cmp);
    let n = samples.len();
    let rank = |p: f32| samples[((p * n as f32).ceil() as usize).clamp(1, n) - 1];
    Some(Stats {
        n,
        avg: samples.iter().sum::<f32>() / n as f32,
        median: rank(0.5),
        p95: rank(0.95),
        p99: rank(0.99),
        max: samples[n - 1],
    })
}

/// How many frames went over a budget. Pure — tested.
#[must_use]
pub fn over(samples: &[f32], budget_ms: f32) -> usize {
    samples.iter().filter(|ms| **ms > budget_ms).count()
}

/// What a bench run collects.
#[derive(Resource)]
pub struct Bench {
    scenario: String,
    frame_ms: Vec<f32>,
    cpu_ms: Vec<f32>,
    gpu_ms: Vec<f32>,
    entities_max: usize,
    notes_max: usize,
    particles_max: usize,
    counted: u32,
    entered: Option<Instant>,
    song_start_ms: Option<f32>,
    frame_began: Option<Instant>,
    truncated: bool,
    written: bool,
}

impl Bench {
    fn new(scenario: String) -> Bench {
        Bench {
            scenario,
            frame_ms: Vec::with_capacity(CAPACITY),
            cpu_ms: Vec::with_capacity(CAPACITY),
            gpu_ms: Vec::with_capacity(CAPACITY),
            entities_max: 0,
            notes_max: 0,
            particles_max: 0,
            counted: 0,
            entered: None,
            song_start_ms: None,
            frame_began: None,
            truncated: false,
            written: false,
        }
    }
}

fn mark_frame_start(mut bench: ResMut<Bench>) {
    bench.frame_began = Some(Instant::now());
}

fn mark_gameplay_entered(mut bench: ResMut<Bench>) {
    bench.entered = Some(Instant::now());
    bench.song_start_ms = None;
}

/// The sum of the top-level passes' GPU time this frame, if the
/// device measures it.
fn gpu_time(store: &DiagnosticsStore) -> Option<f32> {
    let mut sum = 0.0f64;
    let mut any = false;
    for diagnostic in store.iter() {
        let path = diagnostic.path().as_str();
        // `render/<pass>/elapsed_gpu` — one level deep only: nested
        // passes are already inside their parent's time.
        if path.starts_with("render/")
            && path.ends_with("/elapsed_gpu")
            && path.matches('/').count() == 2
            && let Some(value) = diagnostic.value()
        {
            sum += value;
            any = true;
        }
    }
    // A device without timestamp queries reports zeros, not nothing
    // (seen on the M1 Pro): a zero GPU frame is no measurement.
    (any && sum > 0.0).then_some(sum as f32)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI, not an API
fn record_frame(
    mut bench: ResMut<Bench>,
    time: Res<Time<Real>>,
    phase: Option<Res<State<GamePhase>>>,
    game_clock: Res<GameClock>,
    virtual_time: Res<Time>,
    store: Option<Res<DiagnosticsStore>>,
    entities: Query<()>,
    notes: Query<
        (),
        Or<(
            With<crate::gameplay::notes::NoteSprite>,
            With<crate::gameplay::stage3d::Note3d>,
        )>,
    >,
    particles: Query<(), With<crate::gameplay::spark3d::Spark3d>>,
) {
    let cpu = bench
        .frame_began
        .map(|began| began.elapsed().as_secs_f32() * 1000.0);
    let playing = phase.is_some_and(|phase| *phase.get() == GamePhase::Playing);
    if !playing {
        return;
    }
    let Some(song) = game_clock.song_time(&virtual_time) else {
        return;
    };
    if bench.song_start_ms.is_none()
        && let Some(entered) = bench.entered
    {
        bench.song_start_ms = Some(entered.elapsed().as_secs_f32() * 1000.0);
    }
    if song < WARMUP_S {
        return;
    }
    if bench.frame_ms.len() >= CAPACITY {
        bench.truncated = true;
        return;
    }
    let delta = time.delta_secs() * 1000.0;
    if delta > 0.0 {
        bench.frame_ms.push(delta);
    }
    if let Some(cpu) = cpu {
        bench.cpu_ms.push(cpu);
    }
    if let Some(gpu) = store.as_deref().and_then(gpu_time) {
        bench.gpu_ms.push(gpu);
    }
    bench.counted += 1;
    if bench.counted.is_multiple_of(COUNT_EVERY) {
        bench.entities_max = bench.entities_max.max(entities.iter().count());
        bench.notes_max = bench.notes_max.max(notes.iter().count());
        bench.particles_max = bench.particles_max.max(particles.iter().count());
    }
}

/// Write the result the moment the playing phase is over.
fn write_result(
    mut bench: ResMut<Bench>,
    windows: Query<&Window, With<PrimaryWindow>>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    settings: Res<crate::config::Settings>,
    song: Option<Res<crate::boot::LoadedSong>>,
    difficulty: Res<crate::song_select::SelectedDifficulty>,
) {
    if bench.written || bench.frame_ms.is_empty() {
        return;
    }
    bench.written = true;
    let mut frames = bench.frame_ms.clone();
    let over_60 = over(&frames, BUDGET_60_MS);
    let frame = stats(&mut frames);
    let cpu = stats(&mut bench.cpu_ms.clone());
    let gpu = stats(&mut bench.gpu_ms.clone());
    let (width, height, scale) = windows.single().map_or((0, 0, 1.0), |window| {
        (
            window.physical_width(),
            window.physical_height(),
            window.scale_factor(),
        )
    });
    let (gpu_name, backend) = adapter.map_or_else(
        || ("unknown".to_owned(), "unknown".to_owned()),
        |info| (info.0.name.clone(), format!("{:?}", info.0.backend)),
    );
    let result = serde_json::json!({
        "scenario": bench.scenario,
        "song": song.map_or_else(String::new, |song| song.chart.song.title.clone()),
        "difficulty": format!("{}", difficulty.0),
        "frame_ms": frame,
        "frames_over_16_7": over_60,
        "cpu_main_ms": cpu,
        "gpu_ms": gpu,
        "entities_max": bench.entities_max,
        "notes_visible_max": bench.notes_max,
        "particles_max": bench.particles_max,
        "song_start_ms": bench.song_start_ms,
        "clock_jumps": CLOCK_JUMPS.load(std::sync::atomic::Ordering::Relaxed),
        "probes": std::env::var("BEATBYTE_PROBE").unwrap_or_default(),
        "warmup_s": WARMUP_S,
        "truncated": bench.truncated,
        "gpu": gpu_name,
        "backend": backend,
        "resolution": [width, height],
        "scale_factor": scale,
        "uncapped": std::env::var_os("BEATBYTE_UNCAPPED").is_some(),
        "settings": {
            "stage_3d": settings.stage_3d,
            "fx_intensity": settings.fx_intensity,
            "particles": settings.particles,
            "reduced_flashing": settings.reduced_flashing,
        },
        "version": crate::VERSION,
    });
    let path = std::env::var("BEATBYTE_BENCH_OUT").unwrap_or_else(|_| "bench-result.json".into());
    match serde_json::to_string_pretty(&result)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&path, text))
    {
        Ok(()) => info!(
            "bench: wrote {path} — p99 {:.2} ms, {over_60} frame(s) over 16.7 ms",
            frame.map_or(0.0, |f| f.p99)
        ),
        Err(error) => error!("bench: could not write {path}: {error}"),
    }
}

/// Which cost probes this run switches off (`BEATBYTE_PROBE`, a
/// comma list): `msaa`, `bloom`, `shadows`, `lights`. Measurement
/// only — each takes one cost out of the frame so its price can be
/// read off the difference. Never read outside a bench run.
fn probe(name: &str) -> bool {
    std::env::var("BEATBYTE_PROBE")
        .is_ok_and(|list| list.split(',').any(|item| item.trim() == name))
}

/// Apply the probes to whatever was just spawned. Components only,
/// on the frame they appear.
#[allow(clippy::type_complexity)]
fn apply_probes(
    mut commands: Commands,
    cameras: Query<Entity, Added<Camera>>,
    mut directional: Query<&mut DirectionalLight, Added<DirectionalLight>>,
    lights: Query<Entity, Or<(Added<PointLight>, Added<SpotLight>)>>,
) {
    for camera in &cameras {
        if probe("msaa") {
            commands.entity(camera).insert(Msaa::Off);
        }
        if probe("bloom") {
            commands
                .entity(camera)
                .remove::<bevy::post_process::bloom::Bloom>();
        }
    }
    if probe("shadows") {
        for mut light in &mut directional {
            light.shadow_maps_enabled = false;
        }
    }
    if probe("lights") {
        for light in &lights {
            commands
                .entity(light)
                .remove::<PointLight>()
                .remove::<SpotLight>();
        }
    }
}

/// The `hidpi` probe: render at one pixel per point — a quarter of
/// a Retina display's pixels, the stand-in for a render scale of 50 %
/// until one exists. Anything else passes through.
#[must_use]
pub fn probe_resolution(
    resolution: bevy::window::WindowResolution,
) -> bevy::window::WindowResolution {
    if active() && probe("hidpi") {
        resolution.with_scale_factor_override(1.0)
    } else {
        resolution
    }
}

/// The bench, wired in only when `BEATBYTE_BENCH` is set.
pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        let Ok(scenario) = std::env::var("BEATBYTE_BENCH") else {
            return;
        };
        // GPU timestamps cost something themselves; only on request.
        if std::env::var_os("BEATBYTE_BENCH_GPU").is_some() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
        app.insert_resource(Bench::new(scenario))
            .add_systems(First, mark_frame_start)
            .add_systems(Last, record_frame)
            .add_systems(PostUpdate, apply_probes)
            .add_systems(OnEnter(AppState::Gameplay), mark_gameplay_entered)
            .add_systems(OnExit(GamePhase::Playing), write_result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_use_nearest_rank_and_the_real_worst_frame() {
        let mut samples: Vec<f32> = (1..=100).map(|i| i as f32).collect();
        samples.reverse();
        let s = stats(&mut samples).expect("some");
        assert_eq!(s.n, 100);
        assert!((s.avg - 50.5).abs() < 1e-4);
        assert_eq!(s.median, 50.0);
        assert_eq!(s.p95, 95.0);
        assert_eq!(s.p99, 99.0);
        assert_eq!(s.max, 100.0);
        // One sample: every percentile is it.
        let one = stats(&mut [7.0]).expect("one");
        assert_eq!((one.median, one.p99, one.max), (7.0, 7.0, 7.0));
        assert!(stats(&mut []).is_none());
        // A single spike in 1000 frames is the max, not the p99.
        let mut spiky = vec![6.0f32; 999];
        spiky.push(40.0);
        let s = stats(&mut spiky).expect("some");
        assert_eq!(s.p99, 6.0);
        assert_eq!(s.max, 40.0);
    }

    #[test]
    fn frames_over_the_budget_are_counted_strictly() {
        assert_eq!(over(&[16.0, 16.6, 16.8, 30.0], BUDGET_60_MS), 2);
        assert_eq!(over(&[], BUDGET_60_MS), 0);
    }
}
