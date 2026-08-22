//! The boot screen: the first thing a player sees.
//!
//! Deliberately simple in Milestone 1 — a title card on the game's
//! background color, using Bevy's built-in font. The real pixel-art
//! identity (custom font, logo, animation) arrives with the UI milestone.

use bevy::prelude::*;

use crate::states::AppState;

/// Plugin for the boot screen shown in [`AppState::Boot`].
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Boot), spawn_boot_screen);
    }
}

/// Marker for entities belonging to the boot screen.
#[derive(Component)]
struct BootScreen;

fn spawn_boot_screen(mut commands: Commands) {
    commands.spawn((Camera2d, BootScreen));

    commands
        .spawn((
            BootScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(12),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("BEATBYTE"),
                TextFont {
                    font_size: FontSize::Px(96.0),
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.85, 0.25)),
            ));
            parent.spawn((
                Text::new(format!("v{}", crate::VERSION)),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.6, 0.75)),
            ));
        });
}
