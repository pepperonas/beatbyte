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
full app (window, renderer, plugins) actually boots:

```bash
BEATBYTE_SMOKE_TEST=1 cargo run -p beatbyte
```

The app exits with a success code after ~2 seconds if everything
initialized correctly.

## Compile times

The workspace uses the standard Bevy profile split (`opt-level = 1` for
workspace code, `3` for dependencies). The first build is slow (Bevy);
incremental builds are fast. If iteration feels sluggish, consider
`cargo run --features bevy/dynamic_linking -p beatbyte` locally — do not
commit that feature into any Cargo.toml.
