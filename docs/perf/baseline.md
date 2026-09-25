# Performance baseline (2026-09-25)

Measured with the bench (`docs/perf/README.md`) at 0.18.50 + the bench
itself (`43745e3` plus uncommitted harness work). Raw results:
`docs/perf/data/`.

## Device B — MacBook Pro 15" mid-2015

MacBookPro11,5, i7-4870HQ, AMD Radeon R9 M370X (the discrete GPU is the
one in use — every result names it), macOS 12.7.6. Full screen at
2880×1752, highest quality (the only quality there is), uncapped.

| Scenario | median | p95 | p99 | max | frames > 16.7 ms | song start |
|---|---|---|---|---|---|---|
| normal (Maria, Medium) | 102.5 ms | 112.6 | 117.7 ± 1.2 | 765 | all | 24–31 ms |
| dense (What You Get…, Expert) | — | — | 120.2 ± 2.0 | 147 | all | |
| hype ("Heroes", Expert) | — | — | 122.0 ± 0.8 | 143 | all | |
| soak (dense × 15 min, warm) | — | — | 123.2 ± 0.9 | 153 | all | |

About 10 fps. The scenario barely matters (±4 %): the cost is not in
the notes. Heat costs about 5 % (soak against a cold run).

### Cost probes (normal, 2 runs each)

Each probe takes one cost out of the frame (`BEATBYTE_PROBE`).

| Taken out | median | Δ median |
|---|---|---|
| nothing | 103.6 ms | — |
| MSAA (4× on both cameras) | 94.8 | −8 % |
| bloom (both cameras) | 92.5 | −11 % (repeat; the first run was disturbed) |
| the directional shadow map | 93.1 | −10 % (p99 noisy in both attempts) |
| **point and spot lights** | **60.8** | **−41 %** |
| all four | 32.9 | −68 % |

### Pixels

| Window | pixels | median |
|---|---|---|
| full screen 2880×1752 | 5.0 MP | 102.5 ms |
| window 2560×1440 | 3.7 MP | 79.8 ms |
| window 1280×720 (one pixel per point) | 0.9 MP | 42.9 ms |
| 1280×720 without lights | 0.9 MP | 42.2 ms |

About 14 ms per megapixel on top of a **floor near 30–40 ms that does
not depend on pixels or lights**. At 1280×720 the lights cost nothing
measurable: their cost is per pixel.

The `hidpi` probe has no effect in full screen (macOS keeps the native
resolution), which is why the pixel table is windowed.

### Findings the bench made on the way

- **The song clock jumped forward** 0.7–1.0 s in a single short frame
  (0.08–0.19 s) three times in one soak run — an audio/clock problem at
  low frame rates, not a harness artefact.
- The autopilot's teleport check compared song time against
  `Time`'s delta, which Bevy clamps to 250 ms: every one-second stall
  counted as a teleport and failed the run. Fixed (wall time).
- `cpu_main_ms` rises with GPU load (p95 54–60 ms at full screen,
  5 ms with the probes on): under back-pressure the main world waits
  for the render world, so this number measures the wait as well as
  the work. Median CPU is 3–4 ms throughout.

## Device A — MacBook Pro 14" M1 Pro

Not measured yet. The machine was under heavy load (load average
19–76 from other work) in every attempt, and a measuring window steals
focus from whoever is working. It needs an hour nobody uses it.
