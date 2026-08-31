//! The results screen: solo glory or the band's verdict.

use bevy::prelude::*;

use crate::gameplay::{LastResults, player_color};
use crate::multiplayer::MultiplayerMode;
use crate::palette;
use crate::scores::{BestScore, ScoreBoard, save_scores};
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Plugin for the results screen.
pub struct ResultsPlugin;

impl Plugin for ResultsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FeedbackGiven>()
            .add_systems(OnEnter(AppState::Results), spawn_results)
            .add_systems(
                Update,
                (results_input, animate_grade, count_up_score).run_if(in_state(AppState::Results)),
            )
            .add_systems(OnExit(AppState::Results), despawn_results);
    }
}

#[derive(Component)]
struct ResultsScreen;

/// The line that confirms recorded feedback (A5), updated in place.
#[derive(Component)]
struct FeedbackStatus;

/// What feedback this results visit has recorded so far. A resource
/// reset on every spawn — a `Local` would leak the previous song's
/// rating into the next results screen's status line.
#[derive(Resource, Default)]
struct FeedbackGiven {
    fun: Option<u8>,
    versus: Option<&'static str>,
}

/// Which digit key rates how much fun (1 = none, 5 = loved it).
fn fun_rating_for(key: KeyCode) -> Option<u8> {
    match key {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(1),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(2),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(3),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(4),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(5),
        _ => None,
    }
}

/// The footer for the current feedback offer. No session log means
/// no rating hint — a key hint that does nothing would be a lie, and
/// skipping must stay free of any nudge (A5: zero friction).
fn results_footer(can_rate: bool, can_versus: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if can_rate {
        parts.push("1-5 rate fun");
    }
    if can_versus {
        parts.push("LEFT worse than before");
        parts.push("RIGHT better");
    }
    parts.push("ENTER back to menu");
    parts.join("  ")
}

/// What the status line says for the feedback given so far.
fn feedback_status(fun: Option<u8>, versus: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(rating) = fun {
        parts.push(format!("fun {rating}/5"));
    }
    if let Some(verdict) = versus {
        parts.push(format!("felt {verdict} than the previous version"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("recorded: {}", parts.join(" · "))
    }
}

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
    logs: Option<Res<crate::telemetry::SessionLogFiles>>,
    song: Option<Res<crate::boot::LoadedSong>>,
    mut given: ResMut<FeedbackGiven>,
    font: Res<UiFont>,
) {
    let Some(results) = results else {
        return;
    };
    // Feedback (A5) is offered when this run left a session log and
    // a human played: the fun rating appends to that log, and the
    // versus verdict additionally needs a parent version to compare
    // against (the played chart's provenance carries it).
    // Autopilot sessions stay ratable on purpose: the header marks
    // them and the review excludes them by default, and the rating
    // drill needs the REAL path (a gate here would force the drill
    // to test a bypass instead).
    let can_rate = logs.is_some_and(|l| !l.files.is_empty());
    let can_versus = can_rate && song.is_some_and(|s| s.chart.provenance.is_some());
    *given = FeedbackGiven::default();

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
        .spawn((ResultsScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            if solo {
                spawn_solo(parent, &results, new_record, &font);
            } else {
                spawn_multi(parent, &results, &font);
            }
            // The confirmation line sits above the footer, empty
            // until feedback is given — no visual noise for the
            // player who just wants out.
            parent.spawn((
                FeedbackStatus,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT_DIM),
            ));
            ui_kit::footer(parent, &font, &results_footer(can_rate, can_versus));
        });
}

/// One judgment row: a colour chip, its name, and how many.
fn tally(parent: &mut ChildSpawnerCommands, font: &UiFont, label: &str, count: u32, colour: Color) {
    parent.spawn(ui_kit::row()).with_children(|row| {
        // ONE Node, not two: an explicit `Node` beside `label_node()`
        // puts two of the same component in one bundle, and Bevy
        // rejects that at spawn time rather than merging them.
        row.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(10),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|left| {
            // The chip is the same colour the judgment popped in
            // during the song, so the breakdown reads as a
            // summary of what was on screen rather than a table.
            left.spawn((
                Node {
                    width: px(10),
                    height: px(10),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(colour),
            ));
            left.spawn((
                Text::new(label.to_owned()),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT_DIM),
            ));
        });
        row.spawn((
            Text::new(count.to_string()),
            font.text(ui_kit::ROW),
            TextColor(if count == 0 {
                palette::dimmed(palette::TEXT_DIM, 0.6)
            } else {
                palette::TEXT
            }),
            ui_kit::value_node(),
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

    // The song is the subject of this screen, so it gets the heading
    // rather than a dim line under the grade.
    ui_kit::header(
        parent,
        font,
        &font.safe(&results.title).to_uppercase(),
        &format!("{} - {}", font.safe(&results.artist), results.difficulty),
    );

    parent.spawn(ui_kit::panel()).with_children(|panel| {
        // ── The verdict: grade badge beside the score counter ───────
        panel
            .spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(22),
                padding: UiRect::axes(px(14), px(10)),
                ..default()
            })
            .with_children(|top| {
                top.spawn((
                    Node {
                        width: px(96),
                        height: px(96),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(10)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(palette::BRAND.with_alpha(0.10)),
                    BorderColor::all(palette::BRAND),
                ))
                .with_child((
                    GradeSlam { age: 0.0 },
                    Text::new(grade),
                    font.text(20.0),
                    TextColor(palette::BRAND.with_alpha(0.0)),
                ));
                top.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|block| {
                    block.spawn((
                        Text::new("SCORE"),
                        font.text(ui_kit::SMALL),
                        TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
                    ));
                    block.spawn((
                        ScoreCountUp {
                            target: perf.score(),
                            age: 0.0,
                        },
                        Text::new("0"),
                        font.text(30.0),
                        TextColor(palette::TEXT),
                    ));
                    if new_record {
                        block.spawn((
                            Text::new("NEW RECORD"),
                            font.text(ui_kit::SMALL),
                            TextColor(palette::PERFECT),
                        ));
                    }
                });
            });

        // ── Accuracy, as a bar and a number ─────────────────────────
        panel.spawn(ui_kit::row()).with_children(|row| {
            row.spawn((
                Text::new("ACCURACY"),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT_DIM),
                ui_kit::label_node(),
            ));
            row.spawn((
                Text::new(format!("{accuracy:.1}%")),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                ui_kit::value_node(),
            ));
        });
        // A bar says "nearly all of it" at a glance, which a figure to
        // one decimal place does not.
        panel
            .spawn((
                Node {
                    width: percent(100),
                    height: px(8),
                    margin: UiRect::axes(px(14), px(2)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.22)),
            ))
            .with_child((
                Node {
                    width: percent(accuracy),
                    height: percent(100),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(if counts.miss == 0 {
                    palette::PERFECT
                } else {
                    palette::BRAND
                }),
            ));

        tally(panel, font, "PERFECT", counts.perfect, palette::PERFECT);
        tally(panel, font, "GREAT", counts.great, palette::GREAT);
        tally(panel, font, "GOOD", counts.good, palette::GOOD);
        tally(panel, font, "MISS", counts.miss, palette::MISS);
        tally(
            panel,
            font,
            "OVERSTRUMS",
            perf.overstrums(),
            palette::dimmed(palette::MISS, 0.6),
        );

        panel.spawn(ui_kit::row()).with_children(|row| {
            row.spawn((
                Text::new("BEST STREAK"),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT_DIM),
                ui_kit::label_node(),
            ));
            row.spawn((
                Text::new(perf.best_streak().to_string()),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                ui_kit::value_node(),
            ));
        });
    });
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
        Text::new(format!(
            "\"{}\" on {}",
            font.safe(&results.title),
            results.difficulty
        )),
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
        // Capped so the letter stays inside its badge: the old
        // free-standing version could grow to any size it liked.
        font.font_size = FontSize::Px(16.0 + 40.0 * eased);
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

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn results_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    logs: Option<Res<crate::telemetry::SessionLogFiles>>,
    song: Option<Res<crate::boot::LoadedSong>>,
    sfx: Option<Res<crate::sfx::SfxLib>>,
    settings: Res<crate::config::Settings>,
    mut status: Query<&mut Text, With<FeedbackStatus>>,
    mut given: ResMut<FeedbackGiven>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    // Feedback first, so a digit or arrow never doubles as an exit.
    let can_rate = logs.as_ref().is_some_and(|l| !l.files.is_empty());
    if can_rate && let Some(logs) = logs.as_ref() {
        let mut changed = false;
        if let Some(rating) = keys.get_just_pressed().find_map(|k| fun_rating_for(*k)) {
            crate::telemetry::append_feedback(
                logs,
                &beatbyte_core::telemetry::NoteLine::Fun { fun: rating },
            );
            given.fun = Some(rating);
            changed = true;
        }
        let parent_hash = song
            .as_ref()
            .and_then(|s| s.chart.provenance.as_ref())
            .map(|p| p.parent_hash.clone());
        if let Some(parent) = parent_hash {
            let verdict = if keys.just_pressed(KeyCode::ArrowLeft) {
                Some("worse")
            } else if keys.just_pressed(KeyCode::ArrowRight) {
                Some("better")
            } else {
                None
            };
            if let Some(verdict) = verdict {
                crate::telemetry::append_feedback(
                    logs,
                    &beatbyte_core::telemetry::NoteLine::Versus {
                        versus: verdict.to_owned(),
                        parent,
                    },
                );
                given.versus = Some(verdict);
                changed = true;
            }
        }
        if changed {
            if let Ok(mut text) = status.single_mut() {
                text.0 = feedback_status(given.fun, given.versus);
            }
            if let Some(sfx) = sfx.as_ref() {
                crate::sfx::play(&mut commands, &sfx.ui_move, settings.sfx_volume);
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::Escape)
        || mouse.just_pressed(MouseButton::Left)
        || mouse.just_pressed(MouseButton::Right)
    {
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_results(mut commands: Commands, entities: Query<Entity, With<ResultsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::{feedback_status, fun_rating_for, grade_for, results_footer};
    use bevy::prelude::KeyCode;

    #[test]
    fn digits_map_to_their_rating_and_nothing_else_rates() {
        assert_eq!(fun_rating_for(KeyCode::Digit1), Some(1));
        assert_eq!(fun_rating_for(KeyCode::Digit5), Some(5));
        assert_eq!(fun_rating_for(KeyCode::Numpad3), Some(3));
        // The keys that mean something else on this screen must
        // never double as a rating.
        for key in [
            KeyCode::Enter,
            KeyCode::Escape,
            KeyCode::Digit6,
            KeyCode::Digit0,
            KeyCode::ArrowLeft,
        ] {
            assert_eq!(fun_rating_for(key), None, "{key:?} must not rate");
        }
    }

    #[test]
    fn the_footer_offers_only_what_works() {
        // Zero friction cuts both ways: no phantom hints when there
        // is no log to rate into, and the exit hint is always there.
        assert_eq!(results_footer(false, false), "ENTER back to menu");
        let rate_only = results_footer(true, false);
        assert!(rate_only.contains("1-5 rate fun"));
        assert!(
            !rate_only.contains("worse"),
            "no versus hint without a parent"
        );
        let full = results_footer(true, true);
        assert!(full.contains("LEFT worse"));
        assert!(full.contains("RIGHT better"));
        assert!(full.ends_with("ENTER back to menu"));
    }

    #[test]
    fn the_status_line_reads_back_what_was_recorded() {
        assert_eq!(feedback_status(None, None), "");
        assert_eq!(feedback_status(Some(4), None), "recorded: fun 4/5");
        assert_eq!(
            feedback_status(Some(4), Some("better")),
            "recorded: fun 4/5 · felt better than the previous version"
        );
        assert_eq!(
            feedback_status(None, Some("worse")),
            "recorded: felt worse than the previous version"
        );
    }

    #[test]
    fn grade_thresholds_are_exact() {
        // S is accuracy AND perfection: 97%+ with zero misses.
        assert_eq!(grade_for(97.0, 0), "S");
        assert_eq!(grade_for(100.0, 0), "S");
        // One miss demotes even a 100% weighted accuracy to A.
        assert_eq!(grade_for(100.0, 1), "A");
        assert_eq!(grade_for(96.9, 0), "A");
        assert_eq!(grade_for(92.0, 5), "A");
        assert_eq!(grade_for(91.9, 0), "B");
        assert_eq!(grade_for(82.0, 0), "B");
        assert_eq!(grade_for(81.9, 0), "C");
        assert_eq!(grade_for(70.0, 0), "C");
        assert_eq!(grade_for(55.0, 0), "D");
        assert_eq!(grade_for(54.9, 0), "E");
    }
}
