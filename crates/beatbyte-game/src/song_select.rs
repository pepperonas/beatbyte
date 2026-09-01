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
    /// Most notes first (of the selected difficulty).
    Notes,
    /// Highest challenge rating first.
    Diff,
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
            SortMode::Length => SortMode::Notes,
            SortMode::Notes => SortMode::Diff,
            SortMode::Diff => SortMode::Best,
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
            SortMode::Notes => "NOTES",
            SortMode::Diff => "DIFF",
        }
    }
}

impl SortMode {
    /// Parse a persisted label (the inverse of [`SortMode::label`],
    /// case-insensitive). `None` for anything unknown, so a mangled
    /// settings file falls back instead of panicking.
    #[must_use]
    pub fn from_label(label: &str) -> Option<SortMode> {
        match label.to_lowercase().as_str() {
            "standard" => Some(SortMode::Standard),
            "title" => Some(SortMode::Title),
            "artist" => Some(SortMode::Artist),
            "genre" => Some(SortMode::Genre),
            "length" => Some(SortMode::Length),
            "best" => Some(SortMode::Best),
            "notes" => Some(SortMode::Notes),
            "diff" => Some(SortMode::Diff),
            _ => None,
        }
    }
}

/// What a click on a column header does: a new column sorts by it in
/// its default direction, the ACTIVE column flips the direction —
/// the convention of every library UI. Standard has no direction to
/// flip; clicking its concept (there is no Standard header) cannot
/// happen, but the function stays total.
#[must_use]
pub fn sort_click(current: SortMode, flipped: bool, clicked: SortMode) -> (SortMode, bool) {
    if clicked == current && clicked != SortMode::Standard {
        (current, !flipped)
    } else {
        (clicked, false)
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
    /// Whether the sort runs against its default direction.
    pub flipped: bool,
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
    flipped: bool,
    difficulty: Difficulty,
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
        SortMode::Notes => {
            order.sort_by_key(|i| {
                let notes = entries[*i].note_count(difficulty);
                (
                    notes.is_none(),
                    std::cmp::Reverse(notes.unwrap_or(0)),
                    tie(i),
                )
            });
        }
        SortMode::Diff => {
            order.sort_by_key(|i| {
                let rating = entries[*i].rating(difficulty);
                (
                    rating.is_none(),
                    std::cmp::Reverse(rating.unwrap_or(0)),
                    // Same rating: the denser chart is the harder one.
                    std::cmp::Reverse(entries[*i].note_count(difficulty).unwrap_or(0)),
                    tie(i),
                )
            });
        }
    }
    // Against the grain: the flip reverses every mode's default
    // direction. Standard is the library's own order and keeps it -
    // there is no "reverse standard" a player would ask for by name.
    if flipped && sort != SortMode::Standard {
        order.reverse();
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

/// The status line's text for the current view state.
fn status_text(view: &BrowserView) -> String {
    let direction = if view.flipped { " (reversed)" } else { "" };
    if view.searching {
        format!(
            "SEARCH: {}_   ({} match{})   ESC to close",
            view.filter,
            view.order.len(),
            if view.order.len() == 1 { "" } else { "es" }
        )
    } else if view.filter.is_empty() {
        format!("sort {}{direction}   F to search", view.sort.label())
    } else {
        format!(
            "sort {}{direction}   filter: {} ({} match{})",
            view.sort.label(),
            view.filter,
            view.order.len(),
            if view.order.len() == 1 { "" } else { "es" }
        )
    }
}

/// A caption's label: the active column carries the direction marker.
fn caption_label(text: &str, mode: SortMode, view: &BrowserView) -> String {
    if view.sort == mode {
        format!("{text} {}", if view.flipped { "^" } else { "v" })
    } else {
        text.to_owned()
    }
}

/// Where the cursor goes after the view rebuilt: typing a filter
/// selects the FIRST match (the search expectation: type, Enter,
/// play), any other change keeps the cursor on its song.
fn cursor_after_change(
    filter_changed: bool,
    old_order: &[usize],
    cursor: usize,
    new_order: &[usize],
) -> usize {
    if filter_changed {
        0
    } else {
        stable_cursor(old_order, cursor, new_order)
    }
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
            .add_systems(Startup, load_browser_prefs)
            .add_systems(OnEnter(AppState::SongSelect), spawn_browser)
            .add_systems(
                Update,
                (
                    browser_input,
                    search_sort_input,
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

/// A clickable column caption. Carries the mode it sorts by.
#[derive(Component)]
struct SortHeader(SortMode);

/// The sort/search status line.
#[derive(Component)]
struct StatusLine;

/// The dimmed "no match" hint row inside an empty list.
#[derive(Component)]
struct EmptyHint;

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

fn spawn_browser(mut commands: Commands, font: Res<UiFont>, view: Res<BrowserView>) {
    spawn_shell(&mut commands, &font, &view);
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

fn spawn_shell(commands: &mut Commands, font: &UiFont, view: &BrowserView) {
    commands
        .spawn((BrowserScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, font, "SONG SELECT", "pick a track and a difficulty");
            // Sort / search status line.
            parent.spawn((
                StatusLine,
                Text::new(font.safe(&status_text(view))),
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
                    // Every caption is a BUTTON that sorts its
                    // column; the active one shows the direction and
                    // wears the accent - the sort must be visible
                    // where the data is, not only in a status line.
                    let caption = |head: &mut ChildSpawnerCommands,
                                   text: &str,
                                   mode: SortMode,
                                   width: Option<f32>| {
                        let active = view.sort == mode;
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
                        let label = caption_label(text, mode, view);
                        head.spawn((
                            SortHeader(mode),
                            Button,
                            Text::new(label),
                            font.text(ui_kit::SMALL),
                            TextColor(if active {
                                palette::BRAND
                            } else {
                                palette::dimmed(palette::TEXT_DIM, 0.7)
                            }),
                            node,
                        ));
                    };
                    caption(head, "TITLE", SortMode::Title, None);
                    caption(head, "ARTIST", SortMode::Artist, Some(COL_ARTIST));
                    caption(head, "GENRE", SortMode::Genre, Some(COL_GENRE));
                    caption(head, "LEN", SortMode::Length, Some(COL_LEN));
                    caption(head, "NOTES", SortMode::Notes, Some(COL_NOTES));
                    caption(head, "DIFF", SortMode::Diff, Some(COL_RATING));
                    caption(head, "BEST", SortMode::Best, Some(COL_BEST));
                });
            parent.spawn((SongList, ui_kit::scroll_panel(ui_kit::PANEL_WIDE)));
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
                "UP/DOWN song  LEFT/RIGHT difficulty  S sort  F search  ENTER rock  E edit  DEL delete  ESC back",
            );
        });
}

/// Sort and search input. Its own system: `browser_input` sits at
/// Bevy's parameter limit, and the two concerns share no state
/// beyond the view. Runs AFTER `browser_input` in the chain, so the
/// Esc that closes the search is not also read as "back to menu" -
/// `browser_input` still sees the searching flag of this frame.
fn search_sort_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut view: ResMut<BrowserView>,
    headers: Query<(&SortHeader, &Interaction), Changed<Interaction>>,
    mut settings: ResMut<crate::config::Settings>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    if view.searching {
        // Printable keys EDIT THE FILTER - every letter shortcut is
        // suppressed while searching (in `browser_input`, off this
        // same flag), or typing "elle" would open the editor and arm
        // a delete on the way.
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
            } else if event.logical_key == bevy::input::keyboard::Key::Backspace {
                // Handled here rather than via `just_pressed` so the
                // OS key repeat erases while held, like every text
                // field.
                view.filter.pop();
            }
        }
        // Esc leaves search AND clears it: the recoverable state is
        // "the whole list", not "a filter you can no longer see".
        if keys.just_pressed(KeyCode::Escape) {
            view.searching = false;
            view.filter.clear();
        }
        return;
    }
    // Search opens on F (a letter key sits in the same place on every
    // layout) or on a TYPED "/" - the logical character, because the
    // physical Slash KeyCode is a US-layout position: on QWERTZ that
    // key is "-", and "/" lives on Shift+7. The first wiring used the
    // KeyCode and search was simply unreachable from a German
    // keyboard.
    let mut open_search = keys.just_pressed(KeyCode::KeyF);
    for event in typed.read() {
        if event.state.is_pressed()
            && let bevy::input::keyboard::Key::Character(text) = &event.logical_key
            && text.as_str() == "/"
        {
            open_search = true;
        }
    }
    if open_search {
        view.searching = true;
        sounds.write(crate::sfx::UiSound::Confirm);
    }
    let mut sorted = false;
    if keys.just_pressed(KeyCode::KeyS) {
        let next = view.sort.next();
        view.sort = next;
        view.flipped = false;
        sorted = true;
    }
    // Column headers sort on click; clicking the active one flips
    // the direction - the convention of every library UI.
    for (header, interaction) in &headers {
        if *interaction == Interaction::Pressed {
            let (sort, flipped) = sort_click(view.sort, view.flipped, header.0);
            view.sort = sort;
            view.flipped = flipped;
            sorted = true;
        }
    }
    // A sort ACTION blips like every other menu key and persists.
    // Deliberately an explicit flag, not `view.is_changed()`: that
    // also sees this system's own filter edits, and a blip per typed
    // letter is noise, not feedback.
    if sorted {
        sounds.write(crate::sfx::UiSound::Toggle);
        settings.browser_sort = view.sort.label().to_lowercase();
        settings.browser_sort_reversed = view.flipped;
    }
}

/// The browser's pointer inputs, bundled: `browser_input` sits at
/// Bevy's parameter cap, and these three always travel together.
#[derive(bevy::ecs::system::SystemParam)]
struct PointerInput<'w, 's> {
    mouse: Res<'w, ButtonInput<MouseButton>>,
    wheel: MessageReader<'w, 's, bevy::input::mouse::MouseWheel>,
    moved: MessageReader<'w, 's, bevy::window::CursorMoved>,
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn browser_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    view: Res<BrowserView>,
    pads: Query<&Gamepad>,
    mut library: ResMut<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut selected: ResMut<SelectedDifficulty>,
    builtins: Res<BuiltinSongs>,
    mut next_state: ResMut<NextState<AppState>>,
    mut pointer_in: PointerInput,
    rows: Query<(&SongRow, &Interaction), Changed<Interaction>>,
    time: Res<Time>,
    mut status: ResMut<crate::import::ImportStatus>,
    mut delete_armed: Local<(Option<usize>, f32)>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = if view.searching {
        MenuNav::read_typing(&map, &keys, pads.iter())
    } else {
        MenuNav::read(&map, &keys, pads.iter())
    };
    let searching = view.searching;
    let back = (!searching && nav.back) || pointer_in.mouse.just_pressed(MouseButton::Right);
    let count = view.order.len();
    if count == 0 {
        if back {
            sounds.write(crate::sfx::UiSound::Back);
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
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    // Mouse: wheel scrolls the list; clicking a row selects it, and
    // clicking the already-selected row starts it.
    for event in pointer_in.wheel.read() {
        if event.y > 0.0 {
            cursor.0 = (cursor.0 + count - 1) % count;
        } else if event.y < 0.0 {
            cursor.0 = (cursor.0 + 1) % count;
        }
        if event.y != 0.0 {
            sounds.write(crate::sfx::UiSound::Navigate);
        }
    }
    // Hover selects, click starts — the same rule as every other
    // menu. This list used to need two clicks (one to select, one to
    // start) and ignored hover entirely.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    // Hover selects only on REAL mouse motion. A freshly rebuilt row
    // fires Hovered under a resting pointer, and before this gate a
    // typed letter could yank the selection to wherever the mouse
    // happened to lie. A click is always deliberate and always
    // counts.
    let mouse_moved = pointer_in.moved.read().next().is_some();
    if let Some(index) = pointer.hovered
        && (mouse_moved || pointer.clicked)
    {
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
    if (nav.left && position > 0) || (nav.right && position + 1 < offered.len()) {
        sounds.write(crate::sfx::UiSound::Slider);
    }
    if nav.left && position > 0 {
        selected.0 = offered[position - 1];
    }
    if nav.right && position + 1 < offered.len() {
        selected.0 = offered[position + 1];
    }

    if nav.confirm || clicked_selected {
        sounds.write(crate::sfx::UiSound::Confirm);
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
                // Armed by LIBRARY index: a re-sort between the two
                // presses moves positions, and the confirmation was
                // asked about a song, not a row number.
                let song_index = view.order.get(cursor.0).copied();
                if delete_armed.0.is_some() && delete_armed.0 == song_index {
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
                    *delete_armed = (song_index, 3.0);
                    status.0 = format!(
                        "delete \"{}\" and its files? press again to confirm",
                        entry.title
                    );
                }
            }
        }
    }
    if back {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::MainMenu);
    }
}

/// Fill the (already spawned) list with the view's rows. Rows are
/// the ONLY part of the screen that rebuilds — header, footer, panel
/// and scroll state stay alive, which is what stopped every keypress
/// from re-laying-out the whole screen (the "feels buggy" core).
#[allow(clippy::too_many_arguments)] // plain helper, one call site
fn spawn_rows_into(
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
                        font.safe(&clip_chars(entry.genre.as_deref().unwrap_or("-"), 11)),
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
                        entry
                            .rating(effective)
                            .map_or_else(|| "-".to_owned(), |r| "*".repeat(usize::from(r))),
                        COL_RATING,
                    );
                    cell(row, font, position, best, COL_BEST);
                });
        }

        // An empty result must SAY so; a bare empty panel reads as a
        // broken screen, not as a search with no matches.
        if view.order.is_empty() {
            panel.spawn((
                EmptyHint,
                Text::new(font.safe(&format!("no match for \"{}\"  -  ESC clears", view.filter))),
                font.text(ui_kit::ROW),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::all(px(12.0)),
                    ..default()
                },
            ));
        }
    });
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
/// Colour queries for the row texts, factored so clippy's type cap
/// and Bevy's disjointness proofs both hold.
type TitleColors<'w, 's> = Query<
    'w,
    's,
    (&'static SongTitle, &'static mut TextColor),
    (
        Without<SongArtist>,
        Without<SortHeader>,
        Without<StatusLine>,
    ),
>;
/// See [`TitleColors`].
type ArtistColors<'w, 's> = Query<
    'w,
    's,
    (&'static SongArtist, &'static mut TextColor),
    (Without<SongTitle>, Without<SortHeader>, Without<StatusLine>),
>;

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system
fn refresh_browser(
    library: Res<SongLibrary>,
    view: Res<BrowserView>,
    cursor: Res<BrowserCursor>,
    selected: Res<SelectedDifficulty>,
    scores: Res<ScoreBoard>,
    font: Res<UiFont>,
    mut rows: Query<(&SongRow, &mut BackgroundColor, &mut BorderColor)>,
    mut titles: TitleColors,
    mut artists: ArtistColors,
    mut texts: ParamSet<(
        Query<&'static mut Text, With<DetailText>>,
        Query<
            (&'static mut Text, &'static mut TextColor),
            (With<StatusLine>, Without<SongTitle>, Without<SongArtist>),
        >,
        Query<
            (
                &'static SortHeader,
                &'static mut Text,
                &'static mut TextColor,
            ),
            (Without<SongTitle>, Without<SongArtist>, Without<StatusLine>),
        >,
    )>,
) {
    // Status line and column captions update IN PLACE - text writes
    // only when the string actually changed, or every frame would
    // re-shape the glyphs.
    if let Ok((mut text, mut color)) = texts.p1().single_mut() {
        let wanted = font.safe(&status_text(&view));
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = if view.searching {
            palette::BRAND
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.85)
        };
    }
    for (header, mut text, mut color) in &mut texts.p2() {
        let base = match header.0 {
            SortMode::Title => "TITLE",
            SortMode::Artist => "ARTIST",
            SortMode::Genre => "GENRE",
            SortMode::Length => "LEN",
            SortMode::Notes => "NOTES",
            SortMode::Diff => "DIFF",
            SortMode::Best => "BEST",
            SortMode::Standard => "",
        };
        let wanted = caption_label(base, header.0, &view);
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = if view.sort == header.0 {
            palette::BRAND
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.7)
        };
    }
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
    if let Ok(mut text) = texts.p0().single_mut() {
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
/// Rebuild the visible ROWS whenever what they show changed: the
/// sort, the filter, the library (import/delete), or the selected
/// difficulty (the cells follow it). Everything else on the screen
/// updates in place and never respawns — a keypress must not
/// re-layout the world.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn sync_view(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut view: ResMut<BrowserView>,
    scores: Res<ScoreBoard>,
    selected: Res<SelectedDifficulty>,
    lists: Query<Entity, With<SongList>>,
    fresh: Query<(), Added<SongList>>,
    mut rendered: Local<Option<(Vec<usize>, Difficulty)>>,
    mut last_filter: Local<String>,
) {
    let entered = !fresh.is_empty();
    let dirty = entered
        || (view.is_changed() && !view.is_added())
        || (library.is_changed() && !library.is_added())
        || (selected.is_changed() && !selected.is_added());
    if !dirty {
        return;
    }
    let difficulty = selected.0;
    let order = build_order(
        &library.entries,
        view.sort,
        view.flipped,
        difficulty,
        &view.filter,
        |entry| {
            scores
                .best(&entry.title, &entry.artist, difficulty)
                .map(|b| b.score)
        },
    );
    let filter_changed = *last_filter != view.filter;
    last_filter.clone_from(&view.filter);
    let raw = view.bypass_change_detection();
    cursor.0 = cursor_after_change(filter_changed, &raw.order, cursor.0, &order);
    raw.order = order;
    // Rows rebuild only when their CONTENT changed — the order, or
    // the difficulty the cells follow. A pure status change (opening
    // the search) touches neither.
    let key = (raw.order.clone(), difficulty);
    if (entered || library.is_changed() || rendered.as_ref() != Some(&key))
        && let Ok(list) = lists.single()
    {
        spawn_rows_into(
            &mut commands,
            list,
            &font,
            &library,
            raw,
            &scores,
            difficulty,
        );
        *rendered = Some(key);
    }
}

/// Restore the persisted sort. The filter deliberately starts empty.
fn load_browser_prefs(settings: Res<crate::config::Settings>, mut view: ResMut<BrowserView>) {
    if let Some(sort) = SortMode::from_label(&settings.browser_sort) {
        view.sort = sort;
        view.flipped = settings.browser_sort_reversed && sort != SortMode::Standard;
    }
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
        let order = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn title_and_artist_sort_alphabetically() {
        let order = build_order(
            &lib(),
            SortMode::Title,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(order, vec![1, 2, 3, 0], "Africa, Ella, Life, Maria");
        let order = build_order(
            &lib(),
            SortMode::Artist,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(order, vec![0, 3, 2, 1], "Blondie, Des'ree, France, Toto");
    }

    #[test]
    fn missing_genres_sort_last_not_first() {
        // An absent genre is an absence, not the alphabet's start.
        let order = build_order(
            &lib(),
            SortMode::Genre,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(
            *order.last().expect("non-empty"),
            2,
            "the untagged song is last"
        );
    }

    #[test]
    fn best_sorts_highest_first_and_unplayed_last() {
        let order = build_order(
            &lib(),
            SortMode::Best,
            false,
            Difficulty::Medium,
            "",
            |entry| match entry.title.as_str() {
                "Maria" => Some(139_968),
                "Africa" => Some(87_000),
                _ => None,
            },
        );
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
        let order = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "ELLA",
            |_| None,
        );
        assert_eq!(order, vec![2]);
        let order = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "élla",
            |_| None,
        );
        assert_eq!(order, vec![2]);
        // And it searches the artist and genre columns too.
        let order = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "toto",
            |_| None,
        );
        assert_eq!(order, vec![1]);
        let order = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "new wave",
            |_| None,
        );
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn an_empty_filter_shows_everything() {
        assert_eq!(
            build_order(
                &lib(),
                SortMode::Standard,
                false,
                Difficulty::Medium,
                "",
                |_| None
            )
            .len(),
            4
        );
    }

    #[test]
    fn a_hopeless_filter_yields_an_empty_view_not_a_panic() {
        assert!(
            build_order(
                &lib(),
                SortMode::Standard,
                false,
                Difficulty::Medium,
                "zzzz",
                |_| None
            )
            .is_empty()
        );
    }

    #[test]
    fn the_cursor_follows_its_song_through_a_sort_change() {
        // Standard order, cursor on "Ella" (position 2). After
        // sorting by title, Ella sits at position 1 - and that is
        // where the cursor must be, not still at raw position 2
        // (which would now be "Life").
        let old = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        let new = build_order(
            &lib(),
            SortMode::Title,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(stable_cursor(&old, 2, &new), 1);
    }

    #[test]
    fn a_cursor_whose_song_was_filtered_away_clamps() {
        let old = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        let new = build_order(
            &lib(),
            SortMode::Standard,
            false,
            Difficulty::Medium,
            "maria",
            |_| None,
        );
        // Cursor was on "Life" (3); Maria-only view has one row.
        assert_eq!(stable_cursor(&old, 3, &new), 0);
        // And an empty view clamps to zero without panicking.
        assert_eq!(stable_cursor(&old, 3, &[]), 0);
    }

    #[test]
    fn the_sort_cycle_visits_every_mode_and_returns() {
        let mut mode = SortMode::Standard;
        let mut seen = vec![mode];
        for _ in 0..7 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(mode.next(), SortMode::Standard, "the cycle closes");
        seen.sort_by_key(|m| m.label());
        seen.dedup();
        assert_eq!(seen.len(), 8, "every mode is reachable");
    }

    #[test]
    fn the_flip_reverses_every_mode_but_standard() {
        let forward = build_order(
            &lib(),
            SortMode::Title,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        let reversed = build_order(
            &lib(),
            SortMode::Title,
            true,
            Difficulty::Medium,
            "",
            |_| None,
        );
        let mut expected = forward.clone();
        expected.reverse();
        assert_eq!(reversed, expected, "flipped title = reversed title");
        // Standard is the library's own order and has no reverse a
        // player would ask for by name.
        let standard = build_order(
            &lib(),
            SortMode::Standard,
            true,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(standard, vec![0, 1, 2, 3], "standard ignores the flip");
    }

    #[test]
    fn a_header_click_sorts_then_flips_then_a_new_column_resets() {
        // The convention of every library UI, as one pure function.
        assert_eq!(
            sort_click(SortMode::Standard, false, SortMode::Artist),
            (SortMode::Artist, false),
            "a new column sorts in its default direction"
        );
        assert_eq!(
            sort_click(SortMode::Artist, false, SortMode::Artist),
            (SortMode::Artist, true),
            "the active column flips"
        );
        assert_eq!(
            sort_click(SortMode::Artist, true, SortMode::Artist),
            (SortMode::Artist, false),
            "and flips back"
        );
        assert_eq!(
            sort_click(SortMode::Artist, true, SortMode::Best),
            (SortMode::Best, false),
            "a new column drops the old direction"
        );
    }

    #[test]
    fn notes_and_diff_sort_densest_first() {
        let mut entries = lib();
        entries[0].note_counts = vec![500];
        entries[1].note_counts = vec![100];
        entries[2].note_counts = vec![300];
        entries[3].note_counts = vec![900];
        let order = build_order(
            &entries,
            SortMode::Notes,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(order, vec![3, 0, 2, 1], "most notes first");
        // A song without the selected difficulty sorts last, not
        // first with a phantom zero.
        entries[3].difficulties = vec![];
        entries[3].note_counts = vec![];
        let order = build_order(
            &entries,
            SortMode::Notes,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        assert_eq!(*order.last().expect("non-empty"), 3);
    }

    #[test]
    fn typing_a_filter_selects_the_first_match() {
        // The search expectation: type, Enter, play. Any non-filter
        // change keeps the cursor glued to its song instead.
        // Cursor on song 2, which survives the narrowing at
        // position 1 — so "first match" (0) and "follow the song"
        // (1) give DIFFERENT answers and the branch is really pinned.
        let old = vec![3, 1, 4, 2];
        let narrowed = vec![4, 2];
        assert_eq!(
            cursor_after_change(true, &old, 3, &narrowed),
            0,
            "a filter change selects the first match"
        );
        assert_eq!(
            cursor_after_change(false, &old, 3, &narrowed),
            1,
            "a non-filter change follows the song"
        );
        let resorted = vec![2, 4, 1, 3];
        assert_eq!(
            cursor_after_change(false, &old, 2, &resorted),
            1,
            "a sort change follows the song"
        );
    }

    #[test]
    fn every_sort_label_round_trips_through_persistence() {
        // The label is what settings.json stores; a mode whose label
        // does not parse back would silently reset the sort on the
        // next launch.
        let mut mode = SortMode::Standard;
        loop {
            assert_eq!(
                SortMode::from_label(mode.label()),
                Some(mode),
                "label {} must round-trip",
                mode.label()
            );
            mode = mode.next();
            if mode == SortMode::Standard {
                break;
            }
        }
        assert_eq!(SortMode::from_label("TITLE"), Some(SortMode::Title));
        assert_eq!(SortMode::from_label("garbage"), None);
        assert_eq!(SortMode::from_label(""), None);
    }

    #[test]
    fn clipping_marks_the_cut() {
        assert_eq!(clip_chars("short", 10), "short");
        assert_eq!(clip_chars("exactlyten", 10), "exactlyten");
        assert_eq!(clip_chars("elevenchars", 10), "elevencha~");
    }
}
