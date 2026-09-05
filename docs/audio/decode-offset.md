# Decode offset audit (plan L0)

Measured 2026-09-05 on macOS with the workspace's own decoder stack
(rodio 0.22.2 over Symphonia 0.5.5). **Fixed the same day (v0.14.10):**
both decode paths now skip the declared priming, and every chart from
before carries a marker after a one-time move — see *What was done*
at the end. The measurement below is the state BEFORE. Asked for by
[`docs/plans/ai-song-graph-upgrade.md`](../plans/ai-song-graph-upgrade.md)
§1.3 and milestone L0: does a lossy container's encoder delay end up
in the decoded timeline, and do analysis and playback agree?

## Method

A click track — one full-scale sample at exactly 1.000, 2.000 … 10.000 s
in an 11-second 44.1 kHz mono WAV — encoded with every encoder on this
machine, then decoded through the game's analysis path
(`beatbyte_audio::decode_file`) and, separately, appended to a rodio
`Player` on a headless mixer and pulled through it (everything the music
thread does short of the device). The number reported is the
cross-correlation lag of the decode against the original: a lossy codec
smears a single-sample click over its transform window, so the peak
sample alone would mislead; the correlation peak survives the smear.

Tool: `crates/beatbyte-audio/examples/click_offset.rs`.

```bash
cargo run -p beatbyte-audio --example click_offset -- --write /tmp/click.wav
afconvert -f m4af -d aac -b 128000 /tmp/click.wav /tmp/click-apple.m4a
ffmpeg -i /tmp/click.wav -c:a aac -b:a 128k /tmp/click-ffmpeg.m4a
lame -b 128 /tmp/click.wav /tmp/click-lame.mp3
ffmpeg -i /tmp/click.wav -c:a libmp3lame -b:a 128k /tmp/click-ffmpeg.mp3
flac -o /tmp/click.flac /tmp/click.wav
cargo run -p beatbyte-audio --example click_offset -- /tmp/click.wav /tmp/click-apple.m4a …
```

## Result

| File | Encoder | Analysis path lag | Playback path lag | Container says |
| --- | --- | ---: | ---: | --- |
| `click.wav` | — | 0 samples (0.00 ms) | +430 (+9.75 ms) | — |
| `click.flac` | flac 1.5 | 0 | +430 | — |
| `click-apple.m4a` | `afconvert` (Apple AAC) | **+2112 (+47.89 ms)** | +2542 | `iTunSMPB` priming `0x840` = 2112, padding 212 |
| `click-ffmpeg.m4a` | `ffmpeg -c:a aac` (FFmpeg 8.1) | **+1024 (+23.22 ms)** | +1454 | `elst` media_time 1024 |
| `click-lame.mp3` | `lame` 3.100 | 0 | +430 | LAME header delay, applied |
| `click-ffmpeg.mp3` | `ffmpeg -c:a libmp3lame` | 0 | +430 | LAME header delay, applied |

(Ogg Vorbis could not be produced: this FFmpeg build's `vorbis` encoder
refuses mono and has no `libvorbis`; the existing stereo `tone.ogg`
fixture still covers decoding.)

Three findings, in the order the plan asked them:

1. **Where the first impulse lands.** WAV, FLAC and both MP3s decode on
   the master's timeline. Every `.m4a` decodes **late by exactly its
   declared priming**: 1024 samples for FFmpeg's encoder, 2112 for
   Apple's. The priming samples are decoded and delivered as audio (the
   files come back 486400 and 487424 samples long instead of 485100).

2. **Does Symphonia's MP4 demuxer apply the edit list / `iTunSMPB`?**
   No, in 0.5.5. `symphonia-format-isomp4` parses `edts`/`elst`
   (`src/atoms/elst.rs`) but the `ElstAtom` struct is
   `#[allow(dead_code)]` and nothing in the demuxer reads it; `iTunSMPB`
   is not referenced at all. rodio's decoder builder enables gapless by
   default (`DecoderBuilder { gapless: true }` → Symphonia
   `enable_gapless`), which is why the MP3 path is correct: the MP3
   decoder honours the LAME delay under that flag, the MP4 demuxer has
   nothing that would.

3. **Analysis and playback agree.** Both construct the same
   `rodio::Decoder::try_from(File)` (`decode.rs::decode_file`,
   `playback.rs::play_file`/`crossfade_to_file`), so the chart a song
   gets and the audio it is played against carry the same shift — the
   game is consistent with itself, and gameplay judgment is not affected.
   The playback path additionally shows a **constant +430 samples
   (9.75 ms) for every format**, WAV included: rodio's `Player`/queue
   pipeline between the position counter (`get_pos`, which the song
   clock reads) and the mixed output. Format-independent, so it is
   exactly what the latency calibration screen measures and absorbs;
   not an `.m4a` matter and not a defect of this audit's kind.

## What this means for the library

Metadata survey of the 71 `.m4a` files in the reference library (box
walk only, no audio read): **70 declare `elst` media_time 1024**, one
declares 0. Evidently FFmpeg-side encodes (several folder names carry
YouTube ids). Their tracks are 48 kHz, so the in-game timeline of nearly
every imported song ran **21.3 ms behind the master's** (1024 / 48000 —
the 23.2 ms in the table above is the same 1024 frames at 44.1 kHz).

Consequences:

- **Not for gameplay.** Chart and audio are shifted together.
- **For anything timed against the master.** lrclib's LRC lines, a
  `.lrc` from another tool, and the word alignments Track L will produce
  *if they are computed on a correctly trimmed decode*, all sat 21–23 ms
  (Apple: 48 ms) early relative to what the game played. At line level
  nobody sees it; at PCO@0.1 s it is a quarter of the budget.

## What was done (v0.14.10)

The proposal below was carried out as written, on the user's decision
("jetzt fixen, Charts migrieren"):

1. `beatbyte_audio::priming::container_priming` reads `iTunSMPB`
   (Apple) or the audio track's `elst` (everyone else) with untrusted-
   input caps; `decode_file` skips that many frames, and the music
   thread appends the same file through `playback::open_trimmed`, a
   sample-exact `SkipFrames` source. ⚠️ Not rodio's `skip_duration`:
   it converts the span back to samples through nanoseconds and
   truncates — 1024 frames at 44.1 kHz come out as 1023 — and the
   entire point is that both paths skip the *same* count. Re-measured
   with the audit example: every fixture now lags **0** on the
   analysis path and the constant +430 on the playback path.
2. Charts carry `audio_trim` (`docs/chart-format/chart-format-v1.md`).
   The three writers (import, `beatbyte-cli generate`, `redesign`)
   stamp it from the decode.
3. The library scan moves every chart file without the marker —
   every version, not only the active one — by `−priming` and writes
   the marker, atomically, after copying the original to
   `<app data>/beatbyte/migrations/audio-trim/<absolute path>`.
   First run on the reference library: **117 chart files moved
   21.3 ms earlier, 3 marked (WAV/MP3), 26 left alone** (25 version
   pointers and `imported-hashes.json`, which are not charts). A
   hand-judged expert chart checked field by field: 970 notes, every
   time −21.333 ms, lanes/HOPO/lengths/phrases/provenance untouched.
   ⚠️ The first run keyed backups by song-folder name, and one folder
   name existed in both scan roots — the second original was
   rewritten without a backup. Fixed (absolute-path keying, pinned by
   a test that fails on the old keying), the missing original
   reconstructed by the exact inverse (no note had been clamped), and
   the backups re-laid under absolute paths.
4. **Consequence for telemetry:** a moved chart hashes differently
   (its times are different), so sessions recorded before v0.14.10
   stay bound to the old content — correctly, they were played on
   the old timeline. A WAV/MP3 chart keeps its hash: the marker alone
   is excluded from it.
5. Noted, not changed: loading a chart through `serde_json` without
   its `float_roundtrip` feature can move a float by one ULP (2 of
   970 sustain lengths differed in the 16th digit after the rewrite,
   2e-16 s). Pre-existing on every load, invisible in play, and left
   alone here because enabling the feature would itself re-hash
   charts.

## Proposal, as it stood before the fix

1. `beatbyte-audio` gets a small MP4 box walker that reads the priming
   from the container — `iTunSMPB` when present (authoritative for
   Apple files), else the first `elst` entry's `media_time` — with the
   same untrusted-input discipline as the chart reader (size cap, depth
   cap, no allocation from declared sizes). ~80 lines, pure, tested on
   the two fixtures.
2. One shared `open_decoder(path) -> (Decoder, priming_samples)` used by
   `decode_file` **and** by the music thread, skipping `priming_samples`
   frames on both. Both or neither: skipping in analysis alone would put
   every new chart 23 ms ahead of the audio it is played against.
3. **Migration, and this is the part that needs a decision.** Existing
   charts were generated on the padded timeline. After the fix their
   notes would land 23 ms (48 ms) late against corrected playback. The
   redesigned hard/expert charts in the library are hand-judged work,
   not regenerable, so a blanket re-import is out. Proposed: the chart
   file records the priming its audio was decoded with
   (`"decode": { "priming_samples": 1024 }`, written by import and by
   `beatbyte-cli generate`); a chart *without* the field is treated as
   generated on the padded timeline and its times shifted by
   −priming/rate on load. Lenient reader, deterministic, nothing on disk
   rewritten.
4. Alternatively, take a Symphonia release that applies edit lists,
   if and when one ships through rodio (not checked upstream — 0.5.5 is
   what the lockfile has). Same migration question either way: the
   timeline changes the moment any decoder starts trimming.

## Fixtures and tests

`crates/beatbyte-audio/tests/fixtures/click-{apple,ffmpeg}.m4a` and
`click-lame.mp3` (the 4-second, 3-click variant, `--short`, 64 kbit/s),
recipe in the fixtures README. Tests in
`crates/beatbyte-audio/tests/decode_formats.rs` pin that every
container decodes on the master's timeline (lag 0) and that the decode
reports what it skipped; `priming.rs` pins the declarations themselves
(2112 / 1024 / none) and that garbage is "none", never a panic;
`playback.rs` pins the sample-exact skip; the chart crate pins the
marker, the move and the hash rule; the game pins the migration's
three outcomes and the backup keying.
