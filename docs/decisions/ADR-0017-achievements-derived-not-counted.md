# ADR-0017 — Achievements are re-derived from the whole history, and only their dates are stored

**Status: Accepted** (2026-09-15, the user's commission: an
achievement system with exactly one hundred meaningful achievements,
some of them secret, an overview of completion, extensible for future
additions, and gamification that keeps a player engaged)

## Context

An achievement system has to answer two questions on every pass —
"has this been earned?" and "how close am I?" — and the obvious way
to answer them is to keep a counter per player per achievement and
add to it as things happen. (That is my own expectation of how such
systems are usually built, not a surveyed fact — no save format was
examined for this decision, and none needed to be.)

This repository had already decided the same question once, for
statistics (`beatbyte-core::stats`, 0.16.0): those are **pure
aggregation over the whole play log**, not incremental counters,
explicitly so that a rule added later evaluates retroactively and
needs no migration. The play log has been append-only since before
players existed, and it already carries one line per run.

The commission adds a requirement that decides it: *adding a new
achievement must not require touching the evaluation logic, and
existing progress must not be lost or become retroactively wrong.*

## Decision

**The catalogue is data and the evaluator is total.**
`beatbyte-core::achievements::CATALOGUE` is one array of one hundred
`Achievement` values, each a title, a blurb and a `Rule` built from a
small closed vocabulary (`Rule`, `Test`, `Metric`, `Facet`).
`evaluate(&log, player)` re-derives every rule from the player's
entire history on every pass and returns one `Progress` per entry.
Adding an achievement is adding a row; only a genuinely new *kind* of
condition touches the evaluator, and then as one more `Rule` or
`Test` variant that every later row can reuse.

**The only thing persisted is `{player: {achievement: unlocked_ms}}`**
(`achievements.json`, beside `players.json`). No counters, no totals,
no progress. `Unlocks::merge` only ever adds, never removes, and
keeps the earliest date.

The date stored is **the start of the run that completed the rule**,
found by walking the history forward until the rule first holds — not
the moment the sweep ran. A catalogue added to after a year of play
would otherwise stamp a hundred unlocks with today's date and tell
the player nothing.

## Alternatives considered

| Approach | Why not |
|---|---|
| **Incremental counters per player per achievement** | The usual shape, and the one the statistics module already rejected for the same reason. A rule added next year starts at zero for a player who earned it two years ago; a counter that drifts from the log has no way to be told it is wrong; and every catalogue change is a migration. Three costs, all avoided by re-deriving. |
| **Store the computed progress alongside the date** | Would save a pass over the log — measured at 259 lines here, which is nothing. It would also make the file the authority on something the log already answers, which is exactly the drift the counters have. |
| **Evaluate only the runs since the last sweep** | Correct for a monotone rule and wrong for the rest: "the same song three times in a row" can be true of a prefix and false of the whole, and "played on thirty consecutive days" needs every day. A partial evaluator would have to know which rules are monotone, which is a second vocabulary to keep in step with the first. |
| **A scripting language for rules** | Maximum flexibility, and it would put untrusted-looking code in a save directory for a feature whose entire job is to be honest about what happened. The closed `Rule`/`Test` vocabulary covers all one hundred entries with fourteen variants. |

## Consequences

**Good.** An achievement added in a year unlocks retroactively, with
the date it really happened. A threshold raised after the fact cannot
take an unlock back, because the store remembers only that it
happened. A corrupt store costs the dates and nothing else — the next
sweep re-derives every unlock. The autopilot is excluded in exactly
one place (`achievements::runs`) and no filter can put it back. The
CLI and the game compute identical answers from identical functions,
because there is only one.

**Costs.** Every sweep walks the whole log, and `earned_at` walks it
once per earned achievement to find the date — O(runs × catalogue) in
the worst case. At 259 runs and 100 achievements that is free and it
runs three times a session (startup, after a song, opening the
overview), never per frame. A history of tens of thousands of runs
would want the dates cached; it is not close.

**A rule can only ask what the log records.** Nine signals were
added to the log for this (Hype activations, phrases, sustains held
and dropped, whether the run failed, the genre, and the three assist
flags), eight of them read by a rule today, and every one is `Option`: a run played before they existed reads
as "not recorded", never as zero. `Test::NoMiss` is `miss == Some(0)`
and not `miss.unwrap_or(0) == 0` for that reason — an achievement may
not credit a run that cannot answer.

## Verification

Pure logic: 22 tests in `beatbyte-core`. Store and screen: 21 more in
`beatbyte-game`, including that a hidden achievement gives up neither
its name, its description, nor its progress bar — checked on the
screen as actually built, its `Text` nodes read back, because this
machine's display was locked for the whole session and every capture
would have been black.

22 of those pins were mutated once each and seen to fail: the
autopilot exclusion, the absent-signal rule, the never-taken-back
merge, the unlock date, the assist exclusion, the conjunction of a
rule's tests, the day streak, the ban on duplicate rules, the ban on
dead `Test` variants, the row-text caps, the covered secret on
screen, play reaching the screen, the two screen switches not sitting
on a menu direction, the rows refusing to wrap, the catalogue's
reference document, both directions of the list rebuild, the roster's
choice not sticking to the screen, where Escape leads, the ordering
behind the log reload, the clamped category press, and the accuracy
comparison.

⚠️ The day-streak pin was **blind on its first probe**: counting runs
instead of days passed it, because no case in it played twice on one
day — the commonest shape a real week has. One further probe was
**invalid** rather than blind — it read a field without comparing it,
so it changed no behaviour; a probe that reports nothing is a
suspect before the pin is.

⚠️ Seven of the 22 come from a re-reading after the feature was first
pushed, and six defects came with them — three of the screen's
controls were inert, the screen kept the last player looked at, and
three screens read the play log unordered against the system that
reloads it. Versions 0.17.1 to 0.17.5 are that re-reading; the
CHANGELOG has each one.

Against real data, three ways that share no code: the game's startup
sweep wrote 27 unlocks to `achievements.json`; `beatbyte-cli awards`
computed the same 27 from the log without reading that file; and an
independent script recomputed a sample by hand and matched every one,
including its date. 22 of the 27 carry historical dates rather than
the day of the sweep, which is the date rule holding on a real log.
The autopilot then played 290 notes perfectly and the store stayed at
27 — the exclusion holds live, not only in a test.
