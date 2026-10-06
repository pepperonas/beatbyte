//! The song browser: pick a song and a difficulty, see your best.

use beatbyte_core::Difficulty;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;

use crate::boot::{BuiltinSongs, LoadedSong, SongAudio};
use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::scores::ScoreBoard;
use crate::song_browser_view::{
    self as look, ActionsButton, BrowserScreen, DiffChip, EmptyHint, ImportNote, SongList, SongRow,
    SortButton, VersionChip,
};
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The difficulty the player will play.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedDifficulty(pub Difficulty);

/// The browser's input system, as a set others can order against —
/// it applies the player's saved difficulty every frame, so anything
/// that sets the difficulty for a song start must run after it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BrowserInputSet;

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
    /// The LYRICS column: songs whose words are still untimed first,
    /// then the word-timed ones, then the ones with no lyrics.
    Lyrics,
    /// The CHART column: the import's own draft first, then by
    /// generation.
    Chart,
    /// The AUDIO column: the files the loudness pass judged poor
    /// first, then fair, then good, the unmeasured last — a list of
    /// what still wants a better file.
    Audio,
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
            SortMode::Best => SortMode::Lyrics,
            SortMode::Lyrics => SortMode::Chart,
            SortMode::Chart => SortMode::Audio,
            SortMode::Audio => SortMode::Standard,
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
            SortMode::Lyrics => "LYRICS",
            SortMode::Chart => "CHART",
            SortMode::Audio => "AUDIO",
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
            "lyrics" | "lyr" => Some(SortMode::Lyrics),
            "chart" => Some(SortMode::Chart),
            "audio" => Some(SortMode::Audio),
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
    /// The rows as a tree (song, variants, revisions), parallel to
    /// `order`: `order[i]` is `tree[i].entry`.
    pub tree: Vec<crate::song_tree::Row>,
    /// What the player opened this session, by song folder and level.
    /// Kept by FOLDER, not by library index: a rescan renumbers the
    /// entries, and an open song must stay open through an import.
    pub open: std::collections::HashSet<(std::path::PathBuf, crate::song_tree::Level)>,
    /// The rows: one per song, its versions as members
    /// (`crate::song_family`). Parallel to `order`: `order[i]` is the
    /// member of `families[i]` that plays.
    pub families: Vec<crate::song_family::Family>,
    /// The version the player chose for a song this session: the
    /// song's folder → the chosen member's folder. Folders, because a
    /// rescan renumbers the entries.
    pub chosen: std::collections::HashMap<std::path::PathBuf, std::path::PathBuf>,
}

/// The song folder of an entry, if it has one (built-ins do not).
pub(crate) fn entry_folder(entry: &SongEntry) -> Option<std::path::PathBuf> {
    match &entry.source {
        SongSource::File { chart_path, .. } => {
            chart_path.parent().map(std::path::Path::to_path_buf)
        }
        SongSource::Builtin(_) => None,
    }
}

/// The entry a twin was made from, if that entry is in `order` — the
/// same match `pair_twins` sorts by (title without one prefix, same
/// artist).
///
/// A `[BG]` version of a song that is NOT in the library has no
/// original to stand under; its BG siblings (same title behind the
/// prefix, same artist) then gather under the lowest-numbered one, so
/// two downloads of one song are one family rather than two songs.
/// Pure — tested.
pub(crate) fn original_in(entries: &[SongEntry], order: &[usize], twin: usize) -> Option<usize> {
    let base = beatbyte_chart::twin::base_title(&entries[twin].title)?;
    let artist = &entries[twin].artist;
    if let Some(original) = order
        .iter()
        .copied()
        .find(|&j| entries[j].title == base && entries[j].artist == *artist)
    {
        return Some(original);
    }
    let (number, _) = beatbyte_chart::twin::split_bridge_title(&entries[twin].title)?;
    order
        .iter()
        .copied()
        .filter_map(|j| {
            let (n, rest) = beatbyte_chart::twin::split_bridge_title(&entries[j].title)?;
            (rest == base && entries[j].artist == *artist && n < number).then_some((n, j))
        })
        .min()
        .map(|(_, j)| j)
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
        SortMode::Lyrics => {
            // Untimed words first: a browser sorted by this column is
            // a list of what still wants aligning.
            order.sort_by_key(|i| {
                let polish = entries[*i].polish;
                let rank = match (polish.has_lyrics, polish.aligned) {
                    (true, false) => 0,
                    (true, true) => 1,
                    (false, _) => 2,
                };
                (rank, tie(i))
            });
        }
        SortMode::Chart => {
            // The import's own draft first, then by generation.
            order.sort_by_key(|i| (entries[*i].polish.chart_version.unwrap_or(0), tie(i)));
        }
        SortMode::Audio => {
            // The poor files first: a browser sorted by this column
            // is a list of what still wants a better file.
            order.sort_by_key(|i| {
                (
                    crate::loudness::audio_rank(entries[*i].loudness.as_ref()),
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
    pair_twins(entries, order)
}

/// Put every `[GS]` twin directly under its original —
/// whatever the sort, whichever way it runs, search or no search.
///
/// The list is sorted on the ORIGINALS: a twin never claims a place
/// of its own (under TITLE it would sit among the G's, under NOTES
/// wherever its lighter chart falls), it follows the song it is a
/// version of. A twin whose original is not in the list — filtered
/// out, or a hand-made study of a song that was since removed —
/// keeps the place the sort gave it. User, 2026-09-15: "die guitar
/// study tracks immer unter dem normalen track", the alphabet for
/// everything else. Pure — tested.
#[must_use]
pub fn pair_twins(entries: &[SongEntry], order: Vec<usize>) -> Vec<usize> {
    // ⚠️ The original may itself be a twin: a `[CL]` of a `[GS]` is
    // a twin OF THE STUDY, and `base_title` takes one prefix off for
    // exactly that reason. The old rule demanded a non-twin original,
    // so a chain's last link matched nothing, fell through to the
    // re-insertion loop — which walks originals only — and was
    // DROPPED from the list. A song vanishing from the browser is a
    // worse outcome than any ordering.
    let original_of = |twin: usize| original_in(entries, &order, twin);
    // Twins with an original in the list step out; everyone else
    // keeps their order, and each twin is re-inserted right after
    // its original.
    let mut paired: Vec<(usize, usize)> = Vec::new(); // (twin, original)
    let mut rest: Vec<usize> = Vec::with_capacity(order.len());
    for &i in &order {
        match original_of(i) {
            Some(original) => paired.push((i, original)),
            None => rest.push(i),
        }
    }
    // Each original brings its twins, and their twins after them.
    // A chain cannot loop — every link's title is strictly shorter
    // than the one that names it — but `seen` makes that structural
    // rather than argued: whatever the titles say, no entry is
    // emitted twice and the tail sweep catches anything missed.
    let mut out: Vec<usize> = Vec::with_capacity(order.len());
    let mut seen = vec![false; entries.len()];
    for i in rest {
        let mut stack = vec![i];
        while let Some(current) = stack.pop() {
            if std::mem::replace(&mut seen[current], true) {
                continue;
            }
            out.push(current);
            let mut children: Vec<usize> = paired
                .iter()
                .filter(|(_, original)| *original == current)
                .map(|(twin, _)| *twin)
                .collect();
            children.reverse();
            stack.extend(children);
        }
    }
    for &(twin, _) in &paired {
        if !std::mem::replace(&mut seen[twin], true) {
            out.push(twin);
        }
    }
    out
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

/// Whether a keypress may start the highlighted song.
///
/// CONFIRM is bound to Space AND Enter, and the browser used to act
/// on it whatever was being typed. So a space in the search filter
/// was both a space and "play" — typing a song's name started a
/// song. Reported against the add-a-song field, and true of the
/// filter since long before it.
///
/// The rule: **while a text field is taking keys, a printable key is
/// text.** Enter is not printable, so the filter keeps "narrow the
/// list, then Enter plays the highlighted one". The add-a-song field
/// means Enter as "search", so nothing there starts a song.
/// Pure — tested.
#[must_use]
pub fn may_start(filtering: bool, prompt_open: bool, enter: bool, confirm: bool) -> bool {
    if prompt_open {
        false
    } else if filtering {
        enter
    } else {
        confirm
    }
}

/// What a key press means for deleting the highlighted song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteStep {
    /// Ask first. Nothing is removed yet.
    Arm,
    /// The player answered yes.
    Confirm,
    /// Take the question away.
    Cancel,
    /// Nothing to do.
    Ignore,
}

/// The delete rule, in one place.
///
/// **The key that asks may never be the key that answers.** Remove is
/// Backspace, and somebody clearing a typed name taps Backspace many
/// times in a row; while the same key also confirmed, two of those
/// taps landing after a field had closed deleted the song under the
/// cursor and its audio. Reported by the player as "I keep deleting
/// songs by accident, because I mean to delete text" — and they were
/// right, two presses is no protection at all against a key you are
/// already repeating.
///
/// So Backspace only ever ARMS, however often it is pressed, and the
/// answer is **`Y`** — a key this screen uses for nothing else. ENTER
/// was the first fix and was not enough: it is the key that starts a
/// song, so a stray Backspace followed by the ENTER the player meant
/// as "play" would still have deleted. Any key that is not the
/// question and not the answer takes the question away, ENTER
/// included, which then starts the song as it always did.
/// Pure — tested.
#[must_use]
pub fn delete_step(armed: bool, remove: bool, confirm: bool, other: bool) -> DeleteStep {
    if !armed {
        // Nothing is pending: only the remove key does anything, and
        // all it does is ask.
        return if remove {
            DeleteStep::Arm
        } else {
            DeleteStep::Ignore
        };
    }
    if confirm {
        return DeleteStep::Confirm;
    }
    if remove {
        // A repeat of the asking key is still only asking. It
        // refreshes the timer and nothing else.
        return DeleteStep::Arm;
    }
    if other {
        return DeleteStep::Cancel;
    }
    DeleteStep::Ignore
}

/// Whether ENTER starts the highlighted song, given that it may have
/// just answered a delete instead.
///
/// One press, one meaning. Without this the confirming ENTER would
/// also start the song whose files it had just removed — the player
/// would be dropped into a track that no longer exists.
/// Pure — tested.
#[must_use]
pub fn starts_song(step: DeleteStep, may_start: bool) -> bool {
    !matches!(step, DeleteStep::Confirm) && may_start
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
/// What decides whether the rows are rebuilt ([`rebuild_key`]).
type RebuildKey = (Vec<usize>, Difficulty, String, SortMode);

fn rebuild_key(
    order: &[usize],
    difficulty: Difficulty,
    filter: &str,
    sort: SortMode,
) -> RebuildKey {
    let quoted = if order.is_empty() {
        filter.to_owned()
    } else {
        String::new()
    };
    // The sort is in the key because a row's right-hand column shows
    // the sorted-by value: a sort that happens to leave the order as
    // it was must still redraw the column.
    (order.to_vec(), difficulty, quoted, sort)
}

/// The line that stands in for the list when it has no rows. Two
/// different silences, two different sentences: a library with songs
/// in it whose filter matched nothing can be cleared with ESC, while
/// an empty library needs a song — telling the latter "no match for
/// """ would be both false and useless. Pure — tested.
#[must_use]
pub fn empty_hint(library_len: usize, filter: &str) -> String {
    if library_len == 0 {
        "no songs yet  -  click or press D to add  ·  or drag an audio file onto the window"
            .to_owned()
    } else {
        format!("no match for \"{filter}\"  -  ESC clears")
    }
}

/// Plugin for the song browser.
pub struct SongSelectPlugin;

impl Plugin for SongSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LyricsLookup>()
            .init_resource::<DownloadPrompt>()
            .init_resource::<SelectedDifficulty>()
            .init_resource::<BrowserCursor>()
            .init_resource::<crate::mc::McQueue>()
            .init_resource::<BrowserView>()
            .init_resource::<crate::preview::SongPreview>()
            .init_resource::<ActionBarClicks>()
            .init_resource::<ActionMenu>()
            .init_resource::<DeleteQuestion>()
            .add_systems(Startup, load_browser_prefs)
            .add_systems(
                OnEnter(AppState::SongSelect),
                (apply_preferred_difficulty, spawn_browser).chain(),
            )
            .add_systems(
                Update,
                action_menu
                    .before(browser_input)
                    .run_if(in_state(AppState::SongSelect)),
            )
            .add_systems(
                Update,
                (
                    browser_input.in_set(BrowserInputSet),
                    poll_lyrics_lookup,
                    download_input,
                    search_sort_input,
                    sync_view,
                    // After `sync_view`: the cursor and the order are
                    // settled for this frame, so a filter that moves
                    // the selection is heard as a move, not a start.
                    crate::preview::drive_preview,
                    look::sync_versions,
                    look::paint_rows,
                    look::paint_bar,
                    look::paint_panel,
                    rebuild_after_import,
                    look::follow_selection,
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

/// The action menu: every tool the browser has, behind TAB or the
/// ACTIONS button. Choosing an item presses the same chip id the old
/// chip row pressed, so every tool runs through the handler it always
/// ran through.
#[derive(Resource, Debug, Default)]
pub struct ActionMenu {
    /// Whether the menu is on screen.
    pub open: bool,
    /// The highlighted item.
    pub cursor: usize,
    /// The key that closed the menu this frame was spent there: the
    /// browser must not also read it (an Enter that chose "Edit chart"
    /// must not start the song as well).
    pub spent: bool,
    /// The tools offered, as chip ids, fixed when the menu opened.
    pub items: Vec<u8>,
}

/// One tool of the action menu: its label, its shortcut, its chip id.
pub struct Tool {
    /// What the row says.
    pub label: &'static str,
    /// The shortcut, as the row shows it.
    pub keys: &'static str,
    /// The chip id the tool's handler answers to.
    pub id: u8,
}

/// Every tool, in menu order.
pub const TOOLS: [Tool; 14] = [
    Tool {
        label: "PLAY",
        keys: "ENTER",
        id: chip::PLAY,
    },
    Tool {
        label: "EDIT CHART",
        keys: "CTRL E",
        id: chip::EDIT,
    },
    Tool {
        label: "SONG INFO",
        keys: "CTRL I",
        id: chip::INFO,
    },
    Tool {
        label: "SWITCH REVISION",
        keys: "",
        id: chip::REVISION,
    },
    Tool {
        label: "FETCH LYRICS",
        keys: "CTRL L",
        id: chip::LYRICS,
    },
    Tool {
        label: "ALIGN LYRICS",
        keys: "CTRL K",
        id: chip::ALIGN,
    },
    Tool {
        label: "REDESIGN CHART",
        keys: "CTRL G",
        id: chip::REDESIGN,
    },
    Tool {
        label: "TASTE TEST",
        keys: "CTRL T",
        id: chip::TASTE,
    },
    Tool {
        label: "ADD TO QUEUE / REMOVE",
        keys: "CTRL Q",
        id: chip::QUEUE,
    },
    Tool {
        label: "PLAY THE QUEUE",
        keys: "CTRL P",
        id: chip::PLAY_SET,
    },
    Tool {
        label: "ADD A SONG",
        keys: "CTRL D",
        id: chip::ADD,
    },
    Tool {
        label: "IMPORT BRIDGE DOWNLOADS",
        keys: "CTRL B",
        id: chip::BRIDGE,
    },
    Tool {
        label: "SORT",
        keys: "CTRL S",
        id: chip::SORT,
    },
    Tool {
        label: "DELETE SONG",
        keys: "CTRL BACKSPACE",
        id: chip::DELETE,
    },
];

/// The tools the menu offers for a song: everything that applies, in
/// menu order — a built-in has no files to edit, inspect, redesign or
/// delete, and an empty queue has nothing to play. Pure — tested.
#[must_use]
pub fn tools_for(is_file: bool, has_song: bool, queue_nonempty: bool) -> Vec<u8> {
    TOOLS
        .iter()
        .filter(|tool| match tool.id {
            chip::PLAY => has_song,
            chip::EDIT
            | chip::INFO
            | chip::REVISION
            | chip::REDESIGN
            | chip::TASTE
            | chip::DELETE => has_song && is_file,
            chip::LYRICS | chip::ALIGN | chip::QUEUE => has_song,
            chip::PLAY_SET => queue_nonempty,
            _ => true,
        })
        .map(|tool| tool.id)
        .collect()
}

/// The marker of the action menu's rows (the shared list renderer).
pub struct ActionRows;

/// The action menu's overlay.
#[derive(Component)]
pub struct ActionOverlay;

/// Open, drive and close the action menu. Runs before the browser's
/// input: a chosen tool becomes this frame's chip id, and the browser
/// runs it through the handler it always had.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub(crate) fn action_menu(
    mut commands: Commands,
    font: Res<UiFont>,
    settings: Res<crate::config::Settings>,
    library: Res<SongLibrary>,
    view: Res<BrowserView>,
    cursor: Res<BrowserCursor>,
    queue: Res<crate::mc::McQueue>,
    mut menu: ResMut<ActionMenu>,
    mut clicks: ResMut<ActionBarClicks>,
    mut list: crate::menu_list::list::ListInput<ActionRows>,
    mut paint: crate::menu_list::list::ListPaint<ActionRows>,
    overlays: Query<Entity, With<ActionOverlay>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    clicks.0.clear();
    if !menu.open {
        for overlay in &overlays {
            commands.entity(overlay).despawn();
        }
        return;
    }
    // The tools follow the selected song; rebuilt when they change
    // (the screen can open the menu before the list exists).
    let entry = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i));
    let items = tools_for(
        entry.is_some_and(look::is_file),
        entry.is_some(),
        !queue.0.is_empty(),
    );
    if overlays.is_empty() || items != menu.items {
        for overlay in &overlays {
            commands.entity(overlay).despawn();
        }
        menu.cursor = menu.cursor.min(items.len().saturating_sub(1));
        menu.items = items;
        let song = view.families.get(cursor.0).map_or_else(String::new, |f| {
            crate::song_family::row_title(&library.entries, f)
        });
        look::spawn_action_menu(&mut commands, &font, &menu.items, &song);
        return;
    }
    let count = menu.items.len();
    let mut at = menu.cursor;
    let events = list.read(&mut at, count);
    menu.cursor = at;
    if let Some(sound) = crate::menu_list::list::sound_for(None, events.moved) {
        sounds.write(sound);
    }
    let close = events.nav.back || events.right_click;
    if (events.nav.confirm || events.clicked)
        && let Some(&id) = menu.items.get(menu.cursor)
    {
        clicks.0 = vec![id];
        menu.open = false;
        menu.spent = true;
        sounds.write(crate::sfx::UiSound::Confirm);
    } else if close {
        menu.open = false;
        menu.spent = true;
        sounds.write(crate::sfx::UiSound::Back);
    }
    paint.paint(menu.cursor, settings.high_contrast, |index| {
        menu.items
            .get(index)
            .and_then(|id| TOOLS.iter().find(|t| t.id == *id))
            .map_or_else(String::new, |tool| tool.keys.to_owned())
    });
}

/// Screen-local ActionBar chip ids. Keys and chips share one path.
mod chip {
    pub const ADD: u8 = 1;
    pub const SORT: u8 = 2;
    pub const LYRICS: u8 = 3;
    pub const ALIGN: u8 = 4;
    pub const REDESIGN: u8 = 5;
    pub const TASTE: u8 = 6;
    pub const QUEUE: u8 = 7;
    pub const PLAY_SET: u8 = 8;
    pub const EDIT: u8 = 9;
    pub const DELETE: u8 = 10;
    pub const CONFIRM: u8 = 11;
    pub const CANCEL: u8 = 12;
    pub const BRIDGE: u8 = 14;
    pub const INFO: u8 = 15;
    pub const PLAY: u8 = 16;
    pub const REVISION: u8 = 17;
}

/// The tool chosen in the action menu this frame, as a chip id (the
/// menu fills it before the input systems; empty otherwise).
#[derive(Resource, Default)]
pub(crate) struct ActionBarClicks(pub(crate) Vec<u8>);

/// Pick the difficulty for the highlighted chart.
///
/// Returns `(difficulty, persist)` when the session selection should
/// move. `persist` is true only for an intentional LEFT/RIGHT step —
/// a per-song fallback never rewrites the player's preference. Pure
/// — tested.
#[must_use]
fn step_offered_difficulty(
    selected: Difficulty,
    preferred: Option<Difficulty>,
    offered: &[Difficulty],
    step: i8,
) -> Option<(Difficulty, bool)> {
    if offered.is_empty() {
        return None;
    }
    if step != 0 {
        let current = if offered.contains(&selected) {
            selected
        } else {
            Difficulty::among(preferred.unwrap_or(selected), offered).unwrap_or(offered[0])
        };
        let position = offered.iter().position(|d| *d == current).unwrap_or(0);
        let next = if step < 0 && position > 0 {
            Some(offered[position - 1])
        } else if step > 0 && position + 1 < offered.len() {
            Some(offered[position + 1])
        } else {
            None
        };
        return next.map(|d| (d, true));
    }
    let want = preferred.unwrap_or(selected);
    let effective = Difficulty::among(want, offered)?;
    (effective != selected).then_some((effective, false))
}

// Column widths in px. Press Start 2P advances ~1 em per glyph, so
// SMALL (10 px) columns hold width/10 characters.

fn spawn_browser(
    mut commands: Commands,
    font: Res<UiFont>,
    mut view: ResMut<BrowserView>,
    library: Res<SongLibrary>,
) {
    // The search is not open when the screen is entered: coming back
    // from a song into a field that swallows every letter (S, E, L,
    // Q, P all "dead") read as a broken screen. The FILTER stays, so
    // the next song is still a match away; F reopens the field, Esc
    // or the CLEAR button empties it.
    // The search is ALWAYS taking keys now (the rebuild of
    // 2026-10-05: "type to search"); the tools moved to Ctrl/Cmd and
    // the action menu.
    view.searching = true;
    let total = library.entries.len();
    look::spawn_shell(&mut commands, &font, &view, total);
}

/// Open song select on the current player's last difficulty.
///
/// No preference → leave the resource alone (its default is Medium,
/// and a mid-session visit keeps whatever they already stepped to
/// when nobody is on the roster).
fn apply_preferred_difficulty(
    mut selected: ResMut<SelectedDifficulty>,
    players: Res<crate::players::Players>,
) {
    if let Some(pref) = players.0.current_preferred_difficulty() {
        selected.0 = pref;
    }
}

/// Sort and search input. Its own system: `browser_input` sits at
/// Bevy's parameter limit, and the two concerns share no state
/// beyond the view.
///
/// The search is always taking keys (the rebuild of 2026-10-05: "type
/// to search"). Every printable key edits the filter — EXCEPT with
/// Ctrl/Cmd held (a tool shortcut, never text), while the action menu
/// is open or just closed, while the delete question waits for its
/// `Y`, and while the ADD field owns the keys.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn search_sort_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut view: ResMut<BrowserView>,
    sort_button: Query<&Interaction, (With<SortButton>, Changed<Interaction>)>,
    clicks: Res<ActionBarClicks>,
    menu: Res<ActionMenu>,
    question: Res<DeleteQuestion>,
    prompt: Res<DownloadPrompt>,
    mut settings: ResMut<crate::config::Settings>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let command = crate::editor_ui::command_held(&keys);
    if typing_allowed(
        command,
        menu.open || menu.spent,
        question.armed.is_some(),
        prompt.open,
    ) {
        for event in typed.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match &event.logical_key {
                bevy::input::keyboard::Key::Character(text) => {
                    view.filter.extend(text.chars().filter(|c| !c.is_control()));
                }
                bevy::input::keyboard::Key::Space => view.filter.push(' '),
                bevy::input::keyboard::Key::Backspace => {
                    // Handled here rather than via `just_pressed` so
                    // the OS key repeat erases while held, like every
                    // text field.
                    view.filter.pop();
                }
                _ => {}
            }
        }
    } else {
        typed.clear();
    }
    let mut sorted = false;
    let clicked = sort_button.iter().any(|i| *i == Interaction::Pressed);
    if (command && keys.just_pressed(KeyCode::KeyS))
        || clicked
        || ui_kit::chip_hit(&clicks.0, chip::SORT)
    {
        view.sort = view.sort.next();
        view.flipped = false;
        sorted = true;
    }
    if command && keys.just_pressed(KeyCode::KeyR) && view.sort != SortMode::Standard {
        view.flipped = !view.flipped;
        sorted = true;
    }
    // A sort ACTION blips like every other menu key and persists.
    if sorted {
        sounds.write(crate::sfx::UiSound::Toggle);
        settings.browser_sort = view.sort.label().to_lowercase();
        settings.browser_sort_reversed = view.flipped;
    }
}

/// Whether a printable key types into the search. Pure — tested.
#[must_use]
pub fn typing_allowed(command: bool, menu: bool, question: bool, field: bool) -> bool {
    !command && !menu && !question && !field
}

/// The member `step` versions away from `current` in `family`,
/// stopping at the ends like every list here. `None` for no step.
/// Pure — tested.
#[must_use]
pub fn next_version(
    family: &crate::song_family::Family,
    current: Option<usize>,
    step: i32,
) -> Option<usize> {
    if step == 0 {
        return None;
    }
    let at = current.and_then(|c| family.members.iter().position(|&m| m == c))?;
    let last = family.members.len().saturating_sub(1);
    let next = (at as i64 + i64::from(step.signum())).clamp(0, last as i64) as usize;
    (next != at).then(|| family.members[next])
}

/// The revision after `current` in `revisions` (wrapping), or `None`
/// when there is no other. Pure — tested.
#[must_use]
pub fn next_revision<'a>(
    revisions: &'a [beatbyte_chart::versions::Revision],
    current: Option<&str>,
) -> Option<&'a beatbyte_chart::versions::Revision> {
    if revisions.len() < 2 {
        return None;
    }
    let at = current
        .and_then(|name| revisions.iter().position(|r| r.name == name))
        .unwrap_or(0);
    revisions.get((at + 1) % revisions.len())
}

/// The delete question: which song it is about (by LIBRARY index —
/// a re-sort between the question and the answer moves rows, and
/// the question was about a song) and how long it still waits.
#[derive(Resource, Debug, Default)]
pub struct DeleteQuestion {
    /// The song asked about.
    pub armed: Option<usize>,
    /// Seconds left before the question lapses.
    pub left_s: f32,
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
    /// The difficulty chips in the panel.
    diff_chips: Query<'w, 's, (&'static DiffChip, &'static Interaction), Changed<Interaction>>,
    /// The version chips in the panel.
    version_chips:
        Query<'w, 's, (&'static VersionChip, &'static Interaction), Changed<Interaction>>,
    /// The ACTIONS button (same as Tab).
    actions_button:
        Query<'w, 's, &'static Interaction, (With<ActionsButton>, Changed<Interaction>)>,
    /// The action menu's state.
    menu: ResMut<'w, ActionMenu>,
    /// The delete question.
    question: ResMut<'w, DeleteQuestion>,
    /// Empty-library hint → add-a-song prompt (same as `D`).
    empty_hint: Query<'w, 's, &'static Interaction, (With<EmptyHint>, Changed<Interaction>)>,
    builtins: Res<'w, BuiltinSongs>,
    mc_queue: ResMut<'w, crate::mc::McQueue>,
    /// Who is playing — their preferred difficulty drives the browser.
    players: ResMut<'w, crate::players::Players>,
    /// The in-flight lyrics lookup — bundled here because Bevy caps
    /// a system at sixteen parameters and this one is at the line.
    lookup: ResMut<'w, LyricsLookup>,
    /// The aligner's state (`K` aligns the highlighted song).
    smart: ResMut<'w, crate::smart_lyrics::SmartLyrics>,
    /// The "add a song by name" field (`D` opens it).
    prompt: ResMut<'w, DownloadPrompt>,
    /// The one background job (`G` redesigns the highlighted chart).
    chore: ResMut<'w, crate::chore::Chore>,
    /// ActionBar presses for this frame (from [`paint_action_bar`]).
    clicks: Res<'w, ActionBarClicks>,
}

/// The folder a song's chart lives in, or `None` for a built-in.
///
/// A built-in is synthesized at boot and has no folder to write a
/// version into; the redesign has nothing to hold on to there, and
/// the browser says so rather than failing silently.
fn chart_folder(source: &SongSource) -> Option<std::path::PathBuf> {
    match source {
        SongSource::Builtin(_) => None,
        SongSource::File { chart_path, .. } => chart_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf),
    }
}

/// Redesign one song folder, off the main thread.
///
/// The reading of the recording is this side's to supply — the chart
/// crate deliberately knows nothing of audio — and it is the same
/// decode and analysis an import does, so a redesigned chart and a
/// freshly imported one come from the same reading.
fn redesign_song(folder: &std::path::Path) -> Result<String, String> {
    beatbyte_chart::redesign::redesign_folder(
        folder,
        &|audio_path| {
            let audio = beatbyte_audio::decode_file(audio_path)
                .map_err(|error| format!("cannot decode `{}`: {error}", audio_path.display()))?;
            let priming = audio.priming();
            let trim = beatbyte_chart::AudioTrim::declared(
                priming.samples,
                priming.timescale,
                audio.sample_rate(),
            );
            let analysis = <beatbyte_audio::SpectralAnalyzer as beatbyte_audio::Analyzer>::analyze(
                &beatbyte_audio::SpectralAnalyzer::default(),
                &audio,
            );
            Ok(beatbyte_chart::redesign::Reading { analysis, trim })
        },
        // A game has nowhere to print diagnostics, and the line the
        // player sees is the result rather than the working.
        &|_| {},
    )
    .map(|line| format!("redesigned: {line}"))
    .map_err(|reason| format!("redesign: {reason}"))
}

/// The "add a song by name" prompt: its own field, not the browser's
/// filter.
///
/// The first cut read the filter box and started on `Y`, which is
/// unusable and was reported as such: to press the key you must
/// first leave the field, and a song whose name contains the key is
/// a song you cannot type. A field of its own takes every letter,
/// starts on Enter and cancels on Esc, like any other text box.
#[derive(Resource, Default)]
pub struct DownloadPrompt {
    /// Whether the field is taking keys.
    pub open: bool,
    /// What has been typed into it.
    pub text: String,
}

impl DownloadPrompt {
    /// The line the panel shows while the field is open. Pure —
    /// tested.
    #[must_use]
    /// What the panel says while the field is open.
    ///
    /// It names the paste, because a link is the one thing nobody
    /// types — and ENTER means two things now, so it says which one
    /// it is about to do rather than promising a search and
    /// downloading a named video.
    pub fn line(&self) -> String {
        let action = if crate::discover::parse_target(&self.text).is_some() {
            "ENTER fetches that video"
        } else {
            "ENTER searches"
        };
        format!(
            "ADD A SONG: {}_   name or link ({} pastes)   {action}, ESC cancels",
            self.text,
            paste_chord()
        )
    }
}

/// The paste chord, named for the machine it is running on.
#[must_use]
const fn paste_chord() -> &'static str {
    if cfg!(target_os = "macos") {
        "CMD+V"
    } else {
        "CTRL+V"
    }
}

/// The prompt's keys. Runs before `browser_input`, which suppresses
/// its own letter shortcuts while this field is open — otherwise
/// typing a name would open the editor, queue a set and arm a delete
/// on the way through.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn download_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut prompt: ResMut<DownloadPrompt>,
    mut discovery: ResMut<crate::discover::Discovery>,
    mut status: ResMut<crate::import::ImportStatus>,
    settings: Res<crate::config::Settings>,
    clicks: Res<ActionBarClicks>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    if !prompt.open {
        // `D` opens the field HERE and not in `browser_input`, which
        // runs one system earlier: with the open there, the very
        // keystroke that caused it was still unread when this system
        // collected the text, and a `d` appeared in the empty field
        // (reported). One system owning the field is what fixes it —
        // proven in the running game, where the field now opens
        // empty and the next characters land whole.
        //
        // The drain is belt to that braces: a reader that returns
        // without reading leaves its cursor behind a frame. ⚠️ No
        // test covers it — removing it fails nothing, because the
        // harness loses a stale message the game would deliver. It
        // is kept as the cheap half of a hazard that is real in
        // principle, not as the demonstrated fix. A reader's cursor
        // is its own, so emptying it takes nothing from the other
        // systems that read the keyboard.
        //
        // The Add chip is the mouse door; empty-library CTA opens
        // the same prompt from `browser_input`.
        let opening = ((crate::editor_ui::command_held(&keys) && keys.just_pressed(KeyCode::KeyD))
            || ui_kit::chip_hit(&clicks.0, chip::ADD))
            && !discovery.running();
        for _ in typed.read() {}
        if opening {
            prompt.open = true;
            prompt.text.clear();
            sounds.write(crate::sfx::UiSound::Confirm);
            status.0 = prompt.line();
        }
        return;
    }
    // ⚠️ The modifier is read BEFORE the characters, or Cmd+V types
    // a literal "v" — which is what it did, so the one thing a
    // player would actually do with a link was impossible.
    let pasting = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]) && keys.just_pressed(KeyCode::KeyV);
    if pasting {
        match crate::clipboard::read() {
            Some(text) if !text.is_empty() => {
                prompt.text.push_str(&text);
                sounds.write(crate::sfx::UiSound::Toggle);
            }
            Some(_) | None => {
                sounds.write(crate::sfx::UiSound::Error);
                status.0 = "nothing to paste".to_owned();
                return;
            }
        }
        status.0 = prompt.line();
        return;
    }
    for event in typed.read() {
        if !event.state.is_pressed() {
            continue;
        }
        // A modifier held means a shortcut, not typing: without this
        // the paste above would also leave its "v" behind.
        if keys.any_pressed([
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
        ]) {
            continue;
        }
        match &event.logical_key {
            bevy::input::keyboard::Key::Character(text) => {
                let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                prompt.text.push_str(&typed);
            }
            bevy::input::keyboard::Key::Space => prompt.text.push(' '),
            // Here rather than on `just_pressed`, so the OS key
            // repeat erases while held like every text field.
            bevy::input::keyboard::Key::Backspace => {
                prompt.text.pop();
            }
            _ => {}
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        prompt.open = false;
        prompt.text.clear();
        status.0 = "search cancelled".to_owned();
        sounds.write(crate::sfx::UiSound::Back);
        return;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        let query = prompt.text.trim().to_owned();
        if query.is_empty() {
            sounds.write(crate::sfx::UiSound::Error);
            status.0 = "type a song name, then ENTER".to_owned();
            return;
        }
        if discovery.running() {
            sounds.write(crate::sfx::UiSound::Error);
            status.0 = "a search is already running".to_owned();
            return;
        }
        if !crate::discover::tool_available() {
            sounds.write(crate::sfx::UiSound::Error);
            status.0 = crate::discover::missing_tool_message();
            return;
        }
        let backend = crate::discover::backend_for(
            settings.ai_search,
            crate::discover::cli_available(),
            crate::discover::api_key(&settings.anthropic_api_key).as_deref(),
        );
        let fetching = crate::discover::parse_target(&query).is_some();
        discovery.start(query, backend);
        prompt.open = false;
        prompt.text.clear();
        sounds.write(crate::sfx::UiSound::Confirm);
        status.0 = if fetching {
            "fetching that video...".to_owned()
        } else {
            "searching...".to_owned()
        };
        return;
    }
    // While it is open the panel shows what is being typed.
    status.0 = prompt.line();
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
    builtins: Option<Res<crate::boot::BuiltinSongs>>,
    library: Option<ResMut<SongLibrary>>,
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
    // A lookup that wrote a file changed what the LYRICS column should
    // say; the rows only learn it from a rescan (`sync_view` rebuilds
    // on the library changing), as after an import or a delete.
    if let (Some(builtins), Some(mut library)) = (builtins, library) {
        *library = crate::boot::scan_with_builtins(&builtins.0);
    }
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
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    // The action menu owns the keys while it is open; the frame that
    // closed it is spent there too, but a tool it chose still runs
    // below (through its chip id).
    if start.menu.open {
        return;
    }
    let spent = std::mem::take(&mut start.menu.spent);
    let nav = if spent {
        MenuNav::default()
    } else {
        // The search takes every printable key, and Tab opens the
        // action menu — neither may move the cursor.
        MenuNav::read_typing_without_tab(&map, &keys, pads.iter())
    };
    // The ADD field owns the keys while it is open: nothing here may
    // run from a keystroke meant for a song name.
    let searching = start.prompt.open;
    let command = crate::editor_ui::command_held(&keys) && !spent;
    let shortcut = |key: KeyCode| command && keys.just_pressed(key);
    let clicks = &start.clicks.0;
    let clicked_back = ui_kit::back_pressed(&mut start.back_button);
    let empty_clicked = start.empty_hint.iter().any(|i| *i == Interaction::Pressed);
    // Esc with a filter still narrowing the list CLEARS it first and
    // leaves on the next press — the whole list is the state to
    // return to. The button and the right mouse button leave straight
    // away: they are pointed at the door, not at the list.
    if !searching && nav.back && !view.filter.is_empty() {
        view.filter.clear();
        sounds.write(crate::sfx::UiSound::Back);
        return;
    }
    let leave = ui_kit::wants_leave(
        !searching && nav.back,
        !searching && clicked_back,
        pointer_in.mouse.just_pressed(MouseButton::Right),
    );
    // TAB or the ACTIONS button opens the menu.
    let actions_clicked = start
        .actions_button
        .iter()
        .any(|i| *i == Interaction::Pressed);
    if !searching && !spent && (keys.just_pressed(KeyCode::Tab) || actions_clicked) {
        start.menu.open = true;
        start.menu.cursor = 0;
        sounds.write(crate::sfx::UiSound::Confirm);
        return;
    }
    let count = view.order.len();
    if count == 0 {
        // Empty library: the hint opens the add-a-song prompt.
        if empty_clicked && library.entries.is_empty() && !searching {
            start.prompt.open = true;
            start.prompt.text.clear();
            sounds.write(crate::sfx::UiSound::Confirm);
            status.0 = start.prompt.line();
            return;
        }
        if leave {
            sounds.write(crate::sfx::UiSound::Back);
            next_state.set(AppState::MainMenu);
        }
        return;
    }
    if nav.up {
        cursor.0 = crate::ui_kit::step_cursor(cursor.0, count, -1);
    }
    if nav.down {
        cursor.0 = crate::ui_kit::step_cursor(cursor.0, count, 1);
    }
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    // Mouse: wheel scrolls the list; clicking a row selects it, and
    // clicking the already-selected row starts it.
    for event in pointer_in.wheel.read() {
        if event.y > 0.0 {
            cursor.0 = crate::ui_kit::step_cursor(cursor.0, count, -1);
        } else if event.y < 0.0 {
            cursor.0 = crate::ui_kit::step_cursor(cursor.0, count, 1);
        }
        if event.y != 0.0 {
            sounds.write(crate::sfx::UiSound::Navigate);
        }
    }
    // Hover selects, click starts — the same rule as every other
    // menu. This list used to need two clicks (one to select, one to
    // start) and ignored hover entirely.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    let mouse_moved = pointer_in.moved.read().next().is_some();
    if let Some(index) = ui_kit::hover_moves_cursor(&pointer, mouse_moved) {
        cursor.0 = index;
    }
    let clicked_selected = pointer.clicked;
    // The version of the selected song: SHIFT+LEFT/RIGHT steps through
    // them, a click on a version chip picks one. Remembered by folder
    // for the session, so the song keeps it through a re-sort.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let version_step = if shift && !spent {
        i32::from(keys.just_pressed(KeyCode::ArrowRight))
            - i32::from(keys.just_pressed(KeyCode::ArrowLeft))
    } else {
        0
    };
    let picked = start
        .version_chips
        .iter()
        .find(|(_, i)| **i == Interaction::Pressed)
        .map(|(chip, _)| chip.0);
    if let Some(family) = view.families.get(cursor.0).cloned()
        && let Some(member) = picked
            .or_else(|| next_version(&family, view.order.get(cursor.0).copied(), version_step))
        && Some(member) != view.order.get(cursor.0).copied()
        && let (Some(head), Some(chosen)) = (
            entry_folder(&library.entries[family.head]),
            entry_folder(&library.entries[member]),
        )
    {
        view.chosen.insert(head, chosen);
        sounds.write(crate::sfx::UiSound::Toggle);
        return;
    }
    let Some(entry) = view
        .order
        .get(cursor.0)
        .and_then(|i| library.entries.get(*i))
    else {
        return;
    };

    // Difficulty: the current player's preference drives the pick.
    // A chart that does not offer it falls back for this song only —
    // the stored preference is untouched. Stepping LEFT/RIGHT (or the
    // `<`/`>` buttons) updates both the session and the profile.
    let preferred = start.players.0.current_preferred_difficulty();
    let offered = &entry.difficulties;
    let mut step_diff = 0i8;
    if nav.left && !shift {
        step_diff = -1;
    }
    if nav.right && !shift {
        step_diff = 1;
    }
    // A click on a difficulty chip goes straight there (when the song
    // offers it) and persists, like a step.
    let clicked_diff = start
        .diff_chips
        .iter()
        .find(|(chip, i)| **i == Interaction::Pressed && offered.contains(&chip.0))
        .map(|(chip, _)| chip.0);
    let chosen = clicked_diff.map_or_else(
        || step_offered_difficulty(selected.0, preferred, offered, step_diff),
        |d| (d != selected.0).then_some((d, true)),
    );
    if let Some(chosen) = chosen {
        let (next, persist) = chosen;
        selected.0 = next;
        if persist {
            sounds.write(crate::sfx::UiSound::Slider);
            if let Some(id) = start.players.0.selected {
                start.players.0.set_preferred_difficulty(id, next);
                start.players.0.stamp(id, crate::players::now_ms());
                crate::players::save_roster(&start.players);
            }
        }
    }

    // CTRL/CMD+BACKSPACE (or DELETE in the action menu) asks to
    // remove the highlighted song from disk; only `Y` answers, and it
    // answers the QUESTION rather than starting the song. Backspace
    // alone is text: it erases the search. Built-ins cannot be removed.
    start.question.left_s = (start.question.left_s - time.delta_secs()).max(0.0);
    if start.question.left_s <= 0.0 {
        start.question.armed = None;
    }
    let armed = start.question.armed.is_some();
    let yes =
        (armed && keys.just_pressed(KeyCode::KeyY)) || ui_kit::chip_hit(clicks, chip::CONFIRM);
    let remove = shortcut(KeyCode::Backspace)
        || shortcut(KeyCode::Delete)
        || ui_kit::chip_hit(clicks, chip::DELETE);
    let other = keys.get_just_pressed().any(|key| {
        !matches!(
            key,
            KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::KeyY
                | KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight
        )
    }) || ui_kit::chip_hit(clicks, chip::CANCEL);
    let step = if searching {
        // A field is taking keys: Backspace is text there, and a
        // question asked from a keystroke meant for a name is the
        // whole defect this rule exists for.
        DeleteStep::Ignore
    } else {
        delete_step(armed, remove, yes, other)
    };
    // Taken by value before the match: the rescan below needs
    // `library` mutably, and `entry` is a borrow of it.
    let mut confirmed_delete: Option<(std::path::PathBuf, String)> = None;
    let here = view.order.get(cursor.0).copied();
    let title = entry.title.clone();
    let removable = match &entry.source {
        crate::library::SongSource::File { chart_path, .. } => Some(chart_path.clone()),
        crate::library::SongSource::Builtin(_) => None,
    };
    match step {
        DeleteStep::Arm => {
            if removable.is_none() {
                status.0 = "built-in songs cannot be deleted".to_owned();
            } else {
                // Armed by LIBRARY index: a re-sort between the
                // question and the answer moves positions, and the
                // question was about a song, not a row number.
                start.question.armed = here;
                start.question.left_s = 8.0;
                status.0 =
                    format!("delete \"{title}\" and its files? Y deletes, any other key keeps it");
            }
        }
        DeleteStep::Confirm => {
            // Decided here so the ENTER is spent; CARRIED OUT at the
            // end of the function, where `entry` — a borrow of the
            // library this rescans — is finally out of scope.
            if start.question.armed == here {
                confirmed_delete = removable.map(|path| (path, title.clone()));
            }
            start.question.armed = None;
        }
        DeleteStep::Cancel => {
            start.question.armed = None;
            status.0 = "delete cancelled".to_owned();
        }
        DeleteStep::Ignore => {}
    }

    // An ENTER that answered the delete question is spent: it must
    // not also start the song whose files were just removed.
    let start_song = starts_song(
        step,
        may_start(
            view.searching,
            start.prompt.open,
            keys.just_pressed(KeyCode::Enter),
            nav.confirm,
        ),
    );
    // SWITCH REVISION (action menu): the next revision of this song's
    // chart becomes the one that plays — the browser's tree used to
    // offer them as rows; one row per song keeps them a step away.
    if ui_kit::chip_hit(clicks, chip::REVISION)
        && let Some(folder) = entry_folder(entry)
    {
        let title = entry.title.clone();
        let names = beatbyte_chart::twin::names_in(&folder).unwrap_or_default();
        let revisions = beatbyte_chart::versions::list_revisions(&names);
        let current = match &entry.source {
            SongSource::File { chart_path, .. } => chart_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            SongSource::Builtin(_) => None,
        };
        match next_revision(&revisions, current.as_deref()) {
            None => status.0 = format!("\"{title}\" has only one revision"),
            Some(next) => match beatbyte_chart::io::activate_revision(&folder, &next.name) {
                Ok(()) => {
                    *library = crate::boot::scan_with_builtins(&start.builtins.0);
                    status.0 = format!("revision {} of \"{title}\" plays from now on", next.number);
                    sounds.write(crate::sfx::UiSound::Toggle);
                }
                Err(reason) => {
                    status.0 = format!("cannot choose revision {}: {reason}", next.number);
                }
            },
        }
        return;
    }
    if start_song || clicked_selected || ui_kit::chip_hit(clicks, chip::PLAY) {
        sounds.write(crate::sfx::UiSound::Confirm);
        match prepare_song(entry, &start.builtins) {
            Ok(song) => {
                // An ordinary run is not a blind test: a leftover one
                // would crop this song to a window and ask for a
                // verdict nobody gave.
                commands.remove_resource::<crate::taste::TasteTest>();
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("cannot load \"{}\": {reason}", entry.title),
        }
    }
    // E opens the chart editor (file-based songs only — the demo is
    // generated, editing it would be lost on the next boot).
    if !searching
        && (shortcut(KeyCode::KeyE) || ui_kit::chip_hit(clicks, chip::EDIT))
        && let crate::library::SongSource::File {
            chart_path,
            audio_path,
        } = &entry.source
    {
        // A revision row edits THAT revision; any other row the one
        // that plays.
        let chart_path = chart_path.clone();
        match crate::editor_ui::open_editor(&mut commands, &chart_path, audio_path, selected.0) {
            Ok(()) => next_state.set(AppState::Editor),
            Err(reason) => error!("cannot edit \"{}\": {reason}", entry.title),
        }
    }
    // I (or the INFO button) shows everything the song's document
    // says — why it is Deep House, when it arrived, which analyser
    // said what. A built-in has no folder and so no document; a
    // folder that has never been migrated has none yet either, and
    // in both cases the key does nothing rather than opening an
    // empty panel.
    let info_clicked = ui_kit::chip_hit(clicks, chip::INFO);
    if !searching
        && (shortcut(KeyCode::KeyI) || info_clicked)
        && let crate::library::SongSource::File { chart_path, .. } = &entry.source
        && let Some(folder) = chart_path.parent()
    {
        match crate::song_info::Showing::read(folder) {
            Some(showing) => {
                commands.insert_resource(showing);
                next_state.set(AppState::SongInfo);
            }
            None => info!("\"{}\" has no document yet", entry.title),
        }
    }
    // L looks the highlighted song's karaoke lyrics up in lrclib's
    // catalogue - the lookup inspector-rust has been running in its
    // Shazam mode. Deliberately a key press, not something that
    // happens on its own: it is the one moment BeatByte talks to the
    // network, and only the artist and the title leave the machine.
    if !searching
        && (shortcut(KeyCode::KeyL) || ui_kit::chip_hit(clicks, chip::LYRICS))
        && start.lookup.task.is_none()
    {
        match &entry.source {
            SongSource::Builtin(_) => {
                status.0 = "built-in songs ship with their own lyrics".to_owned();
            }
            SongSource::File { audio_path, .. } => {
                let (artist, title) = (entry.artist.clone(), entry.title.clone());
                let audio = audio_path.clone();
                let shown = title.clone();
                // The song's own length goes with the question: the
                // catalogue must answer about THIS recording, not
                // about whatever else carries the title.
                let seconds = entry.duration_s;
                start.lookup.task = Some(LyricsTask(
                    bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                        crate::lyrics_fetch::fetch_and_cache(&artist, &title, seconds, &audio)
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
    if !searching && (shortcut(KeyCode::KeyK) || ui_kit::chip_hit(clicks, chip::ALIGN)) {
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
    // G re-runs the chart's own design on the highlighted song: a
    // fresh reading of the recording, hard and expert regenerated
    // from it, written as the folder's NEXT version with the pointer
    // moved — so a revert is one pointer away and nothing is
    // overwritten. The same redesign the command line runs
    // (`beatbyte_chart::redesign`), not a second one.
    //
    // It runs as a chore: off the main thread, reported on the import
    // overlay, and collected wherever the player has gone by the time
    // it finishes. A four-minute song takes the better part of a
    // minute, which is not a wait to hold a browser still for.
    if !searching && (shortcut(KeyCode::KeyG) || ui_kit::chip_hit(clicks, chip::REDESIGN)) {
        match chart_folder(&entry.source) {
            None => {
                sounds.write(crate::sfx::UiSound::Error);
                status.0 = "the built-in songs are generated, not designed".to_owned();
            }
            Some(folder) => {
                let title = crate::ui::font_safe(&entry.title);
                let line = format!("redesigning \"{title}\"...");
                match start
                    .chore
                    .start(line.clone(), move || redesign_song(&folder))
                {
                    Ok(()) => {
                        sounds.write(crate::sfx::UiSound::Confirm);
                        status.0 = line;
                    }
                    Err(reason) => {
                        sounds.write(crate::sfx::UiSound::Error);
                        status.0 = reason.to_owned();
                    }
                }
            }
        }
    }
    // B converts what the Bridge downloader fetched into BG versions
    // of the library's songs — every download not converted yet, not
    // only the highlighted song's. A chore, like the redesign: it
    // transcodes audio, and the rescan when it finishes puts the new
    // versions in the tree.
    if !searching && (shortcut(KeyCode::KeyB) || ui_kit::chip_hit(clicks, chip::BRIDGE)) {
        let line = "converting Bridge downloads...";
        match start.chore.start(line, crate::bridge_import::import_all) {
            Ok(()) => {
                sounds.write(crate::sfx::UiSound::Confirm);
                status.0 = line.to_owned();
            }
            Err(reason) => {
                sounds.write(crate::sfx::UiSound::Error);
                status.0 = reason.to_owned();
            }
        }
    }
    // T hears the same half minute twice, on two chart versions, in
    // an order that is not told: the blind taste test. The verdict
    // and the rating land on the results screen like any other run's.
    if !searching && (shortcut(KeyCode::KeyT) || ui_kit::chip_hit(clicks, chip::TASTE)) {
        match &entry.source {
            SongSource::Builtin(_) => {
                sounds.write(crate::sfx::UiSound::Error);
                status.0 = "the built-in songs have only one version".to_owned();
            }
            SongSource::File { chart_path, .. } => {
                // The seed is the clock: a fresh order every test, and
                // the order it chose is in the log.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64);
                match crate::taste::build(chart_path, selected.0, seed) {
                    Ok(test) => {
                        let Some(index) = test.current() else {
                            return;
                        };
                        let mut song = match prepare_song(entry, &start.builtins) {
                            Ok(song) => song,
                            Err(reason) => {
                                sounds.write(crate::sfx::UiSound::Error);
                                status.0 = format!("taste test: {reason}");
                                return;
                            }
                        };
                        song.chart = test.versions[index].chart.clone();
                        info!(
                            "taste test: \"{}\" on {} — {} against {}, {:.1}-{:.1}s",
                            entry.title,
                            selected.0,
                            test.versions[0].name,
                            test.versions[1].name,
                            test.window.0,
                            test.window.1
                        );
                        sounds.write(crate::sfx::UiSound::Confirm);
                        // A set left over from an earlier P would
                        // take the handover away from the test.
                        commands.remove_resource::<crate::mc::McSet>();
                        commands.insert_resource(test);
                        commands.insert_resource(song);
                        next_state.set(AppState::Gameplay);
                        return;
                    }
                    Err(reason) => {
                        sounds.write(crate::sfx::UiSound::Error);
                        status.0 = format!("taste test: {reason}");
                    }
                }
            }
        }
    }
    // Q queues the highlighted song for an MC set (again removes it);
    // P plays the queued set as one continuous DJ performance.
    if !searching && (shortcut(KeyCode::KeyQ) || ui_kit::chip_hit(clicks, chip::QUEUE)) {
        // Queued by folder, not by list position: a rescan before P
        // would otherwise play the neighbours of what was queued.
        if let Some(key) = crate::mc::QueuedSong::of(entry) {
            if let Some(at) = start.mc_queue.0.iter().position(|q| *q == key) {
                start.mc_queue.0.remove(at);
            } else {
                start.mc_queue.0.push(key.clone());
            }
            sounds.write(crate::sfx::UiSound::Toggle);
            // The row list carries no queued-marker (rows rebuild on
            // view changes only); the status line names the action
            // and the count instead.
            let added = start.mc_queue.0.contains(&key);
            status.0 = format!(
                "{} \"{}\" - MC set: {} song(s), P plays it",
                if added { "queued" } else { "removed" },
                entry.title,
                start.mc_queue.0.len()
            );
        }
    }
    if !searching
        && (shortcut(KeyCode::KeyP) || ui_kit::chip_hit(clicks, chip::PLAY_SET))
        && !start.mc_queue.0.is_empty()
    {
        let keys: Vec<Option<crate::mc::QueuedSong>> = library
            .entries
            .iter()
            .map(crate::mc::QueuedSong::of)
            .collect();
        let (found, missing) = crate::mc::resolve(&start.mc_queue.0, &keys);
        if !missing.is_empty() {
            // Said, not swallowed: the set is shorter than what was
            // queued, and the player should know why.
            warn!(
                "mc set: {} queued song(s) no longer in the library",
                missing.len()
            );
        }
        if found.is_empty() {
            status.0 = "MC set: the queued songs are no longer in the library".to_owned();
            start.mc_queue.0.clear();
            return;
        }
        let mut songs = Vec::new();
        for index in &found {
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
        commands.remove_resource::<crate::taste::TasteTest>();
        commands.insert_resource(crate::mc::McSet { songs, position: 0 });
        commands.insert_resource(first);
        start.mc_queue.0.clear();
        next_state.set(AppState::Gameplay);
        return;
    }
    // The delete the ENTER above answered yes to. Down here the
    // library may be rewritten: nothing borrows it any more.
    if let Some((chart_path, title)) = confirmed_delete {
        match crate::library::remove_song_files(&chart_path) {
            Ok(()) => {
                status.0 = format!("\"{title}\" deleted");
                *library = crate::boot::scan_with_builtins(&start.builtins.0);
            }
            Err(reason) => status.0 = format!("cannot delete: {reason}"),
        }
    }
    if leave {
        sounds.write(crate::sfx::UiSound::Back);
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
            let chart = match beatbyte_chart::load_chart_file(chart_path) {
                Ok(chart) => chart,
                // The version the scan saw is gone (a rollover, a
                // revert, under a running game): ask the folder
                // where the chart is now, once, instead of failing
                // every press until a rescan.
                Err(error) if !chart_path.is_file() => {
                    let Some(current) = crate::library::reresolve_chart(chart_path) else {
                        return Err(error.to_string());
                    };
                    warn!(
                        "`{}` is gone; the folder's active chart is now `{}` — loading that \
                         (the list refreshes on the next scan)",
                        chart_path.display(),
                        current.display()
                    );
                    beatbyte_chart::load_chart_file(&current).map_err(|e| e.to_string())?
                }
                Err(error) => return Err(error.to_string()),
            };
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
                vocals: beatbyte_chart::vocals::vocals_beside(audio_path),
                // One small read beside the chart, at the moment a
                // song is chosen — not per frame, and not at scan
                // time for a hundred and seventy folders.
                song_id: chart_path
                    .parent()
                    .and_then(beatbyte_library::store::read)
                    .map(|doc| doc.identity.song_id.as_str().to_owned()),
            })
        }
    }
}

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
    history: Res<crate::history::PlayHistory>,
    selected: Res<SelectedDifficulty>,
    lists: Query<Entity, With<SongList>>,
    fresh: Query<(), Added<SongList>>,
    mut rendered: Local<Option<RebuildKey>>,
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
    let best = |entry: &SongEntry| {
        scores
            .best(
                entry.song_id.as_deref(),
                &entry.title,
                &entry.artist,
                difficulty,
            )
            .map(|b| b.score)
    };
    // The whole library in the sort's order, and what the filter let
    // through: families gather over the first and show by the second.
    let full = build_order(
        &library.entries,
        view.sort,
        view.flipped,
        difficulty,
        "",
        best,
    );
    let filtered = build_order(
        &library.entries,
        view.sort,
        view.flipped,
        difficulty,
        &view.filter,
        best,
    );
    let families = crate::song_family::families(&library.entries, &full, &filtered);
    // When each title was played last, for the default version.
    let mut last: std::collections::HashMap<(&str, &str), u64> = std::collections::HashMap::new();
    for run in &history.0 {
        let at = last
            .entry((run.title.as_str(), run.artist.as_str()))
            .or_insert(0);
        *at = (*at).max(run.started_ms);
    }
    let order: Vec<usize> = families
        .iter()
        .map(|family| {
            let head = entry_folder(&library.entries[family.head]);
            let chosen = head.as_ref().and_then(|folder| view.chosen.get(folder));
            crate::song_family::default_member(
                &library.entries,
                family,
                chosen.map(std::path::PathBuf::as_path),
                |entry| {
                    last.get(&(entry.title.as_str(), entry.artist.as_str()))
                        .copied()
                },
            )
        })
        .collect();
    let filter_changed = *last_filter != view.filter;
    last_filter.clone_from(&view.filter);
    let raw = view.bypass_change_detection();
    // The cursor stays on its SONG through a re-sort or a version
    // change: follow the family head, not the row number.
    let old_heads: Vec<usize> = raw.families.iter().map(|f| f.head).collect();
    let new_heads: Vec<usize> = families.iter().map(|f| f.head).collect();
    cursor.0 = if filter_changed {
        0
    } else {
        cursor_after_change(false, &old_heads, cursor.0, &new_heads)
    };
    raw.order = order;
    raw.families = families;
    raw.tree.clear();
    let key = rebuild_key(&raw.order, difficulty, &raw.filter, raw.sort);
    if (entered || library.is_changed() || rendered.as_ref() != Some(&key))
        && let Ok(list) = lists.single()
    {
        look::spawn_rows(
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod difficulty_pref_tests {
    use super::{SelectedDifficulty, step_offered_difficulty};
    use beatbyte_core::Difficulty;
    use beatbyte_core::player::Roster;

    #[test]
    fn the_browser_defaults_to_medium() {
        assert_eq!(SelectedDifficulty::default().0, Difficulty::Medium);
    }

    #[test]
    fn a_step_persists_and_a_fallback_does_not() {
        let offered = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];
        // Prefer Hard on a chart that has it — no change, no persist.
        assert_eq!(
            step_offered_difficulty(Difficulty::Hard, Some(Difficulty::Hard), &offered, 0),
            None
        );
        // Prefer Hard on a Medium-only chart — session falls back, no persist.
        assert_eq!(
            step_offered_difficulty(
                Difficulty::Hard,
                Some(Difficulty::Hard),
                &[Difficulty::Easy, Difficulty::Medium],
                0
            ),
            Some((Difficulty::Medium, false))
        );
        // Intentional step: persist.
        assert_eq!(
            step_offered_difficulty(Difficulty::Medium, Some(Difficulty::Medium), &offered, 1),
            Some((Difficulty::Hard, true))
        );
    }

    #[test]
    fn two_players_keep_independent_preferences_across_a_restart() {
        let mut roster = Roster::default();
        let martin = roster.add("Martin", 1).unwrap();
        let kim = roster.add("Kim", 2).unwrap();
        roster.set_preferred_difficulty(martin, Difficulty::Hard);
        roster.set_preferred_difficulty(kim, Difficulty::Easy);

        let json = serde_json::to_string(&roster).unwrap();
        let mut back: Roster = serde_json::from_str(&json).unwrap();
        back.select(martin);
        assert_eq!(back.current_preferred_difficulty(), Some(Difficulty::Hard));
        back.select(kim);
        assert_eq!(back.current_preferred_difficulty(), Some(Difficulty::Easy));
        // New player: no preference → browser default path.
        let mut fresh = Roster::default();
        fresh.add("New", 3).unwrap();
        assert_eq!(fresh.current_preferred_difficulty(), None);
        assert_eq!(
            SelectedDifficulty::default().0,
            Difficulty::Medium,
            "no preference keeps the Medium default"
        );
    }

    #[test]
    fn fallback_does_not_overwrite_the_stored_preference() {
        let mut roster = Roster::default();
        let id = roster.add("Martin", 1).unwrap();
        roster.set_preferred_difficulty(id, Difficulty::Hard);
        let offered = [Difficulty::Easy];
        let (session, persist) =
            step_offered_difficulty(Difficulty::Hard, Some(Difficulty::Hard), &offered, 0)
                .expect("fallback");
        assert_eq!(session, Difficulty::Easy);
        assert!(!persist);
        assert_eq!(
            roster.get(id).unwrap().preferred_difficulty,
            Some(Difficulty::Hard)
        );
    }
}

#[cfg(test)]
mod delete_tests {
    use super::{DeleteStep, delete_step};

    #[test]
    fn delete_chips_drive_the_same_rule_as_the_keys() {
        use super::chip;
        use crate::ui_kit::chip_hit;
        let remove = chip_hit(&[chip::DELETE], chip::DELETE);
        assert_eq!(delete_step(false, remove, false, false), DeleteStep::Arm);
        let yes = chip_hit(&[chip::CONFIRM], chip::CONFIRM);
        assert_eq!(delete_step(true, false, yes, false), DeleteStep::Confirm);
        let cancel = chip_hit(&[chip::CANCEL], chip::CANCEL);
        assert_eq!(delete_step(true, false, false, cancel), DeleteStep::Cancel);
    }

    #[test]
    fn the_key_that_asks_to_delete_never_answers() {
        // The defect, in the player's words: "I keep deleting songs
        // by accident, because I mean to delete text." Clearing a
        // typed name is Backspace tapped many times; while the same
        // key also confirmed, two taps landing after a field closed
        // removed the song under the cursor AND its audio. Two
        // presses is no protection against a key you are repeating.
        assert_eq!(
            delete_step(false, true, false, false),
            DeleteStep::Arm,
            "the first press must only ask"
        );
        // However many more times it is pressed.
        for _ in 0..8 {
            assert_eq!(
                delete_step(true, true, false, false),
                DeleteStep::Arm,
                "a repeat of the asking key deleted a song"
            );
        }
        // Only the answer key answers.
        assert_eq!(delete_step(true, false, true, false), DeleteStep::Confirm);
        // And it means nothing until something was asked.
        assert_eq!(delete_step(false, false, true, false), DeleteStep::Ignore);
    }

    #[test]
    fn the_enter_that_confirms_a_delete_does_not_also_start_the_song() {
        // Otherwise the player is dropped into a track whose files
        // the same keystroke just removed.
        assert!(!super::starts_song(DeleteStep::Confirm, true));
        // Every other step leaves ENTER alone — the browser's normal
        // "narrow the list, then play the highlighted one" must not
        // be collateral damage of the delete rule.
        for step in [DeleteStep::Arm, DeleteStep::Cancel, DeleteStep::Ignore] {
            assert!(super::starts_song(step, true), "{step:?} swallowed ENTER");
            assert!(!super::starts_song(step, false));
        }
    }

    #[test]
    fn the_answer_key_means_nothing_else_on_this_screen() {
        // The whole point of moving the answer off ENTER. A key that
        // also does something else can be pressed for that something
        // else — which is how Backspace deleted songs, and then how
        // ENTER nearly did. `Y` is read in exactly one place.
        let source = include_str!("song_select.rs");
        // Only the shipped code: the tests below name the key too,
        // and counting this very assertion would make the pin pass
        // for the wrong reason.
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);
        let code: String = code
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // The form that MAKES something happen. `KeyY` also appears
        // in the list of keys that are neither question nor answer,
        // which is not a second meaning — counting that spelling too
        // was this test's own first mistake.
        assert_eq!(
            code.matches("just_pressed(KeyCode::KeyY)").count(),
            1,
            "the delete answer key triggers something else too"
        );
        // And the footer says so, or it is a secret handshake.
        assert!(
            source.contains("Y confirms"),
            "the footer hides the answer key"
        );
    }

    #[test]
    fn anything_else_takes_the_question_away() {
        // An armed delete that survived until the player happened to
        // press something else would be a trap with a fuse — and ENTER,
        // the key they press to play, is one of those others now.
        assert_eq!(delete_step(true, false, false, true), DeleteStep::Cancel);
        // Nothing pressed changes nothing — the timer does the rest.
        assert_eq!(delete_step(true, false, false, false), DeleteStep::Ignore);
        assert_eq!(delete_step(false, false, false, true), DeleteStep::Ignore);
        // Both in one frame: the answer wins, or a key pressed
        // alongside it would swallow the confirmation.
        assert_eq!(delete_step(true, false, true, true), DeleteStep::Confirm);
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use crate::library::{SongEntry, SongSource};

    fn entry(title: &str, artist: &str, genre: Option<&str>, len: f64) -> SongEntry {
        SongEntry {
            loudness: None,
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
            polish: crate::library::Polish::default(),
            song_id: None,
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

    fn file_entry(title: &str, chart_path: std::path::PathBuf) -> SongEntry {
        let mut e = entry(title, "Band", None, 200.0);
        e.source = SongSource::File {
            audio_path: chart_path.with_file_name("a.ogg"),
            chart_path,
        };
        e
    }

    #[test]
    fn a_queued_song_is_its_folder_whichever_revision_plays() {
        use crate::mc::QueuedSong;
        let folder = std::env::temp_dir().join("bb-queue-key");
        let first = file_entry("Song", folder.join("chart.json"));
        let later = file_entry("Song", folder.join("chart.v3.json"));
        // Choosing another revision changes the chart path; the queue
        // must still know the song.
        assert_eq!(
            QueuedSong::of(&first),
            Some(QueuedSong::Folder(folder.clone()))
        );
        assert_eq!(QueuedSong::of(&first), QueuedSong::of(&later));
        let other = file_entry(
            "Other",
            std::env::temp_dir().join("bb-other").join("chart.json"),
        );
        assert_ne!(QueuedSong::of(&first), QueuedSong::of(&other));
    }

    /// BG versions sit under the song when the library has it, and
    /// under their lowest-numbered sibling when it does not — two
    /// downloads of one song are one family, never two songs.
    #[test]
    fn bridge_versions_gather_under_the_song_or_under_bg_01() {
        let mut lib = lib();
        lib.push(entry("[BG-02] Life", "Des'ree", None, 200.0));
        lib.push(entry("[BG-01] Life", "Des'ree", None, 200.0));
        lib.push(entry("[BG-03] Only Here", "Band", None, 200.0));
        lib.push(entry("[BG-01] Only Here", "Band", None, 200.0));
        lib.push(entry("[BG-02] Only Here", "Other Band", None, 200.0));
        let at = |title: &str| {
            lib.iter()
                .position(|e| e.title == title)
                .expect("in the fixture")
        };
        let order: Vec<usize> = (0..lib.len()).collect();
        assert_eq!(
            original_in(&lib, &order, at("[BG-02] Life")),
            Some(at("Life"))
        );
        assert_eq!(
            original_in(&lib, &order, at("[BG-01] Life")),
            Some(at("Life"))
        );
        assert_eq!(
            original_in(&lib, &order, at("[BG-03] Only Here")),
            Some(at("[BG-01] Only Here")),
            "without the song, the lowest BG version stands in for it"
        );
        assert_eq!(original_in(&lib, &order, at("[BG-01] Only Here")), None);
        assert_eq!(
            original_in(&lib, &order, at("[BG-02] Only Here")),
            None,
            "another artist's song is another song"
        );
        let order = build_order(&lib, SortMode::Title, false, Difficulty::Medium, "", |_| {
            None
        });
        let titles: Vec<&str> = order.iter().map(|i| lib[*i].title.as_str()).collect();
        let life = titles.iter().position(|t| *t == "Life").expect("listed");
        assert_eq!(
            &titles[life + 1..life + 3],
            &["[BG-01] Life", "[BG-02] Life"],
            "{titles:?}"
        );
        let here = titles
            .iter()
            .position(|t| *t == "[BG-01] Only Here")
            .expect("listed");
        assert_eq!(titles[here + 1], "[BG-03] Only Here", "{titles:?}");
    }

    #[test]
    fn a_twin_sits_under_its_original_whatever_the_sort() {
        // Library order puts the twin first and far from its original;
        // its own values (title, length, notes) would place it
        // elsewhere under every sort. It follows its original anyway.
        let mut lib = lib();
        lib.insert(0, entry("[GS] Life", "Des'ree", Some("Pop"), 200.0));
        lib[0].note_counts = vec![40];
        let titles = |order: &[usize]| -> Vec<&str> {
            order.iter().map(|i| lib[*i].title.as_str()).collect()
        };
        for sort in [
            SortMode::Standard,
            SortMode::Title,
            SortMode::Artist,
            SortMode::Length,
            SortMode::Notes,
        ] {
            for flipped in [false, true] {
                let order = build_order(&lib, sort, flipped, Difficulty::Medium, "", |_| None);
                let t = titles(&order);
                let life = t
                    .iter()
                    .position(|x| *x == "Life")
                    .expect("the original is listed");
                assert_eq!(
                    t.get(life + 1),
                    Some(&"[GS] Life"),
                    "{sort:?} flipped={flipped}: {t:?}"
                );
                assert_eq!(t.len(), lib.len(), "nobody lost: {t:?}");
            }
        }
        // A search filter too: the twin never outranks its original.
        let order = build_order(
            &lib,
            SortMode::Title,
            false,
            Difficulty::Medium,
            "life",
            |_| None,
        );
        assert_eq!(titles(&order), vec!["Life", "[GS] Life"]);
    }

    /// ⚠️ A `[CL]` twin of a `[GS]` twin is a twin OF THE STUDY, and
    /// its original is itself a twin. The rule that an original must
    /// NOT be a twin left that last link matching nothing — and the
    /// re-insertion loop walks originals only, so the entry was
    /// DROPPED from the list entirely. A song vanishing from the
    /// browser is worse than any ordering, so the pin is both: the
    /// chain reads mix, study, classic-of-study, and the count comes
    /// out whole under every sort.
    #[test]
    fn a_chain_of_twins_reads_in_order_and_nobody_is_dropped() {
        let mut lib = lib();
        // Deliberately far from their original and out of order.
        lib.insert(0, entry("[CL] [GS] Life", "Des'ree", Some("Pop"), 200.0));
        lib.insert(0, entry("[CL] Life", "Des'ree", Some("Pop"), 200.0));
        lib.push(entry("[GS] Life", "Des'ree", Some("Pop"), 200.0));
        for sort in [
            SortMode::Standard,
            SortMode::Title,
            SortMode::Artist,
            SortMode::Length,
            SortMode::Notes,
        ] {
            for flipped in [false, true] {
                let order = build_order(&lib, sort, flipped, Difficulty::Medium, "", |_| None);
                let t: Vec<&str> = order.iter().map(|i| lib[*i].title.as_str()).collect();
                assert_eq!(t.len(), lib.len(), "{sort:?} flipped={flipped}: {t:?}");
                let mut sorted = order.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), lib.len(), "an entry was listed twice: {t:?}");
                let at = |title: &str| {
                    t.iter()
                        .position(|x| *x == title)
                        .unwrap_or_else(|| panic!("{title} is missing: {t:?}"))
                };
                // The family stands together, the original first —
                // which of the two direct twins comes first is the
                // sort's business, not this rule's.
                let family = ["Life", "[GS] Life", "[CL] Life", "[CL] [GS] Life"];
                let mut places: Vec<usize> = family.iter().map(|x| at(x)).collect();
                places.sort_unstable();
                assert_eq!(places[0], at("Life"), "{sort:?}: the original leads: {t:?}");
                assert_eq!(
                    places,
                    (places[0]..places[0] + family.len()).collect::<Vec<_>>(),
                    "{sort:?}: the family is not contiguous: {t:?}"
                );
                assert_eq!(
                    at("[CL] [GS] Life"),
                    at("[GS] Life") + 1,
                    "the classic twin must follow the study it was made from: {t:?}"
                );
            }
        }
    }

    #[test]
    fn a_twin_without_its_original_keeps_the_place_the_sort_gave_it() {
        // The original filtered out, or removed since: the twin is not
        // hidden and not moved — it is where TITLE puts it, among the
        // G's, which is honest about what it is.
        let mut lib = lib();
        lib.push(entry("[GS] Maria", "Blondie", None, 248.0));
        let order = build_order(&lib, SortMode::Title, false, Difficulty::Medium, "", |_| {
            None
        });
        let t: Vec<&str> = order.iter().map(|i| lib[*i].title.as_str()).collect();
        assert_eq!(
            t,
            vec!["Africa", "Ella, elle l'a", "Life", "Maria", "[GS] Maria"]
        );
        let order = build_order(
            &lib,
            SortMode::Title,
            false,
            Difficulty::Medium,
            "gs",
            |_| None,
        );
        let t: Vec<&str> = order.iter().map(|i| lib[*i].title.as_str()).collect();
        assert_eq!(
            t,
            vec!["[GS] Maria"],
            "alone when its original is filtered out"
        );
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
    fn audio_sorts_the_poor_files_first_and_the_unmeasured_last() {
        use crate::loudness::LoudnessMark;
        use beatbyte_audio::quality::Verdict;
        let mark = |verdict: Verdict| {
            Some(LoudnessMark {
                gain_db: 0.0,
                peak_limited: false,
                verdict,
                issue: None,
            })
        };
        let mut good = entry("Good", "a", None, 100.0);
        good.loudness = mark(Verdict::Good);
        let mut poor = entry("Poor", "b", None, 100.0);
        poor.loudness = mark(Verdict::Poor);
        let mut fair = entry("Fair", "c", None, 100.0);
        fair.loudness = mark(Verdict::Fair);
        let unmeasured = entry("Unknown", "d", None, 100.0);
        let entries = vec![good, unmeasured, fair, poor];
        let order = build_order(
            &entries,
            SortMode::Audio,
            false,
            Difficulty::Medium,
            "",
            |_| None,
        );
        let titles: Vec<&str> = order.iter().map(|i| entries[*i].title.as_str()).collect();
        assert_eq!(titles, vec!["Poor", "Fair", "Good", "Unknown"]);
        assert_eq!(SortMode::from_label("AUDIO"), Some(SortMode::Audio));
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
        let empty_q = rebuild_key(&[], Difficulty::Medium, "q", SortMode::Standard);
        let empty_queen = rebuild_key(&[], Difficulty::Medium, "queen", SortMode::Standard);
        assert_ne!(empty_q, empty_queen, "the hint must follow the word");
        let full_a = rebuild_key(&[0, 1], Difficulty::Medium, "a", SortMode::Standard);
        let full_ab = rebuild_key(&[0, 1], Difficulty::Medium, "ab", SortMode::Standard);
        assert_eq!(full_a, full_ab, "same rows, no rebuild per keystroke");
        // The right-hand column shows the sorted-by value, so a new sort
        // redraws the rows even when it leaves their order as it was.
        assert_ne!(
            rebuild_key(&[0, 1], Difficulty::Medium, "a", SortMode::Genre),
            full_a,
            "a sort that keeps the order must still redraw the column"
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
        for _ in 0..10 {
            mode = mode.next();
            seen.push(mode);
        }
        assert_eq!(mode.next(), SortMode::Standard, "the cycle closes");
        seen.sort_by_key(|m| m.label());
        seen.dedup();
        assert_eq!(seen.len(), 11, "every mode is reachable");
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
}

/// The rule that keeps a typed space from starting a song.
#[cfg(test)]
mod may_start_tests {
    use super::may_start;

    #[test]
    fn a_printable_key_is_text_while_a_field_is_taking_keys() {
        // Reported against the add-a-song field: pressing space
        // started a track. CONFIRM is bound to Space and Enter, and
        // the browser acted on it whatever was being typed — so the
        // filter had it too, since long before the field existed.
        let space_only = |filtering, prompt| may_start(filtering, prompt, false, true);
        assert!(!space_only(true, false), "a space in the filter is a space");
        assert!(!space_only(false, true), "and in the field it is a space");
        assert!(
            space_only(false, false),
            "with nothing being typed, CONFIRM still plays"
        );
    }

    #[test]
    fn enter_still_plays_from_the_filter_but_never_from_the_field() {
        // Narrowing the list and hitting Enter is worth keeping;
        // Enter in the add-a-song field means "search".
        assert!(may_start(true, false, true, true), "filter: Enter plays");
        assert!(!may_start(false, true, true, true), "field: Enter searches");
        assert!(may_start(false, false, true, true));
    }

    #[test]
    fn the_field_wins_over_the_filter_when_both_are_somehow_open() {
        // The field is the narrower state, so it decides.
        assert!(!may_start(true, true, true, true));
    }
}

/// The "add a song by name" field as the player meets it.
#[cfg(test)]
mod download_prompt_tests {
    use super::*;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};

    /// An app that runs `download_input` and nothing else, with the
    /// field already open.
    fn app() -> App {
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .add_message::<crate::sfx::UiSound>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<DownloadPrompt>()
            .init_resource::<crate::discover::Discovery>()
            .init_resource::<crate::import::ImportStatus>()
            .init_resource::<ActionBarClicks>()
            .insert_resource(crate::config::Settings::default())
            .add_systems(Update, download_input);
        app.world_mut().resource_mut::<DownloadPrompt>().open = true;
        app
    }

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            let key = if character == ' ' {
                Key::Space
            } else {
                Key::Character(character.to_string().into())
            };
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::KeyA, // the physical key is not read here
                logical_key: key,
                state: ButtonState::Pressed,
                text: Some(character.to_string().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
    }

    fn tap(app: &mut App, code: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(code);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(code);
    }

    /// Press the opening key for real, from a closed field.
    /// CTRL+D: the shortcut that opens the field since the browser
    /// types every plain letter into its search.
    fn press_d(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyD);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyD,
            logical_key: Key::Character("d".into()),
            state: ButtonState::Pressed,
            text: Some("d".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyD);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ControlLeft);
    }

    #[test]
    fn the_key_that_opens_the_field_does_not_type_itself_into_it() {
        // Reported: "wenn ich d drücke wird direkt d in die eingabe
        // übernommen." The open sat one system earlier, so the
        // keystroke that caused it was still unread when the text
        // was collected. Opening and reading now happen in one pass.
        let mut app = app();
        app.world_mut().resource_mut::<DownloadPrompt>().open = false;
        press_d(&mut app);
        assert!(
            app.world().resource::<DownloadPrompt>().open,
            "CTRL+D opens the field"
        );
        // A quiet frame, and this is the one that matters: the
        // keystroke was written before the field existed, so an
        // unread message would be delivered NOW and typed in. It is
        // also what the harness alone will not show — writing the
        // next characters loses the stale one, and the first version
        // of this test passed with the drain removed.
        app.update();
        let prompt = app.world().resource::<DownloadPrompt>();
        assert!(
            prompt.text.is_empty(),
            "the field starts empty, not with a d: {:?}",
            prompt.text
        );
        // And the next keystroke does land in it.
        type_text(&mut app, "aft Punk");
        assert_eq!(app.world().resource::<DownloadPrompt>().text, "aft Punk");
    }

    #[test]
    fn a_name_carrying_the_key_that_opens_the_field_types_in_full() {
        // The whole reason this field exists. The first cut read the
        // browser's filter and started the search on `Y`, so a song
        // with that letter in its name could not be typed — reported,
        // and right. Every letter goes in, the opening key included.
        let mut app = app();
        type_text(&mut app, "Daft Punk - Da Funk");
        let prompt = app.world().resource::<DownloadPrompt>();
        assert_eq!(prompt.text, "Daft Punk - Da Funk");
        assert!(prompt.open, "typing a name does not start anything");
    }

    #[test]
    fn escape_cancels_the_field_and_forgets_what_was_typed() {
        let mut app = app();
        type_text(&mut app, "Nirvana - Lithium");
        tap(&mut app, KeyCode::Escape);
        let prompt = app.world().resource::<DownloadPrompt>();
        assert!(!prompt.open, "Esc closes it");
        assert!(prompt.text.is_empty(), "and it does not come back typed");
        assert_eq!(
            app.world().resource::<crate::import::ImportStatus>().0,
            "search cancelled"
        );
    }

    #[test]
    fn backspace_erases_and_the_line_shows_what_is_there() {
        let mut app = app();
        type_text(&mut app, "abc");
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Backspace,
            logical_key: Key::Backspace,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let prompt = app.world().resource::<DownloadPrompt>();
        assert_eq!(prompt.text, "ab");
        let line = prompt.line();
        assert!(line.contains("ab_"), "the caret follows the text: {line}");
        assert!(line.contains("ESC"), "and the way out is on it: {line}");
    }

    #[test]
    fn an_empty_field_does_not_start_a_search() {
        // Enter on nothing must not reach the network, and must say
        // why rather than doing nothing at all.
        let mut app = app();
        tap(&mut app, KeyCode::Enter);
        let prompt = app.world().resource::<DownloadPrompt>();
        assert!(prompt.open, "the field stays open to be typed into");
        assert!(
            app.world()
                .resource::<crate::import::ImportStatus>()
                .0
                .contains("type a song name"),
            "it says what to do"
        );
        assert!(
            !app.world()
                .resource::<crate::discover::Discovery>()
                .running()
        );
    }

    #[test]
    fn a_closed_field_ignores_everything() {
        let mut app = app();
        app.world_mut().resource_mut::<DownloadPrompt>().open = false;
        type_text(&mut app, "typed at the browser, not the field");
        assert!(app.world().resource::<DownloadPrompt>().text.is_empty());
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
            .init_resource::<ActionBarClicks>()
            .init_resource::<ActionMenu>()
            .init_resource::<DeleteQuestion>()
            .init_resource::<DownloadPrompt>()
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
    fn q_is_a_letter_like_any_other_however_long_it_is_held() {
        // THE original bug: "q" closed the search instead of landing
        // in it, so nothing beginning with q could be searched for.
        // Its first fix made a HELD q the gesture, and a letter that
        // is also a gesture arrives late and pops a bar up mid-word.
        // Now q is only ever a letter, typed on the press.
        let mut app = app();
        press(&mut app, KeyCode::KeyQ, "q");
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "q".to_owned()), "typed on the press");
        // Held for a second, with the OS repeating it: still searching.
        for _ in 0..10 {
            frame(&mut app, 0.1);
        }
        assert!(view(&app).0, "a held q leaves nothing");
        release(&mut app, KeyCode::KeyQ);
        press(&mut app, KeyCode::KeyU, "u");
        frame(&mut app, 0.016);
        assert_eq!(view(&app), (true, "qu".to_owned()));
        // Upper case arrives as typed.
        press(&mut app, KeyCode::KeyQ, "Q");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "quQ");
    }

    /// Ctrl/Cmd + a letter is a tool, never text: Ctrl+E opens the
    /// editor and must not leave an "e" in the search.
    #[test]
    fn a_command_key_types_nothing() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        press(&mut app, KeyCode::KeyE, "e");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "");
    }

    /// While the action menu owns the keys, or the delete question
    /// waits for its `Y`, nothing lands in the search — the `Y` that
    /// deletes must not also search for "y".
    #[test]
    fn nothing_types_while_the_menu_or_the_delete_question_has_the_keys() {
        let mut app = app();
        app.world_mut().resource_mut::<ActionMenu>().open = true;
        press(&mut app, KeyCode::KeyA, "a");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "", "the menu has the keys");
        app.world_mut().resource_mut::<ActionMenu>().open = false;
        app.world_mut().resource_mut::<DeleteQuestion>().armed = Some(3);
        press(&mut app, KeyCode::KeyY, "y");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "", "the question has the keys");
        app.world_mut().resource_mut::<DeleteQuestion>().armed = None;
        press(&mut app, KeyCode::KeyB, "b");
        frame(&mut app, 0.016);
        assert_eq!(view(&app).1, "b", "and then letters are text again");
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
mod redesign_tests {
    use super::{SongSource, chart_folder};

    #[test]
    fn a_redesign_needs_a_folder_and_a_builtin_has_none() {
        // What `G` decides before it starts anything: a song on disk
        // gives the folder its versions are written into; a built-in
        // is synthesized at boot and has nowhere to put one, which
        // the browser says instead of failing quietly.
        assert_eq!(chart_folder(&SongSource::Builtin(0)), None);
        let source = SongSource::File {
            chart_path: std::path::PathBuf::from("/songs/a-song/chart.json"),
            audio_path: std::path::PathBuf::from("/songs/a-song/a.m4a"),
        };
        assert_eq!(
            chart_folder(&source),
            Some(std::path::PathBuf::from("/songs/a-song")),
            "the folder is where the next version goes"
        );
        // A chart path with no parent is not a folder either.
        let bare = SongSource::File {
            chart_path: std::path::PathBuf::from("chart.json"),
            audio_path: std::path::PathBuf::from("a.m4a"),
        };
        assert_eq!(chart_folder(&bare), None);
    }
}

#[cfg(test)]
mod rebuild_tests {
    use super::{TOOLS, chip, next_revision, next_version, tools_for, typing_allowed};
    use crate::song_family::Family;

    /// The search takes a key only when nothing else owns it: not with
    /// Ctrl/Cmd held, not while the menu or the delete question has
    /// the keys, not while the ADD field is open. Every combination.
    #[test]
    fn a_key_is_text_only_when_nothing_else_owns_it() {
        for bits in 0..16u8 {
            let (command, menu, question, field) =
                (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
            assert_eq!(
                typing_allowed(command, menu, question, field),
                bits == 0,
                "command {command}, menu {menu}, question {question}, field {field}"
            );
        }
    }

    /// The menu offers what applies: a built-in has no files to edit,
    /// inspect, redesign or delete; an empty queue has nothing to play;
    /// without a song only the library-wide tools remain.
    #[test]
    fn the_menu_offers_only_what_applies() {
        let file = tools_for(true, true, false);
        for id in [chip::PLAY, chip::EDIT, chip::INFO, chip::DELETE, chip::ADD] {
            assert!(file.contains(&id), "a file song offers {id}");
        }
        assert!(
            !file.contains(&chip::PLAY_SET),
            "an empty queue plays nothing"
        );
        let builtin = tools_for(false, true, true);
        for id in [
            chip::EDIT,
            chip::INFO,
            chip::REVISION,
            chip::REDESIGN,
            chip::TASTE,
            chip::DELETE,
        ] {
            assert!(!builtin.contains(&id), "a built-in has no files: {id}");
        }
        assert!(builtin.contains(&chip::PLAY_SET));
        let nothing = tools_for(false, false, false);
        assert_eq!(nothing, vec![chip::ADD, chip::BRIDGE, chip::SORT]);
        // Every tool has a label, and no id is offered twice.
        let mut ids: Vec<u8> = TOOLS.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TOOLS.len());
    }

    /// SHIFT+LEFT/RIGHT walks the versions and stops at the ends, like
    /// every list here; no step and a lone version change nothing.
    #[test]
    fn versions_step_and_stop_at_the_ends() {
        let family = Family {
            head: 0,
            members: vec![0, 4, 7],
        };
        assert_eq!(next_version(&family, Some(0), 1), Some(4));
        assert_eq!(next_version(&family, Some(4), 1), Some(7));
        assert_eq!(next_version(&family, Some(7), 1), None, "stops at the end");
        assert_eq!(next_version(&family, Some(4), -1), Some(0));
        assert_eq!(
            next_version(&family, Some(0), -1),
            None,
            "stops at the start"
        );
        assert_eq!(next_version(&family, Some(4), 0), None);
        let lone = Family {
            head: 3,
            members: vec![3],
        };
        assert_eq!(next_version(&lone, Some(3), 1), None);
    }

    /// SWITCH REVISION goes to the next one and wraps; a chart with a
    /// single revision has nowhere to go.
    #[test]
    fn the_next_revision_wraps_and_a_single_one_has_none() {
        let revisions = beatbyte_chart::versions::list_revisions(&[
            "chart.json".to_owned(),
            "chart.v2.json".to_owned(),
            "chart.v3.json".to_owned(),
        ]);
        let next = |current| next_revision(&revisions, current).map(|r| r.name.clone());
        assert_eq!(next(Some("chart.json")).as_deref(), Some("chart.v2.json"));
        assert_eq!(
            next(Some("chart.v3.json")).as_deref(),
            Some("chart.json"),
            "wraps"
        );
        let single = beatbyte_chart::versions::list_revisions(&["chart.json".to_owned()]);
        assert!(next_revision(&single, Some("chart.json")).is_none());
    }
}
