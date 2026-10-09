//! The controls screen: view and remap every binding.
//!
//! Enter on a row arms capture mode — the next key or gamepad button
//! becomes an additional binding for that action (stolen from any
//! action that had it). Backspace restores the row's defaults.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::Settings;
use crate::controls::{Binding, GameAction, InputMap, UiAction};
use crate::menu_list::list::{self, ListInput, ListPaint, ListPanel, ListRow, ListValue};
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
    /// A capture ended this frame (bound or cancelled). The key that
    /// ended it — Escape, a right click — must not ALSO leave the
    /// screen through the navigation that runs after it.
    capture_ended: bool,
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
            .init_resource::<ActionBarClicks>()
            .add_systems(OnEnter(AppState::Controls), spawn_controls)
            .add_systems(
                Update,
                (
                    paint_action_bar.before(controls_edit),
                    (
                        controls_edit,
                        controls_input,
                        refresh_controls,
                        refresh_pad_tester,
                        follow_bindings_cursor,
                    )
                        .chain(),
                )
                    .run_if(in_state(AppState::Controls)),
            )
            // The screen goes with the state (`DespawnOnExit`).
            .add_systems(OnExit(AppState::Controls), persist_map);
    }
}

mod chip {
    pub const RESET: u8 = 0;
}

#[derive(Resource, Default)]
struct ActionBarClicks(Vec<u8>);

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

/// The marker of the binding rows (the shared list renderer).
struct BindingRows;

/// The status/hint line.
#[derive(Component)]
struct HintLine;

fn spawn_controls(mut commands: Commands, font: Res<UiFont>, mut state: ResMut<ControlsState>) {
    state.capturing = false;
    state.pending = None;
    state.capture_ended = false;
    commands
        .spawn((DespawnOnExit(AppState::Controls), ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "CONTROLS", "every action, on any device");
            ui_kit::action_bar(
                parent,
                &font,
                &[ui_kit::ChipSpec {
                    id: chip::RESET,
                    label: "Reset defaults",
                    enabled: true,
                }],
            );
            // Fifteen rows outgrow the safe area (the screenshot that
            // proved it clipped the title AND the footer), so the list
            // scrolls like the song browser and the cursor drags the
            // window along. The menu rows carry their own "MENU"
            // prefix - a mid-list caption would break the uniform row
            // pitch the scroll math relies on.
            parent
                .spawn((
                    ListPanel::<BindingRows>::new(),
                    ui_kit::scroll_panel(ui_kit::PANEL_WIDTH),
                ))
                .with_children(|panel| {
                    let labels: Vec<String> = row_actions()
                        .iter()
                        .map(|action| action.label().to_owned())
                        .collect();
                    list::spawn_rows::<BindingRows>(
                        panel,
                        &font,
                        labels.iter().map(String::as_str),
                    );
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
            ui_kit::back_button(parent, &font, "SETTINGS");
        });
}

/// Keep the cursor row in view, exactly the way the song browser
/// does: measured row height, whole-row window, minimal travel.
fn follow_bindings_cursor(
    state: Res<ControlsState>,
    rows: Query<(&ListRow<BindingRows>, &ComputedNode)>,
    mut lists: Query<(&mut ScrollPosition, &mut Node), With<ListPanel<BindingRows>>>,
) {
    if !state.is_changed() {
        return;
    }
    list::follow_cursor(state.cursor, row_actions().len(), &rows, &mut lists);
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

/// What edits the map: capturing a new binding, and resetting a row.
/// Separate from the navigation because it writes the `InputMap` the
/// list renderer reads.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn controls_edit(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    clicks: Res<ActionBarClicks>,
    mut state: ResMut<ControlsState>,
    mut map: ResMut<InputMap>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let actions = row_actions();
    if !state.capturing {
        if keys.just_pressed(KeyCode::Backspace) || ui_kit::chip_hit(&clicks.0, chip::RESET) {
            actions[state.cursor].reset(&mut map);
            sounds.write(crate::sfx::UiSound::Toggle);
        }
        return;
    }
    // Escape (or right-click) cancels the capture; anything else
    // binds. Mouse buttons are not bindable, so a click can never BE
    // the captured input.
    if keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        state.capturing = false;
        state.pending = None;
        state.capture_ended = true;
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
        // A binding that already serves another action is not stolen
        // silently: the row names the owner and waits for the SAME
        // press again as confirmation. Any other press starts the
        // check over on the new binding.
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
                state.capture_ended = true;
                sounds.write(crate::sfx::UiSound::Toggle);
            }
        }
    }
}

/// Navigation through the shared list renderer — `MenuNav` like every
/// other screen: reading the arrow keys directly, as this screen once
/// did, meant a player holding a guitar could not reach the screen
/// that rebinds it.
fn controls_input(
    mut list: ListInput<BindingRows>,
    mut state: ResMut<ControlsState>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut back_button: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
) {
    if std::mem::take(&mut state.capture_ended) || state.capturing {
        return;
    }
    let mut cursor = state.cursor;
    let events = list.read(&mut cursor, row_actions().len());
    state.cursor = cursor;
    if let Some(sound) = list::sound_for(None, events.moved) {
        sounds.write(sound);
    }
    if events.nav.confirm || events.clicked {
        state.capturing = true;
        state.pending = None;
        sounds.write(crate::sfx::UiSound::Confirm);
    }
    if ui_kit::wants_leave(
        events.nav.back,
        ui_kit::back_pressed(&mut back_button),
        events.right_click,
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::Settings);
    }
}

/// The hint line, kept apart from the list's own texts.
type HintOnly = (
    With<HintLine>,
    Without<ListValue<BindingRows>>,
    Without<list::ListLabel<BindingRows>>,
);

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn refresh_controls(
    map: Res<InputMap>,
    settings: Res<Settings>,
    state: Res<ControlsState>,
    mut paint: ListPaint<BindingRows>,
    mut hint: Query<&mut Text, HintOnly>,
    active: Res<crate::prompts::ActiveDevice>,
    fresh: Query<(), Added<ListRow<BindingRows>>>,
) {
    let actions = row_actions();
    let armed = state.capturing.then_some(state.cursor);
    if !fresh.is_empty() || map.is_changed() || settings.is_changed() || state.is_changed() {
        paint.paint_armed(
            state.cursor,
            armed,
            settings.high_contrast,
            usize::MAX,
            |index| {
                let value = if armed == Some(index) {
                    "press a key or button...".to_owned()
                } else {
                    actions[index].bindings(&map)
                };
                (None, value)
            },
        );
    }
    if let Ok(mut text) = hint.single_mut() {
        let idle = match *active {
            crate::prompts::ActiveDevice::Keyboard => {
                "UP/DOWN choose  ENTER rebind  Reset chip or BACKSPACE  ESC back"
            }
            crate::prompts::ActiveDevice::Gamepad => "D-PAD choose  SOUTH rebind  EAST back",
        };
        let line = match (&state.pending, state.capturing) {
            (Some((binding, owner)), true) => format!(
                "{} is {owner} - press it again to move it  ESC keep it",
                binding.label()
            ),
            (None, true) => "press the new key or button  ESC cancel".to_owned(),
            _ => idle.to_owned(),
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

    /// The two input systems, wired as the plugin wires them.
    fn wired(capturing: bool) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<InputMap>()
            .init_resource::<ActionBarClicks>()
            .init_resource::<NextState<AppState>>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_message::<bevy::window::CursorMoved>()
            .add_message::<crate::sfx::UiSound>()
            .insert_resource(ControlsState {
                capturing,
                ..Default::default()
            })
            .add_systems(Update, (controls_edit, controls_input).chain());
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    fn leaving(app: &App) -> bool {
        matches!(
            app.world().resource::<NextState<AppState>>(),
            NextState::Pending(_)
        )
    }

    #[test]
    fn the_escape_that_cancels_a_capture_does_not_also_leave() {
        let mut app = wired(true);
        press(&mut app, KeyCode::Escape);
        assert!(!app.world().resource::<ControlsState>().capturing);
        assert!(!leaving(&app), "one Escape left the screen as well");
        // The next Escape is an ordinary one again.
        press(&mut app, KeyCode::Escape);
        assert!(leaving(&app));
    }

    #[test]
    fn a_captured_key_is_bound_and_does_not_navigate() {
        let mut app = wired(true);
        press(&mut app, KeyCode::KeyJ);
        let state = app.world().resource::<ControlsState>();
        assert!(!state.capturing);
        assert_eq!(state.cursor, 0);
        assert!(!leaving(&app));
    }
}
