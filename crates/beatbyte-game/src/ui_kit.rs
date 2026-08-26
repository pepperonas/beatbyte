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
const PANEL_PAD: f32 = 16.0;

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
            border: UiRect::all(px(1)),
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
            border: UiRect::all(px(1)),
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
