//! The results screen: solo glory or the band's verdict.

use bevy::prelude::*;

use crate::gameplay::{LastResults, player_color};
use crate::multiplayer::MultiplayerMode;
use crate::palette;
use crate::scores::{BestScore, ScoreBoard, save_scores};
use crate::states::AppState;
use crate::ui::UiFont;

/// Plugin for the results screen.
pub struct ResultsPlugin;

impl Plugin for ResultsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Results), spawn_results)
            .add_systems(
                Update,
                (results_input, animate_grade, count_up_score).run_if(in_state(AppState::Results)),
            )
            .add_systems(OnExit(AppState::Results), despawn_results);
    }
}

#[derive(Component)]
struct ResultsScreen;

/// The grade letter slams in over the first third of a second.
#[derive(Component)]
struct GradeSlam {
    age: f32,
}

/// The score ticks up from zero.
#[derive(Component)]
struct ScoreCountUp {
    target: u64,
    age: f32,
}

fn spawn_results(
    mut commands: Commands,
    results: Option<Res<LastResults>>,
    mut scores: ResMut<ScoreBoard>,
    font: Res<UiFont>,
) {
    let Some(results) = results else {
        return;
    };

    // Record solo runs only — multiplayer scoreboards would mix
    // devices and players into one book. Tap mode records normally
    // since it became the default way to play; `tap_mode` on the
    // results stays available for display.
    let solo = results.players.len() == 1;
    let mut new_record = false;
    if solo && let Some(player) = results.players.first() {
        let perf = &player.performance;
        new_record = scores.record(
            &results.title,
            &results.artist,
            results.difficulty,
            BestScore {
                score: perf.score(),
                accuracy: perf.accuracy(),
                best_streak: perf.best_streak(),
            },
        );
        if new_record {
            save_scores(&scores);
        }
    }

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
            if solo {
                spawn_solo(parent, &results, new_record, &font);
            } else {
                spawn_multi(parent, &results, &font);
            }
            parent.spawn((
                Text::new("ENTER back to menu"),
                font.text(10.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(30)),
                    ..default()
                },
            ));
        });
}

fn spawn_solo(
    parent: &mut ChildSpawnerCommands,
    results: &LastResults,
    new_record: bool,
    font: &UiFont,
) {
    let perf = &results.players[0].performance;
    let counts = perf.counts();
    let accuracy = perf.accuracy() * 100.0;
    let grade = grade_for(accuracy, counts.miss);

    parent.spawn((
        GradeSlam { age: 0.0 },
        Text::new(grade),
        font.text(20.0),
        TextColor(palette::BRAND.with_alpha(0.0)),
    ));
    if new_record {
        parent.spawn((
            Text::new("NEW RECORD!"),
            font.text(14.0),
            TextColor(palette::PERFECT),
        ));
    }
    parent.spawn((
        Text::new(format!("\"{}\" on {}", results.title, results.difficulty)),
        font.text(12.0),
        TextColor(palette::TEXT_DIM),
    ));
    parent.spawn((
        ScoreCountUp {
            target: perf.score(),
            age: 0.0,
        },
        Text::new("0"),
        font.text(30.0),
        TextColor(palette::TEXT),
    ));
    parent.spawn((
        Text::new(format!(
            "accuracy {accuracy:.1}%  |  best streak {}",
            perf.best_streak()
        )),
        font.text(12.0),
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
        font.text(10.0),
        TextColor(palette::TEXT_DIM),
    ));
}

fn spawn_multi(parent: &mut ChildSpawnerCommands, results: &LastResults, font: &UiFont) {
    parent.spawn((
        Text::new(match results.mode {
            MultiplayerMode::Versus => "VERSUS RESULTS",
            MultiplayerMode::Coop => "BAND RESULTS",
        }),
        font.text(22.0),
        TextColor(palette::BRAND),
    ));
    parent.spawn((
        Text::new(format!("\"{}\" on {}", results.title, results.difficulty)),
        font.text(11.0),
        TextColor(palette::TEXT_DIM),
        Node {
            margin: UiRect::bottom(px(14)),
            ..default()
        },
    ));

    match results.mode {
        MultiplayerMode::Coop => {
            let total: u64 = results
                .players
                .iter()
                .map(|player| player.performance.score())
                .sum();
            parent.spawn((
                ScoreCountUp {
                    target: total,
                    age: 0.0,
                },
                Text::new("0"),
                font.text(30.0),
                TextColor(palette::TEXT),
            ));
            parent.spawn((
                Text::new("band total"),
                font.text(10.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::bottom(px(12)),
                    ..default()
                },
            ));
            for player in &results.players {
                parent.spawn(player_row(player, font));
            }
        }
        MultiplayerMode::Versus => {
            // Ranked: the winner tops the list.
            let mut ranked: Vec<_> = results.players.iter().collect();
            ranked.sort_by_key(|player| core::cmp::Reverse(player.performance.score()));
            for (place, player) in ranked.into_iter().enumerate() {
                let mut row = player_row(player, font);
                if place == 0 {
                    row.1 = font.text(15.0);
                }
                parent.spawn(row);
            }
        }
    }
}

/// A compact per-player result line.
fn player_row(
    player: &crate::gameplay::PlayerResult,
    font: &UiFont,
) -> (Text, TextFont, TextColor) {
    let perf = &player.performance;
    (
        Text::new(format!(
            "P{}   {}   {:.1}%   streak {}",
            player.index + 1,
            perf.score(),
            perf.accuracy() * 100.0,
            perf.best_streak()
        )),
        font.text(12.0),
        TextColor(player_color(player.index)),
    )
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

/// Overshooting scale-in for the grade letter.
fn animate_grade(
    time: Res<Time>,
    mut grades: Query<(&mut GradeSlam, &mut TextFont, &mut TextColor)>,
) {
    for (mut slam, mut font, mut color) in &mut grades {
        slam.age += time.delta_secs();
        let t = (slam.age / 0.35).min(1.0);
        // Ease-out-back: overshoot to ~1.1× then settle.
        let eased = 1.0 + 2.7 * (t - 1.0).powi(3) + 1.7 * (t - 1.0).powi(2);
        font.font_size = FontSize::Px(20.0 + 50.0 * eased);
        color.0 = palette::BRAND.with_alpha(t.min(1.0));
    }
}

/// The score earns itself back over a moment.
fn count_up_score(time: Res<Time>, mut scores: Query<(&mut ScoreCountUp, &mut Text)>) {
    for (mut count, mut text) in &mut scores {
        if count.age >= 0.9 {
            continue;
        }
        count.age += time.delta_secs();
        let t = (count.age / 0.9).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        let value = (count.target as f64 * f64::from(eased)) as u64;
        let shown = value.to_string();
        if text.0 != shown {
            text.0 = shown;
        }
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
