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
    /// Active search text, as typed. Case and diacritics are folded
    /// when the filter is APPLIED (`build_order`), never on the way
    /// in: the field shows what the player wrote, and Backspace
    /// removes exactly the character they typed — folding first
    /// turned some letters into two code points and left a stray
    /// half behind after one Backspace.
    pub filter: String,
    /// Whether typing currently goes into the filter.
    pub searching: bool,
    /// Whether the sort runs against its default direction.
    pub flipped: bool,
}

/// How long the letter q has to be held to leave the search.
pub const HOLD_S: f32 = 1.0;

/// The verdict of one frame on a pending q.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoldVerdict {
    /// No q is pending.
    Idle,
    /// Still held; the fraction of [`HOLD_S`] elapsed.
    Holding(f32),
    /// Released before the second was up: it was this letter.
    TypeQ(char),
    /// Held the full second: leave the search.
    Close,
}

/// A press of the letter q while searching, waiting to learn whether
/// it was a letter or a gesture.
///
/// q is both: tapped, it is a character like any other (a search for
/// "Queen" must work); held for [`HOLD_S`], it leaves the search. The
/// letter is therefore written on RELEASE, not on press — and the
/// moment any other character arrives, the pending q is written first,
/// so rolling from q into the next key ("qu…" with q still down) keeps
/// its order. Pure, so the timing is tested without a keyboard.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct QuitHold {
    /// The physical key the pending q came in on (release is tracked
    /// on the physical key, whatever the layout calls it), the letter
    /// as typed (q or Q), and how long the key has been down.
    pending: Option<(KeyCode, char, f32)>,
}

impl QuitHold {
    /// A fresh press of `letter` (q or Q) on this physical key.
    pub fn begin(&mut self, key: KeyCode, letter: char) {
        self.pending = Some((key, letter, 0.0));
    }

    /// The physical key a pending q sits on, if any — its OS key
    /// repeats are not letters.
    #[must_use]
    pub fn key(&self) -> Option<KeyCode> {
        self.pending.map(|(key, _, _)| key)
    }

    /// Another character arrived: the pending q, if any, is a letter
    /// and must be written before it. Returns that letter.
    pub fn flush(&mut self) -> Option<char> {
        self.pending.take().map(|(_, letter, _)| letter)
    }

    /// Forget any pending q without writing it (the search closed
    /// some other way).
    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// One frame. `released` is whether the key's own release event
    /// arrived; `still_down` is the physical key's state now. A key
    /// that is up WITHOUT having been released was taken away — the
    /// window lost focus and every key was let go at once — and that
    /// is neither a tap nor a hold: the pending q is dropped silently
    /// (seen: a Cmd-Tab mid-hold left a "q" in the field).
    pub fn tick(&mut self, still_down: bool, released: bool, dt: f32) -> HoldVerdict {
        let Some((_, letter, held)) = self.pending.as_mut() else {
            return HoldVerdict::Idle;
        };
        if released {
            let letter = *letter;
            self.pending = None;
            return HoldVerdict::TypeQ(letter);
        }
        if !still_down {
            self.pending = None;
            return HoldVerdict::Idle;
        }
        *held += dt;
        if *held >= HOLD_S {
            self.pending = None;
            return HoldVerdict::Close;
        }
        HoldVerdict::Holding(*held / HOLD_S)
    }

    /// The fill of the hold bar, while a q is held.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        self.pending
            .map(|(_, _, held)| (held / HOLD_S).clamp(0.0, 1.0))
    }
}

/// Whether a typed string is the letter q, in either case.
fn is_q(text: &str) -> bool {
    text.eq_ignore_ascii_case("q")
}

/// Case- and diacritic-insensitive key for SORTING: `fold_latin` is
/// what puts "Sacré" beside "Sacre". Matching lives in
/// [`crate::search`], which also drops apostrophes and punctuation.
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
    // The filter, fuzzily: every entry gets a score or is out, and
    // the survivors are RANKED by it below, after the sort — so the
    // song the player meant sits first and the chosen sort only
    // breaks ties. See `crate::search` for the rules.
    let query = crate::search::words(filter);
    let scored: Vec<(usize, u32)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            crate::search::Haystack::new(&entry.title, &entry.artist, entry.genre.as_deref())
                .score(&query)
                .map(|score| (i, score))
        })
        .collect();
    let mut order: Vec<usize> = scored.iter().map(|(i, _)| *i).collect();
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
    if !query.is_empty() {
        // Stable: equal scores keep the sort's order.
        let score_of = |i: &usize| scored.iter().find(|(j, _)| j == i).map_or(0, |(_, s)| *s);
        order.sort_by_key(|i| std::cmp::Reverse(score_of(i)));
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
            "SEARCH: {}_   ({} match{}, best first)   hold Q keeps it  ESC clears",
            view.filter,
            view.order.len(),
            if view.order.len() == 1 { "" } else { "es" }
        )
    } else if view.filter.is_empty() {
        format!("sort {}{direction}   F to search", view.sort.label())
    } else {
        format!(
            "sort {}{direction}   filter: {} ({} match{}, best first)   F edits  ESC clears",
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

/// What the rows are built FROM, for deciding whether to rebuild
/// them: the order and the difficulty — and, while the list is empty,
/// the filter, because the only row then is the "no match for …"
/// hint and it quotes the filter. Without that third part the hint
/// showed the first letter that emptied the list ("q") for the rest
/// of the word ("queen"): an empty order equals an empty order.
fn rebuild_key(
    order: &[usize],
    difficulty: Difficulty,
    filter: &str,
) -> (Vec<usize>, Difficulty, String) {
    let quoted = if order.is_empty() {
        filter.to_owned()
    } else {
        String::new()
    };
    (order.to_vec(), difficulty, quoted)
}

/// The line that stands in for the list when it has no rows. Two
/// different silences, two different sentences: a library with songs
/// in it whose filter matched nothing can be cleared with ESC, while
/// an empty library needs a song — telling the latter "no match for
/// """ would be both false and useless. Pure — tested.
#[must_use]
pub fn empty_hint(library_len: usize, filter: &str) -> String {
    if library_len == 0 {
        "no songs yet  -  drag an audio file onto the window".to_owned()
    } else {
        format!("no match for \"{filter}\"  -  ESC clears")
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
        app.init_resource::<LyricsLookup>()
            .init_resource::<SelectedDifficulty>()
            .init_resource::<BrowserCursor>()
            .init_resource::<crate::mc::McQueue>()
            .init_resource::<BrowserView>()
            .init_resource::<QuitHold>()
            .init_resource::<crate::preview::SongPreview>()
            .add_systems(Startup, load_browser_prefs)
            .add_systems(OnEnter(AppState::SongSelect), spawn_browser)
            .add_systems(
                Update,
                (
                    browser_input,
                    poll_lyrics_lookup,
                    search_sort_input,
                    drive_hold_bar,
                    sync_view,
                    // After `sync_view`: the cursor and the order are
                    // settled for this frame, so a filter that moves
                    // the selection is heard as a move, not a start.
                    crate::preview::drive_preview,
                    refresh_browser,
                    rebuild_after_import,
                    follow_selection,
                )
                    .chain()
                    .run_if(in_state(AppState::SongSelect)),
            )
            .add_systems(
                OnExit(AppState::SongSelect),
                (despawn_browser, crate::preview::stop_preview),
            );
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

fn spawn_browser(
    mut commands: Commands,
    font: Res<UiFont>,
    mut view: ResMut<BrowserView>,
    mut hold: ResMut<QuitHold>,
) {
    // A q held when the screen was left is not held now — and the
    // search is not open either: coming back from a song into a
    // field that swallows every letter (S, E, L, Q, P all "dead")
    // read as a broken screen. The FILTER stays, so the next song
    // is still a match away; F reopens the field, Esc clears it.
    hold.clear();
    view.searching = false;
    spawn_shell(&mut commands, &font, &view);
    spawn_hold_bar(&mut commands, &font);
}

/// The hold bar's root: a full-window layer that centres its panel.
#[derive(Component)]
struct HoldBarRoot;

/// The bar's fill, whose width is the hold's progress.
#[derive(Component)]
struct HoldBarFill;

/// The (hidden) "leaving search" bar. Its own layer over the browser,
/// centred in the window, above everything else on the screen; it
/// shows only while q is held in the search.
fn spawn_hold_bar(commands: &mut Commands, font: &UiFont) {
    commands
        .spawn((
            BrowserScreen,
            HoldBarRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            // A scrim: the list dims behind the panel, so the panel
            // reads as the one thing on screen.
            BackgroundColor(palette::BACKGROUND.with_alpha(0.6)),
            Pickable::IGNORE,
            GlobalZIndex(40),
            Visibility::Hidden,
        ))
        .with_children(|layer| {
            layer
                .spawn(ui_kit::panel_centered())
                // The kit's frame is translucent by design — it sits
                // on the page ground. Over a list, the rows showed
                // through the bar (seen on screen); an overlay's
                // plate is opaque. FULLY opaque: Bevy blends alpha in
                // linear light, and at 0.96 the bright row text still
                // came through as legible grey (seen too).
                .insert(BackgroundColor(palette::SURFACE))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("LEAVING SEARCH"),
                        font.text(ui_kit::ROW),
                        TextColor(palette::BRAND),
                    ));
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(10),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(palette::TEXT.with_alpha(ui_kit::FILL_ALPHA)),
                        ))
                        .with_children(|bar| {
                            bar.spawn((
                                HoldBarFill,
                                Node {
                                    width: percent(0),
                                    height: percent(100),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(palette::BRAND),
                            ));
                        });
                    panel.spawn((
                        Text::new("keep holding Q to leave, release to type it"),
                        ui_kit::subtitle_text(font),
                    ));
                });
        });
}

/// Show the bar while a q is held, filled to the hold's progress.
fn drive_hold_bar(
    hold: Res<QuitHold>,
    mut roots: Query<&mut Visibility, With<HoldBarRoot>>,
    mut fills: Query<&mut Node, With<HoldBarFill>>,
) {
    let Ok(mut visibility) = roots.single_mut() else {
        return;
    };
    match hold.progress() {
        Some(progress) => {
            *visibility = Visibility::Visible;
            if let Ok(mut node) = fills.single_mut() {
                node.width = percent(progress * 100.0);
            }
        }
        None => *visibility = Visibility::Hidden,
    }
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
            crate::prompts::device_footer(
                parent,
                font,
                "UP/DOWN song  LEFT/RIGHT difficulty  S sort  F search  ENTER rock  L lyrics  K align  Q queue MC set  P play set  E edit  DEL delete  ESC back",
                "D-PAD song and difficulty  SOUTH rock  EAST back",
            );
            ui_kit::back_button(parent, font, "MAIN MENU");
        });
}

/// Sort and search input. Its own system: `browser_input` sits at
/// Bevy's parameter limit, and the two concerns share no state
/// beyond the view. Runs AFTER `browser_input` in the chain, so the
/// Esc that closes the search is not also read as "back to menu" -
/// `browser_input` still sees the searching flag of this frame.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn search_sort_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut view: ResMut<BrowserView>,
    mut hold: ResMut<QuitHold>,
    headers: Query<(&SortHeader, &Interaction), Changed<Interaction>>,
    mut settings: ResMut<crate::config::Settings>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    if view.searching {
        // Printable keys EDIT THE FILTER - every letter shortcut is
        // suppressed while searching (in `browser_input`, off this
        // same flag), or typing "elle" would open the editor and arm
        // a delete on the way.
        //
        // The letter q is the one exception, and only in TIMING: a
        // tap is the letter, a hold of `HOLD_S` leaves the search (and
        // keeps the filter; Esc below leaves AND clears). The first
        // wiring made q leave on the tap, which made every title and
        // artist beginning with q unsearchable — the reported bug.
        let mut released = false;
        for event in typed.read() {
            if !event.state.is_pressed() {
                if hold.key() == Some(event.key_code) {
                    released = true;
                }
                continue;
            }
            match &event.logical_key {
                bevy::input::keyboard::Key::Character(text) if is_q(text) => {
                    // The OS repeats a held key; those repeats are
                    // the same gesture, not more letters.
                    if event.repeat && hold.key() == Some(event.key_code) {
                        continue;
                    }
                    if let Some(letter) = hold.flush() {
                        view.filter.push(letter);
                    }
                    hold.begin(event.key_code, text.chars().next().unwrap_or('q'));
                    info!(
                        "search: q down on {:?} (repeat {})",
                        event.key_code, event.repeat
                    );
                }
                bevy::input::keyboard::Key::Character(text) => {
                    if let Some(letter) = hold.flush() {
                        view.filter.push(letter);
                    }
                    view.filter.extend(text.chars().filter(|c| !c.is_control()));
                }
                bevy::input::keyboard::Key::Space => {
                    if let Some(letter) = hold.flush() {
                        view.filter.push(letter);
                    }
                    view.filter.push(' ');
                }
                bevy::input::keyboard::Key::Backspace => {
                    // Handled here rather than via `just_pressed` so
                    // the OS key repeat erases while held, like every
                    // text field. A pending q is written first: the
                    // keys were "q, Backspace", and that is what they
                    // do.
                    if let Some(letter) = hold.flush() {
                        view.filter.push(letter);
                    }
                    view.filter.pop();
                }
                _ => {}
            }
        }
        let still_down = hold.key().is_some_and(|key| keys.pressed(key));
        match hold.tick(still_down, released, time.delta_secs()) {
            HoldVerdict::TypeQ(letter) => {
                info!("search: q released early -> typed '{letter}'");
                view.filter.push(letter);
            }
            HoldVerdict::Close => {
                info!("search: q held {HOLD_S}s -> search closed, filter kept");
                view.searching = false;
                sounds.write(crate::sfx::UiSound::Back);
                return;
            }
            HoldVerdict::Idle | HoldVerdict::Holding(_) => {}
        }
        // Esc leaves search AND clears it: the recoverable state is
        // "the whole list", not "a filter you can no longer see".
        if keys.just_pressed(KeyCode::Escape) {
            info!("search: Esc -> search closed, filter cleared");
            view.searching = false;
            view.filter.clear();
            hold.clear();
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
        info!("search: opened");
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

/// Song-starting dependencies, bundled for the same parameter-cap
/// reason: what a start needs (the built-ins) and what an MC set
/// adds (the queue).
#[derive(bevy::ecs::system::SystemParam)]
struct StartDeps<'w, 's> {
    /// The clickable way back, bundled here because the browser is
    /// at Bevy's sixteen-parameter cap.
    back_button: Query<
        'w,
        's,
        (
            &'static Interaction,
            &'static mut BackgroundColor,
            &'static mut BorderColor,
        ),
        With<ui_kit::BackButton>,
    >,
    builtins: Res<'w, BuiltinSongs>,
    mc_queue: ResMut<'w, crate::mc::McQueue>,
    /// The in-flight lyrics lookup — bundled here because Bevy caps
    /// a system at sixteen parameters and this one is at the line.
    lookup: ResMut<'w, LyricsLookup>,
    /// The aligner's state (`K` aligns the highlighted song).
    smart: ResMut<'w, crate::smart_lyrics::SmartLyrics>,
}

/// The in-flight lyrics lookup. One at a time: the browser is not a
/// place to start a dozen network calls by holding a key.
#[derive(Resource, Default)]
pub struct LyricsLookup {
    task: Option<LyricsTask>,
    /// The song the running lookup is for, for the status line.
    title: String,
}

/// The lookup's background task.
struct LyricsTask(bevy::tasks::Task<crate::lyrics_fetch::Outcome>);

/// Report a finished lookup. Every outcome writes a line — a lookup
/// never ends in silence.
fn poll_lyrics_lookup(
    mut lookup: ResMut<LyricsLookup>,
    mut status: ResMut<crate::import::ImportStatus>,
) {
    let Some(task) = lookup.task.as_mut() else {
        return;
    };
    let Some(outcome) =
        bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(&mut task.0))
    else {
        return;
    };
    status.0 = outcome.message(&lookup.title);
    lookup.task = None;
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn browser_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    mut view: ResMut<BrowserView>,
    pads: Query<&Gamepad>,
    mut library: ResMut<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut selected: ResMut<SelectedDifficulty>,
    mut start: StartDeps,
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
    let clicked_back = ui_kit::back_pressed(&mut start.back_button);
    // Esc with a filter still narrowing the list CLEARS it first and
    // leaves on the next press — the whole list is the state to
    // return to, and a filtered list with no field open had no key
    // that cleared it. The button and the right mouse button leave
    // straight away: they are pointed at the door, not at the list.
    if !searching && nav.back && !view.filter.is_empty() {
        view.filter.clear();
        sounds.write(crate::sfx::UiSound::Back);
        return;
    }
    let back = (!searching && (nav.back || clicked_back))
        || pointer_in.mouse.just_pressed(MouseButton::Right);
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
        match prepare_song(entry, &start.builtins) {
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
    // L looks the highlighted song's karaoke lyrics up in lrclib's
    // catalogue - the lookup inspector-rust has been running in its
    // Shazam mode. Deliberately a key press, not something that
    // happens on its own: it is the one moment BeatByte talks to the
    // network, and only the artist and the title leave the machine.
    if !searching && keys.just_pressed(KeyCode::KeyL) && start.lookup.task.is_none() {
        match &entry.source {
            SongSource::Builtin(_) => {
                status.0 = "built-in songs ship with their own lyrics".to_owned();
            }
            SongSource::File { audio_path, .. } => {
                let (artist, title) = (entry.artist.clone(), entry.title.clone());
                let audio = audio_path.clone();
                let shown = title.clone();
                start.lookup.task = Some(LyricsTask(
                    bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                        crate::lyrics_fetch::fetch_and_cache(&artist, &title, &audio)
                    }),
                ));
                sounds.write(crate::sfx::UiSound::Confirm);
                status.0 = format!("looking up lyrics for \"{shown}\"...");
                start.lookup.title = shown;
            }
        }
    }
    // K aligns the highlighted song's lyrics against its own audio
    // (plan L4b) - word and letter timing from the `.lrc` beside it,
    // with the model from the settings screen; K again cancels. Every
    // reason it cannot run is a line on the status row. (`A` would be
    // the natural key and is menu LEFT: it changes the difficulty.)
    if !searching && keys.just_pressed(KeyCode::KeyK) {
        if start.smart.is_aligning() {
            start.smart.cancel_align();
            status.0 = "cancelling the alignment...".to_owned();
        } else {
            let (is_file, audio) = match &entry.source {
                SongSource::Builtin(_) => (false, None),
                SongSource::File { audio_path, .. } => (true, Some(audio_path.as_path())),
            };
            match start.smart.start_align(&entry.title, is_file, audio) {
                Ok(()) => {
                    sounds.write(crate::sfx::UiSound::Confirm);
                    status.0 = format!("aligning \"{}\": starting", entry.title);
                }
                Err(reason) => {
                    sounds.write(crate::sfx::UiSound::Error);
                    status.0 = reason.message().to_owned();
                }
            }
        }
    }
    // Q queues the highlighted song for an MC set (again removes it);
    // P plays the queued set as one continuous DJ performance.
    if !searching && keys.just_pressed(KeyCode::KeyQ) {
        let song_index = view.order.get(cursor.0).copied();
        if let Some(song_index) = song_index {
            if let Some(at) = start.mc_queue.0.iter().position(|i| *i == song_index) {
                start.mc_queue.0.remove(at);
            } else {
                start.mc_queue.0.push(song_index);
            }
            sounds.write(crate::sfx::UiSound::Toggle);
            // The row list carries no queued-marker (rows rebuild on
            // view changes only); the status line names the action
            // and the count instead.
            let added = start.mc_queue.0.contains(&song_index);
            status.0 = format!(
                "{} \"{}\" - MC set: {} song(s), P plays it",
                if added { "queued" } else { "removed" },
                entry.title,
                start.mc_queue.0.len()
            );
        }
    }
    if !searching && keys.just_pressed(KeyCode::KeyP) && !start.mc_queue.0.is_empty() {
        let mut songs = Vec::new();
        for index in &start.mc_queue.0 {
            let Some(entry) = library.entries.get(*index) else {
                continue;
            };
            match prepare_song(entry, &start.builtins) {
                Ok(song) => songs.push(song),
                Err(reason) => {
                    error!("mc set: cannot load \"{}\": {reason}", entry.title);
                    status.0 = format!("MC set: cannot load \"{}\"", entry.title);
                    return;
                }
            }
        }
        let Some(first) = songs.first().cloned() else {
            return;
        };
        info!("mc set: starting with {} song(s)", songs.len());
        sounds.write(crate::sfx::UiSound::Confirm);
        commands.insert_resource(crate::mc::McSet { songs, position: 0 });
        commands.insert_resource(first);
        start.mc_queue.0.clear();
        next_state.set(AppState::Gameplay);
        return;
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
                            *library = crate::boot::scan_with_builtins(&start.builtins.0);
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
                    spawn_mic(row, entry.has_lyrics);
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
                Text::new(font.safe(&empty_hint(library.entries.len(), &view.filter))),
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
                lyrics: beatbyte_chart::lyrics::lyrics_beside(audio_path, chart_path),
                lyric_offset_ms: beatbyte_chart::lyrics::load_song_lyric_offset(audio_path),
            })
        }
    }
}

/// Width the microphone reserves, so titles line up whether a song
/// has lyrics or not.
const MIC_W: f32 = 14.0;

/// Draw the lyrics marker at the head of a row: a microphone, built
/// from nodes.
///
/// ⚠️ Not the 🎤 character. Press Start 2P has 656 glyphs and that
/// is not one of them — rendered, it comes out as the font's
/// `.notdef` box (verified by rendering it and comparing the bitmap
/// against a private-use codepoint). A box in every row would say
/// nothing at all, so the microphone is drawn: a capsule head, a
/// stem and a base.
///
/// A song WITHOUT lyrics keeps the same space empty, so the titles
/// stay on one left edge — a marker that shifts the whole column is
/// harder to scan than no marker.
fn spawn_mic(row: &mut ChildSpawnerCommands, has_lyrics: bool) {
    let mut slot = row.spawn(Node {
        width: px(MIC_W),
        height: px(14.0),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: px(1.0),
        ..default()
    });
    if !has_lyrics {
        return;
    }
    let tint = palette::dimmed(palette::BRAND, 0.85);
    slot.with_children(|mic| {
        // Head: a capsule.
        mic.spawn((
            Node {
                width: px(5.0),
                height: px(7.0),
                border_radius: BorderRadius::all(px(2.5)),
                ..default()
            },
            BackgroundColor(tint),
        ));
        // Stem.
        mic.spawn((
            Node {
                width: px(1.0),
                height: px(2.0),
                ..default()
            },
            BackgroundColor(tint),
        ));
        // Base.
        mic.spawn((
            Node {
                width: px(7.0),
                height: px(1.0),
                ..default()
            },
            BackgroundColor(tint),
        ));
    });
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
    settings: Res<crate::config::Settings>,
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
        let style = ui_kit::styled_row(
            ui_kit::state_for(row.0 == cursor.0, false),
            settings.high_contrast,
        );
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (title, mut color) in &mut titles {
        color.0 = ui_kit::styled_row(
            ui_kit::state_for(title.0 == cursor.0, false),
            settings.high_contrast,
        )
        .label;
    }
    for (artist, mut color) in &mut artists {
        color.0 = ui_kit::styled_row(
            ui_kit::state_for(artist.0 == cursor.0, false),
            settings.high_contrast,
        )
        .value;
    }
    // The details line follows the highlighted song — and goes BLANK
    // when there is none: with the early return it kept the last
    // song's line under an empty list ("1/71 … 336 notes" beneath
    // "no match", seen on screen).
    let entry = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i));
    if let Ok(mut text) = texts.p0().single_mut() {
        let line = detail_line(cursor.0, view.order.len(), entry, selected.0, |entry| {
            scores
                .best(&entry.title, &entry.artist, selected.0)
                .map(|b| (b.score, b.accuracy))
        });
        if text.0 != line {
            text.0 = line;
        }
    }
}

/// The details line under the list: position in the VIEW (under a
/// filter, "3/7" answers "of the matches"), tempo, length, the
/// selected difficulty with its rating and note count, and the best
/// record. Empty when no song is highlighted. Pure — tested.
fn detail_line(
    cursor: usize,
    count: usize,
    entry: Option<&SongEntry>,
    difficulty: Difficulty,
    best: impl Fn(&SongEntry) -> Option<(u64, f64)>,
) -> String {
    let Some(entry) = entry else {
        return String::new();
    };
    let duration = entry.duration_s.map_or_else(String::new, |d| {
        format!("  {}:{:02}", d as u32 / 60, d as u32 % 60)
    });
    let best = best(entry).map_or_else(
        || "no record yet".to_owned(),
        |(score, accuracy)| format!("best {score}  ({:.1}%)", accuracy * 100.0),
    );
    let rating = entry
        .rating(difficulty)
        .map_or_else(|| "-".to_owned(), |r| "*".repeat(usize::from(r)));
    let notes = entry
        .note_count(difficulty)
        .map_or_else(|| "-".to_owned(), |n| n.to_string());
    format!(
        "{}/{count}   {:.0} BPM{duration}   <{}>   {rating}   {notes} notes   {best}",
        cursor + 1,
        entry.bpm,
        difficulty.display_name().to_uppercase()
    )
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
    mut rendered: Local<Option<(Vec<usize>, Difficulty, String)>>,
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
    let key = rebuild_key(&raw.order, difficulty, &raw.filter);
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
    mut lists: Query<(&mut ScrollPosition, &mut Node), With<SongList>>,
) {
    let Ok((mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    // Every row is the same height, so any of them answers the
    // question - but a row may not have been laid out yet on the
    // first frame, and a height of zero would send the offset to
    // infinity.
    let Some(row) = rows
        .iter()
        .map(|(_, node)| node)
        .find(|node| node.size().y > 0.0)
    else {
        return;
    };
    ui_kit::follow_list(cursor.0, view.order.len(), row, &mut scroll, &mut node);
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
            preview_start_s: None,
            source: SongSource::Builtin(0),
            has_lyrics: false,
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
    fn an_empty_library_is_told_apart_from_an_empty_search() {
        // Since BeatByte ships no songs, a fresh install lands on an
        // empty browser. "no match for """ would be nonsense there,
        // and ESC would clear nothing — the screen has to say what
        // is actually missing and how to fix it.
        let fresh = empty_hint(0, "");
        assert!(fresh.contains("no songs"), "{fresh}");
        assert!(fresh.contains("drag"), "must name the way in: {fresh}");
        assert!(!fresh.contains("ESC"), "nothing to clear: {fresh}");
        // A library that HAS songs keeps the search wording.
        let filtered = empty_hint(12, "queen");
        assert!(filtered.contains("queen"), "{filtered}");
        assert!(filtered.contains("ESC"), "{filtered}");
    }

    #[test]
    fn every_word_of_the_filter_must_match_in_some_column() {
        // "toto rock" names the artist and the genre; "blondie maria"
        // the artist and the title. The whole-phrase test found
        // neither.
        let find = |filter: &str| {
            build_order(
                &lib(),
                SortMode::Standard,
                false,
                Difficulty::Medium,
                filter,
                |_| None,
            )
        };
        assert_eq!(find("toto rock"), vec![1]);
        assert_eq!(find("blondie maria"), vec![0]);
        assert_eq!(find("toto maria"), Vec::<usize>::new(), "AND, not OR");
        // Whitespace around and between words is not part of a word.
        assert_eq!(find("  toto  "), vec![1]);
        assert_eq!(find("   "), vec![0, 1, 2, 3]);
        // A phrase inside one column still matches, as before.
        assert_eq!(find("elle l'a"), vec![2]);
        // The column join is not a place a word can live.
        assert_eq!(find("mariablondie"), Vec::<usize>::new());
    }

    #[test]
    fn an_empty_list_rebuilds_when_the_filter_changes_a_full_one_does_not() {
        // The hint row quotes the filter; the song rows do not.
        let empty_q = rebuild_key(&[], Difficulty::Medium, "q");
        let empty_queen = rebuild_key(&[], Difficulty::Medium, "queen");
        assert_ne!(empty_q, empty_queen, "the hint must follow the word");
        let full_a = rebuild_key(&[0, 1], Difficulty::Medium, "a");
        let full_ab = rebuild_key(&[0, 1], Difficulty::Medium, "ab");
        assert_eq!(full_a, full_ab, "same rows, no rebuild per keystroke");
    }

    #[test]
    fn the_details_line_goes_blank_under_an_empty_list() {
        let songs = lib();
        let line = detail_line(0, 4, songs.first(), Difficulty::Medium, |_| {
            Some((1234, 0.987))
        });
        assert_eq!(
            line,
            "1/4   120 BPM  4:08   <MEDIUM>   *   100 notes   best 1234  (98.7%)"
        );
        assert_eq!(
            detail_line(0, 0, None, Difficulty::Medium, |_| None),
            "",
            "no song, no line - not the previous song's line"
        );
    }

    #[test]
    fn a_filter_ranks_the_best_match_first_and_tolerates_a_typo() {
        let songs = vec![
            entry("Lifeline", "Someone", None, 200.0),
            entry("Life", "Des'ree", Some("Pop"), 200.0),
            entry("Livin' On A Prayer", "Bon Jovi", Some("Rock"), 250.0),
            entry("Smells Like Teen Spirit", "Nirvana", Some("Grunge"), 300.0),
        ];
        let find = |filter: &str| {
            build_order(
                &songs,
                SortMode::Standard,
                false,
                Difficulty::Medium,
                filter,
                |_| None,
            )
        };
        // Standard order would put "Lifeline" first; the exact hit
        // outranks the prefix hit, and the typo-distance hit ("like")
        // comes last.
        assert_eq!(find("life"), vec![1, 0, 3]);
        // A missed letter, a swapped pair, an apostrophe not typed.
        assert_eq!(find("smels like"), vec![3]);
        assert_eq!(find("nirvana spirti"), vec![3]);
        assert_eq!(find("livin prayr"), vec![2]);
        assert_eq!(find("bon jovi"), vec![2], "artist");
        assert_eq!(
            find("seven nation armi"),
            Vec::<usize>::new(),
            "not in this library"
        );
        // Three letters get no slack: "lie" is not "life".
        assert_eq!(find("lie"), Vec::<usize>::new());
        // A sort still orders EQUAL scores: both "Life…" titles are
        // prefix hits for "lif", and by title Life sorts before
        // Lifeline.
        let by_title = build_order(
            &songs,
            SortMode::Title,
            false,
            Difficulty::Medium,
            "lif",
            |_| None,
        );
        assert_eq!(by_title, vec![1, 0]);
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

/// The search input as the player meets it: real keyboard messages
/// through the real system, one frame at a time.
#[cfg(test)]
mod search_input_tests {
    use super::*;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};

    /// A minimal app that runs `search_sort_input` and nothing else.
    fn app() -> App {
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .add_message::<crate::sfx::UiSound>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time>()
            .init_resource::<BrowserView>()
            .init_resource::<QuitHold>()
            .insert_resource(crate::config::Settings::default())
            .add_systems(Update, search_sort_input);
        app.world_mut().resource_mut::<BrowserView>().searching = true;
        app
    }

    /// Press a key: the physical state AND the typed message, as the
    /// keyboard plugin would produce them together.
    fn press(app: &mut App, code: KeyCode, text: &str) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(code);
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: Key::Character(text.into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    fn release(app: &mut App, code: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(code);
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: Key::Character("".into()),
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }

    /// What a focus loss does: every key let go at once, no release
    /// message for any of them.
    fn focus_lost(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release_all();
    }

    /// One frame of `dt` seconds.
    fn frame(app: &mut App, dt: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(dt));
        app.update();
        // What the input plugin does at the start of every frame.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn view(app: &App) -> (bool, String) {
        let view = app.world().resource::<BrowserView>();
        (view.searching, view.filter.clone())
    }

    #[test]
    fn a_tapped_q_is_a_letter_like_any_other() {
        // THE bug: "q" closed the search instead of landing in it,
        // so nothing beginning with q could be searched for.
        let mut app = app();
        press(&mut app, KeyCode::KeyQ, "q");
        frame(&mut app, 0.016);
        release(&mut app, KeyCode::KeyQ);
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "qu".to_owned()));
    }

    #[test]
    fn rolling_from_q_into_the_next_key_keeps_the_order() {
        // Typists press the next key before releasing the last one.
        // The pending q must land BEFORE the u, not after it.
        let mut app = app();
        press(&mut app, KeyCode::KeyQ, "q");
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        release(&mut app, KeyCode::KeyQ);
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "qu".to_owned()));
    }

    #[test]
    fn a_held_q_leaves_the_search_after_one_second_and_keeps_the_filter() {
        let mut app = app();
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyQ, "q");
        // Nine tenths of a second: still searching, q still pending.
        for _ in 0..9 {
            frame(&mut app, 0.1);
        }
        assert_eq!(view(&app), (true, "u".to_owned()));
        assert!(
            app.world().resource::<QuitHold>().progress().is_some(),
            "the bar is showing"
        );
        // The OS repeats the held key: not more letters.
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyQ,
            logical_key: Key::Character("q".into()),
            state: ButtonState::Pressed,
            text: Some("q".into()),
            repeat: true,
            window: Entity::PLACEHOLDER,
        });
        frame(&mut app, 0.11);
        assert_eq!(
            view(&app),
            (false, "u".to_owned()),
            "left the search, filter kept, no q written"
        );
        assert!(app.world().resource::<QuitHold>().progress().is_none());
    }

    #[test]
    fn a_q_released_early_is_typed_with_its_case() {
        let mut app = app();
        press(&mut app, KeyCode::KeyQ, "Q");
        for _ in 0..5 {
            frame(&mut app, 0.1);
        }
        release(&mut app, KeyCode::KeyQ);
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "Q".to_owned()));
    }

    #[test]
    fn a_focus_loss_mid_hold_types_nothing_and_closes_nothing() {
        let mut app = app();
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyQ, "q");
        frame(&mut app, 0.3);
        focus_lost(&mut app);
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "u".to_owned()));
        assert!(app.world().resource::<QuitHold>().progress().is_none());
    }

    #[test]
    fn escape_clears_the_filter_and_any_pending_q() {
        let mut app = app();
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyQ, "q");
        frame(&mut app, 0.1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (false, String::new()));
        assert!(app.world().resource::<QuitHold>().progress().is_none());
    }

    #[test]
    fn the_filter_keeps_what_was_typed_and_backspace_removes_one_character() {
        let mut app = app();
        press(&mut app, KeyCode::KeyM, "M");
        frame(&mut app, 0.016);
        press(&mut app, KeyCode::KeyO, "ö");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "Mö", "as typed, not folded on the way in");
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Backspace,
            logical_key: Key::Backspace,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "M");
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;

    #[test]
    fn a_tap_is_the_letter_and_a_hold_is_the_gesture() {
        let mut hold = QuitHold::default();
        assert_eq!(hold.tick(false, false, 0.016), HoldVerdict::Idle);
        hold.begin(KeyCode::KeyQ, 'q');
        assert_eq!(hold.tick(true, false, 0.05), HoldVerdict::Holding(0.05));
        assert_eq!(hold.tick(false, true, 0.016), HoldVerdict::TypeQ('q'));
        assert_eq!(hold.progress(), None);

        hold.begin(KeyCode::KeyQ, 'q');
        assert_eq!(hold.tick(true, false, 0.5), HoldVerdict::Holding(0.5));
        assert_eq!(hold.tick(true, false, 0.49), HoldVerdict::Holding(0.99));
        assert_eq!(hold.tick(true, false, 0.02), HoldVerdict::Close);
        assert_eq!(
            hold.tick(true, false, 0.02),
            HoldVerdict::Idle,
            "closed once"
        );

        // Taken away without a release: dropped, not typed.
        hold.begin(KeyCode::KeyQ, 'q');
        assert_eq!(hold.tick(false, false, 0.1), HoldVerdict::Idle);
        assert_eq!(hold.progress(), None);
    }

    #[test]
    fn flushing_hands_back_the_letter_once() {
        let mut hold = QuitHold::default();
        assert_eq!(hold.flush(), None);
        hold.begin(KeyCode::KeyQ, 'Q');
        assert_eq!(hold.key(), Some(KeyCode::KeyQ));
        assert_eq!(hold.flush(), Some('Q'));
        assert_eq!(hold.flush(), None);
        assert_eq!(hold.key(), None);
    }

    #[test]
    fn q_is_recognised_in_either_case_and_nothing_else() {
        assert!(is_q("q") && is_q("Q"));
        assert!(!is_q("qu") && !is_q("a") && !is_q(""));
    }
}
