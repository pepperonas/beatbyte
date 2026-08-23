//! The main menu: play, settings, calibration, quit.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// The four menu actions, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Open the song browser.
    Play,
    /// Open settings.
    Settings,
    /// Open latency calibration.
    Calibration,
    /// Quit the game.
    Quit,
}

impl MenuAction {
    const ALL: [MenuAction; 4] = [
        MenuAction::Play,
        MenuAction::Settings,
        MenuAction::Calibration,
        MenuAction::Quit,
    ];

    const fn label(self) -> &'static str {
        match self {
            MenuAction::Play => "PLAY",
            MenuAction::Settings => "SETTINGS",
            MenuAction::Calibration => "CALIBRATION",
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

fn spawn_menu(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            MenuScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(22),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                MenuTitle,
                Text::new("BEATBYTE"),
                font.text(52.0),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new("an 8-bit rhythm game"),
                font.text(11.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::bottom(px(26)),
                    ..default()
                },
            ));
            for (index, action) in MenuAction::ALL.iter().enumerate() {
                parent.spawn((
                    MenuRow(index),
                    Text::new(action.label()),
                    font.text(18.0),
                    TextColor(palette::TEXT_DIM),
                ));
            }
            parent.spawn((
                Text::new("UP/DOWN choose    ENTER confirm"),
                font.text(10.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::top(px(30)),
                    ..default()
                },
            ));
        });
}

fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut cursor: ResMut<MenuCursor>,
    mut next_state: ResMut<NextState<AppState>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let nav = MenuNav::read(&keys, &pads);
    let count = MenuAction::ALL.len();
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    if nav.confirm {
        match MenuAction::ALL[cursor.0] {
            MenuAction::Play => next_state.set(AppState::SongSelect),
            MenuAction::Settings => next_state.set(AppState::Settings),
            MenuAction::Calibration => next_state.set(AppState::Calibration),
            MenuAction::Quit => {
                app_exit.write(AppExit::Success);
            }
        }
    }
}

/// Paint the highlighted row.
fn highlight_cursor(cursor: Res<MenuCursor>, mut rows: Query<(&MenuRow, &mut TextColor)>) {
    for (row, mut color) in &mut rows {
        color.0 = if row.0 == cursor.0 {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
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
