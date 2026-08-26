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
struct MenuCursor(usize);

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
struct MenuRow(usize);

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
            ui_kit::footer(
                parent,
                &font,
                "UP/DOWN choose  ENTER confirm  MOUSE works too",
            );
        });
}

fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    rows: Query<(&MenuRow, &Interaction), Changed<Interaction>>,
    mut cursor: ResMut<MenuCursor>,
    mut roster: ResMut<crate::multiplayer::PlayerRoster>,
    mut next_state: ResMut<NextState<AppState>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let nav = MenuNav::read(&keys, pads.iter());
    let count = MenuAction::ALL.len();
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    // Mouse: hovering selects, clicking activates.
    let mut clicked = false;
    for (row, interaction) in &rows {
        match interaction {
            Interaction::Hovered => cursor.0 = row.0,
            Interaction::Pressed => {
                cursor.0 = row.0;
                clicked = true;
            }
            Interaction::None => {}
        }
    }
    if nav.confirm || clicked {
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
    cursor: Res<MenuCursor>,
    mut rows: Query<(&MenuRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&MenuLabel, &mut TextColor)>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = ui_kit::row_style(ui_kit::state_for(row.0 == cursor.0, false));
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 = ui_kit::row_style(ui_kit::state_for(label.0 == cursor.0, false)).label;
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
