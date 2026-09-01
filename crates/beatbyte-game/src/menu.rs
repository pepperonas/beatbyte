//! The main menu: play, settings, calibration, quit.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The four menu actions, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Open the song browser (solo).
    Play,
    /// Open the multiplayer join screen.
    Multiplayer,
    /// Open settings.
    Settings,
    /// Open latency calibration.
    Calibration,
    /// Free-play tester for keyboard and guitar input.
    InputTest,
    /// Quit the game.
    Quit,
}

impl MenuAction {
    const ALL: [MenuAction; 6] = [
        MenuAction::Play,
        MenuAction::Multiplayer,
        MenuAction::Settings,
        MenuAction::Calibration,
        MenuAction::InputTest,
        MenuAction::Quit,
    ];

    const fn label(self) -> &'static str {
        match self {
            MenuAction::Play => "PLAY",
            MenuAction::Multiplayer => "MULTIPLAYER",
            MenuAction::Settings => "SETTINGS",
            MenuAction::Calibration => "CALIBRATION",
            MenuAction::InputTest => "INPUT TEST",
            MenuAction::Quit => "QUIT",
        }
    }
}

/// The currently highlighted menu row.
#[derive(Resource, Default)]
pub(crate) struct MenuCursor(usize);

/// Plugin for the main menu.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuCursor>()
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(
                Update,
                (menu_input, highlight_cursor, pulse_title).run_if(in_state(AppState::MainMenu)),
            )
            .add_systems(OnExit(AppState::MainMenu), despawn_menu);
    }
}

#[derive(Component)]
struct MenuScreen;

/// Marker for the pulsing title.
#[derive(Component)]
struct MenuTitle;

/// A selectable row, carrying its index.
#[derive(Component)]
pub(crate) struct MenuRow(usize);

/// A row's label text, carrying the same index. The label is a child
/// of the row now that a row has chrome of its own.
#[derive(Component)]
struct MenuLabel(usize);

fn spawn_menu(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((MenuScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            // The title keeps its outsized treatment — it is the
            // game's wordmark, not a screen heading.
            parent.spawn((
                MenuTitle,
                Text::new("BEATBYTE"),
                font.text(ui_kit::WORDMARK),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new("five lanes. your music."),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
                Node {
                    margin: UiRect::top(px(10)).with_bottom(px(ui_kit::HEADER_GAP)),
                    ..default()
                },
            ));
            parent.spawn(ui_kit::panel()).with_children(|panel| {
                for (index, action) in MenuAction::ALL.iter().enumerate() {
                    panel
                        .spawn((MenuRow(index), Button, ui_kit::row()))
                        .with_children(|row| {
                            row.spawn((
                                MenuLabel(index),
                                Text::new(action.label()),
                                font.text(ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                            ));
                        });
                }
            });
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  ENTER confirm  ESC quit  MOUSE works too",
                "D-PAD choose  SOUTH confirm",
            );
        });
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub(crate) fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    pads: Query<&Gamepad>,
    rows: Query<(&MenuRow, &Interaction), Changed<Interaction>>,
    mut cursor: ResMut<MenuCursor>,
    mut roster: ResMut<crate::multiplayer::PlayerRoster>,
    mut next_state: ResMut<NextState<AppState>>,
    mut app_exit: MessageWriter<AppExit>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let count = MenuAction::ALL.len();
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    // Mouse: hovering selects, clicking activates.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        cursor.0 = index;
    }
    // Escape closes the game from here, since there is no screen
    // above this one to go back to.
    //
    // Deliberately the KEY and not `nav.back`: that also fires on the
    // pad's East button, which the default map gives to fret 1. With a
    // guitar plugged in, noodling on the red fret at the menu would
    // close the application. A test pins that pairing so this cannot
    // be "simplified" to `nav.back` later.
    if keys.just_pressed(KeyCode::Escape) {
        app_exit.write(AppExit::Success);
        return;
    }
    let clicked = pointer.clicked;
    if nav.confirm || clicked {
        sounds.write(crate::sfx::UiSound::Confirm);
        match MenuAction::ALL[cursor.0] {
            MenuAction::Play => {
                // Solo: one keyboard player.
                *roster = crate::multiplayer::PlayerRoster::default();
                next_state.set(AppState::SongSelect);
            }
            MenuAction::Multiplayer => next_state.set(AppState::MultiplayerSetup),
            MenuAction::Settings => next_state.set(AppState::Settings),
            MenuAction::Calibration => next_state.set(AppState::Calibration),
            MenuAction::InputTest => next_state.set(AppState::InputTest),
            MenuAction::Quit => {
                app_exit.write(AppExit::Success);
            }
        }
    }
}

/// Paint the highlighted row: accent bar, fill and label together.
fn highlight_cursor(
    settings: Res<crate::config::Settings>,
    cursor: Res<MenuCursor>,
    mut rows: Query<(&MenuRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&MenuLabel, &mut TextColor)>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = ui_kit::styled_row(
            ui_kit::state_for(row.0 == cursor.0, false),
            settings.high_contrast,
        );
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 = ui_kit::styled_row(
            ui_kit::state_for(label.0 == cursor.0, false),
            settings.high_contrast,
        )
        .label;
    }
}

/// The title breathes gently — a static menu reads as a frozen app.
fn pulse_title(time: Res<Time>, mut title: Query<&mut TextColor, With<MenuTitle>>) {
    if let Ok(mut color) = title.single_mut() {
        let pulse = 0.88 + 0.12 * (time.elapsed_secs() * 2.1).sin();
        let base = palette::BRAND.to_linear();
        color.0 = Color::LinearRgba(bevy::color::LinearRgba {
            red: base.red * pulse,
            green: base.green * pulse,
            blue: base.blue * pulse,
            alpha: 1.0,
        });
    }
}

fn despawn_menu(mut commands: Commands, entities: Query<Entity, With<MenuScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use crate::controls::{Binding, GameAction, InputMap};
    use bevy::input::gamepad::GamepadButton;

    #[test]
    fn the_pads_back_button_is_a_fret_so_it_must_not_quit() {
        // `MenuNav::back` is Escape OR the pad's East button, and the
        // default map gives East to fret 1. Wiring the menu's quit to
        // `nav.back` would close the game when a guitarist rests a
        // finger on the red fret at the menu.
        //
        // If this ever stops being true - East freed from the frets -
        // this test should be deleted along with the workaround in
        // `menu_input`, not silenced.
        let map = InputMap::default();
        let fret_one = map
            .bindings
            .iter()
            .find(|(action, _)| *action == GameAction::Fret(1))
            .map(|(_, bindings)| bindings.clone())
            .expect("fret 1 is bound");
        assert!(
            fret_one.contains(&Binding::Pad(GamepadButton::East)),
            "fret 1 no longer uses East: revisit the menu quit key"
        );
    }
}
