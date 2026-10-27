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

### The floor, found (2026-10-27, 0.19.5 → 0.19.6)

Windowed at 2560×1440 (the player's case), PERFORMANCE MODE, uncapped,
normal scenario. Raw results: `mbp2015:~/bench/w-*.json`.

| Build / probe | median | p99 |
|---|---|---|
| installed 0.19.0-beta.1 | 45–50 ms | 137–151 |
| 0.19.5 (bar lines share materials, no idle writes) | 26 | 33–43 |
| 0.19.5, stage pinned to 85 / 70 / 55 % | 25 / 24 / 23 | 32 / 31 / 37 |
| probe `hdr` (no HDR on either camera) | 27 | 44 |
| probe `distfog` | 27 | 35 |
| probe `unlit` (no PBR, normal maps, clearcoat) | 24 | 39 |
| probe `blend` (no blended or additive meshes) | 26 | 33 |
| **probe `gpupre` (CPU-built instance buffers)** | **12** | **18** |
| probe `nostage` (lower bound: HUD only) | 8 | 14 |
| 0.19.6 build, live governor (later series) | 15–16 | 29–33 |
| the same build, GPU preprocessing forced on | 22 | 36 |
| 0.19.6 build with vsync | 16.6 (60 fps) | 24 |

The later series ran the shipped build after two hours of benching;
it is 3–4 ms slower than the `gpupre` probe was, which ran at full
scale on the 0.19.5 binary. The controlled comparison is the pair
above it, same binary, same session: 22 → 15–16 ms. Whether the rest
is heat or the build, the next series alternates the two.

### 0.19.7 against 0.19.6 (2026-10-27, alternating, stage pinned at 100 %)

Windowed, uncapped, "Maria" medium, `BEATBYTE_STAGE_SCALE=0`, two rounds
of old → new → new with GPU preprocessing on. Raw: `mbp2015:~/bench/d-*.json`.

| Build | median | p99 | max | CPU main median |
|---|---|---|---|---|
| 0.19.6 | 17.1 / 14.8 | 30 / 31 | 149 / 44 | 7.3 / 6.1 |
| **0.19.7** | **14.1 / 12.3** | 33 / 20 | 66 / 57 | 6.1 / 4.8 |
| 0.19.7, GPU preprocessing on | 34.7 / 26.1 | 47 / 33 | 81 / 60 | 5.0 / 3.6 |

The per-frame cleanup takes 2.5–3 ms (about 17 %) off the median and a
little off the CPU; the 99th percentile moves too much between rounds
to read. The same series found 0.19.7's live runs and both autopilot
runs quitting on the step back to full scale — fixed in 0.19.8.

⚠️ **Withdrawn (0.19.9): the "floor" was not found.** 0.19.6 turned
Bevy's GPU mesh preprocessing off on Intel Macs and the frame time
dropped by ~14 ms — because the flag switches only the mesh uniform
builder while cameras and render phases stay on the GPU path, and with
the two out of step most meshes were not drawn: gems, receptors,
strings, crowd and band were missing on the 2015 (engine screenshots,
2026-10-27; the player saw an empty highway). Every 2015 number from
0.19.6 to 0.19.8 that ran with it off measures that incomplete scene.
With the full scene (0.19.9, "Maria" medium, windowed, uncapped, two runs) the 2015 measures median 20.7 ms (~48 fps), p99 32.5–33.0 ms, worst frame 48.6 ms; main-world CPU median 2.7 ms.

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
