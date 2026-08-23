//! The results screen: how did the run go?

use bevy::prelude::*;

use crate::gameplay::LastResults;
use crate::palette;
use crate::states::AppState;

/// Plugin for the results screen.
pub struct ResultsPlugin;

impl Plugin for ResultsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Results), spawn_results)
            .add_systems(Update, results_input.run_if(in_state(AppState::Results)))
            .add_systems(OnExit(AppState::Results), despawn_results);
    }
}

#[derive(Component)]
struct ResultsScreen;

fn spawn_results(mut commands: Commands, results: Option<Res<LastResults>>) {
    let Some(results) = results else {
        return;
    };
    let perf = &results.performance;
    let counts = perf.counts();
    let accuracy = perf.accuracy() * 100.0;
    let grade = grade_for(accuracy, counts.miss);

    commands
        .spawn((
            ResultsScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(10),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new(grade),
                TextFont {
                    font_size: FontSize::Px(110.0),
                    ..default()
                },
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new(format!("\"{}\" on {}", results.title, results.difficulty)),
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                Text::new(format!("{}", perf.score())),
                TextFont {
                    font_size: FontSize::Px(52.0),
                    ..default()
                },
                TextColor(palette::TEXT),
            ));
            parent.spawn((
                Text::new(format!(
                    "accuracy {accuracy:.1}%  |  best streak {}",
                    perf.best_streak()
                )),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                Text::new(format!(
                    "perfect {}   great {}   good {}   miss {}   overstrums {}",
                    counts.perfect,
                    counts.great,
                    counts.good,
                    counts.miss,
                    perf.overstrums()
                )),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                Text::new("ENTER back to menu"),
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
        });
}

/// A simple letter grade from accuracy and misses.
fn grade_for(accuracy_percent: f64, misses: u32) -> &'static str {
    match accuracy_percent {
        a if a >= 97.0 && misses == 0 => "S",
        a if a >= 92.0 => "A",
        a if a >= 82.0 => "B",
        a if a >= 70.0 => "C",
        a if a >= 55.0 => "D",
        _ => "E",
    }
}

fn results_input(keys: Res<ButtonInput<KeyCode>>, mut next_state: ResMut<NextState<AppState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) {
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_results(mut commands: Commands, entities: Query<Entity, With<ResultsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
