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
| [0006](ADR-0006-synthesized-demo-content.md) | Demo songs synthesized at build time | **Amended** — synthesis stays as the test fixture, the songs left the game |
| [0007](ADR-0007-input-abstraction.md) | Actions and bindings, not keys | Accepted |
| [0008](ADR-0008-theme-system.md) | Data-driven stage themes | Accepted |
| 0009 | Automatic guitar transcription engine | **Parked** — see below |
| [0010](ADR-0010-ui-design-system.md) | One UI kit for every menu | Accepted |
| [0011](ADR-0011-adaptive-charting.md) | Adaptive charting: AI designs offline, telemetry decides what to redesign | Accepted |
| [0012](ADR-0012-note-style-boundary.md) | The 8-bit look is data behind the style boundary, not a second renderer | Accepted |
| [0013](ADR-0013-local-ml-runtime.md) | Local ML inference: a pure-Rust runtime, models fetched once on explicit action | Accepted |
| [0014](ADR-0014-vocal-stems-as-local-input.md) | Vocal separation as a local tool the aligner accepts, not a model the game ships | Accepted |
| [0015](ADR-0015-beat-this-as-the-meter.md) | Beat This! as the meter (beats + downbeats), through the runtime we already have; the model's grid over the tracker's, by the corpus | Accepted |
| [0016](ADR-0016-drawing-diagrams.md) | Statistics diagrams drawn in `ui_kit`'s own hand (`plot.rs`), not by a plotting library — the one crate that fits Bevy 0.19 is a second UI toolkit | Accepted |
| [0017](ADR-0017-achievements-derived-not-counted.md) | Achievements re-derived from the whole play log on every pass; only the date each was earned is stored, so a new one unlocks retroactively and none can be taken back | Accepted |
| [0018](ADR-0018-gameplay-telemetry-store.md) | Gameplay telemetry as a versioned local event store: events never frames, nothing derivable stored, the song referenced rather than copied, and the physical input layer opt-in | Accepted |
| [0019](ADR-0019-song-metadata-and-the-library-index.md) | A song's metadata lives in its own folder and the queryable index is a rebuildable projection; a given `SongId` rather than a derived one; playing stays an event, never a counter | Accepted |
| [0020](ADR-0020-classic-rules-in-the-chart.md) | The classic programme: a judgment rule travels in the chart (and its hash), classic levels are derived from the chart's own Expert by deletion only, and chords come only from a polyphonic transcription of the separated stem (`beatbyte-poly`, Basic Pitch) | Accepted |
| [0021](ADR-0021-two-devices-one-career.md) | Two devices, one career: device-owned snapshots on a hub (the raspi5), pure per-data merge rules in `beatbyte-sync` (never last writer wins), device-independent player ids, one library root in the data directory | Accepted |

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
