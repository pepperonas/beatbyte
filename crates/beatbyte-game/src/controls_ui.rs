//! The controls screen: view and remap every binding.
//!
//! Enter on a row arms capture mode — the next key or gamepad button
//! becomes an additional binding for that action (stolen from any
//! action that had it). Backspace restores the row's defaults.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::Settings;
use crate::controls::{Binding, GameAction, InputMap, MenuNav, UiAction};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Cursor + capture state.
#[derive(Resource, Default)]
struct ControlsState {
    cursor: usize,
    capturing: bool,
    /// A captured binding that CONFLICTS with another action, held
    /// until the player presses it again to confirm the move. The
    /// string is the current owner's label, for the hint line.
    pending: Option<(Binding, String)>,
}

/// What a row on this screen rebinds: a game action or a menu one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAction {
    /// A gameplay action.
    Game(GameAction),
    /// A menu-navigation action.
    Ui(UiAction),
}

/// Every row, in display order: the game actions, then navigation.
fn row_actions() -> Vec<RowAction> {
    GameAction::ALL
        .iter()
        .map(|a| RowAction::Game(*a))
        .chain(UiAction::ALL.iter().map(|a| RowAction::Ui(*a)))
        .collect()
}

impl RowAction {
    fn label(self) -> String {
        match self {
            RowAction::Game(action) => action.label(),
            RowAction::Ui(action) => action.label().to_owned(),
        }
    }

    /// The action (in the SAME table) that currently owns a binding,
    /// as a label — `None` when the binding is free or already ours.
    /// Conflicts are per table on purpose: A may be Fret 1 in play
    /// and NavLeft in menus at once.
    fn conflict_with(self, map: &InputMap, binding: Binding) -> Option<String> {
        match self {
            RowAction::Game(action) => map
                .owner_of(binding)
                .filter(|owner| *owner != action)
                .map(GameAction::label),
            RowAction::Ui(action) => map
                .ui_owner_of(binding)
                .filter(|owner| *owner != action)
                .map(|owner| owner.label().to_owned()),
        }
    }

    fn rebind(self, map: &mut InputMap, binding: Binding) {
        match self {
            RowAction::Game(action) => map.rebind(action, binding),
            RowAction::Ui(action) => map.rebind_ui(action, binding),
        }
    }

    fn reset(self, map: &mut InputMap) {
        match self {
            RowAction::Game(action) => map.reset_action(action),
            RowAction::Ui(action) => map.reset_ui_action(action),
        }
    }

    fn bindings(self, map: &InputMap) -> String {
        let list = match self {
            RowAction::Game(action) => map.of(action),
            RowAction::Ui(action) => map.ui_of(action),
        };
        list.iter()
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// Plugin for the controls screen.
pub struct ControlsUiPlugin;

impl Plugin for ControlsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlsState>()
            .add_systems(OnEnter(AppState::Controls), spawn_controls)
            .add_systems(
                Update,
                (
                    controls_input,
                    refresh_controls,
                    refresh_pad_tester,
                    follow_bindings_cursor,
                )
                    .run_if(in_state(AppState::Controls)),
            )
            .add_systems(OnExit(AppState::Controls), (persist_map, despawn_controls));
    }
}

#[derive(Component)]
struct ControlsScreen;

/// One action row (index into [`GameAction::ALL`]). Carries `Button`,
/// so this screen finally answers the mouse like every other one.
#[derive(Component)]
struct ActionRow(usize);

/// A row's action name.
#[derive(Component)]
struct ActionLabel(usize);

/// A row's current bindings.
#[derive(Component)]
struct ActionBindings(usize);

/// The status/hint line.
#[derive(Component)]
struct HintLine;

fn spawn_controls(mut commands: Commands, font: Res<UiFont>, mut state: ResMut<ControlsState>) {
    state.capturing = false;
    state.pending = None;
    commands
        .spawn((ControlsScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "CONTROLS", "every action, on any device");
            // Fifteen rows outgrow the safe area (the screenshot that
            // proved it clipped the title AND the footer), so the list
            // scrolls like the song browser and the cursor drags the
            // window along. The menu rows carry their own "MENU"
            // prefix - a mid-list caption would break the uniform row
            // pitch the scroll math relies on.
            parent
                .spawn((BindingsList, ui_kit::scroll_panel(ui_kit::PANEL_WIDTH)))
                .with_children(|panel| {
                    for (index, action) in row_actions().into_iter().enumerate() {
                        panel
                            .spawn((ActionRow(index), Button, ui_kit::row()))
                            .with_children(|row| {
                                row.spawn((
                                    ActionLabel(index),
                                    Text::new(action.label()),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::label_node(),
                                ));
                                row.spawn((
                                    ActionBindings(index),
                                    Text::new(""),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::value_node(),
                                ));
                            });
                    }
                });
            // Device diagnostics: which pads are connected, and five
            // live fret lamps — press a fret on your controller and
            // watch it light up. This exists because a real guitar
            // was plugged in and there was no way to SEE it working.
            parent.spawn((
                PadLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(16)),
                    ..default()
                },
            ));
            parent
                .spawn(Node {
                    column_gap: px(14),
                    margin: UiRect::top(px(8)),
                    ..default()
                })
                .with_children(|lamps| {
                    for fret in 0..5u8 {
                        lamps.spawn((
                            FretLamp(fret),
                            Node {
                                width: px(26),
                                height: px(26),
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::all(px(13)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.5)),
                        ));
                    }
                });
            parent.spawn((
                HintLine,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                Node {
                    margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
                    ..default()
                },
            ));
        });
}

/// The scrolling list of binding rows.
#[derive(Component)]
struct BindingsList;

/// Keep the cursor row in view, exactly the way the song browser
/// does: measured row height, whole-row window, minimal travel.
fn follow_bindings_cursor(
    state: Res<ControlsState>,
    rows: Query<(&ActionRow, &ComputedNode)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition, &mut Node), With<BindingsList>>,
) {
    let Ok((list, mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row_h) = rows
        .iter()
        .map(|(_, node)| node.size().y)
        .find(|height| *height > 0.0)
    else {
        return;
    };
    let count = row_actions().len();
    let pitch = row_h + ui_kit::ROW_GAP;
    if let Some(height) =
        ui_kit::whole_rows_height(row_h, ui_kit::ROW_GAP, count, ui_kit::PANEL_MAX_H)
    {
        let wanted = px(height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
    }
    let total = count as f32;
    let content_h = total.mul_add(row_h, (total - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = list.size().y - 2.0 * ui_kit::PANEL_PAD;
    let row_top = state.cursor as f32 * pitch;
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

/// The connected-devices line.
#[derive(Component)]
struct PadLine;

/// One live fret-test lamp (0 = green .. 4 = orange).
#[derive(Component)]
struct FretLamp(u8);

/// Show connected pads and light the lamps from LIVE input — through
/// the real InputMap, so this validates the whole chain.
fn refresh_pad_tester(
    pads: Query<(&Name, &bevy::input::gamepad::Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<InputMap>,
    mut line: Query<&mut Text, With<PadLine>>,
    mut lamps: Query<(&FretLamp, &mut BackgroundColor)>,
) {
    if let Ok(mut text) = line.single_mut() {
        let names: Vec<String> = pads.iter().map(|(name, _)| name.to_string()).collect();
        let wanted = if names.is_empty() {
            "no controller connected - keyboard ready".to_owned()
        } else {
            format!("connected: {}", names.join(", "))
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let sources = crate::controls::InputSources {
        keys: &keys,
        pads: pads.iter().map(|(_, pad)| pad).collect(),
    };
    for (lamp, mut color) in &mut lamps {
        let pressed = sources.pressed(&map, GameAction::Fret(lamp.0));
        color.0 = if pressed {
            palette::LANES[lamp.0 as usize]
        } else {
            Color::NONE
        };
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn controls_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    rows: Query<(&ActionRow, &Interaction), Changed<Interaction>>,
    mut state: ResMut<ControlsState>,
    mut map: ResMut<InputMap>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let actions = row_actions();
    let count = actions.len();
    if state.capturing {
        // Escape (or right-click) cancels the capture; anything
        // else binds. Mouse buttons are not bindable, so a click can
        // never BE the captured input.
        if keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
            state.capturing = false;
            state.pending = None;
            sounds.write(crate::sfx::UiSound::Back);
            return;
        }
        let captured = keys
            .get_just_pressed()
            .next()
            .map(|key| Binding::Key(*key))
            .or_else(|| {
                pads.iter()
                    .flat_map(|pad| pad.get_just_pressed())
                    .next()
                    .map(|button| Binding::Pad(*button))
            });
        if let Some(binding) = captured {
            let action = actions[state.cursor];
            // A binding that already serves another action is not
            // stolen silently: the row names the owner and waits for
            // the SAME press again as confirmation. Any other press
            // starts the check over on the new binding.
            let confirmed = state.pending.as_ref().is_some_and(|(b, _)| *b == binding);
            match action.conflict_with(&map, binding) {
                Some(owner) if !confirmed => {
                    state.pending = Some((binding, owner));
                    sounds.write(crate::sfx::UiSound::Error);
                }
                _ => {
                    action.rebind(&mut map, binding);
                    state.capturing = false;
                    state.pending = None;
                    sounds.write(crate::sfx::UiSound::Toggle);
                }
            }
        }
        return;
    }

    // Navigation goes through MenuNav like every other screen. Reading
    // the arrow keys directly, as this screen used to, meant a player
    // holding a guitar could not reach the screen that rebinds it.
    let nav = MenuNav::read(&map, &keys, pads.iter());
    if nav.up {
        state.cursor = (state.cursor + count - 1) % count;
    }
    if nav.down {
        state.cursor = (state.cursor + 1) % count;
    }
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        state.cursor = index;
    }
    let clicked = pointer.clicked;
    if nav.confirm || clicked {
        state.capturing = true;
        state.pending = None;
        sounds.write(crate::sfx::UiSound::Confirm);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        actions[state.cursor].reset(&mut map);
        sounds.write(crate::sfx::UiSound::Toggle);
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::Settings);
    }
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn refresh_controls(
    map: Res<InputMap>,
    state: Res<ControlsState>,
    mut rows: Query<(&ActionRow, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&ActionLabel, &mut TextColor), Without<ActionBindings>>,
    mut bindings: Query<(&ActionBindings, &mut Text, &mut TextColor), Without<ActionLabel>>,
    mut hint: Query<&mut Text, (With<HintLine>, Without<ActionBindings>)>,
) {
    let style_of = |index: usize| {
        ui_kit::row_style(ui_kit::state_for(
            index == state.cursor,
            state.capturing && index == state.cursor,
        ))
    };
    for (row, mut background, mut border) in &mut rows {
        let style = style_of(row.0);
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 = style_of(label.0).label;
    }
    let actions = row_actions();
    for (row, mut text, mut color) in &mut bindings {
        let wanted = if state.capturing && row.0 == state.cursor {
            "press a key or button...".to_owned()
        } else {
            actions[row.0].bindings(&map)
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = style_of(row.0).value;
    }
    if let Ok(mut text) = hint.single_mut() {
        let line = match (&state.pending, state.capturing) {
            (Some((binding, owner)), true) => format!(
                "{} is {owner} - press it again to move it  ESC keep it",
                binding.label()
            ),
            (None, true) => "press the new key or button  ESC cancel".to_owned(),
            _ => "UP/DOWN choose  ENTER rebind  BACKSPACE reset  ESC back".to_owned(),
        };
        if text.0 != line {
            text.0 = line;
        }
    }
}

fn persist_map(map: Res<InputMap>, mut settings: ResMut<Settings>) {
    settings.input_map = map.clone();
    crate::config::save_settings(&settings);
}

fn despawn_controls(mut commands: Commands, entities: Query<Entity, With<ControlsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_names_the_owner_and_self_is_never_a_conflict() {
        let map = InputMap::default();
        // Space serves StrumDown; capturing it for Fret 1 conflicts.
        let space = Binding::Key(KeyCode::Space);
        assert_eq!(
            RowAction::Game(GameAction::Fret(0)).conflict_with(&map, space),
            Some("STRUM DOWN".to_owned())
        );
        // Re-capturing an action's own binding is not a conflict.
        assert_eq!(
            RowAction::Game(GameAction::StrumDown).conflict_with(&map, space),
            None
        );
    }

    #[test]
    fn game_and_menu_tables_do_not_conflict_with_each_other() {
        // A is Fret 1 in play AND NavLeft in menus - by design.
        let map = InputMap::default();
        let a = Binding::Key(KeyCode::KeyA);
        assert_eq!(
            RowAction::Ui(UiAction::NavLeft).conflict_with(&map, a),
            None,
            "NavLeft owns A in its own table; Fret 1 owning it in the game table is no conflict"
        );
        // But WITHIN the menu table it is one.
        assert_eq!(
            RowAction::Ui(UiAction::Confirm).conflict_with(&map, a),
            Some("MENU LEFT".to_owned())
        );
    }

    #[test]
    fn the_rows_list_every_action_of_both_tables() {
        let rows = row_actions();
        assert_eq!(rows.len(), GameAction::ALL.len() + UiAction::ALL.len());
        for action in GameAction::ALL {
            assert!(rows.contains(&RowAction::Game(action)));
        }
        for action in UiAction::ALL {
            assert!(rows.contains(&RowAction::Ui(action)));
        }
    }
}
