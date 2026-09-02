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
| `BEATBYTE_UNCAPPED` | set | Renders without vsync (measurement only): frame times become real cost instead of display pacing. Combine with `BEATBYTE_FPS=1`. |
| `BEATBYTE_SMOKE_TEST` | set | Boots to the menu and exits 0. The cheapest proof that nothing panics on startup. |
| `BEATBYTE_AUTOPILOT` | set | Plays a song perfectly. **Exits non-zero on any miss or overstrum.** |
| `BEATBYTE_AUTOPILOT_SONG` | index or title substring | Which song to play (default: the first bundled one). |
| `BEATBYTE_AUTOPILOT_PLAYERS` | `2`…`4` | Local multiplayer run. |
| `BEATBYTE_AUTOPILOT_DIFFICULTY` | `easy`…`expert` | Which difficulty to play (default: medium). Unknown names and difficulties the song does not offer fail loudly. |
| `BEATBYTE_AUTOPILOT_LOOP` | `<from>,<to>` | Arms the practice section loop (seconds) and verifies it: song time must wrap twice and the section's notes must reopen for judgment. |
| `BEATBYTE_AUTOPILOT_FAIL` | set | The fail drill: plays nothing, switches No Fail off in memory for the run, and passes only if the rock meter emptied, the run is marked failed and the history line says not completed. The one automated path through the failure flow. |
| `BEATBYTE_AUTOPILOT_SPEED` | `50`…`150` | Plays the run at that practice speed (percent) and verifies mid-run that song time advances at the rate — slope over 8+ wall seconds, ±5 %. |
| `BEATBYTE_AUTOPILOT_RATE` | `1`…`5` | Results-feedback drill: presses the real digit key (and RIGHT when the chart has a parent version), then parses the session log back and verifies the feedback lines landed — and finally presses Enter and verifies the navigation lands on the song browser (supersedes the end-of-song verdict in this variant). |
| `BEATBYTE_AUTOPILOT_PAUSE` | set | Mid-song pause-menu drill with real keys: pause, step to the SFX row, adjust down and back up (checked against the exact clamp model), resume — then the run must still finish flawlessly. |
| `BEATBYTE_AUTOPILOT_MUTE` | set | Silences the run. Seeds the mute state only — the in-game `M` toggle still works. |
| `BEATBYTE_AUTOPILOT_KEYS` | set | Presses real `KeyCode`s instead of feeding timestamped inputs, exercising `InputMap` resolution and gameplay routing exactly as a human would. |
| `BEATBYTE_AUTOPILOT_NO_STRUM` | set | Fret presses only, no strum. Proves tap mode is really on (and, with tap off, that it is really off). |
| `BEATBYTE_AUTOPILOT_EDIT` | set | Opens the editor and runs an add / undo / redo / save cycle. |
| `BEATBYTE_AUTOPILOT_DROP` | set | Injects a drag-and-drop import and then plays the imported chart — the only check that covers `import.rs`, which the CLI path does not. |
| `BEATBYTE_AUTOPILOT_DELETE` | title substring | Drives the browser with real arrow and backspace keys through the two-press delete confirmation, deleting the first song whose title matches. Needs a disposable song — it deletes files. (The value is the needle; `=1` matches nothing and times out, which is how this line got corrected.) |
| `BEATBYTE_SHOT_DIR` | directory | Screenshots at named moments of a run. |
| `BEATBYTE_SHOT_STATE` | screen name | Boots straight into one screen, photographs it and quits. |
| `BEATBYTE_SHOT_ROW` | row index | With `BEATBYTE_SHOT_STATE`, selects that row first — in the song browser AND the settings list. A scrolling list is indistinguishable from a short one until the selection moves past the fold. |
| `BEATBYTE_SHOT_SORT` | column name | With `BEATBYTE_SHOT_STATE=songselect`, activates that sort (title/artist/genre/length/notes/diff/best) so the active-column marker is photographable. |
| `BEATBYTE_SHOT_SEARCH` | filter text | With `BEATBYTE_SHOT_STATE=songselect`, opens the search with that filter typed, so the prompt, the first-match selection and the empty-result hint are photographable. |
| `BEATBYTE_ABOUT_EXPANDED` | set | With `BEATBYTE_SHOT_STATE=about`, pre-expands the changelog section (default collapsed), so the expanded state is photographable. |
| `BEATBYTE_AUTOPILOT_MC` | comma-separated title needles | Queues the named songs as an MC set and plays it as one continuous performance — the only automated path through the DJ crossfade between songs. Combines with `BEATBYTE_AUTOPILOT_PLAYERS`. |
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

- **An occluded window renders black** — and this is the common case,
  not an edge case: a full-screen terminal in front of the game window
  is enough. Bevy's screenshot renders that window's surface, so it
  comes back solid black while the run reports PASS.

  Capture **by window ID** instead, which macOS honours regardless of
  stacking:

  ```bash
  ID=$(python3 -c "
  import Quartz
  print(next(w['kCGWindowNumber']
             for w in Quartz.CGWindowListCopyWindowInfo(
                 Quartz.kCGWindowListOptionOnScreenOnly, Quartz.kCGNullWindowID)
             if w.get('kCGWindowOwnerName') == 'beatbyte'))")
  screencapture -x -o -l"$ID" shot.png
  ```

  ⚠️ Match the **owning process**, never the window title: a terminal
  sitting in the project directory is itself titled "BeatByte rhythm
  game", and matching on the title photographs the terminal.

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
