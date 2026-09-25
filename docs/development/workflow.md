# Development Workflow

## Daily commands

```bash
cargo run -p beatbyte            # run the game
cargo test --workspace           # all tests
cargo fmt --all                  # format
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`cargo run -p beatbyte` builds and launches the current checkout, so
changed source files are applied automatically on the next start. When
working with changes pushed by another checkout, update this checkout
first with `git pull --ff-only`; launching an already-built binary does
not rebuild it.

## Quality gate (must pass before every commit)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace
```

## Smoke test

The game supports a self-exiting smoke-test mode used to verify that the
full app (window, renderer, plugins, demo-song build) actually boots:

```bash
BEATBYTE_SMOKE_TEST=1 cargo run -p beatbyte
```

The app exits with a success code once it reaches the main menu.

## Autopilot (end-to-end gameplay validation)

The game can play itself, perfectly, through the real screens and the
real judgment engine:

```bash
BEATBYTE_AUTOPILOT=1 cargo run -p beatbyte
```

Autopilot starts the first song in your library (or the one named by
`BEATBYTE_AUTOPILOT_SONG`), feeds exact-time inputs into
the session, and exits at the results screen — success **only** on a
flawless run (any miss or overstrum in autopilot is a gameplay bug).
Because judgment is input-stamp-driven, autopilot is frame-rate
independent; run it before every release.

Variants:

- `BEATBYTE_AUTOPILOT_PLAYERS=N` — simulated local multiplayer (1–4).
- `BEATBYTE_AUTOPILOT_SONG=<index|title-substring>` — validate a
  specific library song instead of the first (case-insensitive
  substring or numeric index; a selector that matches nothing fails
  the run).
- `BEATBYTE_AUTOPILOT_EDIT=1` — drive the editor (add/undo/redo/save).
- `BEATBYTE_SHOT_DIR=<dir>` — also capture menu/gameplay/results
  screenshots along the way (used for README/docs media).

## Toolchain

`rust-toolchain.toml` tracks **stable**, and CI installs the latest
stable — so keep your local toolchain current (`rustup update stable`).
A stale local stable can pass the gate locally and still fail CI when
a newer clippy ships additional lints; this has happened. When CI's
clippy flags something your local one doesn't, update first, then fix.

## Compile times

The workspace uses the standard Bevy profile split (`opt-level = 1` for
workspace code, `3` for dependencies). The first build is slow (Bevy);
incremental builds are fast. If iteration feels sluggish, consider
`cargo run --features bevy/dynamic_linking -p beatbyte` locally — do not
commit that feature into any Cargo.toml.

## Judging a chart's rhythm without playing it

```bash
beatbyte-cli chart-check <song.m4a> <chart.json> --difficulty medium [--from 63.7 --secs 30]
beatbyte-cli study <song folder> --lead <other.wav>     # the [GS] twin, original untouched
tools/guitar-study.sh                                   # every folder without a twin: decode → demucs → study
```

writes the song with a **click on every chart note** and prints the two
numbers that do not need an ear:

| Number | What it says |
|---|---|
| on the grid | share of notes sitting on a beat subdivision — the generator quantizes, but a note further than 55 ms from every subdivision keeps its RAW detector time, so a chart can silently be a mixture |
| with an attack | share of notes with a detected onset within 55 ms — "a player hears something there" |
| median offset | signed distance to the nearest attack; positive = charted LATER than the music |

A click beside the music is a note beside the music, and that is audible
in one pass where a table of times is not. `--secs` cuts a slice (from
the chart's own preview anchor unless `--from` says otherwise), which is
what makes two variants comparable in half a minute.

Measured on Nothing Else Matters, medium (2026-09-14): the stem-charted
guitar pilot 100 % on the grid, **56 %** with an attack, median
**−1.3 ms**; the mix-charted default 100 % / **62.9 %** / −1.3 ms. Same
phase, different sources — which is why "is there a delay?" and "is the
rhythm right?" are two questions, not one.

## Disk: what `target/` holds, and what may be deleted

`target/debug` reaches tens of gigabytes here, and the parts are not
equally reclaimable (measured 2026-09-14):

| Part | Size | May be deleted? |
|---|---:|---|
| `deps/` | 32 GB | Only as a WHOLE profile directory — never by file age |
| `examples/` | 2.6 GB | Yes, as a whole |
| `incremental/` | 2.9 GB | Yes, always — see below |

**Incremental caches are the part that grows while you work.** Cargo
keeps one directory per *unit* (crate × profile × feature set × target
kind) and one **session** inside it per build. It collects a superseded
session only when it builds that same unit again — so a configuration
that ran once (`clippy --all-features`, `cargo doc`, a one-off
`-p <crate>` probe) keeps its cache for ever. Half an hour of ordinary
work left **95 unit directories holding 2.9 GB**, one of them with a
520 MB session no later build ever collected.

```bash
python3 tools/prune-incremental.py --dry-run      # what it would free
python3 tools/prune-incremental.py                # superseded sessions
python3 tools/prune-incremental.py --stale-days 7 # + configurations unused for a week
python3 tools/prune-incremental.py --self-test    # the rule, pinned
```

This can never break a build: the cost of a deleted cache is one
non-incremental compile of that unit. It does not touch `deps/`.

**Incremental compilation stays ON** — that is a measured decision, not
a default. Editing one file in `beatbyte-game` and rebuilding:

| | Rebuild | Cache written |
|---|---:|---:|
| incremental (default) | **8.0 s** | +196 MB |
| `CARGO_INCREMENTAL=0` | **30.5 s** | none |

So the ~200 MB per edit-and-build cycle buys a 3.8× faster loop and is
worth paying *while iterating*. It is NOT worth paying for a run whose
configuration happens once and is never rebuilt — a full-workspace
`clippy --all-features` or `cargo doc` pass writes caches nothing will
ever read. Put `CARGO_INCREMENTAL=0` in front of those when disk is
tight.

## Syncing two devices

One career on several machines (ADR-0021, plan in
`docs/plans/two-macs-sync.md`). The hub is a directory reachable with
rsync — here `raspi5:beatbyte-hub`:

```bash
beatbyte-cli sync --hub raspi5:beatbyte-hub --dry-run  # what would change
beatbyte-cli sync --hub raspi5:beatbyte-hub            # the first time
beatbyte-cli sync                                      # the hub is remembered
tools/play-synced.sh                                   # sync → game → sync
```

It refuses while the game runs, prints the counts before and after,
and carries the ML models once (`--no-models` to leave them). The API
key, the calibration and the machine's own settings never leave the
device. A failed sync changes nothing half-way that the next one does
not complete; a lock older than 30 minutes is a crashed sync and is
broken.

## Reading the telemetry store

The gameplay store ([ADR-0018](../decisions/ADR-0018-gameplay-telemetry-store.md))
lives beside `scores.json` as `telemetry.db` and never leaves the
machine. Everything below is a read.

```bash
beatbyte-cli telemetry status                 # what is in it, and whether anything was lost
beatbyte-cli telemetry import                 # the older JSONL sessions, once (idempotent)
beatbyte-cli telemetry list --limit 20        # sessions, newest first
beatbyte-cli telemetry show 471               # one run, as lines a person reads
beatbyte-cli telemetry problems <chart_hash> --difficulty 1
beatbyte-cli telemetry generators --genre rock --difficulty 1
beatbyte-cli telemetry calibration --min-hits 200
beatbyte-cli telemetry input                  # strums that reached the engine and did nothing
beatbyte-cli telemetry export --what dataset --out runs.csv
```

`--store <path>` points any of them at another database, which is how
to try something without touching the one the game writes.

### …and what the song was doing there

A miss on its own says a note is hard. What it cannot say is *why*.
The analysis that made the chart — the onset each note came from, the
energy around it, where in the bar it fell — is computed at import and
never kept, so it has to be written down beside the chart:

```bash
beatbyte-cli context --all "$LIB"           # a sidecar per chart version
beatbyte-cli telemetry context "$LIB"       # load them into the store
beatbyte-cli telemetry music                  # what the misses have in common
```

New charts get a sidecar from the generator, which has the analysis
in hand. ⚠️ A **backfilled** one is today's analysis of the song, not
the one the generator saw: the pipeline has moved since some charts
were made. The numbers are still measured from the song and still the
right dimensions to group by; they are not a reconstruction of a
historical run.

Two things worth knowing before reading a number out of it:

- **The autopilot and practice runs are excluded from every analysis**
  in `beatbyte-cli telemetry`. A perfect robot makes every chart look
  easy, and a run played at half speed is not a run.
- **A single miss means nothing.** Every ranking takes a minimum
  sample count and reports the one it had; `confidence` is a
  saturating curve on that count, not a statistical interval.

The `TELEMETRY` row in SETTINGS decides how much is recorded —
`OFF` · `RESULTS` · `ACTIONS` (the default) · `DIAGNOSTIC`. The
`input` analysis needs `ACTIONS` or above, because it asks about the
action stream.
