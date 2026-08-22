//! # beatbyte-game
//!
//! The presentation layer of BeatByte: Bevy plugins for gameplay
//! rendering, UI, effects and state management. Gameplay *rules* live in
//! [`beatbyte_core`]; this crate turns them into pixels and sound.

pub mod boot;
pub mod states;

use bevy::prelude::*;
use bevy::window::PresentMode;

use states::AppState;

/// The crate version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Deep-space navy used as the base clear color of the game.
pub const BACKGROUND_COLOR: Color = Color::srgb(0.043, 0.043, 0.086);

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
    .insert_resource(ClearColor(BACKGROUND_COLOR))
    .init_state::<AppState>()
    .add_plugins(boot::BootPlugin);

    // Headless-ish smoke testing: `BEATBYTE_SMOKE_TEST=1 beatbyte` exits
    // cleanly after a couple of seconds. Used by CI and local validation.
    if std::env::var_os("BEATBYTE_SMOKE_TEST").is_some() {
        app.add_systems(Update, smoke_test_exit);
    }

    app.run()
}

/// Exit the app automatically shortly after startup (smoke-test mode only).
fn smoke_test_exit(time: Res<Time>, mut app_exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() > 2.0 {
        info!(
            "smoke test: exiting cleanly after {:.1}s",
            time.elapsed_secs()
        );
        app_exit.write(AppExit::Success);
    }
}
