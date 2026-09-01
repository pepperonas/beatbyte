//! The input abstraction: physical inputs → game actions.
//!
//! ```text
//! Physical Input (key / gamepad button)
//!       ↓  Binding
//! Game Action (Fret3, StrumDown, Hype, …)
//!       ↓
//! Gameplay / Menus
//! ```
//!
//! Bindings are data (persisted with the settings), so remapping never
//! touches gameplay code. Guitar-style controllers register as
//! gamepads; the default pad layout matches the common guitar mapping
//! (frets on face buttons + left shoulder, strum on the D-pad).

use beatbyte_core::Lane;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Everything a player can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameAction {
    /// Hold a fret lane (0–4).
    Fret(u8),
    /// Strum up.
    StrumUp,
    /// Strum down.
    StrumDown,
    /// Activate Hype.
    Hype,
    /// Pause / back.
    Pause,
}

impl GameAction {
    /// All remappable actions, in display order.
    pub const ALL: [GameAction; 9] = [
        GameAction::Fret(0),
        GameAction::Fret(1),
        GameAction::Fret(2),
        GameAction::Fret(3),
        GameAction::Fret(4),
        GameAction::StrumUp,
        GameAction::StrumDown,
        GameAction::Hype,
        GameAction::Pause,
    ];

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            GameAction::Fret(index) => format!("FRET {}", index + 1),
            GameAction::StrumUp => "STRUM UP".to_owned(),
            GameAction::StrumDown => "STRUM DOWN".to_owned(),
            GameAction::Hype => "HYPE".to_owned(),
            GameAction::Pause => "PAUSE".to_owned(),
        }
    }

    /// The lane, when this is a fret action.
    #[must_use]
    pub fn lane(self) -> Option<Lane> {
        match self {
            GameAction::Fret(index) => Lane::from_index(index as usize),
            _ => None,
        }
    }
}

/// Everything a player can do in a MENU. Separate from
/// [`GameAction`] on purpose: a binding may serve a game action and
/// a UI action at once (A is Fret 1 in play and NavLeft in menus) —
/// different contexts, so conflicts are detected per table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UiAction {
    /// Move the cursor up.
    NavUp,
    /// Move the cursor down.
    NavDown,
    /// Move the cursor / value left.
    NavLeft,
    /// Move the cursor / value right.
    NavRight,
    /// Confirm / select.
    Confirm,
    /// Back / cancel.
    Back,
}

impl UiAction {
    /// All remappable UI actions, in display order.
    pub const ALL: [UiAction; 6] = [
        UiAction::NavUp,
        UiAction::NavDown,
        UiAction::NavLeft,
        UiAction::NavRight,
        UiAction::Confirm,
        UiAction::Back,
    ];

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            UiAction::NavUp => "MENU UP",
            UiAction::NavDown => "MENU DOWN",
            UiAction::NavLeft => "MENU LEFT",
            UiAction::NavRight => "MENU RIGHT",
            UiAction::Confirm => "CONFIRM",
            UiAction::Back => "BACK",
        }
    }
}

/// One physical trigger for an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Binding {
    /// A keyboard key.
    Key(KeyCode),
    /// A button on any connected gamepad.
    Pad(GamepadButton),
}

impl Binding {
    /// Short display text.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Binding::Key(key) => format!("{key:?}").replace("Key", "").replace("Arrow", ""),
            Binding::Pad(button) => format!("PAD {button:?}"),
        }
    }
}

/// The bindings table: each action can have several triggers.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct InputMap {
    /// Action → bindings.
    pub bindings: Vec<(GameAction, Vec<Binding>)>,
    /// Menu action → bindings. Defaulted so settings files from
    /// before this table existed keep loading.
    #[serde(default = "default_ui_bindings")]
    pub ui: Vec<(UiAction, Vec<Binding>)>,
}

/// The default menu bindings: arrows AND WASD navigate, Enter AND
/// Space confirm, Escape backs out — with the pad's D-pad, South and
/// East beside them. Enter and Escape additionally work as
/// hard-wired fallbacks in [`MenuNav::read`], so no rebinding can
/// strand the player in a menu.
fn default_ui_bindings() -> Vec<(UiAction, Vec<Binding>)> {
    use Binding::{Key, Pad};
    use GamepadButton as B;
    use KeyCode as K;
    vec![
        (
            UiAction::NavUp,
            vec![Key(K::ArrowUp), Key(K::KeyW), Pad(B::DPadUp)],
        ),
        (
            UiAction::NavDown,
            vec![Key(K::ArrowDown), Key(K::KeyS), Pad(B::DPadDown)],
        ),
        (
            UiAction::NavLeft,
            vec![Key(K::ArrowLeft), Key(K::KeyA), Pad(B::DPadLeft)],
        ),
        (
            UiAction::NavRight,
            vec![Key(K::ArrowRight), Key(K::KeyD), Pad(B::DPadRight)],
        ),
        (
            UiAction::Confirm,
            vec![Key(K::Enter), Key(K::Space), Pad(B::South)],
        ),
        (UiAction::Back, vec![Key(K::Escape), Pad(B::East)]),
    ]
}

impl Default for InputMap {
    fn default() -> Self {
        use Binding::{Key, Pad};
        use GamepadButton as B;
        use KeyCode as K;
        InputMap {
            bindings: vec![
                // Frets: home row + guitar-style face buttons
                // (green=South, red=East, yellow=North, blue=West,
                // orange=LeftTrigger — the common guitar layout).
                (GameAction::Fret(0), vec![Key(K::KeyA), Pad(B::South)]),
                (GameAction::Fret(1), vec![Key(K::KeyS), Pad(B::East)]),
                (GameAction::Fret(2), vec![Key(K::KeyD), Pad(B::North)]),
                (GameAction::Fret(3), vec![Key(K::KeyF), Pad(B::West)]),
                (GameAction::Fret(4), vec![Key(K::KeyG), Pad(B::LeftTrigger)]),
                (GameAction::StrumUp, vec![Key(K::ArrowUp), Pad(B::DPadUp)]),
                (
                    GameAction::StrumDown,
                    // Space strums too: with tap mode off, ASDFG +
                    // Space is the natural two-hand keyboard split.
                    vec![Key(K::ArrowDown), Key(K::Space), Pad(B::DPadDown)],
                ),
                (
                    GameAction::Hype,
                    vec![Key(K::Enter), Pad(B::Select), Pad(B::RightTrigger)],
                ),
                (GameAction::Pause, vec![Key(K::Escape), Pad(B::Start)]),
            ],
            ui: default_ui_bindings(),
        }
    }
}

impl InputMap {
    /// The bindings for an action.
    #[must_use]
    pub fn of(&self, action: GameAction) -> &[Binding] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, b)| b.as_slice())
    }

    /// Replace an action's bindings with a single new one (the default
    /// keyboard/pad pair is restored via [`InputMap::reset_action`]).
    pub fn rebind(&mut self, action: GameAction, binding: Binding) {
        // A binding may serve only one action: steal it if needed.
        for (_, bindings) in &mut self.bindings {
            bindings.retain(|b| *b != binding);
        }
        if let Some((_, bindings)) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
            bindings.push(binding);
        }
    }

    /// Restore an action's default bindings.
    pub fn reset_action(&mut self, action: GameAction) {
        let defaults = InputMap::default();
        if let Some((_, bindings)) = self.bindings.iter_mut().find(|(a, _)| *a == action) {
            *bindings = defaults.of(action).to_vec();
        }
    }

    /// The bindings for a menu action.
    #[must_use]
    pub fn ui_of(&self, action: UiAction) -> &[Binding] {
        self.ui
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, b)| b.as_slice())
    }

    /// The game action a binding currently serves, if any.
    #[must_use]
    pub fn owner_of(&self, binding: Binding) -> Option<GameAction> {
        self.bindings
            .iter()
            .find(|(_, b)| b.contains(&binding))
            .map(|(a, _)| *a)
    }

    /// The menu action a binding currently serves, if any.
    #[must_use]
    pub fn ui_owner_of(&self, binding: Binding) -> Option<UiAction> {
        self.ui
            .iter()
            .find(|(_, b)| b.contains(&binding))
            .map(|(a, _)| *a)
    }

    /// Add a binding to a menu action, stealing it from any other
    /// MENU action (game actions keep theirs — different context).
    pub fn rebind_ui(&mut self, action: UiAction, binding: Binding) {
        for (_, bindings) in &mut self.ui {
            bindings.retain(|b| *b != binding);
        }
        if let Some((_, bindings)) = self.ui.iter_mut().find(|(a, _)| *a == action) {
            bindings.push(binding);
        }
    }

    /// Restore a menu action's default bindings.
    pub fn reset_ui_action(&mut self, action: UiAction) {
        let defaults = default_ui_bindings();
        if let Some((_, bindings)) = self.ui.iter_mut().find(|(a, _)| *a == action) {
            *bindings = defaults
                .iter()
                .find(|(a, _)| *a == action)
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
        }
    }

    /// Drop actions/bindings that no longer parse (config files are
    /// input too) and re-add missing actions with defaults.
    pub fn sanitize(&mut self) {
        let defaults = InputMap::default();
        for action in GameAction::ALL {
            if !self.bindings.iter().any(|(a, _)| *a == action) {
                self.bindings.push((action, defaults.of(action).to_vec()));
            }
        }
        self.bindings.retain(|(a, _)| GameAction::ALL.contains(a));
        for action in UiAction::ALL {
            if !self.ui.iter().any(|(a, _)| *a == action) {
                self.ui.push((action, defaults.ui_of(action).to_vec()));
            }
        }
        self.ui.retain(|(a, _)| UiAction::ALL.contains(a));
    }
}

/// Whether a key types a character. During text entry (the browser
/// search) bindings on these keys are ignored, so W/A/S/D and Space
/// TYPE instead of navigating; arrows and the D-pad still work.
#[must_use]
pub fn key_is_printable(key: KeyCode) -> bool {
    use KeyCode as K;
    matches!(
        key,
        K::KeyA
            | K::KeyB
            | K::KeyC
            | K::KeyD
            | K::KeyE
            | K::KeyF
            | K::KeyG
            | K::KeyH
            | K::KeyI
            | K::KeyJ
            | K::KeyK
            | K::KeyL
            | K::KeyM
            | K::KeyN
            | K::KeyO
            | K::KeyP
            | K::KeyQ
            | K::KeyR
            | K::KeyS
            | K::KeyT
            | K::KeyU
            | K::KeyV
            | K::KeyW
            | K::KeyX
            | K::KeyY
            | K::KeyZ
            | K::Digit0
            | K::Digit1
            | K::Digit2
            | K::Digit3
            | K::Digit4
            | K::Digit5
            | K::Digit6
            | K::Digit7
            | K::Digit8
            | K::Digit9
            | K::Space
            | K::Minus
            | K::Period
            | K::Comma
    )
}

/// Read-side helper bundling every input source for one frame.
pub struct InputSources<'a> {
    /// Keyboard state.
    pub keys: &'a ButtonInput<KeyCode>,
    /// Every connected gamepad.
    pub pads: Vec<&'a Gamepad>,
}

impl InputSources<'_> {
    /// Was any binding of this action just pressed?
    #[must_use]
    pub fn just_pressed(&self, map: &InputMap, action: GameAction) -> bool {
        map.of(action).iter().any(|binding| match binding {
            Binding::Key(key) => self.keys.just_pressed(*key),
            Binding::Pad(button) => self.pads.iter().any(|pad| pad.just_pressed(*button)),
        })
    }

    /// Was any binding of this action just released?
    #[must_use]
    pub fn just_released(&self, map: &InputMap, action: GameAction) -> bool {
        map.of(action).iter().any(|binding| match binding {
            Binding::Key(key) => self.keys.just_released(*key),
            Binding::Pad(button) => self.pads.iter().any(|pad| pad.just_released(*button)),
        })
    }

    /// Is any binding of this action currently held?
    #[must_use]
    pub fn pressed(&self, map: &InputMap, action: GameAction) -> bool {
        map.of(action).iter().any(|binding| match binding {
            Binding::Key(key) => self.keys.pressed(*key),
            Binding::Pad(button) => self.pads.iter().any(|pad| pad.pressed(*button)),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_pin_the_user_contract() {
        // These exact defaults were settled in live playtests:
        // ASDFG frets, Space strums (two-hand split with tap off),
        // Enter is Hype, guitar face buttons match the X-plorer.
        use Binding::{Key, Pad};
        let map = InputMap::default();
        let fret_keys = [
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::KeyF,
            KeyCode::KeyG,
        ];
        for (fret, key) in fret_keys.iter().enumerate() {
            assert!(
                map.of(GameAction::Fret(fret as u8)).contains(&Key(*key)),
                "fret {fret} lost its home-row key"
            );
        }
        let strum = map.of(GameAction::StrumDown);
        assert!(strum.contains(&Key(KeyCode::Space)), "Space must strum");
        assert!(strum.contains(&Key(KeyCode::ArrowDown)));
        assert!(strum.contains(&Pad(GamepadButton::DPadDown)));
        assert!(
            map.of(GameAction::Hype).contains(&Key(KeyCode::Enter)),
            "Enter must trigger Hype"
        );
        assert!(map.of(GameAction::Pause).contains(&Key(KeyCode::Escape)));
        assert!(
            map.of(GameAction::Pause)
                .contains(&Pad(GamepadButton::Start))
        );
    }

    #[test]
    fn defaults_cover_every_action() {
        let map = InputMap::default();
        for action in GameAction::ALL {
            assert!(!map.of(action).is_empty(), "{action:?} has no binding");
        }
    }

    #[test]
    fn rebind_steals_the_binding_from_other_actions() {
        let mut map = InputMap::default();
        let space = Binding::Key(KeyCode::Space);
        // Space starts on Hype; move it to Fret 1.
        map.rebind(GameAction::Fret(0), space);
        assert!(map.of(GameAction::Fret(0)).contains(&space));
        assert!(!map.of(GameAction::Hype).contains(&space));
    }

    #[test]
    fn reset_restores_defaults() {
        let mut map = InputMap::default();
        map.rebind(GameAction::Hype, Binding::Key(KeyCode::KeyQ));
        map.reset_action(GameAction::Hype);
        assert_eq!(
            map.of(GameAction::Hype),
            InputMap::default().of(GameAction::Hype)
        );
    }

    #[test]
    fn sanitize_restores_missing_actions() {
        let mut map = InputMap {
            bindings: vec![(GameAction::Hype, vec![Binding::Key(KeyCode::Space)])],
            ui: vec![],
        };
        map.sanitize();
        for action in GameAction::ALL {
            assert!(!map.of(action).is_empty());
        }
        // A settings file from before the menu table existed comes
        // back with the full navigation set - including this one.
        for action in UiAction::ALL {
            assert!(!map.ui_of(action).is_empty(), "{action:?} has no binding");
        }
    }

    #[test]
    fn binding_labels_are_short_names_not_debug_dumps() {
        // These strings go straight onto the controls screen and into
        // the conflict message ("Space is STRUM DOWN - press it
        // again"); the Key/Arrow prefixes are implementation noise.
        assert_eq!(Binding::Key(KeyCode::KeyA).label(), "A");
        assert_eq!(Binding::Key(KeyCode::ArrowUp).label(), "Up");
        assert_eq!(Binding::Key(KeyCode::Space).label(), "Space");
        assert_eq!(Binding::Pad(GamepadButton::South).label(), "PAD South");
    }

    #[test]
    fn only_fret_actions_resolve_to_a_lane() {
        for index in 0..5u8 {
            let lane = GameAction::Fret(index).lane().expect("a fret has a lane");
            assert_eq!(lane.index(), index as usize);
        }
        assert_eq!(GameAction::Fret(9).lane(), None, "out of range is None");
        for action in [GameAction::StrumUp, GameAction::Hype, GameAction::Pause] {
            assert_eq!(action.lane(), None, "{action:?} is not a fret");
        }
    }

    #[test]
    fn menu_defaults_add_wasd_and_space() {
        use Binding::Key;
        let map = InputMap::default();
        assert!(map.ui_of(UiAction::NavUp).contains(&Key(KeyCode::KeyW)));
        assert!(map.ui_of(UiAction::NavDown).contains(&Key(KeyCode::KeyS)));
        assert!(map.ui_of(UiAction::NavLeft).contains(&Key(KeyCode::KeyA)));
        assert!(map.ui_of(UiAction::NavRight).contains(&Key(KeyCode::KeyD)));
        assert!(map.ui_of(UiAction::Confirm).contains(&Key(KeyCode::Space)));
        assert!(map.ui_of(UiAction::Confirm).contains(&Key(KeyCode::Enter)));
        assert!(map.ui_of(UiAction::Back).contains(&Key(KeyCode::Escape)));
    }

    #[test]
    fn a_settings_file_without_the_menu_table_loads_with_defaults() {
        // Forward compatibility: every settings.json written before
        // this table existed lacks the "ui" key entirely.
        let stripped = serde_json::json!({
            "bindings": serde_json::to_value(InputMap::default().bindings).unwrap()
        });
        let map: InputMap = serde_json::from_value(stripped).unwrap();
        for action in UiAction::ALL {
            assert!(!map.ui_of(action).is_empty(), "{action:?} came back empty");
        }
    }

    #[test]
    fn rebind_ui_steals_within_the_menu_table_only() {
        let mut map = InputMap::default();
        let a = Binding::Key(KeyCode::KeyA);
        // A starts on NavLeft (menus) and Fret 1 (game).
        map.rebind_ui(UiAction::Confirm, a);
        assert!(map.ui_of(UiAction::Confirm).contains(&a));
        assert!(
            !map.ui_of(UiAction::NavLeft).contains(&a),
            "stolen in-table"
        );
        assert!(
            map.of(GameAction::Fret(0)).contains(&a),
            "the GAME table must keep A - different context"
        );
    }

    #[test]
    fn wasd_navigates_and_space_confirms() {
        let map = InputMap::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::Space);
        let nav = MenuNav::read(&map, &keys, std::iter::empty());
        assert!(nav.up, "W must navigate up");
        assert!(nav.confirm, "Space must confirm");
    }

    #[test]
    fn typing_mode_lets_letters_type_but_keeps_the_arrows() {
        // The browser search: W in the box must not move the cursor,
        // but the arrows still step through the results.
        let map = InputMap::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::Space);
        keys.press(KeyCode::ArrowDown);
        let nav = MenuNav::read_typing(&map, &keys, std::iter::empty());
        assert!(!nav.up, "a typed W must not navigate");
        assert!(!nav.confirm, "a typed Space must not confirm");
        assert!(nav.down, "arrows still navigate while typing");
    }

    #[test]
    fn enter_and_escape_survive_an_emptied_table() {
        // The safety net: no rebinding can strand the player.
        let map = InputMap {
            ui: vec![],
            ..Default::default()
        };
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Enter);
        keys.press(KeyCode::Escape);
        let nav = MenuNav::read(&map, &keys, std::iter::empty());
        assert!(nav.confirm, "Enter is a hard-wired confirm");
        assert!(nav.back, "Escape is a hard-wired back");
    }

    #[test]
    fn tab_cycles_down_and_shift_tab_up() {
        let map = InputMap::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Tab);
        let nav = MenuNav::read(&map, &keys, std::iter::empty());
        assert!(nav.down && !nav.up);
        keys.press(KeyCode::ShiftLeft);
        let nav = MenuNav::read(&map, &keys, std::iter::empty());
        assert!(nav.up && !nav.down, "Shift+Tab must reverse");
    }
}

/// Merged menu navigation for one frame (keyboard arrows/Enter/Esc
/// plus gamepad D-pad/South/East on any pad).
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuNav {
    /// Move up.
    pub up: bool,
    /// Move down.
    pub down: bool,
    /// Move left.
    pub left: bool,
    /// Move right.
    pub right: bool,
    /// Confirm / select.
    pub confirm: bool,
    /// Back / cancel.
    pub back: bool,
}

impl MenuNav {
    /// Read this frame's menu navigation from all devices, through
    /// the player's own [`UiAction`] bindings.
    ///
    /// Two things are hard-wired on top of the table: **Enter always
    /// confirms and Escape always backs out** (a mangled bindings
    /// file must never strand the player in a menu), and
    /// **Tab / Shift+Tab cycle the cursor** (a desktop convention,
    /// deliberately not bindable).
    #[must_use]
    pub fn read<'a>(
        map: &InputMap,
        keys: &ButtonInput<KeyCode>,
        pads: impl IntoIterator<Item = &'a Gamepad>,
    ) -> MenuNav {
        MenuNav::read_mode(map, keys, pads, false)
    }

    /// [`MenuNav::read`] for screens that are currently TYPING (the
    /// browser search): bindings on printable keys are ignored, so
    /// letters and Space go into the text instead of navigating.
    /// Arrows, the D-pad, Enter and Escape keep working.
    #[must_use]
    pub fn read_typing<'a>(
        map: &InputMap,
        keys: &ButtonInput<KeyCode>,
        pads: impl IntoIterator<Item = &'a Gamepad>,
    ) -> MenuNav {
        MenuNav::read_mode(map, keys, pads, true)
    }

    fn read_mode<'a>(
        map: &InputMap,
        keys: &ButtonInput<KeyCode>,
        pads: impl IntoIterator<Item = &'a Gamepad>,
        typing: bool,
    ) -> MenuNav {
        let pads: Vec<&Gamepad> = pads.into_iter().collect();
        let hit = |action: UiAction| {
            map.ui_of(action).iter().any(|binding| match binding {
                Binding::Key(key) => {
                    (!typing || !key_is_printable(*key)) && keys.just_pressed(*key)
                }
                Binding::Pad(button) => pads.iter().any(|pad| pad.just_pressed(*button)),
            })
        };
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let tab = keys.just_pressed(KeyCode::Tab);
        MenuNav {
            up: hit(UiAction::NavUp) || (tab && shift),
            down: hit(UiAction::NavDown) || (tab && !shift),
            left: hit(UiAction::NavLeft),
            right: hit(UiAction::NavRight),
            confirm: hit(UiAction::Confirm) || keys.just_pressed(KeyCode::Enter),
            back: hit(UiAction::Back) || keys.just_pressed(KeyCode::Escape),
        }
    }

    /// Any navigation happened at all (for UI sounds).
    #[must_use]
    pub fn any_move(self) -> bool {
        self.up || self.down || self.left || self.right
    }
}
