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
