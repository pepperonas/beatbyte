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
            .init_resource::<CommentField>()
            .init_resource::<ActionBarClicks>()
            .init_resource::<FeedbackOffer>()
            .add_systems(OnEnter(AppState::Results), spawn_results)
            .add_systems(
                Update,
                // The field runs FIRST and owns every key while it is
                // open: the browser's lesson, that one system must
                // own a text field, or the keystroke that opened it
                // lands inside it. Chips paint before both.
                (
                    paint_action_bar.before(results_comment),
                    (
                        results_comment,
                        results_input,
                        animate_grade,
                        count_up_score,
                    )
                        .chain(),
                )
                    .run_if(in_state(AppState::Results)),
            )
            .add_systems(OnExit(AppState::Results), despawn_results);
    }
}

/// What feedback this visit can accept (set once at spawn).
#[derive(Resource, Default, Clone, Copy)]
struct FeedbackOffer {
    rate: bool,
    versus: bool,
    taste: bool,
}

/// ActionBar chip ids on the results screen.
mod chip {
    pub const RATE_1: u8 = 1;
    pub const RATE_2: u8 = 2;
    pub const RATE_3: u8 = 3;
    pub const RATE_4: u8 = 4;
    pub const RATE_5: u8 = 5;
    pub const COMMENT: u8 = 10;
    pub const LEFT: u8 = 11;
    pub const RIGHT: u8 = 12;
    pub const SAME: u8 = 13;
}

#[derive(Resource, Default)]
struct ActionBarClicks(Vec<u8>);

fn results_chips(offer: FeedbackOffer) -> Vec<ui_kit::ChipSpec> {
    let mut chips = Vec::new();
    if offer.rate {
        for (id, label) in [
            (chip::RATE_1, "1"),
            (chip::RATE_2, "2"),
            (chip::RATE_3, "3"),
            (chip::RATE_4, "4"),
            (chip::RATE_5, "5"),
        ] {
            chips.push(ui_kit::ChipSpec {
                id,
                label,
                enabled: true,
            });
        }
        chips.push(ui_kit::ChipSpec {
            id: chip::COMMENT,
            label: "Comment",
            enabled: true,
        });
    }
    if offer.taste {
        chips.push(ui_kit::ChipSpec {
            id: chip::LEFT,
            label: "1st better",
            enabled: true,
        });
        chips.push(ui_kit::ChipSpec {
            id: chip::RIGHT,
            label: "2nd better",
            enabled: true,
        });
        chips.push(ui_kit::ChipSpec {
            id: chip::SAME,
            label: "Same",
            enabled: true,
        });
    } else if offer.versus {
        chips.push(ui_kit::ChipSpec {
            id: chip::LEFT,
            label: "Worse",
            enabled: true,
        });
        chips.push(ui_kit::ChipSpec {
            id: chip::RIGHT,
            label: "Better",
            enabled: true,
        });
    }
    chips
}

fn paint_action_bar(
    mut chips: Query<(
        &ui_kit::ActionChip,
        &ui_kit::ChipEnabled,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &Children,
    )>,
    mut labels: Query<&mut TextColor>,
    mut clicks: ResMut<ActionBarClicks>,
) {
    clicks.0 = ui_kit::read_chips(&mut chips, &mut labels);
}

/// Which fun rating a chip press means (1–5), if any.
fn chip_rating(clicks: &[u8]) -> Option<u8> {
    [
        chip::RATE_1,
        chip::RATE_2,
        chip::RATE_3,
        chip::RATE_4,
        chip::RATE_5,
    ]
    .into_iter()
    .find(|&id| ui_kit::chip_hit(clicks, id))
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
    /// How many sentences this visit recorded. A rating replaces the
    /// previous one; a sentence is added to it.
    comments: usize,
    /// The blind test's verdict and its reveal, once one was given.
    taste: Option<String>,
}

/// The comment field: closed until `C` opens it.
#[derive(Resource, Default)]
struct CommentField {
    open: bool,
    text: String,
}

/// What ENTER does on the results screen.
///
/// The browser learned this the hard way (`song_select::may_start`):
/// while a text field is taking keys, a printable key is text — and
/// Enter belongs to the field, not to the screen behind it. Without
/// this, finishing a sentence would leave the screen and throw the
/// sentence away. Pure — tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterMeans {
    /// Commit what was typed.
    CommitComment,
    /// Leave for the browser.
    Leave,
}

/// See [`EnterMeans`].
#[must_use]
pub fn enter_means(comment_open: bool) -> EnterMeans {
    if comment_open {
        EnterMeans::CommitComment
    } else {
        EnterMeans::Leave
    }
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
fn results_footer(can_rate: bool, can_versus: bool, taste: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if can_rate {
        parts.push("chips rate and comment");
    }
    if taste {
        parts.push("or 1st / 2nd / Same chips");
    } else if can_versus {
        parts.push("or Worse / Better chips");
    }
    parts.push("ENTER back to browser");
    parts.join("  ")
}

/// The pad wording of the same footer. Ratings stay keyboard-only
/// (digits), so the pad line honestly offers only what the pad can
/// do here.
fn results_footer_pad() -> String {
    "SOUTH back to browser".to_owned()
}

/// Mean drift below which the run counts as on time (ms) — inside
/// it an EARLY/LATE claim would be noise.
const ON_TIME_MS: f64 = 3.0;

/// Mean drift at which the screen suggests recalibrating (ms): half
/// the perfect window — a player this far off is donating half
/// their margin to a constant the calibration screen can remove.
const RECALIBRATE_MS: f64 = 15.0;

/// The TIMING row's value for a run's mean signed offset.
fn drift_value(mean_ms: f64) -> String {
    if mean_ms.abs() < ON_TIME_MS {
        "on time".to_owned()
    } else if mean_ms < 0.0 {
        format!("{:.0} ms early", mean_ms.abs())
    } else {
        format!("+{mean_ms:.0} ms late")
    }
}

/// Whether the drift warrants the recalibration hint.
fn needs_recalibration(mean_ms: f64) -> bool {
    mean_ms.abs() >= RECALIBRATE_MS
}

/// What the status line says for the feedback given so far.
fn feedback_status(
    fun: Option<u8>,
    versus: Option<&str>,
    taste: Option<&str>,
    comments: usize,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(rating) = fun {
        parts.push(format!("fun {rating}/5"));
    }
    // The blind test's own wording, reveal included; it replaces the
    // versus phrase rather than sitting beside it, because they are
    // two ways of saying the same verdict.
    if let Some(verdict) = taste {
        parts.push(verdict.to_owned());
    } else if let Some(verdict) = versus {
        parts.push(format!("felt {verdict} than the previous version"));
    }
    if comments > 0 {
        parts.push(if comments == 1 {
            "a comment".to_owned()
        } else {
            format!("{comments} comments")
        });
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
    /// The colour the letter settles into.
    tone: Color,
}

/// The score ticks up from zero.
#[derive(Component)]
struct ScoreCountUp {
    target: u64,
    age: f32,
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn spawn_results(
    mut commands: Commands,
    results: Option<Res<LastResults>>,
    mut scores: ResMut<ScoreBoard>,
    last_run: Res<crate::telemetry::LastRun>,
    song: Option<Res<crate::boot::LoadedSong>>,
    mut given: ResMut<FeedbackGiven>,
    mut field: ResMut<CommentField>,
    mut offer: ResMut<FeedbackOffer>,
    taste: Option<Res<crate::taste::TasteTest>>,
    font: Res<UiFont>,
) {
    let Some(results) = results else {
        return;
    };
    // Feedback (A5) is offered when this run left a session open in
    // the store: the fun rating is recorded against it, and the
    // versus verdict additionally needs a parent version to compare
    // against (the played chart's provenance carries it).
    // Autopilot sessions stay ratable on purpose: the session marks
    // them and the analytics exclude them by default, and the rating
    // drill needs the REAL path (a gate here would force the drill
    // to test a bypass instead).
    let can_rate = last_run.open();
    let can_versus = can_rate && song.is_some_and(|s| s.chart.provenance.is_some());
    // A blind test asks its own question instead: which of the two
    // passages just heard was better. It is offered only once BOTH
    // sides have actually played — an abandoned test has nothing to
    // compare, and asking anyway would collect an answer about one.
    let can_taste = can_rate && taste.is_some_and(|test| !test.has_next());
    *given = FeedbackGiven::default();
    *field = CommentField::default();
    *offer = FeedbackOffer {
        rate: can_rate,
        versus: can_versus,
        taste: can_taste,
    };

    // Record solo runs only — multiplayer scoreboards would mix
    // devices and players into one book. Tap mode records normally
    // since it became the default way to play; `tap_mode` on the
    // results stays available for display.
    let solo = results.players.len() == 1;
    let mut new_record = false;
    // Practice runs (slowed at any point) never touch the book — a
    // best set at 50 % speed would be a lie the scoreboard repeats.
    // A failed run is a partial run; its score is not a best.
    if solo
        && !results.practice
        && !results.failed
        && let Some(player) = results.players.first()
    {
        let perf = &player.performance;
        new_record = scores.record(
            results.song_id.as_deref(),
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
            // The singer's own panel, when somebody sang. It is a
            // panel of its own rather than rows in the guitarist's:
            // they are two performances, and one run can hold both.
            for vocal in &results.vocalists {
                spawn_vocal(parent, vocal, &font);
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
            let chips = results_chips(FeedbackOffer {
                rate: can_rate,
                versus: can_versus,
                taste: can_taste,
            });
            if !chips.is_empty() {
                ui_kit::action_bar(parent, &font, &chips);
            }
            crate::prompts::device_footer(
                parent,
                &font,
                &results_footer(can_rate, can_versus, can_taste),
                &results_footer_pad(),
            );
            ui_kit::back_button(parent, &font, "SONG SELECT");
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
    let grade = if results.failed {
        "F"
    } else {
        grade_for(accuracy, counts.miss)
    };
    // A failed run's badge is red, not brand yellow: the letter says
    // what happened, the colour must not say the opposite.
    let badge = grade_tone(results.failed);

    // The song is the subject of this screen, so it gets the heading
    // rather than a dim line under the grade.
    let mut subtitle = format!("{} - {}", font.safe(&results.artist), results.difficulty);
    if results.failed {
        subtitle.push_str(" - FAILED (no record)");
    }
    if results.practice {
        subtitle.push_str(" - practice (no record)");
    }
    ui_kit::header(
        parent,
        font,
        &font.safe(&results.title).to_uppercase(),
        &subtitle,
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
                    BackgroundColor(badge.with_alpha(0.10)),
                    BorderColor::all(badge),
                ))
                .with_child((
                    GradeSlam {
                        age: 0.0,
                        tone: badge,
                    },
                    Text::new(grade),
                    font.text(20.0),
                    TextColor(badge.with_alpha(0.0)),
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
        // The run's timing drift (optimization plan P2): the one
        // number that turns "felt off" into an action. Absent when
        // nothing was hit — there is no drift to report.
        if let Some(mean_ms) = perf.mean_offset_ms() {
            panel.spawn(ui_kit::row()).with_children(|row| {
                row.spawn((
                    Text::new("TIMING"),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT_DIM),
                    ui_kit::label_node(),
                ));
                row.spawn((
                    Text::new(drift_value(mean_ms)),
                    font.text(ui_kit::ROW),
                    TextColor(if needs_recalibration(mean_ms) {
                        palette::GOOD
                    } else {
                        palette::TEXT
                    }),
                    ui_kit::value_node(),
                ));
            });
            if needs_recalibration(mean_ms) {
                panel.spawn((
                    Text::new("consistently off — recalibrate in settings"),
                    font.text(ui_kit::SMALL),
                    TextColor(palette::TEXT_DIM),
                ));
            }
        }
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

/// A pitch as a note name, e.g. `A4`, `C#5`.
///
/// Sharps rather than flats throughout: one spelling, consistently,
/// because a range printed `A#3-Db5` reads as two different
/// conventions arguing. Pure — tested.
#[must_use]
pub fn note_name(midi: f32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    if !midi.is_finite() {
        return "-".to_owned();
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a MIDI note number, 0..127 after clamping"
    )]
    let rounded = midi.round().clamp(0.0, 127.0) as i32;
    let octave = rounded / 12 - 1;
    let name = NAMES[(rounded % 12) as usize];
    format!("{name}{octave}")
}

/// The lines a vocal result puts on the screen, in order.
///
/// Pure, so what the screen says can be tested without building one.
/// A measurement nobody produced is left OUT rather than printed as
/// zero: a run with no pitch in it (all rap, or a microphone that
/// never worked) must not claim 0 % accuracy, which reads as "you
/// were terrible" rather than "there is nothing to report".
#[must_use]
pub fn vocal_rows(result: &crate::gameplay::VocalResult) -> Vec<(&'static str, String)> {
    let perf = &result.performance;
    let mut rows = Vec::with_capacity(8);
    let percent = |value: f32| format!("{:.0}%", value * 100.0);
    if let Some(pitch) = perf.pitch_accuracy() {
        rows.push(("PITCH", percent(pitch)));
    }
    if let Some(cents) = perf.mean_abs_cents() {
        rows.push(("AVERAGE OFF", format!("{cents:.0} cents")));
    }
    if let Some(timing) = perf.timing_accuracy() {
        rows.push(("TIMING", percent(timing)));
    }
    if let Some(hold) = perf.coverage() {
        rows.push(("HOLD", percent(hold)));
    }
    if let Some(steady) = perf.stability() {
        rows.push(("STEADY", percent(steady)));
    }
    rows.push(("NOTES", format!("{} of {}", perf.notes_hit, perf.notes)));
    rows.push((
        "PHRASES",
        format!("{} perfect of {}", perf.perfect_phrases, perf.phrases),
    ));
    rows.push(("BEST RUN", format!("{} phrases", perf.best_streak)));
    if let Some(range) = perf.range {
        rows.push((
            "YOUR RANGE",
            format!(
                "{} - {}  ({:.0} semitones)",
                note_name(range.low_midi),
                note_name(range.high_midi),
                range.semitones()
            ),
        ));
    }
    rows
}

/// The singer's panel, under the player's.
fn spawn_vocal(
    parent: &mut ChildSpawnerCommands,
    result: &crate::gameplay::VocalResult,
    font: &UiFont,
) {
    let perf = &result.performance;
    let grade = perf.grade(&result.config);
    let tone = match grade {
        beatbyte_core::vocal_session::VocalGrade::Perfect => palette::PERFECT,
        beatbyte_core::vocal_session::VocalGrade::Great => palette::GREAT,
        beatbyte_core::vocal_session::VocalGrade::Good => palette::GOOD,
        _ => palette::MISS,
    };
    parent.spawn(ui_kit::panel()).with_children(|panel| {
        panel.spawn(ui_kit::row()).with_children(|row| {
            row.spawn((
                Text::new("VOCALS"),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT_DIM),
                ui_kit::label_node(),
            ));
            row.spawn((
                Text::new(format!("{}   {}", grade.label(), perf.score)),
                font.text(ui_kit::ROW),
                TextColor(tone),
                ui_kit::value_node(),
            ));
        });
        if result.assisted {
            // Said plainly rather than hidden in a footnote: this
            // score is not comparable with one sung against the
            // backing alone, and nothing about the numbers shows it.
            panel.spawn((
                Text::new("assisted - the original singer was audible"),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT_DIM),
            ));
        }
        for (label, value) in vocal_rows(result) {
            panel.spawn(ui_kit::row()).with_children(|row| {
                row.spawn((
                    Text::new(label),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT_DIM),
                    ui_kit::label_node(),
                ));
                row.spawn((
                    Text::new(value),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT),
                    ui_kit::value_node(),
                ));
            });
        }
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

/// The badge colour: brand yellow for a finished run, the miss red
/// for a failed one. Pure — tested.
fn grade_tone(failed: bool) -> Color {
    if failed {
        palette::MISS
    } else {
        palette::BRAND
    }
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
        color.0 = slam.tone.with_alpha(t.min(1.0));
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

/// The comment field: `C` opens it, typing fills it, ENTER records it
/// in the session log, ESC throws it away.
///
/// ONE system owns the field and it runs before [`results_input`] —
/// the browser paid for that lesson: with the opening in an earlier
/// system, the very keystroke that opened the field was still unread
/// and appeared inside it. The drain on the closed branch is the
/// other half: a reader that returns without reading leaves its
/// cursor a frame behind.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn results_comment(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    store: Res<crate::telemetry::TelemetryStore>,
    last_run: Res<crate::telemetry::LastRun>,
    clicks: Res<ActionBarClicks>,
    mut field: ResMut<CommentField>,
    mut given: ResMut<FeedbackGiven>,
    mut status: Query<&mut Text, With<FeedbackStatus>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    use bevy::input::keyboard::Key;

    let can_rate = last_run.open();
    if !field.open {
        let opening = can_rate
            && (keys.just_pressed(KeyCode::KeyC) || ui_kit::chip_hit(&clicks.0, chip::COMMENT));
        for _ in typed.read() {}
        if opening {
            field.open = true;
            field.text.clear();
            sounds.write(crate::sfx::UiSound::Confirm);
            say(&mut status, field_line(""));
        }
        return;
    }
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            Key::Character(text) => {
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                field.text.push_str(&clean);
            }
            Key::Space => field.text.push(' '),
            // Here rather than on `just_pressed`, so the OS key
            // repeat erases while held like every text field.
            Key::Backspace => {
                field.text.pop();
            }
            _ => {}
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        field.open = false;
        field.text.clear();
        sounds.write(crate::sfx::UiSound::Back);
        say(
            &mut status,
            feedback_status(
                given.fun,
                given.versus,
                given.taste.as_deref(),
                given.comments,
            ),
        );
        return;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        debug_assert_eq!(enter_means(true), EnterMeans::CommitComment);
        let line = beatbyte_core::telemetry::comment_line(&field.text);
        if let Some(line) = line.filter(|_| last_run.open()) {
            crate::telemetry::append_feedback(&store, &last_run, &line);
            given.comments += 1;
        }
        field.open = false;
        field.text.clear();
        sounds.write(crate::sfx::UiSound::Confirm);
        say(
            &mut status,
            feedback_status(
                given.fun,
                given.versus,
                given.taste.as_deref(),
                given.comments,
            ),
        );
        return;
    }
    let echo = field.text.clone();
    say(&mut status, field_line(&echo));
}

/// Put one line on the feedback status row.
fn say(status: &mut Query<&mut Text, With<FeedbackStatus>>, line: String) {
    if let Ok(mut text) = status.single_mut() {
        text.0 = line;
    }
}

/// The status line while the field is open.
fn field_line(typed: &str) -> String {
    format!("comment: {typed}_   ENTER records  ESC cancels")
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn results_input(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    store: Res<crate::telemetry::TelemetryStore>,
    last_run: Res<crate::telemetry::LastRun>,
    song: Option<Res<crate::boot::LoadedSong>>,
    taste: Option<Res<crate::taste::TasteTest>>,
    clicks: Res<ActionBarClicks>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut status: Query<&mut Text, With<FeedbackStatus>>,
    mut given: ResMut<FeedbackGiven>,
    field: Res<CommentField>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut next_state: ResMut<NextState<AppState>>,
) {
    // While the field is taking keys it owns them: a digit is text,
    // and ENTER commits the sentence rather than leaving the screen.
    if field.open {
        debug_assert_eq!(enter_means(true), EnterMeans::CommitComment);
        return;
    }
    // Feedback first, so a digit or arrow never doubles as an exit.
    if last_run.open() {
        let mut changed = false;
        let rating = keys
            .get_just_pressed()
            .find_map(|k| fun_rating_for(*k))
            .or_else(|| chip_rating(&clicks.0));
        if let Some(rating) = rating {
            crate::telemetry::append_feedback(
                &store,
                &last_run,
                &beatbyte_core::telemetry::NoteLine::Fun { fun: rating },
            );
            given.fun = Some(rating);
            changed = true;
        }
        // The blind test owns the arrows when it is the question on
        // screen — one verdict, not two competing ones.
        let blind = taste.as_ref().filter(|test| !test.has_next());
        if let Some(test) = blind {
            let choice = if keys.just_pressed(KeyCode::ArrowLeft)
                || ui_kit::chip_hit(&clicks.0, chip::LEFT)
            {
                Some(crate::taste::Choice::First)
            } else if keys.just_pressed(KeyCode::ArrowRight)
                || ui_kit::chip_hit(&clicks.0, chip::RIGHT)
            {
                Some(crate::taste::Choice::Second)
            } else if keys.just_pressed(KeyCode::ArrowDown)
                || ui_kit::chip_hit(&clicks.0, chip::SAME)
            {
                Some(crate::taste::Choice::Same)
            } else {
                None
            };
            if let Some(choice) = choice {
                let hashes = [
                    test.versions[0].hash.as_str(),
                    test.versions[1].hash.as_str(),
                ];
                if let Some((verdict, parent)) =
                    crate::taste::versus_line(choice, test.order, hashes)
                {
                    crate::telemetry::append_feedback(
                        &store,
                        &last_run,
                        &beatbyte_core::telemetry::NoteLine::Versus {
                            versus: verdict,
                            parent,
                        },
                    );
                }
                let said = match choice {
                    crate::taste::Choice::First => "the first one was better",
                    crate::taste::Choice::Second => "the second one was better",
                    crate::taste::Choice::Same => "no difference between them",
                };
                // The reveal comes only now: knowing which was which
                // before answering would decide the answer.
                let names = [
                    test.versions[0].name.as_str(),
                    test.versions[1].name.as_str(),
                ];
                given.taste = Some(format!(
                    "{said} · {}",
                    crate::taste::reveal(test.order, names)
                ));
                changed = true;
            }
        }
        let parent_hash = song
            .as_ref()
            .and_then(|s| s.chart.provenance.as_ref())
            .map(|p| p.parent_hash.clone());
        if let Some(parent) = parent_hash.filter(|_| blind.is_none()) {
            let verdict = if keys.just_pressed(KeyCode::ArrowLeft)
                || ui_kit::chip_hit(&clicks.0, chip::LEFT)
            {
                Some("worse")
            } else if keys.just_pressed(KeyCode::ArrowRight)
                || ui_kit::chip_hit(&clicks.0, chip::RIGHT)
            {
                Some("better")
            } else {
                None
            };
            if let Some(verdict) = verdict {
                crate::telemetry::append_feedback(
                    &store,
                    &last_run,
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
                text.0 = feedback_status(
                    given.fun,
                    given.versus,
                    given.taste.as_deref(),
                    given.comments,
                );
            }
            sounds.write(crate::sfx::UiSound::Navigate);
        }
    }
    // Confirm, back, the visible button or right-click leave — a
    // left-click on empty space must not (buttons own left-click).
    let nav = crate::controls::MenuNav::read(&map, &keys, pads.iter());
    if ui_kit::wants_leave(
        nav.confirm || nav.back,
        ui_kit::back_pressed(&mut back),
        mouse.just_pressed(MouseButton::Right),
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        // Back to where the song was picked: the browser, with its
        // cursor, sort and search intact (they live in resources) —
        // the flow is browse, play, land on the NEXT choice, not on
        // the main menu's start line every time (user request).
        next_state.set(AppState::SongSelect);
    }
}

fn despawn_results(mut commands: Commands, entities: Query<Entity, With<ResultsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    // The blind test ends with its verdict. Left behind, it would
    // crop the NEXT song to this one's window.
    commands.remove_resource::<crate::taste::TasteTest>();
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_pitch_reads_as_the_note_a_singer_would_name() {
        assert_eq!(note_name(60.0), "C4", "middle C");
        assert_eq!(note_name(69.0), "A4", "concert A");
        assert_eq!(note_name(21.0), "A0", "the bottom of a piano");
        assert_eq!(note_name(61.0), "C#4");
        // Fractional pitches round to the nearest name.
        assert_eq!(note_name(60.4), "C4");
        assert_eq!(note_name(60.6), "C#4");
        // Nonsense does not panic or print an octave from nowhere.
        assert_eq!(note_name(f32::NAN), "-");
        assert_eq!(note_name(-500.0), "C-1");
        assert_eq!(note_name(9999.0), "G9");
    }

    #[test]
    fn the_vocal_panel_leaves_out_what_nobody_produced() {
        use beatbyte_core::vocal_session::{VocalPerformance, VocalScoreConfig};
        let config = VocalScoreConfig::default();
        // A result with nothing measured: no pitch line claiming 0 %,
        // which would read as "you were terrible" rather than as
        // "there is nothing to report".
        let bare = crate::gameplay::VocalResult {
            performance: VocalPerformance::default(),
            config,
            assisted: false,
        };
        let rows = vocal_rows(&bare);
        let labels: Vec<&str> = rows.iter().map(|(label, _)| *label).collect();
        assert!(!labels.contains(&"PITCH"), "{labels:?}");
        assert!(!labels.contains(&"AVERAGE OFF"), "{labels:?}");
        assert!(!labels.contains(&"YOUR RANGE"), "{labels:?}");
        // The counts are always there: zero of zero is a fact.
        assert!(labels.contains(&"NOTES"), "{labels:?}");
        assert!(labels.contains(&"PHRASES"), "{labels:?}");
    }

    #[test]
    fn a_real_vocal_run_reports_every_number_the_plan_asks_for() {
        use beatbyte_core::Difficulty;
        use beatbyte_core::vocal::{VocalKind, VocalNote, VocalPart, VocalPhrase, VocalRole};
        use beatbyte_core::vocal_session::{VocalInputFrame, VocalScoreConfig, VocalSession};

        let config = VocalScoreConfig::for_difficulty(Difficulty::Medium);
        let part = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![VocalPhrase {
                start_s: 0.0,
                end_s: 2.0,
                confidence: 1.0,
                tokens: Vec::new(),
                notes: vec![VocalNote {
                    start_s: 0.0,
                    end_s: 2.0,
                    kind: VocalKind::Pitched,
                    target_midi: Some(64.0),
                    contour: Vec::new(),
                    confidence: 1.0,
                    token_range: None,
                }],
            }],
        };
        let mut session = VocalSession::new(part, config);
        let mut events = Vec::new();
        let mut t = 0.0f64;
        while t < 2.0 {
            session.feed(
                VocalInputFrame {
                    song_time_s: t,
                    midi: Some(64.0),
                    confidence: 0.9,
                    rms_dbfs: -18.0,
                    voiced: true,
                    clipped: false,
                },
                &mut events,
            );
            t += 0.016;
        }
        session.finish(&mut events);
        let result = crate::gameplay::VocalResult {
            performance: session.performance().clone(),
            config,
            assisted: false,
        };
        let rows = vocal_rows(&result);
        let labels: Vec<&str> = rows.iter().map(|(label, _)| *label).collect();
        for wanted in [
            "PITCH",
            "AVERAGE OFF",
            "TIMING",
            "HOLD",
            "STEADY",
            "NOTES",
            "PHRASES",
            "BEST RUN",
            "YOUR RANGE",
        ] {
            assert!(labels.contains(&wanted), "{wanted} missing from {labels:?}");
        }
        let range = rows
            .iter()
            .find(|(label, _)| *label == "YOUR RANGE")
            .map(|(_, value)| value.clone())
            .expect("a range");
        assert!(range.starts_with("E4 - E4"), "{range}");
        let notes = rows
            .iter()
            .find(|(label, _)| *label == "NOTES")
            .map(|(_, value)| value.clone())
            .expect("the counts");
        assert_eq!(notes, "1 of 1");
    }
    use super::{
        EnterMeans, enter_means, feedback_status, field_line, fun_rating_for, grade_for, note_name,
        results_footer, vocal_rows,
    };
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
        assert_eq!(results_footer(false, false, false), "ENTER back to browser");
        let rate_only = results_footer(true, false, false);
        assert!(
            rate_only.contains("chips rate"),
            "rate offer names the chips: {rate_only}"
        );
        assert!(
            !results_footer(false, false, false).contains("comment"),
            "no log, no offer — a hint that does nothing is a lie"
        );
        assert!(
            !rate_only.contains("Worse"),
            "no versus hint without a parent"
        );
        let full = results_footer(true, true, false);
        assert!(full.contains("Worse") && full.contains("Better"), "{full}");
        assert!(full.ends_with("ENTER back to browser"));
        // A blind test asks about the two runs just heard — and asks
        // it INSTEAD of the versus question, never beside it: two
        // verdicts on the same arrows would record whichever the code
        // happened to reach first.
        let blind = results_footer(true, true, true);
        assert!(blind.contains("1st") && blind.contains("2nd"), "{blind}");
        assert!(blind.contains("Same"), "{blind}");
        assert!(
            !blind.contains("Worse"),
            "the blind question replaces the versus one: {blind}"
        );
    }

    #[test]
    fn drift_reads_as_a_side_and_flags_only_real_drift() {
        use super::{drift_value, needs_recalibration};
        // Negative mean = the hits came before the notes = early.
        assert_eq!(drift_value(-32.4), "32 ms early");
        assert_eq!(drift_value(18.0), "+18 ms late");
        assert_eq!(drift_value(1.5), "on time");
        assert_eq!(drift_value(-2.9), "on time");
        // The hint fires at half the perfect window, either side —
        // and never inside it (a nag on 5 ms would teach players to
        // ignore it).
        assert!(needs_recalibration(15.0));
        assert!(needs_recalibration(-15.0));
        assert!(!needs_recalibration(14.9));
        assert!(!needs_recalibration(-5.0));
    }

    #[test]
    fn the_status_line_reads_back_what_was_recorded() {
        assert_eq!(feedback_status(None, None, None, 0), "");
        assert_eq!(feedback_status(Some(4), None, None, 0), "recorded: fun 4/5");
        assert_eq!(
            feedback_status(Some(4), Some("better"), None, 0),
            "recorded: fun 4/5 · felt better than the previous version"
        );
        assert_eq!(
            feedback_status(None, Some("worse"), None, 0),
            "recorded: felt worse than the previous version"
        );
        // A sentence ADDS to a rating; a second one adds again,
        // because unlike a rating it does not replace the first.
        assert_eq!(
            feedback_status(Some(3), None, None, 1),
            "recorded: fun 3/5 · a comment"
        );
        assert_eq!(feedback_status(None, None, None, 2), "recorded: 2 comments");
        // The blind verdict carries its own wording AND the reveal,
        // and it stands in for the versus phrase rather than beside
        // it — one verdict must not read as two.
        let blind = feedback_status(
            Some(5),
            Some("better"),
            Some("the second one was better · you heard chart.json first, then chart.v2.json"),
            0,
        );
        assert_eq!(
            blind,
            "recorded: fun 5/5 · the second one was better · you heard chart.json first, then \
             chart.v2.json"
        );
        assert!(!blind.contains("previous version"));
    }

    #[test]
    fn enter_belongs_to_the_field_while_it_is_open() {
        // The browser's rule, one screen on: finishing a sentence must
        // not also leave the screen and throw the sentence away.
        assert_eq!(enter_means(true), EnterMeans::CommitComment);
        assert_eq!(enter_means(false), EnterMeans::Leave);
    }

    #[test]
    fn the_field_shows_what_was_typed_and_how_to_end_it() {
        let line = field_line("the chorus drags");
        assert!(line.contains("the chorus drags"), "{line}");
        assert!(
            line.contains("ENTER"),
            "a field must say how to commit: {line}"
        );
        assert!(line.contains("ESC"), "and how to abandon: {line}");
        assert!(
            field_line("").contains("comment:"),
            "an empty field still names itself"
        );
    }

    #[test]
    fn a_failed_badge_is_red_and_a_finished_one_is_not() {
        assert_eq!(super::grade_tone(true), crate::palette::MISS);
        assert_eq!(super::grade_tone(false), crate::palette::BRAND);
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

    #[test]
    fn rating_chips_map_one_to_one_onto_the_star_keys() {
        // Click equals 1–5: the ActionBar ids are the ratings.
        assert_eq!(super::chip_rating(&[super::chip::RATE_3]), Some(3));
        assert_eq!(super::chip_rating(&[super::chip::RATE_1]), Some(1));
        assert_eq!(super::chip_rating(&[super::chip::RATE_5]), Some(5));
        assert_eq!(super::chip_rating(&[super::chip::COMMENT]), None);
        assert_eq!(super::chip_rating(&[]), None);
    }
}
