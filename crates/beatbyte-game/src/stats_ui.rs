//! One player's statistics, in four views.
//!
//! Each view answers one question and says so in its subtitle:
//!
//! - **OVERVIEW** — am I getting better? (accuracy per finished run,
//!   one line per difficulty, with the trend stated in words)
//! - **TIMING** — do I drift early or late, and did calibrating help?
//!   (mean offset per run against the zero line, plus the judgment
//!   mix the drift produced)
//! - **DIFFICULTY** — where do I actually play, and how far do I get?
//! - **VERSUS** — how do I stand against the others? Only on songs
//!   both have finished at the same difficulty, because that is the
//!   only comparison this game can make without inventing weights
//!   (`beatbyte_core::stats::head_to_head`).
//!
//! The drawing is [`crate::plot`]; the arithmetic is
//! [`beatbyte_core::stats`]. This module is the arrangement in
//! between, and holds no formula of its own — the CLI prints the same
//! numbers from the same functions.

use beatbyte_core::player::PlayerId;
use beatbyte_core::stats::{self, Filter, PlayerRun};
use bevy::prelude::*;
use bevy::ui::Val::Px as px;

use crate::controls::{InputMap, MenuNav};
use crate::history::PlayHistory;
use crate::palette;
use crate::players::Players;
use crate::plot;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Width of the plot area inside the wide panel.
const PLOT_W: f32 = 700.0;
/// Height of a line plot.
const PLOT_H: f32 = 170.0;
/// Width of a horizontal bar's track.
const BAR_W: f32 = 320.0;

/// Whose statistics the screen shows. Set by the roster before the
/// state change; falls back to whoever is playing.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct StatsFor(pub Option<PlayerId>);

/// The view `BEATBYTE_SHOT_VIEW` asks for, for the harness.
///
/// Without it only the first of four views could ever be
/// photographed, which is the same blind spot `BEATBYTE_SHOT_ROW`
/// exists to close for a scrolling list. Pure — tested.
#[must_use]
pub fn view_named(raw: &str) -> Option<View> {
    let wanted = raw.to_ascii_lowercase();
    View::ALL
        .into_iter()
        .find(|view| view.label().eq_ignore_ascii_case(&wanted))
}

/// Which of the four views is on screen.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsView(pub View);

/// The four views, in tab order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum View {
    /// Progress over time.
    #[default]
    Overview,
    /// Drift and judgment mix.
    Timing,
    /// Where the player plays.
    Difficulty,
    /// Against the other players.
    Versus,
}

impl View {
    /// All four, in tab order.
    pub const ALL: [View; 4] = [View::Overview, View::Timing, View::Difficulty, View::Versus];

    /// The tab's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            View::Overview => "OVERVIEW",
            View::Timing => "TIMING",
            View::Difficulty => "DIFFICULTY",
            View::Versus => "VERSUS",
        }
    }

    /// The question the view answers, shown under the title.
    #[must_use]
    pub const fn question(self) -> &'static str {
        match self {
            View::Overview => "AM I GETTING BETTER?",
            View::Timing => "DO I PLAY EARLY OR LATE?",
            View::Difficulty => "WHERE DO I PLAY, AND HOW FAR DO I GET?",
            View::Versus => "HOW DO I STAND AGAINST THE OTHERS?",
        }
    }

    /// The next view along.
    ///
    /// Stops at the ends rather than wrapping, because every list in
    /// this game does ([`ui_kit::step_cursor`]) — a screen with its
    /// own navigation model is the drift `ui_kit` exists to stop.
    /// Pure — tested.
    #[must_use]
    pub fn step(self, delta: i32) -> View {
        let at = View::ALL.iter().position(|v| *v == self).unwrap_or(0);
        View::ALL[ui_kit::step_cursor(at, View::ALL.len(), delta)]
    }
}

/// Everything this screen spawns.
#[derive(Component)]
struct StatsScreen;

/// Systems of the statistics screen.
pub struct StatsUiPlugin;

impl Plugin for StatsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatsFor>()
            .init_resource::<StatsView>()
            .add_systems(
                OnEnter(AppState::Stats),
                (pick_shot_view, spawn_stats).chain(),
            )
            .add_systems(Update, stats_nav.run_if(in_state(AppState::Stats)))
            .add_systems(OnExit(AppState::Stats), despawn_stats);
    }
}

/// Open the view the harness asked for, so all four can be
/// photographed rather than only the first.
fn pick_shot_view(mut view: ResMut<StatsView>) {
    if let Ok(raw) = std::env::var("BEATBYTE_SHOT_VIEW") {
        match view_named(&raw) {
            Some(wanted) => view.0 = wanted,
            None => bevy::log::error!("unknown BEATBYTE_SHOT_VIEW `{raw}`"),
        }
    }
}

/// A trend, said in words rather than left as a slope.
///
/// Per ten runs, because "0.0004 per run" is a number nobody can
/// picture. `None` below three runs — two points always fit a line,
/// and calling that a trend would claim more than the data holds.
/// Pure — tested.
#[must_use]
pub fn trend_line(slope: Option<f64>) -> String {
    let Some(slope) = slope else {
        return "NOT ENOUGH RUNS FOR A TREND".to_owned();
    };
    let per_ten = slope * 10.0 * 100.0;
    if per_ten.abs() < 0.5 {
        return "HOLDING STEADY".to_owned();
    }
    let direction = if per_ten > 0.0 { "UP" } else { "DOWN" };
    format!("{direction} {:.1} POINTS PER 10 RUNS", per_ten.abs())
}

/// How a drift reads to a player. The thresholds are the results
/// screen's, so one run and a hundred runs are described the same
/// way. Pure — tested.
#[must_use]
pub fn drift_line(mean_ms: Option<f64>) -> String {
    let Some(mean) = mean_ms else {
        return "NO TIMING RECORDED YET".to_owned();
    };
    if mean.abs() < 3.0 {
        "ON TIME".to_owned()
    } else if mean < 0.0 {
        format!("{:.0} MS EARLY ON AVERAGE", mean.abs())
    } else {
        format!("{mean:.0} MS LATE ON AVERAGE")
    }
}

/// The colour a difficulty's line wears. Reuses the judgment palette
/// so the four levels read as a ramp rather than four random hues.
#[must_use]
const fn difficulty_colour(difficulty: beatbyte_core::Difficulty) -> Color {
    match difficulty {
        beatbyte_core::Difficulty::Easy => palette::PERFECT,
        beatbyte_core::Difficulty::Medium => palette::GREAT,
        beatbyte_core::Difficulty::Hard => palette::GOOD,
        beatbyte_core::Difficulty::Expert => palette::MISS,
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system
fn spawn_stats(
    mut commands: Commands,
    font: Res<UiFont>,
    players: Res<Players>,
    history: Res<PlayHistory>,
    view: Res<StatsView>,
    chosen: Res<StatsFor>,
) {
    let Some(id) = chosen.0.or(players.0.selected) else {
        // Reached with nobody chosen: say so rather than drawing an
        // empty frame the player cannot explain.
        commands
            .spawn((ui_kit::screen_root(), StatsScreen))
            .with_children(|root| {
                ui_kit::header(root, &font, "STATISTICS", "NO PLAYER CHOSEN");
                root.spawn(ui_kit::panel()).with_children(|panel| {
                    plot::empty_note(panel, &font, "CREATE A PLAYER ON THE PLAYERS SCREEN FIRST");
                });
                ui_kit::footer(root, &font, "ESC BACK");
            });
        return;
    };
    let name = players
        .0
        .name_of(id)
        .map_or_else(|| "PLAYER".to_owned(), str::to_owned);
    let runs = stats::runs_of(&history.0, id, Filter::default());
    let summary = stats::summarize(&runs);

    commands
        .spawn((ui_kit::screen_root(), StatsScreen))
        .with_children(|root| {
            ui_kit::header(root, &font, &name.to_uppercase(), view.0.question());
            spawn_tabs(root, &font, view.0);
            root.spawn(ui_kit::panel_wide())
                .with_children(|panel| match view.0 {
                    View::Overview => overview(panel, &font, &runs, &summary),
                    View::Timing => timing(panel, &font, &runs, &summary),
                    View::Difficulty => difficulty(panel, &font, &runs),
                    View::Versus => versus(panel, &font, &history.0, &players, id, &name),
                });
            ui_kit::footer(root, &font, "LEFT/RIGHT SWITCH VIEW   ESC BACK");
        });
}

/// The row of view tabs.
fn spawn_tabs(parent: &mut ChildSpawnerCommands, font: &UiFont, active: View) {
    parent
        .spawn(Node {
            column_gap: px(18.0),
            margin: UiRect::bottom(px(12.0)),
            ..default()
        })
        .with_children(|tabs| {
            for view in View::ALL {
                tabs.spawn((
                    Text::new(view.label().to_owned()),
                    font.text(ui_kit::SMALL),
                    TextColor(if view == active {
                        palette::BRAND
                    } else {
                        ui_kit::dimmed_subtitle()
                    }),
                ));
            }
        });
}

/// A labelled figure, the screen's unit of "one number".
fn tile(parent: &mut ChildSpawnerCommands, font: &UiFont, label: &str, value: &str) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            min_width: px(120.0),
            ..default()
        })
        .with_children(|cell| {
            cell.spawn((
                Text::new(label.to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            cell.spawn((
                Text::new(value.to_owned()),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
            ));
        });
}

/// A row of tiles.
fn tiles(parent: &mut ChildSpawnerCommands, font: &UiFont, cells: &[(&str, String)]) {
    parent
        .spawn(Node {
            column_gap: px(ui_kit::CELL_GAP),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(10.0),
            margin: UiRect::bottom(px(14.0)),
            ..default()
        })
        .with_children(|row| {
            for (label, value) in cells {
                tile(row, font, label, value);
            }
        });
}

/// A duration, said the way a person would.
#[must_use]
pub fn played_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    if minutes < 90 {
        format!("{minutes} MIN")
    } else {
        format!("{:.1} H", seconds / 3600.0)
    }
}

/// OVERVIEW: the headline numbers and the accuracy line per
/// difficulty.
fn overview(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
) {
    tiles(
        parent,
        font,
        &[
            ("RUNS", summary.runs.to_string()),
            ("FINISHED", summary.completed.to_string()),
            ("SONGS", summary.songs.to_string()),
            ("PLAYED", played_time(summary.seconds)),
            (
                "BEST ACCURACY",
                summary
                    .best_accuracy
                    .map_or_else(|| "-".to_owned(), plot::percent),
            ),
            (
                "BEST SCORE",
                summary
                    .best_score
                    .map_or_else(|| "-".to_owned(), |score| score.to_string()),
            ),
            (
                "BEST STREAK",
                summary
                    .best_streak
                    .map_or_else(|| "-".to_owned(), |s| s.to_string()),
            ),
        ],
    );

    let points = stats::progression(runs);
    // One line per difficulty: a single line mixing Easy and Expert
    // would show a player "getting worse" the day they moved up.
    let series: Vec<plot::Series> = beatbyte_core::Difficulty::ALL
        .into_iter()
        .filter_map(|difficulty| {
            let values: Vec<f64> = points
                .iter()
                .filter(|p| p.difficulty == Some(difficulty))
                .map(|p| p.accuracy)
                .collect();
            (!values.is_empty()).then(|| plot::Series {
                label: difficulty.display_name().to_uppercase(),
                colour: difficulty_colour(difficulty),
                values,
            })
        })
        .collect();
    let bounds = plot::Bounds::around(points.iter().map(|p| p.accuracy))
        .unwrap_or(plot::Bounds { min: 0.0, max: 1.0 })
        .clamped(0.0, 1.0);
    plot::spawn_line_plot(
        parent,
        font,
        &plot::LinePlot {
            width: PLOT_W,
            height: PLOT_H,
            series: &series,
            bounds,
            rule: None,
            label: plot::percent,
            caption: "FINISHED RUNS, OLDEST FIRST".to_owned(),
        },
    );
    // The trend is taken per difficulty for the same reason, and the
    // one with the most runs is the one worth stating.
    let busiest = series.iter().max_by_key(|s| s.values.len());
    let trend = busiest.and_then(|s| stats::trend(&s.values));
    parent.spawn((
        Node {
            margin: UiRect::top(px(10.0)),
            ..default()
        },
        Text::new(busiest.map_or_else(
            || "NO FINISHED RUNS YET".to_owned(),
            |s| format!("{}: {}", s.label, trend_line(trend)),
        )),
        font.text(ui_kit::SMALL),
        TextColor(palette::BRAND),
    ));
}

/// TIMING: drift against the zero line, and the judgment mix.
fn timing(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    runs: &[PlayerRun<'_>],
    summary: &stats::Summary,
) {
    tiles(
        parent,
        font,
        &[
            ("AVERAGE DRIFT", drift_line(summary.mean_offset_ms)),
            (
                "PERFECT SHARE",
                summary
                    .perfect_share
                    .map_or_else(|| "-".to_owned(), plot::percent),
            ),
            (
                "OVERSTRUMS / MIN",
                summary
                    .overstrums_per_min
                    .map_or_else(|| "-".to_owned(), |v| format!("{v:.1}")),
            ),
        ],
    );

    let drift: Vec<f64> = stats::progression(runs)
        .iter()
        .filter_map(|p| p.mean_offset_ms)
        .collect();
    let series = [plot::Series {
        label: "DRIFT".to_owned(),
        colour: palette::HYPE,
        values: drift.clone(),
    }];
    // The zero line has to be on screen even when every run was
    // late, or "late" has nothing to be late against.
    let bounds = plot::Bounds::around(drift.iter().copied())
        .unwrap_or(plot::Bounds {
            min: -30.0,
            max: 30.0,
        })
        .including(0.0);
    plot::spawn_line_plot(
        parent,
        font,
        &plot::LinePlot {
            width: PLOT_W,
            height: PLOT_H,
            series: &series,
            bounds,
            rule: Some(0.0),
            label: plot::millis,
            caption: "ABOVE THE LINE IS LATE".to_owned(),
        },
    );

    parent.spawn((
        Node {
            margin: UiRect::top(px(16.0)).with_bottom(px(6.0)),
            ..default()
        },
        Text::new("JUDGMENT MIX".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let (mut perfect, mut great, mut good, mut miss) = (0_u64, 0_u64, 0_u64, 0_u64);
    for run in runs {
        let detail = &run.part.detail;
        perfect += u64::from(detail.perfect.unwrap_or(0));
        great += u64::from(detail.great.unwrap_or(0));
        good += u64::from(detail.good.unwrap_or(0));
        miss += u64::from(detail.miss.unwrap_or(0));
    }
    let total = (perfect + great + good + miss) as f64;
    if total > 0.0 {
        let slice = |count: u64, colour: Color| plot::Slice {
            share: count as f64 / total,
            colour,
        };
        plot::spawn_stack(
            parent,
            &[
                slice(perfect, palette::PERFECT),
                slice(great, palette::GREAT),
                slice(good, palette::GOOD),
                slice(miss, palette::MISS),
            ],
            PLOT_W,
            14.0,
        );
        parent.spawn((
            Node {
                margin: UiRect::top(px(5.0)),
                ..default()
            },
            Text::new(format!(
                "PERFECT {perfect}   GREAT {great}   GOOD {good}   MISS {miss}"
            )),
            font.text(ui_kit::SMALL),
            TextColor(ui_kit::dimmed_subtitle()),
        ));
    } else {
        // Runs older than the judgment counts. Saying so is the
        // honest answer; a bar of zeroes would not be.
        plot::empty_note(parent, font, "NO RUN HERE RECORDED ITS JUDGMENTS YET");
    }
}

/// DIFFICULTY: where the player plays, and how far they get.
fn difficulty(parent: &mut ChildSpawnerCommands, font: &UiFont, runs: &[PlayerRun<'_>]) {
    let split = stats::by_difficulty(runs);
    parent.spawn((
        Node {
            margin: UiRect::bottom(px(8.0)),
            ..default()
        },
        Text::new("BEST ACCURACY".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let bars: Vec<plot::Bar> = split
        .iter()
        .map(|stat| plot::Bar {
            label: stat.difficulty.display_name().to_uppercase(),
            // No finished run means no accuracy — not a zero. A bar
            // of length zero and a bar that was never earned must not
            // look alike.
            value: stat.best_accuracy,
            colour: difficulty_colour(stat.difficulty),
            note: match stat.best_accuracy {
                Some(best) => format!("{}   {} RUNS", plot::percent(best), stat.runs),
                None if stat.runs > 0 => format!("{} RUNS, NONE FINISHED", stat.runs),
                None => "NEVER PLAYED".to_owned(),
            },
        })
        .collect();
    plot::spawn_bar_plot(
        parent,
        font,
        &bars,
        plot::Bounds { min: 0.0, max: 1.0 },
        BAR_W,
    );

    parent.spawn((
        Node {
            margin: UiRect::top(px(16.0)).with_bottom(px(8.0)),
            ..default()
        },
        Text::new("RUNS FINISHED".to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
    let finished: Vec<plot::Bar> = split
        .iter()
        .map(|stat| plot::Bar {
            label: stat.difficulty.display_name().to_uppercase(),
            value: stat.completion(),
            colour: difficulty_colour(stat.difficulty),
            note: stat.completion().map_or_else(
                || "NEVER PLAYED".to_owned(),
                |share| {
                    format!(
                        "{}   {} OF {}",
                        plot::percent(share),
                        stat.completed,
                        stat.runs
                    )
                },
            ),
        })
        .collect();
    plot::spawn_bar_plot(
        parent,
        font,
        &finished,
        plot::Bounds { min: 0.0, max: 1.0 },
        BAR_W,
    );
}

/// VERSUS: the same song at the same difficulty, or nothing.
fn versus(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    entries: &[beatbyte_core::history::PlayEntry],
    players: &Players,
    id: PlayerId,
    name: &str,
) {
    let others: Vec<&beatbyte_core::player::Player> =
        players.0.players.iter().filter(|p| p.id != id).collect();
    if others.is_empty() {
        plot::empty_note(
            parent,
            font,
            "ONLY ONE PLAYER ON THIS MACHINE - ADD ANOTHER TO COMPARE",
        );
        return;
    }
    for other in others {
        let duels = stats::head_to_head(entries, id, other.id);
        parent.spawn((
            Node {
                margin: UiRect::top(px(10.0)).with_bottom(px(6.0)),
                ..default()
            },
            Text::new(format!(
                "{} VS {}",
                name.to_uppercase(),
                other.name.to_uppercase()
            )),
            font.text(ui_kit::ROW),
            TextColor(palette::TEXT),
        ));
        if duels.is_empty() {
            // The honest empty state, and the reason it is empty:
            // this game will not rank two players who have never
            // played the same thing.
            plot::empty_note(
                parent,
                font,
                "NO SONG BOTH HAVE FINISHED AT THE SAME DIFFICULTY YET",
            );
            continue;
        }
        let ahead = duels.iter().filter(|duel| duel.margin() > 0.0).count();
        parent.spawn((
            Text::new(format!("AHEAD ON {ahead} OF {} SHARED SONGS", duels.len())),
            font.text(ui_kit::SMALL),
            TextColor(palette::BRAND),
        ));
        let rows: Vec<plot::Duel> = duels
            .iter()
            .take(8)
            .map(|duel| plot::Duel {
                label: format!(
                    "{} ({})",
                    duel.title.to_uppercase(),
                    duel.difficulty.to_uppercase()
                ),
                margin: duel.margin(),
                note: format!(
                    "{} / {}",
                    plot::percent(duel.theirs),
                    plot::percent(duel.others)
                ),
            })
            .collect();
        plot::spawn_duel_plot(parent, font, &rows, BAR_W, palette::PERFECT, palette::MISS);
    }
}

/// Tabs, and leaving.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn stats_nav(
    map: Res<InputMap>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mut view: ResMut<StatsView>,
    mut next: ResMut<NextState<AppState>>,
    mut commands: Commands,
    screen: Query<Entity, With<StatsScreen>>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let delta = i32::from(nav.right) - i32::from(nav.left);
    if delta != 0 {
        view.0 = view.0.step(delta);
        // A view change is a respawn: the panel's contents differ in
        // shape, not just in text, and rebuilding is cheaper to get
        // right than a dozen refresh systems.
        for entity in &screen {
            commands.entity(entity).despawn();
        }
        commands.run_system_cached(spawn_stats);
    }
    if nav.back {
        next.set(AppState::Players);
    }
}

fn despawn_stats(mut commands: Commands, entities: Query<Entity, With<StatsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::player::Roster;

    #[test]
    fn the_views_step_and_stop_at_the_ends_like_every_other_list() {
        assert_eq!(View::Overview.step(1), View::Timing);
        assert_eq!(View::Timing.step(-1), View::Overview);
        // Clamping, not wrapping: `ui_kit::step_cursor` is the one
        // cursor rule in this game, and tabs are not an exception.
        assert_eq!(View::Versus.step(1), View::Versus, "the last wrapped");
        assert_eq!(View::Overview.step(-1), View::Overview);
        assert_eq!(View::Overview.step(0), View::Overview);
    }

    #[test]
    fn a_view_can_be_named_for_the_harness() {
        assert_eq!(view_named("timing"), Some(View::Timing));
        assert_eq!(view_named("VERSUS"), Some(View::Versus));
        assert_eq!(view_named("nonsense"), None);
    }

    #[test]
    fn every_view_states_the_question_it_answers() {
        // A tab labelled with a noun leaves the player to infer what
        // the plot is for; the subtitle is the answer.
        for view in View::ALL {
            assert!(view.question().ends_with('?'), "{}", view.label());
            assert!(!view.label().is_empty());
        }
    }

    #[test]
    fn a_trend_is_stated_per_ten_runs_or_not_at_all() {
        assert_eq!(trend_line(None), "NOT ENOUGH RUNS FOR A TREND");
        // 0.004 per run = 4 points per 10 runs.
        assert_eq!(trend_line(Some(0.004)), "UP 4.0 POINTS PER 10 RUNS");
        assert_eq!(trend_line(Some(-0.004)), "DOWN 4.0 POINTS PER 10 RUNS");
        // Noise is not a direction.
        assert_eq!(trend_line(Some(0.0001)), "HOLDING STEADY");
    }

    #[test]
    fn drift_reads_as_a_side_and_matches_the_results_screen() {
        // The same 3 ms window the results screen calls "on time",
        // so one run and a hundred are described the same way.
        assert_eq!(drift_line(None), "NO TIMING RECORDED YET");
        assert_eq!(drift_line(Some(1.5)), "ON TIME");
        assert_eq!(drift_line(Some(-2.9)), "ON TIME");
        assert_eq!(drift_line(Some(-18.0)), "18 MS EARLY ON AVERAGE");
        assert_eq!(drift_line(Some(24.0)), "24 MS LATE ON AVERAGE");
    }

    /// An app that can run `spawn_stats` for real: the resources it
    /// reads, a font handle, and nothing else.
    fn wired(history: Vec<beatbyte_core::history::PlayEntry>, roster: Roster) -> App {
        let mut app = App::new();
        app.add_plugins((bevy::MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Font>()
            .insert_resource(crate::players::Players(roster))
            .insert_resource(PlayHistory(history))
            .init_resource::<StatsFor>()
            .init_resource::<StatsView>();
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .reserve_handle();
        app.insert_resource(UiFont::from_handle(font));
        app
    }

    fn a_run(
        player: Option<PlayerId>,
        accuracy: f64,
        completed: bool,
    ) -> beatbyte_core::history::PlayEntry {
        beatbyte_core::history::PlayEntry {
            title: "Africa".to_owned(),
            artist: "Toto".to_owned(),
            difficulty: "medium".to_owned(),
            started_ms: 1_756_000_000_000,
            played_s: 120.0,
            track_s: Some(120.0),
            completed,
            players: 1,
            practice: false,
            autopilot: false,
            score: 4844,
            accuracy,
            source: "file".to_owned(),
            player,
            detail: beatbyte_core::history::RunDetail {
                best_streak: Some(40),
                perfect: Some(80),
                great: Some(10),
                good: Some(5),
                miss: Some(5),
                overstrums: Some(2),
                mean_offset_ms: Some(-12.0),
            },
            co_players: Vec::new(),
            chart_hash: None,
        }
    }

    /// How many nodes a view builds for a given history.
    fn nodes_for(
        view: View,
        history: Vec<beatbyte_core::history::PlayEntry>,
        roster: &Roster,
    ) -> usize {
        let mut app = wired(history, roster.clone());
        app.world_mut().resource_mut::<StatsView>().0 = view;
        app.add_systems(Update, spawn_stats);
        app.update();
        let screens = app
            .world_mut()
            .query_filtered::<Entity, With<StatsScreen>>()
            .iter(app.world())
            .count();
        assert_eq!(screens, 1, "{} spawned {screens} screens", view.label());
        app.world_mut().query::<&Node>().iter(app.world()).count()
    }

    /// Every view builds for every shape of history, and the data
    /// actually reaches the drawing.
    ///
    /// The locked-screen lesson in reverse: a screenshot proves a
    /// screen renders, but only when somebody can see it. This proves
    /// the four views BUILD — which is where the panics live (an
    /// empty series, a difficulty nobody finished, a player with no
    /// runs at all) — and that a view with runs in it draws
    /// materially MORE than the same view with none. A bare "some
    /// nodes exist" pin passed even with every plot short-circuited
    /// to its empty note, because the tiles and header alone clear
    /// it; the comparison is what bites.
    #[test]
    fn every_view_builds_and_a_history_draws_more_than_an_empty_one() {
        let mut roster = Roster::default();
        let id = roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let full = vec![
            a_run(Some(id), 0.85, true),
            a_run(Some(id), 0.62, true),
            a_run(Some(id), 0.20, false),
        ];
        // VERSUS is excluded here on purpose and pinned separately:
        // with one player in the roster it correctly says "only one
        // player" whatever the history holds, so comparing it here
        // would pin the wrong thing.
        for view in [View::Overview, View::Timing, View::Difficulty] {
            let empty = nodes_for(view, Vec::new(), &roster);
            let drawn = nodes_for(view, full.clone(), &roster);
            assert!(
                drawn > empty,
                "{}: {drawn} nodes with three runs, {empty} with none - \
                 the history is not reaching the drawing",
                view.label()
            );
        }
        // The shapes that have no plot to draw must still build: a
        // player who finished nothing, and one who played nothing.
        for (label, history) in [
            ("runs but none finished", vec![a_run(Some(id), 0.0, false)]),
            ("one finished run", vec![a_run(Some(id), 0.85, true)]),
        ] {
            for view in View::ALL {
                let nodes = nodes_for(view, history.clone(), &roster);
                assert!(nodes > 0, "{} drew nothing for {label}", view.label());
            }
        }
    }

    /// VERSUS draws a duel when there is one to draw.
    ///
    /// Its whole contract is the one the design argued for: compare
    /// like with like, or say nothing. Both halves are pinned here —
    /// a shared song draws more than no shared song, and neither
    /// panics.
    #[test]
    fn versus_draws_a_duel_only_when_two_players_have_met_on_a_song() {
        let mut roster = Roster::default();
        let mine = roster
            .add("Martin", 1)
            .expect("a fresh roster takes a name");
        let theirs = roster.add("Kim", 2).expect("a second name is free");

        let mut apart = a_run(Some(theirs), 0.5, true);
        apart.title = "A different song".to_owned();
        let unshared = vec![a_run(Some(mine), 0.85, true), apart];
        let shared = vec![
            a_run(Some(mine), 0.85, true),
            a_run(Some(theirs), 0.5, true),
        ];

        let without = nodes_for(View::Versus, unshared, &roster);
        let with_duel = nodes_for(View::Versus, shared, &roster);
        assert!(
            with_duel > without,
            "a shared song drew {with_duel} nodes and no shared song {without} -              the duel is not reaching the drawing"
        );
    }

    /// A player the roster does not know must not panic the screen.
    #[test]
    fn a_screen_without_a_player_says_so_instead_of_drawing_nothing() {
        let mut app = wired(Vec::new(), Roster::default());
        app.add_systems(Update, spawn_stats);
        app.update();
        let spawned = app
            .world_mut()
            .query_filtered::<Entity, With<StatsScreen>>()
            .iter(app.world())
            .count();
        assert_eq!(spawned, 1, "an empty roster drew no screen at all");
    }

    #[test]
    fn a_played_time_is_minutes_until_it_is_hours() {
        assert_eq!(played_time(0.0), "0 MIN");
        assert_eq!(played_time(600.0), "10 MIN");
        assert_eq!(played_time(7200.0), "2.0 H");
    }
}
