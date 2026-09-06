# ADR-0015 — Beat This! as the meter, through the runtime we already have

**Status: Accepted** (2026-09-06, the user's call: "Punkte 1 bis 3" of
the song-graph assessment — downbeats via Beat This!)

## Context

The chart had bar lines that were never measured. The spectral
analyzer tracks beats (Phase 2, Ellis DP over a kick-weighted flux)
but knows nothing of bars, so every consumer counted four beats from
the first one: the phrases, the highway's bar lines, the editor. On
the Rekordbox corpus that guess hits the DJ's downbeats with F 0.736
on loop house — and on rock and pop, where the first tracked beat is
rarely a "one", far less. The tracker itself also loses tracks
outright (a half-beat lock: F 0.245 on one house track).

The 2026-09-01 decision had set an ML downbeat stage aside: the
defect measured then was a phase error the kick channel fixed, and an
ONNX runtime was "heavy freight". Since then the runtime arrived for
the lyrics (ADR-0013: `rten`, pure Rust, behind `ml`, models fetched
once on explicit action). With it in the tree, a beat/downbeat model
is one small crate and two registry entries.

*Beat This!* (Foscarin, Schlüter & Widmer, ISMIR 2024) is the current
state of the art in beat and downbeat tracking; its code and its
published weights are MIT (JKU Linz); the Rust port `beat-this-rs`
exports the checkpoints to ONNX and runs them on `rten`.

## Decision

1. **A new crate, `beatbyte-meter`**, drives the pair — the log-mel
   front end and the beat model — through `beatbyte-ml`'s pinned
   runtime: 22 050 Hz mono in, the reference's chunking (1500 frames,
   6-frame borders, keep-first), the reference's minimal peak picker,
   downbeats snapped onto beats. Its pure parts are pinned; the
   driver is proven end to end on a hand-encoded ONNX pair through
   the real runtime, so the repository still carries no weights.
2. **Not the `beat-this` crate as a dependency.** It pins `rten` 0.24
   (ours is 0.26 — two copies of the inference engine), `symphonia`
   0.6 beside our 0.5 and `rubato` for its own decode and resample;
   we already decode (priming skipped, on the game's timeline) and
   resample (the aligner's windowed sinc). The 400 lines we needed
   are the chunking and the peak picker, and they are better owned
   than imported with a second audio stack.
3. **Three registry entries** (`beat-this-mel`, `beat-this-small`,
   `beat-this`), re-hosted unchanged as assets of the `models-v1`
   release with a NOTICE, pinned by size and SHA-256 like the
   aligner's model. `beatbyte-cli models install` is the only way
   they arrive; `installed()` prefers the full model.
4. **The policy is the corpus's** (`merge::Policy`, measured in
   `docs/audio-eval-baseline.md`, "Downbeats from a model"): the
   model's whole grid replaces the tracker's, tempo re-read from its
   median interval — not only its downbeats snapped onto our beats,
   because where the tracker has locked onto the wrong phase its
   beats are the problem and no downbeat placement on them can be
   right.
5. **…and only at the same metrical level** (`merge::apply`). The
   library said what the corpus could not: on four fast rock songs
   the model hears double time (230.8 against the tracker's 112.5
   BPM, bars of 1.06 s against 2.13 s) and on a shuffle a 3:4, all
   on charts the user's ear had approved at the tracker's level.
   Neither reading can be called wrong from here; what is certain is
   that a downbeat at the other level is a half-bar or a bar and a
   half. So the model's answer is adopted when its tempo is within
   5 % of the tracker's — every corpus win is within 2 %, the
   library's nearest disagreement is 6 %, and `redesign` merges
   within the same 5 % — and the chart otherwise keeps the tracker's
   grid, counts bars in fours and says so on stderr. The decision is
   a pure function with the measured ratios pinned. The library after
   the rollover: 41 folders on the model's grid, 23 kept (19 a level
   apart, two at 6 % and 11 %, two more from a shuffle and a 3:4), a
   second `redesign --all` writing nothing.
6. **Never a reason to fail.** Without the models the analysis is
   the analyzer's, bit for bit; a model that fails to run is a line
   on stderr and the same. The built-in tracker is unchanged and the
   rock gate's reference tracks never see a model.

## Alternatives considered

- **An own downbeat stage** (accent/energy periodicity over the
  tracked beats). Cheap to ship, but the literature is unambiguous
  that hand-made downbeat features lose to learned ones by a wide
  margin, and the corpus would have had to prove a hand-made one
  before it could ship — the same corpus that already says the model
  is at 0.93.
- **Only downbeats from the model, beats from the tracker** (the
  hybrid policy). Measured: worse than the model's grid wherever the
  tracker had lost the level, and never better where it had not.
- **Keep counting in fours.** What shipped until now; measured, and
  it is what the model is compared against.

## Consequences

**Good.** Bar lines from the music, on every import with the model
installed; the tracker's outright failures recover. The chart
format needed nothing new — `grid.downbeats` was reserved for
exactly this.

**Open.** Which level is right on fast rock is a listening
question the corpus cannot answer (it has no rock with truth); the
rule keeps the ear-approved level until it can. A rock corpus with
downbeat truth would settle it and might widen the rule.

**Costs.** ~15–30 s per song on an M1 Pro (full model, under load)
on top of the analysis; 94 MB of models the user chooses to install;
one more crate behind `ml`. The tracker's grid and the model's grid
differ by a frame or two on steady tracks (F 0.99 against 1.00) —
under the 70 ms tolerance and under a hit window. Charts imported
with the model differ from charts imported without it; `redesign`
carries the ear-approved difficulties onto the new grid as it did
for the tracked one.

## Verification

- Loop-house corpus, 7 tracks (Rekordbox truth): beat F 0.840 →
  0.935, downbeat F 0.736 (fours) → 0.933 (model grid), hybrid 0.827.
- Every paired track, 11: beat F 0.851 → 0.949, downbeat F 0.694 →
  0.856, hybrid 0.752; two more tracker losses (0.606, 0.880)
  recovered to 0.956 and 0.965. One track where every condition
  scores 0 on downbeats at 0.97+ on beats (the "one" is disputed,
  not the grid). Tables in `docs/audio-eval-baseline.md`.
- The pinned download: `models remove beat-this-small` then `models
  install beat-this-small` from the `models-v1` release, hash intact.
- Driver test on a hand-encoded pair over three chunks; fourteen
  mutation probes (grid filter, validation, phrase bars, both merge
  policies, peak window, chunk pull-back, keep-first, borders, the
  merge guard's ratio, the level check skipped and its tolerance at
  2.0, 0.15 and 0.01) each seen red.
