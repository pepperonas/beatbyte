//! The controls screen: view and remap every binding.
//!
//! Enter on a row arms capture mode — the next key or gamepad button
//! becomes an additional binding for that action (stolen from any
//! action that had it). Backspace restores the row's defaults.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::Settings;
use crate::controls::{Binding, GameAction, InputMap, MenuNav};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

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
                (controls_input, refresh_controls, refresh_pad_tester)
                    .run_if(in_state(AppState::Controls)),
            )
            .add_systems(OnExit(AppState::Controls), (persist_map, despawn_controls));
    }
}

#[derive(Component)]
struct ControlsScreen;

/// One action row (index into [`GameAction::ALL`]). Carries `Button`,
/// so this screen finally answers the mouse like every other one.
#[derive(Component)]
struct ActionRow(usize);

/// A row's action name.
#[derive(Component)]
struct ActionLabel(usize);

/// A row's current bindings.
#[derive(Component)]
struct ActionBindings(usize);

/// The status/hint line.
#[derive(Component)]
struct HintLine;

fn spawn_controls(mut commands: Commands, font: Res<UiFont>, mut state: ResMut<ControlsState>) {
    state.capturing = false;
    commands
        .spawn((ControlsScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "CONTROLS", "every action, on any device");
            parent.spawn(ui_kit::panel()).with_children(|panel| {
                for (index, action) in GameAction::ALL.iter().enumerate() {
                    panel
                        .spawn((ActionRow(index), Button, ui_kit::row()))
                        .with_children(|row| {
                            row.spawn((
                                ActionLabel(index),
                                Text::new(action.label()),
                                font.text(ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                ui_kit::label_node(),
                            ));
                            row.spawn((
                                ActionBindings(index),
                                Text::new(""),
                                font.text(ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                ui_kit::value_node(),
                            ));
                        });
                }
            });
            // Device diagnostics: which pads are connected, and five
            // live fret lamps — press a fret on your controller and
            // watch it light up. This exists because a real guitar
            // was plugged in and there was no way to SEE it working.
            parent.spawn((
                PadLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(16)),
                    ..default()
                },
            ));
            parent
                .spawn(Node {
                    column_gap: px(14),
                    margin: UiRect::top(px(8)),
                    ..default()
                })
                .with_children(|lamps| {
                    for fret in 0..5u8 {
                        lamps.spawn((
                            FretLamp(fret),
                            Node {
                                width: px(26),
                                height: px(26),
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::all(px(13)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.5)),
                        ));
                    }
                });
            parent.spawn((
                HintLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                Node {
                    margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
                    ..default()
                },
            ));
        });
}

/// The connected-devices line.
#[derive(Component)]
struct PadLine;

/// One live fret-test lamp (0 = green .. 4 = orange).
#[derive(Component)]
struct FretLamp(u8);

/// Show connected pads and light the lamps from LIVE input — through
/// the real InputMap, so this validates the whole chain.
fn refresh_pad_tester(
    pads: Query<(&Name, &bevy::input::gamepad::Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    mut line: Query<&mut Text, With<PadLine>>,
    mut lamps: Query<(&FretLamp, &mut BackgroundColor)>,
) {
    if let Ok(mut text) = line.single_mut() {
        let names: Vec<String> = pads.iter().map(|(name, _)| name.to_string()).collect();
        let wanted = if names.is_empty() {
            "no controller connected - keyboard ready".to_owned()
        } else {
            format!("connected: {}", names.join(", "))
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let sources = crate::controls::InputSources {
        keys: &keys,
        pads: pads.iter().map(|(_, pad)| pad).collect(),
    };
    for (lamp, mut color) in &mut lamps {
        let pressed = sources.pressed(&map, GameAction::Fret(lamp.0));
        color.0 = if pressed {
            palette::LANES[lamp.0 as usize]
        } else {
            Color::NONE
        };
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    rows: Query<(&ActionRow, &Interaction), Changed<Interaction>>,
    mut state: ResMut<ControlsState>,
    mut map: ResMut<InputMap>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let count = GameAction::ALL.len();
    if state.capturing {
        // Escape (or right-click) cancels the capture; anything
        // else binds. Mouse buttons are not bindable, so a click can
        // never BE the captured input.
        if keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
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

    // Navigation goes through MenuNav like every other screen. Reading
    // the arrow keys directly, as this screen used to, meant a player
    // holding a guitar could not reach the screen that rebinds it.
    let nav = MenuNav::read(&keys, pads.iter());
    if nav.up {
        state.cursor = (state.cursor + count - 1) % count;
    }
    if nav.down {
        state.cursor = (state.cursor + 1) % count;
    }
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        state.cursor = index;
    }
    let clicked = pointer.clicked;
    if nav.confirm || clicked {
        state.capturing = true;
    }
    if keys.just_pressed(KeyCode::Backspace) {
        map.reset_action(GameAction::ALL[state.cursor]);
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        next_state.set(AppState::Settings);
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn refresh_controls(
    map: Res<InputMap>,
    state: Res<ControlsState>,
    mut rows: Query<(&ActionRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&ActionLabel, &mut TextColor), Without<ActionBindings>>,
    mut bindings: Query<(&ActionBindings, &mut Text, &mut TextColor), Without<ActionLabel>>,
    mut hint: Query<&mut Text, (With<HintLine>, Without<ActionBindings>)>,
) {
    let style_of = |index: usize| {
        ui_kit::row_style(ui_kit::state_for(
            index == state.cursor,
            state.capturing && index == state.cursor,
        ))
    };
    for (row, mut background, mut border) in &mut rows {
        let style = style_of(row.0);
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 = style_of(label.0).label;
    }
    for (row, mut text, mut color) in &mut bindings {
        let wanted = if state.capturing && row.0 == state.cursor {
            "press a key or button...".to_owned()
        } else {
            map.of(GameAction::ALL[row.0])
                .iter()
                .map(|b| b.label())
                .collect::<Vec<_>>()
                .join(" / ")
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = style_of(row.0).value;
    }
    if let Ok(mut text) = hint.single_mut() {
        let line = if state.capturing {
            "press the new key or button  ESC cancel".to_owned()
        } else {
            "UP/DOWN choose  ENTER rebind  BACKSPACE reset  ESC back".to_owned()
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
