//! # beatbyte-game
//!
//! The presentation layer of BeatByte: Bevy plugins for gameplay
//! rendering, UI, effects and state management. Gameplay *rules* live
//! in [`beatbyte_core`]; this crate turns them into pixels and sound.

pub mod audio_sys;
pub mod autopilot;
pub mod boot;
pub mod gameplay;
pub mod menu;
pub mod palette;
pub mod results;
pub mod states;

use bevy::prelude::*;
use bevy::window::PresentMode;

use states::{AppState, GamePhase};

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build and run the BeatByte application.
///
/// This is the single entry point used by the `beatbyte` binary.
pub fn run() -> AppExit {
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
        audio_sys::AudioBridgePlugin,
        boot::BootPlugin,
        menu::MenuPlugin,
        gameplay::GameplayPlugin,
        results::ResultsPlugin,
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
