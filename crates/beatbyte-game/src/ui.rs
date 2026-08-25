//! Shared UI building blocks: the pixel font and common text styling.
//!
//! Press Start 2P is the game's voice — chunky, unmistakably 8-bit,
//! properly licensed (OFL, bundled next to the font). It runs wide, so
//! sizes here are roughly half of what a normal font would use.

use bevy::prelude::*;

/// The UI font, loaded at startup. In the round (non-8-bit) note
/// style the whole game drops the pixel font for the engine's smooth
/// built-in face — "not 8-bit" has to include the type.
#[derive(Resource)]
pub struct UiFont {
    pixel: Handle<Font>,
    /// Mirrors `Settings::round_gems`; synced every frame.
    pub smooth: bool,
}

impl UiFont {
    /// A [`TextFont`] in the active style at the given size.
    #[must_use]
    pub fn text(&self, size: f32) -> TextFont {
        TextFont {
            font: if self.smooth {
                Handle::default().into()
            } else {
                self.pixel.clone().into()
            },
            font_size: FontSize::Px(size),
            ..default()
        }
    }
}

/// Loads the font before any screen spawns text.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // Insert at build time, not from a startup system: the initial
        // state's OnEnter may run before startup-command flushes, and
        // every screen's spawn system reads this resource.
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load("fonts/PressStart2P-Regular.ttf");
        app.insert_resource(UiFont {
            pixel: handle,
            smooth: false,
        })
        .add_systems(Update, sync_font_style);
    }
}

/// Keep the font choice in step with the note-style setting. Screens
/// rebuild on state changes, so newly spawned text picks it up; text
/// already on screen keeps its face until its screen rebuilds.
fn sync_font_style(settings: Res<crate::config::Settings>, mut font: ResMut<UiFont>) {
    if font.smooth != settings.round_gems {
        font.smooth = settings.round_gems;
    }
}
