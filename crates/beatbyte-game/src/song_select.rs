//! The song browser: pick a song and a difficulty, see your best.

use beatbyte_core::Difficulty;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;

use crate::boot::{BuiltinSongs, LoadedSong, SongAudio};
use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::palette;
use crate::scores::ScoreBoard;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The difficulty the player will play.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedDifficulty(pub Difficulty);

impl Default for SelectedDifficulty {
    fn default() -> Self {
        SelectedDifficulty(Difficulty::Medium)
    }
}

/// The highlighted song row (a position in the VIEW, not a library
/// index — the view sorts and filters).
#[derive(Resource, Default)]
pub struct BrowserCursor(pub usize);

/// How the list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// The library's own order: built-ins first, then by title — the
    /// order the browser has always shown, and the default.
    #[default]
    Standard,
    /// Alphabetical by title.
    Title,
    /// Alphabetical by artist.
    Artist,
    /// Alphabetical by genre (untagged songs last).
    Genre,
    /// Shortest first.
    Length,
    /// Highest personal best first (no record last).
    Best,
}

impl SortMode {
    /// The next mode in the `S` cycle.
    #[must_use]
    pub fn next(self) -> SortMode {
        match self {
            SortMode::Standard => SortMode::Title,
            SortMode::Title => SortMode::Artist,
            SortMode::Artist => SortMode::Genre,
            SortMode::Genre => SortMode::Length,
            SortMode::Length => SortMode::Best,
            SortMode::Best => SortMode::Standard,
        }
    }

    /// Label for the status line.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Standard => "STANDARD",
            SortMode::Title => "TITLE",
            SortMode::Artist => "ARTIST",
            SortMode::Genre => "GENRE",
            SortMode::Length => "LENGTH",
            SortMode::Best => "BEST",
        }
    }
}

/// What the list currently shows: which library entries, in which
/// order, under which filter.
#[derive(Resource, Default)]
pub struct BrowserView {
    /// Library indices in display order.
    pub order: Vec<usize>,
    /// Active sort.
    pub sort: SortMode,
    /// Active search text (already lowercase).
    pub filter: String,
    /// Whether typing currently goes into the filter.
    pub searching: bool,
}

/// Case- and diacritic-insensitive haystack for filtering: `fold_latin`
/// is what makes "Sacre" find "Sacré".
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            crate::ui::fold_latin(c).map_or_else(
                || c.to_lowercase().collect::<Vec<_>>(),
                |s| s.chars().collect(),
            )
        })
        .collect::<String>()
        .to_lowercase()
}

/// Whether an entry matches the (already folded) filter.
fn matches_filter(entry: &SongEntry, folded: &str) -> bool {
    if folded.is_empty() {
        return true;
    }
    fold(&entry.title).contains(folded)
        || fold(&entry.artist).contains(folded)
        || entry
            .genre
            .as_deref()
            .is_some_and(|g| fold(g).contains(folded))
}

/// The display order for the current sort and filter. Pure: same
/// inputs, same order — ties always break by title, then by library
/// index, so the list never shuffles between frames.
fn build_order(
    entries: &[SongEntry],
    sort: SortMode,
    filter: &str,
    best: impl Fn(&SongEntry) -> Option<u64>,
) -> Vec<usize> {
    let folded = fold(filter);
    let mut order: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| matches_filter(entry, &folded))
        .map(|(i, _)| i)
        .collect();
    let tie = |i: &usize| (fold(&entries[*i].title), *i);
    match sort {
        SortMode::Standard => {}
        SortMode::Title => order.sort_by_key(tie),
        SortMode::Artist => order.sort_by_key(|i| (fold(&entries[*i].artist), tie(i))),
        SortMode::Genre => {
            // Untagged songs sort last, not first: an absent genre is
            // an absence, not the alphabet's beginning.
            order.sort_by_key(|i| {
                (
                    entries[*i].genre.is_none(),
                    entries[*i].genre.as_deref().map(fold).unwrap_or_default(),
                    tie(i),
                )
            });
        }
        SortMode::Length => {
            order.sort_by_key(|i| {
                (
                    entries[*i].duration_s.is_none(),
                    entries[*i].duration_s.map_or(0, |d| (d * 1000.0) as u64),
                    tie(i),
                )
            });
        }
        SortMode::Best => {
            order.sort_by_key(|i| {
                let score = best(&entries[*i]);
                (
                    score.is_none(),
                    std::cmp::Reverse(score.unwrap_or(0)),
                    tie(i),
                )
            });
        }
    }
    order
}

/// Where the cursor should sit after the order changed: on the same
/// SONG if it survived, else clamped — a sort change must never
/// teleport the selection to a random track.
fn stable_cursor(old_order: &[usize], cursor: usize, new_order: &[usize]) -> usize {
    old_order
        .get(cursor)
        .and_then(|song| new_order.iter().position(|i| i == song))
        .unwrap_or_else(|| cursor.min(new_order.len().saturating_sub(1)))
}

/// Truncate for a fixed column, marking the cut.
fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('~');
    out
}

/// `m:ss` for a duration column.
fn length_label(duration_s: Option<f64>) -> String {
    duration_s.map_or_else(
        || "-".to_owned(),
        |d| format!("{}:{:02}", d as u32 / 60, d as u32 % 60),
    )
}

/// Plugin for the song browser.
pub struct SongSelectPlugin;

impl Plugin for SongSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedDifficulty>()
            .init_resource::<BrowserCursor>()
            .init_resource::<BrowserView>()
            .add_systems(OnEnter(AppState::SongSelect), spawn_browser)
            .add_systems(
                Update,
                (
                    browser_input,
                    sync_view,
                    refresh_browser,
                    rebuild_after_import,
                    follow_selection,
                )
                    .chain()
                    .run_if(in_state(AppState::SongSelect)),
            )
            .add_systems(OnExit(AppState::SongSelect), despawn_browser);
    }
}

#[derive(Component)]
struct BrowserScreen;

/// A song row (index into the library). Carries `Button`.
#[derive(Component)]
struct SongRow(usize);

/// A row's title text.
#[derive(Component)]
struct SongTitle(usize);

/// A row's artist text, in the right-hand column.
#[derive(Component)]
struct SongArtist(usize);

/// The details block under the list.
#[derive(Component)]
struct DetailText;

/// The scrolling viewport the song rows live in.
#[derive(Component)]
struct SongList;

// Column widths in px. Press Start 2P advances ~1 em per glyph, so
// SMALL (10 px) columns hold width/10 characters.
const COL_ARTIST: f32 = 190.0;
const COL_GENRE: f32 = 120.0;
const COL_LEN: f32 = 56.0;
const COL_NOTES: f32 = 56.0;
const COL_RATING: f32 = 62.0;
const COL_BEST: f32 = 92.0;

fn spawn_browser(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut view: ResMut<BrowserView>,
    scores: Res<ScoreBoard>,
    selected: Res<SelectedDifficulty>,
) {
    let difficulty = selected.0;
    let order = build_order(&library.entries, view.sort, &view.filter, |entry| {
        scores
            .best(&entry.title, &entry.artist, difficulty)
            .map(|b| b.score)
    });
    // Bypass change detection: this is the render of the view, not an
    // edit to it — marking it changed would retrigger sync_view.
    let raw = view.bypass_change_detection();
    raw.order = order;
    if cursor.0 >= raw.order.len() {
        cursor.0 = 0;
    }
    spawn_browser_impl(&mut commands, &font, &library, raw, &scores, difficulty);
}

/// One SMALL-font cell of fixed width.
fn cell(row: &mut ChildSpawnerCommands, font: &UiFont, marker: usize, text: String, width: f32) {
    row.spawn((
        SongArtist(marker),
        Text::new(text),
        font.text(ui_kit::SMALL),
        TextColor(palette::TEXT_DIM),
        TextLayout::default().with_no_wrap(),
        Node {
            width: px(width),
            flex_shrink: 0.0,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

fn spawn_browser_impl(
    commands: &mut Commands,
    font: &UiFont,
    library: &SongLibrary,
    view: &BrowserView,
    scores: &ScoreBoard,
    selected: Difficulty,
) {
    commands
        .spawn((BrowserScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, font, "SONG SELECT", "pick a track and a difficulty");
            // Sort / search status line.
            let status = if view.searching {
                format!(
                    "sort {}   search: {}_   ({} match{})",
                    view.sort.label(),
                    view.filter,
                    view.order.len(),
                    if view.order.len() == 1 { "" } else { "es" }
                )
            } else if view.filter.is_empty() {
                format!("sort {}   / to search", view.sort.label())
            } else {
                format!(
                    "sort {}   filter: {} ({} match{})",
                    view.sort.label(),
                    view.filter,
                    view.order.len(),
                    if view.order.len() == 1 { "" } else { "es" }
                )
            };
            parent.spawn((
                Text::new(font.safe(&status)),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
                Node {
                    margin: UiRect::bottom(px(6)),
                    ..default()
                },
            ));
            // Column captions, aligned with the row cells.
            parent
                .spawn(Node {
                    width: px(ui_kit::PANEL_WIDE),
                    padding: UiRect::horizontal(px(ui_kit::PANEL_PAD + 14.0)),
                    column_gap: px(8.0),
                    ..default()
                })
                .with_children(|head| {
                    let caption = |head: &mut ChildSpawnerCommands, text: &str, width: Option<f32>| {
                        let mut node = Node {
                            flex_shrink: 0.0,
                            ..default()
                        };
                        if let Some(w) = width {
                            node.width = px(w);
                        } else {
                            node.flex_grow = 1.0;
                            node.min_width = px(0.0);
                        }
                        head.spawn((
                            Text::new(text.to_owned()),
                            font.text(ui_kit::SMALL),
                            TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                            node,
                        ));
                    };
                    caption(head, "TITLE", None);
                    caption(head, "ARTIST", Some(COL_ARTIST));
                    caption(head, "GENRE", Some(COL_GENRE));
                    caption(head, "LEN", Some(COL_LEN));
                    caption(head, "NOTES", Some(COL_NOTES));
                    caption(head, "DIFF", Some(COL_RATING));
                    caption(head, "BEST", Some(COL_BEST));
                });
            parent
                .spawn((SongList, ui_kit::scroll_panel(ui_kit::PANEL_WIDE)))
                .with_children(|panel| {
                    for (position, song_index) in view.order.iter().enumerate() {
                        let Some(entry) = library.entries.get(*song_index) else {
                            continue;
                        };
                        // Facts follow the SELECTED difficulty; a song
                        // that lacks it shows its first one instead.
                        let effective = if entry.difficulties.contains(&selected) {
                            selected
                        } else {
                            entry.difficulties.first().copied().unwrap_or(selected)
                        };
                        let best = scores
                            .best(&entry.title, &entry.artist, effective)
                            .map_or_else(|| "-".to_owned(), |b| b.score.to_string());
                        panel
                            .spawn((SongRow(position), Button, ui_kit::row()))
                            .with_children(|row| {
                                row.spawn((
                                    SongTitle(position),
                                    Text::new(font.safe(&clip_chars(&entry.title, 32))),
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
                                cell(
                                    row,
                                    font,
                                    position,
                                    font.safe(&clip_chars(&entry.artist, 18)),
                                    COL_ARTIST,
                                );
                                cell(
                                    row,
                                    font,
                                    position,
                                    font.safe(&clip_chars(
                                        entry.genre.as_deref().unwrap_or("-"),
                                        11,
                                    )),
                                    COL_GENRE,
                                );
                                cell(row, font, position, length_label(entry.duration_s), COL_LEN);
                                cell(
                                    row,
                                    font,
                                    position,
                                    entry
                                        .note_count(effective)
                                        .map_or_else(|| "-".to_owned(), |n| n.to_string()),
                                    COL_NOTES,
                                );
                                cell(
                                    row,
                                    font,
                                    position,
                                    entry.rating(effective).map_or_else(
                                        || "-".to_owned(),
                                        |r| "*".repeat(usize::from(r)),
                                    ),
                                    COL_RATING,
                                );
                                cell(row, font, position, best, COL_BEST);
                            });
                    }
                });
            parent.spawn((
                DetailText,
                Text::new(""),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
                    ..default()
                },
            ));
            parent.spawn((
                ImportNote,
                Text::new("drag an audio file onto the window to import it"),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            ui_kit::footer(
                parent,
                font,
                "UP/DOWN song  LEFT/RIGHT difficulty  S sort  / search  ENTER rock  E edit  DEL delete  ESC back",
            );
        });
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn browser_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut view: ResMut<BrowserView>,
    pads: Query<&Gamepad>,
    mut library: ResMut<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut selected: ResMut<SelectedDifficulty>,
    builtins: Res<BuiltinSongs>,
    mut next_state: ResMut<NextState<AppState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    rows: Query<(&SongRow, &Interaction), Changed<Interaction>>,
    time: Res<Time>,
    mut status: ResMut<crate::import::ImportStatus>,
    mut delete_armed: Local<(Option<usize>, f32)>,
) {
    let nav = MenuNav::read(&keys, pads.iter());
    let searching = view.searching;
    // ── Search mode ─────────────────────────────────────────────
    // While it is on, printable keys EDIT THE FILTER — every letter
    // shortcut below (E, S, DEL) is suppressed, or typing "elle"
    // would open the editor and arm a delete on the way.
    if searching {
        for event in typed.read() {
            if !event.state.is_pressed() {
                continue;
            }
            if let bevy::input::keyboard::Key::Character(text) = &event.logical_key {
                for c in text.chars().filter(|c| !c.is_control()) {
                    for low in c.to_lowercase() {
                        view.filter.push(low);
                    }
                }
            } else if event.logical_key == bevy::input::keyboard::Key::Space {
                view.filter.push(' ');
            }
        }
        if keys.just_pressed(KeyCode::Backspace) {
            view.filter.pop();
        }
        // Esc leaves search AND clears it: the recoverable state is
        // "the whole list", not "a filter you can no longer see".
        if keys.just_pressed(KeyCode::Escape) {
            view.searching = false;
            view.filter.clear();
        }
    } else {
        typed.clear();
        if keys.just_pressed(KeyCode::Slash) {
            view.searching = true;
        }
        if keys.just_pressed(KeyCode::KeyS) {
            let next = view.sort.next();
            view.sort = next;
        }
    }
    let back = (!searching && nav.back) || mouse.just_pressed(MouseButton::Right);
    let count = view.order.len();
    if count == 0 {
        if back {
            next_state.set(AppState::MainMenu);
        }
        return;
    }
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    // Mouse: wheel scrolls the list; clicking a row selects it, and
    // clicking the already-selected row starts it.
    for event in wheel.read() {
        if event.y > 0.0 {
            cursor.0 = (cursor.0 + count - 1) % count;
        } else if event.y < 0.0 {
            cursor.0 = (cursor.0 + 1) % count;
        }
    }
    // Hover selects, click starts — the same rule as every other
    // menu. This list used to need two clicks (one to select, one to
    // start) and ignored hover entirely.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        cursor.0 = index;
    }
    let clicked_selected = pointer.clicked;
    let Some(entry) = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i))
    else {
        return;
    };

    // Difficulty stepping is constrained to what the chart offers.
    let offered = &entry.difficulties;
    if !offered.contains(&selected.0)
        && let Some(&first) = offered.first()
    {
        selected.0 = first;
    }
    let position = offered.iter().position(|d| *d == selected.0).unwrap_or(0);
    if nav.left && position > 0 {
        selected.0 = offered[position - 1];
    }
    if nav.right && position + 1 < offered.len() {
        selected.0 = offered[position + 1];
    }

    if nav.confirm || clicked_selected {
        match prepare_song(entry, &builtins) {
            Ok(song) => {
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("cannot load \"{}\": {reason}", entry.title),
        }
    }
    // E opens the chart editor (file-based songs only — the demo is
    // generated, editing it would be lost on the next boot).
    if !searching
        && keys.just_pressed(KeyCode::KeyE)
        && let crate::library::SongSource::File {
            chart_path,
            audio_path,
        } = &entry.source
    {
        match crate::editor_ui::open_editor(&mut commands, chart_path, audio_path, selected.0) {
            Ok(()) => next_state.set(AppState::Editor),
            Err(reason) => error!("cannot edit \"{}\": {reason}", entry.title),
        }
    }
    // BACKSPACE/DEL removes the highlighted song from disk — twice,
    // because it deletes files. Built-ins cannot be removed.
    delete_armed.1 = (delete_armed.1 - time.delta_secs()).max(0.0);
    if delete_armed.1 <= 0.0 {
        delete_armed.0 = None;
    }
    if !searching && (keys.just_pressed(KeyCode::Backspace) || keys.just_pressed(KeyCode::Delete)) {
        match &entry.source {
            crate::library::SongSource::Builtin(_) => {
                status.0 = "built-in songs cannot be deleted".to_owned();
            }
            crate::library::SongSource::File { chart_path, .. } => {
                if delete_armed.0 == Some(cursor.0) {
                    let title = entry.title.clone();
                    match crate::library::remove_song_files(chart_path) {
                        Ok(()) => {
                            status.0 = format!("\"{title}\" deleted");
                            let charts: Vec<_> =
                                builtins.0.iter().map(|song| song.chart.clone()).collect();
                            *library = crate::library::scan_library(&charts);
                        }
                        Err(reason) => status.0 = format!("cannot delete: {reason}"),
                    }
                    *delete_armed = (None, 0.0);
                } else {
                    *delete_armed = (Some(cursor.0), 3.0);
                    status.0 = format!(
                        "delete \"{}\" and its files? press again to confirm",
                        entry.title
                    );
                }
            }
        }
    }
    if back {
        next_state.set(AppState::MainMenu);
    }
}

/// Build the [`LoadedSong`] for an entry. Built-ins come from cache;
/// file songs re-read their chart (it may have changed on disk) and
/// stream audio from the resolved path.
pub fn prepare_song(entry: &SongEntry, builtins: &BuiltinSongs) -> Result<LoadedSong, String> {
    match &entry.source {
        SongSource::Builtin(index) => builtins
            .0
            .get(*index)
            .cloned()
            .ok_or_else(|| format!("built-in song index {index} not loaded")),
        SongSource::File {
            chart_path,
            audio_path,
        } => {
            let chart = beatbyte_chart::load_chart_file(chart_path).map_err(|e| e.to_string())?;
            let issues = chart.validate();
            if let Some(worst) = issues
                .iter()
                .find(|i| i.severity == beatbyte_chart::Severity::Error)
            {
                return Err(format!("chart became invalid: {worst}"));
            }
            Ok(LoadedSong {
                chart,
                audio: SongAudio::File(audio_path.clone()),
            })
        }
    }
}

/// Keep rows and the detail block in sync with the cursor.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn refresh_browser(
    library: Res<SongLibrary>,
    view: Res<BrowserView>,
    cursor: Res<BrowserCursor>,
    selected: Res<SelectedDifficulty>,
    scores: Res<ScoreBoard>,
    mut rows: Query<(&SongRow, &mut BackgroundColor, &mut BorderColor)>,
    mut titles: Query<(&SongTitle, &mut TextColor), Without<SongArtist>>,
    mut artists: Query<(&SongArtist, &mut TextColor), Without<SongTitle>>,
    mut detail: Query<&mut Text, With<DetailText>>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = ui_kit::row_style(ui_kit::state_for(row.0 == cursor.0, false));
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (title, mut color) in &mut titles {
        color.0 = ui_kit::row_style(ui_kit::state_for(title.0 == cursor.0, false)).label;
    }
    for (artist, mut color) in &mut artists {
        color.0 = ui_kit::row_style(ui_kit::state_for(artist.0 == cursor.0, false)).value;
    }
    let Some(entry) = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i))
    else {
        return;
    };
    if let Ok(mut text) = detail.single_mut() {
        let duration = entry.duration_s.map_or_else(String::new, |d| {
            format!("  {}:{:02}", d as u32 / 60, d as u32 % 60)
        });
        let best = scores
            .best(&entry.title, &entry.artist, selected.0)
            .map_or_else(
                || "no record yet".to_owned(),
                |b| format!("best {}  ({:.1}%)", b.score, b.accuracy * 100.0),
            );
        // Where you are in the list, and how long it is. With the
        // rows now clipped to a window, nothing else says whether
        // three songs follow or thirty.
        // Position counts the VIEW - under a filter, "3/7" answers
        // "of the matches", which is the question being asked.
        let rating = entry
            .rating(selected.0)
            .map_or_else(|| "-".to_owned(), |r| "*".repeat(usize::from(r)));
        let notes = entry
            .note_count(selected.0)
            .map_or_else(|| "-".to_owned(), |n| n.to_string());
        let line = format!(
            "{}/{}   {:.0} BPM{duration}   <{}>   {rating}   {notes} notes   {best}",
            cursor.0 + 1,
            view.order.len(),
            entry.bpm,
            selected.0.display_name().to_uppercase()
        );
        if text.0 != line {
            text.0 = line;
        }
    }
}

/// The import hint / status line.
#[derive(Component)]
struct ImportNote;

/// A finished import replaces the [`SongLibrary`] resource — rebuild
/// the list so the new song is visible, and keep the note line
/// showing the import's progress.
fn rebuild_after_import(
    status: Res<crate::import::ImportStatus>,
    mut notes: Query<&mut Text, With<ImportNote>>,
) {
    // Library changes rebuild through `sync_view`; this system keeps
    // only the import status line fresh.
    if status.is_changed()
        && !status.0.is_empty()
        && let Ok(mut text) = notes.single_mut()
    {
        text.0.clone_from(&status.0);
    }
}

/// Rebuild the visible list whenever what it shows changed: the sort,
/// the filter, the library (import/delete), or the selected
/// difficulty (the notes/rating/best columns follow it). The ONE
/// rebuild path — and it keeps the cursor on the same SONG across the
/// rebuild, because a sort change that teleports the selection reads
/// as a glitch.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn sync_view(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut view: ResMut<BrowserView>,
    scores: Res<ScoreBoard>,
    selected: Res<SelectedDifficulty>,
    screens: Query<Entity, With<BrowserScreen>>,
) {
    let dirty = (view.is_changed() && !view.is_added())
        || (library.is_changed() && !library.is_added())
        || (selected.is_changed() && !selected.is_added());
    if !dirty {
        return;
    }
    let difficulty = selected.0;
    let order = build_order(&library.entries, view.sort, &view.filter, |entry| {
        scores
            .best(&entry.title, &entry.artist, difficulty)
            .map(|b| b.score)
    });
    let raw = view.bypass_change_detection();
    cursor.0 = stable_cursor(&raw.order, cursor.0, &order);
    raw.order = order;
    for entity in &screens {
        commands.entity(entity).despawn();
    }
    spawn_browser_impl(&mut commands, &font, &library, raw, &scores, difficulty);
}

fn despawn_browser(mut commands: Commands, entities: Query<Entity, With<BrowserScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

/// Keep the selected row inside the viewport.
///
/// The row height is MEASURED rather than assumed: it comes from the
/// font size and the row padding, and hard-coding it here would put
/// this system and `ui_kit` quietly out of step the first time either
/// changed.
fn follow_selection(
    cursor: Res<BrowserCursor>,
    view: Res<BrowserView>,
    rows: Query<(&SongRow, &ComputedNode)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition, &mut Node), With<SongList>>,
) {
    let Ok((list, mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    // Every row is the same height, so any of them answers the
    // question - but a row may not have been laid out yet on the
    // first frame, and a height of zero would send the offset to
    // infinity.
    let Some(row_h) = rows
        .iter()
        .map(|(_, node)| node.size().y)
        .find(|height| *height > 0.0)
    else {
        return;
    };
    let pitch = row_h + ui_kit::ROW_GAP;
    // Snap the window to whole rows, so the bottom one is not sliced
    // through the middle of its letters.
    if let Some(height) = ui_kit::whole_rows_height(
        row_h,
        ui_kit::ROW_GAP,
        view.order.len(),
        ui_kit::PANEL_MAX_H,
    ) {
        let wanted = bevy::ui::px(height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
    }
    let count = view.order.len() as f32;
    // The gaps sit BETWEEN rows, so there is one fewer of them.
    let content_h = count.mul_add(row_h, (count - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = list.size().y - 2.0 * ui_kit::PANEL_PAD;
    let row_top = cursor.0 as f32 * pitch;
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use crate::library::{SongEntry, SongSource};

    fn entry(title: &str, artist: &str, genre: Option<&str>, len: f64) -> SongEntry {
        SongEntry {
            title: title.to_owned(),
            artist: artist.to_owned(),
            bpm: 120.0,
            duration_s: Some(len),
            difficulties: vec![Difficulty::Medium],
            note_counts: vec![100],
            genre: genre.map(str::to_owned),
            source: SongSource::Builtin(0),
        }
    }

    fn lib() -> Vec<SongEntry> {
        vec![
            entry("Maria", "Blondie", Some("New Wave"), 248.0),
            entry("Africa", "Toto", Some("Rock"), 271.0),
            entry("Ella, elle l'a", "France Gall", None, 250.0),
            entry("Life", "Des'ree", Some("Pop"), 200.0),
        ]
    }

    #[test]
    fn standard_order_is_the_library_order() {
        // The order the browser has always shown - and what the
        // delete harness navigates by real keypresses. Changing the
        // default would silently retarget its arrows.
        let order = build_order(&lib(), SortMode::Standard, "", |_| None);
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn title_and_artist_sort_alphabetically() {
        let order = build_order(&lib(), SortMode::Title, "", |_| None);
        assert_eq!(order, vec![1, 2, 3, 0], "Africa, Ella, Life, Maria");
        let order = build_order(&lib(), SortMode::Artist, "", |_| None);
        assert_eq!(order, vec![0, 3, 2, 1], "Blondie, Des'ree, France, Toto");
    }

    #[test]
    fn missing_genres_sort_last_not_first() {
        // An absent genre is an absence, not the alphabet's start.
        let order = build_order(&lib(), SortMode::Genre, "", |_| None);
        assert_eq!(
            *order.last().expect("non-empty"),
            2,
            "the untagged song is last"
        );
    }

    #[test]
    fn best_sorts_highest_first_and_unplayed_last() {
        let order = build_order(&lib(), SortMode::Best, "", |entry| {
            match entry.title.as_str() {
                "Maria" => Some(139_968),
                "Africa" => Some(87_000),
                _ => None,
            }
        });
        assert_eq!(order[0], 0, "highest score first");
        assert_eq!(order[1], 1);
        assert!(
            order[2..].contains(&2) && order[2..].contains(&3),
            "no record sorts last"
        );
    }

    #[test]
    fn the_filter_folds_case_and_diacritics() {
        // "ella" must find "Ella, elle l'a" - and so must "élla" the
        // other way round: the fold applies to BOTH sides.
        let order = build_order(&lib(), SortMode::Standard, "ELLA", |_| None);
        assert_eq!(order, vec![2]);
        let order = build_order(&lib(), SortMode::Standard, "élla", |_| None);
        assert_eq!(order, vec![2]);
        // And it searches the artist and genre columns too.
        let order = build_order(&lib(), SortMode::Standard, "toto", |_| None);
        assert_eq!(order, vec![1]);
        let order = build_order(&lib(), SortMode::Standard, "new wave", |_| None);
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn an_empty_filter_shows_everything() {
        assert_eq!(
            build_order(&lib(), SortMode::Standard, "", |_| None).len(),
            4
        );
    }

    #[test]
    fn a_hopeless_filter_yields_an_empty_view_not_a_panic() {
        assert!(build_order(&lib(), SortMode::Standard, "zzzz", |_| None).is_empty());
    }

    #[test]
    fn the_cursor_follows_its_song_through_a_sort_change() {
        // Standard order, cursor on "Ella" (position 2). After
        // sorting by title, Ella sits at position 1 - and that is
        // where the cursor must be, not still at raw position 2
        // (which would now be "Life").
        let old = build_order(&lib(), SortMode::Standard, "", |_| None);
        let new = build_order(&lib(), SortMode::Title, "", |_| None);
        assert_eq!(stable_cursor(&old, 2, &new), 1);
    }

    #[test]
    fn a_cursor_whose_song_was_filtered_away_clamps() {
        let old = build_order(&lib(), SortMode::Standard, "", |_| None);
        let new = build_order(&lib(), SortMode::Standard, "maria", |_| None);
        // Cursor was on "Life" (3); Maria-only view has one row.
        assert_eq!(stable_cursor(&old, 3, &new), 0);
        // And an empty view clamps to zero without panicking.
        assert_eq!(stable_cursor(&old, 3, &[]), 0);
    }

    #[test]
    fn the_sort_cycle_visits_every_mode_and_returns() {
        let mut mode = SortMode::Standard;
        let mut seen = vec![mode];
        for _ in 0..5 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(mode.next(), SortMode::Standard, "the cycle closes");
        seen.sort_by_key(|m| m.label());
        seen.dedup();
        assert_eq!(seen.len(), 6, "every mode is reachable");
    }

    #[test]
    fn clipping_marks_the_cut() {
        assert_eq!(clip_chars("short", 10), "short");
        assert_eq!(clip_chars("exactlyten", 10), "exactlyten");
        assert_eq!(clip_chars("elevenchars", 10), "elevencha~");
    }
}
