//! Contextual input prompts: footers speak the player's device.
//!
//! The game tracks which device produced the LAST real input and the
//! hint lines swap wording to match — a player driving with the pad
//! never reads "press ENTER", and a keyboard player never reads
//! "press SOUTH". The mouse counts as keyboard-family: its prompts
//! live in the same footer strings ("MOUSE works too").

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::ui::UiFont;
use crate::ui_kit;

/// The device the player last touched. Defaults to keyboard: the
/// first frame has no history, and a keyboard always exists.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveDevice {
    /// Keyboard (or mouse) drove the last input.
    #[default]
    Keyboard,
    /// A gamepad button drove the last input.
    Gamepad,
}

/// A footer that owns two wordings and shows the active device's.
#[derive(Component, Debug, Clone)]
pub struct DeviceHint {
    /// The keyboard/mouse wording.
    pub keyboard: String,
    /// The gamepad wording.
    pub pad: String,
}

impl DeviceHint {
    /// The wording for a device.
    #[must_use]
    pub fn for_device(&self, device: ActiveDevice) -> &str {
        match device {
            ActiveDevice::Keyboard => &self.keyboard,
            ActiveDevice::Gamepad => &self.pad,
        }
    }
}

/// What this frame's raw inputs say about the device in the hand.
/// `None` when nothing was pressed — the previous answer stands.
#[must_use]
pub fn device_of_frame(keyboard: bool, mouse: bool, pad: bool) -> Option<ActiveDevice> {
    // A pad press wins a shared frame: pressing a fret while a palm
    // brushes the keyboard is guitar play, not typing.
    if pad {
        Some(ActiveDevice::Gamepad)
    } else if keyboard || mouse {
        Some(ActiveDevice::Keyboard)
    } else {
        None
    }
}

/// Track the last active device from the raw input streams.
fn track_active_device(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    mut active: ResMut<ActiveDevice>,
) {
    let keyboard = keys.get_just_pressed().next().is_some();
    let clicked = mouse.get_just_pressed().next().is_some();
    let pad = pads
        .iter()
        .any(|pad| pad.get_just_pressed().next().is_some());
    if let Some(device) = device_of_frame(keyboard, clicked, pad)
        && *active != device
    {
        *active = device;
    }
}

/// Rewrite every device-aware hint when the device changes (and once
/// after spawn, when the text is still empty).
fn swap_device_hints(active: Res<ActiveDevice>, mut hints: Query<(&DeviceHint, &mut Text)>) {
    for (hint, mut text) in &mut hints {
        let wanted = hint.for_device(*active);
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    }
}

/// A footer with a wording per device — the device-aware sibling of
/// [`ui_kit::footer`], same dress, swapped live.
pub fn device_footer(parent: &mut ChildSpawnerCommands, font: &UiFont, keyboard: &str, pad: &str) {
    parent.spawn((
        DeviceHint {
            keyboard: keyboard.to_owned(),
            pad: pad.to_owned(),
        },
        Text::new(String::new()),
        font.text(ui_kit::SMALL),
        TextColor(crate::palette::dimmed(crate::palette::TEXT_DIM, 0.75)),
        Node {
            margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
            ..default()
        },
    ));
}

/// The prompts plugin: device tracking + hint swapping.
pub struct PromptsPlugin;

impl Plugin for PromptsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveDevice>()
            .add_systems(Update, (track_active_device, swap_device_hints).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_speaks_keyboard() {
        // No history yet - and a keyboard always exists.
        assert_eq!(ActiveDevice::default(), ActiveDevice::Keyboard);
    }

    #[test]
    fn a_quiet_frame_keeps_the_previous_device() {
        // Prompts must not flicker back to a default between inputs.
        assert_eq!(device_of_frame(false, false, false), None);
    }

    #[test]
    fn each_device_claims_its_own_frame_and_the_pad_wins_a_shared_one() {
        assert_eq!(
            device_of_frame(true, false, false),
            Some(ActiveDevice::Keyboard)
        );
        assert_eq!(
            device_of_frame(false, true, false),
            Some(ActiveDevice::Keyboard),
            "the mouse belongs to the keyboard prompt family"
        );
        assert_eq!(
            device_of_frame(false, false, true),
            Some(ActiveDevice::Gamepad)
        );
        // Fretting with a palm on the keyboard is guitar play.
        assert_eq!(
            device_of_frame(true, true, true),
            Some(ActiveDevice::Gamepad)
        );
    }

    #[test]
    fn a_hint_answers_with_the_matching_wording() {
        let hint = DeviceHint {
            keyboard: "ENTER confirm".to_owned(),
            pad: "SOUTH confirm".to_owned(),
        };
        assert_eq!(hint.for_device(ActiveDevice::Keyboard), "ENTER confirm");
        assert_eq!(hint.for_device(ActiveDevice::Gamepad), "SOUTH confirm");
    }
}
