# CLAUDE.md

**Persistent source of truth** for engineering BeatByte. Together with
[`docs/ROADMAP.md`](docs/ROADMAP.md) (what to build, in what order),
this file governs all work in this repository. When in doubt, this
file wins over habit; the roadmap wins over improvisation.

## What this is

**BeatByte** — an original five-lane rhythm game in **Rust + Bevy
0.19** (repo `pepperonas/beatbyte`, MIT, © 2026 Martin Pfeffer, public).
A Cargo workspace of fourteen crates plus a thin launcher (map below);
all logic lives in the crates. UI language is English; the game is
fully keyboard/gamepad driven.

## Orientation

### Crate map

Dependencies point one way only, and `beatbyte-game` is the only crate
that may touch Bevy (the full layering and its invariants:
[`docs/architecture/overview.md`](docs/architecture/overview.md)).

- **`beatbyte-core`** — the domain model, dependency-free: lanes,
  notes, timing, judgment, scoring, `TrackSession`, telemetry.
- **`beatbyte-chart`** (core) — the versioned JSON chart format:
  schema, validation of untrusted input, conversion into core tracks.
- **`beatbyte-audio`** (core) — decode, playback + `SongClock`,
  the analysis pipeline (BPM, beats, onsets, energy), and the
  synthesized `demo` reference tracks.
- **`beatbyte-editor`** (core, chart) — invertible `EditOp`s and
  `EditorSession` with undo/redo.
- **`beatbyte-ml`** (—) — the local ONNX runtime: compiled-in
  registry, verified store, inference. Behind the `ml` feature.
- **`beatbyte-lyrics`** (ml, audio) — forced alignment of lyrics the
  player already has → `words.json`.
- **`beatbyte-meter`** (ml, audio, core) — beats and downbeats from
  the Beat This! model pair.
- **`beatbyte-poly`** (ml, audio) — which notes were struck together:
  Basic Pitch (Apache-2.0) through the pinned runtime, windows cut and
  stitched the reference's way, its polyphonic tracker without the
  step that invents unstruck notes. Read only at authoring time, by the
  classic `chords` ingredient, through the `<audio>.poly.json` sidecar
  `beatbyte-cli poly` writes (ADR-0020).
- **`beatbyte-sync`** (core) — two devices, one career (ADR-0021):
  the merge rules for every kind of data as pure functions, no I/O,
  no transport, no clock. `beatbyte-cli sync` reads and writes the
  files and moves the bytes through the hub (the raspi5).
- **`beatbyte-telemetry`** (core) — the gameplay blackbox (ADR-0018):
  the versioned event history a played song leaves behind, the local
  SQLite store that keeps it, the bounded queue and worker thread the
  game records through, the analytics that read it, and the import of
  the older per-session JSONL files. Engine-free; chart identity
  enters as a hash string.
- **`beatbyte-library`** (core, chart, audio) — song metadata: the
  portable `song.json` a song FOLDER carries (ADR-0019), the stable
  `SongId`, the source/override rules a refresh obeys, and the
  timestamp semantics, plus `build` (a document from a folder's
  facts, pure), `store` (the one file it writes), `index` (the
  queryable projection), `report` (the document as lines a screen
  can draw), `completeness` (what is missing, and the one question
  the background worker asks) and `folder` (what a song folder's
  files are, the content fingerprint, duplicate detection). No
  network.
- **`beatbyte-catalogue`** (chart) — asking MusicBrainz which
  recording a song is. **Optional and encapsulated**: behind the
  `catalogue` feature, the only part of the offline tool that talks
  to a network, and a song is fully playable without it. Everything
  that decides is pure and pinned against a recorded real response;
  only its `Client` touches the network, and the rate limit lives
  inside it. ⚠️ The catalogue's own score is worthless here — eight
  recordings of "Heroes", all scored 100, 0 to 393 seconds — so
  length decides and unlabelled recordings are a TIER above labelled
  ones. See `docs/audio/catalogue.md`.
- **`beatbyte-game`** (everything above) — the only Bevy crate:
  screens, HUD, 3D stage, input routing, library, settings, harnesses.
- **`beatbyte-cli`** (all but game and editor) — the offline tool.
- **`apps/beatbyte`** — thin launcher; it also holds the three gates
  that watch the repository itself: `tests/docs_stay_true.rs` (the
  documents must state what the code actually is),
  `tests/rock_is_unchanged.rs` (the built-in songs' charts are
  fingerprinted — when a change to the default IS intended, updating
  the constant is the deliberate act of recording that) and
  `tests/telemetry_stays_evidence.rs` (the store is read back in
  exactly one place, read-only, and nothing decides from it).

`beatbyte-game` is by far the largest crate. `src/gameplay/` is the
playing screen (input, notes, HUD, lyrics, 3D stage, light show); the
files beside it are roughly one screen each; `ui_kit.rs` owns the
shared look. Screens are `AppState` variants (`states.rs`), and
gameplay is split further by `PlayState` (Playing / Paused / Outro).

### Commands

```bash
cargo run -p beatbyte                          # run the game (debug)
cargo run -p beatbyte --features ml            # …with the learned features
cargo test -p beatbyte-core --lib judge        # one crate, filtered by name
cargo test -p beatbyte --test docs_stay_true   # one test file
cargo run -p beatbyte-cli -- --help            # the offline tool's subcommands
```

The gate that must pass before every commit, and the harnesses that
stand in for a player, are further down this file.

⚠️ The binary the user plays is `target/release/beatbyte`, and it is
built **with** `--features ml` — a plain release build silently
replaces it with a lesser one (gotchas below).

### Where the runtime data lives

None of this is in the repository; it is per-machine state the game
writes. On macOS both directories below are
`~/Library/Application Support`.

- `songs/` (repo-relative) and `<data>/beatbyte/songs/` — the library.
  Imports land in `songs/imported/<song>/`, which is gitignored.
- `<config>/beatbyte/settings.json` — settings. The game REWRITES it
  on exit; read the gotcha before editing it to set up a run.
- `<data>/beatbyte/` — `scores.json`, `players.json`, `history.jsonl`,
  `achievements.json`, **`telemetry.db`** (the gameplay store,
  ADR-0018 — `beatbyte-cli telemetry status` to look; a suspicious
  autopilot verdict reads quickest with `telemetry show <id>`),
  `telemetry/` (the older per-session JSONL files — **nothing writes
  them any more**; they are imported by `telemetry import` and kept),
  and the downloaded ML models.
- Beside each chart version in a song folder: **`*.context.json`**,
  what the analysis said at each of its notes. Generated with the
  chart, or backfilled by `beatbyte-cli context --all songs/imported`;
  gitignored like everything else under `songs/imported/`.

### Where to look before deriving something twice

- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the work itself, in order
- [`docs/decisions/`](docs/decisions/) — ADRs; `README.md` is the index
- [`docs/development/workflow.md`](docs/development/workflow.md) —
  daily commands, toolchain, and what in `target/` may be deleted
- [`docs/development/harness.md`](docs/development/harness.md) — every
  `BEATBYTE_*` switch (a test enforces that the list stays complete)
- `docs/gameplay/`, `docs/audio/`, `docs/chart-format/`,
  `docs/lyrics/`, `docs/ui/`, `docs/releases/` — the living specs

## Autonomous execution protocol

You are the full engineering team: architect, implementer, tester,
tech writer, release manager. Operate accordingly:

1. **Work the roadmap.** Pick the next unblocked task from
   `docs/ROADMAP.md`, complete it to the Definition of Done, check it
   off, commit. Never start a task whose dependencies are open.
2. **Small, verifiable steps.** Every task must end in a state that is
   demonstrably working (test, harness run, or manual verification
   noted in the commit). No long-lived broken intermediate states on
   `main`.
3. **Verify before claiming.** Numbers, screenshots, and "it works"
   statements go into commits/CHANGELOG only *after* the measurement.
   A test you have not seen fail is not an assertion — mutate new
   test pins once to prove they bite.
4. **Root-cause, don't patch.** When a symptom appears, find the
   mechanism before writing a workaround (the v0.8.1 lesson: the
   "camera filter" workaround treated a misdiagnosis; the real cause
   was a failed font asset). Remove workarounds whose hypothesis is
   disproven.
5. **Record what you learn.** New gotchas go into this file's gotcha
   section; scope/plan changes go into the roadmap; decisions with
   alternatives go into `docs/decisions/`.
6. **Keep both sources of truth current.** A completed task is not
   done until the roadmap reflects it. A changed rule is not real
   until it is written here.
7. **Stop only for the user's calls.** Destructive/irreversible acts
   (history rewrites, deleting published releases, license changes)
   and genuine scope changes need the user. Everything else: proceed.

## Architecture rules that must not erode

- **Timing is input-stamp-driven, never frame-driven.** `TrackSession`
  (beatbyte-core) judges from stamped input times against the
  `SongClock` (beatbyte-audio: anchored monotonic time, snap ≥30 ms /
  slew 10% against the device position). Music runs on a dedicated
  thread behind `MusicHandle` (mpsc + atomics). Nothing in gameplay
  may depend on frame rate for correctness.
- **Determinism is a feature.** Core judgment, chart generation, and
  the editor are pure and deterministic; same inputs → same outputs.
  Randomness only via seeded/splitmix hashes.
- **Charts are untrusted input**: validation caps (BPM 20–400, 32 MB),
  path traversal + Windows-drive-`:` rejection in beatbyte-chart. Never
  weaken these; every new chart field gets validated.
- **Players are entities** (`PlayerSession`/`PlayerIndex`/
  `PlayerDevice`); input routes by `DeviceId` (Keyboard vs Pad(Entity)).
  No player-count special cases outside `HighwayLayout`.
- **Menus share one design, not just one font.** `ui_kit` owns the
  type scale (three sizes plus the wordmark), the spacing rhythm and
  the row states; every menu draws its header, panel, rows and footer
  from it. A screen may not invent a font size, a panel frame or a
  selection cue of its own — a test forbids near-duplicate sizes.
- **Editor ops are invertible** (`EditOp::apply` returns the inverse) —
  undo/redo correctness depends on it. Every new op ships with an
  inverse round-trip test.
- **Crate layering**: core knows nothing of Bevy or audio; chart knows
  nothing of Bevy; audio knows nothing of Bevy except nothing (it is
  engine-free); game is the only crate that touches Bevy. Keep it so.
  `beatbyte-ml` (ADR-0013) knows nothing of audio, charts or Bevy —
  registry, store, runtime, floats in and out — and is linked only
  behind the `ml` feature; its registry is compiled in (no manifest
  fetch), a download happens only on the user's explicit action and
  is verified against a pinned size + SHA-256 before it exists. No
  model weights in the repository, not even toy ones: the ML tests
  hand-encode their ONNX graph. `beatbyte-lyrics` sits on `ml` and
  `audio` (no Bevy, no chart): transcript → emissions → one Viterbi
  → `words.json`; its Viterbi is pure and pinned on synthetic
  emissions, and a self-check (align the model's own greedy words)
  is the way to tell a pipeline bug from a hard mix.
  `beatbyte-meter` sits on `ml`, `audio` and `core` (no Bevy, no
  chart): audio → the Beat This! pair → beats and downbeats into a
  `SongAnalysis` under a policy the corpus chose (ADR-0015); its
  chunking, peak picking and merge are pure and pinned, and the
  driver is proven on a hand-encoded ONNX pair through the real
  runtime. It never fetches: `installed()` decides from the store.
  `beatbyte-poly` sits on `ml` and `audio` only, the same way: audio
  → Basic Pitch → notes; its framing and tracker are pure and pinned,
  and the driver is proven on a hand-encoded graph that carries the
  REAL model's input and output names, written out — not borrowed from
  the driver's own constants, or a swap would pass unnoticed. The chart
  crate reads what it writes (`poly.rs`) and never runs a model.
- **No copyrighted assets, music, or trademarks — ever.** All assets
  original, generated, CC0, or OFL (fonts: Press Start 2P and Bebas
  Neue, each bundled with its license). No song ships with the game;
  the synthesized reference tracks live in `beatbyte-audio::demo` as
  test fixtures. No rhythm-game
  trademarks, logos, names, fonts or copied artwork. New assets are
  recorded in `docs/asset-licenses.md`.
  **What this rule does NOT forbid:** the genre's conventions — a
  ruled neck with strings, round gems in GRYBO, a black ring on a
  strum note and a white top on a hammer-on, a rock meter as a dial,
  a power meter as a tube, star-shaped power notes, flames on a hit.
  Those are the vocabulary every guitar game shares, and a commission
  that names a specific game as the reference is a commission to
  adopt its conventions **drawn in our own hands**, not to hedge.
  (Clarified 2026-09-02: the earlier wording "no lookalike trade
  dress" made every reference-driven round hold back — the rock
  meter waited months as "mechanics", typography was never touched —
  when the user's ask was the look.)
- **No secrets in the repo.** No tokens, keys, or credentials, not
  even in CI logs or test fixtures.
- **Telemetry is a first-class domain, not a log** (ADR-0018). It is
  **event-based, never frame-based**; it runs **off the frame thread**
  behind a bounded queue and never blocks; it **references** song and
  chart data (session, chart hash, note index) instead of copying it;
  it stores **nothing derivable** from what it already stores; it is
  **schema-versioned and migratable**, and a shipped migration is
  never edited; it keeps **physical input, logical action and gameplay
  result** distinguishable; it records the **versions** a run happened
  under, because two sessions from different scoring rules are two
  different measurements; and it is **local-first** — no upload, no
  microphone audio, no key that BeatByte is not bound to.

  Analytics over it produce **evidence**, never a change. The game
  reads the store back in **exactly one place** — the statistics
  screen, read-only and off the frame thread, to SHOW the player what
  happened. Nothing reads it to DECIDE anything: achievements still
  derive from the play history (ADR-0017). A third repository gate,
  `telemetry_stays_evidence.rs`, pins that rather than trusting it.

- **Two devices, one career** (ADR-0021). Every kind of runtime
  data has an explicit merge rule in `beatbyte-sync` — never a blanket
  "last writer wins" — and every rule is pure, order-independent and
  idempotent (tests apply it on both devices and compare). Devices
  never write the same file on the hub; sync never runs while the game
  runs; a device setting (calibration, volume, display, GPU-bound
  options, input map) and the API key never travel. A new setting is
  classified as shared or device before it ships (a test fails
  otherwise), and a new player id never comes from a counter.

## Data-driven gameplay

BeatByte is meant to improve from what it measures. That only works
if the measuring stays honest, so when a new gameplay system is
added, ask these in order:

1. Does it create a **player action** that matters?
2. Does the **engine make a decision** that matters?
3. Is there an **outcome** worth analysing later?
4. Can any of it already be **reconstructed** from existing events?
   (Combo, score, accuracy, sustain *starts*, phrase completions and
   "early / late" all can — and so are deliberately not stored.)
5. If not: does it need a new [`EventType`], or does an existing one
   plus a flag say it?
6. Can it **reference** song or chart data instead of carrying a copy?
7. Does it carry the **version** of whatever produced it?
8. Is the write **off the frame thread** and non-blocking?
9. Does it avoid personal data, raw audio, and inputs the game is not
   bound to?

If the answer to 1–3 is no, it does not belong in persistent
telemetry. The question that settles most cases: *will this event
help explain what the player did, what BeatByte expected, or why the
engine produced a particular result?*

Two things that are **out of scope on purpose**: automatic training,
and a chart generator that rewrites itself. Telemetry produces
observability, then data, then analytics, then evidence. What is done
with the evidence stays a deliberate, versioned act.

## Testing requirements

- **Unit tests** live next to the code; the deterministic crates
  (core, chart, audio analysis, editor) carry the bulk. New logic ⇒
  new tests; fixed bug ⇒ regression test that fails on the old code.
- **The quality gate** (below) must pass before EVERY commit.
- **The harnesses are part of the test suite** — run the relevant one
  for any change they cover:

```bash
BEATBYTE_SMOKE_TEST=1 cargo run -p beatbyte     # boots to menu, exits 0
BEATBYTE_AUTOPILOT=1 cargo run -p beatbyte      # plays the demo song PERFECTLY
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_PLAYERS=2   # multiplayer variant
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_SONG=<sel>  # index or title substring
BEATBYTE_AUTOPILOT=1 BEATBYTE_AUTOPILOT_EDIT=1  # editor add/undo/redo/save cycle
BEATBYTE_SHOT_DIR=<dir>                          # + screenshots along the way
BEATBYTE_SHOT_STATE=settings|controls|calibration|inputtest|menu|songselect|join|about
                                                # boot into one screen, shoot it, quit
```

  Autopilot exits non-zero on ANY miss/overstrum — judgment is
  input-stamp-driven, so it is frame-rate independent. Its input feed
  must stay `.before(advance_sessions)`. **Run autopilot before every
  release** and after any change to gameplay, timing, input, or state
  flow. **Local verification runs play the user's imported tracks**
  (`BEATBYTE_AUTOPILOT_SONG="Never Gonna"` etc. — user preference,
  2026-08-26). **BeatByte bundles no songs** (2026-09-05, user: the
  synthesized instrumentals were showing karaoke lyrics) — on a clone
  without a library, run `beatbyte-cli demo` first to materialize the
  synthesized reference tracks, which is also how CI would get one.
  Keep local runs `BEATBYTE_AUTOPILOT_MUTE=1` — and the in-app `M`
  toggle now flips sound live either way (it is a gate inside the
  player: never re-introduce mute as a factor at call sites).
- **Artifacts are tested, not assumed.** Before publishing a release,
  download a CI artifact and smoke-test it (portable layout from a
  *neutral* CWD — that is the layout that has actually broken).

### Quality gate (before EVERY commit)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo check --workspace
```

Plus rustdoc as CI runs it — `RUSTDOCFLAGS="-D warnings" cargo doc
--workspace --no-deps` **without** `--all-features` (and once with):
an intra-doc link into an optional dependency resolves only with the
feature on, and the default-feature docs are what CI builds (an
`[`beatbyte_lyrics::…`]` link in the game crate went red in CI after
passing locally with `--all-features`).

⚠️ The four gate commands above do **not** cover rustdoc, so a broken
doc link reaches main and sits there. Tightening an item's visibility
— a `pub` Bevy system to `pub(super)`, a normal refactor — turns every
intra-doc link that names its path into an unresolved link, and only
this step says so; main was red for four hours that way (0.17.10).
Link to what is public and name the rest in a plain code span.

CI installs the **latest stable** — keep the local toolchain current
(`rustup update stable`); a stale local clippy passes locally and fails
CI (happened twice).

## Documentation rules

- **Numbers in documents are enforced, not maintained.**
  `apps/beatbyte/tests/docs_stay_true.rs` counts what is actually in
  the repository and fails when a document disagrees: the per-crate
  test table and its total, the version against the CHANGELOG and the
  internal dependency pins, the badges that state a fact (workspace
  size, MSRV, ADR count), the ADR index against the files on disk,
  every `BEATBYTE_*` switch against the harness reference, and every
  repository link in the README. Update the document, never the test —
  and if a claim genuinely cannot be checked, do not write it as a
  number.

- **CHANGELOG.md** follows Keep a Changelog: every user-visible change
  lands under its own version's heading in the same commit that makes
  it, together with the patch bump. There is no unreleased section:
  the newest heading is the unreleased state until a tag publishes it.
- **README.md** stays truthful to the shipped state — features,
  controls, screenshots. Screenshots must show the *current* build.
- **`docs/decisions/`** records architecture decisions (numbered
  ADRs) when a choice had real alternatives. Update, don't silently
  contradict.
- **`docs/`** holds the living specs (chart format, gameplay rules,
  audio analysis, workflow). A behavior change that touches a spec
  updates the spec in the same commit.
- **Rustdoc**: `missing_docs` warns workspace-wide — public items are
  documented, and rustdoc must build clean (private intra-doc links
  fail CI).
- **This file and the roadmap are maintained documents**, not
  historical artifacts.

## Git conventions

- **Conventional commits**: `feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`, `test:`, `ci:` (+ optional scope, e.g. `fix(ci):`).
  Imperative subject; body explains *why* and records verification for
  non-trivial changes.
- **`main` is always releasable.** The quality gate passes at every
  commit; harness-covered changes ran the harness.
- **Tags only on working builds**: annotated `vX.Y.Z` tags, placed only
  after the full gate + autopilot pass. A tag may be moved only while
  its release is still unpublished and unconsumed.
- **Never commit**: secrets, build products (`target/`, `dist/`),
  user content (`songs/imported/`), scratch (`temp/`, `local/`).
- Commit messages end with the Co-Authored-By / Claude-Session
  trailers per the harness convention.

## Semantic versioning

- **The patch number rises with every user-visible change**, in the
  same commit that makes the change, so the version a build reports
  identifies that build and not the last release. A `vX.Y.Z` tag is a
  separate act — it triggers the release pipeline — and happens at
  milestones, covering however many patch versions accumulated.
  `apps/beatbyte/tests/docs_stay_true.rs` fails if the manifest ever
  carries a version the CHANGELOG does not describe, and if the
  internal dependency pins fall behind it.
- SemVer, currently **0.x**: minor = milestone/feature releases,
  patch = fixes. 0.x lasts until the gameplay tuning settles
  (see roadmap: "Road to 1.0"); **1.0.0** additionally freezes the
  chart format v1 as a compatibility promise.
- A version bump touches `version` **and all internal dep versions**
  in the workspace `Cargo.toml` (they are pinned to each other), plus
  the CHANGELOG.
- Breaking chart-format or save/settings changes after 1.0 ⇒ major
  bump + migration note. Before 1.0 they still require a CHANGELOG
  entry and, where feasible, a lenient reader.

## Definition of Done

A task/change is done when ALL hold:

1. Quality gate passes (fmt, clippy `-D warnings`, tests, check).
2. New/changed behavior is covered by a test or validated by the
   relevant harness (autopilot for gameplay/timing/input/state flow).
3. CHANGELOG updated for user-visible changes; specs/ADRs/README
   updated where touched; new gotchas recorded here.
4. No copyrighted material, no secrets, no build products entered the
   tree.
5. `docs/ROADMAP.md` reflects the new state (task checked, follow-ups
   filed).
6. Version bumped for any user-visible change, with its CHANGELOG
   entry under that version's heading, in the same commit.
7. Committed with a conventional message; **pushed**; CI green.

For a **release**, additionally: version + internal deps bumped,
CHANGELOG sectioned, annotated tag pushed, CI release artifacts
downloaded and smoke-tested, release published (prerelease while 0.x),
superseded drafts removed.

## Release procedure

Bump `version` + internal dep versions in the workspace `Cargo.toml`,
CHANGELOG entry, commit, `git tag -a vX.Y.Z`, push with `--tags`. The
tag triggers `.github/workflows/release.yml` (~25 min: linux/macos/
windows + DMG + AppImage) and creates a **draft** release — download an
artifact, smoke-test it (neutral CWD!), then
`gh release edit vX.Y.Z --draft=false --prerelease`. Local packaging:
`packaging/macos.sh` (.app + DMG), `packaging/appimage.sh`.

## Hard-won gotchas

- **Asset root**: Bevy's default exe-relative resolution misses the
  workspace `assets/` when running `target/debug/beatbyte` directly —
  and a **failed asset never retries**, so a missed font made ALL text
  silently invisible (v0.8.1 fix). `configure_asset_root()` in
  beatbyte-game/lib.rs resolves explicitly: exe dir → `../Resources`
  (macOS .app) → CWD → exe ancestors. Don't remove it; it runs before
  `App::new`, so its `info!` goes nowhere — debug with `eprintln!`.
- Bevy 0.19 renames: `MessageReader/Writer` (not Events),
  `add_message`, `FontSize::Px`, `Camera2d`, `AudioPlayer`. Plugin
  tuples cap at 15 (split `add_plugins`). KeyCode/GamepadButton serde
  needs bevy's `serialize` feature.
- **Never edit files while a cargo build is in flight** (poisons the
  cache); don't casually flip bevy features (full rebuild, huge target/).
- **A locked screen makes every capture solid black**, harness and
  `screencapture` alike — and the run still passes, so it looks like a
  code fault. Check it before diagnosing anything else:
  `python3 -c "import Quartz; print(dict(Quartz.CGSessionCopyCurrentDictionary()).get('CGSSessionScreenIsLocked'))"`.
  Cost an hour once: shots that had worked minutes earlier came back
  black, and two plausible code hypotheses were built and disproven
  before the screen was checked.
- **Occlusion is the usual cause of a black capture** — a full-screen
  terminal in front of the game window suffices, and the run still
  says PASS. Capture by window ID (`screencapture -l<id>`), matching
  the OWNING PROCESS and never the title: a terminal in this project
  directory is itself titled "BeatByte rhythm game". Working script:
  `docs/development/harness.md`. Re-run before blaming a change — I
  attributed this to my own edits three times in one session.
- macOS: `timeout` doesn't exist; screenshots of an **occluded window
  are black** (first-seconds shots often black — window still coming
  up); `grep -c` exits 1 on zero matches and breaks `&&` chains.
- State-entry screenshots must wait out the 0.25 s transition fade
  (autopilot uses a 0.6 s settle delay).
- **The game rewrites `settings.json` when it exits.** Editing that
  file to set up a test run only works if the game is not running and
  will not run again before the measurement — otherwise the previous
  session's values come back and the run silently tests the wrong
  view (happened: a "3D" screenshot that was actually the 2D one).
- **`BEATBYTE_SHOT_DIR` can make autopilot fail spuriously.** Capturing
  a screenshot stalls a frame long enough for the key injector to miss
  a note window (seen: 16 misses + 16 overstrums on a song that scores
  624 perfect without capture). Take screenshots in a separate run
  from the pass/fail verification.
- **Wrap long local game runs in `caffeinate -dis`** — macOS display
  sleep removes the monitor, the window closes, and the run dies
  mid-song (with the old harness that even faked a PASS; autopilot now
  fails loudly on a vanished window). For visual checks prefer
  ECS-level probes over screenshots: an occluded window renders black
  and md5-identical "evidence".
- **hdiutil "No space left on device" on macOS runners is usually a
  LIE** — v0.9.0's failure showed 95 GiB free at that moment (known
  runner flake). `packaging/macos.sh` retries up to 6×; it also
  reclaims `target/` first (CI-only, gated on `CI=true`) from the
  v0.8.1 episode. Verify the free-space claim in the log before
  believing the error.
- **Verify an action's major exists before pinning it** — fetch its
  `action.yml` from raw.githubusercontent (`using: 'node24'`, inputs)
  instead of guessing; the v5 "fix" for upload-artifact was still
  Node 20.
- **Release asset upload**: `download-artifact` with `merge-multiple`
  preserves subpaths — the publish glob must recurse
  (`artifacts/**`); a flat `artifacts/*` silently drops DMG/AppImage.
- **A printed gate is not an enforced gate.** Counting clippy errors
  and then committing on an unrelated `&&` chain shipped a lint to
  main (caught post-push). Run the gate so a failure *stops* the
  commit: chain the commit off the gate commands themselves, or check
  the count before staging.
- **Never `git checkout -- <file>` to undo a test mutation** — it
  reverts ALL uncommitted work in that file, including the feature the
  test pins (happened; the work had to be redone from context). Back
  the file up to the scratchpad, mutate, restore from the copy.
- **Bevy blends `BackgroundColor` alpha in LINEAR space**, so sRGB
  intuition badly underestimates it: a selection fill written as
  `BRAND.with_alpha(0.12)` rendered as sRGB (99, 84, 35) — a solid
  olive bar, not the whisper intended. Sample the rendered pixel
  rather than reasoning about the constant. The same trap sits in a chain of
  `Color::mix`: `mix` converts the right operand into the LEFT one's
  space, so `palette::dimmed(c, 0.16).mix(&c, 1.0).mix(&WHITE, 0.3)`
  mixes that white in LINEAR — where 0.3 is most of the way there,
  not a sheen. And when the emissive is computed FROM the base
  colour, whitening a surface raises what the bloom pass sees on it
  just as surely as raising its glow would: the star impulse's
  whitening had to be weighted per surface for exactly the reason
  its glow already was.
- **A second on-screen camera makes every untargeted UI root
  invisible.** With the 3D stage camera active alongside the 2D
  camera and no `IsDefaultUiCamera` marked, bevy_ui cannot pick a
  target for root nodes: they lay out to ZERO size — entities exist,
  input works, nothing renders. The pause menu shipped invisible
  exactly this way, and the drill that "verified" it was green
  because it checked settings values, not pixels or layout. Twin
  rules: the persistent `Camera2d` carries `IsDefaultUiCamera` (do
  not remove it), and **a UI drill must assert the thing is
  VISIBLE** — `ComputedNode::size() > 0` at minimum (the pause drill
  now does), or an engine-side screenshot of the state.
- **Two cameras on one window carry TWO contracts, and the 8-bit
  3D stage broke both** (black void, HUD only, hidden behind that
  one settings combination). (1) **Cameras sharing a window must
  agree on HDR**: `Bloom` requires `Hdr`, and a mixed pair (SDR 2D
  over HDR stage) silently drops the HDR camera's entire pass — the
  round style worked only because its bloom made both cameras HDR.
  Since v0.14.44 there is only one style and both cameras carry
  Hdr+Bloom from birth, which is why `sync_bloom` — the system that
  kept them in step, and the place the bug lived — is gone. The
  rule still binds anything that adds a third camera.
  (2) **Exactly one camera may clear**: once the stage
  actually renders, the 2D camera's default clear would wipe it —
  `sync_stage_compositing` flips it to `ClearColorConfig::None`
  while a stage camera exists (and back: menus must clear, or they
  smear — a ghost of the song browser burned into gameplay was the
  telltale). The pause drill pins both rules and was seen to fail
  under each mutation. Twin lessons: when a rendering bug is
  reported, test the settings MATRIX (the view, and whatever else
  forks the render), not just the defaults — and **a screenshot that contradicts expectations
  is evidence, not an artifact**: the missing stage in an autopilot
  shot was waved off as a capture quirk hours before the user
  reported the same black screen.
- **The user plays from `target/release/beatbyte`, and that build
  carries `--features ml`.** `ml` is NOT a default feature (ADR-0013:
  the default is the offline game, and the shipped releases turn it
  on). A plain `cargo build --release -p beatbyte` therefore replaces
  the player's binary with a LESSER one: ALIGN answers "this build
  has no aligner", the lyrics-model row says NOT IN THIS BUILD, and
  the 450 MB of models already on the machine become unreachable.
  Rebuilding their binary means
  `cargo build --release -p beatbyte --features ml` — measured, not
  assumed: `ls target/release/deps | grep -c beatbyte_lyrics` is 0
  without it. Cost an evening's confusion once, reported as a bug in
  the game.
- **Never leave an experimental build there.** During an A/B bisect the half-reverted
  binary blacked out the user's LIVE game mid-song ("ich sehe
  highway und die töne nicht mehr"). Experiment builds go to a
  separate target dir (`CARGO_TARGET_DIR=/tmp/...`), and the real
  path gets rebuilt from committed state the moment the experiment
  ends. Related: a render change that passes every drill can still
  be black on screen — judgment does not need pixels; luma-check the
  engine screenshots (play AND pause) before keeping any
  camera/HDR/bloom change.
- **`dist/` is a build product.** It was once committed (118 MB, and
  CI shipped the stale DMG inside every artifact); it is gitignored —
  keep it that way.
- **An occluded window returns a STALE frame, not only a black one.**
  `screencapture -l<id>` hands back the compositor's last image of a
  covered window: a run 60 s into a song photographed as the main
  menu, and a whole 160-frame series came back md5-identical while
  the log advanced. The clue is a metric that does not move at all
  between frames. Raise the window before every capture
  (`osascript … set frontmost of (first process whose name is
  "beatbyte") to true`) — the capture script in the scratchpad does.
- **The autopilot hits by STAMP, so a clock that teleports forward
  is invisible to its verdict.** Every note before the landing point
  is played in that one frame, perfectly, and the run passes with an
  empty highway. That is how the count-in teleport (v0.14.8) shipped
  under a green harness for two days. The autopilot now fails a run
  in which song time jumps forward >0.5 s in one frame; when a
  screenshot shows a score or combo that does not change across a
  run, suspect the clock before the HUD.
- **A fresh timeline follows no device position until its song is
  anchored** (`GameClock::begin` + `anchored`). Start every new
  timeline through `begin()`, never `clock.start()` directly: the
  device reports whatever played last — the browser preview, the
  previous song — for a few frames after `stop()`, and a length
  bound cannot reject a position that lies inside the new song.
- **A lossy container's timeline is not the decoder's timeline.**
  Symphonia 0.5.5 plays AAC priming as audio (1024 frames for FFmpeg,
  2112 for Apple); `beatbyte-audio::priming` reads the declaration
  and BOTH decode paths skip it through the same count
  (`decode_file`, `playback::open_trimmed`). Never let one path skip
  without the other — that puts every chart a frame off the audio.
  And never skip by `rodio::skip_duration`: it truncates 1024 frames
  to 1023 through nanoseconds. Charts say which timeline they are on
  (`audio_trim`); a chart without it is moved once by the scan.
- **A backup keyed by folder name loses the second folder of that
  name.** The first migration run keyed by `<song folder>/<file>`;
  the same import existed in both scan roots, and the second original
  was rewritten without a backup. Key by the absolute path.
- **`serde_json` without `float_roundtrip` can move a float by one
  ULP on load.** Seen on 2 of 970 sustain lengths after a rewrite
  (2e-16 s). Harmless in play; remember it before treating a
  load→save round trip as byte-identical, and before enabling the
  feature: it would re-hash charts.
- **Photographing a moment of a run: `BEATBYTE_SHOT_TIMES`**
  (`=17.5,39` with `BEATBYTE_SHOT_DIR`) names frames by song time —
  the fixed moments (24 s, 44 s) never land on a lyric lead-in or a
  countdown. Two traps met while using it: the raised window
  **steals the user's focus** and a stray key paused the run (the
  harness sat in the pause menu until the timeout — kill the run
  once the wanted frames are on disk instead of waiting for PASS),
  and occlusion **flickers** on a busy desktop: raise the window
  every ~2.5 s across the target window, not once at the start
  (frames at 44 s came back fine and frames at 62 s black in the
  same run). A frame is evidence only after its luma is checked.
- **A drill that arrows through the song browser must count rows in
  the BROWSER's order** (`BrowserView.order`, from where
  `BrowserCursor` sits), not in `SongLibrary.entries` order — the
  browser is sorted, the library is scan order. The first align
  drill pressed `K` on the wrong song and reported "no lyrics
  beside this song" for one that had them. And **keys pressed into a
  screen's 0.25 s entry fade are dropped**: the first model drill
  arrowed into the void and confirmed a row it never meant to. Wait
  ~0.8 s after the state change, as the screenshot harness does.
- **A sweep in one condition can invert in another.** Tuning the
  aligner's anchor window on corpus stamps that were on time said
  ±1 s beats ±4 s on every metric. Run with the stamps three seconds
  off — the condition twelve songs in the real library are actually
  in — and it reverses, losing a song outright. Before a measured
  number changes a default, ask which condition the real data is in
  and measure THAT one too; a single-condition sweep is a hypothesis,
  not a result.
- **Pin the decision, not only the branches.** A test that checked
  the tight window and the wide window separately stayed green while
  a mutation made the code always pick one of them: nothing tested
  the CHOICE. Where behaviour hinges on a condition, lift the choice
  into its own pure function and pin that — the branches will follow.
- **A forced alignment always returns a path, and a path is not
  evidence.** On a mix the model cannot read, the anchored pass —
  which centres each line's window on the shift it is meant to
  test — simply places the words inside those windows. The deltas
  then reproduce the guess and the "consensus" measures the window,
  so a song the model heard as one letter in six seconds came back
  "shifted master, 85 % agreement, −3.34 s" and ran three seconds
  early on screen. Any statistic computed from a fit against the
  thing that produced the fit will confirm it. Ask the input a
  question the fit cannot answer about itself — here, the model's own
  greedy letter rate — and set the threshold against measured error,
  not against the elbow of a distribution (`docs/lyrics/evaluation.md`).
- **A currency check that compares a generated chart with a loaded
  one is dead code.** `serde_json` (without `float_roundtrip`) moves a
  float by one ULP on load; a fresh generation never hashes like the
  file read back, so `redesign --all` wrote a new version of EVERY
  song on every run — 46 of 70 byte-identical to their parent but for
  the provenance — and nobody noticed because a rollover looks like
  work. Compare what a reader would GET: send the fresh chart through
  save → load before hashing (`redesign::is_current`). The proof that
  such a check works is a second run that writes nothing.
- **A legible, agreeing, anchored alignment can still be wrong.** An
  anchored window that cannot hold the truth places the words at its
  own edge, and on a legible stem that reads as evidence: *Mexico*
  came back "shifted master −3.65 s, 85 % agreement" from stamps that
  were ten seconds late (another edit; the plain pass put the verse
  where the model heard it). Two tells, both now checked: the pass
  sits in the outer 15 % of its window, or its confidence collapses
  to under half the plain pass's. And a held final word ("sein") the
  model cannot hear gets **parked** at the next line's onset, 14 s
  late — a gap over 2 s inside a line is a parked word. When a user
  says the lyrics are off, run the plain pass and the `voice` probe
  on the stem before believing any verdict.
- **A container's length is not the song's.** *Mexico* is 283 s of
  file and 168 s of music; the tail is digital silence. The catalogue
  lookup matched the wrong entries by the container's length, the
  text had fifteen verses the recording never sings, and a stamp in
  the silence is "past the end". `AudioData::sounding_end_s` is what
  every length rule now uses (the lookup still asks with the
  container's length — open).
- **Never pattern-match a background job you started from the same
  shell.** `grep '[q]ueue.sh'` matched MY OWN zsh (its command line
  also contained the restart), and the kill loop took the tool call
  down with it (exit 144). Kill by exact process name (`pkill -x`) or
  walk the PID's parents; never by a substring the current command
  line also carries.
- **Restarting an eval after a code change means restarting the
  QUEUE.** A running `lyrics-eval` keeps the binary it started with;
  three rebuilds during one run measured three different pipelines
  under one report name. Rebuild, then kill and restart the queue,
  then leave the code alone until it finishes.
- **The autopilot's clock-teleport rule false-fails under load.** With
  `redesign --all` and a separator saturating the CPU, a run reported
  "song time jumped 4.515 → 5.505 in one frame" on a chart that had
  not changed. Re-run on a quiet machine before believing it.
- **Files under a running game are live.** Deleting chart versions
  and moving pointers while the user played turned every Enter on that
  song into "cannot load" until a rescan (eleven error lines in two
  seconds). The browser now re-resolves a vanished version from the
  folder's pointer; still, batch edits to `songs/imported/` while the
  game runs are edits the user sees.
- **A chart's constant `bpm` is not its grid.** The analysis tracked a
  time-varying grid since Phase 2; the generator, the highway, the
  phrases and the editor kept counting on the constant one — 1.61 s
  apart by the end of a live recording, and nobody saw it because
  every hit is within 55 ms of SOME eighth of any grid. When a stage
  produces a better version of a quantity, check who still consumes
  the old one (`grep` the field), not just who produces it.
- **A snap that recomputes a position moves it by float noise, and
  float noise rewrites files.** `a + k · step` is not the stored
  float; `snap_notes` moved every already-snapped note by ~1e-15 s,
  the hash changed, and `redesign --all` wrote seventy versions on a
  run that should have written none — one layer under the ULP bug
  fixed the same morning. A move under a microsecond is not a move.
  The proof that an idempotent step is idempotent is the second run.
- **Loudness is measured on the channels the player plays.** The
  analysis decode averages to mono; a wide stereo mix read that way
  is up to 3 dB quieter than what comes out of the speakers, and the
  whole point of the level is what is heard. `decode_file_channels`
  exists for that; never feed the meter the mono path.
- **A container's bitrate is not evidence of quality; the spectrum's
  end is.** Four video rips in the library sit at 233–389 kbps AAC
  and end at 9.9–13.4 kHz. And **modern masters reconstruct a few
  tenths over 0 dBTP between samples as a matter of course** — the
  first pass flagged 42 of 70 files for it; that is a note, a full dB
  over is a flag.
- **Silence repeats itself perfectly.** The first run of the
  structure stage on *Mexico* reported "2:49–3:45 repeats at
  3:45–4:42, similarity 1.00" — the rip's 115 s of digital silence,
  centred on a song mean the silence had dragged toward itself.
  Silent beats (RMS under −60 dBFS) get no features and match
  nothing, and the centring runs over SOUNDING beats only; both are
  pinned. Any feature that is normalised against a song-wide
  statistic has to ask what the statistic was computed over.
- **A policy measured on one genre is a hypothesis on the next.**
  The corpus (house, pop, DJ grids) said "take the model's grid";
  the first rock folders of the rollover said the model hears
  double time there (230.8 vs 112.5 BPM) on charts the ear had
  approved at the tracker's level — the "sweep inverts in another
  condition" lesson, one row down. The rule that shipped adopts the
  model only at the tracker's level and says so; measure the
  library's condition (here: tempo ratio per song) before the
  corpus's verdict touches it.
- **A guard tuned to one reading refuses the next reading of the
  same thing.** `redesign` protected the merge with "tempos within
  0.1 BPM" — the tracker against itself, where only float noise
  differs. The meter reads the same grid 0.1–0.9 BPM differently (a
  median interval against an autocorrelation) and the first library
  rollover with it refused two folders in three. A guard against a
  DIFFERENT grid has to name what a different grid is (another
  metrical level: a third, a half, double — a 5 % ratio), not the
  noise of one method. Pin both sides: the other level refused, the
  other reading accepted.
- **Two mutation runs must never overlap.** A second run that starts
  while the first is still going snapshots its "original" from a file
  the first has already mutated — and then restores THAT at the end,
  leaving the mutation in the tree and every later reading worthless.
  It happened here: `judge`'s quality branch came back as `if false`,
  the probe that "caught" it had actually read a contaminated file,
  and only the suite going red afterwards showed it. Wait for a run to
  finish, and re-read the probe results of any run that overlapped
  another. (Restoring is by scratchpad copy, never `git checkout` —
  and for a NEW file git could not have restored it at all.)
- **The game window can open on a second display that renders
  nothing** — measured at (−1473, −76) on this machine. Every capture
  then comes back PURE BLACK: the engine's own
  `Screenshot::primary_window()` too, because Bevy has no rendered
  frame to read back. The screen was unlocked and the desktop
  captured fine, so both usual explanations were wrong. Ask the
  window where it is (`System Events` → `position of window 1`) before
  blaming anything else, move it onto the main screen, and then
  RE-READ the position: a raise or a move that silently fails leaves a
  fixed crop photographing whatever else is there — mine grabbed a
  Mail window and looked plausible enough to reason about. Capture the
  window's own rect (`screencapture -R`), never the whole screen.
- **The song clock steps BACKWARDS at the count-in handover**, and a
  little more whenever it corrects itself against the audio device (a
  30 ms snap, a 10 % slew). Anything scheduled on song POSITION must
  therefore tell a correction from a new timeline: the light show's
  first cut restarted its burst schedule on any backwards step, so the
  top of every song flashed on six beats running — the flicker the
  pacing exists to prevent (seen in the probe log, not in any test).
  The rule is a threshold — a step back bigger than the longest gap
  the schedule can plan is a new song, anything smaller is a clock
  correcting itself and the schedule simply carries on.
- **Two `cargo` runs in one target directory serialise on its lock**,
  so a CLI rebuild waits behind a Bevy release build. Build the CLI
  into its own directory (`CARGO_TARGET_DIR=<scratchpad>/target-cli`)
  when a game build is in flight — and remember the rule about not
  editing a crate that IS in the in-flight build's graph.
- **`pkill -x beatbyte-cli` kills every CLI, the eval queue's job
  included.** Stop one run by PID, found with the bracket trick
  (`ps -eo pid,command | grep '[r]edesign songs'`), which matches
  neither the grep nor the shell that carries the pattern.
- **The input layer judges a same-frame fret change BEFORE the
  strum** (`gameplay/input.rs` sends the five fret edges, then the
  strum, all stamped with the frame's time). Any hit rule that
  punishes "strum after the fret already hit" therefore punishes the
  natural motion on every HOPO: with the chain alive the press hit
  the note and the strum matched nothing — hit plus overstrum, streak
  gone, and the autopilot never saw it because it strums only pending
  notes. The session now absorbs the one strum that lands inside the
  window of a fret-hit note (0.14.36). When a judgment rule is added,
  ask what the SAME-FRAME ordering does to it, and pin it with inputs
  in that order.
- **`AmbientLight` is a component ON THE CAMERA in Bevy 0.19**
  (`#[require(Camera)]`, `bevy_light/src/ambient_light.rs`). Spawned
  on its own entity — as the stage did from v0.11 to v0.14.36 — it
  lights nothing and quietly becomes a phantom camera that no drill
  sees; the stage ran on the engine's white default ambient the whole
  time. Light components go where the engine says they go, and a
  lighting change is verified by looking, not by reading the code
  that "sets" it.
- **A blended or additive mesh casts a SOLID shadow** in Bevy 0.19's
  shadow pass (`pbr_functions.wgsl` discards only masked materials).
  Every haze sheet, beam mantle, halo, lens and grille cloth carries
  `NotShadowCaster`, and a wired test walks the venue for any
  non-opaque material without it. The neck opts out of shadows both
  ways (`stage3d::on_the_neck`): it is a reading surface.
- **A normal map without tangents is silently flat.** Every mesh that
  takes one goes through `surfaces::tangent_mesh`; merge pieces first,
  generate tangents after (`Mesh::merge` wants identical attribute
  sets). Data tiles are `Rgba8Unorm`, colour tiles sRGB; a normal map
  in sRGB leans every normal through the gamma curve.
- **Bright smooth metal under the rig is fairy lights.** The corner
  caps of the PA stacks, chrome in the first pass, stood directly under
  the moving heads and bloomed into white blobs in two consecutive
  frames — dark and matte is what a corner protector is anyway. A
  material that catches a spot at close range needs roughness before
  it needs colour.
- **A leg fold is not symmetric.** "Thigh forward θ, shin back 2θ" only
  keeps the foot under the hip when the two segments are equal; with
  the foot in the shin's length the sole drifted 1.8 mm — the FK test
  caught it. The shin's angle is `asin(THIGH/SHIN · sin θ)`.
- **`RenderLayers` is per entity, never inherited**, and a joint or
  limb without the stage layer is simply invisible; the figure builder
  puts it on every entity it spawns and a wired test walks the tree.
- **`BEATBYTE_SHOT_TIMES` frames are named `beatbyte-gameplay-t<time>.png`**
  — a capture loop that stops on "two PNGs exist" stops on the menu
  shot and the fixed `gameplay-phrase` frame, before the timed ones.
- **On the ProMotion panel a vsync frame time is the refresh rate's,
  not the GPU's.** After the stage realism pass the vsync medians read
  10.00 ms (100 Hz) with stretches at exactly 16.66 ms (60 Hz) — the
  panel choosing a rate, and the 60 Hz stretches were the window not
  being frontmost; a run without the window raised showed 16.66 ms
  even with `BEATBYTE_UNCAPPED=1`. Measure GPU cost uncapped AND with
  the window raised every half second (`stage/uncapped.sh` pattern):
  9.1 → 10.8 ms was the real change, +19 %.
- **A background cargo build gets killed for "low memory" long before
  the OS would kill it.** The session's task watchdog stopped three
  release builds in a row (thin LTO + `codegen-units = 1` on the game
  crate, a 16 GB machine with an emulator and other sessions up) —
  and a kill mid-link leaves NO `target/release/beatbyte` behind. The
  same build finished in the foreground with `-j 2`. Build the
  release in the foreground when the machine is loaded, and check
  the binary exists before running anything.
- **Never prune `target/` by file age.** Deleting `deps` files older
  than a day also took the `.d` files of build scripts, and every
  build after that died on "No such file or directory" until the
  whole profile directory went. Reclaim space by removing a whole
  `target/<profile>` (a full rebuild is cheaper than a broken cache),
  and keep an eye on it: it had grown to 70 GB and filled the disk
  mid-session, at which point no tool could even open its output
  file. A lasting symptom of one of those episodes is
  `couldn't read .../libsqlite3-sys-*/out/bindgen.rs`: the crate's
  build-script output went and cargo does not notice it is gone.
  `cargo clean -p libsqlite3-sys` is the whole fix, and it comes
  back every few weeks. **The one part that IS safe to prune is `incremental/`**, and
  it is the part that grows while you work: cargo collects a superseded
  session only when it rebuilds that same unit, so a configuration that
  ran once (`clippy --all-features`, `cargo doc`, a one-off `-p <crate>`
  probe) keeps its cache for ever — half an hour of work left 95 unit
  directories holding 2.9 GB, one with a 520 MB session nothing ever
  collected. `tools/prune-incremental.py` drops them; the worst case is
  one non-incremental compile of that unit (measured on
  `beatbyte-game`: 8.0 s incremental against 30.5 s without, for
  196 MB of cache per edit-and-build cycle — which is why incremental
  stays ON for iterating and belongs off (`CARGO_INCREMENTAL=0`) for
  one-shot full-workspace passes). ⚠️ Predicting what a deletion frees
  means counting only files whose **link count is 1**: cargo hard-links
  unchanged files into the next session, so the first version of that
  tool summed file sizes and claimed 814 MB where `du` then measured
  393.
- **What the eye sees at a light is its BEAM, not its light.** The
  ceiling strobe flashed its `SpotLight`s white and read on the far,
  small fixtures and not on the near, large ones — reported, then
  measured: the near cones' saturation did not move at all
  (0.711–0.723 over six frames of a running strobe) because the
  additive mantle wears the fixture's colour and was never touched.
  A fixture that flashes has to flash its mantle and its lens too,
  by swapping the material HANDLE (a shared material may not be
  written per frame). And a flash is a property of the flash: a
  factor on the fixture's own intensity made a 900 000 head flash a
  third as hard as a 3 000 000 rim.
- **A colour filter is a hypothesis about the renderer.** Counting
  "warm white" pixels (`r-b > 15`) to find the light strips returned
  ZERO across eight frames, five of which the log proved had an
  effect running — and the strips were there all along: emissive
  white through bloom and tonemapping arrives neutral or clipped
  (r=g=b), so the filter excluded exactly what it was looking for.
  The house rule that a tool reporting 0 needs a counter-check
  applies to image measurements too: difference two frames, or crop
  and LOOK, before believing a feature does not render.
- **`caffeinate -dis` does not stop the screen LOCKING.** Half a
  session's measurements were taken on black frames because the
  display locked on the idle timer a few minutes into each run — and
  a black frame measures as "the feature is not visible", which sent
  two hypotheses down the wrong road. `-d`/`-i`/`-s` only prevent
  sleep; **`-u` asserts user activity**, which is what holds off the
  idle lock: `caffeinate -disu -t 1200 <binary>`. It cannot unlock a
  screen that is already locked, so check `CGSSessionScreenIsLocked`
  first and check it AGAIN when a measurement says nothing changed.
- **A locked screen is not only a black capture: it is a whole
  session without eyes.** Plan verification at ECS/log level from the
  start — `info!` lines that carry the measured values (the monitors
  and the light show log theirs) prove a feature ran; a screenshot
  can only ever confirm it.
- **Work that outlives a screen does not belong on that screen's
  systems.** The song search was registered in `SongSelectPlugin`, so
  its poll stopped the instant a song started: the task kept running
  in the pool and nobody collected it. Anything that runs for tens of
  seconds — a search, a lookup, an import — gets its own plugin with
  a plain `Update` registration, and says where it is on the import
  overlay, which is spawned outside every screen for exactly that
  reason. The pin for it is a wired test on an app that has **no**
  `AppState` at all: if the system asked for a screen, it would not
  run there.
- **`#[serde(default)]` fills a missing field with the FIELD's
  default, not the struct's `Default`.** Flipping `ai_search` to true
  in `Settings::default()` reached nobody: every existing
  `settings.json` has the field written, and a file that predates the
  field would have taken `bool::default()` — false. A default that
  must reach existing files needs `#[serde(default = "…")]` naming a
  function, and existing files that carry the old value need editing
  or migrating; the type's `Default` alone changes nothing for anyone
  who has already played.
- **A mutation-probe helper that restores from the wrong path stacks
  mutations silently**, and every reading after the first is
  worthless — a later probe "passed" here only because three earlier
  mutations were still in the file. Two guards, both cheap: name the
  backup after the FULL path (the `basename` trap already cost a
  clobbered stylesheet in another project), and check `git status`
  is clean between probes rather than trusting the helper's own
  `cp`. And a mutant must compile: a probe that fails to build
  proves nothing (one here needed `IntoScheduleConfigs` in scope).
- **A summary inside this repository is not a source.** Round six of
  the look plan built the gem from a trait table an earlier round had
  written ("dark ring, white centre") instead of from the material,
  and shipped the wrong structure — the user's report was exact:
  "alle Buttons haben jetzt einen weißen Punkt." When a commission
  names a specific reference, re-check the visual claims against it
  (or say plainly that it could not be reached), and look at the
  result against the reference before measuring anything. Pixel
  metrics prove a change happened; only looking proves it is the
  right one.
- **A run that begins inside a song must COUNT IN there, and the
  music thread must START there — a seek sent after `play` is
  already too late.** The blind taste test plays a window from the
  middle of a track. Two wrong builds, both actually made: (1) count
  in to zero, then seek clock and audio forward — the clock teleports,
  which the autopilot's guard catches; (2) count in to the start, send
  `play_file` then `seek_s`, and hold reconciliation — STILL a
  teleport (`0.230 → 172.339`, the first drill run), because the
  clock ANCHORS to the first position it sees under a new generation
  and the hold only governs reconciliation after that. A seek on an
  m4a takes ~0.25 s to land; the anchor had long grabbed zero.
  `MusicHandle::play_file_from` is the fix: one command, the thread
  seeks BEFORE it announces the generation, so the first position the
  game sees is the start. `PendingMusic` carries where a run begins;
  any new "start somewhere else" mode goes through that field and
  that command, never through a seek after the fact.
- **A batch that decodes songs to scratch fills the disk while a gate
  runs beside it — and fails quietly, one song at a time.** The first
  library run of `tools/guitar-study.sh` (a 40 MB WAV plus two stems
  per song) shared the machine with the quality gate and a second
  `target/`; at 14:57 the disk hit zero and eleven consecutive songs
  failed with `No space left on device`, after which space came back
  and the run went on as if nothing had happened. Twelve refusals in
  a log that ends "written 59" read like twelve hard songs. Before a
  scratch-heavy batch: 40 GB free, no parallel builds, and delete each
  song's scratch once its output exists (the script now reports the
  failures by name — the first version lost that list to bash 3.2's
  `set -u` on an empty array).
- **A drill nobody runs rots without a sound.** `BEATBYTE_AUTOPILOT_DELETE`
  still answered the delete question with a second Backspace months
  after the answer became `Y` (Backspace only ASKS since a player lost
  songs while clearing text) — it could not have passed, and nothing
  said so, because no gate runs it. It also counted rows in library
  order while the browser shows them sorted, and asked "is it gone?"
  by title, which after a deletion still finds the `[GS]` twin. When a
  screen's keys change, grep the autopilot for the drills that drive
  that screen; and a drill identifies its target by what cannot
  change under it (its files), not by what it shares with a twin.
- **A number whose meaning is unknown must not decide anything.** The
  classic chord ingredient first took its minimum spacing (200 ms) from
  a field of the early games' configs, `min_combo_spacing`, that the
  research itself marks "meaning unbelegt". On a real song at 156 BPM
  it blocked every chord run on eighths — evidence at 36 % of the
  events, chords written at 9 % — and nothing in the suite said so,
  because every fixture was slower. A value marked [U] in a research
  note is a question, not a parameter.
- **Test a small discrete domain exhaustively, not at its edges.** A
  chord shape is five lanes × twelve intervals × two class counts ×
  four levels — 480 cases, microseconds. The first tests picked lanes
  0, 1 and 4 and passed; the first real song put a wide pair on yellow
  (lane 2, three frets up, two frets of room either way) and the
  `lane - span` underflowed. Where the whole input space fits in a
  loop, loop over it and assert the properties.
- **Which output of a converted model is which is a measurement.**
  Basic Pitch's ONNX names its outputs `StatefulPartitionedCall:0/1/2`,
  meaningless in themselves. A synthetic tone settled it: read with
  `:1` as the note head and `:2` as the onset head, a sustained A4 and
  a struck A-major triad came back as one note of the right length and
  three notes starting within 3 ms of the strike — the peaky onset head
  read as notes could not have produced either. The driver test now
  writes those names out itself.
- **An autopilot verdict could be failed by the room, and the
  telemetry says so.** Real device input went into the same session
  the injector plays into: a key, a pad button or a click at the desk
  added strums the chart never asked for. Seven of 305 recorded
  sessions carried overstrums **with every note still hit perfectly at
  0.0 ms** — more strums than notes, which the injector cannot produce
  (it strums at most once per pending note). One of them failed a
  Metallica playtest that passed untouched minutes later, and cost an
  evening spent looking for a chart defect that was not there. Since
  0.15.8 the injector owns the inputs
  (`autopilot::injector_owns_input`). When a verdict looks impossible,
  read the run's own judgment log —
  `~/Library/Application Support/beatbyte/telemetry/<ms>-p<n>.jsonl`,
  one line per note (`i`, `j`, `off_ms`), per sustain (`s`, `done`)
  and per overstrum (`o`, `near` = the last judged note) — before
  reasoning about the chart: all notes hit plus overstrums is the
  room, not the music.
