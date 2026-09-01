//! # beatbyte-game
//!
//! The presentation layer of BeatByte: Bevy plugins for gameplay
//! rendering, UI, effects and state management. Gameplay *rules* live
//! in [`beatbyte_core`]; this crate turns them into pixels and sound.

pub mod about;
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
pub mod lyrics_fetch;
pub mod mc;
pub mod menu;
pub mod multiplayer;
pub mod mute;
pub mod palette;
pub mod prompts;
pub mod results;
pub mod scores;
pub mod settings_ui;
pub mod sfx;
mod shapes;
pub mod song_select;
pub mod states;
pub mod telemetry;
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
                    // Uncapped rendering exists for MEASUREMENT
                    // (BEATBYTE_UNCAPPED=1): under vsync every
                    // frame-time median is pinned to the display and
                    // an optimization cannot be seen. Play stays
                    // vsynced — tearing buys a player nothing.
                    present_mode: if std::env::var_os("BEATBYTE_UNCAPPED").is_some() {
                        PresentMode::AutoNoVsync
                    } else {
                        PresentMode::AutoVsync
                    },
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
    .add_systems(
        Update,
        (
            sync_bloom,
            sync_stage_compositing,
            sync_ui_scale,
            report_frame_times,
        ),
    )
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
        about::AboutPlugin,
        settings_ui::SettingsUiPlugin,
        controls_ui::ControlsUiPlugin,
        calibration::CalibrationPlugin,
        editor_ui::EditorUiPlugin,
        theme::ThemePlugin,
        gameplay::GameplayPlugin,
        results::ResultsPlugin,
        prompts::PromptsPlugin,
        sfx::SfxPlugin,
        telemetry::TelemetryPlugin,
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
        // Ordered BEFORE the menu's own input so the simulated key
        // press is still `just_pressed` when the menu reads it: Bevy
        // clears that flag at the start of each frame, so a press
        // written after the reader would never be seen at all.
        app.add_systems(Update, smoke_test_exit.before(menu::menu_input));
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
        // THE UI camera, explicitly. With a second camera on screen
        // (the 3D stage) and no marked default, bevy_ui cannot pick
        // a target for root nodes: they lay out to ZERO size and
        // vanish — the pause menu shipped invisible exactly this
        // way, while every plain menu (one camera) worked.
        bevy::ui::IsDefaultUiCamera,
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
fn sync_ui_scale(
    windows: Query<&Window>,
    settings: Res<config::Settings>,
    mut scale: ResMut<bevy::ui::UiScale>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let target = ui_scale_target(window.height(), settings.ui_scale);
    if (scale.0 - target).abs() > 0.01 {
        scale.0 = target;
    }
}

/// The UI scale for a window height and the player's own multiplier
/// (an accessibility setting). The window sync keeps menus
/// proportional; the multiplier stacks on top and the result stays
/// clamped so no settings file can render the UI unusable. Pure —
/// tested.
#[must_use]
pub fn ui_scale_target(window_height: f32, user_scale: f32) -> f32 {
    let auto = (window_height / 720.0).clamp(0.6, 2.5);
    (auto * user_scale.clamp(0.75, 1.5)).clamp(0.5, 3.0)
}

/// Exactly ONE camera may clear the window. While the 3D stage
/// camera is on screen (it renders first, order -1), the 2D camera
/// on top must LOAD the frame instead of wiping it — with the plain
/// SDR 2D pass (8-bit note style, no bloom) the default clear
/// erased the entire stage: score and particles over a black void.
/// The round style dodged the wipe only by accident of its HDR
/// bloom pipeline, which is why the bug hid behind one particular
/// settings combination. Without a stage camera the 2D camera is
/// alone again and must clear, or menus would smear.
fn sync_stage_compositing(
    stage_cameras: Query<(), (With<Camera3d>, With<gameplay::stage3d::Stage3d>)>,
    mut cameras: Query<&mut Camera, With<Camera2d>>,
) {
    let wanted = if stage_cameras.is_empty() {
        bevy::camera::ClearColorConfig::Default
    } else {
        bevy::camera::ClearColorConfig::None
    };
    for mut camera in &mut cameras {
        if core::mem::discriminant(&camera.clear_color) != core::mem::discriminant(&wanted) {
            camera.clear_color = wanted;
        }
    }
}

/// The cameras whose bloom/HDR state follows the note style: the 2D
/// camera and the 3D stage camera.
type BloomCameras = Or<(
    With<Camera2d>,
    (With<Camera3d>, With<gameplay::stage3d::Stage3d>),
)>;

/// HDR bloom rides with the round style: emissive gems and glow
/// strips actually GLOW. The pixel style stays bloom-free — crisp
/// squares are its identity — and that rule now covers EVERY camera
/// on the window, the 3D stage's included. This is load-bearing
/// beyond looks: cameras sharing one window must agree on HDR. A
/// mixed pair (SDR 2D over HDR stage) silently drops the HDR
/// camera's whole pass — the stage vanished under the 8-bit style
/// exactly so, while the round style worked only because its bloom
/// happened to make both cameras HDR.
fn sync_bloom(
    mut commands: Commands,
    settings: Res<config::Settings>,
    cameras: Query<(Entity, Has<bevy::post_process::bloom::Bloom>, Has<Camera2d>), BloomCameras>,
) {
    for (camera, has_bloom, is_2d) in &cameras {
        if settings.round_gems && !has_bloom {
            let intensity = if is_2d { 0.22 } else { 0.18 };
            commands
                .entity(camera)
                .insert(bevy::post_process::bloom::Bloom {
                    intensity,
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
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let reached_menu = *state.get() != AppState::Boot;
    if time.elapsed_secs() > 3.0 && reached_menu {
        // Leave by pressing Escape rather than by writing the exit
        // directly. The smoke test then proves the way a PLAYER
        // leaves actually works, instead of proving only that the
        // process can be told to stop - and it costs nothing, because
        // this run had to end somehow regardless.
        info!(
            "smoke test: pressing ESC after {:.1}s in {:?}",
            time.elapsed_secs(),
            state.get()
        );
        keys.release(KeyCode::Escape);
        keys.press(KeyCode::Escape);
    }
    // If Escape did not close the game, say so and fail. Without this
    // the run would simply hang, which reads as a stuck machine rather
    // than as a broken key.
    if time.elapsed_secs() > 6.0 && reached_menu {
        error!("smoke test: ESC did not close the game from the main menu");
        app_exit.write(AppExit::error());
    }
    if time.elapsed_secs() > 30.0 {
        error!("smoke test: demo load never finished");
        app_exit.write(AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    /// The parser, lifted out of [`parse_window_env`] so it can be
    /// tested without touching the process environment — a test that
    /// sets environment variables is a test that fights every other
    /// test running beside it.
    fn parse_window(value: &str) -> Option<(u32, u32)> {
        let (w, h) = value.split_once('x')?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    }

    #[test]
    fn a_window_size_parses() {
        assert_eq!(parse_window("1280x800"), Some((1280, 800)));
        assert_eq!(parse_window(" 1920 x 1200 "), Some((1920, 1200)));
    }

    #[test]
    fn nonsense_is_declined_rather_than_guessed() {
        // Falling back to a default on bad input would silently
        // measure the wrong layout — the variable exists precisely to
        // pin a size for verification.
        for bad in ["", "1280", "1280*800", "axb", "1280x", "x800", "-5x10"] {
            assert_eq!(parse_window(bad), None, "`{bad}` should not parse");
        }
    }

    #[test]
    fn the_parser_matches_the_one_in_use() {
        // Pins the copy above against the real implementation's
        // shape, so the two cannot drift apart unnoticed.
        let source = include_str!("lib.rs");
        assert!(
            source.contains("let (w, h) = value.split_once('x')?;"),
            "parse_window_env no longer splits on 'x'; update the test copy"
        );
    }
}

#[cfg(test)]
mod scale_tests {
    use super::ui_scale_target;

    #[test]
    fn the_user_multiplier_stacks_on_the_window_sync_and_stays_clamped() {
        let base = ui_scale_target(720.0, 1.0);
        assert!((base - 1.0).abs() < 1e-6);
        assert!(ui_scale_target(720.0, 1.3) > base, "bigger text on request");
        assert!(ui_scale_target(720.0, 0.8) < base);
        // Files are input too: absurd values clamp, never break the UI.
        assert!((ui_scale_target(720.0, 40.0) - 1.5).abs() < 1e-6);
        assert!(ui_scale_target(4320.0, 1.5) <= 3.0);
    }
}
