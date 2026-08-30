# Hard and Expert become designed difficulties

**Executed in full, 2026-08-30 (v0.12.5–v0.12.10)** — P1–P7 landed;
the rollout wrote sibling versions for all 25 imported songs (0
failures, the legacy layout skipped), the post-rollout measurement
flagged **zero** charts (jacks 0 everywhere — down from a median of
106 —, bursts ≤ cap, every hard chart carries HOPO runs, spacing
floors respected), and autopilot played Maria hard 870/870, Immer
expert 1064/1064 and Lille Vals expert 1963/1963 — all perfect.
Restorable copy: `~/backups/beatbyte-charts-pre-hardexpert-20260830/`;
per-song revert = the folder's pointer. The by-ear gate is the
user's.

Diagnosis and plan, 2026-08-30. Commissioned by the user: "currently
there is effectively only medium — implement hard and expert and
build the charts for the current tracks: algorithm first, then
optimize by hand until every difficulty is properly playable."

## The diagnosis — measured, not guessed

Measured over the **25 imported songs' active charts** (script over
event gaps, lanes, chords; jack = same-lane consecutive events under
0.18 s, burst = consecutive gaps under 0.125 s):

**Hard is physically trivial and has no identity.** Median 2.6 notes
per second, minimum gaps 0.16–0.24 s, zero jacks, zero bursts — and
**15 of 25 songs have ZERO HOPOs**, because the profile's
`hopo_max_gap_s` (0.20 s) sits below hard's real gap distribution
(median 0.23–0.37 s). Hard plays as "medium, slightly denser". The
active files also predate the escalation graduation (the v2 rollout
replaced only medium), so hard is uniform even where the song is not.

**Expert is a raw transcription, not a chart.** Median **106
machine-gun jacks per song** — the ContourMapper maps a repeated
pitch to the same lane (interval < 0.5 semitones → step 0), which is
melodically correct and physically brutal at expert's 0.10–0.12 s
spacing. Runs of up to **55 consecutive events under 0.125 s** with
no breathing. Chords are almost absent (median ~20 per ~1000 events;
the absolute `chord_threshold` rarely fires on song-normalized
strengths), so accents don't read. And expert escalates nowhere —
`reduction_chain` gives it no level above, so it is the one
difficulty that ignores the song's own shape.

**Easy and medium are not the problem** and are not touched: medium
is the designed, ear-approved reading (v2); easy derives from it.

## The shape of the fix

Two stages, exactly as commissioned. **Stage 1** teaches the
generator what a human charter knows (all pure, deterministic,
data-driven — tuning lives in `DifficultyProfile`, not code).
**Stage 2** regenerates hard+expert for every imported song as a new
**sibling version** (easy+medium carried over from the active version
unchanged, provenance recorded, pointer moved — per-song revert stays
one pointer away), followed by a per-song optimization pass over the
measured ergonomics. Nothing existing is overwritten; the beloved
medium never regenerates.

## The plan

- **P1 — Lane flow (jack-breaking), master level.** Runs of
  same-lane master notes at gaps < 0.18 s alternate deterministically
  around the anchor lane (trill shape — how real charts write fast
  repeated pitches). Master-level so every difficulty inherits one
  consistent reading and `shared_notes_keep_their_lane_across_
  difficulties` survives by construction. DoD: property test — NO
  difficulty emits same-lane consecutive notes under the jack gap;
  determinism pin; contour test still green.
- **P2 — Burst discipline.** New profile data: a cap on consecutive
  events under the burst gap (expert generous, hard tight). Over-cap
  runs thin their interior to every-second-note (16ths relax to
  8ths), boundaries kept. DoD: property test over generated output;
  caps are data, mutation-checked.
- **P3 — Chords as accents.** Chord selection moves from an absolute
  strength threshold to a song-relative percentile of the kept notes,
  with a breathing gap before every chord; 3-note chords only on the
  very strongest accents (expert). DoD: chord-rate band test on
  fixtures; no chord closer to its predecessor than the breathing
  gap.
- **P4 — Hard gets an identity, expert gets a ceiling.** Hard's
  `hopo_max_gap_s` rises to 0.26 s (HOPO runs actually exist at
  hard's real densities); expert escalates toward the **master's own
  density** in the song's hot bars (the level above expert is the
  transcription itself) and keeps 2.2 npb elsewhere. DoD: measured —
  hard emits HOPOs on real tracks; expert hot bars denser than cold;
  subset invariant tests stay green.
- **P5 — The rollout tool.** `beatbyte-cli redesign <chart|--all>`:
  re-analyzes the audio (deterministic), regenerates **hard+expert
  only**, carries **easy+medium note-for-note from the active
  version**, writes the next sibling version with provenance
  (parent hash, designer, directive `difficulty-redesign`),
  validates, moves the pointer. DoD: unit tests — carried
  difficulties identical to parent, provenance correct, pointer
  moved, original files untouched; legacy layouts without the
  standard `chart.json` name are skipped with a message.
- **P6 — The per-song optimization pass.** The ergonomics report
  (jacks, bursts, spacing floors, chord rate, HOPO presence, density
  vs. energy curve) runs over all 25 × hard+expert; outliers get
  hand-tuned per song (the design-session rules apply: escalate where
  the song escalates, verses breathe, salience over completeness).
  DoD: report clean for every song — zero jacks under the gap, no
  over-cap bursts, spacing floors respected; per-song notes recorded
  in the rollout report.
- **P7 — Playability proven.** New `BEATBYTE_AUTOPILOT_DIFFICULTY`
  switch (documented in the harness reference; `docs_stay_true`
  enforces it). Autopilot runs: both builtins on hard+expert, plus
  the worst-case imports by density (Immer, Two of Hearts, Lille
  Vals expert; Maria hard) — muted, real tracks per the user's
  standing preference. DoD: every run flawless; full quality gate.

Order: P1 → P2 → P3 → P4 (each independently green) → P5 → P6 → P7.

## The gate that stays human

The ear decides, per ADR-0009/ADR-0011: the pointers move to the new
versions so the redesign is what the game plays, and every song can
be reverted individually by pointing back. Maria's pending v3 A/B
stays open — her redesign builds on the **active** version (v2); if
v3 later wins the listening test, its verdict is folded in as a
follow-up version.

## Explicitly not planned

- Regenerating easy or medium anywhere (the ear-approved reading).
- Analyzer/transcription changes (chart-feel-good-20260826 stands;
  ADR-0009).
- Committing `songs/imported/**` (user content stays out of the
  repo; the tooling is the deliverable, the rollout runs locally).
- Difficulty-specific visual/UI work (the browser already shows
  per-difficulty facts).
