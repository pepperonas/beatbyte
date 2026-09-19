# ADR-0007: Input Abstraction — Devices, Bindings, Actions

- **Status**: accepted
- **Date**: 2026-08-23

## Context

Gameplay must work with keyboards, gamepads and guitar-style
controllers (which enumerate as gamepads), be remappable, and support
several devices at once for local multiplayer — without gameplay code
knowing any of it.

## Decision

Three layers (`beatbyte-game::controls`):

```text
Physical input        KeyCode / GamepadButton on a device
      ↓ Binding       data, persisted with the settings
Game action           Fret(0–4), StrumUp/Down, Hype, Pause
      ↓
Gameplay / menus      only ever see actions
```

- **Bindings are data.** `InputMap` lives inside the persisted
  `Settings` (Bevy's `serialize` feature gives input types serde).
  A binding serves exactly one action; rebinding steals it. Config
  files are untrusted input: unknown entries are dropped, missing
  actions restored from defaults.
- **Default pad layout is the guitar layout**: frets on
  South/East/North/West/LeftTrigger (green→orange), strum on the
  D-pad, Hype on Select/RightTrigger, pause on Start. A guitar
  controller that shows up as a gamepad works unmodified; a normal
  pad is playable with the same map.
- **Device routing for multiplayer**: each player entity carries a
  `PlayerDevice` (`Keyboard` or `Pad(entity)`); its session only
  hears that device. Menus listen to *all* devices (`MenuNav`:
  D-pad + South/East merged with arrows + Enter/Esc).
- **Remap screen** (Settings → Controls): select an action, press the
  new key/button; Backspace restores that action's defaults.
- **Derived Hype inputs stay above physical mappings.** Either four adjacent
  logical frets activates `Hype`, independent of their keys or pad buttons.
  Five frets are one held chord, not two activations. The wired RedOctane
  Xplorer (`1430:4748`) is the hardware-specific exception at the controller
  boundary: its 20-byte XInput report carries tilt as signed little-endian RY
  in bytes 12–13. The native USB bridge and driver-backed gamepad path both
  turn that verified axis into the existing `RightTrigger` Hype binding.
  Activation is edge-only above 50%, and re-arms below 40%.

## Consequences

- The judgment engine remains input-agnostic (ADR-0004): it receives
  timestamped fret/strum events and never learns what produced them.
- Windows XInput/WGI and Linux evdev may expose Xplorer RY as right-stick Y;
  BeatByte accepts it only when the device reports the verified VID/PID.
  macOS has no classic-XInput driver, so BeatByte reads the report directly.
  Other guitars, wireless receivers and adapters are deliberately not guessed:
  their tilt continues to work only when their driver maps it to an existing
  Hype button, until their VID/PID and axis report are measured.
- Per-player binding *profiles* (different maps per player) are a
  data extension of `InputMap`, not a redesign.
