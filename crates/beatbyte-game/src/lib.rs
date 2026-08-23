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
pub mod library;
pub mod menu;
pub mod multiplayer;
pub mod palette;
pub mod results;
pub mod scores;
pub mod settings_ui;
pub mod sfx;
pub mod song_select;
pub mod states;
pub mod theme;
pub mod transition;
pub mod ui;

use bevy::prelude::*;
use bevy::window::PresentMode;

use states::{AppState, GamePhase};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Point the engine at the bundled assets when running from an
/// installed layout (macOS .app: `Contents/Resources/assets` next to
/// `Contents/MacOS/<exe>`). Portable layouts (assets next to the
/// executable) and dev runs already resolve correctly.
#[allow(unsafe_code)] // set_var before any threads exist (documented below)
fn configure_asset_root() {
    if std::env::var_os("BEVY_ASSET_ROOT").is_some() {
        return;
    }
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let resources = exe_dir.join("../Resources");
    if resources.join("assets").is_dir() {
        // Safety: called before the app (and its task pools) start;
        // no other threads are reading the environment yet.
        unsafe { std::env::set_var("BEVY_ASSET_ROOT", &resources) };
    }
}

/// Build and run the BeatByte application.
///
/// This is the single entry point used by the `beatbyte` binary.
pub fn run() -> AppExit {
    configure_asset_root();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("BeatByte v{VERSION}"),
                    resolution: (1280, 720).into(),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
            // Pixel art: nearest-neighbor sampling keeps pixels crisp.
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(palette::BACKGROUND))
    .init_state::<AppState>()
    .add_sub_state::<GamePhase>()
    .add_systems(Startup, spawn_camera)
    .add_plugins((
        ui::UiPlugin,
        audio_sys::AudioBridgePlugin,
        config::ConfigPlugin,
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

    // Headless-ish smoke testing: `BEATBYTE_SMOKE_TEST=1 beatbyte` exits
    // cleanly after a few seconds. Used to verify the full app boots.
    // (Autopilot mode owns the exit instead when both are set.)
    if std::env::var_os("BEATBYTE_SMOKE_TEST").is_some()
        && std::env::var_os("BEATBYTE_AUTOPILOT").is_none()
    {
        app.add_systems(Update, smoke_test_exit);
    }

    app.run()
}

/// The one persistent 2D camera.
fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
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
