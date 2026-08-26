//! # beatbyte-game
//!
//! The presentation layer of BeatByte: Bevy plugins for gameplay
//! rendering, UI, effects and state management. Gameplay *rules* live
//! in [`beatbyte_core`]; this crate turns them into pixels and sound.

pub mod audio_sys;
pub mod autopilot;
pub mod boot;
pub mod calibration;
pub mod config;
pub mod controls;
pub mod controls_ui;
pub mod editor_ui;
pub mod gameplay;
pub mod import;
mod input_test;
pub mod library;
pub mod menu;
pub mod multiplayer;
pub mod mute;
pub mod palette;
pub mod results;
pub mod scores;
pub mod settings_ui;
pub mod sfx;
mod shapes;
pub mod song_select;
pub mod states;
mod theme;
pub mod transition;
pub mod ui;
pub mod ui_kit;
mod xplorer;

use bevy::prelude::*;
use bevy::window::PresentMode;

use states::{AppState, GamePhase};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `BEATBYTE_WINDOW=WxH`: explicit window size (verification of the
/// scale-independent layout, or personal preference).
fn parse_window_env() -> Option<(u32, u32)> {
    let value = std::env::var("BEATBYTE_WINDOW").ok()?;
    let (w, h) = value.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Point the engine at the right `assets/` directory for every layout
/// we ship or develop in. Bevy's default resolution is exe-relative,
/// which silently misses the workspace assets when running
/// `target/debug/beatbyte` directly — and a failed font load is
/// permanent, taking all text with it. Resolution order:
///
/// 1. `BEVY_ASSET_ROOT` already set — respect it.
/// 2. `assets/` next to the executable (portable archives).
/// 3. `../Resources/assets` (macOS .app bundle).
/// 4. `assets/` in the current directory (development).
/// 5. `assets/` in an ancestor of the executable (target/debug under
///    the workspace).
#[allow(unsafe_code)] // set_var before any threads exist (documented below)
fn configure_asset_root() {
    if std::env::var_os("BEVY_ASSET_ROOT").is_some() {
        return;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = &exe_dir {
        candidates.push(dir.clone());
        candidates.push(dir.join("../Resources"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Some(dir) = &exe_dir {
        candidates.extend(
            dir.ancestors()
                .skip(1)
                .take(4)
                .map(std::path::Path::to_path_buf),
        );
    }

    for root in candidates {
        if root.join("assets").is_dir() {
            info!("asset root: {}", root.display());
            // Safety: called before the app (and its task pools)
            // start; no other threads read the environment yet.
            unsafe { std::env::set_var("BEVY_ASSET_ROOT", &root) };
            return;
        }
    }
    warn!("no assets directory found near the executable or CWD");
}

/// Build and run the BeatByte application.
///
/// This is the single entry point used by the `beatbyte` binary.
pub fn run() -> AppExit {
    configure_asset_root();
    let autopilot = std::env::var_os("BEATBYTE_AUTOPILOT").is_some();
    let harness = autopilot || std::env::var_os("BEATBYTE_SMOKE_TEST").is_some();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("BeatByte v{VERSION}"),
                    // Harness runs happen on machines people are
                    // USING — a full-size window pops over their work
                    // and gets closed (which rightly fails the run).
                    // A small window is the ONLY safe softening: an
                    // invisible window or AlwaysOnBottom/At-position
                    // kills the macOS event loop silently right after
                    // gameplay starts (observed; exit 0, no verdict —
                    // which the run()-guard would catch only on
                    // platforms where winit returns at all). With
                    // BEATBYTE_SHOT_DIR set, full size wins because
                    // the screenshots are the point.
                    resolution: if let Some((w, h)) = parse_window_env() {
                        (w, h).into()
                    } else if harness && std::env::var_os("BEATBYTE_SHOT_DIR").is_none() {
                        (320, 180).into()
                    } else {
                        (1280, 720).into()
                    },
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                // Harness integrity: in autopilot mode the ONLY valid
                // exits are the autopilot's own verdicts. Bevy's
                // default "all windows closed => AppExit::Success"
                // turned an environment-killed run (macOS display
                // sleep removed the monitor mid-song) into a fake
                // PASS. Autopilot instead detects the vanished window
                // and fails loudly.
                exit_condition: if std::env::var_os("BEATBYTE_AUTOPILOT").is_some() {
                    bevy::window::ExitCondition::DontExit
                } else {
                    bevy::window::ExitCondition::OnAllClosed
                },
                ..default()
            })
            // Pixel art: nearest-neighbor sampling keeps pixels crisp.
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(palette::BACKGROUND))
    .init_state::<AppState>()
    .add_sub_state::<GamePhase>()
    .add_systems(Startup, spawn_camera)
    .add_systems(Update, (sync_bloom, sync_ui_scale, report_frame_times))
    .add_plugins((
        import::ImportPlugin,
        input_test::InputTestPlugin,
        xplorer::XplorerPlugin,
        shapes::ShapesPlugin,
        ui::UiPlugin,
        audio_sys::AudioBridgePlugin,
        config::ConfigPlugin,
        mute::MutePlugin,
        scores::ScoresPlugin,
        boot::BootPlugin,
        menu::MenuPlugin,
        multiplayer::MultiplayerPlugin,
        song_select::SongSelectPlugin,
    ))
    .add_plugins((
        settings_ui::SettingsUiPlugin,
        controls_ui::ControlsUiPlugin,
        calibration::CalibrationPlugin,
        editor_ui::EditorUiPlugin,
        theme::ThemePlugin,
        gameplay::GameplayPlugin,
        results::ResultsPlugin,
        sfx::SfxPlugin,
        transition::TransitionPlugin,
        autopilot::AutopilotPlugin,
    ));

    // `BEATBYTE_FPS=1` turns on periodic frame-time reporting. Off by
    // default so the resource does not exist and the system returns
    // immediately.
    if std::env::var_os("BEATBYTE_FPS").is_some() {
        app.init_resource::<FrameLog>();
    }

    // Headless-ish smoke testing: `BEATBYTE_SMOKE_TEST=1 beatbyte` exits
    // cleanly after a few seconds. Used to verify the full app boots.
    // (Autopilot mode owns the exit instead when both are set.)
    if std::env::var_os("BEATBYTE_SMOKE_TEST").is_some()
        && std::env::var_os("BEATBYTE_AUTOPILOT").is_none()
    {
        app.add_systems(Update, smoke_test_exit);
    }

    let exit = app.run();
    // Autopilot: a clean exit is only real if a verdict was actually
    // delivered — every silent way the event loop can die has at some
    // point produced a fake exit-0 "pass".
    if autopilot
        && exit == AppExit::Success
        && !autopilot::VERDICT_DELIVERED.load(std::sync::atomic::Ordering::Relaxed)
    {
        eprintln!("autopilot: the app exited without a verdict — failing the run");
        return AppExit::error();
    }
    exit
}

/// The one persistent 2D camera.
fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        // The whole game is laid out in a 1280x720 world. AutoMin
        // guarantees at least that much is ALWAYS visible and scales
        // it with the window — resize, fullscreen, ultrawide: the
        // stage fits, extra space shows more backdrop instead of
        // cropping receptors or HUD.
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMin {
                min_width: 1280.0,
                min_height: 720.0,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Frame-time accounting for `BEATBYTE_FPS=1`.
///
/// A view that looks better and drops frames is not an improvement:
/// the key injector and the player both lose notes when a frame
/// stalls. This reports the numbers that decide it — the median frame
/// and the worst 1 % — rather than an average, because an average
/// hides exactly the stutters that cost notes.
#[derive(Resource, Default)]
struct FrameLog {
    samples: Vec<f32>,
    next_report_s: f32,
}

/// Log frame timings periodically when asked to.
fn report_frame_times(time: Res<Time>, mut log: Option<ResMut<FrameLog>>) {
    let Some(log) = log.as_mut() else {
        return;
    };
    let delta = time.delta_secs();
    if delta > 0.0 {
        log.samples.push(delta * 1000.0);
    }
    let elapsed = time.elapsed_secs();
    if elapsed < log.next_report_s || log.samples.len() < 30 {
        return;
    }
    log.next_report_s = elapsed + 5.0;
    let mut sorted = core::mem::take(&mut log.samples);
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let worst = sorted[sorted.len() * 99 / 100];
    info!(
        "frames: median {median:.2} ms ({:.0} fps), 99th percentile {worst:.2} ms, samples {}",
        1000.0 / median.max(0.001),
        sorted.len()
    );
}

/// Scale the (screen-space) UI with the window so menus stay
/// proportional — without this a 4K window renders tiny menus while
/// the world scales correctly.
fn sync_ui_scale(windows: Query<&Window>, mut scale: ResMut<bevy::ui::UiScale>) {
    let Ok(window) = windows.single() else {
        return;
    };
    let target = (window.height() / 720.0).clamp(0.6, 2.5);
    if (scale.0 - target).abs() > 0.01 {
        scale.0 = target;
    }
}

/// HDR bloom rides with the round style: emissive gems and glow
/// strips actually GLOW. The pixel style stays bloom-free — crisp
/// squares are its identity.
fn sync_bloom(
    mut commands: Commands,
    settings: Res<config::Settings>,
    cameras: Query<(Entity, Has<bevy::post_process::bloom::Bloom>), With<Camera2d>>,
) {
    for (camera, has_bloom) in &cameras {
        if settings.round_gems && !has_bloom {
            commands
                .entity(camera)
                .insert(bevy::post_process::bloom::Bloom {
                    intensity: 0.22,
                    ..bevy::post_process::bloom::Bloom::NATURAL
                });
        } else if !settings.round_gems && has_bloom {
            commands
                .entity(camera)
                .remove::<bevy::post_process::bloom::Bloom>()
                .remove::<bevy::camera::Hdr>();
        }
    }
}

/// Exit the app automatically shortly after startup (smoke-test mode
/// only). Waits long enough for the demo song build to finish so the
/// boot → menu path is exercised too.
fn smoke_test_exit(
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let reached_menu = *state.get() != AppState::Boot;
    if time.elapsed_secs() > 3.0 && reached_menu {
        info!(
            "smoke test: exiting cleanly after {:.1}s in {:?}",
            time.elapsed_secs(),
            state.get()
        );
        app_exit.write(AppExit::Success);
    }
    if time.elapsed_secs() > 30.0 {
        error!("smoke test: demo load never finished");
        app_exit.write(AppExit::error());
    }
}
