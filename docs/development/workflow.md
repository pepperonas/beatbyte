# Development Workflow

## Daily commands

```bash
cargo run -p beatbyte            # run the game
cargo test --workspace           # all tests
cargo fmt --all                  # format
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

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

Autopilot starts the bundled demo song, feeds exact-time inputs into
the session, and exits at the results screen — success **only** on a
flawless run (any miss or overstrum in autopilot is a gameplay bug).
Because judgment is input-stamp-driven, autopilot is frame-rate
independent; run it before every release.

Add `BEATBYTE_SHOT_DIR=<dir>` to also capture menu/gameplay/results
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
