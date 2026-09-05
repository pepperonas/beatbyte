# ADR-0013 — Local ML inference: a pure-Rust runtime, models fetched once on explicit action

**Status: Accepted** (2026-09-05, user's decision at the L1 checkpoint of
[`docs/plans/ai-song-graph-upgrade.md`](../plans/ai-song-graph-upgrade.md);
`sha2`, release-asset hosting and the `models` CLI confirmed with it)

## Context

The plan puts three learned models into the game — a vocal separator,
a CTC forced aligner for word-level karaoke, and later a beat tracker
and a note transcriber — and sets four constraints that the runtime
crate (`beatbyte-ml`, milestone L1) has to satisfy before any of them
can be used:

1. **Rust only, one command build.** `cargo run --release -p beatbyte`
   on a clean clone with nothing else installed must keep working, and
   the default build must stay offline and free of the new
   dependencies.
2. **Determinism per platform**, the standing rule for chart generation
   (ADR-0004, ADR-0011): same input, same output, with the same
   documented caveat about last-bit divergence *across* platforms.
3. **No model weights in the repository.** Repo size, the MIT story
   and `docs/development/asset-licenses.md` all depend on it.
4. **The network claim stays true.** The README's "What leaves your
   machine" section and `docs_stay_true.rs` describe every outbound
   path; a model download is a new one and has to be declared as
   exactly what it is: a one-time fetch the user asks for.

The user decided (2026-09-05, plan §11): English first, separation as
its own switch, **no cloud providers**, character-level fill, lyric
offset per song. "No cloud" removes the plan's `cloud` feature
entirely; nothing in this ADR provides for a remote inference path.

## Decision

### Runtime: `rten`

`beatbyte-ml` runs models with [`rten`](https://crates.io/crates/rten)
0.26 (MIT OR Apache-2.0, rust-version 1.94), a pure-Rust ONNX
inference engine. It loads `.onnx` files directly, executes on the CPU
through its own Rayon pool, and exposes a per-run thread pool
(`RunOptions` + `ThreadPool::with_num_threads`) — the knob determinism
needs. Its examples include **wav2vec2** and Whisper, and it ships a
CTC decoder module, so the audio-model coverage the plan needs is
demonstrated upstream rather than hoped for; the `beat-this` crate
(plan C1) is built on it, so Track C shares the runtime.

Measured in a scratch project (2026-09-05, this machine): `rten` +
`sha2` add **38 crates** to the tree, all MIT and/or Apache-2.0, build
in **23 s** in release, and leave 66 MB of artifacts in `target/`.

Alternatives, and why not:

- **`ort`** (ONNX Runtime bindings): the plan's default vocabulary
  ("execution provider", "graph optimization level") is ORT's. It is
  the more complete runtime, but it binds a C++ library that the build
  downloads as a prebuilt binary — a network step inside `cargo`, a
  tens-of-megabytes native dependency to package into the DMG,
  AppImage and portable zips, and per-machine execution providers to
  pin down. It fails constraint 1 in spirit and makes 2 harder.
- **`tract`** (Sonos): pure Rust, mature, broad ONNX coverage. A
  legitimate choice; `rten` wins on the wav2vec2/Whisper examples,
  the `beat-this` reuse and the lighter tree. Should `rten` fall short
  on an operator the aligner needs, `tract` is the fallback, and the
  session API below is small enough to swap.
- **`candle`** (Hugging Face): pure Rust with GPU backends; `wav2vec2-rs`
  offers it. Heavier, GPU-oriented, and its CPU path is not what it is
  optimised for.

### Where models come from

Models never enter the repository. The registry inside `beatbyte-ml`
pins, per model: an id, the file name, **a URL the project controls**
(release assets of `pepperonas/beatbyte`, exported by the maintainer
once, offline), the exact byte size, the SHA-256, and the licence. The
export step (PyTorch → ONNX) is a maintainer tool, documented, run on
the maintainer's machine — it is not part of the build and not part of
the runtime.

A download happens **only on an explicit user action** ("Download"
in settings, or `beatbyte-cli models install <id>`), goes to
`<app data>/beatbyte/models/<id>/`, is written to a `.part` file,
verified against the pinned size and SHA-256 **before** it is renamed
into place, and is never fetched again. No manifest URL is consulted;
the registry is compiled in, so a build knows exactly which bytes it
will accept.

### Determinism

- The thread count is **pinned to a constant** for every run, not
  derived from the machine. L2 measures whether results are bit-
  identical between 1 and N threads on the same machine; if they are
  not, the constant is 1 for the aligner and the cost is accepted.
- Model files are identified by SHA-256; every artifact a model
  produces records the model's hash, the `rten` version and a pipeline
  version, so a cached result never silently changes under the player.
- Cross-platform identity is **not** claimed — `rten` has SIMD paths
  per architecture, and the chart generator already documents the
  same last-bit caveat. Fingerprint per platform, as
  `rock_is_unchanged.rs` does.

### Feature gates

```toml
# crates/beatbyte-game/Cargo.toml, crates/beatbyte-cli/Cargo.toml
[features]
ml = ["dep:beatbyte-ml"]
# apps/beatbyte/Cargo.toml
[features]
ml = ["beatbyte-game/ml"]
```

`default` stays empty everywhere. `beatbyte-ml` is a workspace member
and builds under `cargo test --workspace` (its tests must run in CI);
it is linked into the game and the CLI only with `--features ml`.

### Integrity: SHA-256

`sha2` (RustCrypto, MIT OR Apache-2.0, pure Rust). Considered:
hand-rolling SHA-256 against the FIPS 180-4 vectors, as this
repository hand-rolls its WAV writer and FNV hash. Rejected for the
integrity check of downloaded files that drive a parser on the user's
machine: the standard implementation costs eight small crates and buys
a reviewer's trust that a bespoke one cannot.

## Consequences

- **The README's network section and `docs_stay_true.rs` change in the
  same commit as L1** — a third outbound path exists (opt-in, one
  time, to a project-controlled URL), and the test must name it.
- `beatbyte-ml` carries no domain logic: registry, store, download,
  verification, session cache, pinned execution. The aligner (L2), the
  separator (L6) and the beat tracker (C1) are consumers.
- Memory: a wav2vec2-base model is ~360 MB of f32 weights in memory.
  The session cache holds models by id behind `Arc` and can evict;
  consumers load what they need and release it.
- A licence per model goes into `docs/development/asset-licenses.md`
  when the model is registered — not when it is used.
