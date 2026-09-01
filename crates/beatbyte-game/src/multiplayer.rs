//! Local multiplayer: the roster and the join screen.
//!
//! Players are devices: the keyboard can drive one player, every
//! connected gamepad another. The join screen assigns devices to
//! slots; gameplay then spawns one session entity per slot. All
//! gameplay systems are player-agnostic — multiplayer is more
//! entities, not more code paths (ADR-0002).

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The input device driving one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceId {
    /// The keyboard.
    Keyboard,
    /// A specific gamepad entity.
    Pad(Entity),
}

/// Versus or co-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MultiplayerMode {
    /// Independent scores, ranked results.
    #[default]
    Versus,
    /// One band: the results celebrate the combined score.
    Coop,
}

impl MultiplayerMode {
    /// Display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            MultiplayerMode::Versus => "VERSUS",
            MultiplayerMode::Coop => "CO-OP",
        }
    }
}

/// Who is playing, with what, and how.
#[derive(Resource, Debug, Clone)]
pub struct PlayerRoster {
    /// One entry per joined player, in join order.
    pub devices: Vec<DeviceId>,
    /// The chosen mode (irrelevant for one player).
    pub mode: MultiplayerMode,
}

impl Default for PlayerRoster {
    fn default() -> Self {
        PlayerRoster {
            devices: vec![DeviceId::Keyboard],
            mode: MultiplayerMode::Versus,
        }
    }
}

/// Maximum simultaneous players.
pub const MAX_PLAYERS: usize = 4;

/// Player accent colors (P1–P4).
pub const PLAYER_COLORS: [Color; MAX_PLAYERS] = [
    Color::srgb(1.0, 0.85, 0.25),
    Color::srgb(0.25, 0.77, 1.0),
    Color::srgb(0.24, 0.86, 0.52),
    Color::srgb(1.0, 0.45, 0.65),
];

/// Plugin for the multiplayer join screen.
pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerRoster>()
            .add_systems(OnEnter(AppState::MultiplayerSetup), spawn_join_screen)
            .add_systems(
                Update,
                (join_input, refresh_join_screen).run_if(in_state(AppState::MultiplayerSetup)),
            )
            .add_systems(OnExit(AppState::MultiplayerSetup), despawn_join_screen);
    }
}

#[derive(Component)]
struct JoinScreen;

/// One slot row (index 0–3).
#[derive(Component)]
struct SlotRow(usize);

/// A slot's "P1" label.
#[derive(Component)]
struct SlotLabel(usize);

/// A slot's device text.
#[derive(Component)]
struct SlotValue(usize);

/// The mode line.
#[derive(Component)]
struct ModeText;

fn spawn_join_screen(mut commands: Commands, font: Res<UiFont>, mut roster: ResMut<PlayerRoster>) {
    // Joining starts fresh each time the screen opens.
    roster.devices.clear();
    commands
        .spawn((JoinScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(
                parent,
                &font,
                "MULTIPLAYER",
                "press FRET 1 on a device to join",
            );
            parent.spawn(ui_kit::panel()).with_children(|panel| {
                for index in 0..MAX_PLAYERS {
                    panel
                        .spawn((SlotRow(index), ui_kit::row()))
                        .with_children(|row| {
                            row.spawn((
                                SlotLabel(index),
                                Text::new(format!("P{}", index + 1)),
                                font.text(ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                ui_kit::label_node(),
                            ));
                            row.spawn((
                                SlotValue(index),
                                Text::new(""),
                                font.text(ui_kit::ROW),
                                TextColor(palette::TEXT_DIM),
                                ui_kit::value_node(),
                            ));
                        });
                }
            });
            parent.spawn((
                ModeText,
                Text::new(""),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
                    ..default()
                },
            ));
            crate::prompts::device_footer(
                parent,
                &font,
                "A join  LEFT/RIGHT mode  ENTER continue  ESC back",
                "SOUTH join or continue  D-PAD mode  EAST back",
            );
        });
}

fn join_input(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    pads: Query<(Entity, &Gamepad)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut roster: ResMut<PlayerRoster>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    // Join: keyboard via the first fret key, pads via South.
    if keys.just_pressed(KeyCode::KeyA)
        && !roster.devices.contains(&DeviceId::Keyboard)
        && roster.devices.len() < MAX_PLAYERS
    {
        roster.devices.push(DeviceId::Keyboard);
    }
    for (entity, pad) in &pads {
        if pad.just_pressed(GamepadButton::South)
            && !roster.devices.contains(&DeviceId::Pad(entity))
            && roster.devices.len() < MAX_PLAYERS
        {
            roster.devices.push(DeviceId::Pad(entity));
        }
    }

    let nav = MenuNav::read(&map, &keys, pads.iter().map(|(_, pad)| pad));
    if nav.left || nav.right {
        roster.mode = match roster.mode {
            MultiplayerMode::Versus => MultiplayerMode::Coop,
            MultiplayerMode::Coop => MultiplayerMode::Versus,
        };
        sounds.write(crate::sfx::UiSound::Toggle);
    }
    if nav.confirm && !roster.devices.is_empty() {
        sounds.write(crate::sfx::UiSound::Confirm);
        next_state.set(AppState::SongSelect);
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_join_screen(
    roster: Res<PlayerRoster>,
    mut rows: Query<(&SlotRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&SlotLabel, &mut TextColor), Without<SlotValue>>,
    mut values: Query<(&SlotValue, &mut Text, &mut TextColor), Without<SlotLabel>>,
    mut mode: Query<&mut Text, (With<ModeText>, Without<SlotValue>)>,
) {
    // A joined slot lights its accent bar in that player's colour —
    // the same cue the rest of the game uses for "this is you".
    for (row, mut background, mut border) in &mut rows {
        let joined = roster.devices.get(row.0).is_some();
        background.0 = if joined {
            PLAYER_COLORS[row.0].with_alpha(ui_kit::FILL_ALPHA)
        } else {
            Color::NONE
        };
        *border = BorderColor::all(if joined {
            PLAYER_COLORS[row.0]
        } else {
            Color::NONE
        });
    }
    for (label, mut color) in &mut labels {
        color.0 = if roster.devices.get(label.0).is_some() {
            PLAYER_COLORS[label.0]
        } else {
            palette::TEXT_DIM
        };
    }
    for (slot, mut text, mut color) in &mut values {
        let line = match roster.devices.get(slot.0) {
            Some(DeviceId::Keyboard) => "KEYBOARD",
            Some(DeviceId::Pad(_)) => "GAMEPAD",
            None => "open",
        };
        if text.0 != line {
            text.0 = line.to_owned();
        }
        color.0 = if roster.devices.get(slot.0).is_some() {
            palette::TEXT
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.6)
        };
    }
    if let Ok(mut text) = mode.single_mut() {
        let line = format!("mode  < {} >", roster.mode.label());
        if text.0 != line {
            text.0 = line;
        }
    }
}

fn despawn_join_screen(mut commands: Commands, entities: Query<Entity, With<JoinScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
