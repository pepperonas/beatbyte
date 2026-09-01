# Visual master plan — modern rendering, audio-reactivity, karaoke lyrics

Commissioned 2026-09-01: a structured spec covering renderer
architecture, a 3D highway, premium notes, HDR/bloom, GPU particles,
audio-reactive environment, hit feedback, dynamic camera, modern HUD
and — as a first-class feature — live karaoke lyrics.

## Phase 1 — inventory: what already ships

Most of the commissioned rendering stack exists. Mapped phase by
phase so the work below is the *gap*, not a rebuild:

| Commission phase | Repository reality |
|---|---|
| 2 Renderer architecture | Gameplay rules live in `beatbyte_core::TrackSession`; presentation consumes `SessionFeedback` messages; the style boundary is data (ADR-0012). Already the commissioned layering. |
| 3 Audio-visual layer | Chart-side analysis (onsets, tempo/beat grid, energy phrases, melody) drives beat pulse, LED wall, phrase bands. Deterministic by design — visuals sample the song timeline, not a live FFT. |
| 4 Modern highway | `stage3d.rs`: true 3D perspective highway (26 world units to a vanishing point), lit geometry, procedural board/rail/gem textures, full venue (crowd, speaker stacks, pulsing woofers, LED wall, sweeping beams, floor spots). |
| 5 Note rendering | Meshes + per-style textures, emissive materials per lane/theme, star meshes for specials, receptor fills, hit flames. |
| 6 HDR/bloom | Per-note-style HDR + Bloom with the two-camera contracts pinned by tests (`sync_bloom`, `sync_stage_compositing`). |
| 7 GPU particles | CPU particles, budgeted (`MAX_PARTICLES` 600), intensity-scaled. **bevy_hanabi 0.19.0 exists and matches Bevy 0.19** — checked 2026-09-01 — but the CPU budget is nowhere near a limit, so no new dependency. Revisit only if particle ambitions exceed the CPU budget (recorded as the H-series backlog). |
| 8 Hit feedback | Judgment-tiered bursts, popups with early/late tags, strum coach, receptor flames, hit bursts. |
| 9 Background | Theme backdrops + venue react to beats (LED wall, woofers) and energy phrases (highway bands, edge flames on hype). |
| 10 Camera | Shake exists (budgeted, setting-gated). Beat-driven micro-motion deliberately NOT added: the two-camera HDR contracts are pinned and fragile, and readability outranks spectacle (commission's own priority list). |
| 11 Modern HUD | Redesigned twice in v0.12.28/v0.13.8 (GH2-grounded streak beads, clear counter line, hype gauge with fan…). |
| 12–14 Lyrics | **Nothing exists. This is the actual commission.** |
| 15 Menus | `ui_kit` design system + logical input actions + device prompts shipped v0.12.29–34. |
| 16–17 Perf/QA | Quality gate + autopilot harness are the standing regime. |

## The gap: live karaoke lyrics

### Data model and parsing (`beatbyte-chart::lyrics`)

Lyrics are song data and untrusted input, so the parser lives beside
the chart format with the same validation culture (size caps, count
caps, clamped timestamps). Model:

```text
Lyrics
 └── lines[]           (sorted by start)
       ├── start, end  (seconds on the song timeline)
       ├── text        (the whole line, for layout)
       └── words[]     (empty = line-level timing only)
             ├── text
             ├── start, end
```

- **Standard LRC**: `[mm:ss.xx]text`, repeated stamps
  (`[t1][t2]text`) emit one line per stamp, metadata tags are
  skipped, `[offset:±ms]` shifts every stamp (positive = lyrics
  appear sooner, per the de-facto LRC convention).
- **Enhanced LRC**: inline `<mm:ss.xx>` stamps split a line into
  karaoke words. Word end = next word's start; line end = next
  line's start, capped so a line never lingers through a long
  instrumental.
- The model already carries what a later lyrics **editor** or
  **auto-sync** pipeline needs (absolute per-word spans); neither is
  built now.

### Timebase (commission §21, §43)

One clock. Lyrics render from `GameClock::visual_time` — the same
presentational time notes are drawn with (song position + video
offset) — plus their own `lyrics_offset_ms`. Judgment reads none of
it. The cue is a pure function `position → state`, so the
deterministic-state test the commission demands is a unit test, not
a screenshot.

### Karaoke rendering (`gameplay/lyrics.rs`)

- **Per-character glyphs.** Press Start 2P advances exactly 1 em per
  glyph (measured from the bundled TTF), so a line lays out as one
  Text2d per character at computed offsets — and the karaoke fill is
  nothing but per-glyph `TextColor` writes. No per-frame string
  allocation, no text re-layout, no entity churn except at line
  changes (a handful per song).
- **Word-timed lines** fill glyph by glyph as `word_progress`
  crosses each glyph's fraction; the boundary glyph lerps, the word
  igniting gets a subtle scale pop (motion-gated).
- **Line-timed lines** fade in, hold bright, fade out. No fake word
  sync — the commission forbids pretending.
- **Layout**: active line centered above the highway's vanishing
  point (world y ≈ +255), next line dimmed below it, a soft scrim
  behind both so the LED wall can never wash the text out. Notes,
  receptors and HUD are never covered. World-space 2D like the rest
  of the HUD, so every view mode and window size behaves alike.
- **Multiplayer**: one shared display (the song is shared).
- **MC sets**: lyrics travel inside `LoadedSong`, so the crossfade
  handover swaps them with the chart.

### Where lyrics come from

1. A `.lrc` beside the song's audio (same stem), or beside the
   chart. Import and the watch folder copy it along with the audio.
2. The demo song "Circuit Breaker" ships hand-written original
   enhanced-LRC (recorded in `docs/development/asset-licenses.md`), so a fresh
   clone demonstrates karaoke without any user content.

### Settings (commission §29, trimmed honestly)

`LYRICS` on/off · `LYRICS SIZE` (small/medium/large) ·
`LYRICS OFFSET` (ms). Position stays fixed (the one place that
covers nothing), opacity folds into the design, intensity/motion
follow the existing `fx_intensity` / motion settings — no settings
that exist only in the menu.

## Deliberate deviations from the commission text

- **No bevy_hanabi** (yet): compatible version exists, but the
  budgeted CPU system isn't the bottleneck of "premium feel" and the
  commission itself forbids dependency bloat.
- **No new camera choreography**: readability and the pinned
  two-camera contracts outrank a micro-zoom. Shake already exists.
- **No graphics presets row**: every effect is individually
  toggleable already; a Low/Med/High/Ultra macro would be a second
  way to write the same settings.
- **Lyrics position/opacity settings**: cut, see above.
- **Previous/active/next three-line stack**: trimmed to
  active + next. Above a five-lane highway the previous line is
  noise; the completed line's exit transition carries the context.
