//! Shared UI building blocks: the pixel font and common text styling.
//!
//! Press Start 2P is the game's voice — chunky, unmistakably 8-bit,
//! properly licensed (OFL, bundled next to the font). It runs wide, so
//! sizes here are roughly half of what a normal font would use.

use bevy::prelude::*;

/// The one UI font, loaded at startup.
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

impl UiFont {
    /// A [`TextFont`] in the pixel font at the given size.
    #[must_use]
    pub fn text(&self, size: f32) -> TextFont {
        TextFont {
            font: self.0.clone().into(),
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
        app.insert_resource(UiFont(handle));
    }
}
