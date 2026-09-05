# CLAUDE.md

**Persistent source of truth** for engineering BeatByte. Together with
[`docs/ROADMAP.md`](docs/ROADMAP.md) (what to build, in what order),
this file governs all work in this repository. When in doubt, this
file wins over habit; the roadmap wins over improvisation.

## What this is

**BeatByte** — an original 8-bit five-lane rhythm game in **Rust + Bevy
0.19** (repo `pepperonas/beatbyte`, MIT, © 2026 Martin Pfeffer, public).
Cargo workspace: `crates/beatbyte-{core,chart,audio,editor,cli,game}` +
`apps/beatbyte` (thin launcher; all logic lives in the crates). UI
language is English; the game is fully keyboard/gamepad driven.

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
   alternatives go into `docs/adr/`.
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
  hand-encode their ONNX graph.
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
- **`docs/adr/`** records architecture decisions (numbered ADRs) when a
  choice had real alternatives. Update, don't silently contradict.
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
  rather than reasoning about the constant.
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
  `sync_bloom` now keeps Bloom+Hdr in step on BOTH cameras per note
  style. (2) **Exactly one camera may clear**: once the stage
  actually renders, the 2D camera's default clear would wipe it —
  `sync_stage_compositing` flips it to `ClearColorConfig::None`
  while a stage camera exists (and back: menus must clear, or they
  smear — a ghost of the song browser burned into gameplay was the
  telltale). The pause drill pins both rules and was seen to fail
  under each mutation. Twin lessons: when a rendering bug is
  reported, test the settings MATRIX (view × note style), not just
  the defaults — and **a screenshot that contradicts expectations
  is evidence, not an artifact**: the missing stage in an autopilot
  shot was waved off as a capture quirk hours before the user
  reported the same black screen.
- **The user plays from `target/release/beatbyte` — never leave an
  experimental build there.** During an A/B bisect the half-reverted
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
