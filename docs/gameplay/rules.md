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

- **Strum**: `Space`, `↑`/`↓`, a gamepad's strum bar, or the **primary
  mouse button** (keyboard players only — a pad player has a strum bar
  under their hand, and in a two-player game one click must not strum
  for both). Hits the earliest pending note in the window whose frets
  match. Single notes use *anchoring* — only the highest held fret must
  match (lower frets may stay held). Chords require exact frets.
- **Overstrum** (strum matching nothing): breaks the streak, ends any
  sustain, counts no note. The unmatched note stays hittable within its
  window.
- **HOPO notes**: while the chain is alive (previous event hit, nothing
  broken since), a matching fret press (hammer-on) or a release exposing
  a lower held fret (pull-off) hits without strumming. Strumming a HOPO
  always works too — and the strum that lands inside the window of a
  note just hit by fretting (the natural fret-then-pick motion; same
  in tap mode) is that note's strum, absorbed once, never an overstrum.
  A miss or an overstrum kills the chain; a HOPO hit by fretting keeps
  it alive for the next one.
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
- The moment a phrase completes, the meter shows the step: the solo
  tube lights white-hot for 0.35 s and its star crown swells for
  0.25 s (a multiplayer bar thickens instead); a partial or broken
  phrase shows nothing. Age-driven, so phrases in quick succession
  restart the flash rather than stack it. Under REDUCED FLASHING the
  light stays off and only the crown's (halved) swell remains — the
  fill's own rise is the step.
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
