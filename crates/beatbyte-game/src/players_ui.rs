//! The roster screen: who is playing.
//!
//! One list, the house list idiom ([`crate::ui_kit`]). What it adds
//! over a plain list is a text field, and with it the rule this game
//! already learned once in the song browser: **while a field is
//! taking keys, a printable key is TEXT.** Typing "Sam" must not
//! leave the screen because "S" also opens statistics.

use beatbyte_core::player::{NameError, PlayerId};
use beatbyte_core::stats::{self, Filter};
use bevy::prelude::*;
use bevy::ui::Val::Px as px;

use crate::controls::{InputMap, MenuNav};
use crate::palette;
use crate::players::{Players, adopt_orphan_runs, now_ms, save_roster};
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Everything this screen spawns.
#[derive(Component)]
struct PlayersScreen;

/// A row standing for one player.
#[derive(Component)]
struct PlayerRow(usize);

/// The line under the list that reports what just happened.
#[derive(Component)]
struct RosterStatus;

/// The text field's line.
#[derive(Component)]
struct FieldLine;

/// Where the cursor is, and what is being typed.
#[derive(Resource, Debug, Clone, Default)]
pub struct RosterCursor {
    /// Selected row.
    pub row: usize,
    /// The open text field, if one is open.
    pub field: Option<Field>,
    /// The last thing the screen has to say.
    pub status: String,
}

/// An open text field, and what it will do with what is typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// What has been typed so far.
    pub text: String,
    /// Whether this renames somebody rather than adding them.
    pub renaming: Option<PlayerId>,
}

/// What a key means on this screen.
///
/// Lifted out of the input system so the rule can be tested without
/// an app: the browser's version of this rule was a bug twice before
/// it was a function. Pure — tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Add the character to the open field.
    Type(char),
    /// Commit what is in the field.
    Commit,
    /// Close the field, forgetting what was typed.
    Cancel,
    /// A gesture the screen acts on (only when no field is open).
    Gesture(Gesture),
    /// Nothing.
    Ignore,
}

/// A screen action a letter can stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Open the field to add a player.
    New,
    /// Open the field to rename the selected player.
    Rename,
    /// Open the selected player's statistics.
    Stats,
    /// Open the selected player's achievements.
    Achievements,
}

/// What a key press means, given whether a field is open.
///
/// The whole rule in one place: with a field open, every printable
/// key is text and only Enter and Escape are commands. Pure —
/// tested.
#[must_use]
pub fn press_means(key: KeyCode, typed: Option<char>, field_open: bool) -> Press {
    match key {
        KeyCode::Enter | KeyCode::NumpadEnter if field_open => Press::Commit,
        KeyCode::Escape if field_open => Press::Cancel,
        _ => {
            if field_open {
                return typed
                    .filter(|c| !c.is_control())
                    .map_or(Press::Ignore, Press::Type);
            }
            match key {
                KeyCode::KeyN => Press::Gesture(Gesture::New),
                KeyCode::KeyR => Press::Gesture(Gesture::Rename),
                KeyCode::KeyS => Press::Gesture(Gesture::Stats),
                KeyCode::KeyA => Press::Gesture(Gesture::Achievements),
                _ => Press::Ignore,
            }
        }
    }
}

/// The footer hint, which changes with what is on screen. Pure —
/// tested.
#[must_use]
pub fn footer_hint(field_open: bool, has_players: bool) -> String {
    if field_open {
        return "TYPE A NAME   ENTER CONFIRM   ESC CANCEL".to_owned();
    }
    if has_players {
        "UP/DOWN SELECT   ENTER PLAY AS   chips above   ESC BACK".to_owned()
    } else {
        "NEW chip or N   ESC BACK".to_owned()
    }
}

/// ActionBar chip ids for this screen.
mod chip {
    pub const NEW: u8 = 0;
    pub const RENAME: u8 = 1;
    pub const STATS: u8 = 2;
    pub const AWARDS: u8 = 3;
}

/// Pressed ActionBar chip ids for this frame.
#[derive(Resource, Default)]
struct ActionBarClicks(Vec<u8>);

fn roster_chips(has_players: bool) -> [ui_kit::ChipSpec; 4] {
    [
        ui_kit::ChipSpec {
            id: chip::NEW,
            label: "New",
            enabled: true,
        },
        ui_kit::ChipSpec {
            id: chip::RENAME,
            label: "Rename",
            enabled: has_players,
        },
        ui_kit::ChipSpec {
            id: chip::STATS,
            label: "Stats",
            enabled: has_players,
        },
        ui_kit::ChipSpec {
            id: chip::AWARDS,
            label: "Awards",
            enabled: has_players,
        },
    ]
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

/// Map a chip press to the same [`Gesture`] the letter keys use.
fn chip_gesture(clicks: &[u8]) -> Option<Gesture> {
    if ui_kit::chip_hit(clicks, chip::NEW) {
        Some(Gesture::New)
    } else if ui_kit::chip_hit(clicks, chip::RENAME) {
        Some(Gesture::Rename)
    } else if ui_kit::chip_hit(clicks, chip::STATS) {
        Some(Gesture::Stats)
    } else if ui_kit::chip_hit(clicks, chip::AWARDS) {
        Some(Gesture::Achievements)
    } else {
        None
    }
}

/// The line that describes a player in the list. Pure — tested.
#[must_use]
pub fn player_line(runs: usize, songs: usize, last_ms: Option<u64>) -> String {
    if runs == 0 {
        return "NO RUNS YET".to_owned();
    }
    let songs_word = if songs == 1 { "SONG" } else { "SONGS" };
    let runs_word = if runs == 1 { "RUN" } else { "RUNS" };
    last_ms.map_or_else(
        || format!("{runs} {runs_word}   {songs} {songs_word}"),
        |ms| {
            let day = beatbyte_core::history::iso_utc(ms);
            format!(
                "{runs} {runs_word}   {songs} {songs_word}   LAST {}",
                &day[..10.min(day.len())]
            )
        },
    )
}

/// Screens and systems of the roster.
pub struct PlayersUiPlugin;

impl Plugin for PlayersUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RosterCursor>()
            .init_resource::<ActionBarClicks>()
            .add_systems(
                OnEnter(AppState::Players),
                spawn_roster.after(crate::history::HistoryReloaded),
            )
            .add_systems(
                Update,
                (
                    paint_action_bar.before(roster_keys),
                    (roster_keys, roster_nav, refresh_rows).chain(),
                )
                    .run_if(in_state(AppState::Players)),
            )
            .add_systems(OnExit(AppState::Players), despawn_roster);
    }
}

fn spawn_roster(
    mut commands: Commands,
    font: Res<UiFont>,
    players: Res<Players>,
    history: Res<crate::history::PlayHistory>,
    mut cursor: ResMut<RosterCursor>,
) {
    cursor.row = cursor.row.min(players.0.len().saturating_sub(1));
    cursor.field = None;
    commands
        .spawn((ui_kit::screen_root(), PlayersScreen))
        .with_children(|root| {
            ui_kit::header(
                root,
                &font,
                "PLAYERS",
                "WHO IS AT THE GUITAR - EVERY RUN IS FILED UNDER THEM",
            );
            ui_kit::action_bar(root, &font, &roster_chips(!players.0.is_empty()));
            root.spawn(ui_kit::panel()).with_children(|panel| {
                if players.0.is_empty() {
                    crate::plot::empty_note(
                        panel,
                        &font,
                        "NOBODY YET - PRESS N TO ADD THE FIRST PLAYER",
                    );
                } else {
                    for (index, player) in players.0.players.iter().enumerate() {
                        let runs = stats::runs_of(&history.0, player.id, Filter::default());
                        let summary = stats::summarize(&runs);
                        spawn_row(
                            panel,
                            &font,
                            index,
                            &player.name,
                            player.colour,
                            players.0.selected == Some(player.id),
                            &player_line(summary.runs, summary.songs, summary.last_played_ms),
                        );
                    }
                }
                panel.spawn((
                    Node {
                        margin: UiRect::top(px(10.0)),
                        ..default()
                    },
                    Text::new(String::new()),
                    font.text(ui_kit::SMALL),
                    TextColor(palette::BRAND),
                    FieldLine,
                ));
                panel.spawn((
                    Text::new(cursor.status.clone()),
                    font.text(ui_kit::SMALL),
                    TextColor(ui_kit::dimmed_subtitle()),
                    RosterStatus,
                ));
            });
            ui_kit::back_button(root, &font, "MAIN MENU");
            crate::prompts::device_footer(
                root,
                &font,
                &footer_hint(false, !players.0.is_empty()),
                "D-PAD select  SOUTH play as  EAST back",
            );
        });
}

fn spawn_row(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    index: usize,
    name: &str,
    colour: usize,
    playing: bool,
    line: &str,
) {
    parent
        .spawn((ui_kit::row(), PlayerRow(index), Button))
        .with_children(|row| {
            // The accent the player wears on the highway, so the two
            // places agree about who is who.
            row.spawn((
                Node {
                    width: px(4.0),
                    height: px(16.0),
                    margin: UiRect::right(px(10.0)),
                    ..default()
                },
                BackgroundColor(
                    crate::multiplayer::PLAYER_COLORS
                        [colour % crate::multiplayer::PLAYER_COLORS.len()],
                ),
            ));
            row.spawn((
                ui_kit::label_node(),
                Text::new(name.to_owned()),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
            ));
            row.spawn((
                Text::new(line.to_owned()),
                font.text(ui_kit::SMALL),
                TextColor(ui_kit::dimmed_subtitle()),
            ));
            if playing {
                row.spawn((
                    Node {
                        margin: UiRect::left(px(10.0)),
                        ..default()
                    },
                    Text::new("PLAYING".to_owned()),
                    font.text(ui_kit::SMALL),
                    TextColor(palette::BRAND),
                ));
            }
        });
}

/// Apply a roster [`Gesture`] (shared by letter keys and ActionBar chips).
fn apply_gesture(
    gesture: Gesture,
    cursor: &mut RosterCursor,
    players: &Players,
    next: &mut NextState<AppState>,
    chosen: &mut crate::stats_ui::StatsFor,
    awards: &mut crate::achievements_ui::AchievementsFor,
) {
    match gesture {
        Gesture::New => {
            cursor.field = Some(Field {
                text: String::new(),
                renaming: None,
            });
            cursor.status.clear();
        }
        Gesture::Rename => {
            if let Some(player) = players.0.players.get(cursor.row) {
                cursor.field = Some(Field {
                    text: player.name.clone(),
                    renaming: Some(player.id),
                });
                cursor.status.clear();
            }
        }
        Gesture::Stats => {
            if let Some(player) = players.0.players.get(cursor.row) {
                chosen.0 = Some(player.id);
                next.set(AppState::Stats);
            }
        }
        Gesture::Achievements => {
            if let Some(player) = players.0.players.get(cursor.row) {
                awards.0 = Some(player.id);
                next.set(AppState::Achievements);
            }
        }
    }
}

/// Typed keys: the field owns them whenever it is open. Chips share
/// the same [`Gesture`] paths when no field is open.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system params
fn roster_keys(
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    clicks: Res<ActionBarClicks>,
    mut cursor: ResMut<RosterCursor>,
    mut players: ResMut<Players>,
    mut history: ResMut<crate::history::PlayHistory>,
    mut next: ResMut<NextState<AppState>>,
    mut chosen: ResMut<crate::stats_ui::StatsFor>,
    mut awards: ResMut<crate::achievements_ui::AchievementsFor>,
) {
    // Chips first: they must not type into an open field, and a
    // chip click is the mouse door onto the same gestures as N/R/S/A.
    if cursor.field.is_none()
        && let Some(gesture) = chip_gesture(&clicks.0)
    {
        apply_gesture(
            gesture,
            &mut cursor,
            &players,
            &mut next,
            &mut chosen,
            &mut awards,
        );
    }
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        let character = match &event.logical_key {
            bevy::input::keyboard::Key::Character(text) => text.chars().next(),
            bevy::input::keyboard::Key::Space => Some(' '),
            _ => None,
        };
        let open = cursor.field.is_some();
        match press_means(event.key_code, character, open) {
            Press::Type(c) => {
                if let Some(field) = cursor.field.as_mut() {
                    field.text.push(c);
                }
            }
            Press::Commit => commit_field(&mut cursor, &mut players, &mut history),
            Press::Cancel => {
                cursor.field = None;
                cursor.status.clear();
            }
            Press::Gesture(gesture) => {
                apply_gesture(
                    gesture,
                    &mut cursor,
                    &players,
                    &mut next,
                    &mut chosen,
                    &mut awards,
                );
            }
            Press::Ignore => {}
        }
        // Backspace is not a character, and a field without one is a
        // field you cannot fix a typo in.
        if open
            && event.key_code == KeyCode::Backspace
            && let Some(field) = cursor.field.as_mut()
        {
            field.text.pop();
        }
    }
}

/// Add or rename, and report what happened either way.
fn commit_field(
    cursor: &mut RosterCursor,
    players: &mut Players,
    history: &mut crate::history::PlayHistory,
) {
    let Some(field) = cursor.field.clone() else {
        return;
    };
    let result = match field.renaming {
        Some(id) => players.0.rename(id, &field.text).map(|()| {
            // A rename is the newest version of this player; another
            // device's older one must not win it back (ADR-0021).
            players.0.stamp(id, now_ms());
            id
        }),
        None => {
            // The one-time adoption: a log older than the roster
            // belongs to the first person who says they play here.
            let adopting = players.0.adopts_orphans();
            players.0.add(&field.text, now_ms()).inspect(|&id| {
                if adopting {
                    let claimed = adopt_orphan_runs(id);
                    players.0.mark_adopted();
                    history.0 = crate::history::load();
                    cursor.status = if claimed > 0 {
                        format!("{claimed} EARLIER RUNS ARE NOW YOURS")
                    } else {
                        String::new()
                    };
                }
            })
        }
    };
    match result {
        Ok(id) => {
            if field.renaming.is_none() {
                players.0.select(id);
                cursor.row = players.0.len().saturating_sub(1);
            }
            save_roster(players);
            cursor.field = None;
        }
        Err(error) => cursor.status = NameError::message(error).to_owned(),
    }
}

/// Cursor movement, choosing a player, leaving the screen.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system params
fn roster_nav(
    map: Res<InputMap>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut moved: MessageReader<bevy::window::CursorMoved>,
    rows: Query<(&PlayerRow, &Interaction), Changed<Interaction>>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut cursor: ResMut<RosterCursor>,
    mut players: ResMut<Players>,
    mut selected: ResMut<crate::song_select::SelectedDifficulty>,
    mut next: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    // A field open means the arrows and Enter belong to it.
    if cursor.field.is_some() {
        return;
    }
    let count = players.0.len();
    if count > 0 {
        if nav.up {
            cursor.row = ui_kit::step_cursor(cursor.row, count, -1);
        }
        if nav.down {
            cursor.row = ui_kit::step_cursor(cursor.row, count, 1);
        }
        for event in wheel.read() {
            if event.y > 0.0 {
                cursor.row = ui_kit::step_cursor(cursor.row, count, -1);
            } else if event.y < 0.0 {
                cursor.row = ui_kit::step_cursor(cursor.row, count, 1);
            }
        }
        let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
        let mouse_moved = moved.read().next().is_some();
        if let Some(index) = ui_kit::hover_moves_cursor(&pointer, mouse_moved) {
            cursor.row = index;
        }
        if (nav.confirm || pointer.clicked)
            && let Some(player) = players.0.players.get(cursor.row)
        {
            let id = player.id;
            let name = player.name.clone();
            players.0.select(id);
            save_roster(&players);
            if let Some(pref) = players.0.current_preferred_difficulty() {
                selected.0 = pref;
            } else {
                selected.0 = beatbyte_core::Difficulty::Medium;
            }
            cursor.status = format!("{name} IS PLAYING");
            sounds.write(crate::sfx::UiSound::Confirm);
        }
    }
    if ui_kit::wants_leave(
        nav.back,
        ui_kit::back_pressed(&mut back),
        mouse.just_pressed(MouseButton::Right),
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        next.set(AppState::MainMenu);
    }
}

/// Keep the rows, the field, the status line and chip enables in step.
#[allow(clippy::type_complexity, clippy::too_many_arguments)] // Bevy queries
fn refresh_rows(
    cursor: Res<RosterCursor>,
    players: Res<Players>,
    settings: Res<crate::config::Settings>,
    mut rows: Query<(&PlayerRow, &mut BackgroundColor, &mut BorderColor)>,
    mut field: Query<&mut Text, (With<FieldLine>, Without<RosterStatus>)>,
    mut status: Query<&mut Text, (With<RosterStatus>, Without<FieldLine>)>,
    mut chips: Query<(&ui_kit::ActionChip, &mut ui_kit::ChipEnabled)>,
) {
    let has_players = !players.0.is_empty();
    ui_kit::set_chip_enabled(&mut chips, chip::RENAME, has_players);
    ui_kit::set_chip_enabled(&mut chips, chip::STATS, has_players);
    ui_kit::set_chip_enabled(&mut chips, chip::AWARDS, has_players);
    for (row, mut background, mut border) in &mut rows {
        let state = ui_kit::state_for(row.0 == cursor.row, false);
        let style = ui_kit::styled_row(state, settings.high_contrast);
        *background = BackgroundColor(style.background);
        *border = BorderColor::all(style.accent);
    }
    if let Ok(mut text) = field.single_mut() {
        **text = cursor.field.as_ref().map_or_else(String::new, |open| {
            let what = if open.renaming.is_some() {
                "RENAME"
            } else {
                "NEW PLAYER"
            };
            format!("{what}: {}_", open.text)
        });
    }
    if let Ok(mut text) = status.single_mut() {
        **text = cursor.status.clone();
    }
}

fn despawn_roster(mut commands: Commands, entities: Query<Entity, With<PlayersScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printable_key_is_text_while_the_field_is_open() {
        // "S" opens statistics and is also a letter in "Sam". The
        // browser learned this rule the hard way; the roster gets it
        // from the start.
        assert_eq!(
            press_means(KeyCode::KeyS, Some('S'), true),
            Press::Type('S')
        );
        assert_eq!(
            press_means(KeyCode::KeyS, Some('S'), false),
            Press::Gesture(Gesture::Stats)
        );
        // A space is a letter in a name, not a command.
        assert_eq!(
            press_means(KeyCode::Space, Some(' '), true),
            Press::Type(' ')
        );
        // Enter and Escape stay commands, or a field could not be
        // closed at all.
        assert_eq!(press_means(KeyCode::Enter, None, true), Press::Commit);
        assert_eq!(press_means(KeyCode::Escape, None, true), Press::Cancel);
        // With no field open, Enter belongs to the list, not here.
        assert_eq!(press_means(KeyCode::Enter, None, false), Press::Ignore);
    }

    #[test]
    fn control_characters_never_enter_a_name() {
        assert_eq!(press_means(KeyCode::Tab, Some('\t'), true), Press::Ignore);
        assert_eq!(press_means(KeyCode::KeyA, None, true), Press::Ignore);
    }

    #[test]
    fn the_footer_says_what_is_possible_right_now() {
        // An empty roster must not advertise renaming and statistics
        // for a player who does not exist — chips carry those actions.
        let empty = footer_hint(false, false);
        assert!(empty.contains("NEW"));
        assert!(
            !empty.contains("RENAME"),
            "offered on an empty roster: {empty}"
        );
        let full = footer_hint(false, true);
        assert!(full.contains("chips above"), "{full}");
        // With a field open, the arrows belong to the field.
        let typing = footer_hint(true, true);
        assert!(typing.contains("ESC CANCEL") && !typing.contains("UP/DOWN"));
    }

    #[test]
    fn chip_gestures_match_the_letter_keys() {
        assert_eq!(chip_gesture(&[chip::NEW]), Some(Gesture::New));
        assert_eq!(chip_gesture(&[chip::RENAME]), Some(Gesture::Rename));
        assert_eq!(chip_gesture(&[chip::STATS]), Some(Gesture::Stats));
        assert_eq!(chip_gesture(&[chip::AWARDS]), Some(Gesture::Achievements));
        assert_eq!(chip_gesture(&[]), None);
    }

    #[test]
    fn a_player_line_counts_in_the_right_plural_and_says_when_empty() {
        assert_eq!(player_line(0, 0, None), "NO RUNS YET");
        assert!(player_line(1, 1, None).starts_with("1 RUN   1 SONG"));
        assert!(player_line(4, 2, None).starts_with("4 RUNS   2 SONGS"));
        // The date is a day, not a millisecond stamp.
        let dated = player_line(3, 1, Some(1_756_000_000_000));
        assert!(dated.contains("LAST 2025-"), "{dated}");
        // A day, not a stamp: the ISO string carries a time of day
        // and its ":" separator, which is noise in a list row.
        assert!(!dated.contains(':'), "a time of day is noise here: {dated}");
        assert!(!dated.contains('Z'), "{dated}");
    }
}
