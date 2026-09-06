# ADR-0014 — Vocal separation as a local tool the aligner accepts, not a model the game ships

**Status: Accepted** (2026-09-06, the user's call after *Mexico* — "sorge
dafür dass das immer so gut funktioniert")

## Context

The alignment measured on the library ([`docs/lyrics/library-pass.md`](../lyrics/library-pass.md))
left 27 of 60 songs with lyrics at line level: 23 whose alignment
failed and 4 whose vocal the model could not read. The corpus placed
the cause ([`docs/lyrics/evaluation.md`](../lyrics/evaluation.md)):
on a dense mix the acoustic model hears too little for a forced
alignment to be evidence at all — *Mexico* read 0.18 letters a second
against a floor of 1.0 — and the plan's no-download lever, a mid/side
plus band-pass emphasis, was measured to make the model read *less*.

The plan's answer is milestone L1/L6, a separator the game runs
itself. Every candidate is blocked on its weights: open-unmix `umxl` is
CC BY-NC-SA 4.0, `umxhq`'s licence is unstated and its training data
research-only, Demucs' README licenses the code MIT and says nothing
about the weights. The runtime side is ready (ADR-0013; `rten`
implements every operator a separator needs). What the project may
*distribute* is the blocker, not what it can *run*.

The user's library, meanwhile, is on a machine that has a separator
installed (demucs 4 via pipx, Apple-silicon GPU, ~40 s a song), and
the user's ask is that every song sing word by word.

## Decision

1. **The aligner takes a vocal stem as input.** `beatbyte-cli align
   --vocals <stem> --separator <label>` computes the emissions from
   the stem; the song still supplies the audio hash, the length the
   stem is checked against, and the length the gate judges by. The
   provenance records the separator. `lyrics-eval --vocals-dir`
   measures the same condition on the corpus. The game does not
   change: the in-game "align this song" stays on the mix.
2. **The separator stays outside the tree.** No weights, no registry
   entry, no download path for it. It is a tool a person runs on their
   own library, documented in
   [`docs/lyrics/optimizing-a-library.md`](../lyrics/optimizing-a-library.md).
3. **The timeline is a contract, checked twice.** `beatbyte-cli
   decode` is the reference for what the game hears (priming
   skipped); the `timeline` example cross-correlates a separator's
   output against it; and the aligner refuses a stem whose length is
   off the song's by more than 15 ms — under one AAC priming, so a
   stem decoded from the container without the skip is caught by
   construction.
4. **A stem alignment replaces a mix alignment only when it outranks
   it** (`--keep-better`: verdict standing, then legibility), so the
   library pass can never regress a song.

## Alternatives considered

- **Ship a separator behind the `ml` feature** (the plan's L1). Rejected
  for now on the licence facts above; the moment a separator's
  weights may be distributed, it becomes a registry entry with a
  pinned hash and this ADR's interface is what it feeds. Nothing here
  forecloses it.
- **The band-pass fallback.** Measured on five illegible songs and one
  legible control: legibility fell on five of six. Refuted, not
  pending (`evaluation.md`).
- **Accept line level for the illegible songs.** Honest, and what the
  gate does without a stem — but the user's standard is word level,
  and the stem gets there on the songs it was measured on.
- **A cloud separator.** Ruled out by the user at the L1 checkpoint
  (ADR-0013: no cloud providers).

## Consequences

**Good.** Songs the model could not read become alignable without any
change to what the game ships or claims; the pipeline's provenance
says which alignment came from a stem; the corpus measures the stem
condition with the same harness as the mix.

**Costs.** A second decode path into the aligner (the stem), guarded
by the length check; the result on a machine without a separator is
exactly what it was. The README's network claim is untouched — the
tool runs locally and the game never calls it — but the library's
word-level count now depends on a tool the repository does not
contain, and the documentation says so.

## Verification

- Timeline: demucs' output (through ffmpeg) measured at **lag 0** at
  16 kHz against `decode` on an m4a with 1024 samples of priming at
  48 kHz and on three corpus mp3s (peak/runner-up 4.1–10.3).
- *Mexico*: mix 0.18 letters a second → stem 2.34; with the text
  mapped onto the recording (ADR-scope: the `stretched` verdict,
  0.14.29), same master, consensus 1.0, judged in sync by ear.
- The corpus numbers for the stem condition are in `evaluation.md`
  under "With a vocal stem".
