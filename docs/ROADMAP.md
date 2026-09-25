# BeatByte Roadmap

**Persistent source of truth** for what gets built, in what order.
Rules of engagement live in [`CLAUDE.md`](../CLAUDE.md); this file
holds the work itself.

## Song metadata and the library index (2026-09-21, ADR-0019)

Every song should carry a structured, migratable record — the ground
for search, filters, sorting, statistics, history, recommendations
and playlists. The shape and the reasons, including what was
rejected: [ADR-0019](decisions/ADR-0019-song-metadata-and-the-library-index.md).
Three domains, and the middle one is disposable: the **document** in
the song's folder is authoritative and portable, the **index** is a
projection that can be rebuilt from it, and **events** stay in the
telemetry store that already holds them.

⚠️ The analysis found three things worth carrying into every
milestone below. There is **no song identity today** — `scores.json`
keys on title and artist, so fixing a typo in a title orphans that
song's records. **SQLite with migrations already exists here**, and
its runner was written to take an arbitrary migration list, so a
second database is nearly free. And **`history.jsonl` and
`gameplay_session` are two records of one event** — a duplication
that predates this work and that M5 ends rather than extends.

- [x] **M1 The model** *(v0.18.1)*. `beatbyte-library`: `SongDoc` and
  its sections, `SongId`, `Sourced<T>` with the source ranking, the
  placeholder rules and the lifecycle semantics. Pure — no IO, no
  SQL, no clock, no network — so every rule in it is testable
  without a disk. The judgement calls it encodes: a provenance
  wrapper only on **contested** fields (a file size has one answer);
  a confidence only from a source that **estimates**; `imported_at`
  written once; and absent meaning `None` rather than `""`,
  `"Unknown"` or a year of `0` — with `Unknown Mortal Orchestra` as
  the counter-case that keeps the placeholder list short and matched
  whole.
  *Verified: 1584 tests (+25), gate green. Five mutation probes —
  the override rule, the confidence clamp, the read-does-not-update
  rule, the id's time prefix and the placeholder filter — each seen
  to fail.*
- [x] **M2 Migration: folders → documents** *(v0.18.3)*. `library`
  in the CLI walks a songs directory, reads what each folder already
  holds — the active chart, the loudness sidecar, which word files
  are there, the oldest file's time — and writes exactly one new
  file per song. No audio decoded, nothing guessed, nothing else
  touched.
  ⚠️ **A chord is ONE note event, not its rows.** A chart file stores
  a row per lane, so counting rows would make every chord-heavy
  chart look three times as dense as it plays — and would disagree
  with the telemetry store, which counts what the engine judged. The
  rows are grouped by the engine's own `CHORD_EPSILON_S`, and the
  proof is a cross-check against an independent source: the document
  says Maria's Medium chart has **428** note events, and the
  telemetry store recorded a session of **428**.
  ⚠️ **The first version of the report lied.** It asked whether a
  document existed *after* writing one, so the first run over a
  fresh library said "171 updated, 0 created" — wrong about the one
  thing it exists to say. Pinned as a pure function now.
  ⚠️ **And it was stricter than the game.** `girls-just-want-to-have-fun`
  keeps its chart as `girls.chart.json`; the version scheme resolves
  to `chart.json`, which is not there, so the folder was skipped
  while the game plays it happily. The migration now falls back the
  way the scanner does.
  *Verified on the real library — 171 folders, 2.4 GB:* 171
  documents written, a second run writing **nothing** (171 "already
  current"), and a checksum of every non-audio file before and
  after: **171 new `song.json`, 0 files changed, 0 files lost**.
  `imported_at` spreads across 2026-06, -08 and -09 rather than
  collapsing onto today, and 25 of 171 songs carry a genre — the
  honest number, because that is how many files carry a genre tag.
  ⚠️ The first checksum comparison reported "1239 changed" and was
  my own broken `join`: the paths contain spaces.
  *Verified: 1601 tests (+17), gate green.*
- [x] **M3 The index** *(v0.18.4)*. `library.db` with its own
  append-only migration list, and `beatbyte-cli library --index` to
  build it. Tables `song`, `song_genre`, `song_chart`; partial
  indexes where a column is mostly `NULL`.
  *Verified on the real library:* **171 songs indexed in 0.5 s into
  180 KB** (debug build), the commission's queries answered — genre
  distribution, 45 songs between 115 and 125 BPM, 73 artists, 13.4
  hours of music — and their plans checked with
  `EXPLAIN QUERY PLAN` rather than assumed.
  ⚠️ **The cross-database question is settled, by running it:**
  `ATTACH` joins `library.db` to `telemetry.db`, so "added but never
  played" (123 of 171) and "accuracy between 120 and 130 BPM" work
  across the split. The two databases cost nothing in reach.
  ⚠️⚠️ **And it measured how badly M5 is needed.** Joining on
  `chart_hash` is all that is possible today, and a hash changes with
  every redesign: *Maria*'s 94 sessions are spread over **nine**
  hashes, of which the index can reach two. Library-wide, **564
  sessions exist and 219 are reachable** — 61 % of the play history
  is invisible to "how often have I played this song".
  ⚠️ Two of four mutation probes did not fire, and both were my own
  blind tests: the rebuild was verified into an EMPTY index, so it
  never proved that a rebuild clears; and the cascade test did not
  prove the foreign-key pragma is on. The first is fixed. The second
  turned out to be an **equivalent mutant**, measured rather than
  guessed: rusqlite's bundled SQLite is compiled with
  `DEFAULT_FOREIGN_KEYS=1`, so a raw connection already reads `1`.
  The pragma stays — plain SQLite defaults it OFF — and the reason
  the probe cannot show it is written at the call.
  *Verified: 1607 tests (+6), gate green.*
- [x] **M4 The browser on the documents** *(v0.18.7)*. The list is
  built from each folder's `song.json`, not from its chart. ⚠️ The
  plan said "on the index" and the measurement said otherwise: a
  scan of 168 songs took **1161 ms cold / 481 ms warm**, and the
  documents are 369 KB against the charts' 102 MB — so the answer
  was the small file beside the big one, not SQL instead of a walk.
  A document that no longer describes the chart beside it is simply
  not used; lyrics stay a question for the disk, because running the
  aligner does not touch the chart.
  ⚠️⚠️ **And the first measurement said the shortcut had changed
  nothing** (1171 / 464 ms). It had not: the scan handed **every**
  JSON in a song folder to the chart parser, including 18.5 MB of
  word alignments and 17.7 MB of analysis context, to conclude they
  were not charts. One rule now says what a chart file is, shared
  with the migration that already had it.
  **Measured, warm: 481 → 317 ms (sidecars) → 229 ms (documents).**
  Cold: 1161 → 225 ms. Same 168 songs.
  **Two defects found on the way.** The browser counted chart
  **rows**, and a chord is several rows and one note to hit — 250 of
  684 charts here differ, one by 183 notes, and the difficulty
  rating is computed from that number. And a stray `foo.json` beside
  a first-generation chart would have been listed as a second copy
  of the song, so a document now names the file it describes.
  *Verified: 1624 tests (+10), gate green. Eight mutation probes —
  the shortcut never taken, the document's note count ignored, rows
  counted again, a sidecar accepted as the chart, a chart newer than
  its document unnoticed, a chartless document standing in, the base
  chart no longer generation one, and a word alignment counting as a
  chart — all seen to fail. The 481 → 229 ms split was measured by
  switching the shortcut off in a temporary build, not estimated.*
- [x] **M4b The import writes the document** *(v0.18.8)*. Found while
  measuring M4: a song imported in the game carried no `song.json`
  until `beatbyte-cli library` had been run over its folder, so until
  then it had no id — its records and its recorded sessions were keyed
  by its NAME, the one thing about a song that changes. The import now
  writes the document itself, from the facts it already holds. A
  re-import keeps the id and the moment the song first arrived, and
  follows the chart version that now plays.
  ⚠️ The loudness fork is closed **by construction, not by a pin**:
  there is one call site, because no test reaches that branch (the
  import decodes real audio) and two call sites is how one of them
  ends up missing the document. A blind mutation probe said so, and
  restructuring was the honest answer to it rather than inventing a
  test that would only have read the source back.
  Also: the conversion from a loudness report into document facts now
  lives in one place, which is why `beatbyte-library` may read what a
  song folder already holds.
  *Verified: 1626 tests (+2), gate green. Two mutation probes — a
  re-import forgetting the song's id and arrival, and a document that
  does not name its chart — both seen to fail.*
- [x] **M5 One identity for the user's own data** *(v0.18.5)*.
  Shipped: the store carries `song_id` beside `chart_hash` (schema
  v3, nullable and staying so — a run whose song was deleted has no
  song, and an invented id would be worse than the gap), the game
  records it from the document in the song's folder, and
  `beatbyte-cli library --backfill` gives the sessions already on
  disk their song.
  ⚠️ **Additive only.** An unmatched session keeps its gap rather
  than gaining a guess; an already-matched one is left alone; a
  title that two songs share resolves to **neither**. So the worst
  case of running it is that nothing happens — and the second run
  proves it: 0 matched, 0 changed.
  *Verified on this machine's store, after a backup:* **564
  sessions, 0 → 352 named** — 219 by chart hash, 133 by title. Of
  the 212 left, **208 are the built-in demo songs**, which have no
  folder and therefore no document, and **4 are one song the library
  holds twice**. Of the sessions belonging to real library songs,
  **352 of 356** resolve. The effect in one query: *Maria* (37 runs)
  appears in a most-played list keyed by song and is entirely absent
  from one keyed by chart hash, because its chart was redesigned
  four times.
  Also shipped *(v0.18.6)*: **`scores.json` is keyed by the song**.
  A record set under a song id survives a corrected title; one set
  before the library had documents still counts, and **moves** to the
  id the first time that song is played after — two records for one
  song would diverge. A song with no document (a built-in) keeps the
  old key, because a record under a guessed id is worse than one
  under a name. File v3; every v2 file reads unchanged.
  **The one thing left open here is CLOSED rather than done**, and
  the reason is worth writing down. The note used to read "`history.
  jsonl` is still written beside the store, the last place one event
  is recorded twice" — but ADR-0018 says plainly that the history is
  "a derived cache, recomputable from the raw events", and CLAUDE.md
  says "nothing reads the store back into the game". Achievements,
  the statistics screen and the CSV export all read the history, so
  making it derive FROM the store would break the rule the store
  exists under. Two records for two purposes is the design, not a
  defect: the store is raw and the game never reads it; the history
  is the game's own summary. What would still be worth having is a
  test that the two cannot DISAGREE — filed below.
  *Verified: 1614 tests (+7), gate green. Four mutation probes — the
  game recording no id, the matcher guessing at an ambiguous title,
  a lookup ignoring the id, and a stale name-keyed record left
  behind — all seen to fail.*
- [x] **M6 Cheap enrichment as a queue** *(v0.18.9)*. A document now
  records the file's **content fingerprint** (FNV-1a 64 + size, the
  value the import already computes, written `fnv1a64:<hex>:<size>`
  so a later build cannot mistake it for a stronger hash), its
  **lyric counts**, and whatever the file says in its **own tags**.
  Three ways in: the import fills them while the file is still warm
  in the page cache, `beatbyte-cli library` fills them for everything
  already on disk, and in the game a quiet pass visits one song at a
  time — never at start-up, never while the player is playing or
  waiting for something they asked for.
  ⚠️⚠️ **Measured before building: not one of 173 files carries a
  single descriptive tag.** They came from video downloads, which
  carry `major_brand` and `encoder` and nothing else. The tag reader
  is built and correct and finds **zero** album, year or label on
  this library — it exists for the other kind of import and must
  never be the reason a field is claimed to be known. What the pass
  really gained here: **171 of 171** fingerprints and **137**
  documents with line counts, 125 of them word-aligned.
  ⚠️ The worker asks ONE question — has this file been fingerprinted
  — and not "is this document complete". Completeness is derived and
  reported, but a worker driven by it would walk the whole library
  on every start for ever, because an incomplete song is the normal
  state here.
  **Also fixed:** the document recorded `aligned` from "a words.json
  exists", which a failed alignment also satisfies. And the
  duplicate report (`library --duplicates`, reports only, never
  deletes) tells a study twin from an accident **by its folder**: the
  first version compared folded titles and flagged a genuine twin
  whose song had been renamed afterwards. On this library: **85
  twins, and 2 real duplicates** — "99 Luftballons" imported twice
  with a twin each, and "Girls Just Want to Have Fun" in both an
  imported and a hand-made folder.
  *Verified: 1645 tests (+19), gate green, and the whole library
  fingerprinted in **5.5 s for 2.4 GB** with a second run writing
  nothing at all. Nine mutation probes — a twin counted as a duplicate, a
  cheap pass erasing a fingerprint, a failed alignment recorded as
  word-level, the worker chasing completeness, housekeeping running
  while the player waits, a folder with no document queued anyway,
  and one unmapped tag stopping the whole read — all seen to fail;
  two of them were blind at first and were sharpened.*
- [x] **M7 What is not measured yet** *(v0.18.10)*. One pass over the
  decoded audio gives energy, the three band shares, the spectral
  centroid, the onset rate, how periodic the song is, and a chroma;
  the key comes from the chroma by Krumhansl–Schmuckler. Every field
  has an algorithm, a range and a meaning in
  [`docs/audio/features.md`](audio/features.md), and every run is
  logged with its version so a better estimator can tell which songs
  the old one measured. Free at import (already decoded for the
  chart), ~0.5 s a song on demand, one song at a time in the game.
  Danceability and valence stay absent: no model here determines
  either.
  ⚠️⚠️ **Two defects, both found by measuring rather than by
  reading.** The chroma summed every FFT bin into the class it
  rounded to — bins are linear, pitch classes are not — so broadband
  content fell into a FIXED pattern: white noise came back with a
  1.9× spread and **135 of 171 songs landed in four black-key
  tonalities**, which is not what a pop library looks like. One
  window per pitch and the MEAN inside it fixed it; the same library
  now reads G major (32), A minor (24), C major (18) across 18 keys.
  And the margin was `(best − second) / best`, which calls two
  equally useless fits 0.15 — noise was read as A minor. It is the
  plain difference of two correlations now.
  ⚠️ A third, found by raising the threshold after a first pass: the
  code only ever WROTE a key, so 144 documents kept one the new bar
  rejected. A measurement that cannot tell now takes the old guess
  away.
  **Honest accuracy:** the margin is 0.12, which keeps 27 songs. Of
  the six whose key could be verified, **four are right** — Seven
  Nation Army, Born to Run, Video Killed the Radio Star, In the
  Shadows — and both failures are the dominant, a known weakness of
  profile matching on a bass-heavy mix. A relative major and minor
  share every note and cannot be separated by construction: *Nothing
  Else Matters* comes back as G major with a margin of 0.008, the
  estimator saying correctly that it cannot tell.
  *Verified: 1659 tests (+14), gate green. The counter-check is a
  test in its own right — noise must produce a flat chroma and no
  key — because a measurement that reports something is believable
  only once it has been shown to report nothing when there is
  nothing.*
- [ ] **M5b The summary and the raw record must agree.** They are
  two records of one moment by design (see M5), written by two
  writers. Nothing checks that the history's summary is what the
  store's events would produce, so a bug in either is invisible.
  *Verify: a played run's summary recomputed from its own events
  matches the line the history wrote.*
- [ ] **M7b A labelled set for the key estimate.** Six songs is a
  spot check, not a measurement. Without one there is no way to move
  `KEY_MIN_MARGIN` on evidence, or to tell whether a change to the
  chroma helped. *Verify: accuracy stated over a set somebody else
  could check.*
- [x] **M8 The developer view** *(v0.18.11)*. `I` in the browser opens
  a song's whole document: who it is and where each claim came from,
  what has been measured and by which analyser at which version, its
  chart per difficulty, and — once, at the end — which areas are
  still unknown. The substance is a pure function over the document
  (`beatbyte_library::report`), tested without a window; the screen
  turns sections into nodes and does nothing else, because a screen
  that decided what to show would be a second place for the rules.
  ⚠️ **An absent field produces no row.** Not "Unknown", not a dash:
  the document exists to keep missing and known apart, and a view
  that draws them the same undoes it. A section with nothing in it is
  not drawn either.
  Two things the picture found that no test would have: the kit's row
  spreads its children apart, so a row WITH a provenance note put its
  value in the middle while one without put it at the right edge —
  the same column reading as two columns down the page; and
  `enter_shot_state` needed `Option<Res<SongLibrary>>`, because a
  system's parameters are validated BEFORE its body runs and the
  early return never got the chance to save it.
  ⚠️ A flake caught in passing and fixed: one telemetry drill read
  the store back without releasing it first, so it passed alone and
  lost the race under a full workspace run — the writer commits on
  its own thread, `flush` only asks it to, and dropping the handle
  is what waits.
  *Verified: 1667 tests (+8), gate green, autopilot 504/504, and the
  screen photographed (`BEATBYTE_SHOT_STATE=songinfo`) rather than
  argued about — which is how both its layout defects were found.*
- [x] **M9 External enrichment, optional and encapsulated**
  *(v0.18.12)*. `beatbyte-cli library <root> --catalogue`, behind
  `--features catalogue`, in its own crate. The only part of the tool
  that talks to a network; off by default; a song is fully playable
  without it. One request every 1.1 s, a descriptive User-Agent, a
  503 waited out rather than believed, nothing asked twice, and a
  study twin never asked at all. Full write-up:
  [`docs/audio/catalogue.md`](audio/catalogue.md).
  ⚠️⚠️ **The catalogue's own score is worthless here, and that is
  measured.** Asked for "Heroes" it returns eight recordings **all
  scored 100**, from 0 to 393 seconds. Length decides instead, on the
  house rule the lyrics lookup already had — which is why that rule
  moved into the chart crate rather than being copied a third time.
  ⚠️ **And length alone is not enough either.** The first twelve
  answers for "Born to Run" are twelve live takes; the 4:31 is one of
  ten unlabelled recordings among 2338. A soft penalty still picked a
  live take that sat a second nearer, so the choice is made in TIERS:
  unlabelled first, labelled only when nothing else fits, and a
  fallback match's confidence is cut to 0.4 — below the bar for
  writing anything.
  ⚠️⚠️ **Only the identifier is written. Not the album, not the
  year.** The release list in a search answer is an arbitrary handful
  and is usually a sampler: *Dance DeLuxe* for "All That She Wants",
  *Kulthits* for "Don't Stop Believin'", a 2023 singles collection
  for "Life Is a Flower". Roughly three of eight were the real album.
  An identifier is a fact about which recording this is; an album
  from that list is a guess that would read as a fact.
  **Result: 86 songs asked, 67 matched, 16 too thin, 52 given an
  identifier**, each with a `catalogue` run in its analysis log.
  *Verified: 1683 tests (+16), gate green, against a recorded real
  response and a live run over the whole library.*

## Menu mouse completion (2026-09-23, v0.18.13)

Every menu already claimed mouse support; several screens still
navigated only by keyboard and gamepad, and leave was right-click
only with no visible Back. One pass made the leave rule shared
(`hover_moves_cursor`, `wants_leave`, `back_pressed`) and rolled it
onto Song Info, Players, Achievements, Stats, Join, Calibration,
Input Test, Results and Pause, plus clickable difficulty / INFO /
empty-library CTA in the song browser.

- [x] **Visible Back + full mouse on every menu** *(v0.18.13)*.
  *Verified: 1685 tests (+2), gate green.*

- [x] **M10 Mouse secondary actions** *(ActionBar pass, v0.18.15)*.
  Shared `ui_kit` chips for every footer accelerator (search, add,
  stats, filters, pause quit, …) so a mouse-only player reaches the
  same paths as the letter keys. Keys stay; typing stays keyboard;
  destructive delete arms Confirm/Cancel chips. Plan:
  `docs/ui/input-ux-plan.md` § Mouse secondary actions.
  *Verified: 1694 tests (+7), gate green.*

- [x] **Mouse wheel on every scroll panel** *(v0.18.16)*. Achievements
  (and any other `scroll_panel` screen) move the list cursor with
  the wheel the same way up/down do; a scan test refuses a scroll
  panel without a wheel reader.
  *Verified: 1695 tests (+1), gate green.*

- [ ] **M9b The album, properly.** The release list a search returns
  is not the recording's; getting the real first release needs a
  second request per match (`inc=releases+release-groups`) and a
  rule that prefers the earliest official album. Until then the
  descriptive section stays empty rather than wrong.
  *Verify: the album written for ten known songs is the album.*
- [ ] **M9c A setting for the catalogue in the game.** The lookup
  exists only in the offline tool today, which is why there is
  nothing to switch on yet. The moment the game can ask, it needs
  the switch the plan asked for — off by default, and off is off.

## The star-power impact (2026-09-20, v0.17.20)

A phrase every note of which was hit credits the Hype meter, and the
only sign of it was the meter itself moving in the corner. One
impulse, read by three systems: the neck takes the energy in, the
ceiling flashes, the screen catches it. `gameplay/starpower.rs` owns
the clock and the shapes and writes no pixels — the systems that
already own the neck, the lamps and the screen read it, because each
of those has exactly one writer and a second would fight it for the
same handle every frame.

- [x] **S1 The impulse** *(v0.17.20)*. The trigger is
  `SessionEvent::PhraseCompleted`, which already existed and is
  already exactly right: the session emits it when a phrase's last
  note is hit AND the phrase was never broken, in the same breath as
  the meter is credited. **No star-power rule is duplicated in a
  visual system** — this reads the event and knows none of them. The
  screen flash is shared code with the combo-break flash
  (`FlashProfile`), one pre-spawned quad, so the alpha cannot
  accumulate and nothing is allocated at the trigger. Under REDUCED
  FLASHING the screen flash is **absent** rather than dimmer — the
  promise this game already makes — the ceiling swells once instead
  of pulsing twice, and the neck's glow carries the moment.
  ⚠️ **The whitening had to obey the same rule as the glow.** This
  file's own recorded failure is a lifted fretboard becoming a lamp
  and the bloom pass washing the whole venue; the first version
  guarded the *glow* against it and then whitened every surface
  equally at 0.55 — and the emissive is computed FROM the base
  colour, so that would have raised what the bloom pass sees on the
  largest surface just as surely. Both terms are now weighted by each
  surface's own `glow_lift`: the rails and trim carry the flash, the
  board gets a floor.
  ⚠️ **A harness limit surfaced while trying to photograph it.**
  `BEATBYTE_SHOT_TIMES` claimed each moment for a whole second, so
  two moments a tenth apart collided and only the first was ever
  shot — which is exactly the spacing a 300 ms effect needs. An
  already-photographed moment now steps aside for the next one.
  *Verified: 1555 tests (+33), gate green.* Every new pin mutated
  once and seen to fail; three of the first attempts did **not**
  fail and were rewritten — a colour test that a white wash alone
  satisfied, a multiplayer test that called the resource directly
  instead of going through the bus that carries the player index,
  and a restart test that never exercised the entry half.
  *Measured in the running game* (`"Heroes"`, Medium, a full
  autopilot run end to end — **297 perfect, 0 miss, 0 overstrum,
  exit 0**, so judgment is untouched — plus a two-player run and a
  flat-view run, with no warning or error in any of them):
  - **Exactly one impulse per completed phrase**: the chart has 12
    phrases on Medium, all 12 were completed, and the log carries
    **12 firings**. Not one on a star note along the way, not two
    for one phrase.
  - **The trigger lands on the phrase's last note**: reported at
    19.97 / 37.04 / 54.11 s against the chart's 19.96 / 37.04 /
    54.10 — inside one frame, which is where the report is taken.
  - **The shapes reach their design values and leave nothing
    behind**: neck 0.95, ceiling 1.00, screen 0.27 of an intended
    0.28, over 26–27 frames ≈ 0.45 s, and a tail of **0.000 on every
    part of all twelve**.
  - **The earliest a note can follow a completed phrase is 100 ms** —
    measured over 26 512 phrase endings in 688 charts; the median is
    481 ms and nothing in the library is tighter than 100. The flash
    is above half its peak for the first ~85 ms, so its bright half
    is over before the earliest possible next note; in the tightest
    23 % of endings that note is judged under a veil of 0.14 and
    falling.
- [x] **S2 Looked at, and twice too bright** *(v0.17.21)*. §15's
  eyes-on pass, once the screen was unlocked. Both numbers that
  could only be reasoned about were wrong, and in a way no
  measurement without a picture would have caught.
  - **The screen flash, 0.28 → 0.12.** On screen 0.28 was a
    whiteout: mean frame luma **2.86×** the baseline, the score
    digits, the hit label, the crowd and the PA all behind a veil —
    and the neck's own lift invisible under it, which is the exact
    opposite of "the highway lights up". The frame that looked right
    was the one 0.12 s into that flash, at 1.90×, where the alpha is
    ~0.10 — the combo-break flash's long-settled value. **The number
    this game had already tuned was the right order all along**, and
    the reasoning that white needs less than red got the direction
    right and the amount 2.8× wrong.
  - **The ceiling, 2 400 000 → 1 000 000 per lamp.** Lowering the
    flash barely moved the peak (2.48× at +50 ms), which said the
    veil was mostly NOT the overlay: the impulse hits **all twelve
    heads at once** where the room strobe hits a shuffled pair, so
    2.4 M per lamp is 28.8 M against the strobe's 12 M. At 1.0 M the
    impulse's total equals the strobe's — still the loudest thing
    the room does, and every fixture going white in the same instant
    (which the strobe never does) is what makes it read as bigger.
  *Measured after* (fire at 19.96 s, frames by song time): −20 ms
  1.00× · **+20 ms 2.50×** · +30 ms 2.33× · +40 ms 2.20× · +50 ms
  1.67× · +60 ms 1.10× · +330 ms 0.96×. **Looked at:** at the peak
  the score reads, the hit label reads, the lyric line reads, the
  crowd and the PA are there, and the beams are visibly white; at
  +60 ms the neck's rails and hit line carry the afterglow; by
  +330 ms it is gone. One impact, readable throughout.
  ⚠️ The lesson is the one this file already carries in another
  form: **pixel metrics prove a change happened, only looking proves
  it is the right one.** Everything checkable without a screen —
  the trigger, the timings, the tails, the 100 ms note gap — was
  green on a version that was unusable.
  ⚠️ And a test that pinned the peak as a literal (`> 0.2`) went red
  on the first tuning step. A tuned number does not belong in an
  assertion: the test now derives its threshold from the miss
  profile, which is the property it actually means.

- [x] **S3 A sound for a banked phrase** *(v0.18.1)*. Three quick
  pulses up a major triad, 116 ms, in `sfx::charge()` beside the
  activation riser it is the sibling of — well under half its 280 ms
  and quieter, because banking happens four times for every activation
  and the loud sound should be the rarer one. One `SessionEvent`
  match arm, one synthesized clip, no new machinery, exactly as the
  module doc had said it would be. Rate-limited at 250 ms so two
  players banking in the same frame make one sound rather than one
  twice as loud. *Verified: the clip's rise and its place under the
  riser pinned by measurement (zero-crossing rate per pulse window,
  peak against the riser's), the limiter's decision pinned as a pure
  function.*
  *Verified: 1559 tests, gate green.*

## Gameplay telemetry (2026-09-20, v0.17.13–)

The blackbox: what the chart expected, what the player did, how the
engine read it, how it was judged — recorded as evidence rather than
as a log. Reasoning and alternatives: [ADR-0018](decisions/ADR-0018-gameplay-telemetry-store.md).
The feedback loop it feeds: [`adaptive-charting.md`](adaptive-charting.md).

- [x] **T11 One sink** *(v0.17.19)*. The JSONL writer is gone. The
  readers moved first and the writer followed — the order that cannot
  lose evidence — so the store is now the only thing the game writes
  telemetry to, and what the player says on the results screen goes
  there too. The old files stay on disk and `telemetry import` takes
  them in.
  ⚠️ **The blind-test drill could not reach a single song in a real
  library.** Every `[GS]` twin's title contains its original's and
  the twins come first, so a substring search always landed on the
  twin — which has one chart version and therefore no test to build.
  The only songs with several versions are exactly the ones that also
  have a twin, so the drill was unrunnable here and nobody had
  noticed. An exact title now wins over a substring.
  ⚠️⚠️ **And the fix immediately found a chart defect that the quirk
  had been hiding.** With the harness reaching originals for the
  first time, `Girls Just Want to Have Fun` on Medium — 690 notes,
  the original rather than its 375-note twin — produced **one
  overstrum**, at note 601, 373.1 s into the song. The store says
  that without anybody reading a log. The twin still passes (375
  perfect, 0 misses, 0 overstrums) on the same build, so judgment is
  untouched — and so, it turned out, is the chart: **C7** (v0.18.1)
  found the Hype activation window taking the note out from under the
  strum. What was true here is only that the harness had never played
  this chart before.
  *Verified by running both drills against the store:* the rating
  drill landed `fun 4` and a `better` verdict in 1 session; the blind
  test played `chart.v7` against `chart.v8` of *Life Is a Flower* and
  recorded "the second one" against the first side's hash. And the
  release binary wrote **no JSONL file at all** (474 before, 474
  after) — the retirement, checked rather than assumed.
  *Verified: 1522 tests (+2), gate green, four autopilot runs.*
- [x] **T10 The Definition of Done, measured** *(v0.17.18)*. The one
  item nothing else had answered: **does recording cost frames?**
  A/B on the release binary, same song, same machine, `BEATBYTE_FPS`:

  | | windows | median of medians | median p99 | p99 over 40 ms | worst p99 |
  |---|---:|---:|---:|---:|---:|
  | `DIAGNOSTIC` (the most talkative level) | 27 | **16.65 ms** | 21.75 ms | 5 | 100 ms |
  | `OFF` (no store opened at all) | 29 | **16.66 ms** | 22.87 ms | 7 | 125 ms |

  The run WITHOUT telemetry was very slightly worse on every measure,
  which is the honest reading: the difference is the machine, not the
  recording. Both runs PASSED (375 perfect, 0 misses, 0 overstrums).
  ⚠️ Vsync-capped at 60 Hz, so this answers "does it drop frames"
  and not "what does it cost" — the queue hand-off is a `try_send`
  and an atomic, and the cost of that is below what a frame timer can
  see.
- [x] **T6 What the song was doing there** *(v0.17.17)*. The gap
  that no amount of telemetry could close: `SongAnalysis` is computed
  at import and **never persisted**, so by the time anybody misses a
  note, the onset it came from, the energy around it and the bar it
  sat in are gone. A `*.context.json` sidecar beside each chart
  version keeps them — six bytes a note, indexed by **merged track
  event** (the thing the store records), and a separate file so
  `chart_hash` never moves and no session loses its evidence.
  Written by the generator and by `redesign`, backfilled by
  `beatbyte-cli context --all`, loaded into the store by
  `telemetry context`, and asked by `telemetry music`: *do the notes
  everybody misses have something in common?*
  ⚠️ **A backfilled sidecar is today's analysis, not the generator's.**
  The pipeline has moved since some of these charts were made (a
  meter model arrived, the grid changed). The numbers are still
  measured from the song rather than guessed, and they are still the
  right dimensions to group by — they are not a reconstruction of a
  historical run, and the document says so where a reader will see
  it.
  ⚠️ No section names. BeatByte has no structure segmentation, so the
  sidecar records which **repeated span** a note is in and nothing
  calls it a chorus. Writing a guess into a file is how a guess
  becomes a fact.
  ⚠️ **Importing every sidecar was wrong, and the number said so.**
  The first import put all 1 664 chart-version × difficulty
  combinations into the store — 970 904 note contexts — and turned a
  7 MB store into 45 MB for **exactly the same analysis**, because
  only 72 of them have a session to join to. The default is now what
  can be joined; `--all` takes the rest. 7.8 MB, identical answers.
  *Measured on the real library:* 171 songs, 416 sidecars, 18 MB, 0
  failed; 36 139 note contexts in the store; the §20 question
  answered across 18 919 judged notes. The first readings, as
  questions rather than conclusions: notes inside a **repeated span**
  are missed less (33.9 % against 42.1 %), and notes with **no onset
  under them** are missed LESS (34.7 %) than notes on a firm one
  (47.6 %) — the opposite of a reasonable guess, and exactly the kind
  of thing somebody now has to go and listen to.
  *Verified: 1520 tests (+17), gate green.*
- [x] **T8 + T9 The debug view, singing, and the benchmark**
  *(v0.17.16)*. The overlay says what the writer is doing and shouts
  a dropped event; vocal verdicts are recorded as cents and grades
  and **never as audio**; `telemetry bench` fills a throwaway store
  and times it.
  *Measured:* 1 000 sessions = 1.13 M events, 51 MB, 2.7 s (424 000
  events/s, 0.86 ms a batch); **10 000 sessions = 11.3 M events,
  513 MB, 29.7 s**. Reading one session stays at 0.24 ms at both
  sizes; the whole-corpus aggregates are 1.7–4.8 s at the larger one,
  which is what an offline command may cost.
  ⚠️ **An index was measured and refused.** A partial index on
  `(event_type, delta_us)` makes the calibration median eight times
  faster (221 → 28 ms) and costs **8.5 % of the whole store**. For a
  query that runs offline in under half a second, that is not a
  trade; the numbers are in the ADR so nobody has to re-derive them.
  **The overlay was photographed** once the screen came back:
  `TELEM level ACTIONS wrote 70 queue 0 dropped 0` and
  `STORE flush 716ms write 193us events 147703 size 7.8MB`, on a
  running song at 35.0 s. (It was written under a locked screen and
  verified at ECS level first — the rows pinned pure, the call site
  read back out of the entity in a headless app. That was the
  strongest evidence available at the time and a different thing
  from having looked; now both exist.)
  *Verified: 1503 tests (+11), gate green.*
- [x] **T5 + T7 The command line, and the history moves in**
  *(v0.17.15)*. `beatbyte-cli telemetry {status,import,list,show,
  export,problems,generators,calibration,input}`, and `review` and
  `dossier` moved onto the store behind one loader that decides where
  a session comes from. `show` keeps the thing a folder of JSONL gave
  for free and a database does not: a run a person can read.
  *Verified on the real corpus:* **471 files, 140 955 observations,
  about a second**, and a second run imports nothing. One session
  read back 504 hits and 166 misses from the store and from its own
  file. A review run both ways produced **byte-identical** output, so
  the port changed no answer. The store now holds 475 sessions and
  146 290 events in 7.1 MB — **51 bytes an event**, which is the
  number the whole design was sized on.
  ⚠️ First real reading, and it is the kind this exists for: over the
  medium corpus the store says **44.2 %** accuracy where the play log
  says 63.8 %. Not a defect in either — the log records accuracy only
  for runs that FINISHED, and a run is usually abandoned because it
  was going badly. Two populations, one of which nobody could see
  before.
  *Verified: 1492 tests (+5), gate green, review cross-checked both ways.*
- [x] **T4 The game records into it** *(v0.17.14)*. A session per
  player at the first frame that has one, every judgment and hold
  and hype window and pause and seek, and the **logical action
  stream** — the part that separates "the player missed" from "the
  input never arrived". A `TELEMETRY` row in SETTINGS chooses how
  much (`OFF` · `RESULTS` · `ACTIONS` · `DIAGNOSTIC`, default
  `ACTIONS`); `DIAGNOSTIC` adds which DEVICE spoke, never which key.
  ⚠️ **The first real run showed an overstrum with nowhere to be.**
  The session engine does not position one — it is a strum that
  matched nothing — so it was stored without a note and could not be
  placed in the song. It is anchored to the note it followed now, the
  way the JSONL layer learned to do it.
  ⚠️ The autopilot's injector **bypasses the input layer** (it owns
  the session while it plays), so it records its own actions; without
  that the action stream would be the one part of the blackbox no
  harness run exercises.
  *Verified on the machine, not estimated:* three autopilot runs
  wrote 4 sessions and 5 335 events — **51.4 bytes per event, 67 KB
  per session**, within a percent of the figure the design was sized
  on. `[GS] Girls Just Want to Have Fun` **PASSED** (375 perfect, 0
  misses, 0 overstrums), so judgment is untouched by the new input
  path. `[GS] Maria` reproduced its **known** three overstrums at
  notes 95, 183 and 312 — byte-identical to the four A/B runs
  recorded before any of this work, and the store now says where they
  are without anybody reading a log.
  *Verified: 1487 tests (+10), gate green, three autopilot runs.*
- [x] **T1–T3 The crate, the store and the writer** *(v0.17.13)*.
  `beatbyte-telemetry`: the vocabulary and its stable integer codes,
  the schema with a migration runner, the SQLite store, and the
  bounded queue plus worker thread the game will record through.
  Engine-free and Bevy-free, so all of it is testable with plain
  values and a database in memory.
  ⚠️ **Two real defects, both found by the tests rather than by
  reasoning.** The first draft put the lifecycle messages on the same
  bounded queue as the events and sent them blocking: a full queue
  then made `shutdown` — which `Drop` calls — wait for a reader that
  was never coming, and the whole test binary hung for ten minutes.
  Control and events now travel on two channels, each with one job.
  The second: nothing orders those two channels against each other,
  so an event can reach the worker **before** the message that opens
  its session — which is exactly what the game does, both within one
  frame. The first draft discarded those events, and every recorded
  run came back empty. A session that opens now adopts what was
  already waiting for it.
  *Verified: 1477 tests (+59), gate green.*

## How to work this roadmap

- [x] Guitar-feel technical pilot (0.15.6): audit import/redesign, research instrument
  transcription, and compare source selection and stable fret mapping on one
  existing song. [Research and single-song protocol](guitar-feel-pilot.md).
  Full local quality gate and release Expert autopilot passed: 512 perfect,
  0 misses, 0 overstrums. All 596 pre-existing library JSON files unchanged.
- [x] Guitar-feel quiet-part reduction (0.15.7): lower-level global strength
  floors discarded accepted notes in the 13.7-second Easy gap. Reduce the
  unchanged Hard part within four tracked beats for Medium/Easy. Pilot max
  Easy gap now 4.331s; default controls and Hard/Expert remain identical.
  Full quality gate passed; release Easy autopilot: 189 perfect, 0 misses,
  0 overstrums. Two new regression tests demonstrated failing before correction
  or under a fixed-tempo mutation. Existing library JSON files unchanged.
- [x] Guitar-feel Medium/Hard playtest: user played the Van Halen pilot on
  both levels and reported a better guitar feel on 2026-09-13
  ("mittel und schwer. ja, war besser"). This is a positive single-song
  result; the exact tested pilot version was not specified.
- [~] Guitar-feel second-song transfer: separate Nothing Else Matters pilot
  generated with unchanged 0.15.7 logic, active v5 grid and original audio.
  Autopilot plays it flawlessly on Medium (498/498) and Hard (679/679); the
  player's verdict on the feel is what is still missing.
  [Transfer measurements and source gap](guitar-feel-transfer.md).
- [ ] Guitar-feel remaining acceptance: playtest Easy/Expert, check the
  remaining source gaps and validate transfer to a second contrasting song.
  No bulk generation; the positive pilot verdict is not a library migration.
  Polyphonic guitar transcription remains a follow-up, not a capability of
  the current mono tracker.

- [x] Stage floor and room-motion pass (0.15.5): finer deck boards and softer
  lacquer, textured concrete around the deck, beat-clustered audience reactions
  with eased energy and secondary body motion, and grounded role-specific band
  weight shifts. Verified with focused regression tests and gameplay captures
  at 10.00, 10.15, 10.30, 20 and 30 seconds, all workspace gates, and a
  capture-free release autopilot run: 278 perfect, 0 misses, 0 overstrums.
  Steady gameplay held a 16.6 ms median frame time, with most five-second p99
  windows at 18–20 ms on this Mac. Scoring and timing code are unchanged.

- [x] In-song HUD polish (0.15.4): stronger score/multiplier/combo typography,
  readable song ribbon and clock, progress playhead, score impact and a short
  landing/lift envelope for judgment words. Reduced flashing removes HUD glow
  and judgment travel. Verified with unit tests, gameplay captures at 6.05,
  6.12, 7 and 10 seconds, workspace quality gates and a capture-free release
  autopilot run on "Lift Me Up" medium. No scoring or timing code changed.

- [x] Hit-flame refinement (0.15.3): curved shared mesh, shorter hot core,
  anchored feet and delayed ember visibility. Verified: workspace tests,
  fmt, all-feature clippy, check, rustdoc with/without all features;
  shape regression fails when the old core height is restored. Inspected
  gameplay captures at 6.1 and 7 seconds. Release autopilot on "Lift Me Up"
  medium: 278 perfect, 0 misses, 0 overstrums. Frame medians about 10 ms;
  after startup every five-second p99 below 15.1 ms on this Mac.
  Screenshot run had one overstrum; the separate capture-free release
  run passed. Four-player/low-end GPU performance remains unmeasured.

- Tasks are **small and independently verifiable**: each one names how
  to prove it done (*Verify:*). A task without a passed verification
  is not done.
- Tasks are **dependency-aware**: `dep:` names the task IDs that must
  be complete first. Never start a blocked task.
- Statuses: `[x]` done · `[ ]` open · `[~]` in progress (at most one
  `[~]` at a time).
- Completing a task = check it here, meet the Definition of Done in
  CLAUDE.md, commit. Scope changes are edited into this file, with a
  one-line rationale, in the same commit.
- IDs are stable (`M5.3`, `A2`, …) — never renumber, only append.

**Status: all 13 foundation milestones shipped (v0.8.1, 2026-08-23).
Current phase: Road to 1.0.**

---

## Phase 1 — Foundation milestones (SHIPPED)

Kept as the dependency record and the map of where things live.

### M1 — Foundation (v0.0.1)

- [x] M1.1 Cargo workspace: `beatbyte-{core,chart,audio,editor,cli,game}` + `apps/beatbyte` launcher. *Verify: `cargo check --workspace`.*
- [x] M1.2 Lints as policy: clippy `-D warnings`, `unsafe_code`/`missing_docs` warn, unwrap/dbg/todo denied in non-test code. *Verify: gate passes.*
- [x] M1.3 CI (fmt/clippy/test/check on latest stable) + repo scaffolding (LICENSE MIT, AUTHORS, CONTRIBUTING, SECURITY, CoC, CHANGELOG). *Verify: CI green on main.*
- [x] M1.4 Press Start 2P bundled with OFL license; `docs/asset-licenses.md`. *Verify: license file next to font.*

### M2 — Core domain (v0.0.2) — dep: M1

- [x] M2.1 `Lane`/`LaneSet` bitmask (incl. `highest()` for anchoring), `Difficulty`. *Verify: unit tests.*
- [x] M2.2 `TempoMap`, `TimingWindows` (30/60/100 ms), `Judgment`. *Verify: unit tests.*
- [x] M2.3 `NoteEvent`/`Phrase`/`Track` invariants (sorted, no simultaneous same-lane). *Verify: constructor tests.*
- [x] M2.4 Scoring: 50/35/20 pts, streak→multiplier (10/level, max 4), Hype (25 %/phrase, 0.5 activation, 32-beat drain). *Verify: score tests.*
- [x] M2.5 `TrackSession` judgment engine: input-stamp-driven hits, misses, overstrums, HOPO/pull-off, per-beat sustains. *Verify: 58-test suite.*

### M3 — Audio engine (v0.0.3) — dep: M1

- [x] M3.1 Decode + resample (rodio decoder, 31-tap FIR half-band), WAV in/out helpers. *Verify: round-trip tests.*
- [x] M3.2 `SongClock`: anchored monotonic time, snap ≥30 ms / slew 10 % vs device position. *Verify: clock tests.*
- [x] M3.3 Music on a dedicated thread behind Send `MusicHandle` (mpsc + atomics): play/pause/seek/volume/position. *Verify: playback tests + game integration.*
- [x] M3.4 Synth: click track + burst synthesis (15 % release ramp — hard truncation is an audible/detectable click). *Verify: onset tests.*
- [x] M3.5 Build-time synthesized demo song (128 BPM, no copyrighted
  audio). *Verify: `beatbyte-cli demo`.* **Superseded 2026-09-05:**
  the synthesized songs no longer ship in the game (they are
  instrumentals that were showing karaoke lyrics — user's call).
  The synthesis stays as the analysis/charting test fixture and
  behind `beatbyte-cli demo`. ADR-0006, amendment.

### M4 — Analysis & chart generation (v0.0.3) — dep: M3

- [x] M4.1 STFT spectral flux onsets, adaptive median threshold, brightness per onset. *Verify: synthetic-fixture tests; frame_offset_s convention documented.*
- [x] M4.2 Tempo: autocorrelation + log-normal prior + parabolic interpolation; beat-grid phase fit. *Verify: demo detects 128.3 BPM.*
- [x] M4.3 Deterministic generation via `DifficultyProfile`: grid quantization (raw-onset fallback), brightness lane walk, chords/HOPOs/sustains, splitmix variety. *Verify: 103/117/249/491 notes across difficulties, stable across runs.*
- [x] M4.4 Chart format v1 + validation: BPM 20–400, 32 MB cap, path traversal + Windows-drive-`:` rejection. *Verify: adversarial tests (incl. `C:\music.ogg` on Unix).*
- [x] M4.5 CLI: analyze/generate/validate/inspect/demo, exit codes 0/1/2. *Verify: end-to-end CLI run.*

### M5 — First playable (v0.1.0) — dep: M2, M3, M4

- [x] M5.1 App states (Boot/MainMenu/SongSelect/Gameplay/Results + GamePhase substate). *Verify: smoke test.*
- [x] M5.2 Highway rendering: lanes, receptors, scrolling notes, sustains. *Verify: screenshot.*
- [x] M5.3 Input → stamped judgment wiring; SessionFeedback message bus. *Verify: autopilot flawless run (117/117).*
- [x] M5.4 Autopilot harness (`BEATBYTE_AUTOPILOT=1`): perfect play through real screens, non-zero exit on any miss/overstrum, `.before(advance_sessions)`. *Verify: exit 0; deliberate offset ⇒ exit non-zero.*
- [x] M5.5 Smoke harness (`BEATBYTE_SMOKE_TEST=1`). *Verify: exit 0.*

### M6 — Game feel (v0.2.0) — dep: M5

- [x] M6.1 Judgment popups, hit particles, receptor flash, combo/multiplier HUD. *Verify: screenshots + autopilot.*
- [x] M6.2 Hype meter + activation flow; `active_sustain()` accessor. *Verify: tests + visual.*
- [x] M6.3 SFX (synthesized, no copyrighted audio). *Verify: audible + no asset-license change.*

### M7 — UI (v0.3.0) — dep: M5

- [x] M7.1 Pixel-font UI kit (`UiFont`, loaded at plugin build — OnEnter can run before startup flushes). *Verify: regression noted; menus render.*
- [x] M7.2 Main menu, song browser (library scan + user songs dir), settings, results (grade + count-up). *Verify: screenshots.*
- [x] M7.3 Latency calibration screen (120 BPM clicks, median of 8+ taps → settings). *Verify: manual calibration run.*
- [x] M7.4 Settings persistence (`Settings` + sanitize + ConfigPlugin). *Verify: restart retains values.*
- [x] M7.5 First published release with CI artifacts. *Verify: v0.3.0 prerelease, artifact smoke-tested.*

### M8 — Controllers (v0.4.0) — dep: M7

- [x] M8.1 Input abstraction: `GameAction`/`Binding`/`InputMap` persisted in Settings (bevy `serialize` feature for KeyCode/GamepadButton). *Verify: remap survives restart.*
- [x] M8.2 Gamepad play (South/East/North/West/LeftTrigger defaults) + `MenuNav` merged keyboard/pad menu input. *Verify: manual pad run.*
- [x] M8.3 Controls remap UI. *Verify: remap flow end-to-end.*

### M9 — Local multiplayer (v0.5.0) — dep: M8

- [x] M9.1 Players as entities (`PlayerSession`/`PlayerIndex`/`PlayerDevice`), input routed by `DeviceId`. *Verify: tests + 2P autopilot.*
- [x] M9.2 `HighwayLayout` for 1–4 highways; `PlayerRoster`, MultiplayerSetup screen, per-player colors. *Verify: 2P/4P screenshots.*
- [x] M9.3 Multi results: ranked + band totals. *Verify: 2P autopilot to results.*

### M10 — Themes (v0.6.0) — dep: M6

- [x] M10.1 Six data-driven themes + procedural backdrop animation (ADR-0008); theme in settings. *Verify: solo + 4P autopilot across themes; screenshots.*

### M11 — Chart editor (v0.7.0) — dep: M4, M7

- [x] M11.1 Invertible `EditOp` (Add/Remove/ToggleHopo/SetLen; apply returns inverse; strict misses = errors). *Verify: inverse round-trip tests.*
- [x] M11.2 `EditorSession`: undo/redo/dirty/validity. *Verify: tests.*
- [x] M11.3 Editor UI (open from song select, grid division stepping, save through chart validation). *Verify: editor autopilot cycle (add/undo/redo/save).*

### M12 — Packaging (v0.7.0) — dep: M7

- [x] M12.1 Hand-rolled PNG icon generator (python stdlib, no deps). *Verify: 1024×1024 PNG parses.*
- [x] M12.2 macOS `.app` + DMG (`packaging/macos.sh`: iconutil, ad-hoc codesign, hdiutil). *Verify: mounted .app smoke from neutral CWD.*
- [x] M12.3 AppImage (`packaging/appimage.sh`). *Verify: CI artifact exists.*
- [x] M12.4 Release workflow: 4 targets, DMG/AppImage, draft release. *Verify: green run + published release.*
- [x] M12.5 Explicit asset-root resolution for bundle layout. *Verify: superseded by M13.4 (full fix).*

### M13 — Polish (v0.8.0, patched v0.8.1) — dep: M10, M11, M12

- [x] M13.1 Screen-transition fades (0.25 s). *Verify: visual; screenshot settle delay added.*
- [x] M13.2 Count-in: 2 s pre-roll, 3-2-1 banner, music starts at exactly zero. *Verify: identical score with/without pre-roll (23399) proves judgment unshifted.*
- [x] M13.3 Docs/README brought to finished-milestone state. *Verify: review.*
- [x] M13.4 **v0.8.1**: asset root resolved across all launch layouts (portable/bundle/CWD/target ancestors) — a failed font never retries and had made all text invisible. *Verify: font `Loaded`, glyph counts > 0, flawless artifact autopilot from neutral CWD, fresh screenshots.*
- [x] M13.5 Release engineering fixes: macOS runner disk reclaim before hdiutil; recursive publish glob; `dist/` untracked + ignored. *Verify: green release run, all 7 assets attached.*

---

## Phase 2 — Road to 1.0 (OPEN)

Exit criterion for 0.x (from CLAUDE.md): **the gameplay tuning
settles** — real hands played it, the numbers stopped moving, and the
chart format v1 can be frozen as a promise.

### A — Feel & tuning

- [~] A1 **Human playtest protocol + first pass.** Protocol written (`docs/playtest.md`: session script over both songs × four difficulties, feel questions mapped to A2/A3/A4, findings-log format). **Blocked on human hands for the first pass** — the harness proves correctness, not feel. *Verify: findings recorded in `docs/playtest.md` with date + build.*
- [ ] A2 **Tuning adjustments from findings.** Apply window/scoring/HOPO changes A1 demands, or record explicitly that none were needed. dep: A1. *Verify: updated tests pin new values; autopilot flawless; CHANGELOG entry.*
- [ ] A3 **Calibration validated against real latency.** Play through a deliberately delayed audio path (e.g. Bluetooth) and confirm the calibrated offset lands within one timing window of measured latency. *Verify: measured vs calibrated numbers noted in `docs/playtest.md`.*
- [ ] A4 **Difficulty curve review.** Check Easy→Expert of both built-in charts reads as a curve (density, chords, HOPO runs); adjust `DifficultyProfile` if not. Known input RESOLVED early (2026-08-25, user playtest report: live track got no sustains): sustain generation is energy-first now — gap only bounds length, an ABSOLUTE strong-onset bar cuts the hold (a relative bar cut everything in live reverb and cost Rick's medium chart 53→37 before the fix). Measured: live track medium 3→51, Rick 53→92, Solder Groove hard/expert 1/2→14/12; both real tracks replayed flawlessly. Remaining for A4: the density/chord curve itself. dep: A1. *Verify: note-count/density table in findings; generation tests updated.*

### B — Content & chart workflow

- [x] B1 **Second synthesized demo song.** "Solder Groove", 92 BPM half-time groove (Dm–Bb–F–C, syncopated bass, held pad bars → real sustains). Multi-builtin refactor: `SongSource::Builtin(usize)` + `BuiltinSongs`; CLI `demo` writes both. *Verified: analyzes to 92.06 BPM (a tresillo bass first read as ~122 — reshaped to keep the quarter pulse dominant); autopilot flawless 1P (89/89) + 2P; Circuit Breaker unaffected; 158 tests.*
- [x] B2 **Autopilot over any library song.** `BEATBYTE_AUTOPILOT_SONG=<index|title-substring>` (bad selector = loud failure). *Verified: 4 unit tests (one mutation-checked); live run with `=circuit` flawless, `=nonexistent` exits 1. Second-song run covered under B1.*
- [x] B3 **Import walkthrough verified end-to-end.** No guide existed — written as `docs/importing-songs.md` with every command executed for real (an MP3 through analyze → generate → validate → library → play → editor). **Two real defects found:** the library scan missed the documented `songs/imported/<song>/` layout (one level too shallow — fixed with a bounded symlink-safe walk + unit test), and the CLI help still claimed ogg/wav/flac/mp3 only. *Verified: autopilot flawless on the imported song (87/87), editor cycle PASSED on it.*
- [x] B4 **Supported-audio-formats truth.** 5 decode tests against committed synthesized fixtures (~5-22 KB each, recipe in `tests/fixtures/README.md`); table in chart-format spec. Finding: **M4A/AAC decodes** — the "not supported" pin failed immediately, README undersold the decoder; docs updated, pin flipped positive. The ogg fixture is stereo on purpose (pins the downmix). *Verified: 5/5 passing.*

### C — Robustness & platform coverage

- [x] C1 **Linux smoke in CI.** `linux-smoke` job: the real game boots to the menu under Xvfb + mesa software Vulkan (lavapipe) on a GPU-less, audio-less runner — also validating the music thread's graceful no-device degradation. Two iterations to green: `libxkbcommon-x11-0` missing at runtime (winit dlopens it). *Verified: job green in 16m28s on main.*
- [x] C2 **Windows artifact validated.** `windows-smoke` CI job boots the real game to the menu on a GPU-less Windows runner (DX12 WARP software rasterizer), no audio device — the same code path the zip artifact ships. *Verified: job green (40m54s cold; caching shrinks it). Real-hardware run of a release zip remains a nice-to-have under F2.*
- [x] C3 **CI actions off deprecated Node runtimes.** Two rounds: v5 of upload-artifact turned out to STILL be Node 20 (release-run annotation) — the reals, verified against each action's own `action.yml`: checkout v5, upload-artifact v7, download-artifact v8 (keeps `merge-multiple`), action-gh-release v2→v3. *Verified: v0.9.0 release run clean except gh-release (bumped after; confirms next release).*
- [x] C6 **The 2-player autopilot flake: named, and it was C7's rule** *(v0.18.1)*. About one run in five reported a single overstrum that broke the streak mid-song, and it is the release gate, so every flake cost a rerun. The cause is the Hype-window rule fixed in C7 — and it explains the part that made C6 look like a separate, spooky problem: **whether it fires is a race with the frame boundary.** The injector checks that a note is still pending and then strums; the window takes the note during the session's own advance. Which happens first depends on where the frame falls, so a note comfortably inside the window fails every time (Maria) while one near the edge fails sometimes (whatever C6 was watching). With the absorb marker armed, both orders are safe: strum first and it hits, window first and the strum is absorbed. *Verified: 20 two-player runs on the chart that exercises the rule — `[GS] Maria`, nine activations per player, three of them a guaranteed overstrum before the fix. **18 produced a verdict and all 18 are clean**: both players 428/428, streak whole, zero overstrums. The other two ended 11 s in with `Skipped event Destroyed … Window` — the window was closed from outside, which is the documented vanished-window mode and not a judgment at all.* ⚠️ **The first attempt at this evidence was worthless and I ran 5 of 40 before measuring it**: the short song I picked to get more runs per hour activates Hype **zero** times, so it exercised none of the mechanism. A batch is only evidence once you have checked that it can fail.
- [x] C7 **Two charts failed the autopilot — and both were innocent** *(v0.18.1)*. `[GS] Maria` with three stray strums at notes 95, 183 and 312, `Girls Just Want to Have Fun` on Medium with one at note 601. Recorded as chart defects for five weeks because that is what they looked like. **The cause is an engine rule, and it costs real players their streak.** A successful Hype activation opens a half-second window that hits the notes inside it *for* the player; the strum they were already making then lands on a note that is no longer there and is counted as a stray one — streak broken, multiplier reset, and the sustain of that very note cut off. The game already forgives this for a hammer-on (the pick lands after the fret change); `auto_hit_hype_notes` now arms the same absorb marker, so one strum inside that note's window is that note's strum. ⚠️ **It hid wherever the next note needed a FRET CHANGE**: the press hits that note itself and the strum is then never sent, which is why it surfaced only on two charts rather than as a rule. *Verified: both charts play clean with nothing changed in them — Maria 428/428 with the streak whole, its score 15 % higher for identical playing (106,466 → 122,750) and three cut sustains recovered (177/181 → 180/181); `Girls` 690/690. Read off the telemetry store rather than guessed: every one of the three overstrums sits one note after a `hype_activated`, and the one activation whose next note needed a fret change produced none.*
- [~] C4 **Gamepad hot-plug** *(v0.13.39, `gameplay/hotplug.rs`)*. Disconnect → pause + the pause screen names the waiting player (`PadLost` on the seat); connect → the longest-waiting seat takes the pad, same `PlayerDevice` slot and session, resume stays manual. Pure policy `react` (event × seats → actions) + headless wired test (real `GamePhase` states, real messages, the pause line). *Verified: 6 tests, 4 mutation probes (no pause, newest waiter, taken pad re-handed, rebind no-op) each caught; autopilot 1P + 2P PASSED.* **Remaining: the manual unplug 1P and 2P on real hardware — human hands; the driver-level event is the only layer not exercised.*
- [x] C5 **Settings/chart forward-compat reads.** Both were already tolerant (`#[serde(default)]` on Settings, selective defaults on chart schema; corrupt settings fall back to defaults with a warning) — now PINNED: settings load with unknown+missing fields, malformed JSON errors cleanly, charts with unknown fields at file/song/chart/note level parse and stay valid. *Verified: 3 tests; deny_unknown_fields mutation makes them fail.*

- [x] C5 **Score keys collide on a pipe character** *(v0.13.38)*. Key is a struct (`RecordKey`), the file is version 2 (a record list, stable order), and a legacy map migrates on load — the difficulty is the piece after the last pipe, the title ends at the first (the old lookup's own reading of the one ambiguous shape), malformed keys are dropped with a warning, and the legacy file is kept once as `scores.v1.bak.json`. The limitation pin flipped to asserting `None`. *Verified: 11 scoreboard tests (migration, round trip with pipes, stable bytes, garbage is an error), three mutation probes; the real 24-record file migrated through a real run — 24 in, 24 out, none changed — and a boot on the restored legacy file wrote the backup and left the file alone.*

### D — Accessibility & options

- [x] D1 **Colorblind-safe lane identity.** Always-on per-lane shapes (square/circle/diamond/triangle/X) as generated 16×16 pixel masks (`shapes.rs`, pure + 3 geometry tests) on gems AND receptors (shaped rings). Bonus catch: HOPOs had NO visual distinction — now smaller gem + bright core. *Verified: flawless autopilot; screenshot; grayscale (total color removal) keeps all five receptors trivially distinct; README media updated.*
- [x] D2 **Reduced-motion option.** New `backdrop_motion` setting ("STAGE MOTION" row) wired Settings → EffectSettings → `animate_backdrop` early-out; particles/shake/beat-pulse toggles already existed. *Verified by transform probe: ON = bit drifts continuously, OFF = position bit-identical over 25 s; both runs flawless. Side catch: the verification exposed that a window-closed autopilot run faked exit 0 — harness hardened (DontExit + fail-on-vanished-window).*
- [x] D3 **Audio mix options.** Already fully implemented (M7-era): `music_volume`/`sfx_volume` settings rows, persisted, music applied centrally in `apply_settings` on any change, SFX per-play. Closed by code-trace audit during C5/D2 work; no change needed.
- [x] D4 **Scroll-speed option.** Global setting existed (240–900 px/s row, persisted); scroll speed touches only note Y/tail height, never judgment. *Verified: identical flawless score (23399) at 240 solo and 900 2P.* Scope amended: per-player speed deferred until a playtest (A1) actually asks for it — speculative multiplayer-UI complexity otherwise.

### E — Editor v2 (quality-of-life)

- [x] E1 **Move/nudge op.** `EditOp::MoveNote` (keeps len/hopo, sorted invariant, Occupied unless the destination is the note itself) + M grab/place UI with ESC cancel. *Verified: 4 op tests incl. exact-chart inverse round-trip; editor autopilot extended with move+undo+redo position checks — which exposed that the harness had been SAVING its probe note into the chart (now sweeps its slots first; passes twice in a row).*
- [x] E2 **Selection + bulk ops.** `EditorSession::edit_batch` (grouped undo stacks, atomic — a failing op rolls back the whole batch) + V/X/H-selection UI, ESC clears. *Verified: 2 session tests (one-step undo, atomicity incl. no-residue on failure, empty batch no-op); editor autopilot extended (batch add ×2 = exactly one undo step, both notes appear and vanish together); cycle PASSED.*
- [x] E3 **Audition playback in editor.** Play-from-cursor already existed (M11); added the beat-click overlay (`preview_clicks` + synthesized tick in SfxLib) and an audition phase in the editor harness that counts real ticks via `AuditionClicks`. *Verified: 7 clicks in ~4 s at 92 BPM (exactly one per beat); mutation (counter disabled) fails the harness; gameplay autopilot still flawless.*

### G — User-requested (2026-08-25, from first hands-on session)

- [x] G1 **Tap mode (keyboard-friendly, no strum).** Core: `tap_mode` generalizes the HOPO rule to every note; settings row; tap runs skip the scoreboard. *Verified: 4 core tests; key-harness proof both directions (tap+no-strum flawless, no-tap+no-strum = all misses).*
- [x] G2 **Real-keyboard harness.** `BEATBYTE_AUTOPILOT_KEYS` presses actual KeyCodes (press-lead, re-tap on held keys in tap mode, missed events skip without phantom strums). Found: the first registration silently no-opped (replace-anchor drift) — every earlier "keyboard proof" had been the direct feed; asserts on anchors now. *Verified: classic PASS / tap PASS / neither FAIL.*
- [x] G3 **Drag-and-drop song import.** `import.rs`: FileDragAndDrop → copy to `songs/imported/` → async analyze+chart → library rescan; browser rebuilds on the changed resource and shows the status; name heuristic strips download noise (4 unit tests). *Verified end to end: injected drop of "Drop Test - Import Works (Official Audio) [abc123].wav" → title "Import Works" by "Drop Test" → chart on disk → autopilot waits for the import and plays it flawlessly.*
- [x] G4 **Sustain-note animation.** `animate_sustains` after `move_notes`: gem pinned + white-pulsing, tail consumed bottom-up with glow, spent tails dim; hold sparks existed (fx). *Verified by probe: head constant at RECEPTOR_Y while tails shrink monotonically (1026→914→809…), run flawless.*

- [x] G5 **Delete library entries in-game.** Backspace/Del with a 3-s double-press confirm; imported folders removed whole, hand-managed charts chart-only, built-ins refused. *Verified: 3 fs tests + real-key E2E (arrow-navigate, double Backspace, folder physically gone).*

- [x] G7 **Round style = whole-game look; tap mode default.** Playtest findings (first real hands-on): smooth font (UiFont style switch), scrolling fret lines from the chart's tempo, soft-disc particles + backdrop dots; tap default ON (scoreboard gate removed), autopilot direct feed made tap-aware (strum only while pending — it double-hit into 107 overstrums otherwise). *Verified: real-key no-strum run flawless on a FRESH profile (proves the default); round + 8-bit direct-feed runs flawless; screenshots.*
- [x] G6 **Note Style: round gems (8-bit off).** `round_gems` setting + "NOTE STYLE" row; round = disc/white-core/dark-ring (strum) vs no ring (HOPO), 128-px AA textures with linear sampling. D1 amendment: colorblind shapes go from "always on" to DEFAULT on — an explicit player choice may prefer color-only. *Verified: 7 mask/coverage tests; screenshots of both styles; flawless autopilot in both.*

- [x] G8 **Multi-track drag-and-drop + import feedback.** ImportQueue (batch counters, skip reasons counted, per-song library rescan) + animated overlay panel (pulse/easing bar/flash/summary, screen-independent). Root cause of "songs seemed lost": the old code imported one file per gesture and SILENTLY discarded the rest — plus the user's own drop had landed in the user songs dir (app ran as a bundle), which then double-listed against the repo copy → scan-level title dedupe. *Verified: 4-file drop E2E (3 real m4a + 1 txt → 3 imported, 1 skipped), The Passenger 568/568 and Two of Hearts 956/956 flawless, panel screenshot mid-batch (3/4, bar at a third).*

- [x] G9 **AAA polish for the round style.** HDR Bloom component synced to the style at runtime; shading-function texture suite (lit sphere, gloss, gaussian dot, tube, glow strip, bed gradient — 5 geometry/lighting tests); emissive gem tint feeds the bloom. Honest scope note: this is 2D done well, not photorealism — a 3D/PBR look would be a renderer rewrite. *Verified: flawless runs both styles; screenshots; 8-bit unchanged.*

- [x] G11 **Depth-view geometry fix + stage polish.** User screenshot: notes ran beside the lines — the guides used a DIFFERENT straight line than the note path; now they are its exact extension (`depth::extend_below`, collinearity-pinned). Polish: flattened receptor rings, glowing hit line, per-gem halos, corner vignette, distance-faded fret lines. *Verified: identical score again (23640), screenshot shows notes ON strings.*
- [x] G10 **Depth view (vanishing-point highway).** `depth::project` (pure, 3 tests: identity at the hit line, monotonic climb/shrink, lane convergence), per-frame projected notes/fret lines/sustains, leaning lane guides, trapezoid bed as a real 2D mesh. Both views coexist as a settings row. *Verified: IDENTICAL autopilot score flat vs depth (23640) — presentation only; screenshots; 8-bit untouched.*

- [x] G12 **Window-independent scaling.** Ortho projection `AutoMin{1280,720}` + `UiScale` synced to window height (0.6..2.5) + `BEATBYTE_WINDOW=WxH` for pinned sizes. *Verified: flawless runs with full stage/HUD at 800x500 and 1920x1200 (screenshots).*

- [x] G13 **Guitar Hero X-plorer support + controller tester.** gilrs cannot see the guitar (Xbox-360 vendor protocol, no macOS driver — verified empirically); own libusb reader thread (rusb vendored) decodes the documented 20-byte reports and injects RawGamepad events, making it a first-class Bevy gamepad matching the default bindings exactly. Controls screen: connected-device line + five live fret lamps through the real InputMap. *Verified: real guitar detected and claimed; USER pressed frets and the lamps lit (screenshot: red+yellow held, both lamps on — "perfekt!!! das funktioniert schon mal!!!"); 2 decode tests. Still open informally: a full song played on the guitar + C4's mid-song unplug.*

- [x] G14 **Strum UX after the guitar session.** Field finds: solo routed ONLY the keyboard (guitar played into the void — receptors dark, no hits) → solo hears all devices; Space strums on keyboard (Hype → Enter, user map migrated); a rate-limited "STRUM!" coach appears when a held-fret note dies with tap off. Key-injector made tap-aware (never strums when tap hits already). *Verified: tap-off + Space-strum flawless twice via real keys; tap-off no-strum = 117 misses; tap-on regression flawless.*
- [x] G15 **Mouse-driven menus + honest tagline.** User: no INPUT TEST entry visible (root cause: `open` foregrounds the stale running instance — killed + relaunched), mouse control requested, "8bit game" subtitle outdated. Menus now fully mouse-operable (hover select, click activate/adjust, wheel scroll/step, right-click back everywhere incl. key-capture cancel); tagline "five lanes. your music."; README rewritten for both looks. *Verified: gate + interactive build.*
- [x] G16 **Depth-view sustain fix + docs sprint.** User screenshot: sustain tails stood vertical while the lane leaned (child sprite kept flat geometry) — tails now connect gem to the projected far-end point on the exact note path (`depth::point` + `align_tail`, collinearity-pinned). Docs: README rebuilt (60+ badges, guitar support matrix, import pipeline walkthrough, donate/review), ~17 new unit tests incl. real-file decode fixtures for m4a/mp3/ogg/flac. *Verified: gate + depth-view screenshots + mutation checks on new pins.*
- [x] G17 **Live mute toggle.** User: "bei testläufen toggle button für (un)mute" — `Muted` resource + always-present clickable corner badge + `M` key (editor excluded: metronome owns M); music thread scaled, SFX via `GlobalVolume`; env var only seeds the start state, every later volume write goes through `factor()`. *Verified: gate, badge screenshot, muted autopilot still silent.*
- [x] G18 **Guitar-Hero-style conversion (melody charting + master derivation).** User: m4a conversion must capture guitar tones + held notes perfectly, research GH and adapt. Research: official charts are HAND-authored against stems; adaptable = the conventions (contour lanes, true-length sustains with tempo-scaled trailing gaps [CSC: 1/32|1/24|1/16 whole note], lead owns the highway). Implementation: melody stage in beatbyte-audio (STFT 2048 → HPSS → register-weighted semitone salience → DP contour → segmentation with sustained-ratio + loneliness guards), `SongAnalysis.melody` (additive, serde-default), generator rebuilt master-first (one 5-lane master, difficulties = thinned/remapped derivations, subsets pinned). *Verified: 5 melody unit tests incl. clicks-vs-tone separation; real track: coverage 22.5%→85.7%, held notes 8→147, vocal register tracked instead of bass; hard/expert sustains 2→34/24; all imported charts regenerated; autopilot flawless.* Follow-up: per-difficulty tuning session by ear (medium first — user decision), then derive tweaks.
- [x] G19 **Difficulty curve + import-path proof.** Measured the G18 curve on all five real imports and found a genuine defect: absolute strength floors made the easy→medium jump song-dependent (1.4x…3.6x; easy 0.42…1.40 notes/s). Fixed by density targeting (notes-per-beat targets, rank-based selection — a threshold bisection has a pathological case where NO threshold hits the target and it empties the chart) plus a reduction CHAIN (expert→hard→medium→easy), which makes nesting structural. *Verified: curve now exactly 2.0x easy→medium on every track, easy 0.66–0.79 n/s; nesting pinned with a fixture proven to break one-shot thinning; **real in-app drag-and-drop import (autopilot DROP mode) produces bit-identical charts to the CLI path** — user's question, and the right check: the CLI does not exercise import.rs.*
- [x] G20 **Chart-feel regression caught and reverted; stronger fret feedback.** The transcription rework (eval harness + tempo/pitch/onset/segmentation) measured better on eight synthetic scenes and played WORSE — user: "das ging gerade komplett in die falsche richtung!! ich fühle es nicht". Rolled his charts back ("jetzt macht das spiel wieder spaß und sinn!! mega!!!"), preserved that state three ways (tag `chart-feel-good-20260826`, `~/backups/beatbyte-charts-good-20260826/`, memory), and reverted main so NEW imports get it too; the work is parked on branch `transcription-v2`. Root cause measured afterwards: candidate ranking by loudness alone moves 59 % of note positions and ignores the beat structure a listener feels. Then: fret buttons swell/brighten/halo on press and punch on a hit, judgment-scaled. *Verified: freshly generated chart byte-identical to the one he enjoyed; autopilot flawless (624 perfect); screenshot shows the hit punch.*
- [x] G22 **One design across all menus.** User: unify the menus and settings and make them look modern. Audit found the screens shared only a font: seven title-to-content gaps (14–26 px), six row sizes (11/12/13/14 px) for the same job, 9 px *and* 10 px for hint text, selection signalled by letter colour alone, no frame anywhere, and hint lines in four separator styles. Two functional defects fell out of it: the controls screen read arrow keys directly (unreachable with a guitar) and ignored the mouse entirely, and the settings label column was padded to 16 characters while "TAP MODE (NO STRUM)" is 19, so that row's value hung outside the column. New `ui_kit` owns the scale, the spacing and the row states (Idle/Selected/Armed); all seven screens plus the pause overlay draw from it; two hard-coded copies of the lane palette removed. *Verified: gate (249 tests, +9); smoke + autopilot bundled/real-track/2-player/editor all PASS; new `BEATBYTE_SHOT_STATE` photographed all seven screens, and the measured selection fill (sRGB 99,84,35 → 69,58,32) fixed a tint that Bevy's linear-space alpha had made far heavier than intended.*
- [x] G23 **A venue behind the stage, and readouts in the corners.** User showed a Guitar Hero screenshot: "ich möchte dass hintergrund und zähler zu sehen sind wie im screenshot. gehe sorgsam vor." Two separate problems. (a) The 3D fretboard ran through a void, and the only thing outside it was the 2D sprite backdrop — which the stage camera renders at order −1, so those sprites were drawn IN FRONT of the board: confetti, not a backdrop. They are now skipped in 3D, and the stage builds a room instead (rear wall, side walls, truss with sweeping beams, speaker stacks, crowd ranks behind barriers), themed and kept outside the bed so it can never occlude a note. (b) The HUD was world-space text above the highway, which in 3D means above the vanishing point — the numbers floated in the middle of the screen. Solo now uses framed corner plates (score/multiplier/combo left, hype meter right); multiplayer keeps per-highway blocks, because four necks leave no free corners. Three self-corrections along the way, each caught by looking at the result: the first venue had cones washing red haze across the fretboard and speaker stacks looming at eye level; the rear wall was lit to fretboard brightness and read as a blank screen; the crowd was loose spheres that read as rubble until they were given barriers to stand behind. *Verified: gate + 271 tests + rustdoc clean; autopilot PASSED solo (624/624), in the depth view, and 2-player; frame time median 16.6 ms / 99th 19 ms (vsync-bound, venue costs nothing measurable); depth view and multiplayer screenshotted to confirm neither regressed.*

- [x] G24 **Sustain hold feedback on the 3D stage.** User: "wenn lang gezogene note getroffen wird, soll der treffer solange animiert werden wie lange die taste gedrückt bleibt." Root cause was not a missing animation but a despawn: `apply_note_events` removed every entity of a hit note, and the sustain tube is one of them — the part still to be played disappeared at the strike, so there was nothing left to animate. The depth view already did this correctly (`animate_sustains`), and `TrackSession::active_sustain` already existed with a doc comment saying presentation uses it for hold feedback; the 3D view simply never asked. Now: tails survive the strike and are consumed from the hit line by `consume_sustains`, the receptor's strike is kept alive and breathing while the hold runs, and the burst re-blooms instead of decaying. New pure `sustain_tail_span` carries the half-length arithmetic. Also replaced `move_notes`' `scale.y > 1.5` guess for "is this a tube" with the real component — the heuristic held only at full length, so a partly eaten tail would have jumped half its length when dropped. *Verified: gate + 276 tests (+5, mutation-checked: removing the hit-line clamp turns 4 red) + rustdoc clean; autopilot PASSED on three songs with identical scores (98/98, 282/282, 624/624); screenshot of a held sustain shows the shortened tail running into a blooming fret.*

- [x] G25 **Gameplay screen reads like the genre (plan U1-U6).** User: "ich möchte dass die aussieht wie bei guitar hero. erstelle dazu einen plan und arbeite ihn dann ab." Plan in `docs/ui/gameplay-look-plan.md`, grounded in the reference screenshot, a measured pass over what the game draws, and the genre's documented conventions — and explicit about the boundary: conventions (odometer, quartered meter, boxed multiplier, marked phrases) are adopted, specific art is not, and the rock meter stays out because it is a fail state rather than a look. Landed: energy phrases marked on the highway with a tinted band (the biggest gap — charts carried `phrases` and paid meter for them while nothing on screen said which notes they were), a fixed-width score counter with dim leading zeros, a boxed multiplier with streak beads, a meter showing its four quarters plus a ready line, a stage that washes to the energy tone while hype runs, a hit line that spans the neck, heavier bar lines, polished gems. Two defects surfaced by the work: the 2D hype overlay is a 900-px vertical band that in 3D missed the receding neck entirely and washed a wall forty units behind the vanishing point (now skipped in 3D, measured before and after), and my own stage tint eased once per SURFACE rather than per player, so the rate depended on entity count. Harness gained `gameplay-phrase` and `gameplay-hype` capture moments — the fixed 24-26 s window falls between phrases on every song in the library, so nothing automated had ever pictured either state. *Verified: gate + 279 tests + rustdoc clean; autopilot PASSED solo, in the depth view and 2-player; screenshots of phrase, hype and normal play in both views.*

- [x] G26 **Results screen rebuilt; unreadable titles fixed.** User: "optimiere die gestaltung der app weiter, gib dir maximal viel mühe." Swept every screen first. The results screen was by far the weakest — bare text floating in a void, using none of the shared kit, at the one moment that is supposed to be the payoff. Rebuilt on the kit: the song is the heading, a grade badge sits beside the score, accuracy gets a bar as well as a figure, and the judgment breakdown uses the SAME colours the popups used during the song. The song list showed two titles with empty boxes in them: measured against the font's cmap, Press Start 2P has 656 glyphs including `å`, `ü` and `ß`, but nothing from the fullwidth or mathematical blocks — exactly the characters a downloader substitutes for `|` and `/` in file names. `ui::font_safe` maps only those look-alikes, at DISPLAY time, so the chart keeps its true title and a script the font cannot draw is left alone rather than turned into question marks. Also fixed a panic in my own screenshot hook (it entered a screen at `Startup`, before boot had created the song library). *Verified: gate + 284 tests (+5) + rustdoc clean; autopilot PASSED; results screenshot. ⚠️ Two hours of black screenshots during this task were the LOCKED SCREEN, not the code — two plausible code hypotheses were built and disproven before checking `CGSSessionScreenIsLocked`; now recorded in CLAUDE.md and the harness doc.*

- [x] G27 **Playfield proportion and surface (plan V1-V3).** Second round on the same request. Round one fixed what the screen SAID; comparing against the reference showed what was left is what the screen IS. Measured: the solo neck filled 31 % of the frame (rails at 793 px of 2560) against the reference's ~50 %, the bed was a flat fill, and the gems were undersized in their own lanes. One `neck_spread` factor applied at `lane_x()` and the three `bed_width()` sites carries rails, lane strips, receptors, bursts, bar lines, phrase bands and notes together — solo only, read from the layout's player count so it cannot drift, and deliberately NOT done by moving the camera in, which magnifies the board but shortens how far up the neck you can read. Gems scale with it. The bed gained a generated grain (`board_shade`, a hash not a random number, so the board is identical every run) with its brightness band pinned, plus a test that it is not flat — a flat texture would pass the band test and be a silently missing feature. *Verified: 31 % → 45 %; gate + 288 tests (+4) + rustdoc clean; autopilot 98/98 perfect before AND after, and 2-player PASSED. ⚠️ Black screenshots struck again mid-task and I again blamed my own change before re-running — it was transient window occlusion; the same run repeated came out correct.*

- [x] G28 **Stage light, lane dividers, gem faces (plan W1-W3).** Third round. Measured the venue alone — outer thirds, above the neck, HUD excluded — at 0.13 brightness and 0.20 saturation: dark and very nearly grey, which is the whole remaining difference from the reference in one pair of numbers. Two coloured lamps from opposite sides (ranged so the room takes the light and the fretboard does not), a generated backdrop on the rear wall (the largest surface in the frame, previously an unlit slab), materials that accept light, four neutral lane dividers, and a generated radial face on the gems. *Verified: venue 0.13 → 0.20 brightness and 0.20 → 0.29 saturation with the board unchanged (0.250 → 0.262); gate + 293 tests (+9) + rustdoc clean; autopilot 98/98 perfect and 2-player PASSED.* ⚠️ W3 took three attempts and both failures are recorded: a `base_color_texture` alone made gems FLATTER (span 50 → 10) because their look is dominated by emissive, which that map does not touch; adding `emissive_texture` gave the shape (116) but dimmed distant notes, so the face got a floor — both constraints are now tests. ⚠️ Also solved for good: the recurring black screenshots are window occlusion by the terminal, and `screencapture -l<window-id>` captures regardless of stacking — matching the OWNING PROCESS, because a terminal in this directory is itself titled "BeatByte rhythm game".

- [x] G29 **The hit flame, and the right font measured at last (plan X1-X3).** Fourth round. A screenshot caught the exact frame a note landed and showed that nothing happened beyond a lit receptor and a flat ring — and the source comment justifying that claimed the genre's flame "spreads across the board rather than rising off it", which is backwards and was my own writing from memory rather than the reference. Added: a cone flame off the fret, white-hot at the strike and cooling to the lane colour, with a low flame held under a sustain; a crowd bobbing on the song's tempo map with per-head phase. Also fixed the diacritic boxes properly: the earlier cmap measurement was of Press Start 2P (656 glyphs, has `å`), but the game runs Bevy's built-in face whenever the round note style is on — the default — and that has **95 glyphs**, plain ASCII. Folding is now style-gated, because folding letters a font CAN draw is damage. *Verified: flame caught on camera after retuning from 5:1 (a laser) to 5:3; gate + 296 tests (+3) + rustdoc clean; autopilot 98/98 perfect, 2-player PASSED; frame time median 10.0 ms (100 fps), 99th 12.4 ms with flames and a moving crowd.* ⚠️ Also built a reliable capture path — `screencapture -l<window-id>`, matching the owning process — after occlusion produced black frames repeatedly.

- [x] G30 **Documentation enforced, versioning stated, v0.11.0 released.** User asked for documentation that stays true, and for the version, changelog and push to be guaranteed rather than remembered — which is one problem: a fact nobody checks decays. Evidence was in the repository: the README's test count had been corrected twice the previous day and was stale again by the evening (271 claimed vs 296 real; beatbyte-game 77 vs 105). `apps/beatbyte/tests/docs_stay_true.rs` now reads the repository as data and fails on disagreement — the per-crate test table and its total, the manifest version against the CHANGELOG and the internal pins, the checkable badges, the ADR index against the files on disk, every `BEATBYTE_*` switch against the harness reference, every repository link, the absence of the retired `[Unreleased]` section, and the figures `docs/gameplay/rules.md` quotes from `ScoreConfig`. Versioning rule agreed and written down: the patch number rises with every user-visible change in the same commit; a tag stays a separate act at milestones (the alternative, a release per change, would have meant 37 full pipeline runs for the work already sitting here). Also new: `docs/ui/3d-stage.md` for the largest module in the game, which was documented only inside itself. *Verified: gate + 313 tests (+17) + rustdoc clean; smoke, autopilot 98/98, 2-player and editor cycle all PASSED; three drift tests were red on real drift when first run, two more proven by mutation.* ⚠️ The counter's first version searched for the substring `#[test]` and counted its own two mentions of the attribute while explaining how it counts — it matches whole lines now.

- [x] G21 **Optimization plan adopted** *(closed 2026-09-04)*. `docs/optimization-plan.md` researched and written; every proposal checked against the code rather than remembered. **Shipped:** P1 practice mode (G32+G33), P2 early/late feedback (G31 — the tag in the popup, the drift line on the results screen), P3 rock meter with No Fail (v0.13.26), P4 the library browser (v0.12.2–4) plus its columns and genres (v0.13.1), its fuzzy ranked search (v0.13.34–37) and — the half that had never been built — **song previews** (v0.14.1: `preview_start_s` was computed and stored by the generator and played by nobody), P7 the 3D renderer (G35 and the look programme). **Remains, and each needs the user, not me:** P5 charting revisited BY EAR against `chart-feel-good-20260826` (the untested idea is ranking candidates by metric position, and the plan's own rule is that the verdict is an ear), P6 the A1–A4 playtest pass. The non-goals still hold: no online leaderboards, no neural separation. One named piece of P4 is unbuilt and unclaimed: sort by *last played* (the history file carries the timestamps).

- [x] G31 **Early/late feedback (optimization plan P2)** *(v0.12.16)*. The judgment popup tags non-perfect hits with the side they landed on — `GREAT (EARLY)` / `GOOD (LATE)` — exactly where the information is actionable (a PERFECT needs no lecture, a miss has no side; negative offset = early, pinned). The solo results screen gains a TIMING row with the run's mean signed drift ("32 ms early" / "+18 ms late" / "on time" inside ±3 ms) fed by new `PlayerPerformance::mean_offset_ms()` (NaN/∞ guarded), plus a recalibration hint at ±15 ms — half the perfect window. *Verified: three pins, each mutation-checked (flipped tag sign, one-sided hint threshold, removed NaN guard — all seen to fail); results screenshot shows the TIMING row live; autopilot flawless.*

- [x] G32 **Practice speed (optimization plan P1, first half)** *(v0.12.17)*. Pause-menu SPEED row, 50–150 % in 5 % steps, applied live to audio and clock together; runs that used it are marked practice and stay out of scoreboard and telemetry, and the results screen says so. Two traps found and pinned: rodio's reported position lives in the OUTPUT timeline (wall-clock pace at any factor), so a mid-song speed change makes the naive `position × factor` wrong by a constant — the position map re-bases on every play/seek/speed change (pure `map_source_position`, the teleport counter-example in its test); and the clock re-anchors before changing its rate so song time is continuous (degenerate rates refused). *Verified: autopilot at 75 % and 125 % — measured slope 0.750x/1.251x, flawless runs, and live proof that a practice run writes NO score (mtime) and NO telemetry (file count); three pins mutation-checked; pause-menu screenshot shows the row.*

- [x] G33 **Section loop (optimization plan P1, second half)** *(v0.12.18)*. LOOP FROM/TO rows on the pause menu (RIGHT pins the paused moment, LEFT clears; a span under 1 s never arms — it would wrap faster than the lead-in plays); reaching the end jumps music, clock, sessions and in-flight note entities back to a 1.5 s lead-in, via the new `TrackSession::rewind_to` (events at/after the point reopen, session clock moves back so nothing phantom-misses, transients reset, phrases recomputed — three core pins). A seek travels to the music thread asynchronously, so reconciliation is held for 0.25 s after a wrap — reconciling against the not-yet-seeked device position would snap the clock straight back and wrap forever. *Verified: loop drill (`BEATBYTE_AUTOPILOT_LOOP`) live — two wraps at exactly the expected 11.5 s cadence, section notes reopened; drill seen to fail with the rewind removed; span pin seen to fail with the min-span dropped. With P4, P2 and P1 shipped, the plan's next in order is P3 (rock meter with No Fail) — deliberately not started without a fresh look at whether the one-player audience wants failure at all.*

- [x] G34 **Stage reads closer to the genre's classics; the red back-wall line is gone** *(v0.12.20)*. User commission (GH2-orientation): sourced the genre's visual language (WikiHero: gems = coloured circles with white centre, black ring on strum notes, ringless HOPOs; star-power notes are star-shaped; rock meter bottom-right; SP meter a tube above it; activation turns all notes toward the power colour) — inventory showed most of it already shipped (GRYBO lanes, white-centre/ring gem distinction, gem-as-button + rim in 3D, themed side rails, flames, crowd, multiplier dots). Landed: phrase notes wear a five-point star plate (pure `star_outline`, pinned + mutation-checked), note gems shift toward the energy colour while Hype runs (solo — shared lane materials would cross-tint in multiplayer), and the emissive accent band across the back wall (the reported red line) is removed. Perf: the hype-tint materials were re-uploaded EVERY frame (settled blend now writes nothing); measured before/after at both display states — the numbers are vsync-paced (8.3 ms @120 Hz, 16.7 ms @60 Hz medians) and ProMotion switches states mid-session, so like-for-like p99 comparison is confounded; the mechanism-level wins (no per-frame material uploads, one draw less) stand on inspection. Deliberately NOT done (asset rule / P3): GH2 highway artwork, fonts, rock meter + SP tube (mechanics first), exact cyan (house hue = Hype purple).

- [x] G35 **The venue becomes a concert** *(v0.12.20–v0.12.27, user-driven visual programme)*. Across seven passes: the reported red back-wall line removed; star phrase notes + Hype note tint (genre-sourced); HUD as an instrument panel (glowing streak bulbs, popping counter, half-circle Hype gauge with the activation tick straight up); beat-pulsing LED wall, then with cabinet + dot-matrix pixel structure; light rig rebuilt as moving-head fixtures (housing, lens, additive double-mantle beams with procedural falloff, swing around the hanger) with lens halos and floor pools sharing ONE pinned beam angle; near-black PA cabinets with procedural driver fronts breathing on the beat; and the realism pass from `docs/ui/stage-realism-plan.md` — club darkness, silhouette crowd, haze sheets, complementary backline, lattice trusses, stage riser. Every pass verified by engine screenshots with luma checks in the user's own settings, pins mutation-checked, autopilot + pause drill green throughout. Ongoing by-eye tuning belongs to the user.

- [x] G36 **The HUD plates become instruments** *(v0.12.28)*. Second HUD pass on commission ("optimiere das hud nochmal deutlich mehr"): corner plates re-skinned as brushed dark metal (top-edge light catch, corner rivets, vignette — `plate_shading`, pinned), the score digits in a recessed scanline well (`well_shading`, pinned), the gauge band gains a sweep gradient with a brighter READY zone past the activation tick, the needle a counterweight tail past the hub (custom sprite anchor). Motion, all transforms + sprite tints: multiplier pops on every change, the next streak bulb carries a faint ember, the gauge breathes toward the Hype tone when it can fire (`ready_glow`, pinned + mutation-checked) and blazes white-hot while Hype runs. *Verified: engine shots in the user's settings (plates/rivets/gradient/counterweight/blaze all visible in late + hype moments), 192 game-crate tests, all new pins seen failing under mutation, pause drill + full gate green.*

- [x] G37 **UX/input/menu system programme** *(commissioned 2026-09-01;
  inventory + phases in `docs/ui/input-ux-plan.md`)*. Shipped:
  **phase 1** *(v0.12.29)* — remappable `UiAction` navigation table
  (WASD + Space + Tab, Enter/Escape hard fallbacks, typing mode for
  the browser search, conflict confirmation on rebind, scrolling
  controls list); **phase 2** *(v0.12.30)* — `UiSound` event system
  (Navigate/Confirm/Back/Error/Toggle/Slider, two new voices, every
  device sounds alike); **phase 3** *(v0.12.31)* — device-aware
  prompts (`ActiveDevice` + `DeviceHint`, every footer swaps wording
  live; pad players can finally leave the results screen); **phase 4**
  *(v0.12.32)* — reduced flashing, effect-intensity slider, UI-scale
  multiplier, high contrast; the settings list scrolls; **phase 5**
  *(v0.12.33)* — video offset (draw-only, judgment provably
  unshifted); **phase 6** *(v0.12.34)* — ADR-0012 records the style
  boundary (8-bit = data through one renderer). The commission's
  phases are complete; the 2D-path prune stays its own filed task.
- [x] G38 **Every hit flame has a character** *(v0.15.17, commission: "realistischer + Randomisierung")*. Each ignition draws a seeded character from the fret and its strike count (height, girth, flicker rate, a slow gutter, resting lean, hue toward yellow- or red-orange, the core's white at the strike, its own decay) — drawn once at the strike, never re-rolled per frame, so a run reconstructs; the shapes and colours read it (a tall flame narrows, a sagging one fattens, the core flashes whiter in the first frames, a dying mantle reddens, some snuff and some linger inside the same half second). Rendering path unchanged (three sprites per fret, transforms + tints only), reduced motion keeps its still flame. *Verified: six new pins (NEUTRAL reproduces the old flame bit for bit; all five mutation probes bite), gate green, autopilot PASSED on "[GS] How Bizarre", frames and frame-time before/after in the commit.*
- [x] G39 **The crowd dial sweeps the way its needle does, in its zones' colours** *(v0.15.18, user review from a screenshot)*. Confirmed defect: the dial's sweep was mirrored (1 at the LEFT horizon — the comment beside it said the opposite), so it brightened toward the empty end and put the READY step on the half the needle leaves; a frame measured the empty end 2.3× brighter than the full one. Fixed, and the dial is now three bands cut at `GAUGE_ZONE_MARKS` = the meter's own zone thresholds, each in its zone's colour (red/yellow/green), the band under the needle lit, the red one pulsing while the crowd turns; the texture is baked in the 2:1 aspect it is drawn in (was a square minified 3.4× vertically without mipmaps) and the ticks have a soft flank. Meter, zones and needle angle untouched. *Verified: four pins, seven mutation probes bite, gate green, autopilot PASSED, before/after frames at mid and full meter.*
- [x] G40 **Players, and statistics for each of them** *(v0.16.0, commission: create players, show development over time and standing against others, detailed, with charts)*. The machine kept a play log, a scoreboard and 423 telemetry sessions, and not one of them knew who was playing. New: `beatbyte_core::player` (roster, ids that climb and are never reused) and `beatbyte_core::stats` (pure aggregation — the game plots it, the CLI prints it, one implementation); `PlayEntry` gains player, per-run detail (streak, judgment counts, overstrums, drift), co-players and the chart hash, all optional so old lines read as "not recorded"; a PLAYERS screen and a STATS screen with four views; `plot.rs` draws them in `ui_kit`'s own hand (ADR-0016 — the one library that fits Bevy 0.19 is a second UI toolkit). The first player adopts the runs that predate the roster, once, after a copy of the log goes down beside it. ⚠️ **The CLI gegenprobe caught a defect the screens would have shown as fact**: an abandoned run is logged with `accuracy: 0.0`, so averaging every run put this machine's player at 14.6 % where his finished runs average 44.6 %; quality metrics now count finished runs only, and a difficulty nobody finished says so rather than showing 0 %. *Verified: 1196 tests (+45), gate green, every printed number cross-checked against the raw log by an independent script, the four views photographed against a real 259-line history.*

- [x] G41 **One hundred achievements, and a screen that says where you stand** *(v0.17.0 – v0.17.5, commission: exactly 100 meaningful achievements, some secret, an overview of completion, extensible, gamified)*. New `beatbyte_core::achievements`: one `CATALOGUE` of 100 entries, each a title, a blurb and a `Rule` from a closed vocabulary (14 rule variants, 28 tests, 8 metrics, 4 facets), and `evaluate` re-deriving every rule from the whole play log on every pass. The store holds only `{player: {id: when}}` (ADR-0017), so a new achievement unlocks retroactively with the date it really happened, a changed catalogue needs no migration, and a raised threshold cannot take an unlock back. 12 are hidden: unearned, they show `? ? ?` and give up neither description nor progress bar, because a bar at three tenths is most of the condition. Nine signals added to `PlayEntry`/`RunDetail`, all optional (Hype activations, phrases, sustains held/dropped, failed, genre, tap mode, No Fail, practice speed — eight of them read by a rule today, `speed_percent` recorded for a later one) — an achievement that cannot tell an assisted run from an unassisted one devalues itself. Screen: ten categories on left/right, `F` filter, `O` order (catalogue / CLOSEST / NEWEST — not `S`, which the menu table binds to NavDown), per-row bar and date, a banner at the moment of unlock. ⚠️ **The re-reading after the push found six defects in five rounds** (0.17.1–0.17.5): a tautological achievement, a list that could not survive a long description, three inert controls, a screen that kept the last player looked at, a clamped cursor press that moved the cursor, and a float comparison against one machine epsilon. ⚠️ **Three screens read the play log on the state entry that reloads it, ordered by nothing** (0.17.3; the roster and statistics screens carried the same ambiguity since 0.16.0) — the reload is in a `HistoryReloaded` set now and all three order behind it. ⚠️ **The screen's three controls were inert on first delivery**: category, filter and sort each edited a resource and nothing rebuilt the list, so the footer named a filter the rows did not obey (0.17.3, found by re-reading the plugin against the statistics screen, which does respawn). Fixed with a `DrawnFrom` stamp so the list rebuilds on a shape change and not on a cursor move. ⚠️ **A tautological achievement was found by reading the CLI output against the log**: `Test::HasPlayer` could never be false (the evaluator only sees runs `part_of` returned), so "Under Your Own Name" was "Plugged In" under a second title — replaced by "Second Verse", with pins against duplicate rules and dead test variants. ⚠️ **The day-streak pin was blind on its first probe**: counting runs instead of days passed it, because no case played twice on one day — the commonest shape a real week has. *Verified: 1250 tests (+53), gate green, autopilot PASSED (290 perfect — and the store stayed at 27, so the exclusion holds live), twenty-two mutation probes bite. Cross-checked three ways that share no code: the game's startup sweep wrote 27 unlocks, `beatbyte-cli awards` computed the same 27 from the log without reading that file, and an independent script matched every sampled achievement and its date. ⚠️ Never seen by eye — the screen was locked all session, so verification is wired (the screen built for real, its `Text` nodes read back) rather than photographed.*


- [x] G41b **Three hundred achievements** *(v0.18.14, commission: expand 100 → 300, quality over grind, PlayEntry-backed only)*. Catalogue grows to 300 under the same ADR-0017 rules; eight new `Test` variants and `Metric::SustainsHeld`; 34 hidden; overview gains a **T** tier filter and earned/total on every category tab. No TECHNIQUE category, no new persisted fields. *Verified: 1687 tests (+2), gate green; catalogue pins (300 unique ids/titles/rules, every test/rule variant used, assists refuse slowed and unknown speed), UI pins for tier and category progress, docs regenerated.*
- [~] G38 **Room Stage — the venue leaves the screen** *(v0.14.2, `room_stage.rs`; vision doc `docs/room-stage.md`)*. Shipped: TODO 1 the bridge (bounded queue, worker thread, drop-never-block, backlog coalesced to the newest cue per path with accents exempt), TODO 2 the mapping (judgment → kick strength, lane → tone, Hype/phrases → accents, miss → dip; pure and mutation-probed), TODO 3 settings (ROOM LIGHTS off by default, `room_stage_url`), TODO 7 the network promise (badge, "what leaves your machine", and the docs test that now counts the second outbound path), TODO 8 autopilot coverage (a mock light service through whole songs: **158/182/166 posts delivered, 0 dropped**, scores identical to the same runs with the bridge off). **Remaining:** TODO 4 the INPUT TEST verification screen, TODO 5 the disco-controller external-beat-source mode (a change in the Pi's repo, not this one), TODO 6 the timing budget against real hardware — all three need the rig, which this machine is not. ⚠️ Never heard: no lamp has answered a note yet; the contract is verified against a mock, not against lichtwerk.

- [x] G42 **Desktop icon and Guitar Study naming cleanup** *(v0.17.9)*. Legacy `[Guitar Study]` titles are shown as `[GS]`, and the browser keeps each study directly below its normal song in every sort order. Desktop packaging now carries the RGBA master plus macOS, Windows and Linux icon formats. *Verified: 1251 tests, docs gate green.*

- [x] G43 **Xplorer tilt and four-fret Hype activation** *(v0.17.10)*. The verified Xplorer RY report and both four-adjacent logical fret shapes feed the existing Hype action through edge latches; 50%/40% tilt hysteresis prevents chatter, and every successful activation opens one shared 0.5-second automatic-hit window. Unknown guitar variants are not guessed. *Verified: 1257 tests, native USB decode, driver-axis bridge, chord edges and grace boundary pinned.*

- [x] G44 **Player Analytics P0 — Stats foundation** *(v0.18.17, design `docs/superpowers/specs/2026-09-23-player-analytics-design.md`)*. Eight tabs (four shells for Technique/Progress/Songs/Insights); difficulty + time-window filter chips; `Store::open_readonly` so Stats never migrates or writes `telemetry.db`; async session-count probe on `AsyncComputeTaskPool`. History views stay dual-source Approach 1. Follow-ups: P1–P6 fill the shells. *Verified: 1699 tests (+4), telemetry RO pins + stats filter/probe pins; gate green.*

- [x] G45 **Player Analytics P1–P6 — fill the shells** *(v0.18.18, plan `docs/superpowers/plans/2026-09-23-player-analytics-p1-p6.md`)*. Async `PlayerSnapshot` (histogram, bias, technique, context, busiest-chart problems + timeline); Timing/Technique/Progress/Songs/Insights wired; plot histogram + heat strip. Mute badge deferred (spec). *Verified: 1705 tests (+6); gate + release build with `--features ml`.*

- [x] G46 **Per-player difficulty preference** *(v0.18.19)*. Each roster profile stores `preferred_difficulty` in `players.json`; song select opens on it when offered, falls back nearest for the session without rewriting the pref, and LEFT/RIGHT (or `<`/`>`) persist the choice. Medium default; profiles independent. *Verified: 1711 tests (+6); gate green.*

- [x] G48 **The mute badge** *(built as G57 — see below)*. Deferred twice without being filed: the Player Analytics spec puts "Mute badge / other UI" out of scope as "a separate task", and G45 records it as deferred — but nothing carried it here, so it existed only as an aside in two documents. It was still an aside when the user asked for the badge directly; that is what G57 built.

- [x] G47 **The telemetry rule says what the code is, and a gate keeps it** *(documents and a gate; no version — nothing here is user-visible)*. ⚠️ Found by reading the new work against its own governing documents: G44 changed the store's read policy deliberately (spec 2026-09-23, "Store read policy — A"), but ADR-0018 still read "the game does not read the store back" and CLAUDE.md still read "Nothing reads the store back into the game". The code was right and the canon was two days stale — and the OLD guarantee had been structural (with no reader, nothing could act on the store), while the new one was prose. Both documents now state what is actually protected — the store may be read to **show**, never to **decide** — and `apps/beatbyte/tests/telemetry_stays_evidence.rs` pins it as three facts about the source: one reader, one writer, and the reader never writable. ⚠️ The gate strips comments before it looks, because this house quotes its own rules verbatim — and its stripper tracks string literals, since cutting at the `//` in `https://` would swallow the rest of that line and let the gate pass blind. That counter-check is itself a test. ⚠️ Writing the gate found a defect in the gate: the first stripper guessed at a character literal from the next character alone, got the CLOSING quote of `'a'` wrong, and from there read every following comment as code — a false alarm about this house's own prose, not blindness, since nothing is ever swallowed. Two characters of lookahead settle it. And a mutation probe then found a case my tests still missed: a lifetime whose quote is the only one before a comment. *Verified: 1716 tests (+5), gate green, autopilot 504/504 on the merged state; six probes mutated and seen to fail — a second reader, a writable reader, a second writer, a reader hidden behind a character literal, a stripper that eats a URL, and a lookahead that guesses.*

- [x] G49 **The statistics filters reach the telemetry** *(v0.18.20, plan `menu-stats-youtube` S1)*. ⚠️ A real defect, not a polish: the difficulty and time-window chips were drawn over all eight tabs, but `analytics::player_snapshot` took **no filters at all** — so on TIMING, TECHNIQUE, SONGS and INSIGHTS pressing a chip rebuilt the screen and produced a byte-identical answer, while the header named one player and the numbers were the machine's, across every player, every difficulty and all time. A control that does nothing is worse than no control. `analytics::Scope` (player, difficulty, since) now threads through all nine scoped queries, written once as `scope_clause` in the module's existing `?n IS NULL OR …` idiom so an absent filter costs nothing and `HONEST_RUNS` is untouched; `Scope::default()` is "everything", so every CLI reading is unchanged. `stats_ui::telemetry_scope` is the bridge, and a `Viewer` system param asks "who is this screen about?" once for both probe sites (`stats_nav` was at Bevy's sixteen-parameter cap). ⚠️ The two halves encode a difficulty differently on purpose — the play log stores its text id, the store its integer code — which is exactly why the pin compares them rather than assuming: one store of nine sessions (two players, four difficulties, five ages, one practice run) counted both ways over the whole 4×4 filter matrix. ⚠️ Filtering correctly then exposed how little the store attributes: on this machine **138 of 144 honest sessions name no player**, so the fix alone would have cut the screen's evidence by 96 % without a word. `PlayerSnapshot::unattributed` counts them under the same difficulty and window, and the screen states it. The cause is G50. *Verified: 1719 tests (+3), gate green, booted into TIMING against the real 144-session store and read; six probes mutated and seen to fail — an empty scope clause, each of the three terms dropped in turn, a `telemetry_scope` that returns everything, and a difficulty code off by one (the encoding bridge itself).*

- [ ] G50 **The telemetry store was never adopted the way the play history was.** Measured while building G49: `history.jsonl` holds 110 honest runs, **all** attributed to player 1 — the one-time adoption did its job. `telemetry.db` holds 144 honest sessions of which **6** name a player; the other 138 were recorded before attribution and no adoption ever ran over them. So the two halves of one screen describe the same person from 110 runs and from 6. G49 makes the screen SAY so rather than fix it, because the fix is a **write** to the store and deserves its own decision: it belongs in `writer.rs` (the gate in `telemetry_stays_evidence.rs` pins that only the writer module opens the store for writing), under the roster adoption's own rules — human runs claimed, autopilot never, somebody else's left alone, claiming twice changing nothing. ⚠️ An unattributed session is not provably the first player's; the history adoption already made that call once, and this should make the same one or say why not. *Verify: after the adoption a player's session count and their run count describe the same career, and a second run changes nothing.*

- [x] G51 **Eight tabs become six** *(v0.18.22, plan `menu-stats-youtube` S2)*. PROGRESS drew OVERVIEW's own accuracies pooled across difficulties, which is the reading OVERVIEW's own comment argues against — a player "gets worse" the day they move up — and the two disagreed in public: 5.7 points per 10 runs against 10.6. Its three tiles were a duplicate count plus two sentences. INSIGHTS was a 1150-px panel holding at most five short lines that answer OVERVIEW's question. Both moved onto OVERVIEW: findings above a hairline, numbers below it, and the trend now stated beside the SPREAD on the series it describes (`HARD: UP 10.6 POINTS PER 10 RUNS · WILD · ±14.6 POINTS` — the pooled spread had read ±26.4, inflated by mixing difficulties). ⚠️ The merge immediately produced the defect it was undoing: the findings carried a pooled trend, which landed four lines above the per-difficulty one. Seen in the screenshot, not in a test — removed, and pinned so it cannot come back. `View::ALL` is six, `needs_telemetry` now includes OVERVIEW, and `BEATBYTE_SHOT_VIEW` plus `docs/development/harness.md` lost the two names (a pin refuses them, or a shot would name a dead tab and silently get the default). The 2026-09-23 design spec carries an amendment rather than being contradicted in silence. *Verified: 1720 tests (+1), gate green; all six tabs photographed against the real library before and after, luma checked; the trend pin mutated and seen to fail.*

- [x] G52 **The statistics screen behaves like the others** *(v0.18.23, plan `menu-stats-youtube` S4, taken before S3 — a screen you cannot reach the bottom of outweighs a missing space in `38%`)*. It was the only one using `ui_kit::panel_wide()`: no ceiling, no clipping, no wheel. Photographed against the real library, SONGS had **no header, no tabs and no back button at all** — the content had pushed them off the window; VERSUS loops over every other player with no limit. Now `scroll_panel(PANEL_WIDE)` plus the song-info reader (UP/DOWN free, since the tabs are on LEFT/RIGHT; the kit scan already refuses a scroll panel without one). Tabs and both filter rows wear a new `ui_kit::SelectionChip` — distinct from `ActionChip`, which is a button that may be disabled, where this is a *choice* and every chip is pressable — and the chosen one differs in fill AND border AND text with hover answering on the unchosen ones, which the kit's own rule demanded and the bare words did not meet. ⚠️ The window filter was bound to `BracketLeft`/`BracketRight`, US-layout POSITIONS that on this machine's QWERTZ keyboard are `ü` and `+`: unreachable, the same way search once was. Both filters now read the typed character, and a gamepad reaches them at last (WEST difficulty, NORTH window — South confirms and East backs out, so both are free). ⚠️⚠️ Chasing the missing footer found a defect in the whole game, not this screen: `device_footer` spawned with an EMPTY string and was filled a frame later, so every rebuild flashed a footerless layout — and the statistics screen rebuilds whenever its telemetry probe lands, which is why the tabs that wait on the store had no footer in any screenshot while the tabs that do not kept theirs. ⚠️ Three of my own measurements were invalid before that landed: a middle-60 % crop that measured the `[M] SOUND` corner marks as "the footer", a black frame (mean luma 0.00) read as evidence, and a taller window that could not have helped because `ui_scale_target` divides by 720 — the UI always has exactly 720 px of height whatever the window is. *Verified: 1722 tests (+2), gate green; all six tabs photographed at 2560×1440 and 1280×720, every frame luma-checked and every footer read back; both new pins mutated and seen to fail.*

- [x] G53 **One vocabulary for the statistics** *(v0.18.24, plan `menu-stats-youtube` S3)*. `plot` owns the forms now: `millis` (one sign character — U+2212, because beside a full-width `+` an ASCII hyphen reads as a dash — and a plain `0 MS`, since zero claims no direction), `millis_abs` for a span, `with_sample` for a reading beside its evidence. TIMING alone had held four millisecond forms and three minus characters; one bar row wrote `100%· 45` where its neighbour wrote `71% · 860`; the difficulty chip said `MED`/`EXP` while the bar below said `MEDIUM`/`EXPERT`, so filtering renamed the thing filtered. Tiles carry numbers and the sentence that explains them sits under the row once — the drift tile had held "24 MS LATE ON AVERAGE" in a 120-px column beside "38%" — and the finding above uses the same wording rather than its own. The empty family lost its outliers (`NEVER PLAYED` → `NO RUNS YET`, an ASCII hyphen → an em dash). ⚠️ And an honesty fix that was not on the plan: the probe's error arm **discarded its reason** and printed "NO TELEMETRY STORE" for everything, so a locked or damaged database claimed the store did not exist; the reason is carried through and a first run is told apart from a failure. *Verified: 1722 tests, gate green; TIMING, DIFFICULTY and OVERVIEW photographed and read back, every frame luma-checked (one came back mean 0.00 and was reshot).*

- [x] G54 **The bar rows have no rhythm** *(v0.18.32)*. On TECHNIQUE and SONGS the rows were 21, 21, 21, 29, 37, 45 px apart while DIFFICULTY's were all 21, and in the loose rows the bar sat below its own label rather than beside it. ⚠️ **The entry that stood here ruled out the two obvious causes on evidence that was right and a conclusion that was wrong** — widening the 96-px label column to 160 changed the rhythm not at all, so it was neither the column nor a wrap. The node tree says it IS a wrap, and widening cannot fix it because **a text node with an explicit `width` is measured against no available space at all**: it breaks at every blank however wide the column is. Measured: "OFF THE BEAT" three lines in 96 px and, unchanged, in 160; a song title EIGHT lines in 300; the note beside it, which sets no width, on one line throughout. `TextLayout::default().with_no_wrap()` on the label, a column per plot (`LABEL_W` 96 for words, `LABEL_W_WIDE` 300 for titles) and `Overflow::clip()` so an over-long title can never shove the bar out of line with the rows above it. VERSUS carried the same defect — fixed width, clip, no `no_wrap` — and goes with it. The pin is the property rather than the two places: any label this module gives a fixed column to must say it does not wrap. ⚠️ **The counter-check caught a blind instrument before it caught the bug.** My whole-screen probe fired on the first frame where ANY text had been measured — a frame before the bar rows exist — and so reported a clean zero WITH the fix and WITHOUT it; hung on the rows themselves it found the wrap at once. A tool that reports 0 is worth nothing until it has been seen to report something. *Verified: 1755 tests (+1), gate green; every row 16 px in TECHNIQUE, SONGS and DIFFICULTY (one distinct height, not three), the label column exactly 96 and 300 px, the library's longest title on one line, both views photographed and read; three mutations seen to fail.*

- [x] G55 **The menu shows the statistics and hides the set-up** *(v0.18.25, plan `menu-stats-youtube` M1)*. Six views of a career were reachable only through PLAYERS → choose a person → `S`, an unlabelled key on a screen about names; STATISTICS is a main-menu row now, taking the chosen player or sending you to the roster when there is none (an empty screen is not an answer). CALIBRATION and INPUT TEST left the entry for SETTINGS — both are visited once — and the door rows there are one list (`Row::opens`) rather than a `Controls` matched by name in an `if`, so a new door cannot be added to the enum and forgotten in the handler. The browser's footer names the navigation and says the rest is a chip above (92 characters did both halves badly), and INFO is dimmed on a song that has no document, which the EDIT chip has said for the same songs all along. ⚠️ Folding that INFO rule into `refresh_browser` as one more parameter COMPILED and then panicked at startup with a Bevy query conflict — that system already writes `TextColor` and `BorderColor` through four other queries, and `With<InfoButton>` is not provably disjoint from them. It lives in its own system. **The autopilot found it; nothing else would have**, which is exactly why it runs after a change to state flow. *Verified: 1722 tests, gate green, autopilot 504/504 perfect with 0 misses and 0 overstrums, menu and settings photographed.*

- [x] G56 **A pasted YouTube link fetches that video** *(v0.18.26, plan `menu-stats-youtube` Y1+Y2)*. `D` sent whatever was typed to `ytsearch6:`, so a link became SEARCH WORDS and the ranking downloaded whatever it thought best — and `CMD+V` typed a literal "v", because nothing checked the modifier before the character arm. Now `clipboard::read` (one `arboard` call, the dependency's whole use) pastes, and `discover::parse_target` — pure, offline, tested both ways — reads watch URLs, `youtu.be`, `/shorts/`, `/embed/`, `music.youtube.com` and the bare eleven characters through whatever `list=`/`t=`/`si=` the share button appended. ⚠️ It answers `None` whenever it is not certain, because a wrong answer downloads SOME OTHER VIDEO; and every id it returns has been through `is_plain_id`, the rule that keeps anything but a plain token out of a URL. A link then skips the catalogue, the search, the ranking, the model **and the verdict** — that ladder is built to avoid live takes, remixes and covers and would fight a deliberate one; the measurement still runs and is still reported, it just no longer decides. The video id is written into the document (`identity.external.source_id`, `source.kind = youtube`) for links **and** for the name search, which is what makes duplicates answerable at all: the field existed since the document did and nothing ever wrote it, so a `D`-fetched song stood in the library as an anonymous local file. Songs imported before this carry no id and cannot take part — the audio fingerprint still catches a re-download, the id is for a re-encode. *Verified: 1729 tests (+7), gate green; the live half run against the real tool and the real network (`--ignored`: a share link with `list=` and `t=` on it resolves to a real title and length, a dead id is a readable error); five mutations seen to fail — three of the parser, and two of the id's path to disk.*

- [x] G57 **A speaker in the corner, and one answer per situation** *(v0.18.27, user's ask; closes G48)*. The badge said `[M] SOUND` / `[M] MUTED` in words; it is a pixel speaker now — two arcs when sound is on, the speaker alone in the warm accent when it is off, with `[M]` beside it in the same colour because the badge is the only place the shortcut is named. And mute is remembered **per situation**: browsing plays a preview of whatever the cursor rests on, a run plays the song you chose, and an autopilot run plays a song nobody is listening to on purpose — one answer for all three was wrong for at least one of them every time. `M` toggles the situation it is in, the three answers live in the settings, and for two seconds afterwards the badge names which one changed (the feature's own surprise: mute the browser, walk into a song, and the sound is back). ⚠️ **Three drafts of the icon, two of them photographed and thrown out.** A bar through the speaker — how the sign is drawn at print sizes — MERGES with the body in one colour at fifteen pixels and the whole thing becomes a blob; print keeps them apart with a knockout, which needs the colour *behind* the badge, and a badge floats over whatever screen it likes. A cross where the arcs were has five columns to live in, and strokes thin enough to leave gaps are a pixel and a half wide. What shipped draws the silence by drawing nothing, which is the older convention anyway, and still changes two channels at once as `ui_kit` requires. ⚠️ **And a cell must cover WHOLE pixels**: the first draft used one and a half, so on a display where the UI scale is 1 the cone's one-cell steps antialiased into each other and the speaker read as three bars — every earlier screenshot had happened to come back at twice that scale. ⚠️ `BEATBYTE_AUTOPILOT_MUTE` is a resource of its own, not a value poked into the settings: leaving the pause menu saves them (`persist_pause_settings`), so one harness run would have left mute in the player's file for ever. *Verified: 1736 tests (+7), gate green, autopilot passed; both badge states photographed and read; a clean harness run with the variable set logged `sound: off in TEST RUN` and left the settings file byte-for-byte as it found it, while a real badge click during a run did persist — which is the feature.*

- [~] **K — The classic programme.** *(All five ingredients built, v0.18.35; which of them join the default recipe is the user's blind test, one at a time — `--with <ingredient>` on a `[CL]` twin, then `T`. **The whole library carries `[CL]` twins with every ingredient since 2026-09-25** (user's call: "alles mit allen Zutaten"): `poly` over the 85 originals (~40 s each; the 85 studies share byte-identical audio, so their sidecars were copied after a SHA-256 match, not recomputed), then `classic --all --twin --with all` — 166 twins, 0 failed, every one with its analysis sidecar; the four blind-test twins kept their single-ingredient versions. Measured over them: Hard 89 % of Expert, Medium 64 % (originals) / 75 % (studies) at 59 % / 42 % on the beat, chords 12–21 % on average — under the early games' 29–36 %, because only what Basic Pitch heard struck together becomes a chord. *Verified: 1841 tests (+1: the dry run now says "already there" for a finished twin, as the real run does — it had promised 170 twins where 166 were written); the autopilot plays three freshly converted twins PERFECTLY — [CL] Africa Expert 905/905, [CL] [GS] Nothing Else Matters Hard 644/644, [CL] [GS] Livin' on a Prayer Medium 423/423.*)* A third chart twin beside the plain one and `[GS]`, working name **`[CL]`**, built from the measured rules of the early guitar games (`local/gh-klassik-recherche-2026-09-24.md`; only numbers and rules come in, never a chart or a note of anybody else's music, and never the marks themselves). One ingredient at a time, each switchable on its own, each decided by the user's BLIND TEST rather than by a metric — that is the G20 lesson, where the measurements improved and the playing got worse. `beatbyte-chart::classic` holds them and re-flags an EXISTING chart, so every test compares exactly one variable.
  - [x] K1 **HOPO like the early games** *(v0.18.28)*. Theirs is 170 of 480 ticks per beat: **tempo-relative and exclusive**, where ours asks in seconds (≤ 0.22 s Expert, ≤ 0.26 s Hard, inclusive). Past ~136 BPM on Expert and ~115 on Hard a straight eighth falls under the old rule and becomes a hammer-on, which is why the library's Hard charts are 39 % HOPO against their 11–15 %. In beats a straight eighth is 0.5 and never qualifies; an eighth-note triplet is 0.333 and always does. Chords never hammer, the first event never does, and a fret the hand already had down is a strum — after a single note and after a chord alike. `beatbyte-cli classic` writes it as a new version over the active one and moves the pointer, so `T` plays the two apart by this alone; `--dry-run` counts what would change, per difficulty and inside the window the test plays. ⚠️ The window rule moved into `beatbyte-chart` (`ChartFile::preview_window`) so the count and the test cannot measure two different thirty seconds. *Verified: 1748 tests (+12), gate green, autopilot on a reference song with the new version; six mutations seen to fail.*
  - [x] K0 **The `[CL]` twin itself** *(v0.18.33)*. The vessel: `beatbyte-cli classic <folder> --twin` writes a twin folder beside a song holding its ACTIVE chart with the ingredients applied — same notes, same times, same frets, same sustains, no audio read and nothing generated — so both charts stand in the library at once and a `[CL]` of a `[GS]` is the combination this library is mostly played on. Ingredients are a `Recipe`, one flag each, and the provenance names what a chart carries (`classic:hopo`); the ones still waiting on a blind test slot in beside `hopo` rather than changing what it means. A chart the recipe would not change gets **no twin** — a second browser entry playing the same notes is worse than none — and the dry run predicts that refusal rather than promising a write the real run declines. What both kinds of twin share now lives in `beatbyte-chart::twin`, because the three rules that must know about twins (the redesign's refusal, the catalogue's skip, the browser's filing) were each written against the study's own prefix and a second kind would have slipped past all three. ⚠️ **Three defects this exposed, none of them in the new code.** (1) A twin was inheriting its source's `song.json` — the `song_id` every score, record and recorded session is keyed by — so two different charts would have been filed as ONE song; measured on disk the moment the first twin existed (one shared id among 172 folders, mine), and invisible for 85 study twins only because documents did not exist when they were written. (2) A twin was inheriting chart sidecars, which describe a chart by content hash and are therefore discarded on every read — one of them describing a version the twin does not even have. (3) The browser's `pair_twins` demanded that a twin's original not itself be a twin, so a chain's last link matched nothing and was **dropped from the list entirely**; a song vanishing from the browser is worse than any ordering, and the rebuilt walk cannot drop or double an entry whatever the titles say. ⚠️ A mutation probe found a real blind spot of my own: `apply` with an ingredient switched OFF was never tested, because the twin path refuses an empty recipe before it ever calls it — so removing the guard left the suite green while the recipe was ignored, which is the one promise the whole programme rests on. *Verified: 1764 tests (+9), gate green; a real twin written from the Tina Turner study (163 flags on hard and on expert, title `[CL] [GS] …`, provenance naming the parent and `classic:hopo`, its sidecar carried), the autopilot playing it 702/702 perfect on Hard and 425/425 on Medium, no shared song id left among the 172 folders on disk, notes and lanes identical to the source but for 2.8e-14 s of float round-trip; eight mutations seen to fail. ⚠️ Photographed nothing: the screen was locked, and `caffeinate` cannot unlock one — the browser's ordering rests on its test, not on a picture.*
  - [x] K2 **Strum tolerance** *(v0.18.34)*. Their engine gives a strum ~60 ms to find its fret: press shortly after and it still counts. **Measured first, read-only, from the recorded sessions: on Hard, 18 of 139 overstrums (13 %) show the pattern the buffer exists to rescue** — a fret pressed within 60 ms after the strum AND a note missed within ±100 ms of it. On Medium, 0 of 10. ⚠️ But that rests on FOUR sessions out of 144: detail level `Results` does not record the individual presses and 138 of the honest sessions are at it, so the question is unanswerable for 97 % of the evidence. A counter-check says the zero in the stricter reading is real and not a broken query — the nearest successful note after an overstrum is at least 160 ms away, median 551. Raise SETTINGS → TELEMETRY to `actions` and the next few sessions will settle it. **Built as a rule the chart carries**, so the blind test separates it like every other ingredient: `rules.strum_grace_ms` (chart format, optional, 0–100, part of the hash — the same notes under another rule are a different chart to have played; an empty rule set is written as nothing so a chart that gains one keeps its identity), `Track::with_strum_grace` in core, and `TrackSession` holds a strum that lands under the wrong fret **with a note in its window**. The fret coming right within the grace makes it that note's strum, judged where the pick landed; otherwise it is the overstrum it always was, charged when the grace runs out. `classic --with strum` writes it (recipe names now selectable: `--with hopo,strum` or `all`; without `--with`, the blind-tested recipe, still `hopo` alone). ⚠️ **One rule the first sketch would have broken:** a late strum waits for a note whose window is CLOSING, so the miss has to wait with it — otherwise the note is missed in the same `advance` that reads the rescuing fret, and the grace could never help the late half of any window (pinned; mutation seen red). ⚠️ The dry-run count now compares notes by moment and fret rather than by list position, because the ingredients still to come remove notes and a positional diff would call the rest of the song changed. *Verified: 1783 tests (+19), gate green; seventeen mutations seen to fail — never holding, the miss not waiting, the grace exclusive, a second strum forgiven, holding with no note in the window, judging at the fret instead of the pick, a rewind keeping the strum, no resolution on press or on release, no clamp, conversion dropping the rule, no validation bound, an empty rule set serialized, the recipe and the parser ignoring it, the rule rewritten when already present, and the window diff ignoring added notes. **Blind test open** (the user's call).*
  - [x] K3 **Hard the way they wrote it** *(v0.18.35)*. `beatbyte-chart::classic::ladder`: a classic Hard is the chart's OWN Expert with notes taken away — never moved, never added — down to 89 % of its events (theirs: 88–90 %). What goes first is a pure score recomputed after every deletion: crowding (distance to the nearest kept neighbour, 0 at half a beat), metrical position (sixteenth before triplet before the off-beat eighth before the beat), a fret change inside a fast run before a repeated fret (the authoring guides' "no sixteenth movement" on Hard), chords and sustains protected; the first event never goes. Chords are shaped for Hard — pairs only, the outer frets of a triple, never green with orange. Phrases are Expert's that still hold a note (theirs carry the same phrases on every level). A hammer-on flag survives only where its predecessor is still there, and the HOPO ingredient re-reads every flag AFTER the ladder (the order is part of the recipe, pinned). ⚠️ **What it cannot fix, measured on the library:** Hard comes out at 3.41 events/s and 89 % of Expert (theirs 2.8–3.4, 88–90 %) but with 26 % hammer-ons against their 11–15 % — because our Expert holds 40 % sixteenth gaps against their 20–24 %, and deletion passes on what the level above holds. That is a finding about Expert, recorded rather than tuned away (ADR-0020). On the `[GS]` studies Hard lands at 3 %: their Expert is sparser still.
  - [x] K4 **Medium: density and beat-faithfulness** *(v0.18.35)*. A classic Medium is its Hard with notes taken away to 2.2 notes a second, held inside 61–76 % of Expert's events and never more than Hard has (`medium_target`), by the same score — so beats are kept longest. Five frets fold to four one PASSAGE at a time (a passage ends at a two-beat rest): a passage without orange is left alone, one without green moves down a fret whole so every step survives, only a passage spanning all five loses its top. Chords are pairs spanning at most two frets. Measured on the library: 2.43 events/s, 64 % of Expert, **60 % on a beat** (theirs 2.0–2.45, 61–76 %, 57–63 %), practically no sixteenth gaps left (theirs 2–8 %), against 1.56 / 42 % / 50 % before. The per-note analysis sidecar now travels by MOMENT, not by position, because a derived level strikes only at moments its parent struck — and a parent difficulty whose entries do not line up with its own track is not read at all (pinned).
  - [x] K5 **Chords** *(v0.18.35)*. The dependency this waited for was "polyphonic recognition"; it is built as **`beatbyte-poly`**, the thirteenth crate: Spotify's Basic Pitch ONNX (Apache-2.0, 0.2 MB, pinned by commit, size and SHA-256 — the only registry entry fetched from upstream rather than re-hosted, which is a publish and the maintainer's call) through the pinned runtime, the reference's windowing and drift correction, its polyphonic tracker without the "melodia" step that invents unstruck notes. `beatbyte-cli poly <folder>` separates the song (local Demucs), transcribes the `other` stem and writes `<audio>.poly.json`; `classic --with chords` reads it and writes a chord only where two or more pitch CLASSES were struck within 60 ms, an overtone of a lower note excluded unless nearly as strong. The interval shapes it (a third side by side, a fifth one fret apart, wider two apart); triples only on Expert; at most 36 % of a level's events chords and at most 16 % of those triples — ceilings, strongest evidence first; no chord on a sixteenth on Hard/Expert, none closer than about a beat on Medium. Without a sidecar it writes nothing and names the command that makes one. ⚠️ **Three defects found on the first real song, none by the fixtures:** (1) the Basic Pitch output names mean nothing (`StatefulPartitionedCall:1/2`); a synthetic A4 and A-major triad settled which is the note head and which the onset head, and the driver test now writes the names out rather than borrowing the driver's constants — the mutation probe showed a swap was invisible before; (2) a wide pair on yellow underflowed `lane - span` — the tests had checked the neck's edges only, and now all 480 lane × interval × class × level combinations are asserted; (3) the minimum spacing was first 200 ms, a config field of the early games whose MEANING the research marks as unknown — at 156 BPM it blocked every chord run on eighths (evidence at 36 % of the events, chords at 9 %); the rule is now beat-relative. Measured on the reference song (Tina Turner study, 156 BPM): evidence at 254 of 704 Expert events; Expert 36 % chords (65 % one fret apart, 16 % side by side, 16 % triples; theirs 54 / 32 / 7 %), Hard 36 %, Medium 20 % (theirs 29–36, 27–33, 17–18 %). *Verified: 1836 tests (+53 over K2), gate green; the autopilot plays the reference song's classic version (hard+medium, then chords on top) PERFECTLY on Expert 704/704, Hard 627/627 and Medium 535/535; the ladder and chord numbers above measured over the 171-folder library (a scratch copy of the charts) and the reference song; 60 mutations across K3–K5 seen to fail after the tests they exposed were sharpened (nine were blind at first: crowding hidden behind movement, the re-scoring, the first-event rule, the tie rule, the eighth's tolerance edge, `apply` for hard/medium/chords, the flag order, a misaligned sidecar, the model's output names), two left standing on purpose as equivalent — the carried sidecar's `insert` vs `or_insert` (one moment has one context whichever difficulty it comes from) and the note tracker's strict onset peak (the later peak uses up the energy, so a plateau reads the same either way). **Blind tests open** (the user's call), one ingredient at a time.*

- [x] G58 **A song folder with a subfolder could never get a guitar twin** *(v0.18.29)*. Separated stems live in `<song>.stems`, and the copy that fills a new twin used `fs::copy`, which fails on a directory. ⚠️ The failure landed **after** `create_dir`, so the twin folder existed, held no chart, and `write_twin`'s first line — "if the folder exists, it is already there" — sent every later attempt home before it could reach the failure again: the song never got a twin and nothing ever said why. Both halves fixed: the copy walks FILES (a twin can separate its own stems from the audio beside it), and a folder counts as a finished twin only once it holds a chart, which is written last on purpose. ⚠️ The first version of the copy test **reimplemented the loop** so that "the test cannot drift from it" and achieved the opposite — a mutation of the real loop changed nothing and the pin was blind; the loop is now a named function both sides call. ⚠️ And the existing "already there" test encoded the old contract, so the suite was RED while I ran two mutation probes against it — a red baseline proves nothing, and both readings were thrown away and taken again. *Verified: 1752 tests (+6), gate green; three mutations seen to fail — an existing folder counted as finished, a subfolder copied after all, and a failed copy swallowed.*

- [x] G59 **A redesign no longer buries a guitar twin's chart** *(v0.18.30)*. A twin's chart comes from the separated instrument, but the file still names the MIX as its audio — so a redesign read the mix, generated against it, and made that the twin's active version, silently replacing the stem chart the player had been enjoying. Both doors lead through `redesign_folder` (`redesign --all` walks every directory it finds; the browser's `G` hands one in), so the refusal lives there rather than at either of them, and a future third caller is covered by construction. A library rollover counts twins as skipped, not failed. *Verified: 1753 tests (+1), gate green; two mutations seen to fail — the refusal removed, and the condition inverted so ordinary folders would be refused instead.*

- [x] G60 **A classic version carries its analysis sidecar** *(v0.18.31)*. `redesign` has always written the per-note `.context.json` beside a new version; `classic` (K1) did not, so a run played on one recorded evidence that nothing could join to what the analysis said at those notes — found by asking what was still open rather than by anything failing. ⚠️ A file copy would not have worked: a sidecar names the chart it describes **by content hash**, and one whose hash does not match is discarded as describing a chart that no longer exists. The hash is rewritten instead, which is honest only because the ingredient changes FLAGS and nothing else — the track events are the same events in the same order, so one packed entry still describes one of them — and the entry count is checked against the new chart's own tracks first. A sidecar that does not line up is refused rather than written, because every later reading would blame the wrong note. The four reference twins were rewritten with theirs. ⚠️ The test's own fixture changed no flag at 0.2 s (0.4 of a beat at 120 BPM is over the threshold) and a guard inside the test caught it. *Verified: 1754 tests (+1), gate green; three mutations seen to fail — the hash left as the parent's, the line-up check skipped, and nothing written at all.*

### F — Release engineering to 1.0

- [x] F0 **v0.9.0 — content, accessibility, editor v2.** Amendment (2026-08-24): every unblocked B/C/D/E task landed while the A tasks wait on human playtesting — that body of work (second song, song-selector harness, import guide + scan fix, format truth, forward-compat pins, Linux/Windows CI smokes, Stage Motion, colorblind lane shapes, HOPO visibility, editor move/bulk/audition, harness integrity) ships now instead of idling behind A2. *SHIPPED 2026-08-24: all 7 assets auto-attached via the recursive glob; aarch64 tarball smoke + flawless artifact autopilot from neutral CWD + DMG .app smoke; two release-run attempts (spurious hdiutil ENOSPC → retry mitigation).*
- [ ] F1 **The tuning release** (version picked at release time). Ship A1–A4 findings. dep: A2. *Verify: full release procedure incl. artifact smoke tests.*
- [ ] F2 **1.0 readiness checklist.** All A tasks done; C1–C3 done; chart format v1 documented as frozen (changes now = format v2); README/docs audit; all four platform artifacts verified at least once on their real OS. dep: F1, A*, C1–C3. *Verify: checklist appended here with evidence links.*
- [ ] F3 **v1.0.0.** Tag, release, drop `--prerelease`. dep: F2. *Verify: published, artifacts tested, CHANGELOG sectioned.*

---

### H — True 3D renderer (planned follow-up, user-approved direction)

The depth view is a projection inside the 2D pipeline. A real 3D
renderer (meshes, PBR materials, lights, camera tilt) is a separate
project. Milestones when picked up:

- [x] H1 3D/2D camera stack (Camera3d for the stage, Camera2d overlay for HUD/text) coexisting with both current views.
- [x] H2 Highway as a lit 3D plane + gem spheres as meshes with emissive PBR materials; judgment stays input-stamp-driven (identical-score proof again).
- [x] H2b Sustains as tube meshes + 3D receptor feedback (press sinks the fret into the neck, a hit makes it flare through the bloom pass). *Verified: identical judgment to 2D (624 perfect / 0 miss / 0 overstrum on the same chart).*
- [x] H3 **3D particles; depth of field measured and rejected** *(v0.14.3)*. The sustain-tube half of this line shipped as H2b and the text had gone stale. What was left: hit sparks in world space (`gameplay/spark3d.rs` — they arc from the struck receptor in perspective, shrink rather than fade, round neck only, capped and scaled by EFFECT INTENSITY; 5 tests, 5 mutation probes, no measurable frame cost against H4's baseline), and the lens. The lens was built and removed: at a usable aperture it does nothing (far/near sharpness 0.962 → 0.969), and wide enough to see it blurs the strike line first (612 → 475). The venue separation it was wanted for is already the stage fog's job. Bloom reviewed, left alone. The flat burst that was left unexplained here was diagnosed the next session (v0.14.4): not invisible but MISPLACED — the 3D solo neck is 1.45× wider than the flat layout it is positioned with — and it now stands down on the stage, the outro fireworks excepted.
- [~] H4 Performance pass + packaging size check. Measured on this machine: the 3D stage holds a vsync-locked 60 fps during a full song with a 99th-percentile frame of 19.4 ms — no stalls, so it costs no notes. `BEATBYTE_FPS=1` now reports median and 99th-percentile frame times (an average would hide exactly the stutters that lose notes). Still open: a low-end GPU and the artifact size check.

## Vocal/karaoke gameplay (IN PROGRESS 2026-09-20 — docs/plans/vocal-karaoke-gameplay.md)

The architecture plan was approved on 2026-09-20 and its nine
milestones are worked in order. The instrumental game stays playable
throughout: every vocal prerequisite that is missing — a microphone,
a separator, a model, lyrics, a reliable vocal chart — is a state the
game **records and shows**, never a failed import and never a song
that will not start.

- [x] V0 **Technical and licensing spike.** The plan's largest open
  risk was whether Spotify's Basic Pitch ONNX runs under `rten` at
  all; it does. Loaded through the existing runtime it takes a
  `[batch, 43844, 1]` window of 22 050 Hz mono and returns note,
  onset and contour heads (172×88, 172×88, 172×264), and on synthetic
  tones it is right: 440 Hz reads as MIDI 69, 220 Hz as MIDI 57.
  *Verified: run through `rten` 0.26 with the graph's own operators,
  27.8 ms per two-second window in a debug build (≈70× realtime for
  offline use). The file is Apache-2.0 and pins to a commit — size
  230 444, SHA-256 `2c3c1d14…a0ec` — and the commit-pinned URL was
  checked to serve those exact bytes. Demucs is present on this
  machine. ⚠️ Re-hosting the model as a release asset of this
  repository, the way the other four are, is an outward-facing
  publish and is left for the user.*
- [x] V1 **The vocal model and its two sidecars.** `beatbyte_core::vocal`
  holds what a singer is asked to sing — parts, phrases, notes, a
  sparse pitch contour per note so a bend stays a bend — plus the
  arithmetic that judges one: Hz enters at exactly one door
  (`hz_to_midi`), everything after it is fractional MIDI and cents,
  and octave-independent comparison is a fold rather than a special
  case. `beatbyte_chart::vocals` stores it as `<audio>.vocals.json`,
  written through `.part` and a rename; `beatbyte_audio::stems` stores
  what separation produced as `<audio>.stems.json`, and its
  `StemState` is the piece that stops a library rescan paying for the
  same expensive answer twice — an instrumental song, a song with no
  usable vocals and a hard failure are *settled*, while a missing
  separator is a fact about the machine and is retried when one
  appears. *Verified: 1292 tests (+35), gate green. The docs gate's
  own doc-example counter was generalised in the same commit — it
  hard-coded "one example, in beatbyte-chart" and the second one
  broke it, which is precisely what its comment said would happen —
  and now counts fenced Rust blocks across the workspace, telling
  prose blocks and closing fences apart, with a test of its own.*
- [x] V2a **The analysis core: stem to notes.** `beatbyte_audio::pitch`
  is McLeod's NSDF written out rather than pulled in — the plan named
  a crate and listed the audit it would have to pass, and writing the
  page of arithmetic satisfies the audit by construction. One
  estimator serves the offline pass and the microphone, so the target
  and the judgement cannot disagree about what a note is.
  `beatbyte_audio::separate` is the shared separator: htdemucs
  computes all four sources whatever you ask it for, so asking for
  all four costs the same minutes and stops the Guitar Study twin
  needing a run of its own. `beatbyte_audio::singing` is the
  deterministic segmentation — median, bridge, split, simplify,
  group — and `beatbyte_audio::stems` now builds and persists what
  comes out. ⚠️ **The synthetic corpus earned its keep twice before
  any of it ran on music**: the parabolic interpolation's sign was
  inverted, so every pitch came back sharp (11 cents at 110 Hz, 35 at
  440), and the "skip the first hump" rule discarded a genuine peak
  whenever the range's short end fell inside it — at 880 Hz it did,
  and the detector reported 440. ⚠️ **The mutation probe found five
  blind spots**, and two of them were whole stages that could have
  been deleted with the suite green: the median filter and the gap
  bridging were tested as functions and by nothing as *stages*. The
  other three were the window-centre stamp (a 32 ms systematic offset
  on every note in the game — the defect that reads as "the lyrics
  feel late"), the minimum note length and the confidence floor.
  *Verified: 1335 tests (+43), gate green. Measured, not assumed:
  256.6 µs per hop in release = 1.6 % of one core at 16 ms hops, and
  zero heap allocations per hop across 600 of them behind a
  thread-local counting allocator that is itself proved to count and
  proved not to see another thread's. ⚠️ The allocator's first
  version was global and duly reported five phantom allocations —
  the test harness runs tests in parallel.*
- [~] V2b **Wiring it into the game** *(v0.17.11)*. Shipped: the
  `VOCAL CHARTS` setting (**off by default and saying why** — a
  separation is minutes of a saturated machine and the two kept stems
  are ~80 MB a song, so two hundred songs is fifteen gigabytes), the
  import queue carrying both jobs so **one** four-source run serves
  the vocal chart and the Guitar Study twin, and
  `beatbyte-cli vocals` for a song or a library with `--status` to
  ask without paying. ⚠️ Fixed in passing: a failed chore start
  re-queued a path rebuilt from the song's NAME, so the retry looked
  for the folder in the working directory. **Remaining:** lazy
  analysis of old songs from the library scan, and linking the
  aligner's words to the notes.
  *Verified: 1340 tests (+5), gate green, and the whole chain run on
  a real song — Blondie's* Maria*: 52 s end to end, 426 notes in 19
  phrases. ⚠️ Its pitch histogram had two peaks exactly twelve
  semitones apart, which is the octave-error signature, so it was
  checked against the stem's own spectrum rather than assumed: of the
  notes charted at the upper peak, 21 % also had a partial an octave
  below. That found a real defect — a local octave correction now
  folds a frame back when a ±12 shift lands it MUCH closer to the
  median of the second around it, which removed the outliers at MIDI
  86, 81, 78, 48, 46 and 44. The two peaks themselves stayed, and the
  phrase-by-phrase listing says why: they are the verses (~56) and
  the choruses (~69). The chart is musically right. ⚠️ Never heard —
  no ear has judged this chart, only its spectrum.*
- [x] V2c **Old songs, words, and the twin.** Shipped: a one-time
  **backfill** puts the library's outstanding vocal work on the same
  one-at-a-time queue, found from the sidecars alone — two hundred
  tiny reads, no separator runs, because the expensive question was
  answered once and written down — and it says out loud what it is
  about to spend rather than letting a player discover it as a full
  disk. And the vocal chart is now placed beside the `[GS]` twin too:
  the twin keeps its own byte-identical COPY of the audio, so the
  sidecar beside the original was invisible from there and selecting
  the study entry silently got no vocals. And the aligner's words now
  meet the notes: `link_tokens` puts each word on the phrase it
  overlaps **most** — phrase boundaries come from silence in the
  singing and word boundaries from the aligner, they will not agree,
  and a word that straddles one has to land somewhere rather than in
  both — and each note's range covers every word it overlaps, so a
  melisma's notes all carry their syllable and a word sung across two
  notes is on both. A word sung where no phrase was found is dropped
  rather than put on notes it is not on.
  *Verified: 1413 tests (+7), gate green. Run on the real Maria:
  178 words across 19 phrases, **57 % of notes carry one**, and the
  second phrase reads "Smooth | as | silk | cool | as | air" note by
  note. The chart records the alignment's own hash, so it can be
  known to be talking about an alignment that has since been
  recomputed.*
- [x] V3 **Microphone engine.** `beatbyte_audio::mic` is the pure
  half — a **streaming integer decimator** (a windowed-sinc resampler
  called per block has an edge at every block boundary, and measuring
  periodicity across those boundaries is the detector's whole job),
  hop scheduling, and the three subtractions that put a captured
  frame on the song's timeline: the anchor where the two clocks were
  seen together, the detector's own half-window delay which is
  arithmetic, and the per-device microphone offset which can only be
  measured. `listen.rs` gains a `VocalTap` on the **one** stream it
  already owns, because opening the same device twice can fail, can
  land on different configurations, and leaves two capture clocks
  that cannot be compared. Off until something asks; a game that
  stops collecting loses the oldest singing, not the newest.
  ⚠️ Two defects the tests found: frames produced by one push all
  carried the **same stamp** (a whole block of estimates landing on
  one instant — the counter advanced before they were emitted), and
  the aliasing test used 7 kHz, which is under the filter's cutoff
  and never aliases at all; what it caught was a subharmonic, and a
  test has to reproduce the mechanism it claims to be about.
  *Verified: 1373 tests (+23), gate green. And the claim that the
  offline and live paths are ONE estimator was measured rather than
  asserted — both run over the real Maria vocal stem, matched frame
  to frame by time: **98.1 % agree within 50 cents, mean absolute
  difference 25.9 cents**, and the remaining 1.0 % are exactly an
  octave apart, which is the known ambiguity the offline pass
  corrects with context and octave-independent scoring absorbs live.
  ⚠️ The first run of that comparison said 81 %: it indexed offline
  frames by `time / hop` and forgot that they are stamped at window
  CENTRES, so it was comparing frames 32 ms apart. The measurement
  was wrong, not the code.*
- [x] V4 **`VocalSession` and scoring.** `beatbyte_core::vocal_session`
  judges a singer the way `session` judges a guitarist: frames in,
  note and phrase outcomes out, deterministic and engine-free. The
  plan's weighting (pitch 65 %, timing 15 %, hold 15 %, steadiness
  5 %), the cent bands scaled per difficulty, and two rules that are
  the point of the whole thing — **loudness is never a reward** (it
  decides whether a frame counts, and nothing else; a game that pays
  for volume teaches people to shout) and **rap and speech leave
  pitch out of their denominator** rather than scoring zero in it.
  ⚠️ The first design capped a note's grade by its TOTAL score, which
  already contains pitch — so the cent table was almost decorative: a
  note 15 cents out is Perfect by the table and came back Great.
  The cap is by **delivery** (timing and hold) now, which is the one
  thing it was ever for: a note perfectly in tune for a third of its
  length is not a Perfect. ⚠️ A blind mutation probe found the
  rewind guard untested — a single rewound frame clamps its own span
  to zero, so removing the guard changed nothing; it takes a rewind
  **followed by a replay**, which is what a clock snap actually looks
  like, and that pays for the same second twice.
  Deliberately NOT here: Hype, the rock meter and the combo ladder's
  owner. This produces outcomes and the game feeds them to the one
  implementation that exists — a second Hype would be two Hypes.
  *Verified: 1360 tests (+20), gate green, nine mutation probes bite
  (including one that makes a louder frame buy coverage, against the
  test that sings the same phrase at a whisper and a shout).*
- [x] V5 **Vocal presentation.** `gameplay::vocal` draws the ribbon —
  the target melody as bars sliding past a playhead, the microphone's
  recent pitch as a trace over them, coloured by how far off it is,
  with the phrase's verdict and a readout that says LISTENING, NO
  VOCAL, TOO LOUD or which way to move. The visible pitch range
  follows the whole visible WINDOW rather than the current phrase, so
  a high phrase is already in frame before it arrives, and it eases
  rather than jumping. ⚠️ Ordered `after(open_ears)`: both run on the
  same state entry, Bevy orders same-schedule systems arbitrarily,
  and unordered this read the microphone before it existed about half
  the time. **Remaining:** the Reduced Motion / Reduced Flashing /
  High Contrast variants, and feeding the phrase outcomes to the
  existing Hype and effects (V8).
  *Verified: 1396 tests (+23), gate green, ten mutation probes bite —
  including the arrow pointing the wrong way, which is the single
  most confusing thing this HUD could do. Live in the game: a run on
  a song that has a chart logs `vocals: holding the singer to 19
  phrases, 426 notes`. ⚠️ **Never seen.** The screen was locked all
  session, so every capture is black by definition; the ribbon is
  verified wired and measured, not looked at.*
  ⚠️ **A pre-existing failure found while testing, not caused by any
  of this:** the autopilot fails `[GS] Maria` with exactly **3
  overstrums at notes 95, 183 and 312**, every note otherwise
  Perfect. Two runs — one with the vocal path completely inactive,
  one with it fully live — produced byte-identical overstrums at the
  same three notes, so it is a property of that stem-charted twin and
  the injector, deterministic and reproducible. Outside this plan — and
  for five weeks "filed" meant only this paragraph. Resolved as
  **C7** in v0.18.1, and the chart was never at fault: it is the
  Hype activation window taking a note out from under the strum the
  player was already making.
- [~] V6 **Calibration and settings.** Shipped: `MIC OFFSET` and
  `VOCAL PITCH` (ANY OCTAVE / AS WRITTEN) in SETTINGS, the octave
  answer reaching the judgment while the **difficulty** keeps setting
  the tolerances — a player who picked Expert asked for Expert, and
  the octave is a fact about their voice rather than about how hard
  they want it — and the measurement the calibration screen exists to
  produce: `round_trip_offset`, a normalised cross-correlation
  against a swept chirp, right to within a millisecond at four
  delays through noise and refusing to answer at all when the room
  returned nothing. ⚠️ Two defects on the way, both the classic ones:
  normalising by the captured energy ALONE lets a nearly-silent
  window win (its energy falls faster than its correlation does), and
  the confidence margin compared the peak against **its own
  neighbour**, which is the same match one sample over — so every
  correct measurement was rejected. **Remaining:** the screen that
  plays the chirp and applies the number, device selection, and the
  gain/gate/sensitivity knobs, which want a real microphone in a real
  room to tune and this session had neither.
  *Verified: 1401 tests (+5), gate green.*
- [x] V7 **Karaoke playback.** A karaoke run plays the backing rather
  than the song. The trick is the default: with the original singer
  at zero, the karaoke track **IS** the instrumental stem — nothing
  is mixed, nothing is written, and a song starts as fast as it
  always did. Above zero the mix is built once and kept beside the
  stems under a name that carries the level, so the same setting
  plays instantly next time and a changed one is not silently
  ignored. It is handed over as a plain `SongAudio::File`, so the
  loudness gain, the crossfade and the seek all work on it unchanged,
  and the stems were separated from BeatByte's own decode so the
  backing sits on the chart's timeline. The instrumental gets its
  **own** loudness measurement when it is kept: it is quieter than
  the song it came from by however much the singer contributed, and
  borrowing the original's number would play it at the wrong level.
  Any run with the original above silence is marked **assisted** —
  a microphone cannot tell the player from a voice out of the
  speakers, and a score that quietly compares the two is worse than
  one that admits which it is. ⚠️ Not used for a taste test: that
  compares two charts of one recording, and swapping the recording
  under it would compare something else.
  *Verified: 1406 tests (+5), gate green, and three mutation probes
  that were BLIND first. Two of them measured a result that looks the
  same either way — adding zero changes nothing, and a rebuilt file
  holds the same bytes — so the tests now check what actually
  differs: that asking for no original singer succeeds even against a
  stem the mixer could not have used, and that a marker written into
  the mix survives a second call.*
- [~] V8 **Events, results and polish.** Shipped: `LastResults` gains
  `vocalists`, the run's last phrase is CLOSED before the result is
  taken (a song ends with a phrase still open, and a run whose final
  line simply vanished would be scored one phrase short), and the
  results screen grows a vocal panel — grade, score, pitch, average
  cents off, timing, hold, steadiness, notes, perfect phrases, best
  run, and the range **you** reached written as note names. A
  measurement nobody produced is left out rather than printed as
  0 %, which reads as "you were terrible" instead of "there is
  nothing to report"; a run where nobody opened their mouth shows no
  panel at all. The debug overlay grows three microphone rows —
  device and state, level, clarity, the pitch heard and the pitch
  asked for, the offset, and the frames the game dropped — read from
  ONE borrow of the run, because sampling the same values in several
  places across a frame would show a state that never existed. And a
  finished sung phrase now lights the room: `post_for_vocal` speaks
  the **same vocabulary** as a played phrase, because the room does
  not know or care which instrument earned the accent, only how big
  it was; a phrase nobody sang lights nothing, since the room is not
  a second scoreboard. **Remaining:** the particles and sound
  effects, which are visual and audible glue and nobody could look at
  or listen to them this session.
  *Verified: 1417 tests (+4), gate green.*
- [~] V9 **Release gate.** The suite, the docs gate and the chart
  fingerprints pass at every commit. Measured rather than assumed:
  the detector is **256.6 µs a hop in release = 1.6 % of one core**
  at 16 ms hops with **zero heap allocations** across 600 of them;
  its unavoidable delay is **32 ms**, half a window, which the
  calibration subtracts before measuring the rest. **Isolation
  proved by A/B**: four autopilot runs of one song — vocals off,
  vocals on without a chart, vocals on with a chart, and vocals on
  with the ribbon live — all produced 428 perfect notes, 0 misses and
  a score within three points. Vocal play does not touch guitar
  judgment.
  ⚠️ **The gate run found a real defect, and it was the hiding kind.**
  A song with a vocal chart but no stems beside it plays its ORIGINAL
  MIX — the record sings every note — while the settings look exactly
  like a clean run, so the score was not marked assisted. It is now:
  `assisted` asks what was PLAYED, not what was configured. Found by
  the autopilot picking a `[GS]` twin, which carries the chart but
  not the stems.
  ⚠️ Also found: the decode caps at 20 minutes, so a longer song
  would get stems SHORTER than itself and a backing that runs out
  mid-song. Refused as a settled failure with a reason a player can
  read.
  **Remaining, and none of it doable here:** the platform pass on
  Windows and Linux devices, and a real singer at a real microphone.
  ⚠️ **Nothing vocal has been heard or seen.** The screen was locked
  for this entire session — every capture is black by definition and
  `caffeinate -u` cannot unlock one — and there was no singer. The
  whole feature is verified wired, measured and mutation-probed, and
  that is a different thing from verified good. It is **off by
  default**, which is what makes shipping it in that state honest.
  *Verified: 1418 tests, gate green, four autopilot runs.*

## Stage monitors and the room's light show (2026-09-07, v0.14.39–)

- [x] **BPM and dB monitors on the PA** *(v0.14.39)*. Commissioned:
  the left stack shows the tempo, the right the level, both measured
  at the laptop; without audio support neither exists. Built on the
  first audio INPUT path in the tree (`beatbyte-audio::listen`, cpal
  — the library rodio already plays through — on its own thread
  like the player; the level as eased block RMS, the tempo by the
  reference rig's estimator, pinned on synthesized kicks at 96/120/
  140 BPM, silence, a whisper under the gate, and a stop going
  stale). The monitors are seven-segment cells of emissive bars,
  spawned the frame a measurement exists and despawned the frame it
  stops. Privacy stated in the README and held there by the docs
  test; the macOS bundle declares the microphone usage. **Tempo
  rewritten in v0.14.41:** the inter-onset median read 82–136 on
  Teen Spirit (117); the chart pipeline's flux + autocorrelation on
  rolling 8 s windows reads 113–125 (measured offline on the decode,
  the `hear_a_file` harness). Through the laptop mic with the song
  from the speakers (`hear_the_room`, `BEATBYTE_LISTEN_SECONDS=32`):
  110–116. **v0.14.42:** user report "zu klein, negative Werte, BPM
  sehe ich gar nicht" — panels rebuilt as dot matrices on top of the
  heads (R4 style), level shown as dBFS + 100 like the dB-Analyse.
  **Seen (2026-09-07, unlocked screen):** both panels legible from
  the camera — 116 on the left, 57 on the right on a −43 dBFS room —
  and a comet running the left barrier rail, absent in a frame
  between effects. The highlight's punch is proven by log, not
  isolated by eye.
- [x] **Strobe on the ceiling, sparks that die** *(v0.14.43)*. On
  report: the ceiling rig strobes white in a shuffled order while
  the level is over the threshold (a pair of ten lamps at 12 Hz,
  every lamp once per cycle, off entirely under REDUCED FLASHING),
  and the sparkle was rebuilt as the reference rig's dying clusters
  after the first cut read as white noise ("aktuell gefällt mir der
  nicht"). Comet eased and given the larger share; strips at 64
  bars. Nine mutation probes and one wired drill (a lamp flares
  white, its neighbour does not, the key light never does,
  everything returns to its own colour).
- [x] **Add a song by name** *(v0.14.50)*. Commissioned as "neue
  Lieder über YouTube hinzufügen, samt Lyrics, optional mit KI, die
  beste Version finden". Built as `discover.rs`: search, a two-stage
  choice, fetch, measure, lyrics, chart — all of it feeding the
  existing `import_song`, so there is still one way for a song to
  enter the library.

  The choice is the interesting part and it is **measured**: the
  catalogue's length through `duration_fits` rejects other edits
  before anything is downloaded, titles are read for what they admit
  to, and only the leader is fetched and put on the instruments this
  project already owns (the loudness report's spectrum verdict, the
  analyzer's tempo confidence). No language model is needed for any
  of that, which is why the optional one is only allowed to reorder
  the shortlist.

  ⚠️ **What was asked for and not built**: a stream extractor inside
  the game. YouTube's signature ciphering exists to keep third-party
  downloaders out; an extractor is thousands of lines with a half-life
  measured in weeks. `yt-dlp` is called as a program instead — the
  player's experience is the same and the moving part stays outside
  this repository. Said plainly at the time rather than quietly
  substituted.

  Ten mutation probes, all firing — but see the CLAUDE.md gotcha: two
  probe runs overlapped, one "caught" reading was taken from a
  contaminated file, and a mutation was left in the tree until the
  suite went red. Re-run cleanly afterwards.

  **Follow-ups, all done** *(v0.14.55–56)*. Reported as "keine
  lyrics und info sagt, dass hinzugefügt wurde, aber ich finde den
  track nicht", then commissioned as "neue Lieder direkt sichtbar,
  Lyrics gezogen, KI optimiert, schöner animiert, im Hintergrund".
  Three separate faults behind the one report: the library was
  scanned once at boot, so a found song was invisible until a
  restart; the fetched file kept the video's id, so the folder was
  named after it; and with no artist in the query the lyrics lookup
  had nothing to ask the catalogue. Then the round itself: the
  search **moved out of the browser into its own plugin**, because
  registered on the browser's systems its poll stopped the moment a
  song started — it now runs and lands wherever the player is, and
  draws itself on the **import overlay**, the panel every screen
  already carries. Typed phases (`Phase`) rather than sentences, so
  the bar knows where it stands: each phase's mark taken at once,
  creeping toward the next while it holds, never arriving early,
  never walking back when a second candidate is tried. AI search now
  defaults on, meaning *use it when there is one to use*.

  ⚠️ **The AI default needed `serde(default = "default_true")`**, not
  the type's `Default`: a missing field takes the FIELD's serde
  default, so plain `#[serde(default)]` would have left every
  existing settings file switched off. Six mutation probes, all
  firing — one invalid on the first attempt (it did not compile) and
  re-run with the trait in scope.

- [x] **The floor** *(v0.14.48)*. Asked for as "gestalte den boden
  schöner". Diagnosed before it was designed: the seams were a hard
  two-texel slot (black chasms), the boards ran thirty metres without
  a joint, and the light pools broke into puddles because the relief
  carried a `value_noise(6, 6)` warp at 0.15 on a tile that repeats
  thirty-six times. Now: eased board edges over a gap that is dark
  and not a hole, platform joints across the boards, each board
  dished its own amount, a long shallow warp — and a **clearcoat**
  over the wood, which is what a sealed stage deck is and what turns
  the rig's spots from matte blotches into reflections. The venue
  floor beyond stopped being a void.

  ⚠️ **The first cut was wrong and only looking showed it**: dishing
  every board alike, with a narrow (therefore steep) chamfer, gave
  one hard rail of light per board — the deck read as corrugated
  plastic. Broadened the chamfer, halved the dish, and gave each
  board its own ageing. Eight mutation probes; one was blind (its
  threshold was looser than the amplitude of the very field it was
  meant to reject) and was replaced by two measures taken against
  the old field first: turns along a board, 3 → 1, and wander across
  a patch of the flat, 0.140 → 0.031.

  ⚠️ **The screenshots were pure black for an hour** and it was not
  the lock and not occlusion: the game window opens at (−1473, −76),
  on a second display that renders nothing, so Bevy had nothing to
  read back. `scratchpad/look.sh` moves the window onto the main
  screen, RE-READS where it actually ended up, refuses to capture
  unless the game is frontmost, and grabs only that rect — the
  version before the re-read photographed a Mail window and I nearly
  reasoned about it.

- [x] **The flashes can follow the song instead of the room**
  *(v0.14.47)*. Asked for as an alternative to the dB threshold, with
  a setting for it: FLASH SYNC = `ROOM LEVEL` (unchanged default) or
  `SONG BEAT`. One `Plan` describes both clocks — a gap, a rolled
  rest, and what each snaps to — so the pacing settled on in v0.14.46
  is literally the same code, only measured in beats: flashes on whole
  beats, bursts on bar lines, rests of one or two bars. The second
  clock also takes the microphone out of the requirement (a chart
  carries its grid), which is what finally gives REDUCED FLASHING's
  swell an edge to rise on when nothing is heard.

  **Measured live**, muted, on the song's clock: 105 flashes in 57
  bursts over a 424-beat song — every one within 0.12 beats of a whole
  beat (median 0.017, i.e. inside a frame), bursts of one, two and
  three, rests of 3.9–8.0 beats, and all 57 starting on a bar line.
  Autopilot on the same run: 293/293 perfect, PASSED. ⚠️ Those are the
  numbers AFTER the count-in fix; the first probe run is what found
  it, and the tests had not.

  Eighteen mutation probes: fifteen fired at once, two showed the test
  could not see the change (both sharpened until it could — one of
  them because the two clocks read the same value at the instant it
  measured), and one could not be made to fail at all because the
  guard it mutated was dead code, which was then removed.
- [x] **The 8-bit note style removed** *(v0.14.44)*. Asked for as a
  development-speed change and it is one: every visual decision was
  being made twice, and the style matrix is where the invisible-stage
  bug lived (two cameras on one window must agree on HDR). Gone: the
  NOTE STYLE row and `round_gems`, `NeckStyle` and every branch on it
  across nine modules, the neon edge fire and hit-flame cones, the
  flat view's square particles and hard tails, the pixel font, and
  `sync_bloom` — both cameras now carry HDR and bloom from birth, so
  that class of bug cannot recur. Losses named in the CHANGELOG: the
  colourblind-safe per-lane shapes, and the flat view's own identity.
- [x] **Highlight on the threshold, white strips, a threshold that
  sets itself** *(v0.14.40)*. Commissioned as an extension of the
  reference rig's dB-Analyse threshold path; BeatByte had none (its
  Room Stage bridge goes the other way — the game's events drive the
  Pi's strip), so the mechanism was built on the monitors' listener:
  a pure duty governor (the reference's rule and numbers), a punch
  envelope on every stage SpotLight (the lamp's own intensity
  remembered on first touch — no fixture retuned), five bar pools
  along the riser edge, the barrier rails and the stacks' corners
  with comet/glimmer effects on hashed schedules. Every rule pinned;
  the strips checked against the geometry they sit on. ⚠️ The
  reference rig itself (raspi5, `stats.html`/`app.py`) was NOT
  touched — see the open questions in the session report.

## Stage realism II — PA, people, light (DONE 2026-09-07, v0.14.37)

Commissioned as "die Grafik und Darstellung während ein Lied gespielt
wird … realistischer, insbesondere die Speaker und die tanzenden
Figuren", with the user's decisions up front: stylized-realistic, the
GH3 / World Tour stage conventions in our own hands, procedural assets
only, one pass with one screenshot set at the end. Plan and the
as-built map: `docs/ui/3d-stage.md`.

- [x] `surfaces.rs` — procedural PBR tiles (tolex, grille cloth, brushed metal, driver cone, stage deck: colour, normal, roughness, mip chains, tangents), format rules pinned.
- [x] `pa.rs` — two full stacks seated on the deck, hardware, amp head, cones that stroke on the beat.
- [x] `figure.rs` + `crowd.rs` — one figure builder (joints, hair, shirts, forward kinematics), 56 dancers in staggered rows with a dance repertoire on the beat and the bar, arms up under Hype, still under STAGE MOTION off.
- [x] `band.rs` retargeted onto the builder; instruments and kit detailed.
- [x] `rig.rs` — real spotlights on the moving-head and backline pivots, fake floor pools removed; the band's warm key; the ambient light fixed onto the camera; shadows on the key light; the neck opted out; every ghost marked.
- [x] Measured (Teen Spirit expert, 2560×1440 window on the M1 Pro). **Judgment identical** in every configuration: 750 perfect / 0 miss / 0 overstrum before and after, in the round style (three runs), the 8-bit style, two players, both pause drills (HDR agreement, one-camera-clears, UI visible) and STAGE MOTION off. **GPU cost, uncapped with the window frontmost:** before median 9.1 ms, after ≈ 10.8 ms (+19 %, inside the plan's +40 % gate). ⚠️ The vsync numbers are NOT a clean measure on this display: ProMotion picks the refresh rate, and the stage that used to fit 120 Hz (8.4 ms flat) now runs at 10.0 ms (100 Hz) with stretches locked at exactly 16.66 ms (60 Hz) whenever the window is not frontmost — a run without the window raised shows 16.66 ms "uncapped" too. Measure uncapped, frontmost, or the number is the panel's, not the GPU's. First fallback if a weaker GPU needs one: `DirectionalLightShadowMap { size: 1024 }`, then one cascade to 22 units, then the far crowd row to `Detail::Simple` (roadmap H4 stays open for the low-end check).
- [x] Looked at (round + 8-bit, t24 / hype frames, stack, crowd and band crops): the first two frames had chrome corner caps blooming into fairy lights under the moving heads and tolex going pale under the blue lamp — both fixed (matte black protectors, near-black vinyl) before the set was accepted.

## AI song graph upgrade (PLANNED 2026-09-05 — docs/plans/ai-song-graph-upgrade.md)

Word-level karaoke from local forced alignment (Track L), then a
better beat grid, melody and structure for charts (Track C). The plan
is the user's document; its five open decisions (§11) are his to make
before L1 starts. L is first: self-contained, measurable against a
public corpus, cannot regress a note.

- [x] **L0 — m4a offset audit** *(2026-09-05)*. Click track through the
  game's own decoder, both paths: `.m4a` decodes late by exactly its
  declared priming (FFmpeg 1024 samples = 23.2 ms, Apple 2112 =
  47.9 ms; Symphonia 0.5.5 parses the edit list and never applies it),
  MP3/WAV/FLAC on the master's timeline, analysis and playback in
  agreement. 70 of 71 library files carry the 1024. **Fixed and
  migrated the same day (v0.14.10, user's call):** both paths skip
  the declared priming sample-exactly, charts carry `audio_trim`,
  and the scan moved 117 chart files 21.3 ms earlier (backups under
  the app data dir) — [`docs/audio/decode-offset.md`](audio/decode-offset.md).
  §11 decided: English first · separation its own switch, no cloud ·
  character fill · per-song lyric offset in the song folder. Next: L1.
- [x] **L1 — `beatbyte-ml` skeleton** *(v0.14.11, ADR-0013)*. Runtime
  `rten` (pure Rust, per-run pinned thread pool), compiled-in registry
  (empty until L2 registers the aligner with its licence), store with
  `.part` + size cap + SHA-256 before rename, session cache, `ml`
  feature on game/cli/app (default off), `beatbyte-cli models
  {list,install,verify,remove}`. 14 tests against a hand-encoded
  MatMul graph and a local web server; four store refusals seen
  failing under mutation; 21 runs bit-identical. README network
  section + `docs_stay_true.rs` name the third outbound path.
- [x] **L2 — aligner, English** *(v0.14.12)*. `beatbyte-lyrics`: own
  CTC Viterbi (self-check: 42 greedy words, all Δ 0.00 s), 60/50-s
  windows stitched on frame boundaries, character spans kept,
  `words.json` + LRC export, `beatbyte-cli align`. `wav2vec2-rs`
  evaluated and set aside (MPL-2.0; candle/`ort` backends). On the
  raw mix: Gotye lines median −0.02 s (56 % within 1 s), Rasmus
  drifts (−18 s) — separation (L6) and gating (L3) are the next
  steps, as planned. Release `models-v1` published 2026-09-05
  (model + NOTICE + Apache-2.0 + SHA256SUMS); `models install`
  verified from it: 378 MB in 14 s, hash intact.
- [x] **L3 — gating and fallback** *(v0.14.13)*. `gate.rs`: verdict
  on the source (no reference / same master / shifted master /
  different edit / failed) from per-line deltas — consensus majority
  within 1.5 s, clean drift = different edit, stamps past the end =
  different edit; sprinted (one frame per letter) and >5 s words
  estimated + retimed; >30 % estimated or an outlier line → line
  level on the source stamp (own span when the source is no
  reference; every line when failed). Confidence floor off by
  default (not discriminative on the mix). 7 tests, 9 mutations seen
  to bite; four real songs each judged as their data deserves.
  Thresholds are assumptions until L5 calibrates them.
- [x] **L4 — renderer** *(v0.14.14)*. `beatbyte-chart` reads
  `words.json` under the LRC caps (wins over the `.lrc`; all-estimated
  lines shown line-timed; `♪` lines are gaps) + per-song offset
  sidecar; the display fills from character spans, has a real line
  end (dim), a lead-in (setting, band grows), the sung-word colour
  step, a four-pulse beat countdown into gaps > 4 s; pause-menu
  LYRIC OFFSET (SONG). `BEATBYTE_SHOT_TIMES` for the harness. Every
  rule pinned pure + mutation-probed; phases photographed on Gotye
  (lead-in 18.4 s, fill 34.5 s, dim 50 s, countdown 62.3 s);
  autopilot 392/392 perfect with the alignment in place. **Not in
  L4:** in-game model download + "realign this song" panel (plan §6)
  — the alignment still comes from `beatbyte-cli align`; filed as L4b.
- [x] **L4b — in-game alignment** *(v0.14.15)*. SETTINGS > LYRICS
  MODEL (download with %, stop, SHA-256, subtitle explains), browser
  `K` aligns/cancels with progress on the status row, one shared
  `align_file` job; releases build with `--features ml` (user
  decision 2026-09-05). Drills `BEATBYTE_AUTOPILOT_ALIGN` (PASSED on
  Aguilera: real K, file written, same verdict as the CLI) and
  `BEATBYTE_AUTOPILOT_MODEL` (PASSED twice under a scratch HOME:
  378 MB each). Vocal separation switch waits for L6.
- [x] Fix `BEATBYTE_AUTOPILOT_DELETE`'s arrow count (library order vs
  the browser's sorted order — the align drill shows the way)
  *(v0.18.36)*. Rows from `BrowserView.order`, counted from the cursor
  in BOTH directions (`arrows_to`, pinned), a settle across the entry
  fade, and the target fixed on the first frame by its SOURCE: asked
  by title, "is it gone?" would still find `[GS] <title>` after the
  original was deleted and never see the deletion it made. ⚠️ And the drill could not have passed at all since Backspace
  became the key that only ASKS: the answer is `Y`, and it still
  pressed Backspace twice. *Verified: 1840 tests (+4), gate green;
  the drill on two throwaway songs, "Delete Drill Probe" and
  "[GS] Delete Drill Probe" — PASSED, exactly the first one gone, the
  second still there; 9 mutations across the two fixes seen to fail.*
- [x] **L5 — eval harness AND the first measurement** *(v0.14.16 /
  v0.14.17)*. `beatbyte-lyrics::eval` + `beatbyte-cli lyrics-eval`
  + the `eval_gates` regression test; **measured on all 79
  JamendoLyrics songs** (see `docs/lyrics/evaluation.md`): AAE mean
  5.30 s / median 1.88 s, PCO@0.1 45.5 %, PCO@0.3 55.9 %, coverage
  100 % — **FAIL on all three gates**, and the reason is one thing:
  20 of 79 songs are LOST in long instrumental passages. Songs whose
  singing is continuous (gap < 10 s) land at AAE median 0.28 s and
  PCO@0.1 59 % — inside the plan's targets. Language barely matters
  (German ≥ English with an English-only model). Nothing was tuned.
- [ ] **Own fixture set** (plan §L5, needs the user): three short
  clips of Martin's own voice + their words, then a click-track
  correction loop — `docs/lyrics/fixtures.md` has the instructions.
  Until then a clone without the corpus measures nothing.
- [~] **L6 separation + multilingual — the runtime is ready, the
  LICENCES are the blocker** (researched 2026-09-05; **the aligner
  accepts a stem since v0.14.29 — ADR-0014 — so a person's own
  separator does the job the game may not ship**). `rten`
  implements everything a separator needs (`STFT`, `DFT`, `LSTM`,
  `GRU`, `ConvTranspose`, `BatchNormalization`,
  `InstanceNormalization`, `Attention`), so the pure-Rust decision of
  ADR-0013 does not stand in the way. What does: **open-unmix `umxl`
  is CC BY-NC-SA 4.0** — unusable in an MIT project we distribute —
  `umxhq`'s weight licence is not stated (and MUSDB18-HQ is itself
  research-only data), and Demucs' README licenses the *code* MIT
  while saying nothing about the weights. No separator with weights
  we may ship has been found yet; MMS-FA carries the same NC risk for
  the multilingual half (the plan flags it).
- [x] **Line stamps as anchors — done and measured** *(v0.14.18)*.
  0 of 79 songs lost (was 20), AAE 5.30 → 0.64 s, PCO@0.3 +10 points,
  with stamps deliberately jittered ±0.5 s. On in the game whenever
  the stamps are structurally plausible. PCO@0.1 barely moved (+4) —
  fine word placement is untouched, and that is what separation is
  for. Numbers: `docs/lyrics/evaluation.md`.
- [x] **The imported library taken through the pipeline** *(2026-09-05,
  `docs/lyrics/library-pass.md`)*: 70 of 71 charts redesigned, lyrics
  fetched for 20 more songs with a length check, all 52 songs with
  lyrics word-aligned. Verdicts: 18 same master, 12 shifted master
  (one was 7.2 s early and had been showing every line seven seconds
  before it was sung), 14 different edit, 8 failed to line level. The
  browser carries `LYRICS` and `CHART` columns for the two states,
  read off the folder — no database, and the write-up says what would
  have to change for one to earn its place.
- [x] **The anchor window follows what is known** *(v0.14.21)*. Swept
  over 26 songs in two conditions: tight (±1 s) wins on stamps that
  are on time and LOSES a song when they are 3 s off, so the width
  now depends on whether the first pass agreed on the offset. Better
  than the old fixed ±4 s in both conditions, never losing a song.
  Numbers in `docs/lyrics/evaluation.md`.
- [x] **A third pass for the derailed songs** *(v0.14.24)*. The
  wide-anchored pass is asked for the offset the unanchored one could
  not see; when it agrees, a tighter third pass runs. PCO@0.1 48.8 →
  50.3 % on time and 47.4 → 48.0 % when the source is 3 s off,
  uncertain words 30.2 → 25.8 %, nothing lost.
- [x] **An alignment is only evidence if the model heard the song**
  *(v0.14.26)*. A user-reported bug: Böhse Onkelz' *Mexico* ran 3.3 s
  early under a confident "shifted master, 85 % agreement". The
  anchored pass centres its windows on the shift it is meant to test,
  so on a mix the model cannot read the words land in the windows and
  the consensus measures the window. The gate now reads the model's
  own greedy letter rate off the emissions it already has, and below
  **1.0 letters a second** an alignment may neither claim a shift nor
  pass its word times off as knowledge. The floor is measured, not
  chosen: median word error on the corpus is 15.46 s below it and
  0.78 s above. Word confidence was the obvious measure and the data
  rejected it.
- [x] **The cheap vocal emphasis — measured and REFUTED** *(2026-09-06)*.
  The plan's own fallback, tried against the population the
  legibility measure identifies (22 of this library's 52 songs and 17
  of the corpus's 79 read below 1.0 letters a second). A 200–3500 Hz
  band-pass makes the model read **less**, not more:

  | song | plain | band-passed |
  | --- | ---: | ---: |
  | Onkelz — Danke für nichts | 0.03 | 0.02 |
  | Onkelz — Mexico | 0.18 | 0.13 |
  | Bon Jovi — Livin' on a Prayer | 0.16 | 0.01 |
  | Nirvana — Smells Like Teen Spirit | 0.49 | 0.34 |
  | Iron Maiden — Fear of the Dark | 0.75 | 0.77 |
  | Toto — Africa (legible control) | 2.03 | 1.63 |

  Clear in hindsight: wav2vec2 is trained on full-band speech, and
  guitar and snare live in 200–3500 Hz along with the voice — the
  filter removes context, not instruments. **And the mid/side half is
  already applied**: `decode_file` averages the channels, so every
  analysis in this project already runs on the mid channel, which is
  the vocal-favouring one. The cheap fallback is exhausted; real
  separation (L6) is the only lever left, and it is blocked on
  weights we may ship, not on the runtime.
- [x] **The library sings word by word — every song with lyrics**
  *(v0.14.29, 2026-09-06, `docs/lyrics/library-pass.md` round three,
  `docs/lyrics/optimizing-a-library.md`, ADR-0014)*. The lever the
  plan reserved for L6 turned out usable today without shipping a
  model: a vocal stem made locally (demucs via pipx, ~40 s a song,
  measured at lag 0 against `beatbyte-cli decode`) and an aligner
  that takes it (`align --vocals`). What the stems then exposed was
  fixed in the pipeline itself, each rule from a real song: an
  anchored pass pushed against the evidence gets a wider window
  (*Mexico*'s "shifted −3.65 s, 85 %" on stamps ten seconds late);
  stamps from another edit are mapped by a robust linear fit and the
  unsung tail dropped (`stretched`; five songs); a held final word
  parked at the next line's onset is retimed; texts with more verses
  than the recording keep their verdict and lose the tail; a
  disagreeing text that ends a minute before the sound is another
  edit's, not a failure; every length rule judges against the sound's
  end (*Mexico*: 168 s of music in 283 s of file). Library: **60 of
  60** songs with lyrics word level (was 33), 0 at line level (was
  27), 5 standing on their own aligned times because no catalogue
  text fits (two 7-minute extended mixes, a remix that sings the text
  twice, two radio cuts of album-length texts). The user judged
  *Mexico* in sync by ear. Also found and fixed on the way:
  `redesign`'s "already current" check had never fired (serde's
  one-ULP float drift) — 25 real hard/expert rollovers kept, 46
  byte-identical ones reverted.
- [x] **Corpus numbers for the stem condition** *(2026-09-06,
  `evaluation.md` "With a vocal stem")*: on the 79 songs the plan's
  three gates pass with a stem and only with a stem (AAE 0.174 s,
  PCO@0.3 90.6 %, PCO@0.1 77.8 % against 0.650 s / 65.8 % / 49.4 %
  on the mix); six seconds of stamp error costs the stem 6 ms and no
  song where the mix derails 24; every song legible (17 cross the
  floor upward, none down); the plain pass on a stem has the best
  fine placement (median 0.082 s, PCO@0.3 94.3 %).
- [x] **The lookup should ask with the sound's length**, not the
  container's (a rip with a silent tail matched the wrong entries)
  *(v0.18.36)*. The loudness report now measures `sounding_s` (the
  last sample any channel sounds above −60 dBFS, one floor shared with
  the aligner as `decode::SOUNDING_FLOOR`), the song document carries
  it (`musical.sounding_s` — declared since ADR-0019, never filled
  before), and `library --catalogue` asks with it where there is one.
  Reports written before the field read it as absent; `loudness --all
  --write` re-measures them.
- [ ] **A text for the five songs no catalogue entry fits**: manual
  (the single's text duplicated for an extended mix, a plain text for
  a remix). And plain texts for the eleven without lyrics — the
  aligner needs words, not stamps.
- [x] **C0 — the tracked grid reaches the chart** *(v0.14.30,
  2026-09-06)*. Found while assessing Track C: the analysis had
  tracked a time-varying grid since Phase 2, and the generator, the
  highway, the phrases, the editor and the sustain ticks all still
  counted on the constant `bpm` — measured 1.61 s apart by the end of
  a live recording (Hotel California 1977, 140.6–152.0 BPM local),
  0.19–0.36 s on studio songs. The chart now carries `grid`
  (`docs/chart-format/chart-format-v1.md`); notes snap to the local
  beat's subdivisions, bar lines follow the drummer, the track's
  tempo map is the grid's, and `redesign` moves carried difficulties
  onto it (≤ 55 ms). No model, no ear gate: a hit moves toward the
  audio or not at all. Library rolled over; numbers in
  `docs/audio-eval-baseline.md` ("The grid reaches the chart").
- [x] **C1 — Beat This! as the meter** *(v0.14.31, 2026-09-06,
  ADR-0015)*. `beatbyte-meter`: the ONNX pair through the game's own
  `rten` runtime (not the `beat-this` crate — that would have carried
  a second runtime and a second audio stack), three registry entries
  re-hosted on `models-v1`, `SongAnalysis.downbeats`, the chart's
  `grid.downbeats` filled and validated, phrases on real bars,
  `downbeat_f` in the harness. A/B on the Rekordbox corpus
  (`docs/audio-eval-baseline.md`, "Downbeats from a model"): loop
  house beat F 0.840 → 0.935, downbeat F 0.736 (fours) → 0.933; all
  11 paired tracks 0.851 → 0.949 and 0.694 → 0.856; three tracker
  losses (0.245, 0.606, 0.880) recovered to 0.96–1.00. Policy = the
  model's whole grid, because the hybrid inherits every level error
  (0.244, 0.623, 0.887) — **adopted only where the model reads the
  tracker's grid** (tempo within 5 %): the library rollover put 41
  folders on the model's grid and kept 23 on the tracker's — mostly
  a metrical level apart (double time on fast rock, 3:2, 3:4), all
  ear-approved at the tracker's level (pure decision, ratios
  pinned; second run writes nothing). Pinned download proven from
  `models-v1`. **Ear gate open** on the 41.
  Driver proven on a hand-encoded pair; nine mutation probes red.
  Off without the models; the built-in tracker and the rock gate
  untouched.
- [x] **C4 — repeated sections charted identically** *(v0.14.32,
  2026-09-06)*. `beatbyte-audio::analysis::structure`: per-beat
  chroma + spectral envelope centred on the song's sounding beats,
  the self-similarity diagonals scanned for ≥ 8-bar runs, snapped to
  bars, the longest non-overlapping pairs kept → `SongAnalysis.repeats`
  (recomputed by the meter when it replaces the grid). The generator
  copies each repeat's first occurrence onto its second at the master
  level, so every difficulty inherits one reading; `repeat_consistency`
  measures it and `redesign` prints it per folder. Synthetic A-B-A-C
  pin finds exactly the planted chorus (and nothing in appended
  silence — the first real run matched a rip's silent tail with
  itself at 1.00). On real songs the pairs read as verse+chorus 1 =
  verse+chorus 2 (Teen Spirit, Africa, The Unforgiven). Library: 62
  of 70 folders have repeats (36 % of the beats); expert consistency
  before the copy 0.25 mean, 0.46 max — the tell in numbers; rolled
  over, rock-gate fingerprints re-recorded. Ten mutation probes seen
  red; autopilot on two rolled-over songs. Ear gate open.
- [ ] C2 stems · [ ] C3 Basic Pitch — each by ear against
  `chart-feel-good-20260826`.
- [x] **Loudness matching + audio-quality warnings** *(v0.14.33,
  2026-09-06, user: "gleiche die lautstärke der tracks an … prüfe
  auf audio qualität")*. Playback, not the graph: EBU R128 meter on
  the played channels (`beatbyte-audio::loudness`, pinned on the
  standard's calibration signals), gain to −16 LUFS under −1 dBTP
  with no limiter, `<audio>.loudness.json` sidecar written by the
  import and `beatbyte-cli loudness`, LOUDNESS MATCH setting,
  quality verdict from the signal (`quality`) in the import line and
  the browser. Library: 20.9 dB spread → 2.4 dB; 48 good / 18 fair
  / 4 poor (the video rips). `docs/audio/loudness.md`.

## Phase 3 — Adaptive charting (DECIDED 2026-08-30, not started)

The architecture is [ADR-0011](decisions/ADR-0011-adaptive-charting.md);
the spec is [`adaptive-charting.md`](adaptive-charting.md). Tasks in
dependency order — each is independently shippable and none may start
before the one it depends on is checked:

- [x] **A1 — Telemetry recorder** *(v0.11.10)*. Per-note session log
  (judgment, signed offset, sustain endings) + session header with
  **chart content hash**, append-only JSONL beside `scores.json`,
  schema v1, autopilot sessions marked. Verified: 7 schema tests (two
  mutated), five autopilot runs each leaving a complete readable log
  (463/463 judged on a real import) with judgment unchanged — and the
  very first log already earned its keep by locating a one-off
  injector overstrum at line 1, during the count-in.
- [x] **A2 — `beatbyte-cli review`** *(v0.11.11)*. Per-section
  accuracy / timing / dropped-sustain / overstrum report over all
  sessions of one (song, difficulty, chart_hash); directives once the
  evidence threshold is met (default 3 sessions, all thresholds in
  one struct, `--min-sessions` exposed). Verified on real logs: four
  autopilot sessions of a real import produced the correct
  `trivially_mastered` directive with `--include-autopilot` and were
  correctly excluded without it. Two green-blind tests caught by
  mutation and rewritten (the mastery veto needed perfect accuracy
  with dropped holds to be exercised at all).
- [x] **A3 — Chart versioning** *(v0.11.12)*. Sibling versions with
  provenance (validated; excluded from the chart hash, with a
  golden-hash pin so no schema change can orphan telemetry as a side
  effect), pointer file the library respects with every failure mode
  falling back to the original, import never overwrites ANY existing
  chart (stronger than the telemetry-conditional wording — simpler
  and strictly safer). Verified live: a hand-made v2 of a real import
  played as 232 events with the pointer and 463 without, while the
  browser showed one entry throughout; scan and import pins mutated
  and seen to fail.
- [x] **A4 — `beatbyte-cli dossier`** *(v0.11.13)*. One
  self-contained briefing per song: active chart (pointer resolved),
  per-bar structure, melody with true held lengths, per-difficulty
  constraints from the generator's own profiles, open directives via
  the same code path as `review`, and the write instructions (next
  version name + parent hash). Workflow in
  `docs/workflow/design-session.md`, bound by a drift test that
  checks every `beatbyte-cli` invocation it teaches against the
  command enum (mutated with a fake subcommand and seen to fail).
  Verified on a real import: 166 bars, 485 melody notes, the open
  directive included, 352 KB.
- [x] **A5 — In-game feedback** *(v0.12.15)*. Results screen: 1–5
  rates the fun, LEFT/RIGHT records worse/better than the parent
  version (offered only when the played chart carries provenance);
  both append to the session log this run just wrote, `review`
  surfaces mean fun + the better/worse tally, and zero friction
  holds both ways (no log = no hint, ENTER always exits untouched).
  Verified: new `BEATBYTE_AUTOPILOT_RATE` drill presses the REAL
  keys and parses the log back — builtin (fun only) and Maria v4
  (fun + versus) both land; four pins mutation-checked, including
  the untagged-serde collision (a field named `o` on the new line
  kind gets mis-parsed as an overstrum — seen to fail).

- [x] **A6 — The blind taste test** *(v0.15.11)*. `T` in the browser
  plays the same thirty-second window twice, on two chart versions,
  in a seeded order and unlabelled; the results screen asks which was
  better (LEFT first / RIGHT second / DOWN neither) and reveals the
  names only after the verdict, which is recorded as the existing
  pairwise `versus` line against the other version's hash. The pair
  is the folder's active version and its parent; both charts are
  cropped to the window so each side runs out rather than being
  stopped, and both get the same run-up. Verified: 15 pins on the
  decisions (the pair, the crop, the run-up, the order, and the
  verdict naming the version the session log names rather than the
  slot it played in), plus a new `BEATBYTE_AUTOPILOT_TASTE` drill
  that presses the real `T`, checks the built test is actually blind
  — two DIFFERENT charts, both sides scheduled, one shared window —
  and answers it on the results screen with a real arrow, requiring
  the verdict to appear in the session log.

Parked with reopen criteria (a real player population, or an explicit
request): population percentiles, automated rollout/A-B
infrastructure, ML preference and skill models, personalization.

## Library browser (DONE 2026-08-30, v0.12.2)

Sorting (S cycles standard/title/artist/genre/length/best), search
filter (`/`, folded matching), and per-row facts: genre, length,
notes, 1-5 density rating, personal best — all per selected
difficulty. Genre lives in the chart format (hash-neutral, validated),
filled from audio tags on import and via `beatbyte-cli set-genre`.

## Browser polish (DONE 2026-08-30, v0.12.4 — docs/ui/browser-polish-plan.md)

User verdict on the new browser was "better, still buggy"; the causes
were confirmed in code, not guessed, and all seven plan items landed:
P1 rows-only rebuild (screen spawns once, status/captions/details
update in place, rows respawn only on order/difficulty change — the
scroll survives typing), P2 hover selects only when the pointer moved,
P3 delete arms on the SONG not the view position, P4 filter typing
selects the first match, P5 the empty result says so, P6 sort mode +
direction persist in settings.json (the filter deliberately not), P7
sort/search actions blip like every other menu key. Backspace repeats
via the OS key-repeat stream. Verified: staged shots (sorted browser,
live search with first-match selection, empty-search hint via new
`BEATBYTE_SHOT_SEARCH`), autopilot on Maria (504/504 perfect), delete
harness on a disposable song, three new pins mutation-checked (one
fixture had to be sharpened — mutant and original coincided at
position 0).

## Difficulty redesign: hard + expert (DONE 2026-08-30, v0.12.5–v0.12.10 — docs/difficulty-redesign-plan.md)

User commission: only medium was a designed difficulty. Measured
diagnosis first (hard: 15/25 songs with zero HOPOs, characterless;
expert: median 106 machine-gun jacks per song, 55-event streams,
almost no chords, no escalation target), then P1–P7: master-level
lane flow turns fast repeated pitches into trills, streams get a
per-difficulty length budget, chords are a percentile of the song's
own accents, hard's HOPO gap matches its real densities, expert
escalates toward the master where the song rises, and
`beatbyte-cli redesign --all` rolled sibling versions over all 25
imported songs (easy+medium carried note-for-note; legacy layout
skipped; per-song revert = pointer; restorable copy in
`~/backups/beatbyte-charts-pre-hardexpert-20260830/`). Verified: 16
new pins, every one mutation-checked; post-rollout measurement
flags ZERO charts (jacks 0, bursts ≤ cap, HOPOs everywhere on hard,
floors respected); autopilot via the new
`BEATBYTE_AUTOPILOT_DIFFICULTY` switch — builtin expert 309/309,
Maria hard 870/870, Immer expert 1064/1064, Lille Vals expert
1963/1963, all perfect. **Open: the by-ear gate** — the user plays
hard/expert and keeps or reverts pointers; Maria's pending v3
listening A/B stayed open (her redesign is v4 on parent v2).

## Live karaoke lyrics (DONE 2026-09-01, v0.13.10 — docs/visual-master-plan.md)

The modern-rendering commission, mapped honestly: most of its
rendering phases already shipped (3D venue stage, HDR/bloom,
budgeted particles, tiered hit feedback, beat-reactive environment
— see the plan doc's inventory table); the real gap was lyrics.

- [x] LRC + enhanced-LRC parser in beatbyte-chart (untrusted-input
  caps, `[offset:]` tag, repeated stamps, word spans; pure cue
  function — deterministic by unit test).
- [x] Karaoke renderer: per-glyph fill on the notes' own visual
  clock, next-line preview, scrim, honest line-timing fallback,
  MC-set handover, multiplayer-safe layout.
- [x] Sources: `.lrc` beside audio/chart; import + watch folder
  carry it, and the lyrics lookup fetches + caches one. (The demo
  song's hand-timed lyrics are gone with the demo song itself —
  2026-09-05, an instrumental must not sing along.)
- [x] Settings: LYRICS, LYRICS SIZE, LYRICS OFFSET.

- [x] **Every line highlights** *(v0.14.38, 2026-09-07)*. Reported:
  "a denial, a denial" (Teen Spirit) never lit. Not lost, not
  mis-timed — DEMOTED: the aligner could not hear the outro, the gate
  fell those lines back to line timing, and a begun line-timed line
  wore the plain text colour (the tone of a word being sung), never
  the amber a highlighted word settles to. Library-wide 563 of 3289
  aligned lines in 58 songs. The line is the unit such a lyric knows,
  so the line lights whole, on its stamp; still no invented word
  sweep. Second, independent defect found on the same path: the face
  folds `ö` → `o` but the words searched for were not folded, so 220
  aligned words in eleven songs lit as a block only once the word was
  over. Both pinned; both pins seen failing under mutation.

Follow-ups (only if wanted): a lyrics editor timeline, automatic
vocal alignment (the data model already carries word spans), a
lyrics position setting.

## Gameplay look, round six: the instrument neck (DONE 2026-09-02, v0.13.24)

User: "passe die grafik so an, dass sie wie in guitar hero 2 aussieht
… erstelle erst einen plan und setze diesen dann um." Plan and
measured status in `docs/ui/gameplay-look-plan.md` (round six). The
attached screenshots arrived empty; the round is grounded in the
documented conventions and a measured capture of the current build.

- [x] R1 neutral neck (dark warm board, pale strings, chrome rails) —
  round style only, gate pinned
- [x] R2 gems as buttons (white centre, bezel ring, saturated cap)
- [x] R3 far end fades (linear fog; formula pinned, venue measured)
- [x] R4 chrome HUD plates in solo
- [x] R5 HIT LABELS setting + lower, smaller word on the instrument neck
- [x] R6 thinner sustains with a pale core
- [x] R7 a typeface for the round style *(v0.13.30, see below)*

Follow-up commission (2026-09-02, "setzt das um"): the three items
left out on purpose, done as the genre's conventions in the house's
own hands — never as anyone's asset.

- [x] **Rock meter (P3)** *(v0.13.26)* — core rule + NO FAIL setting
  (on by default) + fail flow (BOOED OFF! outro, F in red, no record,
  history not completed) + HUD corner (dial + Hype tube, multiplayer
  bars) + `BEATBYTE_AUTOPILOT_FAIL` drill. Solo only can fail.
- [x] **Band figures** *(v0.13.29)* — original primitive-built
  figures on a riser the neck runs into, beat-driven poses (pinned),
  hype-reactive; never over a note. `gameplay/band.rs`.
- [x] **Typeface for the round style** *(v0.13.30)* — Bebas Neue,
  chosen for measured tabular digits; 1.3× on the shared scale; mono
  face kept for karaoke and data text.

P3 in the optimization plan is thereby shipped; P6 (the playtest
pass) is next in that plan's order.

## Search and debug overlay (DONE 2026-09-03, v0.13.32-35)

Small commissions after the look programme, each verified through
the real system rather than by hand.

- [x] Settings rows alphabetical, and pinned that way (v0.13.32).
- [x] A read-only debug overlay during a song (v0.13.33), then on
  `L`, led by a large frame-rate figure, laid out as a table
  (v0.13.35).
- [x] **A broken energy phrase stops showing stars** *(v0.13.40, user
  report: "beim ersten miss sollte die Ansicht der Sterne aufhören")*.
  The genre's rule, researched and cited in the code (WikiHero *Star
  Power*, Clone Hero manual): one miss converts the phrase's remaining
  star notes to standard ones. The drawing asked a time-only test, so
  a dead phrase kept arriving in stars behind its lit band. Now the
  session decides — notes still to come spawn plain, the stars on the
  neck revert on the frame of the miss, the band goes out, the missed
  note keeps its grey, and a rewind re-opens the phrase. *Verified: 6
  tests (3 pure, 1 core, 1 wired ECS swap, 1 rewind), 4 mutation
  probes; live on "Girls Just Want to Have Fun" — at 15.472 s band +
  star note, at 15.873 s the miss counter goes 19 → 20 and both are
  gone; the SECOND phrase at 30.741 s arrives in stars as it should.*
- [x] The song search takes `q` as a letter again; leaving the
  search is a **held** q (one second, a centred bar fills); the
  filter matches every WORD in any column; the field keeps what was
  typed (v0.13.34). Found in the same review and left as a fact:
  `fold_latin` folds Western European diacritics only — a title in
  `š`/`ł`/`ő` is found by its own letters, not by their base form.
- [x] Seen on screen once it unlocked (v0.13.36): the bar, the
  overlay's table and the L key, driven by real key events and
  captured by window ID. The look found two more faults — the kit's
  translucent frame let the rows show through the bar, and an empty
  search kept the previous song's details line and the first
  letter's hint — both fixed and pinned.
- [x] **The search is fuzzy and ranked** (v0.13.37, `search.rs`;
  user: "die suche … ist immer noch komplett buggy … am besten als
  fuzzy matching"). Word scores exact > prefix > inside > one edit
  (four letters up, first letter fixed) > two edits (eight up), AND
  over words, title/artist double weight, the list ordered by score.
  Entering the browser closes the field (it used to stay open after
  a song, every shortcut dead), Esc clears a kept filter before it
  leaves, and a key taken away by focus loss no longer types a q.
  Verified by typing seven misspelt queries into the running game on
  the real library — each found its one song — with the harness
  waiting for the game's own "search: opened" line before typing
  (twice the keys had gone elsewhere: focus, not the game).

## Fire and lightning (DONE 2026-09-03, v0.13.31)

Two-part commission: animate hits with a flame, then make the flames
realistic. Inventory, realism analysis and measures in the session
report; the code carries the reasoning at every constant.

- [x] Hit flame: three nested bodies + flicker + embers + light,
  pure functions pinned and mutation-probed (`gameplay/flame.rs`);
  p99 frame time unchanged at 17–20 ms (display-paced runs).
- [x] Star-Power edge: **lightning arcs** (`gameplay/arc.rs`) — the
  user redirected from a flame sheet mid-round.
- [x] Equal note sizes on the instrument neck.
- [ ] A noise-driven flame shader would be the realism step above
  this (soft volumetric edge, real turbulence); not started — the
  project has no custom shaders, and it is a new asset class.

## Loop-house analysis hardening (Phase 0-2 DONE 2026-09-01, v0.13.20-22)

The commission: make the analysis hold up on sample-based loop
house in DJ format, without regressing rock. Phased, stopping for
approval after each phase.

- [x] **Phase 0 — inventory** (`docs/audio-pipeline-ist.md`, read
  only). Resolved the brief's placeholders from the code and found
  that four of its assumptions do not match the tree: there is no
  beat *tracking* (one global period + one global phase), no
  downbeat stage at all, no SSM/segmentation, and HPSS already
  exists in the melody stage with its percussive half discarded.
- [x] **Phase 1 — measurable harness + baseline**
  (`docs/audio-eval-baseline.md`). MIREX metrics (F, CMLt, AMLt),
  four synthetic property cases, and importers for JSON sidecars,
  Rekordbox XML, and Rekordbox's own ANLZ analysis files. Baseline
  measured against **7 real tracks from the user's library** with
  the grids a DJ has accepted.

**What Phase 1 established, and why it changed the plan.** Tempo is
not the weak point (0.02-0.25 % error, no octave errors). Phase is,
and for two separate reasons: a single global constant tempo drifts
past the +-70 ms tolerance on *every* track measured (worst 18x),
and four of seven lock onto the wrong half of the beat. So the
brief's Phase 3 — "a global fit over (period, phase)" — is what the
code already did; the measurement argued for a time-varying grid
plus an accent cue that could break the offbeat tie.

- [x] **Phase 2 — the grid fix.** A tracked grid (dynamic
  programming, Ellis 2007) and a kick channel (30-130 Hz flux, same
  FFT loop). Mean beat F on the real corpus **0.278 -> 0.840**,
  four of seven at 1.000; drift and offbeat lock both eliminated,
  each verified separately. Rock improved rather than held
  (circuit-breaker 0.000 -> 0.982, fixing the Phase 1 finding);
  nothing with ground truth regressed, so it is the shipped default.
  Rock gate added (`apps/beatbyte/tests/rock_is_unchanged.rs`,
  byte-identical charts). Autopilot PASSED, 98/98 perfect.

**Decisions taken (user delegated, 2026-09-01):** a time-varying
grid was the right call and is now in; `ml-downbeat`/`ort` is NOT
being adopted at all — the measured defect was a phase error on
four-to-the-floor, which the kick channel already in the signal
resolves, and an ONNX runtime is heavy freight for a
one-binary-per-platform project; bit-identical chart output is the
rock gate.
*Superseded on the runtime and the model, 2026-09-06 (user's call, C1):*
the runtime arrived anyway for the lyrics (`rten`, ADR-0013, behind
`ml`), and with it in the tree the Beat This! pair costs one small
crate — measured on the corpus, its grid beats the tracker on the
tracks the tracker lost and supplies the downbeats the tracker never
had (`beatbyte-meter`, ADR-0015). The rock gate stands: the built-in
tracker is unchanged and the reference tracks never see a model.

**Found on the way, not yet fixed:** chart generation is
reproducible per platform but not across platforms — a libm
last-bit difference flips a threshold and moves a note. One of the
two synthesized reference tracks diverges between macOS and Linux. The gate
records a fingerprint per platform and the README says so; finding
the knife-edge comparison is a separate piece of work.

**Phase 3 has a precise target for the first time:** sub-beat
alignment. The whole remaining error is a systematic 70-100 ms
offset on 2 of 7 tracks, sitting just outside the tolerance —
`docs/audio-eval-baseline.md` names the candidates and does not
guess between them. Not started.

## Two Macs, one career (IN PROGRESS 2026-09-25 — ADR-0021, docs/plans/two-macs-sync.md)

The user's commission: BeatByte on this Mac and on a MacBook Pro 2015
(Intel, at most Monterey), with songs and charts, players, scores,
history, achievements, telemetry and shared settings the same on both.
Decided: the raspi5 as hub, one library root in the data directory,
the models synced once, the same name on two devices is one person.

- [x] S0 **Backup before any migration** — `~/Library/Application Support/beatbyte` without `models/` as a tar with its SHA-256, a consistent `.backup` copy of `telemetry.db` beside it (the live file had its WAL), and the SHA-256 of all 3 070 files of the repo's `songs/imported/` (`local/backup-20260925-095346/`, `RESTORE.md` there; `shasum -c` verified).
- [x] S1 **The pure merge crate** *(v0.18.38)* — `beatbyte-sync`, one rule per kind of data, never a blanket last-writer-wins: players (collisions from the old counter, one person under two ids, a remap per side), history (union by start ms + song + difficulty — no new id field, the key is unique; the richer copy wins because the roster adoption rewrote old lines on one device only), scores (the game's own rule: score, then accuracy, then streak), achievements (earliest date), settings (shared keys by newest change stamp, device keys and the API key never), telemetry (insert by `uid`, the finished or longer copy replaces), library (by content, tombstones for deletions, a version regenerated on both devices kept twice under the next free name, the pointer naming the newest version after renames, the older song id kept). Every rule is shown to converge by applying it on both sides. **Player ids** are now `created_ms << 10 | hash(name)` (core), never a counter, and a rename or a new preferred difficulty stamps `updated_ms`. *Verified: 1881 tests (+38 in the new crate, +2 in core), gate green; 31 mutations seen to fail, at least one per rule (one first failed to apply because `cargo fmt` had reflowed its line — a mutation must match before it can prove anything).*
- [x] S2 **Telemetry merge executor** *(v0.18.39)* — `beatbyte-telemetry::merge`: `snapshot` publishes a consistent copy through `VACUUM INTO` (never a file copy under a live WAL), `apply` ATTACHes the other copy and moves the planned sessions in one transaction — events and notes renumbered, `note_context` by its key, the player and song remaps as ONE `CASE` update each (chained updates would remap a remapped id twice), column lists read from the schema so a later column rides along. Four tests on real stores, the mutation probes seen to fail except one equivalent (dropping the old events on a replace: the foreign key's `ON DELETE CASCADE` already does it).
- [x] S3 **Settings stamps** *(v0.18.39)* — `save_settings` diffs against the file on disk and stamps only the SHARED keys that changed (`beatbyte-sync::settings::stamp_changes`), so the game rewriting the whole file on exit does not look like a change; a game test fails for any key the game writes that is neither shared nor device. Four mutations (a key unclassified, every key stamped, a device key stamped, old stamps dropped) seen to fail.
- [x] S4 **One library root** *(v0.18.39)* — `import_dir` is the data directory, always (pinned, and a mutation that brings back the checkout's `songs/` is seen to fail). The repo's 341 folders were moved there by rename on the same volume, `imported-hashes.json` with them; all 3 069 song files match the S0 checksums in their new place. The two folders present in both places were the data directory's OLDER imports (5 and 16 Sep: a bare `chart.json` beside byte-identical audio) against the repo's full folders (v2–v7, sidecars, `song.json`) — the repo's kept, the older two moved to `local/backup-20260925-095346/superseded-data-dir-folders/`. No stored path pointed at the old place (history, settings, every text column of `telemetry.db` searched). The game reads 336 songs from the new root, started from a neutral directory. *Verified: 1888 tests (+7), gate green; three autopilot runs loaded their migrated charts (625 and 334 note events) but ended without a verdict — the window was closed after 18–60 s with nothing in the log, which pointed to the desk rather than the game; repeated after the sync landed: 99 Luftballons Medium PASSED, 334/334 perfect, from a neutral directory against the new root.*
- [x] S5 **`beatbyte-cli sync`** *(v0.18.40)* — against the raspi5 hub (`raspi5:beatbyte-hub`): rsync over SSH, a device-owned folder each (`devices/<id>/`: `snapshot.json` published LAST, `telemetry.db` via `VACUUM INTO`, `library.json`), content-addressed `blobs/<sha256>` (each verified on arrival — a blob that does not verify changes nothing), a `mkdir` lock broken after 30 minutes, the models carried once, refusing while the game runs; `tools/play-synced.sh` syncs around the game. The settings are read where the GAME keeps them (the config directory — the data directory only on macOS). The end-to-end test runs two devices and a hub in a scratch directory with the real rsync and found one defect the pure tests had missed (two merged rosters kept different id counters — `Roster::absorb_counters`, and the roster test now compares the whole file, not only the ids). First real sync of this Mac: 3 074 files, 4.96 GB → 1 839 blobs, 1.4 GB on the hub (the twins share their audio), models 451 MB, counts identical before and after; a second sync 3 s, 0 blobs. **Hardened in v0.18.41** after a review of the pushed code: another device's manifest was trusted as it came, so a path like `../../x` would have been written outside the library — every path, hash, pointer and device name from the hub is now validated and a device with one bad entry is refused whole (6 more mutations caught). *Verified: 1899 tests (+11), gate green; 10 mutations of the sync (API key published, no local tombstones, blob not verified, held lock ignored, stale lock never broken, counters not absorbed, dry run applying, telemetry takes dropped, device keys merged, settings read from the data dir) all caught.*
- [x] S6 **The 2015** *(v0.18.42)* — MacBook Pro 11,5: macOS 12.7.6, i7-4870HQ, 16 GB, Intel Iris Pro + AMD Radeon R9 M370X, 2880×1800. `packaging/macos.sh x86_64-apple-darwin` (minos 11.0, game with `ml` + CLI + launcher) installed to `~/Applications`; signature verifies after the copy. **Smoke** passes on Metal/AMD, the menu in ~5 s. ⚠️ A start over SSH while the 2015's display sleeps HANGS right after the adapter line (0 % CPU, state `U`) until the display wakes — measured: 30 min stuck, continued within a second of `caffeinate -u`; the "4 min 44 s first launch" first recorded here was the same thing, not a system scan. Remote runs go under `caffeinate -disu`. **Autopilot** 99 Luftballons Medium PASSED 334/334 on the 2015, three times (3D, 2D, and during the sync test). **Frame times** (`BEATBYTE_FPS`, AMD, native Retina, window): 3D median ~48 ms (21 fps), p99 ~55–65 ms; 2D the same (~50 ms) — so not the stage; the game uses ~1.2 cores, the GPU is the limit at 2880×1800 → follow-up S8. **Iris Pro: not usable** — its Metal compiler (GPUFamily macOS 1) fails wgpu's indirect-draw validation pipeline (device lost at start, rc 101), and with that switched off (`WGPU_VALIDATION_INDIRECT_CALL=0`) Bevy's own compute and render pipelines; Bevy picks the AMD by default, so this only matters under `WGPU_POWER_PREF=low`. The 2015 reaches the hub with its own key (`id_ed25519_beatbyte`, authorized on the raspi5). ⚠️ This Mac's ssh to the 2015 fails with chacha20-poly1305 ("message authentication code incorrect") — the `mbp2015` alias pins AES. **Calibration** at the machine is still the user's (a tap test needs a person).
- [x] S7 **End to end on both Macs** *(v0.18.42)* — the 2015's first sync pulled everything (2 994 files fetched, 603 sessions / 297 353 events inserted); after two rounds each way the library is identical by hash (3 074 files, one fingerprint on both manifests) and history, scores and achievements are byte-identical. Then offline play on each — the 2015 three autopilot runs, this Mac one — and two syncs each way: before A 391 runs / 604 sessions, B 392 / 606; after both 393 / 607 (A +2, B +1: every run once), no duplicates, further rounds change nothing. **It found three defects the tests had not**: a fresh device's DEFAULTS were stamped as the newest settings and overwrote four of the player's on this Mac (fixed: a key that appears is a default; a pre-stamp value is `LEGACY`; the four restored from the S0 backup), the fresh device selected nobody, and pointers in two spellings were republished as new files. Scores went 86 → 84 on both by the documented rule, not a loss: this Mac held a name-keyed and an id-keyed record for one twin, and the better one (225 714 / 176 199) stays. *Verified: 1903 tests (+4), gate green; 9 more mutations caught (pointer bytes ×4, fresh selection ×2, default stamping ×3).*
- [ ] S8 **Frame rate on the 2015** — ~21 fps at 2880×1800 on the Radeon R9 M370X in 3D AND 2D. A render-scale setting (device key) that renders the world below native resolution and scales up is the lever to measure first; the goal is a steady 60 on that machine.

## Chart editor v3 (IN PROGRESS 2026-09-25 — docs/plans/chart-editor.md)

The editor from M11/E1–E3, grown into a full one: mouse and keyboard
equal, timeline with waveform, playback control, clipboard, warnings,
playtest. Decided defaults: slow-down lowers the pitch (the practice
path), the grid is shown and snapped to but not edited, best scores
stay and older-version ones are marked, the vertical highway stays.

- [x] ED0 **Foundation and data safety** *(v0.18.43)* — the inventory found that the editor saved OVER the file it opened (the active version), that `save_chart_file` was not atomic, that four writers of versions would each bury a hand edit under a new active version, and that `BEATBYTE_AUTOPILOT_EDIT` saved into the player's real library. Now: every chart write goes through `io::write_atomic` (temp file + rename; pinned in the source, since atomicity cannot be seen from outside); a save is a NEW version (`beatbyte_editor::Saver`) with provenance `designer: "editor"` bound to its parent, made active, one version per editing session (later saves rewrite it), nothing written when nothing plays differently, nothing written when the chart does not validate, the analysis sidecar carried where every note still has one; `redesign` (and the browser's `G`), `classic` and a re-import leave a hand-edited active version active (`versions::is_hand_edited`); new invertible ops for star-power phrases (add, remove, set bounds) and for adding an empty difficulty (removing only an empty one); `timecode::Grid` measures from the chart's beat marks — seconds ↔ bar:beat:tick (192 ticks a beat), snap by division, local tempo, extended past both ends of the grid; the library is read again after a save so the browser plays the new version; the drill edits a scratch COPY (real chart checked byte for byte) and takes `BEATBYTE_AUTOPILOT_SONG`. *Verified: 1927 tests (+24), gate green; the drill PASSED on "Heroes" (saved as `chart.v2.json` in the scratch folder, the real chart unchanged); 14 mutations caught (after two invalid or blind probes were sharpened: a non-compiling mutant, and an atomicity that only a source pin can see).*
- [x] ED1 **Timeline and mouse** *(v0.18.44)* — the geometry is pure and tested (`beatbyte_editor::view`: time ↔ screen, zoom around a pivot, lane at x, hit test of head / length tab / empty lane / ruler, box selection, a group move as remove-all-then-add-all in ONE batch so a group may move onto its own old spots, a length drag that never goes negative) and the waveform is a peak envelope computed once (`waveform::Envelope`, 5 ms buckets, a peak of peaks at every zoom). The screen: the song's waveform beside the lanes as a fixed pool of rows (resized, never respawned — following the playhead through a long song costs what a short one does), bar numbers and beat lines from the song's own grid plus the snap division's lines when zoomed in, zoom 30–2400 units a second, every gesture of the mouse (place with snap or Alt-exact, select with Shift, drag to move, tab to lengthen, box, right-click delete, ruler scrub, wheel scroll, Cmd/Ctrl-wheel zoom), a toolbar of chips with their keys, an information block with the stored values, and an F1 reference. ⚠️ Found in the first screenshot: the tint of the keyboard lane (0.035) and the box (0.12) read as SOLID grey and olive — Bevy blends alpha in linear space, the known trap; and the playhead read "bar −1, beat 5" because the grid's bar length was its LONGEST bar (a tracked grid has the odd 5-beat bar) — now its most common. *Verified: 1940 tests (+13), gate green; the drill's mouse half (nine checks through the real pointer code with an injected position) PASSED on "Heroes" and "Africa"; six pointer mutations caught — one only after the drill was fixed: the note was still selected from the length drag, so a box that selected nothing passed.*
- [x] ED2 **Playback** *(v0.18.45)* — scrubbing and following came with ED1; now the loop region (Shift-drag on the ruler, I/O at the playhead, L on/off; an in-point past the end swaps rather than failing) and 100/75/50 % (T). The rules are pure (`beatbyte_editor::playback`: speed cycle, region ordering and minimum length, where the playhead wraps). ⚠️ The song clock KEEPS whatever rate it last had, so every play sets it explicitly (a practice run may have left 0.5), and leaving the editor resets both clock and music to 1×. ⚠️ The first drill measured only the clock and stayed green with the MUSIC at full speed — in the editor the clock runs free after a seek and never learns the audio went elsewhere; the drill now also reads the music thread's own position. *Verified: 1944 tests (+4), gate green; the drill's loop phase on "Heroes" wrapped twice at 50 %, never past the end (4.790 s, end 4.790), clock pace 0.49, music pace 0.52; four mutations caught (no wrap, clock not slowed, music not slowed — after the drill learned to measure it —, wrap late).*
- [x] ED3 **Editing depth** *(v0.18.46)* — pure and tested: the clipboard (`clipboard::Clip`, relative to its first note, all-or-nothing paste, a selection's span), star-power phrases over a span that replace what they overlap (`phrase_over`), the inspector's fields (`inspector`: time in seconds OR bar:beat:tick, lane 1–5, length; a bad value says why), and **undo per difficulty** (`EditorSession`: undo takes the newest step OF THE DIFFICULTY BEING EDITED — sound because steps of different difficulties touch disjoint charts; a step may not mix difficulties; a new edit keeps the other difficulties' redo; switching to a missing difficulty adds it empty as an undoable step). The screen: Cmd/Ctrl + C, X, V, D; the right-click menu (a column, clamped inside the window); `,` `.` `;` typing fields in the information block; Y phrases drawn as a violet band; Q switching. Opening on a difficulty the chart lacks opens on one it has instead of refusing. ⚠️ Three defects found by the drill and the screenshots, none by the unit tests: the chip handler ASSIGNED the frame's action list (any other source that frame lost its action — now appended); the menu's items ran off the panel sideways (a chip bar wraps only at the page width) and then off the bottom of the window; and the drill's injected Backspaces reached the key state a frame after the text field and arrived as Delete — an artefact of injecting after Bevy's input stage, fixed by running the drill between the field and the editor's keys, where real input lands. *Verified: 1956 tests (+12), gate green; the drill (now 17 mouse-and-menu checks: menus on a note and on space, a click beside a menu only closing it, the menu's Delete, copy/paste with length, a typed time through real key events with Enter not also playing, Y, Q leaving the other difficulty untouched, six steps undone exactly) PASSED on "Heroes"; five drill mutations, four caught and one equivalent in the current system order (the chip list assigned instead of appended — nothing else writes to it before the editor reads it).*
- [x] ED4 **Warnings, the quit guard, older bests** *(v0.18.47)* — `beatbyte_editor::lint` (pure): a note under another's held tail on the same lane (a chord on other lanes is fine), stacked notes (< 30 ms), senseless lengths (negative, not a number, over 30 s), before the song, after the MUSIC ends (`AudioData::sounding_end_s`, decoded with the waveform — not the container's length), and "next warning" wrapping round. On screen: red marks beside the lanes and around the note, counts and the next warning in the information block, validation errors counted, W jumps and selects. Quitting (window close, Cmd+Q) with unsaved edits warns once — the one quit handler (`crt::begin_power_off`) asks the editor. Best scores now remember the `chart_hash` they were played on (an optional field: older files and builds do without; a worse run never takes a best's chart, also when the best moves from its name to its song id), and the browser's detail line marks one set on another chart as "older chart" — only for the highlighted song, its chart hashed once per file and forgotten when the library is read again (a hash per row would parse the whole library; the scan often never parses charts at all). *Verified: 1963 tests (+7), gate green; the drill's new checks (a note under a tail warned about and W jumping to it; a real window-close request with unsaved edits only warning) PASSED on "Heroes"; eight mutations caught — one only after the score test learned the name-to-id move, where a worse run could have taken the best's chart.*
- [x] ED5 **Playtest** *(v0.18.48)* — pure and tested: `playback::playtest_window` (the loop when it is on, else the selection, else from the playhead; each with `PLAYTEST_LEAD_S` = 2 s of run-up, never before zero) and `playback::playtest_chart` (the edited chart as it is NOW, the tested difficulty cut to the window — a note before the start would be an unhittable miss, one after the end would keep the test running; the other difficulties and the source untouched). F5 / the chip hands an in-memory `LoadedSong` (no song id) to gameplay, which starts at `Playtest.start_s` through the existing mid-song start (`play_file_from`); `playtest_watch` returns to the editor once the window's notes are done, before any results screen, and Esc in the pause menu does the same. A playtest sets no best, writes no history line and opens no telemetry session (guards in `log_run`, `begin_store_session`); the browser's difficulty and the practice speed are restored, and the editor's state was never torn down. *Verified: 1965 tests (+2), gate green; the drill playtests three notes from 6.08 s on "Heroes" (Easy) and on "Africa" (Expert, 21 of 21), all hit, history and scores unchanged, no telemetry, back at the same playhead and selection; five mutations caught — the last only after the drill learned WHERE the test began (a test from the top of the song still hit every note of its cut chart, so the first form of the drill was blind to it).*

## Backlog (explicitly out of scope until after 1.0)

Not started without a deliberate roadmap edit pulling them forward:

- Practice mode (section looping, speed adjustment without pitch).
- Replays (input-stamp recordings are naturally replayable — the
  architecture already permits it).
- Song-pack distribution format / in-game chart sharing.
- Online features of any kind (leaderboards, netplay).
- Additional themes and theme editor.
- Localization (UI is English-only by decision until then).
- Mod/plugin hooks.

## Known non-issues (documented so they aren't re-investigated)

- Black screenshots in the first seconds of a run: macOS window
  occlusion — frames aren't rendered, nothing is broken.
- `v0.3.0` remains published as a historical milestone prerelease;
  earlier drafts were deliberately deleted (tags remain).
- Menu screenshot in the autopilot set is often black for the same
  occlusion reason; gameplay/results shots are the useful ones.
