//! One list renderer: the part of every "label + value" menu that
//! used to be written again per screen.
//!
//! A screen keeps what is its own — its frame, its title, what a row
//! DOES — and hands this module the rest: spawning the rows, reading
//! the cursor from keyboard, pad, guitar, wheel and pointer, painting
//! the row states, writing the values, keeping the cursor in view and
//! choosing the feedback sound. The settings screen and the pause menu
//! were two copies of all of that with small differences; now they are
//! two callers of this.
//!
//! The components are generic over a marker type `L` (one per list),
//! so two lists never answer each other's queries.

use std::marker::PhantomData;

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::Gamepad;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::CursorMoved;

use crate::controls::{InputMap, MenuNav};
use crate::palette;
use crate::sfx::UiSound;
use crate::ui::UiFont;
use crate::ui_kit;

use super::spec::Feel;

/// A row of list `L` (index into the screen's rows); carries `Button`.
#[derive(Component)]
pub struct ListRow<L: Send + Sync + 'static>(pub usize, PhantomData<L>);

/// A row's label text — written once at spawn.
#[derive(Component)]
pub struct ListLabel<L: Send + Sync + 'static>(pub usize, PhantomData<L>);

/// A row's value text — the part that changes.
#[derive(Component)]
pub struct ListValue<L: Send + Sync + 'static>(pub usize, PhantomData<L>);

/// The scrolling panel that holds list `L`'s rows.
#[derive(Component)]
pub struct ListPanel<L: Send + Sync + 'static>(PhantomData<L>);

impl<L: Send + Sync + 'static> ListPanel<L> {
    /// The marker for list `L`'s panel.
    #[must_use]
    pub fn new() -> Self {
        ListPanel(PhantomData)
    }
}

impl<L: Send + Sync + 'static> Default for ListPanel<L> {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn one row per label into `panel`. Every row carries `Button`
/// and a `RelativeCursorPosition`, so a click on its left half can
/// step down and on its right half up.
pub fn spawn_rows<'a, L: Send + Sync + 'static>(
    panel: &mut ChildSpawnerCommands,
    font: &UiFont,
    labels: impl IntoIterator<Item = &'a str>,
) {
    for (index, label) in labels.into_iter().enumerate() {
        panel
            .spawn((
                ListRow::<L>(index, PhantomData),
                Button,
                RelativeCursorPosition::default(),
                ui_kit::row(),
            ))
            .with_children(|row| {
                // Label and value are separate texts in a space-between
                // line. A single padded string overflowed on the
                // longest label ("TAP MODE (NO STRUM)") and hung its
                // value out of the column.
                row.spawn((
                    ListLabel::<L>(index, PhantomData),
                    Text::new(label.to_owned()),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT_DIM),
                    ui_kit::label_node(),
                ));
                row.spawn((
                    ListValue::<L>(index, PhantomData),
                    Text::new(""),
                    font.text(ui_kit::ROW),
                    TextColor(palette::TEXT_DIM),
                    ui_kit::value_node(),
                ));
            });
    }
}

/// What the player did to a list this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ListEvents {
    /// The navigation keys as read, for what a screen does itself
    /// (Enter on a custom row, Back).
    pub nav: MenuNavCopy,
    /// The cursor moved (keys, wheel or pointer).
    pub moved: bool,
    /// The selected row was clicked.
    pub clicked: bool,
    /// The step the input asks of the selected row, −1 or +1, if any:
    /// LEFT steps down; RIGHT and ENTER step up; a click steps down
    /// on the row's left half and up on its right half.
    pub step: Option<i32>,
    /// The right mouse button went down (a way back out).
    pub right_click: bool,
}

/// The parts of [`MenuNav`] a list screen reads, as plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // six independent keys
pub struct MenuNavCopy {
    /// Up.
    pub up: bool,
    /// Down.
    pub down: bool,
    /// Left.
    pub left: bool,
    /// Right.
    pub right: bool,
    /// Confirm.
    pub confirm: bool,
    /// Back.
    pub back: bool,
}

/// The step the input asks of the selected row. Pure — tested.
///
/// LEFT wins over RIGHT and ENTER pressed in the same frame: the old
/// code applied both, which nets out to nothing on a slider and to
/// two flips on a toggle, and neither was ever meant.
#[must_use]
pub fn step_of(
    left: bool,
    right: bool,
    confirm: bool,
    click_x: Option<Option<f32>>,
) -> Option<i32> {
    if left {
        Some(-1)
    } else if right || confirm {
        Some(1)
    } else {
        // A click without a position yet steps up, as a plain click
        // always did.
        click_x.map(|x| match x {
            Some(x) if x < 0.5 => -1,
            _ => 1,
        })
    }
}

/// The sound for what happened. Pure — tested.
///
/// A step speaks with the row's feel (a switch clicks, a dial ticks);
/// a cursor move without a step is the navigation blip.
#[must_use]
pub fn sound_for(stepped: Option<Feel>, moved: bool) -> Option<UiSound> {
    match stepped {
        Some(Feel::Click) => Some(UiSound::Toggle),
        Some(Feel::Tick) => Some(UiSound::Slider),
        None if moved => Some(UiSound::Navigate),
        None => None,
    }
}

/// Everything a list reads its input from.
#[derive(SystemParam)]
pub struct ListInput<'w, 's, L: Send + Sync + 'static> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    map: Res<'w, InputMap>,
    pads: Query<'w, 's, &'static Gamepad>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    pointer_moved: MessageReader<'w, 's, CursorMoved>,
    rows: Query<'w, 's, (&'static ListRow<L>, &'static Interaction), Changed<Interaction>>,
    positions: Query<'w, 's, (&'static ListRow<L>, &'static RelativeCursorPosition)>,
}

impl<L: Send + Sync + 'static> ListInput<'_, '_, L> {
    /// Read this frame's input and move `cursor` over `count` rows.
    /// The cursor stops at both ends (`ui_kit::step_cursor`), the wheel
    /// scrolls rather than stepping a value (user report 2026-09-01),
    /// and the pointer selects only when it actually moved.
    pub fn read(&mut self, cursor: &mut usize, count: usize) -> ListEvents {
        let nav = MenuNav::read(&self.map, &self.keys, self.pads.iter());
        let mut moved = false;
        let mut step_cursor = |delta: i32, cursor: &mut usize| {
            *cursor = ui_kit::step_cursor(*cursor, count, delta);
            moved = true;
        };
        if nav.up {
            step_cursor(-1, cursor);
        }
        if nav.down {
            step_cursor(1, cursor);
        }
        for event in self.wheel.read() {
            if event.y > 0.0 {
                step_cursor(-1, cursor);
            } else if event.y < 0.0 {
                step_cursor(1, cursor);
            }
        }
        let pointer = ui_kit::read_rows(self.rows.iter().map(|(row, i)| (row.0, i)));
        let pointer_moved = self.pointer_moved.read().next().is_some();
        if let Some(index) = ui_kit::hover_moves_cursor(&pointer, pointer_moved) {
            *cursor = index;
        }
        let click_x = pointer.clicked.then(|| {
            self.positions
                .iter()
                .find(|(row, _)| row.0 == *cursor)
                .and_then(|(_, position)| position.normalized)
                .map(|p| p.x)
        });
        ListEvents {
            nav: MenuNavCopy {
                up: nav.up,
                down: nav.down,
                left: nav.left,
                right: nav.right,
                confirm: nav.confirm,
                back: nav.back,
            },
            moved,
            clicked: pointer.clicked,
            step: step_of(nav.left, nav.right, nav.confirm, click_x),
            right_click: self.mouse.just_pressed(MouseButton::Right),
        }
    }
}

/// The queries that paint list `L`.
#[derive(SystemParam)]
pub struct ListPaint<'w, 's, L: Send + Sync + 'static> {
    rows: Query<'w, 's, RowDress<L>>,
    labels: Query<'w, 's, (&'static ListLabel<L>, &'static mut TextColor), Without<ListValue<L>>>,
    values: Query<'w, 's, ValueText<L>, Without<ListLabel<L>>>,
}

/// A row's frame: its fill and its border.
type RowDress<L> = (
    &'static ListRow<L>,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
);

/// A value's text and colour.
type ValueText<L> = (
    &'static ListValue<L>,
    &'static mut Text,
    &'static mut TextColor,
);

impl<L: Send + Sync + 'static> ListPaint<'_, '_, L> {
    /// Dress every row for `cursor` and write each value `value(index)`
    /// gives — a text is only rewritten when it changed.
    pub fn paint(&mut self, cursor: usize, high_contrast: bool, value: impl Fn(usize) -> String) {
        let style = |index: usize| {
            ui_kit::styled_row(ui_kit::state_for(index == cursor, false), high_contrast)
        };
        for (row, mut background, mut border) in &mut self.rows {
            let style = style(row.0);
            background.0 = style.background;
            *border = BorderColor::all(style.accent);
        }
        for (label, mut color) in &mut self.labels {
            color.0 = style(label.0).label;
        }
        for (slot, mut text, mut color) in &mut self.values {
            let wanted = value(slot.0);
            if text.0 != wanted {
                text.0 = wanted;
            }
            color.0 = style(slot.0).value;
        }
    }
}

/// Keep the cursor row in view: the measured whole-row window the
/// browser and the controls screen use.
pub fn follow_cursor<L: Send + Sync + 'static>(
    cursor: usize,
    count: usize,
    rows: &Query<(&ListRow<L>, &ComputedNode)>,
    lists: &mut Query<(&mut ScrollPosition, &mut Node), With<ListPanel<L>>>,
) {
    let Ok((mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row) = rows
        .iter()
        .map(|(_, node)| node)
        .find(|node| node.size().y > 0.0)
    else {
        return;
    };
    ui_kit::follow_list(cursor, count, row, &mut scroll, &mut node);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_steps_down_and_right_or_enter_step_up() {
        assert_eq!(step_of(true, false, false, None), Some(-1));
        assert_eq!(step_of(false, true, false, None), Some(1));
        assert_eq!(step_of(false, false, true, None), Some(1));
        assert_eq!(step_of(false, false, false, None), None);
        // Left and right together: one step, not two that cancel.
        assert_eq!(step_of(true, true, false, None), Some(-1));
    }

    #[test]
    fn a_click_steps_by_the_half_it_lands_on() {
        assert_eq!(step_of(false, false, false, Some(Some(0.0))), Some(-1));
        assert_eq!(step_of(false, false, false, Some(Some(0.49))), Some(-1));
        assert_eq!(step_of(false, false, false, Some(Some(0.5))), Some(1));
        assert_eq!(step_of(false, false, false, Some(Some(1.0))), Some(1));
        // No position yet: a plain click steps up, as it always did.
        assert_eq!(step_of(false, false, false, Some(None)), Some(1));
    }

    #[test]
    fn a_switch_clicks_a_dial_ticks_and_a_move_blips() {
        assert_eq!(sound_for(Some(Feel::Click), true), Some(UiSound::Toggle));
        assert_eq!(sound_for(Some(Feel::Tick), false), Some(UiSound::Slider));
        assert_eq!(sound_for(None, true), Some(UiSound::Navigate));
        assert_eq!(sound_for(None, false), None);
    }
}
