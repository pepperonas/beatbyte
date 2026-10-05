//! What the song browser looks like (the rebuild of 2026-10-05).
//!
//! The user, about the old screen: "das suchen und navigieren in der
//! songauswahl vor dem eigentlichen spielen finde ich katastrophal
//! dargestellt". It was a ten-column table under thirteen chips of
//! equal weight, a `[BG-01]` in front of nearly every title, and the
//! selected song's facts in a small footer line.
//!
//! Now: a search line that is always there, one sort line, a list with
//! one two-line row per SONG (its versions are a choice, not rows), and
//! beside it a panel that says everything about the selected song —
//! difficulty, versions, best, lyrics and chart state. The tools live
//! in one action menu ([`crate::song_select::ActionMenu`]).
//!
//! This module only draws. What the keys do, and what every tool does,
//! is `song_select`'s; the two meet in [`crate::song_select::BrowserView`].

use beatbyte_core::Difficulty;
use bevy::prelude::*;

use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::palette;
use crate::scores::ScoreBoard;
use crate::song_family::Family;
use crate::song_select::{BrowserCursor, BrowserView, SelectedDifficulty, SortMode};
use crate::ui::UiFont;
use crate::ui_kit;

/// Width of the song list.
pub const LIST_W: f32 = 700.0;
/// Width of the panel beside it.
pub const PANEL_W: f32 = ui_kit::PANEL_WIDE - LIST_W - GAP;
/// Between the list and the panel.
const GAP: f32 = 20.0;
/// The height both share — fixed, so the page never moves while the
/// list grows or shrinks under a search (the old screen jumped 170 px).
pub const BODY_H: f32 = 430.0;
/// One rating pip.
const PIP: f32 = 7.0;

/// The browser's root.
#[derive(Component)]
pub struct BrowserScreen;
/// The scrolling song list.
#[derive(Component)]
pub struct SongList;
/// A song row, by its position in the list.
#[derive(Component)]
pub struct SongRow(pub usize);
/// A row's title.
#[derive(Component)]
pub struct RowTitle(pub usize);
/// A row's artist line.
#[derive(Component)]
pub struct RowArtist(pub usize);
/// A row's best accuracy, right-aligned.
#[derive(Component)]
pub struct RowBest(pub usize);
/// The search line.
#[derive(Component)]
pub struct SearchLine;
/// The sort button (cycles; a second click on the same flips).
#[derive(Component)]
pub struct SortButton;
/// The button that opens the action menu.
#[derive(Component)]
pub struct ActionsButton;
/// The panel's title.
#[derive(Component)]
pub struct PanelTitle;
/// The panel's artist line.
#[derive(Component)]
pub struct PanelArtist;
/// The panel's facts line (genre, length, tempo).
#[derive(Component)]
pub struct PanelFacts;
/// A difficulty chip in the panel.
#[derive(Component)]
pub struct DiffChip(pub Difficulty);
/// What the selected difficulty holds: notes and best.
#[derive(Component)]
pub struct DiffFacts;
/// The rating pips of the selected difficulty, as a row of nodes.
#[derive(Component)]
pub struct PanelPips;
/// The row the version chips are spawned into.
#[derive(Component)]
pub struct VersionRow;
/// A version chip: the member (library index) it chooses.
#[derive(Component)]
pub struct VersionChip(pub usize);
/// The panel's state line (lyrics, chart, audio).
#[derive(Component)]
pub struct PanelStatus;
/// The panel beside the list (its height follows the list's).
#[derive(Component)]
pub struct DetailPanel;
/// One line of the per-difficulty bests.
#[derive(Component)]
pub struct BestLine(pub Difficulty);
/// The line that reports imports and tools.
#[derive(Component)]
pub struct ImportNote;
/// The "nothing here" note in an empty list (a button: it opens ADD
/// when the library itself is empty).
#[derive(Component)]
pub struct EmptyHint;

/// The difficulties in their order on the chips.
pub const DIFFICULTIES: [Difficulty; 4] = [
    Difficulty::Easy,
    Difficulty::Medium,
    Difficulty::Hard,
    Difficulty::Expert,
];

// ---------------------------------------------------------------- words

/// The search line: what is typed, or the invitation to type. Pure —
/// tested.
#[must_use]
pub fn search_line(filter: &str, shown: usize, total: usize, typing: bool) -> String {
    let caret = if typing { "_" } else { "" };
    if filter.is_empty() {
        format!("TYPE TO SEARCH {total} SONGS")
    } else {
        format!("SEARCH  {filter}{caret}   {shown} OF {total}   ESC CLEARS")
    }
}

/// The sort line. Pure — tested.
#[must_use]
pub fn sort_line(sort: SortMode, flipped: bool) -> String {
    let order = match (sort, flipped) {
        (SortMode::Standard, _) => "",
        (_, false) => "",
        (_, true) => "  REVERSED",
    };
    format!("SORT  {}{order}", sort.label())
}

/// The facts line under the artist. Pure — tested.
#[must_use]
pub fn facts_line(entry: &SongEntry) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(genre) = &entry.genre {
        parts.push(genre.clone());
    }
    if let Some(d) = entry.duration_s {
        parts.push(format!("{}:{:02}", d as u32 / 60, d as u32 % 60));
    }
    if entry.bpm > 0.0 {
        parts.push(format!("{:.0} BPM", entry.bpm));
    }
    parts.join("  ·  ")
}

/// What the selected difficulty holds. Pure — tested.
#[must_use]
pub fn difficulty_line(
    entry: &SongEntry,
    difficulty: Difficulty,
    best: Option<(u64, f64)>,
) -> String {
    let Some(notes) = entry.note_count(difficulty) else {
        return format!("NO {} CHART", difficulty.display_name().to_uppercase());
    };
    let best = best.map_or_else(
        || "NOT PLAYED YET".to_owned(),
        |(score, accuracy)| format!("BEST {:.0}%  ·  {score}", accuracy * 100.0),
    );
    format!("{notes} NOTES   {best}")
}

/// One line of the bests table: a difficulty and what was done on it.
/// Pure — tested.
#[must_use]
pub fn best_line(entry: &SongEntry, difficulty: Difficulty, best: Option<(u64, f64)>) -> String {
    let name = difficulty.display_name().to_uppercase();
    if entry.note_count(difficulty).is_none() {
        return format!("{name}   —");
    }
    best.map_or_else(
        || format!("{name}   NOT PLAYED"),
        |(score, accuracy)| format!("{name}   {:.0}%  ·  {score}", accuracy * 100.0),
    )
}

/// The state line: lyrics, chart and audio, in words. Pure — tested.
#[must_use]
pub fn status_line(entry: &SongEntry) -> String {
    let lyrics = match entry.polish.lyrics_mark() {
        crate::library::LyricsMark::None => "NO LYRICS",
        crate::library::LyricsMark::Line => "LYRICS BY LINE",
        crate::library::LyricsMark::Word => "LYRICS WORD BY WORD",
    };
    let chart = match entry.polish.chart_mark() {
        crate::library::ChartMark::Draft => "FIRST CHART".to_owned(),
        crate::library::ChartMark::Redesigned(v) => format!("CHART REV {v}"),
    };
    let audio = match crate::loudness::audio_label(entry.loudness.as_ref()) {
        "-" => "AUDIO NOT MEASURED".to_owned(),
        label => format!("AUDIO {label}"),
    };
    format!("{lyrics}  ·  {chart}  ·  {audio}")
}

/// The label of a version chip: NORMAL, GS, CL, BG-01 … Pure.
#[must_use]
pub fn version_label(entry: &SongEntry) -> String {
    crate::song_tree::variant_label(&entry.title)
}

/// The difficulty whose facts a row and the panel show: the selected
/// one, or the song's first when it lacks it.
#[must_use]
pub fn effective(entry: &SongEntry, selected: Difficulty) -> Difficulty {
    if entry.difficulties.contains(&selected) {
        selected
    } else {
        entry.difficulties.first().copied().unwrap_or(selected)
    }
}

// ---------------------------------------------------------------- spawn

/// A panel frame of a given width and the shared body height.
fn body_panel(width: f32, scroll: bool) -> impl Bundle {
    let mut node = ui_kit::scroll_node(width);
    node.height = px(BODY_H);
    node.max_height = px(BODY_H);
    if !scroll {
        node.overflow = Overflow::clip();
        node.row_gap = px(8.0);
    }
    (
        node,
        ScrollPosition::default(),
        BackgroundColor(palette::SURFACE.with_alpha(0.55)),
        BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.3)),
    )
}

/// The whole screen. Rows and version chips are filled in later.
pub fn spawn_shell(commands: &mut Commands, font: &UiFont, view: &BrowserView, total: usize) {
    commands
        .spawn((BrowserScreen, ui_kit::screen_root()))
        .with_children(|root| {
            ui_kit::header(root, font, "SONG SELECT", "type to search · enter to play");
            // The line above the body: search on the left, sort and
            // the tools on the right.
            root.spawn(Node {
                width: px(ui_kit::PANEL_WIDE),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(12),
                margin: UiRect::bottom(px(10)),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn((
                    SearchLine,
                    Text::new(search_line(&view.filter, view.order.len(), total, true)),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT_DIM),
                    TextLayout::default().with_no_wrap(),
                    Node {
                        flex_grow: 1.0,
                        min_width: px(0.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ));
                let chip = |bar: &mut ChildSpawnerCommands, marker: Option<bool>, label: String| {
                    let (fg, line, fill) = ui_kit::selection_chip_colours(false, false);
                    let mut e = bar.spawn((
                        Button,
                        Text::new(label),
                        font.text(ui_kit::SMALL),
                        TextColor(fg),
                        ui_kit::selection_chip_node(),
                        BorderColor::all(line),
                        BackgroundColor(fill),
                    ));
                    match marker {
                        Some(true) => {
                            e.insert(SortButton);
                        }
                        _ => {
                            e.insert(ActionsButton);
                        }
                    }
                };
                chip(bar, Some(true), sort_line(view.sort, view.flipped));
                chip(bar, None, "ACTIONS  TAB".to_owned());
            });
            root.spawn(Node {
                width: px(ui_kit::PANEL_WIDE),
                flex_direction: FlexDirection::Row,
                column_gap: px(GAP),
                ..default()
            })
            .with_children(|body| {
                body.spawn((SongList, body_panel(LIST_W, true)));
                body.spawn((DetailPanel, body_panel(PANEL_W, false))).with_children(|panel| {
                    spawn_panel(panel, font);
                });
            });
            root.spawn((
                ImportNote,
                Text::new("drag an audio file onto the window to import it"),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            crate::prompts::device_footer(
                root,
                font,
                "UP/DOWN song  LEFT/RIGHT difficulty  SHIFT+LEFT/RIGHT version  ENTER play  TAB actions  ESC back",
                "D-PAD song and difficulty  SOUTH play  EAST back",
            );
            ui_kit::back_button(root, font, "MAIN MENU");
        });
}

fn spawn_panel(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    let small = |panel: &mut ChildSpawnerCommands, text: &str| {
        panel.spawn((
            Text::new(text.to_owned()),
            font.text(ui_kit::SMALL),
            TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
            Node {
                margin: UiRect::top(px(6)),
                ..default()
            },
        ));
    };
    panel.spawn((
        PanelTitle,
        Text::new(""),
        font.text(ui_kit::TITLE),
        TextColor(palette::BRAND),
    ));
    panel.spawn((
        PanelArtist,
        Text::new(""),
        font.text(ui_kit::ROW),
        TextColor(palette::TEXT),
    ));
    panel.spawn((
        PanelFacts,
        Text::new(""),
        font.text(ui_kit::SMALL),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
    ));
    small(panel, "DIFFICULTY");
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6),
            ..default()
        })
        .with_children(|chips| {
            for difficulty in DIFFICULTIES {
                let (fg, line, fill) = ui_kit::selection_chip_colours(false, false);
                chips.spawn((
                    DiffChip(difficulty),
                    Button,
                    Text::new(difficulty.display_name().to_uppercase()),
                    font.text(ui_kit::SMALL),
                    TextColor(fg),
                    ui_kit::selection_chip_node(),
                    BorderColor::all(line),
                    BackgroundColor(fill),
                ));
            }
        });
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        })
        .with_children(|line| {
            line.spawn((
                PanelPips,
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(3),
                    ..default()
                },
            ))
            .with_children(|pips| {
                for _ in 0..5 {
                    pips.spawn((
                        Node {
                            width: px(PIP),
                            height: px(PIP),
                            ..default()
                        },
                        BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.3)),
                    ));
                }
            });
            line.spawn((
                DiffFacts,
                Text::new(""),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
            ));
        });
    small(panel, "VERSION");
    panel.spawn((
        VersionRow,
        Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(6),
            row_gap: px(6),
            ..default()
        },
    ));
    small(panel, "BEST");
    for difficulty in DIFFICULTIES {
        panel.spawn((
            BestLine(difficulty),
            Text::new(""),
            font.text(ui_kit::SMALL),
            TextColor(palette::TEXT_DIM),
        ));
    }
    panel.spawn((
        PanelStatus,
        Text::new(""),
        font.text(ui_kit::SMALL),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
        Node {
            margin: UiRect::top(px(10)),
            ..default()
        },
    ));
}

/// One row per family, two lines each: the song and its artist on the
/// left, the best accuracy of the selected difficulty on the right.
#[allow(clippy::too_many_arguments)] // the row needs all of it
pub fn spawn_rows(
    commands: &mut Commands,
    list: Entity,
    font: &UiFont,
    library: &SongLibrary,
    view: &BrowserView,
    scores: &ScoreBoard,
    selected: Difficulty,
) {
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|panel| {
        if view.families.is_empty() {
            panel.spawn((
                EmptyHint,
                Button,
                Text::new(crate::song_select::empty_hint(
                    library.entries.len(),
                    &view.filter,
                )),
                font.text(ui_kit::ROW),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
            ));
            return;
        }
        for (position, family) in view.families.iter().enumerate() {
            let Some(&member) = view.order.get(position) else {
                continue;
            };
            let Some(entry) = library.entries.get(member) else {
                continue;
            };
            let difficulty = effective(entry, selected);
            let best = scores
                .best(
                    entry.song_id.as_deref(),
                    &entry.title,
                    &entry.artist,
                    difficulty,
                )
                .map_or_else(String::new, |b| format!("{:.0}%", b.accuracy * 100.0));
            let versions = family.members.len();
            panel
                .spawn((SongRow(position), Button, ui_kit::row()))
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        min_width: px(0.0),
                        overflow: Overflow::clip(),
                        ..default()
                    })
                    .with_children(|text| {
                        text.spawn((
                            RowTitle(position),
                            Text::new(
                                font.safe(&crate::song_family::row_title(&library.entries, family)),
                            ),
                            font.text(ui_kit::ROW),
                            TextColor(palette::TEXT_DIM),
                            TextLayout::default().with_no_wrap(),
                        ));
                        let artist = if versions > 1 {
                            format!("{}   ·   {versions} VERSIONS", entry.artist)
                        } else {
                            entry.artist.clone()
                        };
                        text.spawn((
                            RowArtist(position),
                            Text::new(font.safe(&artist)),
                            font.text(ui_kit::SMALL),
                            TextColor(ui_kit::dimmed_subtitle()),
                            TextLayout::default().with_no_wrap(),
                        ));
                    });
                    row.spawn((
                        RowBest(position),
                        Text::new(best),
                        font.text(ui_kit::ROW),
                        TextColor(palette::TEXT_DIM),
                        Node {
                            flex_shrink: 0.0,
                            margin: UiRect::left(px(12)),
                            ..default()
                        },
                    ));
                });
        }
    });
}

/// The version chips of the selected family, rebuilt when the family
/// changes.
pub fn spawn_versions(
    commands: &mut Commands,
    row: Entity,
    font: &UiFont,
    library: &SongLibrary,
    family: Option<&Family>,
) {
    commands.entity(row).despawn_children();
    let Some(family) = family else {
        return;
    };
    commands.entity(row).with_children(|chips| {
        for &member in &family.members {
            let Some(entry) = library.entries.get(member) else {
                continue;
            };
            let (fg, line, fill) = ui_kit::selection_chip_colours(false, false);
            chips.spawn((
                VersionChip(member),
                Button,
                Text::new(version_label(entry)),
                font.text(ui_kit::SMALL),
                TextColor(fg),
                ui_kit::selection_chip_node(),
                BorderColor::all(line),
                BackgroundColor(fill),
            ));
        }
    });
}

/// Paint the rating pips: `rating` of five lit.
pub fn paint_pips(pips: &Children, fills: &mut Query<&mut BackgroundColor, PipOnly>, rating: u8) {
    for (i, child) in pips.iter().enumerate() {
        if let Ok(mut fill) = fills.get_mut(child) {
            fill.0 = if i < usize::from(rating) {
                palette::BRAND
            } else {
                palette::dimmed(palette::TEXT_DIM, 0.3)
            };
        }
    }
}

/// A pip, and nothing else that carries a background.
pub type PipOnly = (
    Without<SongRow>,
    Without<DiffChip>,
    Without<VersionChip>,
    Without<SortButton>,
    Without<ActionsButton>,
    Without<crate::ui_kit::BackButton>,
);

/// Whether a song is one this machine can open in the editor, give a
/// document to, delete: a file, not a built-in.
#[must_use]
pub fn is_file(entry: &SongEntry) -> bool {
    matches!(entry.source, SongSource::File { .. })
}

/// The action menu: a dimmed screen and, centred on it, the tools as
/// a list on the shared renderer — the same rows, keys and look as
/// every other menu.
pub fn spawn_action_menu(commands: &mut Commands, font: &UiFont, items: &[u8], song: &str) {
    let labels: Vec<&str> = items
        .iter()
        .filter_map(|id| crate::song_select::TOOLS.iter().find(|t| t.id == *id))
        .map(|tool| tool.label)
        .collect();
    commands
        .spawn((
            crate::song_select::ActionOverlay,
            DespawnOnExit(crate::states::AppState::SongSelect),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Opaque: a tall menu over a dimmed browser let the title and
            // the back button read through it (seen on the first shot).
            BackgroundColor(palette::BACKGROUND),
            GlobalZIndex(10),
        ))
        .with_children(|overlay| {
            ui_kit::header(overlay, font, "ACTIONS", &font.safe(song));
            overlay
                .spawn((
                    crate::menu_list::list::ListPanel::<crate::song_select::ActionRows>::new(),
                    ui_kit::panel(),
                ))
                // Opaque: the browser behind it must not read through
                // the menu's rows.
                .insert(BackgroundColor(palette::BACKGROUND))
                .with_children(|panel| {
                    crate::menu_list::list::spawn_rows::<crate::song_select::ActionRows>(
                        panel, font, labels,
                    );
                });
            ui_kit::footer(overlay, font, "UP/DOWN choose  ENTER run  ESC close");
        });
}

// ---------------------------------------------------------------- paint

/// The selected song: the member that plays at the cursor.
fn selected_entry<'a>(
    library: &'a SongLibrary,
    view: &BrowserView,
    cursor: &BrowserCursor,
) -> Option<&'a SongEntry> {
    view.order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i))
}

/// Row fills, borders and text colours for the cursor.
#[allow(clippy::type_complexity, clippy::needless_pass_by_value)] // Bevy system
pub fn paint_rows(
    settings: Res<crate::config::Settings>,
    cursor: Res<BrowserCursor>,
    mut rows: Query<(&SongRow, &mut BackgroundColor, &mut BorderColor)>,
    mut titles: Query<(&RowTitle, &mut TextColor), (Without<RowArtist>, Without<RowBest>)>,
    mut artists: Query<(&RowArtist, &mut TextColor), (Without<RowTitle>, Without<RowBest>)>,
    mut bests: Query<(&RowBest, &mut TextColor), (Without<RowTitle>, Without<RowArtist>)>,
) {
    let style = |position: usize| {
        ui_kit::styled_row(
            ui_kit::state_for(position == cursor.0, false),
            settings.high_contrast,
        )
    };
    for (row, mut background, mut border) in &mut rows {
        let style = style(row.0);
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (title, mut colour) in &mut titles {
        colour.0 = style(title.0).label;
    }
    for (artist, mut colour) in &mut artists {
        colour.0 = style(artist.0).value;
    }
    for (best, mut colour) in &mut bests {
        colour.0 = style(best.0).value;
    }
}

/// The search line, the sort button and the actions button.
#[allow(clippy::type_complexity, clippy::needless_pass_by_value)] // Bevy system
pub fn paint_bar(
    view: Res<BrowserView>,
    library: Res<SongLibrary>,
    menu: Res<crate::song_select::ActionMenu>,
    mut search: Query<
        (&mut Text, &mut TextColor),
        (
            With<SearchLine>,
            Without<SortButton>,
            Without<ActionsButton>,
        ),
    >,
    mut sort: Query<
        (
            &Interaction,
            &mut Text,
            &mut TextColor,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            With<SortButton>,
            Without<SearchLine>,
            Without<ActionsButton>,
        ),
    >,
    mut actions: Query<
        (
            &Interaction,
            &mut TextColor,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            With<ActionsButton>,
            Without<SearchLine>,
            Without<SortButton>,
        ),
    >,
) {
    if let Ok((mut text, mut colour)) = search.single_mut() {
        let wanted = search_line(&view.filter, view.order.len(), library.entries.len(), true);
        if text.0 != wanted {
            text.0 = wanted;
        }
        colour.0 = if view.filter.is_empty() {
            ui_kit::dimmed_subtitle()
        } else {
            palette::BRAND
        };
    }
    for (interaction, mut text, mut colour, mut background, mut border) in &mut sort {
        let wanted = sort_line(view.sort, view.flipped);
        if text.0 != wanted {
            text.0 = wanted;
        }
        ui_kit::paint_selection_chip(
            *interaction,
            false,
            &mut background,
            &mut border,
            Some(&mut colour),
        );
    }
    for (interaction, mut colour, mut background, mut border) in &mut actions {
        ui_kit::paint_selection_chip(
            *interaction,
            menu.open,
            &mut background,
            &mut border,
            Some(&mut colour),
        );
    }
}

/// Everything in the panel: the song, its difficulty, its versions.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::needless_pass_by_value
)] // Bevy system
pub fn paint_panel(
    library: Res<SongLibrary>,
    view: Res<BrowserView>,
    cursor: Res<BrowserCursor>,
    selected: Res<SelectedDifficulty>,
    scores: Res<ScoreBoard>,
    font: Res<UiFont>,
    mut texts: ParamSet<(
        Query<&mut Text, With<PanelTitle>>,
        Query<&mut Text, With<PanelArtist>>,
        Query<&mut Text, With<PanelFacts>>,
        Query<&mut Text, With<DiffFacts>>,
        Query<&mut Text, With<PanelStatus>>,
    )>,
    mut diffs: Query<
        (
            &DiffChip,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut TextColor,
        ),
        Without<VersionChip>,
    >,
    mut versions: Query<
        (
            &VersionChip,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut TextColor,
        ),
        Without<DiffChip>,
    >,
    pips: Query<&Children, With<PanelPips>>,
    mut fills: Query<&mut BackgroundColor, PipOnly>,
    mut bests: Query<
        (&BestLine, &mut Text),
        (
            Without<PanelTitle>,
            Without<PanelArtist>,
            Without<PanelFacts>,
            Without<DiffFacts>,
            Without<PanelStatus>,
        ),
    >,
) {
    let entry = selected_entry(&library, &view, &cursor);
    let family = view.families.get(cursor.0);
    let set = |text: &mut Text, wanted: String| {
        if text.0 != wanted {
            text.0 = wanted;
        }
    };
    let title = family.map_or_else(String::new, |f| {
        font.safe(&crate::song_family::row_title(&library.entries, f))
    });
    if let Ok(mut text) = texts.p0().single_mut() {
        set(&mut text, title);
    }
    if let Ok(mut text) = texts.p1().single_mut() {
        set(
            &mut text,
            entry.map_or_else(String::new, |e| font.safe(&e.artist)),
        );
    }
    if let Ok(mut text) = texts.p2().single_mut() {
        set(
            &mut text,
            entry.map_or_else(String::new, |e| font.safe(&facts_line(e))),
        );
    }
    let difficulty = entry.map_or(selected.0, |e| effective(e, selected.0));
    if let Ok(mut text) = texts.p3().single_mut() {
        let line = entry.map_or_else(String::new, |e| {
            let best = scores
                .best(e.song_id.as_deref(), &e.title, &e.artist, difficulty)
                .map(|b| (b.score, b.accuracy));
            difficulty_line(e, difficulty, best)
        });
        set(&mut text, line);
    }
    if let Ok(mut text) = texts.p4().single_mut() {
        set(&mut text, entry.map_or_else(String::new, status_line));
    }
    for (chip, interaction, mut background, mut border, mut colour) in &mut diffs {
        let offered = entry.is_some_and(|e| e.difficulties.contains(&chip.0));
        let chosen = offered && chip.0 == difficulty;
        let hot = offered && *interaction != Interaction::None;
        let (fg, line, fill) = ui_kit::selection_chip_colours(chosen, hot);
        background.0 = fill;
        *border = BorderColor::all(if offered { line } else { line.with_alpha(0.15) });
        colour.0 = if offered { fg } else { fg.with_alpha(0.3) };
    }
    let playing = view.order.get(cursor.0).copied();
    for (chip, interaction, mut background, mut border, mut colour) in &mut versions {
        ui_kit::paint_selection_chip(
            *interaction,
            Some(chip.0) == playing,
            &mut background,
            &mut border,
            Some(&mut colour),
        );
    }
    for (line, mut text) in &mut bests {
        let wanted = entry.map_or_else(String::new, |e| {
            let best = scores
                .best(e.song_id.as_deref(), &e.title, &e.artist, line.0)
                .map(|b| (b.score, b.accuracy));
            best_line(e, line.0, best)
        });
        set(&mut text, wanted);
    }
    let rating = entry.and_then(|e| e.rating(difficulty)).unwrap_or(0);
    for children in &pips {
        paint_pips(children, &mut fills, rating);
    }
}

/// Rebuild the version chips when the selected family changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system
pub fn sync_versions(
    mut commands: Commands,
    library: Res<SongLibrary>,
    view: Res<BrowserView>,
    cursor: Res<BrowserCursor>,
    font: Res<UiFont>,
    rows: Query<Entity, With<VersionRow>>,
    fresh: Query<(), Added<VersionRow>>,
    mut shown: Local<Option<Vec<usize>>>,
) {
    let family = view.families.get(cursor.0);
    let wanted = family.map(|f| f.members.clone());
    if fresh.is_empty() && *shown == wanted && !library.is_changed() {
        return;
    }
    if let Ok(row) = rows.single() {
        spawn_versions(&mut commands, row, &font, &library, family);
        *shown = wanted;
    }
}

/// Keep the cursor row in view, and size both panels to whole rows —
/// a row cut through its letters at the bottom read as a broken list.
/// The height depends only on the row's measured height, never on how
/// many songs a search left, so the page does not move while typing.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)] // Bevy system
pub fn follow_selection(
    cursor: Res<BrowserCursor>,
    view: Res<BrowserView>,
    rows: Query<&ComputedNode, With<SongRow>>,
    mut lists: Query<(&mut ScrollPosition, &mut Node), (With<SongList>, Without<DetailPanel>)>,
    mut panels: Query<&mut Node, (With<DetailPanel>, Without<SongList>)>,
) {
    let Ok((mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row) = rows.iter().find(|node| node.size().y > 0.0) else {
        return;
    };
    let row_h = row.size().y * row.inverse_scale_factor();
    let height =
        ui_kit::whole_rows_height(row_h, ui_kit::ROW_GAP, usize::MAX, BODY_H).unwrap_or(BODY_H);
    for target in std::iter::once(&mut *node).chain(panels.iter_mut().map(|n| n.into_inner())) {
        if target.height != px(height) {
            target.height = px(height);
            target.max_height = px(height);
        }
    }
    let count = view.order.len() as f32;
    let content_h = count.mul_add(row_h, (count - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = height - 2.0 * (ui_kit::PANEL_PAD + ui_kit::PANEL_BORDER);
    #[allow(clippy::cast_precision_loss)] // a row index
    let row_top = cursor.0 as f32 * (row_h + ui_kit::ROW_GAP);
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Polish;
    use std::path::PathBuf;

    fn entry() -> SongEntry {
        SongEntry {
            title: "[BG-01] Killer Queen".to_owned(),
            artist: "Queen".to_owned(),
            bpm: 117.0,
            duration_s: Some(182.0),
            difficulties: vec![Difficulty::Medium, Difficulty::Hard],
            note_counts: vec![250, 386],
            genre: Some("Rock".to_owned()),
            has_lyrics: false,
            preview_start_s: None,
            loudness: None,
            song_id: None,
            polish: Polish::default(),
            source: SongSource::File {
                chart_path: PathBuf::from("/lib/kq/chart.json"),
                audio_path: PathBuf::from("/lib/kq/song.m4a"),
            },
        }
    }

    #[test]
    fn the_search_line_invites_then_counts() {
        assert_eq!(search_line("", 0, 466, true), "TYPE TO SEARCH 466 SONGS");
        assert_eq!(
            search_line("queen", 4, 466, true),
            "SEARCH  queen_   4 OF 466   ESC CLEARS"
        );
    }

    #[test]
    fn the_sort_line_names_the_sort_and_says_when_it_is_reversed() {
        assert_eq!(sort_line(SortMode::Title, false), "SORT  TITLE");
        assert_eq!(sort_line(SortMode::Title, true), "SORT  TITLE  REVERSED");
        assert_eq!(sort_line(SortMode::Standard, true), "SORT  STANDARD");
    }

    #[test]
    fn the_panel_says_what_a_song_is_in_words() {
        let e = entry();
        assert_eq!(facts_line(&e), "Rock  ·  3:02  ·  117 BPM");
        assert_eq!(
            difficulty_line(&e, Difficulty::Hard, None),
            "386 NOTES   NOT PLAYED YET"
        );
        assert_eq!(
            difficulty_line(&e, Difficulty::Hard, Some((79447, 0.85))),
            "386 NOTES   BEST 85%  ·  79447"
        );
        assert_eq!(
            difficulty_line(&e, Difficulty::Expert, None),
            "NO EXPERT CHART"
        );
        assert_eq!(
            status_line(&e),
            "NO LYRICS  ·  FIRST CHART  ·  AUDIO NOT MEASURED"
        );
        assert_eq!(version_label(&e), "BG-01");
        assert_eq!(best_line(&e, Difficulty::Hard, None), "HARD   NOT PLAYED");
        assert_eq!(
            best_line(&e, Difficulty::Hard, Some((5, 0.854))),
            "HARD   85%  ·  5"
        );
        assert_eq!(best_line(&e, Difficulty::Easy, None), "EASY   —");
    }

    #[test]
    fn a_song_without_the_selected_difficulty_shows_its_first() {
        let e = entry();
        assert_eq!(effective(&e, Difficulty::Hard), Difficulty::Hard);
        assert_eq!(effective(&e, Difficulty::Easy), Difficulty::Medium);
    }
}
