# Changelog

All notable changes to BeatByte are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**How versions move here.** The patch number rises with every
user-visible change, so the version a build reports always identifies
that build rather than the last release. A `vX.Y.Z` **tag** is a
separate act: it triggers the release pipeline and publishes
artefacts, and happens at milestones. So a version section exists as
soon as the code carries that version; the git tags record which of
them were published. `apps/beatbyte/tests/docs_stay_true.rs` fails if
the manifest ever carries a version this file does not describe.

## [0.13.36] - 2026-09-03

### Fixed

- **The "leaving search" panel is opaque.** Seen with the screen open
  after 0.13.34 shipped: the kit's frame is translucent by design and
  the song rows read straight through the bar. The plate is solid
  now and the list dims behind it. (A 96 % alpha was tried first and
  still showed the row text as legible grey — Bevy blends in linear
  light.)
- **Under an empty search the screen tells the truth.** The "no match
  for …" hint quoted the first letter that emptied the list (`q`) for
  the rest of the word (`queen`), because an empty order equals an
  empty order and the rows never rebuilt; the rebuild key includes
  the filter while the list is empty. And the details line kept the
  LAST song's figures under the empty list ("1/71  121 BPM … 336
  notes" beneath "no match") — it is blank when nothing is
  highlighted.

## [0.13.35] - 2026-09-03

### Changed

- **The debug overlay opens on `L`** (the key left of `1` still
  works; `F3` is gone — macOS takes it for Mission Control unless a
  system setting says otherwise, so it worked on some machines and
  not others). Inside a song `L` is bound to nothing else; the
  browser's `L` (lyrics lookup) is a different state, and a test
  asks the binding map, not the source text, that no fret, strum,
  hype, pause or menu key is the overlay's.
- **The frame rate is the overlay's headline**: a large figure in the
  display face above the table, green at the display's rate, amber
  under 55 fps, red under 30.
- **The overlay is a table.** Every line is a section label
  (`CLOCK`, `FRAME`, `TEMPO`, `NOTES`, `P1`…, `SET`) followed by up to
  three cells of key and right-aligned value in fixed columns, so a
  figure stays in its place while it changes and two rows of the
  same kind read against each other.

## [0.13.34] - 2026-09-03

### Fixed

- **A search beginning with q works** (`song_select.rs`). Since
  0.13.17 a tap of `q` left the search instead of landing in it —
  the search reader tested the physical key before the typing loop
  and dropped the typed character with it — so nothing whose title
  or artist starts with q could be found. `q` is a letter again.
- **Every word of the filter has to match, in any column.** The
  filter was one phrase looked for inside each column separately,
  so "queen rhapsody" (an artist and a title) found nothing. It is
  split on whitespace now and each word must appear somewhere in
  title, artist or genre; every phrase that matched before still
  does, and leading, trailing and doubled spaces no longer matter.
- **The search shows what was typed.** Characters were lowercased on
  the way in and folded again when applied; the field now keeps the
  text as typed (folding happens only when the filter is applied),
  so Backspace removes exactly the character that was typed — a
  letter whose lowercase form is two code points used to leave a
  stray half behind.

### Changed

- **Leaving the search is a HELD q.** Hold `q` for one second: a bar
  in a centred panel fills over that second, and when it is full the
  search closes and keeps its filter (Esc still closes AND clears).
  Release before the second is up and the q is typed instead — the
  letter is written on release, and the moment another key arrives
  a pending q is written first, so rolling from q into the next key
  keeps its order. The OS's key repeats of the held q are not
  letters. Pinned with a keyboard-message harness and six mutation
  probes.

## [0.13.33] - 2026-09-03

### Added

- **A debug overlay** (`gameplay/debug_overlay.rs`): during a song,
  the key left of `1` (`Backquote` on a US board, `IntlBackslash` on
  a German one — both count) or `F3` shows a monospace block under
  the mode badge with live facts read straight from the game state:
  song clock, visual clock, the audio device's position and the
  drift between them, clock rate and pause state; smoothed frame
  time and fps, entity count, autopilot; tempo and beat, note events
  judged and left; per player score, streak, multiplier, accuracy,
  judgment counts, overstrums, hype and rock meter, mean hit
  offset, held frets, active sustain, spawn cursor and fret heat;
  the settings that affect judgment and drawing. Off by default,
  toggled at any moment without a restart, read-only — it borrows
  everything immutably except its own text, plate and frame
  average. Each toggle is logged.

## [0.13.32] - 2026-09-03

### Changed

- **Settings rows are alphabetical** by their label, BEAT PULSE to
  VIDEO OFFSET, and a test keeps them that way: a row added at the
  end of the list — the natural mistake — fails the suite and names
  the pair that is out of place.

## [0.13.31] - 2026-09-03

### Added

- **The hit flame is fire** (round style; `gameplay/flame.rs`). A
  hit lights three nested bodies per fret — a white-gold core, an
  orange-to-lane mantle, a dark aura in the lane's colour — each
  with its own flicker in height and lean (two incommensurable
  rates, 9–14 Hz), on a rounded foot; six embers rise from the tip
  with buoyancy and a sway and cool from yellow through orange to
  red; a small warm light follows the flame so the board takes its
  glow. Three phases: ignite (life overshoots to 1.15, near-white
  core), flare, die (height falls faster than girth, colour cools).
  Everything is pre-spawned and driven by pure functions of life,
  time and a seed — no allocation per frame, no per-hit spawn. Rapid
  hits re-raise the flame rather than stack; a new ember shower
  needs 80 ms since the last. `reduced_flashing` removes flicker and
  embers, `particles` off removes embers, `fx_intensity` scales
  height and ember count. The neon stage keeps its cone.
- **The Star-Power arc** (`gameplay/arc.rs`). While Hype runs, both
  rails crackle with lightning: chains of thin additive segments
  whose endpoints jump to a new shape 24 times a second, with gaps,
  forks thrown off outward and up, and the odd bright flash — the
  genre's electric edge, in the project's own vocabulary. Fixed
  pools, one shared material, transforms only. Under
  `reduced_flashing` it becomes a slow wander with no gaps or
  flashes. (Commissioned mid-round: a first pass built a sheet of
  flame licks along the rails, which read as an ice fence — the user
  asked for the bolt instead, and the fire came out again.)
- Round-style hit sparks in the 2D layer now **rise and cool** like
  the 3D embers (buoyancy instead of gravity, the same colour ramp);
  the 8-bit confetti keeps its colour and its fall.

### Changed

- **Every note on the instrument neck is the same size** (user:
  "alle Töne sollen gleich groß sein"). The HOPO's white cap is its
  mark; the smaller face on top of it made the notes look uneven.
  The neon stage keeps its smaller HOPO.
- Fret heat (press, hit, held) is published as a resource
  (`FretHeat`) by the receptor system, so the flame reads the same
  numbers the ring, the fill and the burst do.

## [0.13.30] - 2026-09-03

### Added

- **The round style has a voice of its own: Bebas Neue** (OFL,
  bundled with its licence, recorded in `asset-licenses.md`). Bold,
  condensed, all-caps — the register a stage HUD and a setlist speak
  in — set for headings, rows and readouts. Chosen by measurement,
  not taste: of the three candidates, only Bebas Neue has fully
  tabular digits (all ten at one advance), which the score counter
  needs; Oswald had ten widths and Anton a narrow 1. Until now the
  round style borrowed the engine's 95-glyph monospace fallback for
  everything.
- Because its capitals reach 70 % of the em where the pixel face
  fills it, the display face is set at 1.3× the nominal size — the
  first capture had row labels the height of their own margins. The
  type scale stays one scale.
- The engine's monospace face keeps two jobs in the round style: the
  karaoke line (laid out glyph by glyph on a fixed advance) and data
  text — the watch-folder path — where all-caps would misrepresent
  what is there.

### Fixed

- The round style no longer folds "Motörhead" to "Motorhead": the
  new face draws the letters the old fallback could not. The fold
  lives on only for the karaoke line's face, as `mono_safe`.
- The About screen's changelog bullets showed their Markdown —
  `**bold**`, backticks, link syntax — as long as entries have used
  it; the display face made it impossible to overlook. Stripped.

## [0.13.29] - 2026-09-03

### Added

- **A band on the stage** (round style). Four original figures built
  from the crowd's own primitives — singer at a stand, guitarist and
  bassist flanking, drummer on a riser with a kit — on a platform
  whose front edge is exactly the neck's far end, so the neck runs
  INTO the stage and nothing on it can ever sit over a note. They
  play: bodies bob on the beat (higher under Hype), the guitarist
  strums every beat and the bassist every other, the drummer's hands
  alternate on the beat and the off-beat, the singer sways over two
  bars and raises a hand while Hype runs. Every pose is a pure
  function of song beats, pinned. A warm, ranged wash lights them so
  they read as figures and not as more crowd. The first placement,
  seven units further back, had them 65 % into the fog — measured,
  moved, scaled up; the README shot is the current build.

## [0.13.28] - 2026-09-02

### Changed

- `CLAUDE.md` now says what the asset rule does **not** forbid: the
  genre's conventions, drawn in our own hands. The old wording ("no
  lookalike trade dress") had made every reference-driven round hold
  back. It also records the round-six lesson as a gotcha: a summary
  inside the repository is not a source.

## [0.13.27] - 2026-09-02

### Fixed

- **Strum notes no longer wear a naked white dot.** User report:
  "alle Buttons haben jetzt einen weißen Punkt." Researched against
  the genre's own documentation instead of a summary of it — two
  sources agree: *"in the middle of the coloured note is a white
  circle; regular notes have a black circle **around** this white
  circle, hammer-ons don't"*, so a strum note reads as a **black ring
  on top** and a HOPO as a **solid white top**. Round six had put the
  dark ring OUTSIDE the cap and a bare dot in the middle, which is
  the wrong structure. Now, round style only: a strum note carries a
  black ring on its face around a small centre point (the ring is
  the mark, 44 % of the gem; the point 16 %), the outer edge is thin,
  and a HOPO keeps its large white cap and no ring. Pinned. On a
  capture the ring measures ~100/255 against a 208 cap — dark, not
  black, because the cap's bloom glows over it.

## [0.13.26] - 2026-09-02

### Added

- **A rock meter** (optimization plan P3, commissioned 2026-09-02).
  The crowd's verdict, 0–100 %: starts at 50 %, a judged hit adds
  2 % (doubled while Hype runs — the boost is now a rescue, not only
  a multiplier), a miss takes 5 %, an overstrum 2 %. The rules are
  data in `ScoreConfig`, quoted in `docs/gameplay/rules.md`, and a
  test fails if the document and the constants disagree — exactly,
  not rounded.
- **Settings → NO FAIL**, on by default: the meter moves and shows,
  the song never ends on it. Off arms failing in a solo run: an
  empty meter ends the song there — the outro stamps *BOOED OFF!*
  over the live stage in the house's own words, the results carry
  an **F** in red with *FAILED (no record)*, no scoreboard entry is
  written, and the play history logs the run as not completed. A
  run fails exactly once (the transition is latched and pinned).
  With more than one player the meters show but never end the song:
  one player's bad patch should not cut another's song short.
- **The HUD's right plate is now the genre's corner**: the crowd's
  dial with its needle (tinted by zone — cyan while the room is with
  you, amber under half, and a red pulse under a quarter), and the
  Hype tube beside it with four quarter ticks and the READY line,
  breathing when it can fire and white-hot while it runs.
  Multiplayer highways gain a zone-tinted meter bar under their Hype
  bar.
- **`BEATBYTE_AUTOPILOT_FAIL=1`**, the fail drill: plays nothing,
  switches No Fail off in memory for the run, and passes only if the
  meter emptied, the run is marked failed and the history line on
  disk says not completed. The one automated path through the
  failure flow — verified live: empty after 11 misses at 12 s,
  results F, history `completed: false`, the user's settings file
  untouched.

## [0.13.25] - 2026-09-02

### Changed

- **Hammer-on gems wear a bigger white cap** (user: "der weiße knopf
  in den hammer button soll größer sein"). A strum note keeps its
  small centre dot inside the coloured cap; a HOPO's centre is now a
  cap of its own — 68 % of its face — with a thin coloured ring left
  around it, which is the genre's at-a-glance "no strum needed".
  Measured on a capture: cap 71 % of the face width on a HOPO, none
  on the strum note beside it. Round style only; the 8-bit gems are
  untouched.

## [0.13.24] - 2026-09-02

### Changed

- **The round style's neck is an instrument, not a light show**
  (plan: `docs/ui/gameplay-look-plan.md`, round six). Measured
  against what the genre's classic does, the framing was already
  right and the difference was what the neck was *made of*: five
  coloured glowing lane lines, glowing rails and trim. Now — in the
  round style only, the 8-bit stage is untouched and a test pins the
  gate — the board is dark and warm, the lane lines are one pale
  metallic string for all five lanes (lane identity lives in the
  buttons and the gems), the rails are chrome binding with the theme
  left to the decorated trim, and the far end fades into the venue so
  notes emerge from the dark. Neck saturation 0.20 → 0.11, brightness
  0.33 → 0.18; the back wall recedes to 0.13 from 0.28.
- **Gems are buttons.** A white centre on every gem, a near-black
  bezel ring, a more saturated cap. Sustains are thinner rails with a
  pale core that keeps its own light while held.
- **Solo HUD plates are stage chrome.** Neutral frames and white
  digits instead of the player-colour outline and tint (border
  saturation 0.65 → 0.11); the hype gauge keeps its colour — the meter
  is what is coloured, not its housing. Multiplayer keeps the player
  colours: with four necks, the colour is how you find your numbers.
- **The hit word sits lower and smaller on the instrument neck**, beside the
  strike instead of over the approach where the notes are.

### Added

- **Settings → HIT LABELS.** On (the default — nothing changes for
  anyone who did not ask) keeps PERFECT / GREAT / GOOD / MISS; Off
  gives the genre's flame-only feedback. HYPE! still announces
  itself either way: it is a state change, not a grade.

### Fixed

- A colour helper written for this round turned greys red (HSL keeps
  a grey's hue at 0); its own test caught it before it shipped.

## [0.13.23] - 2026-09-01

### Changed

- **The beat grid now follows the music instead of being laid across
  it.** The pipeline produced one period and one phase for a whole
  song; v0.13.21 measured that this cannot work on a 6–8 minute
  track, because a relative tempo error accumulates while the ±70 ms
  tolerance does not. The grid is now tracked by dynamic programming
  over the onset envelope (`beatbyte-audio::analysis::beats`, after
  Ellis 2007), so each beat only has to sit one period after the
  previous one and error cannot pile up.
- Onset detection additionally produces a **kick channel** — the same
  spectral flux restricted to 30–130 Hz, computed in the existing FFT
  loop. It is what the tracker follows, and its value is that it
  cannot hear an offbeat hi-hat, which is the tie the old phase fit
  kept losing on four-to-the-floor material.
- Measured on the real corpus, mean beat F-measure goes **0.278 →
  0.840**, with four of seven tracks at 1.000. Drift is gone (the
  residual stays bounded start to end where it used to grow to
  1238 ms) and the offbeat lock is gone (first-beat phase now within
  ±0.094 beats on all seven, against four tracks at −0.25 to −0.47).
- **Rock did not merely hold, it improved**: `circuit-breaker` goes
  from 0.000 to 0.982, which fixes the 146 ms phase error v0.13.21
  found, and `solder-groove` holds at 0.995. No case with ground
  truth anywhere in the repository got worse, which is why this is
  the shipped default. Note density is unchanged to one decimal on
  every track: the grid moved, the notes did not.
- Newly generated charts therefore differ from ones generated before
  this version. Existing chart files are untouched, and nothing about
  gameplay timing changed — the grid is used when a chart is made,
  not when it is played.
- The kick weight was **measured, not chosen**: mean beat F runs
  0.530 / 0.588 / 0.733 / 0.840 across weights 0.0 / 0.5 / 0.75 / 1.0.
  My first guess was 0.75 on an argument that turned out to be wrong,
  and the code says so where the constant lives.

### Added

- `apps/beatbyte/tests/rock_is_unchanged.rs` — the rock regression
  gate the commission asked for: both built-in songs must generate
  byte-identical charts. Exactness rather than a 2 % tolerance, since
  a real behaviour change can hide inside a tolerance. Its own honest
  limit is documented: it catches the architecture, not the tuning.
- `AnalyzerConfig` is now serialisable end to end, with the new grid
  settings inside it rather than in a second configuration beside it.
  A round-trip test proves it, since a `derive` proves nothing.

### Fixed

- The README said "same audio in → **bit-identical** charts out". The
  new rock gate disproved it within an hour of existing, in two
  stages. First it failed on Linux while passing on macOS, because
  generation runs through `ln`, `exp` and trigonometry and platform
  libm implementations differ in their last bits — so the gate moved
  to comparing charts at millisecond resolution. That fixed one of
  the two built-in songs and **not the other**, which says the
  divergence there is larger than a millisecond: a threshold
  comparison resolves differently, and a note is kept, dropped or
  snapped elsewhere. Rounding harder would have hidden a real
  property of the pipeline, so the gate records a fingerprint per
  platform instead and the README now says what is true:
  reproducible per platform, every time; not across platforms. The
  "no randomness" half was always correct.

### Performance

- 469 s of music analysed in 3.6 s, about 130× real time, against a
  10 s budget for a 7-minute track. The tracker costs roughly a tenth
  of a second.

## [0.13.21] - 2026-09-01

### Added

- The analysis baseline is now measured against **real music**:
  Rekordbox's own beat grids, read from its `ANLZ0000.DAT` analysis
  files (`beatbyte-audio::eval::anlz`), paired with the audio on this
  machine (`eval::corpus`). Seven tracks of the target profile —
  loop house, 118–130 BPM, 5–8 minutes — now stand behind the
  numbers in `docs/audio-eval-baseline.md`. No audio, no grid and no
  library file enters the repository; the corpus is a local path the
  examples take as an argument.
- `eval::corpus` also carries the two pieces of arithmetic the
  measurement turns on, both tested: the nearest-beat `residual`,
  which **wraps at half a period** and therefore must never be
  subtracted from another, and `accumulated_drift_s`, the wrap-free
  drift a tempo error buys over a track's length.

### Changed

- **The diagnosis in `docs/audio-eval-baseline.md` is now founded on
  real material, and it contradicts the assumption it started from.**
  The tempo estimate is not the weak point: seven real tracks come
  back within 0.25 %, with no octave error anywhere. The failure is
  phase, from two separate causes — a single global constant tempo
  cannot hold a 6–8 minute track (every track drifts past the ±70 ms
  tolerance, the worst by a factor of 18), and four of seven lock
  onto the wrong half of the beat. Median beat F-measure on real
  material is **0.33**, against 0.86–0.98 on the synthetic cases,
  which is the most useful thing the synthetic cases have said: they
  do not reproduce the defect.
- The earlier rock finding is folded into the same explanation
  rather than standing as its own mystery: `circuit-breaker` shows
  both causes on cleaner, shorter material.

### Fixed

- **The README claimed the game never uses the network.** That
  stopped being true when the lyrics lookup shipped, and nothing
  about a stale badge looks stale. The badge now says what actually
  happens, a new *What leaves your machine* section names the one
  request, when it is made and what it carries, and
  `docs_stay_true.rs` fails from now on if the claim and the code
  disagree in either direction.

## [0.13.20] - 2026-09-01

### Added

- An EVALUATION HARNESS for the analysis pipeline
  (`beatbyte-audio::eval`), so its quality can be measured before it
  is tuned: MIREX beat scores (F-measure at ±70 ms, CMLt, AMLt),
  downbeat and boundary accuracy, and the game-side note-density
  distribution. The metric definitions are pure and tested against
  cases whose answers can be worked out by hand — half tempo must
  fail CMLt and pass AMLt, a burst of detections may not "hit" a
  whole bar.
- Ground-truth sources: the JSON sidecar from the brief verbatim, and
  a Rekordbox XML importer (`Inizio`/`Bpm`/`Battito`, tempo changes
  included). ⚠️ Ableton `.asd` is deliberately NOT parsed — it is an
  undocumented binary format, and guessing its layout would put
  invented facts into the measurement everything else is judged by.
- Synthetic corpus cases reproducing the material properties that
  break the pipeline on sample-based loop house: two overlaid timing
  rasters, soft transients, a filter sweep, and a flat
  four-to-the-floor. Ground truth is exact by construction. Every
  number taken from a description rather than measured carries an
  `// ASSUMPTION:` comment.
- `docs/audio-pipeline-ist.md` (the pipeline as it stands, with each
  material property mapped to the code line it breaks at) and
  `docs/audio-eval-baseline.md` (the measured baseline).

### Notes

The harness found three things on its first run, none of which
changes behaviour yet: the rock reference's beat grid sits **146 ms
off the music** (invisible in-game, because the chart is generated
from the same grid); the second timing layer of a two-raster track is
**discarded entirely** (128 onsets for 128 beats, note density
halved); and downbeat accuracy is 0 wherever the material is a flat
4/4 — there is no downbeat stage at all.

## [0.13.19] - 2026-09-01

### Fixed

- The CRT power-on now OPENS the window instead of playing over one
  that has been open for seconds. It ran at the main menu, which is
  reached only after the songs finish building — the boot screen
  ("tuning the amps…") was already on display long before. It plays
  on the first frames the window presents, so the first thing on
  screen is the tube.
- A stuttering boot can no longer skip the show. The animation
  advanced by raw frame delta, and boot frames are long (assets,
  pipelines, the first draw): two 300 ms frames consumed the entire
  power-on. One frame may now advance at most 1/30 s, so a hitch
  stretches the animation instead of eating it.
- The titlebar's X plays the power-off. `close_when_requested` is
  switched off and the CRT answers the close request, so the two
  most common ways to quit no longer skip the animation entirely.
  ⚠️ Verified by clicking the real button: the window still closes.

### Added

- SONG FOLDER in the settings names the directories the library is
  actually read from, under the panel. The value line says whether
  a folder is WATCHED for new tracks; this answers the different
  question a player asks when a song is missing. The paths come
  from the scan's own list (`library::scan_roots`), so the screen
  cannot describe a folder the game does not read.
- `BEATBYTE_SHOT_ROW` reaches the settings list too — a row below
  its fold was as unphotographable as a song below the browser's.

## [0.13.18] - 2026-09-01

### Changed

- The CRT power-on and power-off use the source's REAL easing
  curves. The first port interpolated its keyframes linearly, which
  is what made the tube read as a mechanical wipe: `inspector-rust`
  assigns a different `cubic-bezier` to every segment, and those
  curves are most of the character. They are solved here (bisection
  on the parameter, because the parameter is not the x axis) and
  checked against values CSS itself produces.
- The scanline BLOOMS instead of ending at a hard edge — a
  three-stop gradient that fades either side of the bright core.
  This is where the source's `brightness()` filter went: a mask
  cannot brighten the picture, so the light goes where the filter
  would have spilled it.
- The power-off ends on a BURNOUT flare, the bright point a real
  tube dies on, rising over the pinch and gone before the last
  frame — the app never exits on a lit screen. Suppressed under
  reduced flashing, which is exactly what that setting is for.
- The power-on runs 900 ms, the top of the range the source
  documents. The real easings are front-loaded, so dot, scanline
  and opening are all over inside the first 56 %; at 700 ms that
  performance was finished in 390 ms. The offsets are untouched —
  only the total changed.

## [0.13.17] - 2026-09-01

### Added

- The game powers on like a TV TUBE and collapses to a dot when it
  quits — the animation ported from `inspector-rust`
  (`core/frontend/src/lib/md3-motion.ts`, `playCrtOn`/`playCrtOff`),
  keeping its offsets, its front-loaded power-on and its rule that
  the power-off is DERIVED from the power-on so leaving can never
  become slower than arriving. That app scales an HTML shell; a Bevy
  window has none, so the same shape is drawn as a mask of black
  panels closing to a bright scanline and pinching to a dot.
  ⚠️ It plays when the FIRST MENU appears, not at startup: the boot
  screen is empty while the songs are still being built, so a
  power-on there revealed nothing and was over before there was
  anything to see. The duration is 700 ms rather than the source's
  250 ms — inside the range that app documents (80–900), at the
  visible end, because a game being launched is not a popup someone
  is waiting to type into.
- Every menu with a way back now has a CLICKABLE one: song browser,
  settings, controls and about carry a "< …" button under the
  footer that names the key. The keyboard and pad paths are
  untouched.

### Fixed

- `q` closes the song search instead of being typed into it. It
  leaves the filter in place (Esc still closes AND clears), and the
  key press is consumed so the same press cannot both leave the
  field and drop a "q" in it. ⚠️ The cost: a title containing "q"
  can no longer be typed into the search.

## [0.13.16] - 2026-09-01

### Changed

- The in-app history export writes to the platform's DOWNLOADS
  folder — `$HOME/Downloads` on macOS, `FOLDERID_Downloads` on
  Windows, `XDG_DOWNLOAD_DIR` on Linux — instead of the data
  directory next to the save files, which is where a person
  actually looks for a file they just exported. The Linux value
  comes from the user-dirs config and can be absent; the old
  location stays as a documented fallback, and the settings row
  reports the real path either way.
- Exports never overwrite each other. The file is named
  `beatbyte-play-history-<date>.csv` (UTC, the same clock the rows
  inside use, so a name and a row cannot disagree), and a second
  export the same day becomes `-2`, `-3` … Downloads is the
  player's own folder and an export is often the thing they are
  about to send: silently replacing yesterday's file would be data
  loss.

## [0.13.15] - 2026-09-01

### Added

- The play history exports from INSIDE the game: SETTINGS →
  EXPORT PLAY HISTORY writes `play-history.csv` beside the log and
  then shows the path on the row itself — an export that only says
  "done" leaves you hunting for the file. CSV, because the in-app
  button exists for handing a list to someone; the CLI keeps both
  formats and the filters.
- The song browser marks which tracks have lyrics with a small
  microphone at the head of the row. ⚠️ Drawn from nodes, NOT the
  🎤 character: Press Start 2P has 656 glyphs and that is not one
  of them — rendered, it comes out as the font's `.notdef` box
  (verified by rendering it and comparing the bitmap against a
  private-use codepoint). Songs without lyrics keep the same space
  empty so the titles stay on one left edge.

### Changed

- `SongEntry` carries `has_lyrics`, set during the library scan as
  a file check rather than a parse: the browser rebuilds its rows
  on every view change, and reading fifty lyric files to draw fifty
  markers would be work for nothing.
- The CSV rendering moved to `beatbyte-core` beside the schema, so
  the game and the CLI write byte-identical files.

## [0.13.14] - 2026-09-01

### Added

- A PLAY HISTORY: one line per played track, appended to
  `history.jsonl` beside `scores.json`. It carries the work (title
  and artist as separate fields, never the score board's
  collision-prone joined key), the difficulty, when the run started,
  how long it actually ran in WALL-CLOCK seconds, the song's own
  length, whether it reached the end, the player count, the score
  and accuracy, and flags for practice and autopilot.
- `beatbyte-cli history` exports it: `--format csv` for reporting
  (one row per performance, quoted properly so a title with a comma
  stays one column) and `--format json` for analysis (every field).
  Filters: `--from-ms`, `--until-ms` (half-open, so neighbouring
  periods cannot report the same run twice), `--min-seconds`,
  `--exclude-practice`, `--exclude-autopilot`, `--completed-only`.

### Changed

- The history records every run and lets the export decide which
  ones count. It is deliberately NOT the telemetry log, which skips
  practice runs on purpose so slowed evidence cannot poison the
  design loop - a track played at half speed was still played, and
  a report of what was performed may not have a hole in it. Dropping
  a run at recording time would be unrecoverable; filtering it at
  export is one flag.

## [0.13.13] - 2026-09-01

### Added

- Lyrics look themselves up. `L` in the song browser asks lrclib's
  catalogue for the highlighted track and caches the result as an
  `.lrc` beside the audio, where the loader already looks. The call
  is the one that has been finding lyrics reliably in
  `inspector-rust`'s Shazam mode: same endpoint, same two query
  parameters, same ten-second timeout, same reading of a 404 as an
  empty catalogue entry rather than a failure. Anonymous - no
  account, no key, no configuration - and only the artist and the
  title leave the machine. Deliberately a key press: it is the one
  moment BeatByte talks to the network.
- Every outcome is a state the player can read: found (with the
  line count), "lyrics exist but carry no timing", "not in the
  catalogue", or the failure's own reason. A lookup never ends in
  silence.
- The passage being sung now carries a BACKGROUND HIGHLIGHT: a deep
  amber band behind the active line, while the lines around it keep
  their ordinary look.

### Changed

- Unlike the source it was ported from, the response's
  `syncedLyrics` field is what BeatByte keeps - `inspector-rust`
  prefers `plainLyrics` and strips the timestamps out, because it
  only displays words. A track with words but no timing is reported
  as exactly that, instead of as "no lyrics".

## [0.13.12] - 2026-09-01

### Fixed

- Scrolling lists no longer let the selected row walk off the edge.
  `ComputedNode` measures in PHYSICAL pixels while `ScrollPosition`
  and every `Node` length are LOGICAL, and all four scrolling
  screens - song browser, settings, controls, about - had grown
  their own copy of the follow loop and every one of them mixed the
  two. On any display with a scale factor (a Retina panel is 2, and
  the window-height sync stacks on top) the list believed half as
  many rows fitted as really did: the cursor walked past the bottom
  edge before anything scrolled, and the offset it finally wrote
  moved twice as far as asked. There is now ONE implementation
  (`ui_kit::follow_list`), and its pure core is tested at both
  scales.
- Lists show WHOLE rows again: Bevy clips a scrolling node at its
  padding box by default, so the neighbouring rows bled through
  above and below as 12 px slivers of text.
- The viewport is derived from the window height the same call is
  about to set, instead of the panel's one-frame-stale measured
  height - the two disagreed by 6 px, found by a test that walks
  the whole list rather than by eye.

### Added

- The guitar reaches left and right in menus. Its neck reports no
  horizontal direction (the strum bar IS the D-pad's up/down), so a
  guitarist could walk the song list but never change the
  difficulty beside it; the two middle frets now stand in, the way
  Enter and Escape already stand in for a mangled bindings file.
- Calibration works with a guitar: the frets and the strum bar tap
  the beat, START saves and BACK cancels. It was keyboard-only -
  a guitarist could not tap, could not save, and could not even
  leave the screen.

## [0.13.11] - 2026-09-01

### Fixed

- Lyric glyphs track at the FACE's own advance in the smooth note
  style: the engine's bundled monospace moves 0.6 em per glyph
  (Press Start 2P moves a full em), and the first build spaced every
  smooth-style line half again too wide - measured from a live
  frame, after correcting for the UI-scale zoom that disguised the
  number as 0.7.
- The lyric display clears when the "YOU ROCK!!!" outro takes the
  stage, instead of freezing mid-fill behind it.
- The lyric scrim darkens a touch more (HDR tonemapping compresses
  its alpha; measured ~18% on the LED wall).

## [0.13.10] - 2026-09-01

### Added

- LIVE KARAOKE LYRICS. A `.lrc` beside a song's audio (or chart)
  now sings along during gameplay: the active line renders above
  the highway with a true karaoke fill - each glyph lights as the
  word crosses it - the next line waits dimmed below, and a soft
  scrim keeps everything readable over the LED wall. Both standard
  LRC (line timing: honest fade in/hold/out, never fake word sync)
  and enhanced LRC (`<mm:ss.xx>` word timing, plus the `[offset:]`
  tag) are supported, parsed under the same untrusted-input caps as
  charts. Lyrics run on the SAME clock notes are drawn with - one
  timebase, judgment untouched - and MC-set crossfades swap them
  with the song.
- The demo song "Circuit Breaker" ships original, hand-timed
  karaoke lyrics, so a fresh clone demonstrates the feature.
- Importing (and the watch folder) carries a `.lrc` sitting beside
  the source audio into the song's folder.
- Settings: LYRICS on/off, LYRICS SIZE (small/medium/large) and
  LYRICS OFFSET (±500 ms, display only).
- `docs/visual-master-plan.md`: the modern-rendering commission
  mapped against what already ships (3D venue stage, HDR/bloom,
  budgeted particles, tiered hit feedback, beat-reactive
  environment), with the honest deviations recorded - including
  why bevy_hanabi 0.19 was checked and NOT added.

## [0.13.9] - 2026-09-01

### Changed

- The About screen's detail block shows a changelog entry as REAL
  BULLET POINTS at row size - version heading, brand-colored dash
  markers, an honest "+ N more" note - instead of one small
  flattened prose line. Wrapping is exact: Press Start 2P advances
  a full em per glyph (measured from the bundled TTF), so the
  wrapper IS the layout. The block always shows something - the
  highlighted entry, or THIS BUILD's - so About answers "what's
  new" the moment it opens, and its height is fixed so the footer
  never jumps.
- MADE BY opens the maker's website on confirm, like WEBSITE.
- The About column widened to 760 px; the e-mail-bearing values no
  longer wrap mid-address.

## [0.13.8] - 2026-09-01

### Changed

- The streak bulbs are drawn CRISP, the way Guitar Hero II draws
  its own ("10 little dots above the multiplier, each dot one note
  of the combo" - WikiHero): a socket ring whose rim lights with
  the fill and a solid core - the 26 px additive halos on a 13 px
  pitch are gone (they overlapped into one smear; user report "zu
  viel glow, unsauber"). The streak counter got its own clear line
  in bright brand digits, GH2-Deluxe style, below the bulb row -
  it used to sit centred under the plate where its pop animation
  scaled it INTO the bulbs ("nicht gut sichtbar").

## [0.13.7] - 2026-09-01

### Changed

- The mouse wheel SCROLLS through menu rows everywhere - main menu,
  settings, controls, about and the pause menu - exactly like the
  song list, instead of stepping the hovered value (user report: a
  wheel turn while browsing the settings changed them by accident).
  Values adjust with LEFT/RIGHT, Enter or a click, as the footers
  say.

## [0.13.6] - 2026-09-01

### Added

- The MC set: queue songs in the browser (Q adds/removes, P plays
  the set) and they play as ONE continuous performance with a real
  DJ crossfade between them - the outgoing song keeps sounding on
  the audio thread's second player while the next fades in over
  four seconds on an equal-power curve (a linear fade dips audibly
  in the middle; a test pins the power sum). The handover reuses the
  count-in: the next chart's notes are already approaching, fully
  fair, while the previous song still plays underneath - no hard
  stop, no gap, and judgment never spans two clocks. Works solo and
  in local multiplayer (both verified end to end); each song plays
  the selected difficulty or falls back to what it offers; the set
  keeps the first song's stage. `BEATBYTE_AUTOPILOT_MC` drives the
  only automated path through a crossfade. Online multiplayer does
  not exist in BeatByte - the set lives on the session/clock layer,
  which any future netcode would inherit.

## [0.13.5] - 2026-09-01

### Added

- The highway edges catch BLUE fire while Hype runs (the genre's
  classic Star-Power tell): a row of additive flame licks seated
  along both rails, flickering on two incommensurable sines so the
  fire never loops visibly and never blinks out, grown in and out
  with the same eased feel as the hype tint. Purely visual and
  purely the transform channel - one shared material, created once
  and never written again; hidden licks are not even animated. The
  resting edge look is the unchanged rails. Judgment, scoring and
  Star-Power conditions untouched (autopilot scores identically).

## [0.13.4] - 2026-09-01

### Added

- A song-completion celebration in the genre's classic beat: the
  moment the last note has been judged and the timeline has run out,
  "YOU ROCK!!!" slams onto the screen in the house pixel face -
  oversized, squashing below rest on impact, then breathing - over
  the LIVE stage (the venue keeps playing underneath; lane-colored
  firework bursts march the highway, honoring the particle and
  intensity settings), with the Hype riser as the fanfare. Exactly
  five seconds later the detailed results screen takes over
  automatically (grade, score, accuracy, per-judgment counts,
  timing drift, overstrums, best streak - the screen that already
  existed becomes the sequence's second act). Pausing is disarmed
  during the celebration; a quit from the pause menu still skips
  straight to the browser as before. The autopilot rides through
  the new phase (runs are five seconds longer) and photographs it
  as the `gameplay-yourock` moment.

## [0.13.3] - 2026-09-01

### Added

- A watched SONG FOLDER: drop a FOLDER onto the window and BeatByte
  keeps an eye on it (light poll every five seconds, menu and
  browser only) - new audio files are imported automatically through
  the existing pipeline once they sit still for two polls (a file
  still being copied would chart half a song). Duplicates are
  skipped by CONTENT: a 64-bit FNV-1a fingerprint over the file's
  bytes plus its size, persisted in imported-hashes.json - a renamed
  copy is recognized, a different song sharing a file name is not
  wrongly skipped, and a song deleted in-game stays deleted even
  though its file still sits in the folder. The same fingerprint now
  also guards the drag-and-drop path (the old rule matched only the
  sanitized file name). A SONG FOLDER settings row shows the watched
  folder and clears it; failed imports are not retried every poll.

## [0.13.2] - 2026-09-01

### Added

- An ABOUT entry in the main menu: who made this (Martin Pfeffer -
  celox.io - 2026), the MIT license, and rows that open the GitHub
  repository, the website, the Google-Maps review page, a PayPal
  donation and a contact mail in the system's own browser/mail
  client. Below them a collapsible CHANGELOG section (closed by
  default) lists every release of the game, newest first, with the
  highlighted version's changes as a detail line - fed by parsing
  the repository's own CHANGELOG.md at build time, so the next
  release appears there without anyone touching the screen (a test
  pins that the newest entry IS this build's version). Screens with
  more rows than fit scroll with the usual whole-row window;
  `BEATBYTE_SHOT_STATE=about` photographs the screen and
  `BEATBYTE_ABOUT_EXPANDED=1` pre-expands the changelog for it.

### Fixed

- The harness reference listed `BEATBYTE_SHOT_SEARCH` twice; the
  switch-count badge had been counting the duplicate.

## [0.13.1] - 2026-09-01

### Added

- Two self-maintaining badges sit above all others in the README, in
  the large style: the CURRENT VERSION, read live from Cargo.toml on
  main by a shields dynamic-toml badge (no workflow, nothing to
  forget), and LINES OF CODE, recounted by a new `loc-badge` workflow
  on every push to main (tokei, charts and media excluded) and
  served from a committed shields endpoint file. Neither number is
  ever set by hand again.

## [0.13.0] - 2026-09-01

Milestone release: **the game learned to be played your way.** Since
v0.12.0: the chart generator graduated the design pattern that won the
by-ear A/B ("escalate where the song escalates") and grew real Hard
and Expert derivations with jack-free lane flow and burst discipline;
a practice mode slows any song to 50-100 % and loops any section from
the pause menu without ever touching a record; the results screen
grew an in-game feedback channel (rate the fun, judge a redesign
against its parent). The stage became a concert - researched against
the genre's club-first classics: a moving-head light rig with real
beam cones, an LED pixel wall, PA stacks with breathing drivers, a
silhouette crowd, haze, club darkness - and the HUD became an
instrument panel with brushed-metal plates, streak bulbs, a Hype
gauge with a counterweight needle, and star-marked energy phrases on
the highway. The input layer became fully logical: menu navigation is
remappable (WASD, Space, Tab included) with hard-wired Enter/Escape
fallbacks, rebind conflicts ask before stealing, UI feedback is sound
events that treat every device alike, prompts follow the device in
your hand, and an accessibility set (reduced flashing, effect
intensity, UI scale, high contrast) joins a draw-only video offset
that provably never touches judgment. And a held sustain's beam now
burns beside the receptor for exactly as long as you hold it.

Everything since v0.12.0 is described under its own version below.

## [0.12.35] - 2026-09-01

### Fixed

- The sustain beam no longer vanishes mid-hold (user report: the
  drawn-through line must not simply disappear - only on release).
  Two systems both wrote a held tail's transform: the consumer pins
  it to the hit line beside the receptor flame and eats it from the
  front, while the general note mover head-anchored it - and the
  head marches past the camera during a hold, so the past-the-camera
  cleanup despawned the very beam the player was still playing (at
  420 px/s a 2 s sustain lost its beam less than halfway through). A
  held tail now belongs exclusively to the consumer: it stays pinned
  and throbbing at the button for exactly as long as the keys are
  down, greys out and slides away on release, and disappears only
  when fully played out.

## [0.12.34] - 2026-09-01

### Added

- ADR-0012 records the renderer boundary (phase 6, closing the
  UX/input commission): the 8-bit look is DATA through the one
  renderer - per-style textures, a per-style particle sprite and the
  per-style camera contract in `sync_bloom` - never a forked code
  path, and gameplay/input/UI carry no note-style conditionals. The
  dormant flat-2D highway path stays a filed pruning task, distinct
  from the 8-bit style, which is alive and default.

## [0.12.33] - 2026-09-01

### Added

- A VIDEO OFFSET beside the latency offset (phase 5): +-100 ms in
  5 ms steps, shifting where notes, fret bars, phrase bands and
  sustains are DRAWN - never when they judge. The renderers read a
  new `visual_time` (the judgment clock plus the offset); judgment,
  autopilot and the score keep reading the unshifted clock, which an
  autopilot run at +100 ms proves: the score stays perfect while the
  notes draw late. The calibration screen now says what it measures
  (the input offset) and where the video nudge lives.

## [0.12.32] - 2026-09-01

### Added

- Accessibility rounds out (phase 4): four new settings that thread
  through one consumer each. REDUCED FLASHING removes the
  full-screen combo-break flash entirely (not a dimmer one). EFFECT
  INTENSITY scales particle counts, screen-shake strength and flash
  opacity together on one 0-100% slider. UI SCALE stacks a personal
  75-150% multiplier on the window sync, clamped so no settings file
  can render the menus unusable. HIGH CONTRAST lifts idle menu text
  to full brightness and clearly strengthens selection fills on
  every screen. The settings list - seventeen rows now - scrolls
  with the same whole-row window as the browser and the controls
  screen.

## [0.12.31] - 2026-09-01

### Added

- Prompts speak the player's device (phase 3): the game tracks which
  device produced the last real input, and every footer and hint
  line swaps its wording to match - a pad player reads "D-PAD choose
  SOUTH confirm" and never "press ENTER", a keyboard player the
  reverse, and a shared frame goes to the pad (fretting with a palm
  on the keyboard is guitar play). The mouse counts as
  keyboard-family. Where a device honestly cannot do something the
  prompt says only what it can: the pad line on the results screen
  offers just "SOUTH back to browser" (ratings are keyboard digits),
  and the menu's pad line offers no quit (Escape is deliberately not
  on a pad button).

### Fixed

- A gamepad player could not leave the results screen at all - it
  listened only to Enter, Escape and the mouse. Confirm or back on
  any device now returns to the browser.

## [0.12.30] - 2026-09-01

### Added

- UI feedback became a sound EVENT system (phase 2 of the UX/input
  commission): screens emit what happened - Navigate, Confirm, Back,
  Error, Toggle, Slider - and one player system turns it into audio,
  so gamepad and mouse interaction sounds exactly like the keyboard
  (the old system listened to four raw arrow keys and Enter, and
  covered four screens). Two new procedural voices join the library:
  backing out plays the confirm pair falling instead of rising, and
  a refusal - a rebind conflict - gets a low, deliberately unmusical
  buzz. Toggles click, stepped values tick, the browser's difficulty
  stepper ticks, sorting clicks, and the results, calibration,
  input-test and controls screens speak on entry, exit and capture.
  No widget owns an audio asset; everything stays synthesized at
  startup.

## [0.12.29] - 2026-09-01

### Added

- Menu navigation became a logical, remappable input layer (first
  phase of the UX/input commission; plan in
  docs/ui/input-ux-plan.md). A `UiAction` bindings table (menu
  up/down/left/right, confirm, back) joins the game actions in the
  input map: WASD navigates and Space confirms out of the box,
  Tab/Shift+Tab cycle rows, and every menu screen reads the table
  instead of hard-coded keys. Enter and Escape stay hard-wired
  fallbacks so no rebinding can strand you in a menu, and while the
  browser search is typing, letter/space bindings type instead of
  navigating. The controls screen lists the menu actions as
  MENU-prefixed rows in a list that now scrolls with a whole-row
  window (fifteen rows had outgrown the screen), and rebinding no
  longer steals a conflicting binding silently: the row names the
  current owner and asks for the same press again to confirm the
  move. Settings files from before the table existed load unchanged.

## [0.12.28] - 2026-09-01

### Changed

- The HUD plates became instruments. The corner panels are brushed
  dark metal now - top-edge light catch, corner rivets, a vignetted
  field - tinted toward the accent, instead of flat colour
  rectangles; the score digits sit in a truly recessed well that shades
  under its lip. The gauge dial gained a sweep gradient that
  brightens past the activation mark, and its needle got a
  counterweight tail past the hub - a dial needle, not a rotating
  line. New motion, all transforms and sprite tints: the multiplier
  POPS when it changes (up or lost), the next streak bulb carries a
  faint ember so the row points at where the streak is going, and
  the gauge breathes toward the Hype tone once it can fire, blazing
  white-hot while Hype runs.

## [0.12.27] - 2026-09-01

### Changed

- The stage learned what a concert looks like (researched against
  Guitar Hero II's club-first venues; plan in
  docs/ui/stage-realism-plan.md). Darkness first: key, fill and
  ambient light drop to club levels and the walls vanish instead of
  reading as lit cardboard. The crowd is a SILHOUETTE mass now -
  torso + head per person, hash-jittered off the grid, one in four
  with an arm up, the whole person bobbing. A handful of static
  additive haze sheets give the beams a body. A second lattice
  truss above the LED wall carries a backline of four fixtures
  firing short cones toward the camera in the accent's
  complementary tone - the warm/cold opposition concert light lives
  on. The main truss is a real lattice (chords + diagonal bracing),
  and the highway stands on a stage riser with a visible front
  edge instead of floating in the void.

## [0.12.26] - 2026-09-01

### Changed

- The venue got the realism pass across all three of its set
  pieces. The stage has a FLOOR at last (everything used to float
  over a void), with a faint sheen - and each light fixture throws
  a soft additive pool onto it that slides in step with its
  swinging shaft (one shared angle function, pinned). The lenses
  bloom with a soft halo. The speaker stacks are near-black PA
  cabinets with real driver fronts - one big cone on the sub,
  woofer and tweeter on the tops, on a faint grille weave - and the
  fronts breathe with the beat. The LED wall's panels sit on a dark
  cabinet board and carry a dot-matrix module texture in base and
  emissive, so the screen reads as pixels rather than as lamps.

## [0.12.25] - 2026-09-01

### Changed

- The stage lights read as lights. Each beam is now a moving-head
  FIXTURE hanging from the truss - a housing with a bright lens -
  and under it a pair of nested cone mantles wearing a procedural
  beam gradient (dense at the lamp, dissolving into the air, faint
  striations around the shaft), additively blended, swinging from
  the hanger instead of around their own middle. Every second
  fixture runs a paler tone, so the rig reads as lamps rather than
  as a repeated texture. The old uniform alpha cones read as
  coloured glass triangles.

## [0.12.24] - 2026-08-31

### Changed

- The stage gained an LED wall: a 9x3 grid of dim emissive panels in
  two alternating tones behind the stage, swelling with the beat in
  a ripple from the centre - pure transforms, no per-frame material
  writes. The back wall reads as a concert rig instead of a bare
  surface.

## [0.12.23] - 2026-08-31

### Changed

- The HUD reads like an instrument panel (user commission): the
  streak row is ten glowing BULBS in round sockets - lit toward the
  next multiplier, hype-coloured while the power runs - the streak
  counter counts up visibly and POPS as it rises, and the Hype
  meter is a half-circle GAUGE with a needle: the strongest tick
  straight up is the activation threshold, so "can I fire it?" is
  which side of vertical the needle stands on.

## [0.12.22] - 2026-08-31

### Fixed

- The autopilot's pause drill navigates to the SFX row by content
  instead of a remembered row index - inserting the loop rows above
  it had silently retargeted the drill at a loop bound, which the
  next run caught loudly.

## [0.12.21] - 2026-08-31

### Removed

- The 2D "depth" view (user call, with screenshots of both note
  styles): the 3D stage is the game's one view. The VIEW settings
  row is gone, a stale settings file is forced back to the stage,
  and the note-style choice (8-bit shapes / round) now only shapes
  the gems on the 3D highway.

## [0.12.20] - 2026-08-31

### Changed

- The 3D stage lost the glowing accent-coloured band across the back
  wall (red on the default stage) - it read as a stray horizontal
  line behind the highway and was reported as exactly that. The
  floor line now comes from the barriers and speaker stacks.
- Star-power notes look the part: a note inside an energy phrase
  wears a five-point STAR under its gem instead of a lit circle -
  the genre's star-note convention - and while Hype runs, the note
  gems themselves shift toward the energy colour (solo; in
  multiplayer the shared lane materials would recolour the other
  player's notes, so the neck wash alone carries the state there).

### Fixed

- The stage's hype-tint materials were re-uploaded every frame of
  every song, Hype or no Hype - a settled blend now writes nothing.

## [0.12.19] - 2026-08-31

### Changed

- Leaving a finished song returns to the song browser - cursor,
  sort and search intact - instead of the main menu: browse, play,
  land on the next choice. Quitting from the pause screen goes to
  the browser too, and the results footer says "back to browser".

## [0.12.18] - 2026-08-31

### Added

- Section loop (optimization plan P1, second half): LOOP FROM and
  LOOP TO rows on the pause menu - RIGHT pins a bound to the paused
  moment, LEFT clears it. With a real span armed (at least one
  second), reaching the end jumps the whole run back to a 1.5 s
  lead-in before the start: music, clock, sessions and the notes in
  flight together, and the section's notes become judgeable again.
  Looping is practice: no scoreboard entry, no telemetry. Bounds
  clear when a new song starts - they are positions in ONE song.

## [0.12.17] - 2026-08-31

### Added

- Practice speed (optimization plan P1, first half): the pause menu
  gains a SPEED row, 50-150 % in 5 % steps, applied live to the
  audio and the song clock together (pitch moves with it - the
  honest simple version). The whole timeline scales, count-in
  included, so judgment stays relatively untouched; the run is
  marked practice and stays out of the scoreboard AND the telemetry
  (slowed evidence would poison the design loop), and the results
  screen says so. The chosen speed survives into the next song;
  menus always run at life speed.

## [0.12.16] - 2026-08-31

### Added

- Early/late feedback (optimization plan P2): the judgment popup
  tags non-perfect hits with the side they landed on - GREAT
  (EARLY), GOOD (LATE) - and the solo results screen shows the
  run's mean timing drift as a TIMING row ("32 ms early", "+18 ms
  late", "on time" inside 3 ms), with a recalibration hint once the
  drift reaches 15 ms, half the perfect window. The most actionable
  number the game knows, shown where it can be acted on.

## [0.12.15] - 2026-08-31

### Added

- In-game feedback on the results screen (adaptive charting A5):
  keys 1-5 record a fun rating, and when the played chart is a
  designed version, LEFT/RIGHT records whether it felt worse or
  better than the version it was derived from. Both land in the
  session telemetry log just written; `beatbyte-cli review` reports
  the mean fun and the better/worse tally next to its accuracy
  sections. Zero friction when skipped: without a session log no
  hint is shown, and ENTER leaves the screen untouched either way.

## [0.12.14] - 2026-08-30

### Fixed

- The deeper half of the vanishing stage: the stage camera's whole
  render pass was silently dropped because the two cameras on the
  window disagreed on HDR (bloom made the stage camera HDR while
  the 8-bit style left the 2D camera SDR). Bloom and HDR now follow
  the note style on BOTH cameras - the 8-bit look is bloom-free by
  identity, on the stage too - and the pause drill fails loudly on
  any HDR mismatch. Verified at the real window: the venue,
  highway and notes render again in 3D with 8-bit shapes, and
  unchanged with round gems.

## [0.12.13] - 2026-08-30

### Fixed

- The 3D stage no longer vanishes with the 8-bit note style. Two
  cameras drew to one window and the 2D camera's default clear wiped
  the stage rendered beneath it - score and particles over a black
  void; the round style escaped only because its bloom pipeline
  happened to dodge the wipe. The 2D camera now loads the frame
  while a stage camera is on screen and clears it otherwise, and the
  autopilot's pause drill fails loudly if the one-camera-clears rule
  is ever violated again.

## [0.12.12] - 2026-08-30

### Fixed

- The pause menu (and the PAUSED banner) actually renders in the 3D
  stage view. With the stage camera active alongside the 2D camera
  and no marked default UI camera, every gameplay UI root laid out
  to zero size - the menu existed, reacted to input, and drew
  nothing. The 2D camera is now the explicit UI camera, and the
  pause drill fails loudly if the overlay ever lays out to zero
  size again.

## [0.12.11] - 2026-08-30

### Added

- The pause menu adjusts settings mid-song: MUSIC VOLUME, SFX
  VOLUME and SCROLL SPEED as selectable rows (UP/DOWN choose,
  LEFT/RIGHT adjust, mouse and wheel work too), reusing the settings
  screen's own step sizes and clamps. Stepping the SFX row previews
  the MISS sound at the new volume - it is the volume of the error
  sounds, and with the music paused there is nothing else to hear.
  Changes persist on leaving the pause, whether by resuming or
  quitting. Judgment-changing settings (latency offset, tap mode)
  deliberately stay on the settings screen.

### Changed

- Enter on the pause screen steps the selected row (like on the
  settings screen) instead of resuming; ESC remains the resume.

## [0.12.10] - 2026-08-30

### Added

- `BEATBYTE_AUTOPILOT_DIFFICULTY=easy|medium|hard|expert` plays the
  autopilot on a chosen difficulty (default stays medium). Unknown
  names and difficulties the selected song does not offer fail
  loudly - a harness that silently plays the wrong difficulty
  validates nothing.

## [0.12.9] - 2026-08-30

### Added

- `beatbyte-cli redesign <chart>` (and `--all` over a directory of
  song folders): regenerates hard + expert from a fresh deterministic
  analysis and writes the result as the folder's next sibling
  version - easy and medium are carried note-for-note from the
  active version, provenance records the parent, the pointer moves,
  and per-song revert stays one pointer away. Legacy folder layouts
  are skipped with a message; a tempo drift between the active chart
  and the fresh analysis refuses to merge; an unchanged result
  writes nothing.

## [0.12.8] - 2026-08-30

### Changed

- Hard grows HOPO runs at its own speed: its HOPO gap rises to
  0.26 s, matching hard's real gap distribution (0.23-0.37 s on the
  imported library, where 15 of 25 songs previously got zero HOPOs).
- Expert escalates toward the transcription: its level above is the
  master itself, so it rises toward the master's density in the
  song's own hot bars and keeps its anchor everywhere else. It was
  the one difficulty that ignored the song's shape.

## [0.12.7] - 2026-08-30

### Changed

- Chords mark the song's own accents: eligibility is a percentile of
  the difficulty's kept notes (expert 12 %, hard 8 %), never an
  absolute strength bar - a quiet master and a loud one carry the
  same accent rate, where the old threshold gave some songs almost
  no chords and others a flood. Chords need room on both sides
  (setup and landing), three-note chords are reserved for the very
  strongest accents, and a song without accents gets no chords.

## [0.12.6] - 2026-08-30

### Changed

- Streams have a per-difficulty length budget: runs of sixteenths
  longer than the difficulty tolerates (expert 24 events, hard 10)
  relax their interior to eighths, first and last hits kept. The
  imported library carried unbroken machine-gun runs of up to 55
  events under 0.13 s; a stream inside the budget passes untouched —
  the cap is a ceiling, not a mower.

## [0.12.5] - 2026-08-30

### Fixed

- Fast repeated pitches no longer machine-gun a single lane: the
  generator rewrites them into trills at the master level, so every
  difficulty inherits one consistent, physically playable reading.
  Measured before the fix, the imported library's expert charts
  carried a median of 106 same-lane jacks per song at gaps a human
  cannot drum on one finger; repeats at quarter-note speed keep
  their lane — they are a musical statement, not a jack.

## [0.12.4] - 2026-08-30

### Fixed

- The song browser no longer rebuilds the whole screen on every
  keystroke, sort click or difficulty step. The screen spawns once;
  the status line, header captions and details update in place, and
  the rows respawn only when their content (order or difficulty)
  actually changes. Scroll position survives typing.
- The resting mouse no longer steals the selection: hover only
  selects when the pointer actually moved, so typing a search or
  stepping the difficulty cannot yank the cursor to wherever the
  mouse happens to lie.
- Delete-arming binds to the song, not to a view position. Sorting
  or filtering between the two Backspace presses can no longer point
  the armed deletion at a different track.

### Changed

- Typing in the search selects the first match (type, Enter, play);
  sort changes still keep the selection on its song.
- An empty search result says so: `no match — ESC clears` instead of
  a bare panel.
- Sort mode and direction persist in `settings.json` (the filter
  deliberately does not — an invisible stale filter across sessions
  is a trap). Sort, search and header clicks give the same audio
  blip as every other menu key.
- Backspace in the search field repeats when held.

## [0.12.3] - 2026-08-30

### Fixed

- **Search was unreachable from a German keyboard.** It was bound to
  `KeyCode::Slash` — a *physical* key position from the US layout; on
  QWERTZ that key is `-`, and `/` lives on Shift+7, which produces
  `Digit7`. Search now opens on **`F`** (letter keys sit in the same
  place on every layout) or on a *typed* `/` (the logical character,
  layout-aware).

### Changed

- **The sort became visible where the data is.** The active column
  header wears the accent colour and a direction marker (`v`/`^`);
  the status line spells it out too. Column headers are **clickable**:
  a click sorts by that column in its default direction, a second
  click reverses it — the convention of every library UI. NOTES and
  DIFF became sortable alongside the rest, and the `S` cycle covers
  all eight modes. Reversal never applies to STANDARD (the library's
  own order has no "reverse" a player would ask for by name), and a
  changed direction resets when a new column is chosen.
- The search line turns accent-bright while typing, with an explicit
  `ESC to close`.
- `BEATBYTE_SHOT_SORT` photographs the browser under a chosen sort —
  the active-column marker only exists when a sort is active, so
  without it the marker could only be argued about.

## [0.12.2] - 2026-08-30

### Added

- **The song browser became a library.** Seven columns per row —
  title, artist, genre, length, note count, a 1-5 challenge rating
  (from note density, calibrated on the real library) and your
  personal best — all following the selected difficulty. `S` cycles
  the sorting (standard / title / artist / genre / length / best);
  `/` opens a search filter that matches title, artist and genre,
  case- and accent-insensitively ("sacre" finds "Sacré"). While the
  filter is open, letter shortcuts are suppressed — typing "elle"
  must not open the editor and arm a delete on the way. The cursor
  follows its *song* through sort changes rather than staying on a
  raw position, missing genres sort last (an absence is not the
  alphabet's beginning), and a filter with no matches is an empty
  list, not a crash.
- **Genres.** The chart format carries an optional, validated `genre`
  field — deliberately excluded from the chart hash, like provenance,
  so tagging a song can never orphan its recorded sessions (proven on
  live data: sessions recorded before tagging still match after).
  Imports read the audio file's own genre tag; `beatbyte-cli
  set-genre` stamps it into every version of a song, which is how the
  existing library was filled once by hand. The synthesized demo
  songs honestly declare "Chiptune".

### Fixed

- The harness reference described `BEATBYTE_AUTOPILOT_DELETE` as a
  flag; its value is actually the title substring to delete. Running
  it as a flag matches nothing and times out — which is exactly how
  the error in the reference was found.

## [0.12.1] - 2026-08-30

### Changed

- **The winning design pattern graduated into the generator.**
  "Escalate where the song escalates" — validated by ear on one song,
  then across the library — is now how every chart is generated: a
  difficulty's density rises toward the next difficulty's reading in
  the song's own high-energy passages (found from its own percentiles,
  p70 stepping to p80/p90 when that floods; one-bar dips smoothed;
  runs under four bars dropped), and stays at its anchor everywhere
  else. Every FUTURE import gets the better reading at import time
  instead of needing a design session. The mechanics that made the
  pattern safe are preserved by construction: escalation selects
  *more of the parent difficulty's notes*, so "medium is a subset of
  hard" survives; expert has nothing above it and never escalates; a
  song with no high ground of its own — including the flat synthetic
  builtins, whose autopilot baseline is unchanged at 98/98 — generates
  exactly as before. Existing chart files on disk are untouched;
  charts are versioned, so even a regretted regeneration is one
  pointer away from undone.

One of the new pins was born blind and is worth recording: the
"expert never escalates" test used a fixture whose master had fewer
notes than expert's budget, so every note survived regardless and
forcing expert to escalate changed nothing the test could see. The
fixture now provably thins (with a guard assertion), and the mutation
fails.

## [0.12.0] - 2026-08-30

Milestone release: **the adaptive charting loop is closed.** The game
records every session (per-note judgments and millisecond offsets,
bound to the content hash of the exact chart played), the CLI turns
recordings into per-section evidence and directives, charts version
with provenance and nothing overwrites anything, and the design
dossier hands a redesign everything it needs — with the by-ear A/B as
the standing gate. The first design session ran end to end and its
pattern ("escalate where the song escalates") won the ear's verdict,
first on one song, then across the library.

Everything else since v0.11.0 is described under its own version
below: distinguishable miss/overstrum sounds, the beat-ruled neck and
decorated borders, the song ribbon, the scrolling song list, Escape
closing the game, held sustains that glow, and the fix for the race
that made a new song inherit the previous song's position — the cause
of "sometimes no notes appear".

## [0.11.13] - 2026-08-30

### Added

- **`beatbyte-cli dossier`** (adaptive charting phase A4, ADR-0011) —
  the design session's briefing, one self-contained file per song:
  the **active** chart (the folder's pointer is resolved, so a
  redesign can never start from a superseded version and attach the
  wrong parent), a per-bar structure table (onsets, energy, melody
  density), the extracted melody with true held lengths, the
  playability constraints per difficulty straight from the
  generator's own profiles, the open directives from the review (same
  code path, so the two cannot disagree), and the mechanical write
  instructions: the next version's file name and the parent hash the
  provenance must carry.
- **The design-session workflow** is documented in
  `docs/workflow/design-session.md` — play → review → dossier →
  design → validate → pointer → **the ear decides** — and a new drift
  test binds it to the code: every `beatbyte-cli` subcommand the
  document invokes must exist in the CLI's command enum, so a renamed
  command cannot leave the workflow teaching invocations that fail.

With this, the loop is closed: every layer of ADR-0011's architecture
short of the deliberately-parked ones exists and is tested end to end
on real data.

## [0.11.12] - 2026-08-30

### Added

- **Chart versions** (adaptive charting phase A3, ADR-0011). A
  redesigned chart is a sibling file (`chart.v2.json`, `.v3`, …) with
  a provenance block — parent hash, designer, date, the directive it
  answers — and a pointer file (`chart-active.json`) names the one
  the game loads. The library shows one entry per song whichever
  version is active; a version without a pointer is ignored rather
  than becoming a second song; and every broken-pointer case falls
  back to the original, because the recoverable failure is "you see
  the original" and the unrecoverable one is "your song is gone".
  The pointer is untrusted input like every chart file: a target
  with a path in it is never followed.
- **Import never overwrites an existing chart** — stronger than the
  plan's "never one that has telemetry", because it is simpler and
  strictly safer: a re-analysis writes the next version and moves the
  pointer, and whatever was on disk (recorded sessions' chart, hand
  edits, a designed version) stays.
- Provenance is validated like every other field, and it is
  deliberately **excluded from the chart hash**: it is the paper
  trail, not the music — otherwise touching metadata would orphan
  every recorded session of an unchanged chart. A golden-hash test
  pins the identity format itself, so a schema change that would
  orphan all telemetry files cannot happen as a side effect.

## [0.11.11] - 2026-08-30

### Added

- **`beatbyte-cli review`** (adaptive charting phase A2, ADR-0011):
  joins the recorded sessions with the chart they were played on and
  answers *where* a chart struggles or bores — accuracy, timing mean
  and spread, dropped sustains and localized overstrums **per
  four-bar section**. When enough evidence of the current chart
  version accumulates (default 3 sessions, `--min-sessions`), it
  emits generation directives: `low_accuracy`, `dropped_sustains`,
  `sloppy_timing` per section, or `trivially_mastered` for the whole
  chart — the last only when nothing else is wrong, because a chart
  with failing holds is not mastered whatever the average says.
  Sessions from other chart versions are counted as stale and feed
  nothing (the hash-binding payoff); autopilot sessions are excluded
  unless `--include-autopilot`. `--directives <path>` writes the
  machine-readable half for a later design session.
- Overstrums now record the most recently judged event index
  (`near`), so analytics can localize them to a passage. Optional and
  additive: files written before the field parse unchanged.
- The telemetry schema moved to `beatbyte-core::telemetry` and the
  chart hash to `beatbyte-chart` — one implementation shared by the
  game that writes and the CLI that reads (the mechanics reference's
  shared-library rule), instead of a copy on each side.

## [0.11.10] - 2026-08-30

### Added

- **Every session is recorded** (adaptive charting phase A1,
  ADR-0011). The engine has always produced a judgment and a signed
  millisecond offset for every note and thrown them away when the song
  ended; they now land in an append-only session log beside
  `scores.json` (`telemetry/<started_ms>-p<player>.jsonl`): a header
  binding the session to the **content hash of the exact chart
  played**, then one line per observation — hits with their offsets,
  misses, sustain endings (played out vs. dropped — the evidence that
  separates "too hard" from "too easy"), overstrums. Autopilot
  sessions are marked so evidence readers can exclude them. Completion
  is derived (judged events vs. total), never stored, so it cannot
  disagree with the lines. Recording is buffered in memory and written
  once on the way out of gameplay; a write failure logs and drops,
  never touches play.
- The schema learned from the gameplay-mechanics reference the user
  supplied: sustain endings are their own line kind, and title/artist
  are separate fields rather than a joined key — the score board's
  `title|artist` collision (roadmap C5) does not get copied into a new
  format.

## [0.11.9] - 2026-08-30

### Documentation

- **The adaptive-charting decision** ([ADR-0011](docs/decisions/ADR-0011-adaptive-charting.md),
  spec in [docs/adaptive-charting.md](docs/adaptive-charting.md)).
  Four planning documents envisioned AI-designed charts and a closed
  telemetry loop; the decision reconciles them with this repository's
  reality. The load-bearing findings: the per-note millisecond signal
  the plans demand **already exists** (`SessionEvent::NoteHit` carries
  judgment and signed offset, and is currently discarded after every
  session); the population-scale layers (percentiles, A/B cohorts, ML
  models) have no players to feed them and are parked with reopen
  criteria; the runtime never calls a model — Claude designs at design
  time from a CLI-exported dossier; every regenerated chart is a
  hash-bound sibling **version**, and adoption stays gated by the
  by-ear A/B that ADR-0009 established. Phases A1–A5 are on the
  roadmap, telemetry first, because every later ambition feeds off
  recorded truth.

## [0.11.8] - 2026-08-30

### Fixed

- **Notes sometimes never appeared, and the song ended the moment it
  started.** A song change announced itself by bumping a generation
  counter and then clearing the playback position — two relaxed atomic
  stores, in that order. Between them, the game thread could see the
  *new* song still carrying the *previous* song's position and anchor
  its clock there. After a four-minute track that meant starting the
  next one at 248 s: every note already in the past, so the highway
  stayed empty, and the session judged the song finished at once and
  returned to the menu. Only someone playing several songs in a row hit
  it, which is why a harness that plays one song per process almost
  never did. The position is now cleared first and the generation
  published with `Release`, read with `Acquire`, from a single place.

### Changed

- **A held sustain glows while it is being played.** It used to show
  only by getting shorter, which is the one thing a player cannot
  watch — their eyes are at the hit line. The tail now throbs at 7 Hz
  off the song clock, so holding a note looks like playing one. It
  never goes dark, because a tail that blinks out reads as a *dropped*
  hold, which already has its own picture. Each tail gets its own
  material: the lane's is shared by every note in it, and pulsing that
  would light the whole lane.

## [0.11.7] - 2026-08-30

### Added

- **Escape closes the game from the main menu.** There is no screen
  above that one to go back to, so Escape means leave. Bound to the
  key rather than to the menu's general "back", because that also
  fires on the pad's East button, which the default map gives to fret
  1 — with a guitar plugged in, a finger resting on the red fret at
  the menu would have closed the application. A test pins that
  pairing so the shortcut cannot be quietly simplified later.
- The smoke test now leaves by **pressing Escape** instead of writing
  the exit itself, so the cheapest test in the suite proves the way a
  player actually leaves. It fails loudly if Escape stops working,
  rather than hanging.

## [0.11.6] - 2026-08-30

### Fixed

- **The song list scrolls, and stops pushing the screen apart.** The
  list had no height limit, so it simply grew: at 23 songs the title,
  the details line, the import hint and the entire footer had been
  pushed off the screen, and the first and last rows were sliced
  through the middle. The rows now live in a bounded viewport that
  scrolls, and the selection is kept inside it — moving as little as
  possible, because a list that re-centres on every frame twitches
  under the cursor and makes its neighbours unreadable.
- **The viewport holds a whole number of rows.** A window whose height
  is not a multiple of the row pitch cuts its last row through the
  letters. The height is snapped to whole rows from the row height as
  *measured*, not assumed, so this cannot drift out of step with the
  UI kit's type scale. It accounts for the border as well as the
  padding: Bevy sizes a node by its border box, and ignoring that left
  the last row two pixels short of its own space.

### Added

- **The details line names your place in the list** — `7/23` before the
  BPM. With the rows clipped to a window, nothing else said whether
  three songs followed or thirty.
- `BEATBYTE_SHOT_ROW` selects a row before a screen is photographed. A
  scrolling list is indistinguishable from a short one until the
  selection moves past the fold, so without it the scroll could only be
  argued about rather than seen.

## [0.11.5] - 2026-08-28

### Added

- **A song ribbon along the top of the screen** — title, artist, a
  progress bar and `elapsed / total`. Nothing on screen had said where
  you were in a song, and that is not decoration: hype is a resource
  you spend, and spending it well depends on knowing whether there are
  thirty seconds left or three minutes. It sits in the one strip of
  the frame the neck never reaches, since the neck runs to a vanishing
  point in the middle, so it covers nothing.
- Six tests for the ribbon's arithmetic, two of them for edges that
  bite: the song clock starts **negative** (there is a count-in), so an
  unclamped bar would begin part-filled and run backwards, and a
  chart with no declared duration would divide by zero.

### Fixed

- **Leaving gameplay now says why.** Twice in one day a report that the
  game had "jumped back to the menu" could not be answered, because
  every exit was silent: the log showed a song starting, then a song
  starting again, and nothing in between. Each of the three ways out —
  the song finishing, quitting from the pause screen, and a track that
  cannot be built — now logs itself. The **absence** of that line is
  informative too: it means the window or the process went, not the
  state machine.

## [0.11.4] - 2026-08-28

### Changed

- **The neck is ruled by the beat, not by the bar.** A line every four
  beats gives the eye nothing to keep time against — the surface reads
  as a road rather than an instrument. There is now a line on every
  beat, with the downbeat drawn at full width and brightness and the
  three between it at roughly half, so the bar structure stands out of
  the ruling instead of being lost in it.
- **Each theme gets a decorated border.** Researching what the genre's
  necks actually do turned up the trait that most identifies one: the
  stage announces itself along the *edges* of the neck, not only
  behind it. Six motifs — garage rivets, punk sawteeth, metal
  chevrons, stadium bands, psychedelic waves, cyber ticks — drawn for
  this game and generated from a hash like the board texture, so they
  ship no art asset and are identical every run. The strip sits
  outside the rail and costs no playfield.
- **Receptors are seated in a metal collar.** A coloured ring on a bare
  board reads as a drawn outline; a ring in a housing reads as
  something you could press. The collar is deliberately neither
  lane-coloured nor hype-tinted — it is hardware, and hardware does
  not change colour when the song does.

### Documentation

- **The stage guide claimed a stronger invariant than the code has.**
  It said the same song "scores identically" in both renderers. The
  score is not identical between runs of the same build: measured
  139 968 / 139 970 / 139 971 / 139 972 across four runs, each with
  463 perfect and 0 miss. Hype doubles for a fixed number of beats and
  the activation frame decides whether one more note falls inside it.
  The invariant is the judgment — perfect, miss and overstrum counts —
  and the guide now says that, because the old wording invited a
  comparison that proves nothing either way.

## [0.11.3] - 2026-08-28

### Fixed

- **A successful import now says so in the log.** Only the *start* of
  an import was logged, and only failures logged a finish — so a
  successful import and one that silently did nothing looked identical
  from the outside. That ambiguity is not theoretical: investigating a
  report that importing had stopped working, the log could not settle
  whether four imports had produced four charts, and the answer had to
  be reconstructed from file timestamps on disk.

## [0.11.2] - 2026-08-28

### Changed

- **A missed note and a stray strum now sound different.** They shared
  one sound — a low sine plus a click of noise, which read as a bass
  drum, so a mistake sounded musical. Both are now built from one
  voice, a pick landing on damped strings, differing where the mistakes
  differ: a **missed note** is dark and sags a fifth in pitch, because
  a note that never sounded is a deflation; a **stray strum** is
  brighter, tighter and deliberately dissonant (a tritone through a
  thin, buzzing pulse), because it is a noise you actively made. They
  are siblings rather than unrelated sounds, and normalised to the same
  peak — an error that is *louder* reads as the worse error, and these
  two weigh the same. The rate limiter still collapses a chain of
  mistakes into one sound, since a fumble usually produces both at once.

### Added

- **`cargo run -p beatbyte-audio --example sfx_lab`** renders the error
  sounds and four alternatives — a mute thunk, a fret buzz, a downward
  bend, a pick scrape — plus the sound they replaced, as WAV files and
  as one audition track that plays each three times in a row. A
  seventy-millisecond sound cannot be judged by reading its constants,
  and it has to survive firing repeatedly during a bad passage.
- Ten tests for the voices, including the one that matters: the miss
  must put measurably more of its energy below one kilohertz than the
  strum, so the two cannot drift into sounding alike.

### Fixed

- **The pulse oscillator carried a DC offset.** At the narrow duty
  cycles that make a buzz, `1 - 2·duty` is most of the signal: the two
  strongest components of the finished tritone were 16 Hz and 32 Hz —
  inaudible energy eating the headroom the audible part needed. The
  pulse is now zero-mean.
- **Voices ended mid-sound.** The buffer stopped while the envelope was
  still at 5 % and the step to zero was a click. A short release ramp
  now takes every voice to true silence.

## [0.11.1] - 2026-08-28

### Documentation

- **A "Running the Game" section in the README.** Building from source
  was documented; starting the thing you built was not, beyond a single
  `cargo run` line buried in the build instructions. The section now
  covers running the built binary directly, the `caffeinate` wrapper
  that keeps a macOS display from closing the window mid-session, the
  three switches worth knowing for a manual run, and where settings and
  imported songs actually live on each platform.
- **Where the working directory matters, and where it does not.**
  Assets resolve from the executable's own location, so the binary
  starts from anywhere — but the repository's `songs/` folder is read
  relative to the working directory, so starting elsewhere silently
  drops the charts kept there. Measured rather than assumed: nine songs
  from the repository root, four from `/tmp`.

## [0.11.0] - 2026-08-28

### Added

- **The documentation's numbers are enforced, not maintained.**
  `apps/beatbyte/tests/docs_stay_true.rs` reads the repository as data
  and fails when a document disagrees with it: the per-crate test
  table and its total, the manifest version against the CHANGELOG and
  against the internal dependency pins, the badges that state a fact,
  the ADR index against the files on disk, every `BEATBYTE_*` switch
  against the harness reference, and every repository link in the
  README. Written because the test count had already been corrected
  twice in one day and was stale again by the evening: prose can be
  reviewed, a number cannot, because nothing about a wrong one looks
  wrong.

- **A guide to the 3D stage** (`docs/ui/3d-stage.md`): the coordinate
  conventions, why the two scales must differ, what each piece of the
  venue is for, and the traps the module has already sprung — a shared
  material greying a whole lane, emissive bleeding through bloom, an
  eased value advancing once per entity. It is the largest module in
  the game and was documented only inside itself.

- **The gameplay rules document is bound to the code.** It quotes the
  multiplier thresholds, the meter a phrase awards and the activation
  threshold as figures; those live in `ScoreConfig`, and a document
  that quotes a constant goes wrong the moment the constant moves —
  silently, because the prose around it still reads well. A test now
  reads the configuration and checks the document states it.

- **A stated versioning rule.** The patch number now rises with every
  user-visible change, in the same commit, so the version a build
  reports identifies that build rather than the last release. Tags
  stay a separate act at milestones. A test fails if the manifest ever
  carries a version the CHANGELOG does not describe.

- **Tests for the latency calibration and the window-size switch** —
  that too few taps yield no verdict at all, that the offset is
  reported in milliseconds and keeps its sign, that one wild tap
  cannot move the median, and that a malformed `BEATBYTE_WINDOW` is
  declined rather than guessed.

- **A flame off the fret when a note lands.** The genre's signature
  moment, and the one thing the stage still did not do: a hit lit the
  receptor, spread a flat ring across the board, and that was all. The
  flame is white-hot at the strike and cools to the lane's colour as
  it dies; a held sustain keeps a low one burning under the fret.

- **A crowd that moves on the beat**, driven from the song's own tempo
  map rather than a free timer, each head on its own phase so the
  ranks ripple instead of pumping as one block. Honours Stage Motion
  like every other ambient movement.

- **A stage that is lit rather than merely visible.** Measured, the
  venue sat at 0.13 brightness and 0.20 saturation — a white key light
  on grey materials returns grey however many boxes are in the room.
  Two coloured lamps from opposite sides, a lit backdrop on the rear
  wall, and materials that accept light bring it to 0.20 and 0.29,
  while the fretboard's own brightness is unchanged: notes keep their
  contrast against the board, which is worth more than atmosphere.

- **Lane dividers.** Five coloured lines say where the lanes are; a
  divider says where one ends, which is the difference between a
  highway and five parallel wires.

- **Gems with a lit face.** A generated radial highlight, rather than
  a second mesh per note.

- **A neck with the proportions of the genre, and a board with a
  surface.** Measured against the reference rather than eyeballed: a
  solo neck filled 31 % of the frame where the genre's fills about
  half, which left the eye nothing to do with the rest of the screen
  and made the gems read as beads on a thread. One spread factor,
  applied where the width is actually derived, widens rails, lane
  strips, receptors, bar lines, phrase bands and notes together —
  **solo only**, because two to four necks already use the room. The
  bed also gained a generated grain, so a fretboard reads as a thing
  rather than the absence of one; its brightness is pinned by a test,
  because "subtle" is the kind of intent that erodes one tweak at a
  time.

- **The results screen is a verdict, not a receipt.** It used to be
  bare text floating in a void at the one moment that is supposed to
  be the payoff. Now the song is the heading, a grade badge sits
  beside the score, accuracy has a bar as well as a figure, and the
  judgment breakdown carries the same colours the popups used during
  the song — so it reads as a summary of what was on screen.

- **Energy phrases are finally visible.** Charts have carried
  `phrases` all along and completing one has always paid a quarter of
  the hype meter, but nothing on screen said which notes those were —
  the player earned energy without being told why. Notes inside a
  phrase now wear a lit rim (the face keeps its lane colour, because
  the fret to press must never be obscured) and the stretch of neck
  they sit on is tinted, so a phrase can be seen coming.

- **The readouts became instruments.** The score is a fixed-width
  counter with dim leading zeros; the multiplier has its own box; a
  row of beads shows how far the streak has come toward the next
  level and empties when a miss costs it. The hype meter shows the
  four quarters it actually fills in, with a hairline for the quarter
  in progress and a line saying whether it is ready to use.

- **Activating hype transforms the stage.** The neck washes to the
  energy colour and eases back when it ends.

- **A held note stays lit for as long as you hold it.** On the 3D
  stage, striking a sustain used to make the whole note vanish — tail
  included — leaving a single burst and then nothing, however long the
  key stayed down. Now the gem lands and goes while the tail remains
  and is eaten from the hit line inward, and the fret keeps burning:
  the ring re-blooms about three times a second and the glow breathes
  rather than sitting at a fixed brightness, because a constant
  maximum is a state, not an animation. Letting go early greys the
  remaining tail and slides it away, so dropping a hold looks
  different from playing it out. Judgment is untouched — the same runs
  score identically (98/98, 282/282 and 624/624 perfect).

- **A venue behind the 3D stage.** The fretboard used to run through a
  void: outside the bed the screen was black. There is now a room — a
  rear wall, side walls, a lighting truss with sweeping beams, speaker
  stacks flanking the near end, and ranks of crowd silhouettes behind
  barriers — built as real geometry on the stage layer and tinted from
  the active theme. The beams honour the Stage Motion setting like
  every other ambient movement, and the whole venue is kept outside the
  bed so it can never occlude an approaching note.

- **Solo readouts sit in framed corner plates** — score, multiplier and
  combo bottom-left, the hype meter bottom-right — the way the
  arcade-era games laid them out. The old HUD stacked everything above
  the highway, which the depth view could carry but the 3D stage could
  not: there the neck runs to a vanishing point, so "above the highway"
  is the middle of the screen and the numbers floated over the horizon.
  Multiplayer keeps its per-highway blocks: with two to four necks side
  by side there are no free corners, and a score has to sit above the
  highway it belongs to.

- **Documentation for the UI design system** — `docs/ui/design-system.md`
  (tokens, row states, the pointer rule, how to add a screen without
  breaking the set) and [ADR-0010](docs/decisions/ADR-0010-ui-design-system.md),
  which records the alternatives that were rejected and why.

- **A harness reference** — `docs/development/harness.md` documents all
  14 `BEATBYTE_*` variables. Twelve of them existed only in the source.

- **An ADR index** — `docs/decisions/README.md`, which also explains the
  gap at 0009 (parked on a branch) instead of leaving it a mystery.

- **19 new unit tests** covering pure logic that had none: the
  scoreboard's record rule, theme selection and cycling, and the
  settings clamps.

- **One design for every menu.** A shared UI kit (`ui_kit`) now owns
  the type scale, the spacing rhythm and the row states, and the main
  menu, settings, controls, song browser, multiplayer join,
  calibration and input tester all draw from it. Screens sit inside a
  framed panel; a selected row is marked by an accent bar, a tint and
  a bright label together, rather than by the colour of its letters
  alone. Every screen now carries a subtitle saying what it is for,
  and one footer style states its keys as `KEY action` pairs.

- **`BEATBYTE_SHOT_STATE=<screen>`** boots straight into one screen
  and, with `BEATBYTE_SHOT_DIR`, photographs it and quits. The
  autopilot only ever reaches the menu, the browser and the results
  screen, which left settings, controls, calibration and the input
  tester as the screens least likely to be checked after a change —
  exactly backwards.

- **A solid 3D stage.** A third view alongside FLAT and DEPTH,
  reached by cycling the VIEW setting: a perspective camera looking
  down a real fretboard — bar lines crossing the neck at every bar
  and fading with distance, bright rails down both edges, coloured
  lane lines running to the vanishing point. Notes are flat buttons
  lying on the board (a coloured face inside a dark rim), sustains
  are tubes of the note's real held length, and receptors are rings
  that sink into the neck when held and flare through the bloom pass
  when struck. Judgment is untouched — the same run scores the same
  in all three views (624 perfect / 0 miss, verified).

- **`BEATBYTE_FPS=1`** reports median and 99th-percentile frame times
  every five seconds. The percentile rather than an average, because
  an average hides exactly the stutters that make the game drop notes.

- **3D hit feedback.** A struck note now VANISHES at the line instead
  of flying past the camera, the fret it landed on fills solid and
  flares, and a flat ring of light spreads across the board from it —
  the genre's flame, gone in about a fifth of a second. Missed notes
  grey out and keep travelling.

- **Guitar-Hero-style chart generation.** Imports now transcribe the
  LEAD of the song, not just its percussion: a new melody-extraction
  stage (HPSS harmonic/percussive separation → register-weighted
  pitch salience → DP contour tracking → note segmentation) delivers
  melody notes with true start, end and pitch. The generator adapts
  the hand-charting conventions: lanes follow the riff's pitch
  contour (green low → orange high, relative intervals), a held tone
  becomes a sustain of its REAL held length (trimmed by the
  tempo-scaled trailing gap: 1/32 whole note below 100 BPM, 1/24 to
  140, 1/16 above), soft entries without a percussive attack still
  chart, and while a strong melody note is held the lead owns the
  highway (no drum hits stacked on a sustain). Measured on a real
  m4a track: melody coverage 86%, held notes 8 → 147, hard/expert
  sustains 2 → 34/24 with genuine varied lengths.
- **Consistent difficulty curve.** Difficulties are now thinned to a
  target note DENSITY (notes per beat) instead of absolute strength
  thresholds, and each difficulty is a reduction of the next harder
  one — the official workflow. Measured across five real imports, the
  easy→medium jump was 1.4x on one song and 3.6x on another (easy
  ranged from 0.42 to 1.40 notes/s); it is now exactly 2.0x on every
  song with easy at 0.66–0.79 notes/s. The reduction chain also makes
  "every easy note exists on expert" structural rather than lucky: a
  one-shot derivation can drop a note the easier chart kept (pinned
  with the fixture that breaks it).
- **Master-derived difficulties.** All four difficulties now derive
  from ONE master chart (the official charting workflow): lower
  difficulties are subsets, the same musical event keeps the same
  lane (remapped to 3/4/5 lanes) and the same tail everywhere —
  leveling up is the same song with more notes, never a re-chart.
  Pinned by tests: easy/medium ⊆ expert, cross-difficulty lane
  consistency, order-preserving lane remap.
- **`beatbyte-cli analyze --json <path>`** dumps the full analysis
  (including the melody) for inspection; the text output now counts
  melody notes and held tones.

- **Live mute toggle**: `M` — or clicking the always-present corner
  badge — silences/unsilences music AND sound effects at any moment,
  in menus, gameplay and running autopilot sessions alike.
  `BEATBYTE_AUTOPILOT_MUTE` now only sets the starting state instead
  of being the unchangeable truth. (In the editor, `M` stays the
  metronome; the badge still works there.)
- **Test expansion**: exact hit-window boundary pins, Hype
  multiplier math, BPM validation bounds, sustain truncation by
  strong onsets, the default-binding user contract (ASDFG / Space /
  Enter), grade thresholds, X-plorer chord+strum decode, settings
  round-trip, depth-projection collinearity.
- **README overhaul**: 60+ factual badges, a researched guitar/
  controller support matrix, a step-by-step "how your music becomes a
  playable track" pipeline section, expanded testing docs, and a
  support section (PayPal donations, Google review link).

- **Mouse support across the menus.** Main menu rows hover-select and
  click-activate; song select scrolls with the wheel, click selects,
  a second click starts (right-click = back); settings rows
  hover-select, click steps a value (or opens Controls), the wheel
  steps too; multiplayer join, key-capture, the input tester and the
  results screen all honor right-click as back. Gameplay itself
  stays keyboard/guitar — the mouse is a menu device.
- **INPUT TEST menu entry**: the free-play device tester now sits in
  the main menu (it existed but a stale running instance hid it —
  `open` only foregrounds an already-running app).

- **Input-mode badge**: a quiet corner tag in gameplay shows
  `< TAP >` or `< STRUM >` — one glance answers "why did that (not)
  hit" while testing keyboard and guitar in either mode.

- **Space is the keyboard strum** (arrows still work); Hype moved to
  Enter. With tap mode off, ASDFG + Space is the natural two-hand
  split.
- **"STRUM!" coach**: with tap mode off, when a note dies while its
  fret is correctly held, a short on-stage hint explains the strum —
  exactly the confusing moment, rate-limited so it teaches instead
  of nagging.

- **Native Guitar Hero X-plorer support.** The guitar is an
  Xbox-360-class USB device speaking a vendor protocol — macOS (and
  thus the gamepad backend) never sees it, verified on the real
  hardware. A built-in libusb reader now streams its reports and
  feeds them into the engine as a genuine gamepad, so the existing
  bindings (green..orange frets, d-pad strum, Back = Hype, Start =
  pause), menu navigation and multiplayer join all just work.
- **Controller tester in the Controls screen**: shows connected
  devices by name and five live fret lamps driven through the real
  input map — press a fret, see it light.

- **Stage polish for the depth view**: receptors lie flat on the
  board (perspective-squashed rings), a glowing hit line spans the
  highway, every gem carries a colored halo, a stage vignette darkens
  the corners, and fret lines fade with distance.

- **Depth view** ("View: Depth" in settings, next to the flat
  classic): the highway becomes a real trapezoid running into a
  vanishing point, lanes lean toward it, notes approach from the
  distance and grow, fret lines and sustain tails follow the
  projection. Purely presentational — the autopilot scores identically
  in both views (23640 == 23640), because judgment never sees pixels.

- **The round style went AAA**: real HDR bloom on the camera (round
  style only — pixel art stays crisp), gems as lit glossy spheres
  (grayscale-shaded body × lane tint + untinted specular overlay,
  slightly emissive so they glow), lane guides and fret lines as soft
  glow strips, a depth-gradient highway bed, soft gaussian particles
  and backdrop dots, and sustain tails as glowing tubes. All textures
  generated and unit-tested; the 8-bit style is pixel-for-pixel
  untouched.

### Changed

- **Settings rows are two real columns** instead of one string padded
  to a fixed width. The old padding assumed labels of at most 16
  characters, which "TAP MODE (NO STRUM)" overflows by three, so that
  one row's value hung outside the column.

- **The controls screen answers the mouse and the gamepad.** It read
  the arrow keys directly, so a player holding a guitar could not
  reach the screen that rebinds it, and it was the only menu whose
  rows ignored hover and clicks. It now navigates through `MenuNav`
  like every other screen. A row waiting for a new binding is shown
  in its own colour rather than looking like an ordinary highlight.

- **The song browser lists title and artist as separate columns**, so
  the list scans by title.

- **Multiplayer slots show their player colour** on the row itself,
  and an empty slot reads "open" instead of `---`.

- **The flat view is gone.** VIEW now switches between DEPTH and 3D
  STAGE. A settings file that still selected flat is corrected on
  load, so nobody ends up on a highway with no depth and no way back.

- **Fret feedback rebuilt along genre lines.** In this genre the HIT
  is the spectacle — the gem bursting into flame at the target line —
  while holding a fret is a quiet readiness cue. So: a held fret
  **fills** with its lane colour (crisp edge, no haze) and presses
  slightly down, and a landed note fires a burst that starts tight and
  bright at the strike and expands outward as it fades, with its force
  taken from the judgment (a Perfect lands harder than a Good). The
  first attempt had this backwards and haloed every press, which read
  as constant noise.

- **Tagline no longer claims "8-bit game"**: the menu subtitle reads
  "five lanes. your music." and the README describes both looks —
  the game has shipped a smooth high-res style for a while.

### Fixed

- **Letters with diacritics no longer render as boxes.** The earlier
  fix measured the wrong font: Press Start 2P carries 656 glyphs and
  does have `å`, but the game uses the engine's built-in face whenever
  the round note style is on — the default — and that face has **95**,
  plain ASCII. Folding is now gated on the active style, because
  turning "Björk" into "Bjork" when the font can draw it is damage,
  and leaving it when the font cannot is a box.

- **Imported titles no longer show empty boxes.** Press Start 2P has
  656 glyphs — plenty of Latin, including `å` and `ß` — but nothing
  from the fullwidth or mathematical blocks, which is exactly what a
  downloader substitutes for `|` and `/` in a file name. Those
  look-alikes are now mapped back at display time, so the chart keeps
  its true title and a script the font cannot draw is left alone
  rather than turned into question marks.

- **The hype overlay was washing the venue instead of the highway.**
  It is a 900-pixel vertical band the width of the bed — the shape of
  a highway in the flat and depth views, and nothing like one in 3D,
  where the neck is a receding plane. Measured, it left the rails
  untouched and turned a wall forty units behind the vanishing point
  violet. It is skipped in 3D now, which tints its own surfaces.

- **The 2D sprite backdrop no longer speckles the 3D fretboard.** The
  stage camera draws at order −1, so those sprites render in FRONT of
  the 3D stage rather than behind it — in that view they were confetti
  over the board, not a backdrop. They are skipped when the 3D stage is
  active, which now supplies its own.

- **Hovering selects a song.** The song browser handled only
  `Interaction::Pressed` — there was no hover branch at all, so the
  pointer could sit on one row while another stayed highlighted, and
  starting a song took two clicks. All four row screens now read the
  pointer through one shared rule: hovering selects, clicking activates
  the row under the pointer.

- **A wrong cross-reference in the architecture overview** pointed at
  ADR-0005 for gameplay timing; that is ADR-0004.

- **Stale counts in the README** — the test badge and the testing
  section were 37 tests behind.

- **Long bindings no longer collide with their label.** "Enter / PAD
  Select / PAD RightTrigger" ran into the word HYPE; values are now
  bounded and wrap, right-aligned so the column keeps a clean edge.

- **The settings footer no longer claims ENTER confirms.** ENTER
  steps the value, exactly like RIGHT.

- **Two copies of the lane palette are gone.** The controls screen and
  the input tester each carried their own hard-coded copy of the five
  lane colours, which `palette.rs` is documented as being the single
  source of.

- **Notes in the 3D stage turned black.** All notes of a lane shared
  one material, so greying out a single missed note repainted every
  note in that lane for the rest of the song. Missed notes now switch
  to a separate grey material instead of repainting the shared one.
- **Notes in the 3D stage crawled.** Depth was using the same scale
  as width, so a note took 13.7 s to cross a highway it should cross
  in the 2.6 s of spawn lookahead. The two scales are now separate —
  and a compile-time assertion stops them being merged again.

- **Depth-view sustain tails hug their string.** The tail sprite
  extended straight up while the lane leaned toward the vanishing
  point — the far end visibly detached from the line (user
  screenshot). Tails now connect the gem to the projected position of
  their far end along the exact note path (both while approaching and
  while held), with foreshortened length and matching rotation.

- **Solo play now hears every input device.** The single player was
  hard-routed to the keyboard, so a connected guitar lit menus but
  played into the void during gameplay (no receptor highlights, no
  hits). With one player, keyboard and all pads feed the same
  session; strict per-device routing still applies in multiplayer.

- **The stage now fits every window.** The camera used raw window
  pixels, so a small window cropped receptors and HUD while a big one
  shrank the stage into a corner. The world renders through a
  guaranteed-minimum 1280x720 view that scales with the window (extra
  aspect shows more backdrop, never cropping), and the screen-space
  UI scales with window height so menus stay proportional. A
  `BEATBYTE_WINDOW=WxH` variable pins the size for tests or taste.

- **Depth view: notes now sit exactly ON their lane lines.** The
  guides were drawn on a different straight line than the note path
  (full lane width 200 px below the receptors, aimed at the vanishing
  point) — everything visibly missed its string. The guides are now
  the extension of the exact line notes travel (pinned by a
  collinearity test).

## [0.10.0] - 2026-08-25

**The first-playtest release** — everything in it traces back to the
first real hands-on sessions.

### Added

- **Multi-file drag-and-drop import with a visible progress panel**:
  drop any number of files in one gesture — they queue up, an
  animated overlay (pulsing frame, easing progress bar, flash per
  finished song, batch summary) shows the whole batch, in the menu
  and the browser alike. Unsupported files and duplicates are counted
  and reported, never silently discarded — the first version imported
  ONE file per gesture and dropped the rest without a word ("it
  looked like songs were lost").

### Fixed

- The library deduplicates identical songs across its scan roots
  (repo `songs/` vs the user songs directory) — the same import in
  both places showed up twice in the browser.

### Added

- **Sustain generation listens to the music now**: a note holds while
  its energy keeps ringing and no absolutely-strong new onset strikes
  — the gap to the next note only bounds the length. The old rule
  required near-silence after the note, so dense live recordings got
  almost none (a 7-minute live track: 3 sustains on medium — now 51;
  the sparser studio track went 53 -> 92, and Hard/Expert finally get
  sustains at all).

- **App icon shows the game now**: the yellow "B" sits above the
  five round receptor gems (green/red/yellow/blue/orange, white core
  + dark ring) with faint lane guides — still fully generated, no
  binary assets.

- **Tap mode is now the default** — the first real playtest showed
  keyboard players press frets while notes die (the strum requirement
  is invisible); strumming remains as the opt-in setting, and tap
  runs now record to the scoreboard. The autopilot's direct feed only
  strums when the note is still pending, so it plays correctly in
  both modes.
- **Round style is now a full look**: smooth font instead of the
  pixel face, bar ("fret") lines scrolling on the highway, and
  soft-disc particles and backdrop dots. 8-bit remains the default
  style and is pixel-for-pixel unchanged.

- **Note Style setting**: the 8-bit per-lane shapes can be switched
  to a classic round-gem look (colored disc, white center, dark ring
  on strum notes — HOPOs carry no ring). Round gems render from
  128-px anti-aliased, linearly sampled textures, deliberately
  smooth against the pixel-art default. The 8-bit shapes remain the
  default (they are the colorblind-safe look).

### Added

- **Delete songs from the browser**: `Backspace`/`Del` on a
  highlighted song (press twice to confirm). Imported songs lose
  their whole folder, hand-managed charts only the chart file (the
  audio stays); built-ins cannot be deleted.

- **Drag-and-drop song import**: drop an audio file onto the window
  (menu or browser) — it is copied to `songs/imported/`, analyzed and
  charted in the background, and appears in the browser with a status
  line. Downloaded-style file names come out clean ("Artist - Title
  (Official Video) [id]" → title/artist, bracket noise stripped).
- Harness audio is a switch now: audible by default,
  `BEATBYTE_AUTOPILOT_MUTE=1` for silence.

- **Sustain notes are animated while held**: the gem pins to the hit
  line and pulses toward white, the tail is consumed from the bottom
  (remaining length = remaining hold) and glows; released early it
  drops to a spent, dim look. Hold sparks were already there.

- **Tap mode** ("TAP MODE (NO STRUM)" in settings): notes hit on the
  fret press alone — keyboard-friendly assist. Strums still work on
  top; tap-mode runs stay out of the scoreboard.
- **Real-keyboard autopilot** (`BEATBYTE_AUTOPILOT_KEYS=1`, plus
  `BEATBYTE_AUTOPILOT_NO_STRUM=1`): presses actual KeyCodes through
  the full input chain. Proved three ways: classic keyboard play
  flawless, tap mode without strums flawless, no-tap without strums
  = 117 misses.
- Autopilot runs are now **silent** (music and SFX muted) and use a
  small window — they were driving the human at the machine to quit
  them mid-run.

### Fixed

- A second fake-pass hole: the harness exiting without any verdict
  now fails on platforms where the event loop returns (Cmd+Q remains
  invisible to the process — silent runs remove the reason to Cmd+Q).

## [0.9.0] - 2026-08-24

**The content, accessibility and editor-v2 release.**

### Fixed (release engineering)

- macOS DMG creation retries through the runners' spurious
  "No space left on device" hdiutil flake (95 GiB were free when it
  struck); artifact actions moved to their actual Node 24 majors
  (upload v7, download v8).

### Added

- **Editor: metronome overlay during audition** — `P` already played
  from the cursor; it now ticks on every beat so grid alignment is
  audible against the music.

- **Editor: range selection and bulk edits** — `V` anchors a
  selection at the cursor, `X` deletes every note in the range (all
  lanes), `H` toggles HOPO on the whole selection; each bulk edit is
  ONE atomic undo step.

- **Editor: move a note** with `M` (grab, navigate, place; `ESC`
  cancels) — an invertible `MoveNote` op that keeps sustain and HOPO
  flags, so undo/redo stays exact.

- **Per-lane gem shapes** (square, circle, diamond, triangle, cross)
  on notes and receptors — color is never the only lane signal
  (colorblind accessibility, always on). Generated pixel-art masks,
  no assets; geometry unit-tested.
- **HOPOs are finally visible**: smaller gem with a bright core
  (they rendered identically to strum notes before).

- **Stage Motion setting** (reduced-motion accessibility): turning it
  off leaves the themed backdrop as a still image; particles, screen
  shake and beat pulse already had their own toggles.

- Forward-compatibility pins: settings and chart files with unknown
  (newer-version) fields load cleanly; missing settings fields fall
  back to defaults. This was already true — now tests keep it true.

- `docs/importing-songs.md`: a verified end-to-end guide for importing
  your own music (analyze → generate → play → correct in the editor).

- Supported import formats are now verified by decode tests against
  committed synthesized fixtures (WAV, Ogg Vorbis, FLAC, MP3 — and
  M4A/AAC, which turned out to work and is now documented) and listed
  in the chart-format spec.
- **Second built-in song: "Solder Groove"** (92 BPM) — a half-time
  groove over Dm–Bb–F–C with syncopated bass, sparse drums and held
  pad bars, so generated charts exercise sustains and slower reading
  instead of note streams (Medium charts 6 sustains vs 1 in "Circuit
  Breaker"). The library, song browser, autopilot and `beatbyte-cli
  demo` all know both songs.
- Autopilot can validate any library song: `BEATBYTE_AUTOPILOT_SONG`
  selects by index or case-insensitive title substring; a selector
  that matches nothing fails the run instead of silently playing the
  wrong song.

### Fixed

- **Autopilot can no longer fake a pass**: with the default window
  behavior, an environment-killed run (e.g. macOS display sleep
  closing the window mid-song) exited 0 without ever reaching a
  verdict. In autopilot mode the app now ignores window-close as an
  exit condition and fails loudly if the window vanishes before the
  results verdict.
- CI Linux smoke: `libxkbcommon-x11-0` was missing at runtime
  (winit's X11 path dlopens it).

- The song library now finds charts up to two folder levels below
  `songs/` — the documented `songs/imported/<song>/` layout was
  silently ignored by the one-level scan (found while validating the
  import walkthrough). Symlinked directories are not followed.

## [0.8.1] - 2026-08-23

### Fixed

- **All text was invisible when the game was launched directly from
  `target/` or any layout Bevy's default asset resolution misses**: the
  pixel font failed to load (a failed asset never retries in Bevy), so
  HUD, judgment popups, count-in, menus and results rendered no glyphs.
  The game now resolves its asset root explicitly across every
  supported layout — portable (assets next to the executable), macOS
  .app bundle (`../Resources`), development (current directory), and
  the workspace `target/` tree.
- Autopilot screenshots taken on state entry no longer capture the
  transition fade (short settle delay before each capture).
- Release CI: the arm64 macOS runner ran out of disk while creating
  the DMG — the packaging script now reclaims the build tree (CI only)
  before `hdiutil` runs.

### Changed

- README media refreshed: gameplay and results screenshots with the
  full HUD/text actually visible.

## [0.8.0] - 2026-08-23

**The polish milestone** (Milestone 13).

### Added

- **Screen-transition fades**: every state change fades in over a
  quarter second instead of cutting.
- **Count-in**: songs start with a two-second pre-roll — the first
  notes scroll in over a 3-2-1 banner and the music starts exactly at
  zero. No song opens with a wall anymore.

### Changed

- README and docs brought up to the finished-milestone state.

## [0.7.0] - 2026-08-23

**The editor and packaging milestones** (Milestones 11 + 12).

### Added

- **Chart editor** — engine-free core in `beatbyte-editor`
  (invertible edit operations, an `EditorSession` with undo/redo and
  dirtiness tracking, all unit-tested) plus an in-game screen: open
  any file-based song from the browser with `E`, step the beat grid
  (1/1 · 1/2 · 1/4), place/remove notes per lane, toggle HOPOs,
  preview the audio from the cursor, undo/redo, and save — saving is
  gated on chart validation. Leaving with unsaved changes asks twice.
- **macOS .app + DMG**: proper bundle with a procedurally generated
  pixel icon (hand-rolled PNG encoder, stdlib only), Info.plist,
  ad-hoc signature; assets resolve from `Contents/Resources`.
- **Linux AppImage** with desktop entry and icon.
- Release CI now attaches DMG and AppImage next to the portable
  tar.gz/zip archives.
- Songs are also scanned from the user data directory
  (`…/beatbyte/songs`), so installed builds have a place for music.
- Autopilot editor mode (`BEATBYTE_AUTOPILOT_EDIT=1`): opens the
  editor on a real file, edits, undoes, redoes, saves and verifies
  the file on disk.

## [0.6.0] - 2026-08-23

**The themes milestone** (Milestone 10): six original stages, all
data (ADR-0008).

### Added

- **Six stage themes**: Garage (warm amber, twinkling starfield),
  Punk (hot pink, pogo crowd), Metal (steel, rising embers), Stadium
  (deep blue, sweeping spotlights), Psychedelic (violet, drifting
  bubbles), Cyber (neon, rolling synth grid).
- **Procedural backdrops** — engine-drawn pixel sprites animated by
  one system, beat-aware where it reads well; no textures, no
  assets.
- Theme selection in settings: a fixed stage or **AUTO**, which picks
  deterministically per song title (same song, same stage).
- Highway beds, lane guides, receptors, notes, sustain tails and hit
  particles all take the active theme's palette; the beat pulse
  strength is per theme. Judgment colors stay constant — readability
  first.

## [0.5.0] - 2026-08-23

**The multiplayer milestone** (Milestone 9): 2–4 players, one machine.

### Added

- **Join screen** (main menu → Multiplayer): the keyboard and every
  connected gamepad claim player slots by pressing fret 1; mode
  toggle between **Versus** and **Co-op**; player accent colors.
- **Split highways**: the layout scales for 1–4 players (lane
  spacing, note sizes and receptor sizes shrink as highways
  multiply); every player gets their own receptors, notes, sustain
  tails and lane guides.
- **Per-device input routing**: a keyboard player only hears the
  keyboard, a pad player only their own pad — through the same
  bindings table.
- **Per-player everything**: world-space HUD blocks (score, combo,
  multiplier, Hype bar) above each highway, judgment popups, hit
  particles, Hype overlays and sustain sparks all follow their
  player. The stage pulse hardens when *anyone* is in Hype.
- **Multiplayer results**: ranked list for Versus, band total plus
  breakdown for Co-op; solo results (grade slam, count-up,
  NEW RECORD) unchanged. High scores stay solo-only by design.
- Autopilot can now simulate N players
  (`BEATBYTE_AUTOPILOT_PLAYERS=2..4`) and requires a flawless run
  from every one of them.

### Changed

- Sessions, spawn cursors and feedback messages are fully per-player;
  the gameplay systems iterate players instead of assuming one
  (ADR-0002's "players are data" delivered end to end).

## [0.4.0] - 2026-08-23

**The controllers milestone** (Milestone 8).

### Added

- **Input abstraction**: physical input → binding → game action.
  Bindings are data, persisted with the settings; gameplay only ever
  sees actions (ADR-0004's input model).
- **Gamepad support** on every connected pad: frets on the face
  buttons + left shoulder (the common guitar-controller layout —
  green=South … orange=LB), strum on the D-pad, Hype on Select/RT,
  pause on Start. Guitar-style controllers that enumerate as gamepads
  work out of the box.
- **Menus speak gamepad**: D-pad navigation, South=confirm,
  East=back on all menu screens.
- **Remapping screen** (Settings → Controls): every action listed
  with its bindings; Enter captures the next key or button (stealing
  it from whichever action held it), Backspace restores a row's
  defaults. Persisted with the settings; invalid entries in edited
  config files are dropped safely.

### Changed

- Bevy's `serialize` feature is enabled so input types persist
  naturally.

## [0.3.0] - 2026-08-23

**The UI milestone** (Milestone 7): BeatByte grows its screens — and
its voice.

### Added

- **Pixel font identity**: Press Start 2P (OFL 1.1, license bundled)
  across every screen — boot, menus, HUD, popups, results.
- **Main menu**: Play / Settings / Calibration / Quit with keyboard
  navigation.
- **Song browser**: the bundled demo plus every valid chart found in
  `songs/` (invalid charts are skipped with a log line, never a
  crash). Difficulty stepping is constrained to what each chart
  offers; the details line shows BPM, duration and your best score.
  File songs stream from disk; the demo plays from memory.
- **Settings screen**: music/SFX volume, scroll speed, latency
  offset, particles/shake/beat-pulse toggles, fullscreen — changes
  apply immediately and persist to the platform config directory.
  Corrupt settings files fall back to defaults instead of crashing.
- **Latency calibration**: tap along with a click track, the median
  offset (8+ taps) becomes your setting. Gameplay subtracts the
  offset from input timestamps (ADR-0004's calibration model).
- **High scores**: best score/accuracy/streak per song + difficulty,
  saved to the platform data directory; the results screen celebrates
  new records, the browser shows your best.
- Scroll speed and latency offset now actually drive gameplay
  rendering and input timestamping.

### Fixed

- A startup ordering crash (system reading a resource before its
  startup command applied) — caught by the autopilot harness; shared
  UI resources are now inserted at plugin build time.
- Strict-docs CI failure (private intra-doc link).

## [0.2.0] - 2026-08-23

**The game feel milestone** (Milestone 6): BeatByte stops feeling like
a tech demo.

### Added

- **Session feedback bus**: judgment events are broadcast as engine
  messages once per frame; note visuals, particles, sounds and popups
  are independent subscribers (multiplayer-ready fan-out).
- **Pixel-confetti hit particles**: bursts sized by judgment (Perfect
  adds white sparks), sustain hold sparks at the receptor, a Hype
  activation salvo across all lanes — deterministic seeding, hard
  particle cap, zero allocations in steady state beyond spawns.
- **Trauma-based screen shake** on misses, overstrums and Hype
  activation (decaying, squared response — subtle by design).
- **The stage breathes**: highway brightness pulses on the beat grid
  (stronger under Hype), and a translucent Hype overlay glows when the
  meter is ready and breathes while it burns.
- **Combo-break flash**: a brief red wash so a dropped streak is felt
  without reading the HUD.
- **Procedural sound effects** — synthesized at startup, no audio
  binaries: menu move/confirm blips, a dry miss thud (rate-limited),
  a rising Hype sweep. Note hits stay deliberately silent: the music
  is the hit sound.
- **Menu & results juice**: the title breathes, the grade letter
  slams in with overshoot, the score counts itself up.
- `EffectSettings` resource (particles / shake / beat pulse toggles)
  ready for the accessibility settings screen.

## [0.1.0] - 2026-08-23

**BeatByte is playable.** First playable prototype (Milestone 5).

### Added

- **The gameplay screen**: five-lane highway with receptors, falling
  notes (chords, HOPO markers, sustain tails), all note positions
  derived from the song clock every frame — never from frame counts.
- **Keyboard play**: frets `A S D F G`, strum `↑`/`↓`, Hype `Space`,
  pause `Esc`. Inputs are timestamped with song time and fed to the
  deterministic judgment engine from Milestone 2.
- **Live HUD**: score, combo, multiplier (with Hype state), accuracy,
  Hype meter with activation hint; judgment popups and receptor
  flashes on every hit.
- **Screen flow** as explicit states: boot (background demo build) →
  main menu (difficulty select) → gameplay (with pause sub-state) →
  results (grade, score, judgment breakdown).
- **Players are entities**: each carries its own session component —
  the multiplayer-ready shape from day one.
- **Autopilot mode** (`BEATBYTE_AUTOPILOT=1`): the game plays itself
  perfectly through the real screens and input path, then exits with
  success only on a flawless run — the end-to-end validation harness
  used before every release.
- The music thread bridge: song clock reconciliation against the
  audio device every frame; missing audio devices degrade gracefully.

### Changed

- Dev profile builds `beatbyte-audio` at full optimization (demo
  synthesis + analysis: ~30 s → ~3 s at boot).

## [0.0.3] - 2026-08-23

### Added

- **Audio infrastructure** (`beatbyte-audio`):
  - Decoding of OGG/WAV/FLAC/MP3 into analyzable mono buffers with
    untrusted-input caps, plus a half-band FIR downsampler.
  - The `SongClock`: an anchored, monotonic, fully unit-testable song
    timeline with snap/slew reconciliation against the audio device
    (ADR-0005).
  - Music playback on a dedicated thread (rodio) behind a `Send`
    handle: play file/buffer, pause, seek, volume, atomic position;
    the game runs silently instead of crashing when no output exists.
  - The analysis pipeline: spectral-flux onset detection (with
    per-onset strength and brightness), autocorrelation tempo
    estimation with octave prior and sub-BPM interpolation, beat-grid
    phase fitting, RMS energy envelope — all pure and tested against
    synthesized ground truth.
  - Deterministic signal synthesis (`synth`) and the original bundled
    demo track "Circuit Breaker" by The Null Pointers, rendered
    entirely by code (ADR-0006) — no audio binaries in the repository.
- **Automatic chart generation** (`beatbyte-chart::generate`):
  difficulty-profile-driven and deterministic — grid quantization with
  raw-onset fallback, strength filtering, density limits,
  brightness-driven lane assignment with jump limiting, chords on
  strong hits, auto-HOPO for fast runs, energy-aware sustains, phrase
  placement, loudest-window preview selection.
- **Real CLI** (`beatbyte-cli`): `analyze`, `generate`, `validate`,
  `inspect` now do real work, plus `demo` (renders the demo song and
  charts it through the actual pipeline). Proper exit codes.
- Analysis types (`SongAnalysis`, `Onset`) in `beatbyte-core::music`
  as the shared vocabulary between analysis and generation.
- Documentation: ADR-0005 (audio architecture), ADR-0006 (synthesized
  demo content), `docs/audio/analysis.md` including honest known
  limitations.

## [0.0.2] - 2026-08-23

### Added

- **Core domain model** (`beatbyte-core`), engine-free and fully
  unit-tested:
  - Lanes and lane sets (chords, held frets) with bitmask semantics.
  - Tempo maps (beats ↔ seconds, tempo changes ready), configurable
    symmetric hit windows and Perfect/Great/Good/Miss judgment.
  - Note events (taps, chords, sustains, HOPOs), special phrases and
    validated playable tracks.
  - Data-driven scoring: judgment-tiered points, streak multiplier,
    per-beat sustain scoring, weighted accuracy, and the Hype special
    meter (phrase gains, activation, beat-based drain).
  - The deterministic gameplay session (`TrackSession`): strum matching
    with anchoring, note skipping, overstrums, hammer-ons and pull-offs,
    sustain lifecycles, phrase tracking — identical inputs always
    produce identical outcomes.
- **Chart format v1** (`beatbyte-chart`): versioned JSON schema,
  tolerant reader with strict all-issues validation (version gate,
  numeric ranges, duplicate notes, phrase overlaps, note-count and
  file-size caps), path-traversal-safe audio resolution, chord grouping
  into gameplay events, and load/save helpers.
- Chart format specification (`docs/chart-format/`), gameplay rules
  documentation, ADR-0003 (chart format) and ADR-0004 (gameplay timing).

## [0.0.1] - 2026-08-23

### Added

- Cargo workspace with the full crate architecture:
  `beatbyte-core`, `beatbyte-chart`, `beatbyte-audio`, `beatbyte-game`,
  `beatbyte-cli`, `beatbyte-editor` and the `beatbyte` application.
- Minimal Bevy 0.19 application that opens the BeatByte window and shows
  the boot screen.
- Continuous integration (formatting, clippy, tests, multi-platform build).
- Release workflow scaffolding for macOS, Windows and Linux.
- Project documentation structure with the first Architecture Decision
  Records (Rust + Bevy, workspace layout).
- README, MIT license, contributing guide, code of conduct, security policy.

[0.11.0]: https://github.com/pepperonas/beatbyte/compare/v0.10.0...v0.11.0
[0.8.0]: https://github.com/pepperonas/beatbyte/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/pepperonas/beatbyte/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/pepperonas/beatbyte/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/pepperonas/beatbyte/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/pepperonas/beatbyte/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/pepperonas/beatbyte/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pepperonas/beatbyte/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/pepperonas/beatbyte/compare/v0.0.3...v0.1.0
[0.0.3]: https://github.com/pepperonas/beatbyte/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/pepperonas/beatbyte/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/pepperonas/beatbyte/releases/tag/v0.0.1
