//! The controls screen: view and remap every binding.
//!
//! Enter on a row arms capture mode — the next key or gamepad button
//! becomes an additional binding for that action (stolen from any
//! action that had it). Backspace restores the row's defaults.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::Settings;
use crate::controls::{Binding, GameAction, InputMap};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// Cursor + capture state.
#[derive(Resource, Default)]
struct ControlsState {
    cursor: usize,
    capturing: bool,
}

/// Plugin for the controls screen.
pub struct ControlsUiPlugin;

impl Plugin for ControlsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsState>()
            .add_systems(OnEnter(AppState::Controls), spawn_controls)
            .add_systems(
                Update,
                (controls_input, refresh_controls).run_if(in_state(AppState::Controls)),
            )
            .add_systems(OnExit(AppState::Controls), (persist_map, despawn_controls));
    }
}

#[derive(Component)]
struct ControlsScreen;

/// One action row's text (index into [`GameAction::ALL`]).
#[derive(Component)]
struct ActionRow(usize);

/// The status/hint line.
#[derive(Component)]
struct HintLine;

fn spawn_controls(mut commands: Commands, font: Res<UiFont>, mut state: ResMut<ControlsState>) {
    state.capturing = false;
    commands
        .spawn((
            ControlsScreen,
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
                Text::new("CONTROLS"),
                font.text(26.0),
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(px(18)),
                    ..default()
                },
            ));
            for (index, _) in GameAction::ALL.iter().enumerate() {
                parent.spawn((
                    ActionRow(index),
                    Text::new(""),
                    font.text(12.0),
                    TextColor(palette::TEXT_DIM),
                ));
            }
            parent.spawn((
                HintLine,
                Text::new(""),
                font.text(9.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::top(px(22)),
                    ..default()
                },
            ));
        });
}

fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut state: ResMut<ControlsState>,
    mut map: ResMut<InputMap>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let count = GameAction::ALL.len();
    if state.capturing {
        // Escape cancels the capture; anything else binds.
        if keys.just_pressed(KeyCode::Escape) {
            state.capturing = false;
            return;
        }
        let captured = keys
            .get_just_pressed()
            .next()
            .map(|key| Binding::Key(*key))
            .or_else(|| {
                pads.iter()
                    .flat_map(|pad| pad.get_just_pressed())
                    .next()
                    .map(|button| Binding::Pad(*button))
            });
        if let Some(binding) = captured {
            map.rebind(GameAction::ALL[state.cursor], binding);
            state.capturing = false;
        }
        return;
    }

    if keys.just_pressed(KeyCode::ArrowUp) {
        state.cursor = (state.cursor + count - 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        state.cursor = (state.cursor + 1) % count;
    }
    if keys.just_pressed(KeyCode::Enter) {
        state.capturing = true;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        map.reset_action(GameAction::ALL[state.cursor]);
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(AppState::Settings);
    }
}

fn refresh_controls(
    map: Res<InputMap>,
    state: Res<ControlsState>,
    mut rows: Query<(&ActionRow, &mut Text, &mut TextColor)>,
    mut hint: Query<&mut Text, (With<HintLine>, Without<ActionRow>)>,
) {
    for (row, mut text, mut color) in &mut rows {
        let action = GameAction::ALL[row.0];
        let selected = row.0 == state.cursor;
        let marker = if selected { ">" } else { " " };
        let bindings = map
            .of(action)
            .iter()
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join(" / ");
        let line = if selected && state.capturing {
            format!("{marker} {:<12} press a key or button...", action.label())
        } else {
            format!("{marker} {:<12} {bindings}", action.label())
        };
        if text.0 != line {
            text.0 = line;
        }
        color.0 = if selected {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
    }
    if let Ok(mut text) = hint.single_mut() {
        let line = if state.capturing {
            "press the new key/button   ESC cancel".to_owned()
        } else {
            "ENTER rebind   BACKSPACE reset row   ESC back".to_owned()
        };
        if text.0 != line {
            text.0 = line;
        }
    }
}

fn persist_map(map: Res<InputMap>, mut settings: ResMut<Settings>) {
    settings.input_map = map.clone();
    crate::config::save_settings(&settings);
}

fn despawn_controls(mut commands: Commands, entities: Query<Entity, With<ControlsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
