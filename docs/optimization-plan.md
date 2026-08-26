# Where BeatByte Goes Next

Written 2026-08-26, after the session in which the chart generator was
rebuilt, reverted, and settled. This is a research-backed proposal,
not a commitment: `docs/ROADMAP.md` stays the source of truth, and
anything adopted here moves into it as a numbered task.

## 1. Honest position

**What is genuinely good already**

- Timing is input-stamp driven and frame-rate independent, with a
  calibration screen and an autopilot that must play every build
  flawlessly.
- Chart generation now produces charts that are fun to play — proven
  the only way that counts, by playing them. That state is pinned
  (tag `chart-feel-good-20260826`).
- Real hardware works: keyboard, gamepads, and a Guitar Hero X-plorer
  through a driver written for it, with a dedicated INPUT TEST screen.
- Two looks, six themes, local multiplayer, an editor, high scores,
  drag-and-drop import of the player's own music.

**What is missing compared with what people actually play**

Clone Hero — the reference point for this genre today — has three
things BeatByte does not, and all three are about *getting better at a
song* rather than about spectacle:

- **Practice mode**: pick a section, loop it, and slow it down (25 %
  to 200 %), with No Fail so learning is not interrupted.
- **A fail state at all.** BeatByte cannot currently be lost. Without
  stakes, a hard passage has no consequence, and without consequence
  a chart has no arc.
- **Feedback precise enough to improve on.** The game says PERFECT or
  GREAT; it never says *late* or *early*, so a player who is
  consistently 40 ms behind has no way to discover that.

## 2. What the research says matters

The game-feel literature is unanimous on one point that BeatByte is
already exploiting and should keep exploiting: feedback must be
**context-appropriate and reinforce the mechanic**, not merely be
loud. "Juice" that does not carry information reads as noise — which
is exactly what the first version of the fret glow did in this
session, and exactly why replacing the halo with a crisp fill was an
improvement rather than a reduction.

The second point is that the satisfying loop in a rhythm game is
*action → readable reaction → sense of mastery*. Mastery needs a way
to see one's own error, which is the argument for practice mode and
timing feedback over any further visual polish.

## 3. Proposals, ordered by value per unit of work

### P1 — Practice mode (high value, medium effort)

Slow a song down, loop a section, no failure. This is the single
feature that turns "I can't play this" into "I can play this now",
and it is the reason Clone Hero players spend hours in it.

Concretely: a speed setting from 50 % to 150 % applied to the audio
*and* the clock (the song clock already owns time, so nothing in the
judgment engine changes); section bounds set from the pause menu;
scores from practice runs deliberately NOT recorded.

Risk: time-stretching audio without pitch-shifting needs care.
Simplest honest first version — playback rate change, pitch moves
with it — is what most rhythm games did for years and is a five-line
change against the existing player. Pitch-preserving stretch is a
separate, larger job.

### P2 — Early/late feedback (high value, small effort)

The judgment popup already knows the signed offset. Showing it — a
small "EARLY"/"LATE" tag, and a drift indicator on the results screen
("you are consistently 32 ms late; consider recalibrating") — costs
almost nothing and is the most actionable information the game can
give a player. It also makes the calibration screen's value obvious
instead of theoretical.

### P3 — A rock meter (medium value, medium effort)

Missing notes drains, hitting fills, empty means the song ends. This
is what gives a chart an arc, and it makes Hype meaningful as a
rescue rather than only a score multiplier. Ship it with a No Fail
toggle in settings, on by default for now — the goal is tension, not
punishment, and the audience is one player who wants to enjoy his own
music.

### P4 — Song browser for a growing library (medium value, small effort)

Seven songs work fine in a list; seventy do not. Needed before that:
search-as-you-type, sort by title/artist/BPM/last played, and the
audio preview the chart format already carries (`preview_start_s` is
computed and stored but never played). Show the best rank per
difficulty on the card so progress is visible while browsing.

### P5 — Revisit chart generation, by ear this time (high value, unknown effort)

The transcription work on branch `transcription-v2` measurably
improved accuracy on synthetic scenes and made the game worse to
play. The most promising untested idea from it is **ranking note
candidates by metric position** (downbeat before beat before eighth)
rather than by loudness alone — the current generator picks the *N
loudest events*, which is why a chart can sit on real onsets and
still feel arbitrary.

The rule going in: A/B it on the player's own tracks, by ear, against
`chart-feel-good-20260826`, before it touches any chart on disk. The
synthetic harness is a regression guard, not a verdict.

### P6 — Finish the open playtest tasks (A1–A4)

Timing windows (30/60/100 ms), HOPO feel, Hype rate and the
difficulty curve have never been reviewed by a human against a
structured script, even though `docs/playtest.md` exists for exactly
that. P2's drift indicator would make this far easier to do honestly.

### P7 — The 3D renderer (H1–H4) — deliberately last

A lit 3D highway with mesh gems is the biggest visual step available
and is already scoped in the roadmap. It is last here because it
changes how the game *looks*, and everything above changes how it
*plays* — and this session's clearest lesson is that a change which
looks better on paper can still be the wrong change.

## 4. What NOT to do

- **No online leaderboards.** They require an account system, a
  server, anti-cheat and replay validation. Clone Hero re-simulates
  every submitted run server-side; that is a project, not a feature.
- **No neural source separation** for charting (see ADR-0009).
- **No more visual polish before P1–P3.** The look is in good shape;
  the gap is in what the game tells the player about their playing.

## 5. Suggested order

```text
P2 (early/late)  →  P1 (practice mode)  →  P4 (browser)
                          ↓
                    P3 (rock meter)  →  P6 (playtest pass)
                                              ↓
                                   P5 (charting, by ear)  →  P7 (3D)
```

P2 first because it is hours, not days, and it makes every later
playtest sharper.
