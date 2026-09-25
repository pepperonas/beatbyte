# Performance measurement

How BeatByte's frame times are measured. The numbers themselves live
beside this file (`baseline.md`, later `report.md`).

## The bench

`tools/bench.sh <scenario> [out.json]` runs the autopilot with
`BEATBYTE_BENCH` and `BEATBYTE_UNCAPPED=1` — full screen at the
display's native resolution, the song played perfectly — three times
(`RUNS`), and writes one JSON with every run, the spread across runs,
the machine, the load average after each run and the commit. A run
whose autopilot verdict fails is not counted.

| Scenario | Song | Why |
|---|---|---|
| `normal` | Maria (Blondie), Medium | an ordinary song |
| `dense` | [CL] What You Get Is What You See, Expert | 5.4 notes/s, 217 sustains — the densest short chart in the library |
| `hype` | "Heroes", Expert | Hype runs (the autopilot fires it the moment it can), lyrics on screen, the full stage |
| `soak` | as `dense`, back to back for `SOAK_MIN` (15) minutes | heat: the last run is the warm number |

`tools/bench-compare.py before.json after.json` prints both side by side
with the change in percent; `--fail-over 10` exits 1 when p99 got more
than 10 % worse.

## Rules

- **Uncapped.** Under vsync a frame time is the display's pacing, not
  the frame's cost.
- **A quiet machine.** The script refuses above a load average of one
  per core (`MAX_LOAD`, `FORCE=1` overrides) and records the load
  after each run. The first attempt at a baseline on the M1 Pro ran at
  a load average of 19–32 with other sessions busy and measured a
  median of 13–17 ms against 6.2 ms on a quiet machine the same day.
- **Unlocked, in front.** A locked screen or a covered window is not
  rendered; the script refuses a locked screen and raises the window
  every two seconds.
- **No screenshots in a measuring run** (CLAUDE.md): a capture stalls
  the frame it is taken in.
- **The warm-up is dropped**: the first 5 s of song time carry the
  song's own load and are measured once, as `song_start_ms`.

## What each number is

- `frame_ms` — wall time between frames (`Time<Real>`), playing phase
  only.
- `cpu_main_ms` — the main world's schedule, `First` to `Last`. The
  render world runs in parallel on its own thread (pipelined
  rendering), so this is the simulation's share, not the frame's.
- `gpu_ms` — only with `BEATBYTE_BENCH_GPU=1`, and only on a device
  with timestamp queries; `null` otherwise.
- `frames_over_16_7` — frames that would have missed a 60 Hz display.
- `song_start_ms` — entering gameplay to the first frame the song clock
  runs.

## The 2015 MacBook Pro

Build the Intel binary on the M1 (`cargo build --release -p beatbyte
--features ml --target x86_64-apple-darwin`), copy it and the script
over (`ssh mbp2015`), and run it there under `caffeinate -disu` with
someone logged in graphically — a game started over SSH hangs while
that display sleeps. Set `COMMIT=<sha>` there, since the binary is
measured away from its checkout. The result names the GPU; it has to
be the AMD Radeon R9 M370X, not the Iris Pro (which cannot run this
renderer at all).
