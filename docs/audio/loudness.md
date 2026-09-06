# Loudness matching and audio quality

Every song plays at the same level, and every import is checked for
what its file can resolve. Both are measurements written beside the
audio once; the game reads them and never measures at play time.

## The level

**Target −16 LUFS integrated, ceiling −1 dBTP, no limiter** (the
user's call, 2026-09-06). A song's gain is

```
gain_db = min(TARGET − integrated_lufs, CEILING − true_peak_dbtp)
```

clamped to ±24 dB. A loud master turns down to the target; a quiet
one turns up until its peaks reach the ceiling and no further — it
then plays quieter than the target, the sidecar says `peak_limited`,
and nothing in the signal is changed. The gain rides beside the
MUSIC VOLUME setting (it multiplies it; it never replaces it), and
the setting **LOUDNESS MATCH** (on by default) switches it off for
a listener who wants the files as they are.

The measurement is ITU-R BS.1770-4 / EBU R128 in
`beatbyte-audio::loudness`: K-weighting (a +4 dB high shelf above
~1.5 kHz and the 38 Hz RLB high-pass, derived for the file's own
sample rate the way libebur128 does — the standard tabulates 48 kHz
only), 400 ms blocks at 75 % overlap, the absolute gate at −70 LUFS
and the relative gate 10 LU under the ungated mean; loudness range
per EBU Tech 3342 (3 s blocks, −20 LU gate, P95 − P10); true peak by
4× windowed-sinc oversampling. It runs on the channels the player
plays, through a second decode path that keeps them
(`decode_file_channels`): a mono average of a wide stereo mix reads
up to 3 dB too quiet, and the whole point is to read what will be
heard.

Pinned on the standard's calibration signals: a 997 Hz sine at
−20 dBFS in both channels reads −20.0 LUFS, in one channel −23.0,
at 48 and 44.1 kHz; the shelf lifts 8 kHz by more than 3 dB and the
high-pass drops 40 Hz; a tone followed by silence reads the tone; a
sine at a quarter of the rate sampled at 45° has a sample peak of
0.707 and a true peak of 1.0.

## The quality checks

A container's bitrate is not evidence: a 320 kbps file made from a
96 kbps source looks fine and sounds like the source. So the checks
read the signal (`beatbyte-audio::quality`):

| Check | poor | fair | note |
|---|---|---|---|
| spectrum's end (a cliff of ≥ 20 dB inside 500 Hz above 8 kHz that stays down) | < 15 kHz | < 17 kHz | — |
| sample rate | < 32 kHz | < 44.1 kHz | — |
| bitrate, lossy containers only | < 96 kbps | < 160 kbps | — |
| clipping (runs of ≥ 3 samples at full scale) | > 0.1 % of samples | > 0.01 % | — |
| true peak | — | > +1.0 dBTP | > 0 dBTP |
| DC offset | — | > 2 % | — |
| mono | — | — | always |

The verdict is the worst issue; every threshold is a named constant
and the verdict function is pinned at each of them. A **poor** or
**fair** file is still imported (the user's call) — the import line
says `"Song" imported - audio poor: spectrum ends at 11.0 kHz`, and
the song browser's detail line carries `!! spectrum ends at 11.0 kHz`
(`!` for fair) for as long as the sidecar says so.

## The sidecar

`<audio stem>.loudness.json` beside the audio (the `words.json`
convention), schema `beatbyte.loudness/1`: who measured, the file's
name and size, integrated loudness, loudness range, true and sample
peak, duration, and the quality block (sample rate, channels,
bitrate, bandwidth, clipping share, DC offset, verdict, issues).
The game reads it at scan time for the browser and at song start for
the gain; a sidecar of another schema is ignored, never an error.

Written by the import (in the game, right after the chart) and by
`beatbyte-cli loudness <song|folder|audio> [--all] [--write]`, which
also prints the table and, with `--all`, the library's spread before
and after. `beatbyte-cli inspect` shows the sidecar's line.

## The library, measured (2026-09-06)

`beatbyte-cli loudness songs/imported --all --write`, 70 songs, 1.2 s
each:

| | mean | sd | min | max | spread |
|---|---:|---:|---:|---:|---:|
| as the files are | −12.5 LUFS | 4.6 | −27.0 | −6.1 | 20.9 dB |
| as the game plays them | −16.1 LUFS | 0.4 | −18.4 | −16.0 | 2.4 dB |

Seven songs are peak-limited — three by a whisker (a true peak at
−1.0 dBTP asks for exactly the ceiling), and *In the Air Tonight*
(−27.0 LUFS, a video rip) gets +8.6 of the +11 it would want and
plays at −18.4. Quality: 48 good, 18 fair, 4 poor. The four poor
files are the video rips, and the check that found them is the
spectrum's end: 9.9 kHz (*Through the Fire and Flames*), 11.9
(*In the Air Tonight*), 12.1 (*Dream On*), 13.4 (*Welcome to St.
Tropez*). Of the 18 fair, most are loud masters a few tenths over
0 dBTP on the first pass — which is why that became a note and only
a full dB over is a flag — and a handful end between 15 and 17 kHz.
