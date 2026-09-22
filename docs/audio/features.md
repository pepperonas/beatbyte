# Song-level features

What BeatByte measures about a whole song, how, and what each number
means. One pass over the decoded audio produces all of it
(`beatbyte-audio/src/features.rs`); the document records it together
with the version of the measurement that produced it, so a later,
better estimator can tell which songs the old one measured without
re-measuring anything.

**Version 1.**

## The pass

A short-time Fourier transform: 4096-sample Hann window (~93 ms at
44.1 kHz), hop 2048. Long enough to resolve a bass note's pitch
class, short enough that a chord change is not smeared across the
whole chroma.

Everything below falls out of that one pass. Decoding the song is
most of the cost, which is why nothing here runs during a library
scan or at start-up: the import measures while the song is already
decoded for charting, `beatbyte-cli library` measures on demand, and
in the game a background pass visits one song at a time.

Measured cost: **171 songs, 2.4 GB, about 90 seconds** — roughly half
a second a song, nearly all of it decoding.

## What is measured

| Field | Range | Meaning |
|---|---|---|
| `energy` | 0–1 | The mean frame's RMS against the loudest frame's. How much of the song is spent near its own ceiling. Peak-relative on purpose: how loud the master *is* belongs to the loudness sidecar, which measures it properly in LUFS. |
| `bass_energy` | 0–1 | Share of the spectrum's magnitude below **250 Hz**. |
| `mid_energy` | 0–1 | …between 250 Hz and **4 kHz**. |
| `high_energy` | 0–1 | …above 4 kHz. The three sum to 1. |
| `spectral_centroid_hz` | Hz | Magnitude-weighted mean frequency — what "bright" means when said about a mix. |
| `onset_density` | per second | Peaks per second in the spectral flux (only what *grew*: a note starting is a rise, a note ending is not an onset). A peak is a local maximum above the song's own median by half its own spread, **and** above 5 % of its loudest rise. |
| `beat_strength` | 0–1 | The best autocorrelation peak of the flux over the lags a beat can occupy (60–200 BPM), normalised by the lag-0 energy. A metronome approaches 1; a free-time piece sits near 0. |
| `musical.key` | — | See below. Stored only above a margin, and always as an estimate with its confidence. |

⚠️ The onset threshold needs **both** parts. A purely relative one has
nothing to hold on to when there are no onsets at all: on a held tone
the "spread" is numerical ripple, and the first version reported a
higher onset rate for a sustained note than for a steady pulse.

## Key

Krumhansl–Schmuckler: the song's pitch-class weights are correlated
against the two profiles listeners produced in the 1982 probe-tone
experiments, once for each of the twelve tonics. The best of the
twenty-four wins, and the **margin** is its correlation minus the
runner-up's.

### Two defects this went through, both found by measuring

**The chroma read the transform, not the music.** The first version
summed every FFT bin into the pitch class it rounded to. FFT bins are
linearly spaced and pitch classes are not, so broadband content falls
into a *fixed* pattern that has nothing to do with the song. White
noise came back with a 1.9× spread across the twelve classes, and 135
of this library's 171 songs landed in just four keys — A♯ minor, C♯
minor, F♯ major, C♯ major. A real pop library does not look like that.

The fix is one window per **pitch** (G3 to C8, ±50 cents) and the
**mean** magnitude inside it rather than a sum over every bin. Noise
then gives every window the same mean, which is flat, which is the
only honest chroma for noise. Afterwards the same library reads G
major (32), A minor (24), C major (18), G minor (13) — the guitar
keys — across 18 distinct tonalities.

**The margin was scale-free in the wrong way.** It was
`(best − second) / best`. On a flat chroma the two best fits are 0.020
and 0.017, both useless, and that formula calls the difference a
margin of 0.15 — white noise was read as A minor. It is now the plain
difference of two correlations, which is already comparable across
songs *and* small when nothing fits.

A test pins both: noise must produce a flat chroma and must not be
read as a key. It is the counter-check for the whole feature — a
measurement that reports something is only believable once it has been
shown to report nothing when there is nothing.

### How good is it, honestly

`KEY_MIN_MARGIN` is **0.12**, which keeps 27 of this library's 171
songs. Spot-checked against documented keys, of the six kept songs
whose key could be verified:

| Song | Documented | Estimated | |
|---|---|---|---|
| Seven Nation Army | E minor | E minor | ✓ |
| Born to Run | E major | E major | ✓ |
| Video Killed the Radio Star | A♭ major | G♯ major | ✓ |
| In the Shadows | F♯ minor | F♯ minor | ✓ |
| Like a Rolling Stone | C major | G major | ✗ |
| Ms. Jackson | B♭ minor | G major | ✗ |

Four of six. **Both failures are the dominant**, a known weakness of
profile matching on a bass-heavy mix. And a relative major and minor
share every note, so the profiles cannot separate them by
construction: *Nothing Else Matters* (E minor) comes back as G major
with a margin of **0.008** — the estimator saying, correctly, that it
cannot tell.

That is why the key is stored as `Sourced::analyzed` with its margin
as the confidence, never as a fact, and why nothing below the margin
is stored at all. **A proper evaluation needs a labelled set**; six
songs is a spot check, not a measurement, and the roadmap carries it
as an open item.

## What is deliberately absent

Danceability and valence. The commission asked for them; there is no
model here that determines either, and a number that looks like an
answer is worse than a missing one.
