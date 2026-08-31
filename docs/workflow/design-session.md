# The design session

How a chart gets redesigned from evidence — layer 3 of
[adaptive charting](../adaptive-charting.md) (ADR-0011). The game is
never involved: it only ever loads chart files.

## The loop, end to end

```text
play → beatbyte-cli review → beatbyte-cli dossier → design →
chart.vN.json + provenance → beatbyte-cli validate → pointer →
play both → THE EAR DECIDES → keep or revert the pointer
```

## 1. Evidence

Play the song. Every session is recorded automatically (layer 1); at
three sessions of the same chart version the review starts drawing
conclusions:

```bash
beatbyte-cli review songs/imported/<song>/chart.json
```

The report says *where* — accuracy, timing spread, dropped sustains
per four-bar section — and emits directives when the thresholds are
met. No directives means no evidence yet; a redesign without evidence
is a guess, and the pipeline exists so nobody has to guess.

## 2. The dossier

```bash
beatbyte-cli dossier songs/imported/<song>/chart.json
```

One self-contained file: the **active** chart (the folder's pointer is
resolved — designing against a superseded version would attach the
wrong parent), a per-bar structure table (onsets, energy, melody
density), the extracted melody with true held lengths, the playability
constraints per difficulty, the open directives, and the mechanical
write instructions — the next version's file name and the parent hash
the provenance must carry.

## 3. Design

A design session — Claude Code, or a human in the editor — produces a
new complete chart file from the dossier. The taste rules are the
distilled philosophy of [`docs/planning/`](../planning/):

- **Musical feel over density.** A note the player does not feel is
  clutter; removing it is design, not loss.
- **Salience over completeness.** Chart what the ear follows — the
  riff, the hook, the accent — not everything the analyzer heard.
- **Pauses are gameplay.** If the music leaves room, leave room.
- **Difficulty is a design, not a filter.** Each difficulty is its
  own reading of the song under its own constraints (they are in the
  dossier), not a thinned copy of expert.
- **A directive names a problem, not a solution.** `low_accuracy` in
  bars 33–40 means those bars need to *play* better — which change
  achieves that is the designer's call.

The new file is a **sibling** (`chart.v2.json`, never an overwrite),
carries `provenance` (parent hash, designer, date, the directive it
answers — the dossier's `write` block has the values), and must pass:

```bash
beatbyte-cli validate songs/imported/<song>/chart.v2.json
```

## 4. The gate

Point the folder at the new version (`chart-active.json`, content
`{"active": "chart.v2.json"}`), play it, then point back and play the
old one. **The ear decides** — ADR-0009 is the precedent: a rework
that measured better on every synthetic metric played worse and was
reverted. Metrics are a guard, the ear is the arbiter, and no layer of
this pipeline may replace that comparison.

Keep whichever version won by moving the pointer to it; the loser
stays on disk. Telemetry keeps the two apart on its own — sessions
bind to the content hash of whatever was actually played. The
verdict itself can be given without leaving the game: on the results
screen, 1–5 rates the fun and LEFT/RIGHT records worse/better than
the parent version; `review` reports both.

## Fresh charts, same rules

`beatbyte-cli generate` and the in-game import produce the *original*
(`chart.json`); re-importing a song whose chart already exists writes
the next version and moves the pointer instead of overwriting —
nothing with evidence attached is ever destroyed.
