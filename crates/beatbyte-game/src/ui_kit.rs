//! The shared chrome for every menu and settings screen.
//!
//! Before this module existed, each screen invented its own layout:
//! seven different title-to-content gaps, six font sizes for the same
//! kind of row, and rows that were bare text — selectable only by the
//! colour of their letters. The screens looked related because they
//! shared a font, not because they shared a design.
//!
//! Everything here is a *token* or a *scaffold*, never behaviour. A
//! screen keeps its own state, input handling and marker components;
//! it only borrows the shell: header, framed panel, rows, footer.
//!
//! The look is an arcade cabinet, not a settings app — the pixel font
//! and the navy/brand-yellow palette were already the game's voice, so
//! this adds structure (frame, accent bar, one rhythm of spacing)
//! rather than a new style.

use bevy::prelude::*;

use crate::palette;
use crate::ui::UiFont;

// ── Type scale ──────────────────────────────────────────────────────
// Three sizes, and no screen may invent a fourth. Press Start 2P runs
// wide, so these are roughly half of what a normal face would use.

/// The game's wordmark on the main menu, and the pause banner. Not a
/// heading — the one place the name is allowed to shout.
pub const WORDMARK: f32 = 52.0;
/// Screen title ("SETTINGS", "SONG SELECT"), and any big transient
/// readout such as the input tester's HIT flash.
pub const TITLE: f32 = 28.0;
/// Any selectable row: menu entries, settings, bindings, songs.
pub const ROW: f32 = 13.0;
/// Incidental text: the subtitle under a title, the footer hint, a
/// note beside a list. Deliberately ONE size — the screens this
/// replaced used 9 px and 10 px for exactly these two jobs, a
/// difference nobody can see and a test now forbids.
pub const SMALL: f32 = 10.0;

// ── Spacing, on a 4 px grid ─────────────────────────────────────────

/// Gap between rows inside a panel.
pub const ROW_GAP: f32 = 4.0;
/// Panel width. Wide enough for the longest settings row
/// ("TAP MODE (NO STRUM)" plus "8-BIT SHAPES") at [`ROW`] px.
pub const PANEL_WIDTH: f32 = 620.0;
/// Space between the header block and the panel.
pub const HEADER_GAP: f32 = 24.0;
/// Space between the panel and the footer hint.
pub const FOOTER_GAP: f32 = 20.0;

/// Horizontal padding inside a row.
const ROW_PAD_X: f32 = 14.0;
/// Vertical padding inside a row.
const ROW_PAD_Y: f32 = 7.0;
/// Width of the accent bar on a row's left edge.
const ACCENT_WIDTH: f32 = 3.0;
/// Padding inside the panel frame.
pub const PANEL_PAD: f32 = 16.0;

/// A panel's border, on each side.
///
/// Named because [`whole_rows_height`] has to subtract it: Bevy sizes
/// a node by its BORDER box, so a height that accounts only for the
/// padding leaves the last row two pixels short of its own space.
pub const PANEL_BORDER: f32 = 1.0;

/// How strongly a selected row is filled.
///
/// Measured, not guessed: Bevy blends `BackgroundColor` alpha in
/// LINEAR space, so the first attempt at 0.12 rendered as sRGB
/// (99, 84, 35) — a solid olive bar that shouted over the accent and
/// the label both. At this weight it reads as sRGB (68, 57, 24): a
/// tint the eye accepts as "this one", with the bar doing the work.
pub const FILL_ALPHA: f32 = 0.055;
/// The armed fill sits slightly higher — it is a modal state, and it
/// has no bright label to lean on.
const FILL_ARMED: f32 = 0.08;

// ── Row states ──────────────────────────────────────────────────────

/// What a row is currently doing. Hover is deliberately absent: on
/// every screen hovering *moves the cursor*, so a hovered row is a
/// selected row and a second visual state would only contradict it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// Not the cursor row.
    Idle,
    /// The cursor row.
    Selected,
    /// The cursor row, waiting for the player to press something
    /// (the controls screen while capturing a new binding).
    Armed,
}

/// The four colours a row is painted with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowStyle {
    /// Fill behind the row.
    pub background: Color,
    /// The bar on the left edge — the primary selection cue.
    pub accent: Color,
    /// Colour of the row's name.
    pub label: Color,
    /// Colour of the row's value, where it has one.
    pub value: Color,
}

/// The palette for a row state.
///
/// Pure, so the contrast decisions are testable: an idle row must
/// stay legible, and a selected row must differ in more than one
/// channel (bar *and* fill *and* text), because colour alone is a
/// weak cue on a dark background.
#[must_use]
pub fn row_style(state: RowState) -> RowStyle {
    match state {
        RowState::Idle => RowStyle {
            background: Color::NONE,
            accent: Color::NONE,
            label: palette::TEXT_DIM,
            value: palette::dimmed(palette::TEXT_DIM, 0.8),
        },
        RowState::Selected => RowStyle {
            background: palette::BRAND.with_alpha(FILL_ALPHA),
            accent: palette::BRAND,
            label: palette::BRAND,
            value: palette::TEXT,
        },
        RowState::Armed => RowStyle {
            background: palette::HYPE.with_alpha(FILL_ARMED),
            accent: palette::HYPE,
            label: palette::HYPE,
            value: palette::TEXT,
        },
    }
}

/// The state for a row, from the two questions every screen asks.
#[must_use]
pub const fn state_for(selected: bool, armed: bool) -> RowState {
    match (selected, armed) {
        (true, true) => RowState::Armed,
        (true, false) => RowState::Selected,
        (false, _) => RowState::Idle,
    }
}

// ── Pointer ─────────────────────────────────────────────────────────

/// What the pointer did to a screen's rows this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowPointer {
    /// The row under the pointer, if any — it becomes the cursor row.
    pub hovered: Option<usize>,
    /// A row was pressed, so the cursor row should be activated.
    pub clicked: bool,
}

/// Read a screen's rows into one answer, so every menu obeys the same
/// rule: **hovering selects, clicking activates.**
///
/// This exists because the song browser did not. It handled only
/// `Interaction::Pressed` and needed two clicks — one to select a row,
/// another to start it — while the main menu selected on hover. Two
/// lists that look identical behaved differently, which is the kind of
/// inconsistency a shared style guide hides rather than fixes.
///
/// A press implies a hover: pressing a row makes it the cursor row
/// first, so the click always activates the row under the pointer and
/// never whatever happened to be selected before.
pub fn read_rows<'a>(rows: impl Iterator<Item = (usize, &'a Interaction)>) -> RowPointer {
    let mut pointer = RowPointer::default();
    for (index, interaction) in rows {
        match interaction {
            Interaction::Hovered => pointer.hovered = Some(index),
            Interaction::Pressed => {
                pointer.hovered = Some(index);
                pointer.clicked = true;
            }
            Interaction::None => {}
        }
    }
    pointer
}

// ── Scaffold ────────────────────────────────────────────────────────

/// The full-screen root every menu spawns: one centred column.
#[must_use]
pub fn screen_root() -> Node {
    Node {
        width: percent(100),
        height: percent(100),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// Title, and the line that says what the screen is for.
///
/// The subtitle is not decoration: before this, four screens opened
/// with a bare word and left the player to infer the rest.
pub fn header(parent: &mut ChildSpawnerCommands, font: &UiFont, title: &str, subtitle: &str) {
    parent.spawn((
        Text::new(title.to_owned()),
        font.text(TITLE),
        TextColor(palette::BRAND),
    ));
    parent.spawn((
        Text::new(subtitle.to_owned()),
        font.text(SMALL),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
        Node {
            margin: UiRect::top(px(8)).with_bottom(px(HEADER_GAP)),
            ..default()
        },
    ));
}

/// The frame every panel wears: lifted surface, hairline border,
/// small radius. Split out so the two panel shapes cannot drift.
fn frame() -> (BackgroundColor, BorderColor) {
    (
        BackgroundColor(palette::SURFACE.with_alpha(0.55)),
        BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.3)),
    )
}

/// The tallest a list panel grows before its rows start scrolling.
///
/// Chosen so the header above and the details, hint and footer below
/// all still fit at the smallest window the projection guarantees.
/// Without a ceiling the panel simply grows: at 22 songs the title,
/// the details line and the whole footer had been pushed off the
/// screen, and the first and last rows were cut in half.
pub const PANEL_MAX_H: f32 = 400.0;

/// The scroll offset that brings a row into view, moving as little as
/// it can.
///
/// `row_top` and `content_h` are measured from the top of the content,
/// `current` is where the viewport is now. Returning `current`
/// unchanged when the row is already visible is the whole point: a
/// list that re-centred on every frame would twitch under the cursor
/// and make the rows above and below unreadable.
#[must_use]
pub fn scroll_to_show(
    row_top: f32,
    row_h: f32,
    viewport_h: f32,
    content_h: f32,
    current: f32,
) -> f32 {
    // Never scroll past the end - below the last row there is nothing
    // to look at, and a list that can be scrolled into blank space
    // feels broken.
    let furthest = (content_h - viewport_h).max(0.0);
    let wanted = if row_top < current {
        // Above the fold: bring its top to the top edge.
        row_top
    } else if row_top + row_h > current + viewport_h {
        // Below the fold: bring its bottom to the bottom edge.
        row_top + row_h - viewport_h
    } else {
        current
    };
    wanted.clamp(0.0, furthest)
}

/// The viewport height that shows whole rows and no more.
///
/// A window whose height is not a multiple of the row pitch cuts its
/// last row through the middle of the letters. Half a row is a fine
/// way to say "there is more below" in a list you drag with a mouse;
/// in one you step through with a cursor it just looks unfinished, and
/// the position readout says the same thing in words.
///
/// Returns `None` when even one row will not fit, so the caller can
/// leave the panel alone rather than collapse it to nothing.
#[must_use]
pub fn whole_rows_height(row_h: f32, gap: f32, rows: usize, ceiling: f32) -> Option<f32> {
    if row_h <= 0.0 || rows == 0 {
        return None;
    }
    let pitch = row_h + gap;
    let inner = ceiling - 2.0 * (PANEL_PAD + PANEL_BORDER);
    // The gaps sit BETWEEN rows: n rows have n-1 of them, so the
    // naive `inner / pitch` undercounts by very nearly one row.
    let fits = ((inner + gap) / pitch).floor().max(1.0) as usize;
    let shown = fits.min(rows);
    if shown >= rows {
        // Everything fits; no ceiling needed, and forcing one would
        // clip a list that had no reason to scroll.
        return None;
    }
    let content = shown as f32 * pitch - gap;
    Some(content + 2.0 * (PANEL_PAD + PANEL_BORDER))
}

/// A list panel that scrolls once its rows outgrow [`PANEL_MAX_H`].
///
/// Same frame and rhythm as [`panel`]; the only difference is the
/// ceiling and the clipping, so a short list is indistinguishable from
/// the unscrolled one.
#[must_use]
pub fn scroll_panel() -> impl Bundle {
    let (background, border) = frame();
    (
        Node {
            width: px(PANEL_WIDTH),
            max_height: px(PANEL_MAX_H),
            flex_direction: FlexDirection::Column,
            row_gap: px(ROW_GAP),
            padding: UiRect::all(px(PANEL_PAD)),
            border: UiRect::all(px(PANEL_BORDER)),
            border_radius: BorderRadius::all(px(6)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollPosition::default(),
        background,
        border,
    )
}

/// The framed container that holds a screen's rows.
#[must_use]
pub fn panel() -> impl Bundle {
    let (background, border) = frame();
    (
        Node {
            width: px(PANEL_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(ROW_GAP),
            padding: UiRect::all(px(PANEL_PAD)),
            border: UiRect::all(px(PANEL_BORDER)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        background,
        border,
    )
}

/// The same frame around something that is not a list — the
/// calibration dot, the input tester's lamps. Centred, and roomier,
/// because an instrument needs air where a list needs density.
#[must_use]
pub fn panel_centered() -> impl Bundle {
    let (background, border) = frame();
    (
        Node {
            width: px(PANEL_WIDTH),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(16),
            padding: UiRect::all(px(24)),
            border: UiRect::all(px(PANEL_BORDER)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        background,
        border,
    )
}

/// One row's chrome: fill, accent bar and the two-column layout.
///
/// Deliberately *without* `Button` — a screen adds that itself when
/// the row is actually clickable. The multiplayer slots are rows that
/// only report status, and giving them a button would promise an
/// interaction that does not exist.
#[must_use]
pub fn row() -> impl Bundle {
    (
        Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(16),
            padding: UiRect::axes(px(ROW_PAD_X), px(ROW_PAD_Y)),
            border: UiRect::left(px(ACCENT_WIDTH)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    )
}

/// A row's left-hand label. Never shrinks: the name of a setting is
/// what the player scans for.
#[must_use]
pub fn label_node() -> Node {
    Node {
        flex_shrink: 0.0,
        ..default()
    }
}

/// A row's right-hand value. Bounded, so a long one wraps instead of
/// running into the label — the controls screen has bindings like
/// "Enter / PAD Select / PAD RightTrigger", and unbounded they
/// collided with the word HYPE.
///
/// Right-justified, because once a value wraps its second line would
/// otherwise start at the left edge of the box and leave the column's
/// right edge ragged.
#[must_use]
pub fn value_node() -> impl Bundle {
    (
        Node {
            max_width: percent(62),
            ..default()
        },
        TextLayout::justify(Justify::Right),
    )
}

/// The hint line at the bottom of a screen.
///
/// Uniform wording matters as much as uniform styling: `KEY action`
/// pairs, separated by two spaces, lower-case verbs.
pub fn footer(parent: &mut ChildSpawnerCommands, font: &UiFont, hint: &str) {
    parent.spawn((
        Text::new(hint.to_owned()),
        font.text(SMALL),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
        Node {
            margin: UiRect::top(px(FOOTER_GAP)),
            ..default()
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hovering_a_row_selects_it() {
        // The song browser ignored hover entirely: the pointer could
        // sit on a row while a different one stayed highlighted.
        let rows = [(0, &Interaction::None), (1, &Interaction::Hovered)];
        let pointer = read_rows(rows.into_iter());
        assert_eq!(pointer.hovered, Some(1));
        assert!(!pointer.clicked, "hovering must not activate");
    }

    #[test]
    fn clicking_activates_the_row_under_the_pointer() {
        // Not "the row that was already selected". The browser used
        // to need two clicks — the first only moved the selection.
        let rows = [(0, &Interaction::None), (2, &Interaction::Pressed)];
        let pointer = read_rows(rows.into_iter());
        assert_eq!(pointer.hovered, Some(2), "a press implies a hover");
        assert!(pointer.clicked);
    }

    #[test]
    fn an_untouched_list_changes_nothing() {
        // Keyboard navigation must survive a frame in which the mouse
        // is simply lying still somewhere off the list.
        let rows = [(0, &Interaction::None), (1, &Interaction::None)];
        assert_eq!(read_rows(rows.into_iter()), RowPointer::default());
        assert_eq!(read_rows(std::iter::empty()), RowPointer::default());
    }

    #[test]
    fn selection_differs_from_idle_in_every_channel() {
        // Colour alone is a weak cue on a dark background, and one of
        // the screens this replaced signalled selection ONLY by
        // changing the text colour.
        let idle = row_style(RowState::Idle);
        let selected = row_style(RowState::Selected);
        assert_ne!(idle.background, selected.background, "fill must change");
        assert_ne!(idle.accent, selected.accent, "accent bar must appear");
        assert_ne!(idle.label, selected.label, "label must change");
    }

    #[test]
    fn an_idle_row_stays_visible() {
        // A row that is not selected still has to be readable — it is
        // the thing the player is navigating towards.
        let idle = row_style(RowState::Idle);
        let linear = idle.label.to_linear();
        let brightness = linear.red.max(linear.green).max(linear.blue);
        assert!(brightness > 0.4, "idle label too dark: {brightness}");
        assert!(
            idle.label.alpha() > 0.9,
            "idle label must not be transparent"
        );
    }

    #[test]
    fn arming_is_not_mistaken_for_selection() {
        // Capturing a binding is a modal moment: the row is waiting
        // for a keypress and must not look like an ordinary highlight.
        let selected = row_style(RowState::Selected);
        let armed = row_style(RowState::Armed);
        assert_ne!(selected.accent, armed.accent);
        assert_ne!(selected.label, armed.label);
    }

    #[test]
    fn state_for_maps_the_two_questions() {
        assert_eq!(state_for(false, false), RowState::Idle);
        assert_eq!(state_for(true, false), RowState::Selected);
        assert_eq!(state_for(true, true), RowState::Armed);
        // An unselected row can never be armed: capture belongs to
        // the cursor row, so this combination must not sneak through.
        assert_eq!(state_for(false, true), RowState::Idle);
    }

    #[test]
    fn the_panel_fits_the_longest_settings_row() {
        // Press Start 2P is fixed-width at roughly 0.6 em per glyph.
        // "TAP MODE (NO STRUM)" + "8-BIT SHAPES" is the widest pair
        // the settings screen can produce; it has to fit inside the
        // panel with its padding and the gap between the columns.
        let glyphs = "TAP MODE (NO STRUM)".len() + "8-BIT SHAPES".len();
        let text_width = glyphs as f32 * ROW * 0.6;
        let chrome = 2.0 * PANEL_PAD + 2.0 * ROW_PAD_X + ACCENT_WIDTH + 16.0;
        assert!(
            text_width + chrome <= PANEL_WIDTH,
            "widest row needs {:.0} px, panel is {PANEL_WIDTH} px",
            text_width + chrome
        );
    }

    #[test]
    fn the_type_scale_has_no_near_duplicates() {
        // Sizes that differ by a point or two read as a mistake, and
        // the screens this replaced used 11/12/13/14 px for rows that
        // do the same job, plus 9 px and 10 px for two kinds of note.
        let mut sizes = [WORDMARK, TITLE, ROW, SMALL];
        sizes.sort_by(f32::total_cmp);
        for pair in sizes.windows(2) {
            let ratio = pair[1] / pair[0];
            assert!(
                ratio >= 1.2,
                "{} and {} are too close to tell apart",
                pair[0],
                pair[1]
            );
        }
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::scroll_to_show;

    /// Twenty rows of 24 px in a 120 px window: five fit, content is
    /// 480 tall, so the furthest the view can travel is 360.
    const ROW: f32 = 24.0;
    const VIEW: f32 = 120.0;
    const CONTENT: f32 = 480.0;

    fn top_of(row: usize) -> f32 {
        row as f32 * ROW
    }

    #[test]
    fn a_visible_row_does_not_move_the_view() {
        // The important one. A list that re-centred every frame would
        // twitch under the cursor and make its neighbours unreadable.
        for row in 0..5 {
            let now = scroll_to_show(top_of(row), ROW, VIEW, CONTENT, 0.0);
            assert!(
                (now - 0.0).abs() < f32::EPSILON,
                "row {row} is already visible but the view moved to {now}"
            );
        }
    }

    #[test]
    fn a_row_below_the_fold_comes_to_the_bottom_edge() {
        // Row 5 is the first one out of sight: its bottom is 144, so
        // the view has to travel 24 to end at 144.
        assert!((scroll_to_show(top_of(5), ROW, VIEW, CONTENT, 0.0) - 24.0).abs() < 1e-4);
    }

    #[test]
    fn a_row_above_the_fold_comes_to_the_top_edge() {
        assert!((scroll_to_show(top_of(2), ROW, VIEW, CONTENT, 200.0) - 48.0).abs() < 1e-4);
    }

    #[test]
    fn the_first_row_shows_the_top_of_the_list() {
        assert!((scroll_to_show(top_of(0), ROW, VIEW, CONTENT, 300.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_last_row_stops_at_the_end_of_the_list() {
        // Not past it: below the last row there is nothing to look at,
        // and a list that scrolls into blank space reads as broken.
        let now = scroll_to_show(top_of(19), ROW, VIEW, CONTENT, 0.0);
        assert!((now - 360.0).abs() < 1e-4, "stopped at {now}, not 360");
    }

    #[test]
    fn a_list_that_fits_never_scrolls() {
        // Three rows in a window that holds five. Every row is
        // visible, and there is nowhere to go.
        for row in 0..3 {
            let now = scroll_to_show(top_of(row), ROW, VIEW, 72.0, 0.0);
            assert!(
                (now - 0.0).abs() < f32::EPSILON,
                "row {row} scrolled to {now}"
            );
        }
    }

    #[test]
    fn a_stale_index_cannot_scroll_into_blank_space() {
        // The caller derives `row_top` from the cursor and `content_h`
        // from the library, and for one frame after an import removes
        // or reorders songs those two disagree. Without the clamp the
        // view would jump below the last row into nothing.
        //
        // This is the case the clamp exists for: for a row that really
        // IS the last one, the arithmetic already lands exactly on the
        // limit, so a test using that row passes with the clamp
        // deleted - which is how this one came to be written.
        let now = scroll_to_show(top_of(40), ROW, VIEW, CONTENT, 0.0);
        assert!(
            (now - 360.0).abs() < 1e-4,
            "a cursor past the end scrolled to {now}, past the limit of 360"
        );
    }

    #[test]
    fn a_row_taller_than_the_window_shows_its_top() {
        // Degenerate, but it must not send the offset off to infinity:
        // with the row taller than the view both branches want to
        // move, and the clamp is what keeps the answer sane.
        let now = scroll_to_show(0.0, 200.0, VIEW, CONTENT, 0.0);
        assert!(now.is_finite() && (0.0..=360.0).contains(&now), "got {now}");
    }
}

#[cfg(test)]
mod whole_row_tests {
    use super::{PANEL_BORDER, PANEL_MAX_H, PANEL_PAD, whole_rows_height};

    const ROW: f32 = 24.0;
    const GAP: f32 = 4.0;

    /// How many rows a returned height actually shows.
    fn rows_in(height: f32) -> f32 {
        ((height - 2.0 * (PANEL_PAD + PANEL_BORDER)) + GAP) / (ROW + GAP)
    }

    #[test]
    fn the_window_holds_a_whole_number_of_rows() {
        // The defect this exists for: a window that is not a multiple
        // of the pitch cuts its last row through the letters.
        let height = whole_rows_height(ROW, GAP, 40, PANEL_MAX_H).expect("40 rows must scroll");
        let rows = rows_in(height);
        assert!(
            (rows - rows.round()).abs() < 1e-3,
            "the window shows {rows} rows, not a whole number"
        );
    }

    #[test]
    fn the_window_never_exceeds_the_ceiling() {
        let height = whole_rows_height(ROW, GAP, 40, PANEL_MAX_H).expect("40 rows must scroll");
        assert!(height <= PANEL_MAX_H, "{height} is taller than the ceiling");
    }

    #[test]
    fn a_list_that_fits_is_left_alone() {
        // Forcing a ceiling on a short list would clip something that
        // had no reason to scroll.
        assert!(whole_rows_height(ROW, GAP, 3, PANEL_MAX_H).is_none());
    }

    #[test]
    fn the_window_is_packed_as_full_as_it_will_go() {
        // n rows have n-1 gaps between them, so counting a gap for
        // every row undercounts and wastes very nearly a whole row of
        // panel. Stated as the property rather than as a comparison
        // against the wrong formula: ONE MORE ROW MUST NOT FIT.
        //
        // Written that way after the first version compared the count
        // against the naive one with `>=` and stayed green when the
        // naive count was substituted - at these dimensions the two
        // happen to agree, so the test could never have failed.
        //
        // Several ceilings, because whether the two formulas differ
        // depends on where the ceiling falls between two rows.
        for ceiling in [PANEL_MAX_H, 393.2, 300.0, 250.0, 187.5] {
            let height = whole_rows_height(ROW, GAP, 40, ceiling).expect("40 rows scroll");
            let shown = rows_in(height).round();
            let one_more =
                (shown + 1.0).mul_add(ROW + GAP, -GAP) + 2.0 * (PANEL_PAD + PANEL_BORDER);
            assert!(
                one_more > ceiling + 1e-3,
                "at ceiling {ceiling} the window shows {shown} rows but {one_more} still fits"
            );
        }
    }

    #[test]
    fn a_degenerate_row_height_changes_nothing() {
        // Rows have no measured height on the first frame, and
        // dividing by it would send the answer to infinity.
        assert!(whole_rows_height(0.0, GAP, 40, PANEL_MAX_H).is_none());
        assert!(whole_rows_height(ROW, GAP, 0, PANEL_MAX_H).is_none());
    }

    #[test]
    fn at_least_one_row_survives_a_tiny_ceiling() {
        // A panel showing zero rows is worse than one that overflows.
        let height = whole_rows_height(ROW, GAP, 40, 10.0).expect("still shows something");
        assert!(rows_in(height).round() >= 1.0);
    }
}
