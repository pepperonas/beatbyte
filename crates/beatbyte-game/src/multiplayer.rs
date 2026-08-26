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

/// The mode line.
#[derive(Component)]
struct ModeText;

fn spawn_join_screen(mut commands: Commands, font: Res<UiFont>, mut roster: ResMut<PlayerRoster>) {
    // Joining starts fresh each time the screen opens.
    roster.devices.clear();
    commands
        .spawn((
            JoinScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(16),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("MULTIPLAYER"),
                font.text(26.0),
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(px(14)),
                    ..default()
                },
            ));
            parent.spawn((
                Text::new("press FRET 1 (A / pad button) to join"),
                font.text(11.0),
                TextColor(palette::TEXT),
            ));
            for index in 0..MAX_PLAYERS {
                parent.spawn((
                    SlotRow(index),
                    Text::new(""),
                    font.text(13.0),
                    TextColor(palette::TEXT_DIM),
                ));
            }
            parent.spawn((
                ModeText,
                Text::new(""),
                font.text(13.0),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(16)),
                    ..default()
                },
            ));
            parent.spawn((
                Text::new("LEFT/RIGHT mode   ENTER continue   ESC back"),
                font.text(9.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::top(px(18)),
                    ..default()
                },
            ));
        });
}

fn join_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<(Entity, &Gamepad)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut roster: ResMut<PlayerRoster>,
    mut next_state: ResMut<NextState<AppState>>,
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

    let nav = MenuNav::read(&keys, pads.iter().map(|(_, pad)| pad));
    if nav.left || nav.right {
        roster.mode = match roster.mode {
            MultiplayerMode::Versus => MultiplayerMode::Coop,
            MultiplayerMode::Coop => MultiplayerMode::Versus,
        };
    }
    if nav.confirm && !roster.devices.is_empty() {
        next_state.set(AppState::SongSelect);
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_join_screen(
    roster: Res<PlayerRoster>,
    mut rows: Query<(&SlotRow, &mut Text, &mut TextColor), Without<ModeText>>,
    mut mode: Query<&mut Text, With<ModeText>>,
) {
    for (row, mut text, mut color) in &mut rows {
        let line = match roster.devices.get(row.0) {
            Some(DeviceId::Keyboard) => format!("P{}  KEYBOARD", row.0 + 1),
            Some(DeviceId::Pad(_)) => format!("P{}  GAMEPAD", row.0 + 1),
            None => format!("P{}  ---", row.0 + 1),
        };
        if text.0 != line {
            text.0 = line;
        }
        color.0 = if roster.devices.get(row.0).is_some() {
            PLAYER_COLORS[row.0]
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
