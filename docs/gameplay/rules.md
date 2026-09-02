# Gameplay Rules

Implemented and unit-tested in `beatbyte-core` (`session`, `score`,
`timing`). This document describes the *rules*; the code is the
authority.

## Hit windows & judgment

Symmetric windows around each note time (defaults, configurable):

| Judgment | Window | Accuracy weight | Base points |
|----------|--------|-----------------|-------------|
| Perfect | ±30 ms | 1.0 | 50 |
| Great | ±60 ms | 0.75 | 35 |
| Good | ±100 ms | 0.4 | 20 |
| Miss | outside | 0.0 | 0 |

Chords score per lane (a 3-lane Perfect chord = 150 base points).

## Hitting notes

- **Strum**: hits the earliest pending note in the window whose frets
  match. Single notes use *anchoring* — only the highest held fret must
  match (lower frets may stay held). Chords require exact frets.
- **Overstrum** (strum matching nothing): breaks the streak, ends any
  sustain, counts no note. The unmatched note stays hittable within its
  window.
- **HOPO notes**: while the chain is alive (previous event hit, nothing
  broken since), a matching fret press (hammer-on) or a release exposing
  a lower held fret (pull-off) hits without strumming.
- **Note skipping**: aiming past a note hits the matching later note;
  the skipped one misses when its window expires.

## Streak & multiplier

- Streak = consecutive hits; a miss or overstrum resets it.
- Multiplier: ×1 → ×2 at streak 10 → ×3 at 20 → ×4 at 30 (cap).
  The note reaching a threshold already scores at the new multiplier.

## Sustains

- Hold the note's frets to earn **25 points per musical beat** (scaled
  by the current multiplier), converted through the tempo map.
- Releasing early stops the points (no other penalty). Releasing within
  the final **50 ms** counts as completed.
- Overstrumming or hitting the next note ends the tail.

## Hype (special meter)

- Charts define special **phrases**; hitting every note event inside a
  phrase earns **25%** meter. A miss inside the phrase breaks it
  (overstrums don't).
- Activation requires **≥50%** meter and doubles the multiplier
  (up to ×8).
- A full meter drains over **32 beats** of song time.

## Rock meter (the crowd)

- Starts at **50%**. A judged hit adds **2%** (doubled while Hype
  runs — the boost is a rescue, not only a multiplier); a miss takes
  **5%**; an overstrum **2%**. Clamped to 0–100%. First tuning: from
  the middle, ten straight misses fail a run and twenty-five clean
  hits fill the meter.
- **No Fail** is on by default: the meter moves and shows, and the
  song never ends on it. With No Fail off, an empty meter **fails the
  run** — once, latched — and the song ends there with a FAILED
  result that enters no scoreboard.
- Only a solo run can fail. With more than one player the meters
  show but never end the song: one player's bad patch should not cut
  another's song short.

All numbers live in `ScoreConfig`/`TimingWindows` and are data, not
hardcoded rules.
