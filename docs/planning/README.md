# Planning material — superseded by a decision

The four documents in this directory are iterations of one vision
(written 2026-08-29, 06:03 → 06:24; the last subsumes the rest):
AI-designed song charts, and a closed loop that records every
gameplay interaction and feeds it back into chart regeneration.

They are **source material, not the spec**. The decision that
reconciles them with this repository's reality — local single-player
game, deterministic offline runtime, the ADR-0009 by-ear precedent —
is [ADR-0011](../decisions/ADR-0011-adaptive-charting.md), and the
living spec is [`docs/adaptive-charting.md`](../adaptive-charting.md).

Read these when designing a chart (the philosophy sections are the
distilled taste: musical feel over density, salience over
completeness, pauses are gameplay). Do not implement from them
directly — several of their assumptions (player populations, rollout
cohorts, ML pipelines) do not hold here, and the spec says which
parts were adopted, deferred, or cut.
