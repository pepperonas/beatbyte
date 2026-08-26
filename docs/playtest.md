# Playtest Protocol

The 0.x exit criterion is "the tuning settles" — and tuning is judged
by hands, not harnesses. This is the script for a structured playtest
session (~30–45 min). Record findings in the log at the bottom; every
entry needs the build version. Roadmap tasks A1–A4 consume this file.

## Setup (once per device/audio chain)

1. Settings → run **Calibration** (tap along; the median lands in
   `latency_offset_ms`). Note the value here — and if you switch to
   Bluetooth audio, recalibrate and note both values (that pair is
   roadmap **A3**'s measurement).
2. Pick your input (keyboard `A S D F G` + arrows, or a gamepad) and
   note it: timing feel differs per device.

## Session script

Play **both** built-in songs. For each, one run per difficulty, Easy →
Expert ("Circuit Breaker" 128 BPM, "Solder Groove" 92 BPM half-time).
After each run, before looking at the score, answer from feel:

### Timing windows (A2)

- Did hits you *felt* were on-time ever judge Great instead of
  Perfect (window too tight) — or sloppy hits judge Perfect (too
  loose)? Current windows: 30/60/100 ms.
- Misses that felt like hits (and the reverse)? Note the song time if
  you can.

### HOPOs (A2)

- Do hammer-on runs (small gems with bright cores, Hard/Expert) feel
  hittable without strumming? Do pull-offs register?
- Does the HOPO chain ever drop where you kept fretting cleanly?

### Sustains (A2)

- Does holding to the tail's end feel rewarded? Does releasing early
  penalize about right?
- "Solder Groove" Medium carries the most sustains — judge there.

### Hype (A2)

- Does Hype charge at a satisfying rate (25 %/phrase)? Is the 32-beat
  drain long enough to feel powerful, short enough to stay precious?

### Difficulty curve (A4)

- Does each difficulty step feel like ONE step up? Note any cliff
  (e.g. "Hard→Expert on Circuit Breaker doubles note count").
- Known suspicion to confirm/refute: Hard/Expert generate almost no
  sustains (1–2 per song vs Medium's 6) — does their absence hurt?
- Do generated charts *follow the music* — lanes tracking melody
  direction, chords landing on accents? Note passages that feel
  random.

### Presentation sanity (no roadmap task — file new ones if hit)

- Scroll speed 420 px/s default: readable at Expert? (Adjustable in
  settings 240–900.)
- Any judgment popup / feedback moment that reads wrong or late?

## How to record

Append a dated entry below. Bad entries: "felt fine". Good entries
name the song, difficulty, moment, and the direction of the error —
"SG Medium ~0:40: held sustain judged as early release twice" is
actionable; adjectives alone are not.

---

## Findings log

**2026-08-26 · melody charting (G18)**
- User: conversion should capture guitar tones + sustains "perfekt",
  GH-style; difficulty policy decided: tune MEDIUM first, derive
  easier/expert from it. Implemented as master + derivations.
- To judge by ear: do lanes follow the vocal/riff on the imported
  tracks? Do sustains start/end with the actually held notes?

**2026-08-26 · depth view · sustain tails**
- User screenshot: "die langgezogenen töne sind noch verschoben (die
  linie)" — sustain tails stood vertical while the lane leaned toward
  the vanishing point. **Action taken:** tails now connect the gem to
  the projected far-end point on the exact note path (approaching AND
  held); verified by screenshot + collinearity tests.

**2026-08-26 · Guitar Hero X-plorer**
- Real hardware connected through the app's own libusb reader;
  user-confirmed with the Controls-screen fret lamps (red + yellow
  held, both lit). Default bindings matched the guitar exactly.

**2026-08-25 · zweiter Befund · imported tracks**
- Sustains on the Rick Astley import "hat mir gut gefallen" — but the
  live Cyndi Lauper track got none. **Action taken:** sustain
  generation rewritten energy-first (see CHANGELOG); live track
  medium 3 → 51 sustains, both tracks verified flawless.

**2026-08-25 · post-v0.9.0 dev build · keyboard**
- "Töne werden nicht erkannt/zerstört wenn ich Taste drücke, aber
  (Receptors) leuchten auf" — fret presses registered, notes died:
  the strum requirement is invisible and unintuitive on keyboard.
  **Action taken: tap mode is now the DEFAULT** (strumming stays as
  the opt-in purist setting); tap runs record to the scoreboard.
- Round note style alone read as insufficient — "ohne 8-Bit-Modus
  soll das GESAMTE Spiel nicht mehr 8-bit erscheinen." **Action
  taken:** round style now also swaps the pixel font for the smooth
  built-in face, draws bar ("fret") lines on the highway, and renders
  particles/backdrop dots as soft discs.
