//! Screen transitions: a quick fade over every state change.
//!
//! One persistent overlay above everything; entering a new top-level
//! state starts it opaque and fades it out. Cheap, engine-agnostic,
//! and it makes screen switches feel intentional instead of abrupt.

use bevy::prelude::*;

use crate::states::AppState;

/// Fade duration in seconds.
const FADE_S: f32 = 0.25;

/// The overlay's fade state.
#[derive(Resource, Default)]
struct Fade {
    /// Remaining opaque time (counts down to transparent).
    remaining: f32,
}

/// Marker for the overlay node.
#[derive(Component)]
struct FadeOverlay;

/// The transition plugin.
pub struct TransitionPlugin;

impl Plugin for TransitionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fade>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, run_fade);
        for state in [
            AppState::MainMenu,
            AppState::SongSelect,
            AppState::MultiplayerSetup,
            AppState::Settings,
            AppState::Controls,
            AppState::Calibration,
            AppState::Editor,
            AppState::Gameplay,
            AppState::Results,
        ] {
            app.add_systems(OnEnter(state), start_fade);
        }
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        FadeOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.01, 0.0)),
        GlobalZIndex(100),
        Pickable::IGNORE,
    ));
}

fn start_fade(mut fade: ResMut<Fade>) {
    fade.remaining = FADE_S;
}

fn run_fade(
    time: Res<Time>,
    mut fade: ResMut<Fade>,
    mut overlay: Query<&mut BackgroundColor, With<FadeOverlay>>,
) {
    let Ok(mut color) = overlay.single_mut() else {
        return;
    };
    if fade.remaining <= 0.0 {
        if color.0.alpha() != 0.0 {
            color.0 = color.0.with_alpha(0.0);
        }
        return;
    }
    fade.remaining = (fade.remaining - time.delta_secs()).max(0.0);
    let alpha = (fade.remaining / FADE_S).clamp(0.0, 1.0);
    color.0 = color.0.with_alpha(alpha * 0.9);
}
