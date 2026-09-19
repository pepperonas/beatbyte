//! Native Guitar Hero X-plorer support.
//!
//! The X-plorer is an Xbox-360-class USB device: it speaks a vendor
//! protocol, not standard HID, so macOS (and thus gilrs) never sees
//! it — verified on the actual hardware before this module existed.
//! A background thread reads the well-documented 20-byte interrupt
//! reports over libusb and feeds them into Bevy as raw gamepad
//! events. From there the guitar IS a gamepad: the existing input
//! map (green=South … orange=LeftTrigger, strum=D-pad), menu
//! navigation and multiplayer join all work untouched.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use bevy::input::gamepad::{
    GamepadAxis, GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
    RawGamepadEvent,
};
use bevy::prelude::*;

/// RedOctane vendor id.
const VENDOR: u16 = 0x1430;
/// X-plorer product id.
const PRODUCT: u16 = 0x4748;

/// Raise past 50% to engage; lower below 40% to re-arm. These values match
/// the measured X-plorer RY range (rest near zero, vertical near full scale).
pub const TILT_ON: f32 = 0.50;
/// Release threshold, separated from [`TILT_ON`] to reject axis chatter.
pub const TILT_OFF: f32 = 0.40;

/// Hysteretic edge detector shared by native USB and driver-backed Xplorers.
#[derive(Debug, Default)]
struct TiltGate {
    active: bool,
}

impl TiltGate {
    fn update(&mut self, value: f32) -> Option<bool> {
        if self.active {
            if value < TILT_OFF {
                self.active = false;
                return Some(false);
            }
        } else if value > TILT_ON {
            self.active = true;
            return Some(true);
        }
        None
    }
}

/// One state snapshot from the reader thread.
enum GuitarMessage {
    /// Device opened successfully.
    Connected,
    /// Device gone (unplugged or read failure).
    Disconnected,
    /// Button bitmask changed (bytes 2 and 3 of the report).
    Buttons(u16),
    /// Tilt crossed a hysteresis boundary.
    Tilt(bool),
}

/// Channel + entity bookkeeping on the Bevy side.
#[derive(Resource)]
struct GuitarBridge {
    // Mutex only for Sync (a Bevy resource must be); one reader.
    receiver: std::sync::Mutex<Receiver<GuitarMessage>>,
    entity: Option<Entity>,
    last_buttons: u16,
}

/// The X-plorer plugin: spawns the reader thread and the feed system.
pub struct XplorerPlugin;

impl Plugin for XplorerPlugin {
    fn build(&self, app: &mut App) {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("beatbyte-xplorer".into())
            .spawn(move || reader_thread(&sender))
            .ok();
        app.insert_resource(GuitarBridge {
            receiver: std::sync::Mutex::new(receiver),
            entity: None,
            last_buttons: 0,
        })
        .add_systems(
            PreUpdate,
            feed_guitar_events.before(bevy::input::InputSystems),
        )
        // Windows/Linux drivers expose the same XInput RY value as the
        // right-stick Y axis. Restrict this bridge to the verified VID/PID;
        // an ordinary pad's right stick must never activate Hype.
        .add_systems(PreUpdate, feed_driver_tilt.after(bevy::input::InputSystems));
    }
}

/// The (bit, bevy button) pairs we translate. Report layout is the
/// standard Xbox 360 one: byte 2 = dpad/start/back, byte 3 = buttons.
/// On the guitar: A=green, B=red, Y=yellow, X=blue, LB=orange,
/// D-pad up/down = strum, Back = star power, Start = pause — exactly
/// the game's default pad bindings.
const BUTTON_BITS: [(u16, GamepadButton); 9] = [
    (1 << 0, GamepadButton::DPadUp),
    (1 << 1, GamepadButton::DPadDown),
    (1 << 4, GamepadButton::Start),
    (1 << 5, GamepadButton::Select),
    (1 << 8, GamepadButton::LeftTrigger),
    (1 << 12, GamepadButton::South),
    (1 << 13, GamepadButton::East),
    (1 << 14, GamepadButton::West),
    (1 << 15, GamepadButton::North),
];

/// Decode bytes 2/3 of a report into our bitmask (byte2 = low bits,
/// byte3 = high bits). Pure — tested.
#[must_use]
pub fn decode_report(byte2: u8, byte3: u8) -> u16 {
    u16::from(byte2) | (u16::from(byte3) << 8)
}

/// Forward thread messages into Bevy's raw gamepad pipeline.
fn feed_guitar_events(
    mut commands: Commands,
    mut bridge: ResMut<GuitarBridge>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
) {
    while let Ok(message) = bridge
        .receiver
        .get_mut()
        .map_err(|_| ())
        .and_then(|receiver| receiver.try_recv().map_err(|_| ()))
    {
        match message {
            GuitarMessage::Connected => {
                let entity = commands.spawn_empty().id();
                bridge.entity = Some(entity);
                bridge.last_buttons = 0;
                info!("x-plorer: guitar connected");
                let event = GamepadConnectionEvent {
                    gamepad: entity,
                    connection: GamepadConnection::Connected {
                        name: "Guitar Hero X-plorer".to_owned(),
                        vendor_id: Some(VENDOR),
                        product_id: Some(PRODUCT),
                    },
                };
                connections.write(event.clone());
                raw.write(RawGamepadEvent::Connection(event));
            }
            GuitarMessage::Disconnected => {
                if let Some(entity) = bridge.entity.take() {
                    info!("x-plorer: guitar disconnected");
                    let event = GamepadConnectionEvent {
                        gamepad: entity,
                        connection: GamepadConnection::Disconnected,
                    };
                    connections.write(event.clone());
                    raw.write(RawGamepadEvent::Connection(event));
                }
            }
            GuitarMessage::Buttons(state) => {
                let Some(entity) = bridge.entity else {
                    continue;
                };
                let changed = state ^ bridge.last_buttons;
                bridge.last_buttons = state;
                for (bit, button) in BUTTON_BITS {
                    if changed & bit != 0 {
                        let value = if state & bit != 0 { 1.0 } else { 0.0 };
                        let event = RawGamepadButtonChangedEvent {
                            gamepad: entity,
                            button,
                            value,
                        };
                        buttons.write(event);
                        raw.write(RawGamepadEvent::Button(event));
                    }
                }
            }
            GuitarMessage::Tilt(pressed) => {
                let Some(entity) = bridge.entity else {
                    continue;
                };
                emit_button(
                    entity,
                    GamepadButton::RightTrigger,
                    pressed,
                    &mut buttons,
                    &mut raw,
                );
            }
        }
    }
}

fn emit_button(
    gamepad: Entity,
    button: GamepadButton,
    pressed: bool,
    buttons: &mut MessageWriter<RawGamepadButtonChangedEvent>,
    raw: &mut MessageWriter<RawGamepadEvent>,
) {
    let event = RawGamepadButtonChangedEvent {
        gamepad,
        button,
        value: if pressed { 1.0 } else { 0.0 },
    };
    buttons.write(event);
    raw.write(RawGamepadEvent::Button(event));
}

/// Turn a driver-provided X-plorer RY axis into the existing Hype binding.
fn feed_driver_tilt(
    mut pads: Query<(Entity, &mut Gamepad)>,
    mut gates: Local<HashMap<Entity, TiltGate>>,
) {
    for (entity, mut pad) in &mut pads {
        if pad.vendor_id() != Some(VENDOR) || pad.product_id() != Some(PRODUCT) {
            continue;
        }
        let value = pad.get(GamepadAxis::RightStickY).unwrap_or(0.0);
        if let Some(pressed) = gates.entry(entity).or_default().update(value) {
            if pressed {
                pad.digital_mut().press(GamepadButton::RightTrigger);
            } else {
                pad.digital_mut().release(GamepadButton::RightTrigger);
            }
        }
    }
}

/// RY (bytes 12/13, signed little-endian) normalized to the useful 0..1
/// tilt range. Negative values mean the neck points below neutral.
#[must_use]
pub fn decode_tilt(report: &[u8]) -> Option<f32> {
    if report.len() < 14 || report[0] != 0x00 {
        return None;
    }
    let raw = i16::from_le_bytes([report[12], report[13]]);
    Some((f32::from(raw) / f32::from(i16::MAX)).clamp(0.0, 1.0))
}

/// Poll for the guitar forever; when present, stream its reports.
fn reader_thread(sender: &Sender<GuitarMessage>) {
    loop {
        match open_guitar() {
            Some(handle) => {
                if sender.send(GuitarMessage::Connected).is_err() {
                    return;
                }
                stream_reports(&handle, sender);
                if sender.send(GuitarMessage::Disconnected).is_err() {
                    return;
                }
            }
            None => std::thread::sleep(std::time::Duration::from_secs(2)),
        }
    }
}

/// Find and claim the guitar, if plugged in.
fn open_guitar() -> Option<rusb::DeviceHandle<rusb::GlobalContext>> {
    let devices = rusb::devices().ok()?;
    for device in devices.iter() {
        let descriptor = device.device_descriptor().ok()?;
        if descriptor.vendor_id() == VENDOR && descriptor.product_id() == PRODUCT {
            let handle = device.open().ok()?;
            handle.claim_interface(0).ok()?;
            return Some(handle);
        }
    }
    None
}

/// Read interrupt reports until the device goes away. Endpoint 0x81,
/// 20-byte input reports (type 0x00, length 0x14); bytes 2/3 carry
/// the buttons.
fn stream_reports(
    handle: &rusb::DeviceHandle<rusb::GlobalContext>,
    sender: &Sender<GuitarMessage>,
) {
    let mut buffer = [0u8; 32];
    let mut last = 0u16;
    let mut tilt = TiltGate::default();
    let mut consecutive_errors = 0u32;
    loop {
        match handle.read_interrupt(0x81, &mut buffer, std::time::Duration::from_millis(100)) {
            Ok(n) if n >= 14 && buffer[0] == 0x00 => {
                consecutive_errors = 0;
                let state = decode_report(buffer[2], buffer[3]);
                if state != last {
                    last = state;
                    if sender.send(GuitarMessage::Buttons(state)).is_err() {
                        return;
                    }
                }
                if let Some(active) = decode_tilt(&buffer[..n]).and_then(|v| tilt.update(v))
                    && sender.send(GuitarMessage::Tilt(active)).is_err()
                {
                    return;
                }
            }
            Ok(_) | Err(rusb::Error::Timeout) => {
                consecutive_errors = 0;
            }
            Err(_) => {
                consecutive_errors += 1;
                if consecutive_errors > 5 {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BUTTON_BITS, TILT_OFF, TILT_ON, TiltGate, decode_report, decode_tilt};
    use bevy::prelude::GamepadButton;

    #[test]
    fn report_bytes_map_to_the_documented_layout() {
        // byte2 bit0 = dpad up (strum), byte3 bit4 = A (green fret).
        assert_eq!(decode_report(0b0000_0001, 0) & 1, 1);
        let green = decode_report(0, 0b0001_0000);
        assert_eq!(
            BUTTON_BITS.iter().find(|(bit, _)| green & bit != 0),
            Some(&(1 << 12, GamepadButton::South)),
            "A must decode as the green fret (South)"
        );
        // byte3 bit0 = LB = orange fret.
        let orange = decode_report(0, 0b0000_0001);
        assert_eq!(
            BUTTON_BITS.iter().find(|(bit, _)| orange & bit != 0),
            Some(&(1 << 8, GamepadButton::LeftTrigger)),
        );
    }

    #[test]
    fn the_guitar_can_drive_the_menus_and_the_song_list() {
        // The whole point of feeding the guitar in as a GAMEPAD: it
        // must be able to walk the menus and the song browser without
        // a keyboard. That is not a property of this module alone -
        // it needs the report bits, THIS translation table and the
        // default menu bindings to agree - so the contract is pinned
        // end to end here rather than assumed in a doc comment.
        use crate::controls::{Binding, InputMap, UiAction};
        let map = InputMap::default();
        // What the guitar sends: strum up/down are the D-pad's first
        // two bits, green is A, red is B.
        let sent = |byte2: u8, byte3: u8| {
            BUTTON_BITS
                .iter()
                .find(|(bit, _)| decode_report(byte2, byte3) & bit != 0)
                .map(|(_, button)| *button)
                .expect("the report bit is translated")
        };
        let strum_up = sent(0b0000_0001, 0);
        let strum_down = sent(0b0000_0010, 0);
        let green = sent(0, 0b0001_0000);
        let red = sent(0, 0b0010_0000);
        // What the menus listen for.
        let bound = |action: UiAction, button| map.ui_of(action).contains(&Binding::Pad(button));
        assert!(bound(UiAction::NavUp, strum_up), "strum up walks up");
        assert!(
            bound(UiAction::NavDown, strum_down),
            "strum down walks down"
        );
        assert!(bound(UiAction::Confirm, green), "green confirms");
        assert!(bound(UiAction::Back, red), "red goes back");
        // The neck reports NO horizontal direction - the strum bar is
        // the D-pad's up/down and bits 2/3 are deliberately dropped -
        // so left/right cannot come from the bindings table. They are
        // hard-wired onto the two middle frets in `MenuNav`; without
        // that a guitarist can walk the song list but never change
        // the difficulty beside it.
        assert!(
            BUTTON_BITS.iter().all(|(_, button)| !matches!(
                button,
                GamepadButton::DPadLeft | GamepadButton::DPadRight
            )),
            "if the guitar ever reports left/right, revisit the fret fallback"
        );
        let yellow = sent(0, 0b1000_0000);
        let blue = sent(0, 0b0100_0000);
        assert_eq!(yellow, GamepadButton::North, "yellow is the menu's left");
        assert_eq!(blue, GamepadButton::West, "blue is the menu's right");
    }

    #[test]
    fn untranslated_bits_stay_untranslated() {
        // D-pad left/right and the Guide button exist in the report
        // but mean nothing on a guitar — they must not map to any
        // game input, or phantom presses would leak through.
        for bit in [1u16 << 2, 1 << 3, 1 << 10] {
            assert!(
                BUTTON_BITS.iter().all(|(b, _)| b & bit == 0),
                "bit {bit:#06x} must not be translated"
            );
        }
    }

    #[test]
    fn chord_and_strum_decode_simultaneously() {
        // Real play: two frets held while the strum bar flicks —
        // one report carries all three, nothing masks anything.
        let state = decode_report(0b0000_0010, 0b0011_0000);
        assert_ne!(state & (1 << 1), 0, "strum down");
        assert_ne!(state & (1 << 12), 0, "green fret");
        assert_ne!(state & (1 << 13), 0, "red fret");
        assert_eq!(state & (1 << 8), 0, "orange must stay released");
    }

    #[test]
    fn tilt_is_the_signed_little_endian_ry_axis() {
        let mut report = [0u8; 20];
        report[0] = 0x00;
        report[1] = 0x14;
        report[12..14].copy_from_slice(&16384i16.to_le_bytes());
        assert!((decode_tilt(&report).expect("input report") - 0.5).abs() < 0.001);
        report[12..14].copy_from_slice(&(-500i16).to_le_bytes());
        assert_eq!(decode_tilt(&report), Some(0.0));
    }

    #[test]
    fn tilt_triggers_once_then_requires_a_neutral_reset() {
        let mut gate = TiltGate::default();
        assert_eq!(gate.update(TILT_ON - 0.01), None, "below threshold");
        assert_eq!(gate.update(TILT_ON + 0.01), Some(true), "rising edge");
        assert_eq!(gate.update(0.9), None, "held high");
        assert_eq!(
            gate.update((TILT_ON + TILT_OFF) / 2.0),
            None,
            "hysteresis band"
        );
        assert_eq!(gate.update(TILT_OFF - 0.01), Some(false), "neutral re-arms");
        assert_eq!(gate.update(TILT_ON + 0.01), Some(true), "second lift");
    }

    #[test]
    fn every_translated_bit_is_unique() {
        for (i, (a, _)) in BUTTON_BITS.iter().enumerate() {
            for (b, _) in BUTTON_BITS.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }
}
