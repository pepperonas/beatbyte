# Architecture Decision Records

Each ADR records one decision that had real alternatives: what was
decided, what else was on the table, and what it costs. They are
written when the choice is made and **updated rather than silently
contradicted** — if a decision is reversed, the ADR says so.

| # | Decision | Status |
|---|---|---|
| [0001](ADR-0001-rust-and-bevy.md) | Rust and Bevy as the foundation | Accepted |
| [0002](ADR-0002-workspace-architecture.md) | Six-crate workspace with one-way dependencies | Accepted |
| [0003](ADR-0003-chart-format.md) | JSON chart format, versioned | Accepted |
| [0004](ADR-0004-gameplay-timing.md) | Input-stamp-driven timing and judgment | Accepted |
| [0005](ADR-0005-audio-architecture.md) | Playback thread, song clock, analysis pipeline | Accepted |
| [0006](ADR-0006-synthesized-demo-content.md) | Demo songs synthesized at build time | Accepted |
| [0007](ADR-0007-input-abstraction.md) | Actions and bindings, not keys | Accepted |
| [0008](ADR-0008-theme-system.md) | Data-driven stage themes | Accepted |
| 0009 | Automatic guitar transcription engine | **Parked** — see below |
| [0010](ADR-0010-ui-design-system.md) | One UI kit for every menu | Accepted |

## The gap at 0009

ADR-0009 exists, but on the `transcription-v2` branch rather than on
`main`. The transcription rework it describes measured better on eight
synthetic scenes and played *worse* — it moved 59 % of note positions
and ignored the beat structure a listener feels. It was reverted, and
the branch keeps both the work and its ADR.

The number is left unused on `main` so that the branch can be merged or
revived without renumbering. If it is abandoned for good, this row
should say so rather than the number being reused.

The episode is itself a decision worth remembering: **the synthetic
harness is a regression guard, not a verdict on chart quality.** Any
change to transcription is A/B'd by ear against the tag
`chart-feel-good-20260826` before it touches a chart on disk.

## Writing a new one

Copy the shape of a recent ADR: Context (what forced a choice),
Decision, Alternatives considered (with the reason each was rejected —
this is the part that stays useful years later), Consequences split into
good and costs, and Verification.

Number sequentially. An ADR that only records what was built, without
naming what was not, is a summary rather than a decision record.
