# UX / input / menu system — inventory and plan

Commissioned 2026-09-01: a structured spec covering logical input
actions, remapping, contextual prompts, UI navigation, menu UX, UI
sound, accessibility, calibration, retro-mode isolation and the
input→gameplay→rendering dependency rule.

## Inventory — what already ships

Much of the spec is BeatByte's existing architecture. Mapped point by
point so the work below is the *gap*, not a rebuild:

- **Logical actions**: `controls.rs` — `GameAction` (Fret 0–4,
  StrumUp/Down, Hype = the spec's *Special*, Pause) behind a
  data-driven `InputMap`. Gameplay and menus never read fret keys
  directly; bindings are data, persisted with the settings, sanitized
  on load. The spec's dependency diagram is the module's doc comment.
- **Remapping**: the controls screen captures a key/pad press per
  action, steals conflicting bindings, resets to defaults, persists
  on exit. Keyboard + gamepad both bindable.
- **Menu navigation**: `MenuNav` (up/down/left/right/confirm/back)
  merges keyboard + every connected pad; used by main menu, browser,
  settings, controls, multiplayer and the pause menu.
- **Mouse**: `ui_kit::read_rows` — one rule everywhere, *hovering
  selects, clicking activates*; wheel scrolls the browser;
  right-click is back.
- **Focus**: one cursor row per screen; `RowState`
  Idle/Selected/Armed with a style that differs in fill AND accent
  bar AND text (pinned). Hover deliberately *is* selection — two
  cursors would contradict each other (documented in `ui_kit`).
- **UI sound**: procedural, centralized (`SfxLib`), no audio
  binaries, volume from settings. Move + confirm exist.
- **Menu UX**: `ui_kit` owns the type scale, spacing rhythm, panel
  chrome, footer grammar; state transitions fade (0.25 s);
  `prefers-reduced-motion`-style toggles exist (below).
- **Accessibility**: particles, screen shake, beat pulse, backdrop
  motion are settings and thread through `EffectSettings`.
- **Calibration**: `latency_offset_ms` (input-vs-audio) with a
  tap-to-the-beat calibration screen. Judgment is input-stamp-driven;
  visuals never bend gameplay timing.
- **Retro/8-bit**: the 8-bit look is *data* (`round_gems` +
  per-style textures), not a forked renderer; the per-style camera
  contracts (Bloom/HDR) are centralized in `sync_bloom`, not spread
  through gameplay. The dormant 2D highway path is filed for pruning.

## Gaps — the plan

Phased; each phase lands as its own gated, versioned commit.

1. **Navigation becomes logical and remappable** *(shipped v0.12.29)*
   - `UiAction` {NavUp, NavDown, NavLeft, NavRight, Confirm, Back}
     with its own bindings table in `InputMap` (serde-default so old
     settings files load). Defaults add **WASD** navigation and
     **Space** confirm; Tab/Shift+Tab cycle rows (fixed, unbindable
     desktop convention).
   - `MenuNav::read` consults the table. **Enter/Escape stay
     hard-wired fallbacks** — a mangled bindings file must never
     strand the player in a menu.
   - **Text-entry mode**: while the browser search is typing,
     printable-key bindings (letters/digits/space) are ignored so
     W/A/S/D type instead of navigating; arrows/D-pad still work.
   - Controls screen lists the menu actions as their own
     MENU-prefixed rows; the list scrolls (15 rows outgrew the safe
     area) with the browser's whole-row window + cursor follow.
   - **Conflict confirmation**: capturing a binding that already
     serves another action no longer steals silently — the row shows
     *"X is FRET 2 — press again to move it"*; same press confirms,
     anything else cancels.
2. **UI sound becomes an event system** *(shipped v0.12.30)* —
   `UiSoundEvent` {Navigate, Confirm, Back, Error, Toggle, Slider}
   with one playback system and new procedural voices; screens emit
   events instead of `sfx` reading raw keys, so gamepad and mouse
   navigation sound too. No widget hard-codes an asset (voices stay
   synthesized in `SfxLib`).
3. **Contextual input prompts** *(shipped v0.12.31)* — an
   `ActiveDevice` resource (keyboard | gamepad) updated from the last
   real input; footers and hint lines render device-appropriate
   labels and swap when the device changes. Keyboard prompts never
   show while the player drives with the pad, and vice versa.
4. **Accessibility completion** — reduced flashing (gates strobe-like
   pulses), UI scale multiplier on top of the window-height sync,
   high contrast (ui_kit row styles + HUD text step up), and a visual
   effect intensity slider scaling particle counts/glow. All through
   `Settings` → one consumer each, no scattered conditionals.
5. **Calibration split** *(shipped v0.12.33)* — a VIDEO OFFSET
   beside the input offset, shifting only where notes are drawn
   (`GameClock::visual_time`); judgment provably unshifted (an
   autopilot run at +100 ms stays perfect). Deliberately a manual
   nudge, not a measured flow: without photometric hardware any
   "video calibration" is eyeballing, and the settings row IS that,
   honestly labeled. The tap flow keeps measuring the input offset
   and its screen now says so.
6. **Renderer boundary** *(shipped v0.12.34)* — ADR-0012 documents
   the 8-bit style as data behind the style boundary (one renderer,
   three data touch points, no style conditionals in gameplay/
   input/UI). The dormant 2D highway path stays a filed roadmap
   task, deliberately separate from the style boundary.

## Deliberate deviations

- **Hover = selection** stays (spec asks for separate hover/focus
  states): on every screen hovering moves the cursor, so a second
  visual state would contradict the first. The spec's goal —
  the focused element is always obvious — is met by the row style.
- **Mouse buttons are not bindable** (capture would be un-cancelable
  with the same device that cancels).
- **Editor keys stay raw**: the editor is a tool with tool shortcuts
  (17 of them), not a menu; remapping them is out of scope.
- **Browser shortcuts (F search, S sort, E edit, DEL delete) stay
  raw**: they are accelerator keys, not navigation; the footer names
  them.
- A binding may serve a *game* action and a *UI* action at once
  (A = Fret 1 in play, NavLeft in menus) — different contexts,
  detected conflicts are per-table.
