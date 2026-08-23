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
                    vec![Key(K::ArrowDown), Pad(B::DPadDown)],
                ),
                (
                    GameAction::Hype,
                    vec![Key(K::Space), Pad(B::Select), Pad(B::RightTrigger)],
                ),
                (GameAction::Pause, vec![Key(K::Escape), Pad(B::Start)]),
            ],
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
    }
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
        };
        map.sanitize();
        for action in GameAction::ALL {
            assert!(!map.of(action).is_empty());
        }
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
    /// Read this frame's menu navigation from all devices.
    #[must_use]
    pub fn read(keys: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>) -> MenuNav {
        let pad = |button: GamepadButton| pads.iter().any(|p| p.just_pressed(button));
        MenuNav {
            up: keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp),
            down: keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown),
            left: keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft),
            right: keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight),
            confirm: keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South),
            back: keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East),
        }
    }

    /// Any navigation happened at all (for UI sounds).
    #[must_use]
    pub fn any_move(self) -> bool {
        self.up || self.down || self.left || self.right
    }
}
