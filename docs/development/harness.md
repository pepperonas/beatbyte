# The verification harnesses

BeatByte ships with harnesses that run a **real build of the game** —
window, renderer, audio thread, input pipeline and all — and report a
pass or a fail. They are part of the test suite, not a debugging
convenience: a change that touches gameplay, timing, input or state
flow is not done until the relevant one has run.

All of them are switched on with environment variables, so nothing
about them is compiled into a normal build.

## Quick reference

| Variable | Value | What it does |
|---|---|---|
| `BEATBYTE_SMOKE_TEST` | set | Boots to the menu and exits 0. The cheapest proof that nothing panics on startup. |
| `BEATBYTE_AUTOPILOT` | set | Plays a song perfectly. **Exits non-zero on any miss or overstrum.** |
| `BEATBYTE_AUTOPILOT_SONG` | index or title substring | Which song to play (default: the first bundled one). |
| `BEATBYTE_AUTOPILOT_PLAYERS` | `2`…`4` | Local multiplayer run. |
| `BEATBYTE_AUTOPILOT_MUTE` | set | Silences the run. Seeds the mute state only — the in-game `M` toggle still works. |
| `BEATBYTE_AUTOPILOT_KEYS` | set | Presses real `KeyCode`s instead of feeding timestamped inputs, exercising `InputMap` resolution and gameplay routing exactly as a human would. |
| `BEATBYTE_AUTOPILOT_NO_STRUM` | set | Fret presses only, no strum. Proves tap mode is really on (and, with tap off, that it is really off). |
| `BEATBYTE_AUTOPILOT_EDIT` | set | Opens the editor and runs an add / undo / redo / save cycle. |
| `BEATBYTE_AUTOPILOT_DROP` | set | Injects a drag-and-drop import and then plays the imported chart — the only check that covers `import.rs`, which the CLI path does not. |
| `BEATBYTE_AUTOPILOT_DELETE` | set | Drives the browser with real arrow and backspace keys through the two-press delete confirmation. |
| `BEATBYTE_SHOT_DIR` | directory | Screenshots at named moments of a run. |
| `BEATBYTE_SHOT_STATE` | screen name | Boots straight into one screen, photographs it and quits. |
| `BEATBYTE_WINDOW` | `WxH` | Pins the window size, for layout verification. |
| `BEATBYTE_FPS` | set | Reports median and 99th-percentile frame time every five seconds. |

## The autopilot

```bash
BEATBYTE_AUTOPILOT=1 cargo run --release -p beatbyte
```

It plays the chart from the note data and **fails on any miss or
overstrum**. That strictness is only meaningful because judgment is
input-stamp-driven: the autopilot stamps its inputs against the song
clock, so a frame hitch cannot cause a miss and the verdict does not
depend on the machine it runs on.

Its input feed must stay ordered `.before(advance_sessions)`. Moving it
after would make the run frame-quantised, at which point Greats become
legitimate and the pass/fail line stops meaning anything.

Because it is exact, it doubles as a **presentation-independence
proof**: the same song scores identically in the depth view and the 3D
stage (624 perfect, 0 miss on a real import), which is how "the renderer
cannot affect scoring" is verified rather than asserted.

### Choosing a song

```bash
BEATBYTE_AUTOPILOT_SONG=2 …                 # index into the library
BEATBYTE_AUTOPILOT_SONG="Never Gonna" …     # title substring
```

Local verification runs play imported tracks, because that is what the
game is actually used with. The **bundled synthesized songs stay the CI
and release baseline**: a fresh clone has nothing else, and nothing else
may legally be bundled.

Keep local runs muted (`BEATBYTE_AUTOPILOT_MUTE=1`) unless you are
listening for something specific.

## Screenshots

```bash
BEATBYTE_AUTOPILOT=1 BEATBYTE_SHOT_DIR=/tmp/shots …    # along a full run
BEATBYTE_SHOT_STATE=settings BEATBYTE_SHOT_DIR=/tmp/shots …   # one screen
```

`BEATBYTE_SHOT_STATE` accepts `menu`, `songselect`, `settings`,
`controls`, `calibration`, `inputtest` and `join` (spelling is forgiving
about case, spaces, hyphens and underscores; an unknown name is rejected
rather than guessed, because silently falling back to the main menu
would photograph the wrong screen and look like a pass). It exists
because the autopilot only ever reaches the menu, the browser and the
results screen — which left the four hand-reached screens as the ones
least likely to be checked after a change.

**Take screenshots in a separate run from any pass/fail verdict.**
Capturing a frame stalls it long enough for the key injector to miss a
note: a song that scores 624 perfect without capture produced 16 misses
and 16 overstrums with it.

## Traps worth knowing

These all cost real time at least once.

- **Wrap long runs in `caffeinate -dis` on macOS.** Display sleep
  removes the monitor, the window closes and the run dies mid-song.
- **A locked screen makes every capture solid black** — and the run
  still reports PASS, so it reads as a rendering bug in whatever you
  just changed. Check the screen before the code:

  ```bash
  python3 -c "import Quartz; print(dict(Quartz.CGSessionCopyCurrentDictionary()).get('CGSSessionScreenIsLocked'))"
  ```

- **An occluded window renders black.** Screenshots of a covered window
  are not evidence of anything; two black frames are md5-identical, so
  a comparison will happily "pass". Prefer ECS-level probes to pictures
  when checking behaviour rather than looks.
- **The game rewrites `settings.json` when it exits.** Editing that file
  to set up a run only works if the game is not running and will not run
  again before the measurement — otherwise the previous session's values
  come back and the run silently tests the wrong configuration. This
  produced a "3D stage" screenshot that was actually the depth view.
- **State-entry screenshots must outlast the transition fade** (0.25 s;
  the harness waits 0.6 s).

## Related

- [Development workflow](workflow.md)
- [Gameplay rules and judgment windows](../gameplay/rules.md)
- [ADR-0004 — deterministic timing](../decisions/ADR-0004-gameplay-timing.md)
