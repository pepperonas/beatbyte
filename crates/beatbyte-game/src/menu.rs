//! The main menu: pick a difficulty, start the song.
//!
//! Milestone 5 scope — one bundled song, keyboard-driven. The real
//! song browser arrives with the UI milestone.

use beatbyte_core::Difficulty;
use bevy::prelude::*;

use crate::boot::LoadedSong;
use crate::palette;
use crate::states::AppState;

/// The difficulty the player will play.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedDifficulty(pub Difficulty);

impl Default for SelectedDifficulty {
    fn default() -> Self {
        SelectedDifficulty(Difficulty::Medium)
    }
}

/// Plugin for the main menu.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedDifficulty>()
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(
                Update,
                (menu_input, update_difficulty_row).run_if(in_state(AppState::MainMenu)),
            )
            .add_systems(OnExit(AppState::MainMenu), despawn_menu);
    }
}

#[derive(Component)]
struct MenuScreen;

/// Marker for the difficulty labels, carrying their difficulty.
#[derive(Component)]
struct DifficultyLabel(Difficulty);

fn spawn_menu(mut commands: Commands, song: Res<LoadedSong>) {
    commands
        .spawn((
            MenuScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("BEATBYTE"),
                TextFont {
                    font_size: FontSize::Px(84.0),
                    ..default()
                },
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new(format!(
                    "\"{}\" by {}",
                    song.chart.song.title, song.chart.song.artist
                )),
                TextFont {
                    font_size: FontSize::Px(28.0),
                    ..default()
                },
                TextColor(palette::TEXT),
            ));
            parent.spawn((
                Text::new(format!("{:.0} BPM", song.chart.song.bpm)),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));

            // Difficulty row.
            parent
                .spawn(Node {
                    column_gap: px(28),
                    margin: UiRect::top(px(26)),
                    ..default()
                })
                .with_children(|row| {
                    for difficulty in Difficulty::ALL {
                        row.spawn((
                            DifficultyLabel(difficulty),
                            Text::new(difficulty.display_name().to_uppercase()),
                            TextFont {
                                font_size: FontSize::Px(26.0),
                                ..default()
                            },
                            TextColor(palette::TEXT_DIM),
                        ));
                    }
                });

            parent.spawn((
                Text::new("LEFT/RIGHT difficulty      ENTER rock"),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(30)),
                    ..default()
                },
            ));
            parent.spawn((
                Text::new("frets A S D F G  |  strum UP/DOWN  |  hype SPACE  |  pause ESC"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
            ));
        });
}

fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedDifficulty>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let all = Difficulty::ALL;
    let index = all.iter().position(|d| *d == selected.0).unwrap_or(1);
    if keys.just_pressed(KeyCode::ArrowLeft) && index > 0 {
        selected.0 = all[index - 1];
    }
    if keys.just_pressed(KeyCode::ArrowRight) && index + 1 < all.len() {
        selected.0 = all[index + 1];
    }
    if keys.just_pressed(KeyCode::Enter) {
        next_state.set(AppState::Gameplay);
    }
}

/// Highlight the selected difficulty (cheap enough to run every frame,
/// which also covers the freshly spawned labels).
fn update_difficulty_row(
    selected: Res<SelectedDifficulty>,
    mut labels: Query<(&DifficultyLabel, &mut TextColor)>,
) {
    for (label, mut color) in &mut labels {
        color.0 = if label.0 == selected.0 {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
    }
}

fn despawn_menu(mut commands: Commands, entities: Query<Entity, With<MenuScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
