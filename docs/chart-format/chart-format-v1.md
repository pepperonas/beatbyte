# BeatByte Chart Format v1

A BeatByte chart is a JSON file describing one song and its playable
charts across difficulties. Implemented by the `beatbyte-chart` crate.

## Example

```json
{
  "format_version": 1,
  "song": {
    "title": "Circuit Breaker",
    "artist": "The Null Pointers",
    "audio": "circuit-breaker.ogg",
    "bpm": 128.0,
    "offset_s": 0.35,
    "preview_start_s": 42.0,
    "duration_s": 213.5
  },
  "charts": [
    {
      "difficulty": "expert",
      "lanes": 5,
      "notes": [
        { "time": 1.25, "lane": 0 },
        { "time": 1.25, "lane": 2 },
        { "time": 1.75, "lane": 1, "len": 1.5 },
        { "time": 2.0,  "lane": 2, "hopo": true }
      ],
      "phrases": [
        { "start": 8.0, "end": 12.0 }
      ]
    }
  ]
}
```

## Conventions

- **All times are seconds** (`f64`) on the song timeline; `0.0` is the
  start of the audio file.
- Unknown fields are tolerated (forward compatibility within a format
  version); the `format_version` gate protects against incompatible
  files. Readers reject `format_version` values above what they support.

## Fields

### Top level

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `format_version` | u32 | ✅ | Currently `1`. |
| `song` | object | ✅ | Song metadata. |
| `charts` | array | ✅ | One entry per difficulty; at least one. |

### `song`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `title` | string | ✅ | Non-empty. |
| `artist` | string | – (default `"Unknown"`) | |
| `audio` | string | ✅ | Relative path to the audio file, resolved against the chart's directory. No absolute paths, no `..`, no `:`. |
| `bpm` | f64 | ✅ | 20–400. Format v1 is constant-tempo; the domain model already supports tempo maps for a future version. |
| `offset_s` | f64 | – (default `0`) | Song time of musical beat 0 (audio lead-in). ±60 s. |
| `preview_start_s` | f64 | – | Song-browser preview start. |
| `duration_s` | f64 | – | Total song length. |

### `charts[]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `difficulty` | string | ✅ | `easy` \| `medium` \| `hard` \| `expert`; unique per file. |
| `lanes` | u8 | ✅ | Must be `5` in v1. |
| `notes` | array | ✅ | Per-lane notes (may be empty → validation warning). |
| `phrases` | array | – | Special (Hype) phrases; non-overlapping. |

### `notes[]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `time` | f64 | ✅ | Hit time in seconds, 0–7200. |
| `lane` | u8 | ✅ | 0–4, left to right. |
| `len` | f64 | – (default `0`) | Sustain length in seconds (0 = tap), max 300. |
| `hopo` | bool | – (default `false`) | Hammer-on/pull-off note. |

Notes on different lanes within **5 ms** of each other are grouped into
one chord event at load time (lanes union, longest sustain, HOPO only if
every grouped note is HOPO). Two notes on the *same* lane at the same
millisecond are a validation error.

### `phrases[]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `start` | f64 | ✅ | Inclusive start, seconds. |
| `end` | f64 | ✅ | Inclusive end, ≥ start. |

Hitting every note event inside a phrase (no misses) earns Hype meter.

## Validation

`ChartFile::validate()` returns all problems at once, each with a
severity (`error` = unplayable, `warning` = advisory), a location
(e.g. `charts[expert].notes[3]`) and a message. Hard limits: 100 000
notes per chart, 32 MB file size.

Charts are treated as **untrusted input** throughout: numeric ranges are
checked, the audio reference cannot escape the chart directory, and
malformed files produce errors, never crashes.

## Planned for future versions

Tempo changes and time signatures (the domain `TempoMap` already
supports them), section markers, per-event show effects. Additions that
old readers can safely ignore will not bump the format version; semantic
changes will.
