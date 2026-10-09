# Performance work, 2026-10-08

## Song browser

The release game profiled the real 7,263-entry library on an M1 Pro.
Opening the browser took 825.61 ms in family grouping before the indexed
parent lookup and 4.61 ms after it. The ordering step after the change
took 18.39 ms on entry, and painting the visible row window took 0.06 ms.
The family measurements were taken with `BEATBYTE_BROWSER_PROFILE=1`;
logging was outside the timed sections.

In the deterministic 4,706-entry `order_bench` fixture, filtering for
`o` took 10.35 ms with the old order path and 0.80 ms with prepared
search keys and cached base order. These are compute times, not full
input-to-display frame times.

## Gameplay

PERFORMANCE MODE is enabled by default. It removes 4× MSAA, bloom,
directional shadows and point/spot lights from newly spawned stage
content. The old look is available by turning it off in Settings.

A complete 150%-speed `Maria` autopilot run at 1280×720 in performance
mode passed with 504 perfect notes, 0 misses and a 17.56 ms frame-time
p99 (26,236 measured frames). It is not a matched before/after result:
the older baseline used a different resolution and speed. A full-screen
attempt with the old build was interrupted by live input and recorded
the wrong song; the benchmark now rejects wrong-song and incomplete
samples. The 60 fps goal at native resolution therefore remains
unverified.
