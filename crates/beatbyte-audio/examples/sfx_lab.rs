//! Render the error-sound candidates to WAV files for auditioning.
//!
//! Judging a 60-millisecond sound by reading its constants does not
//! work; it has to be heard, next to the alternatives and next to what
//! it replaces. This writes every candidate to a directory so they can
//! be played back to back.
//!
//! ```text
//! cargo run -p beatbyte-audio --example sfx_lab -- /tmp/sfx
//! ```
//!
//! The first two are what the game ships; the rest are the other
//! shapes that were considered, kept so a different choice is a small
//! edit rather than a rewrite. `legacy-thud` is the sound they
//! replaced, for an honest A/B.

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::synth::{ErrorVoice, MISS_VOICE, OVERSTRUM_VOICE};
use beatbyte_audio::write_wav_mono16;

const RATE: u32 = 44_100;

/// A very dark, purely damped thunk: no pitch at all, minimum drama.
const MUTE_THUNK: ErrorVoice = ErrorVoice {
    length_s: 0.070,
    clank_hz: 165.0,
    clank_q: 1.1,
    clank_gain: 0.55,
    tones: &[],
    bend: 1.0,
    duty: 0.5,
    tone_gain: 0.0,
    decay_s: 0.022,
    peak: 0.34,
    seed: 0x1234_5678_9ABC_DEF0,
};

/// A rattling fret buzz: narrow pulse, close partials beating against
/// each other, brighter clank.
const FRET_BUZZ: ErrorVoice = ErrorVoice {
    length_s: 0.075,
    clank_hz: 720.0,
    clank_q: 3.0,
    clank_gain: 0.30,
    tones: &[147.0, 155.0],
    bend: 0.88,
    duty: 0.12,
    tone_gain: 0.34,
    decay_s: 0.026,
    peak: 0.34,
    seed: 0xDEAD_BEEF_CAFE_1234,
};

/// A pure downward bend with almost no noise: the cartoon deflate.
const DROP_BEND: ErrorVoice = ErrorVoice {
    length_s: 0.110,
    clank_hz: 300.0,
    clank_q: 1.5,
    clank_gain: 0.08,
    tones: &[330.0],
    bend: 0.38,
    duty: 0.5,
    tone_gain: 0.42,
    decay_s: 0.040,
    peak: 0.34,
    seed: 0x0BAD_F00D_1357_9BDF,
};

/// A pick scrape: bright, airy, no pitch, slightly longer tail.
const SCRAPE: ErrorVoice = ErrorVoice {
    length_s: 0.095,
    clank_hz: 2_400.0,
    clank_q: 0.9,
    clank_gain: 0.5,
    tones: &[],
    bend: 1.0,
    duty: 0.5,
    tone_gain: 0.0,
    decay_s: 0.030,
    peak: 0.34,
    seed: 0xFEED_FACE_8BAD_F00D,
};

/// The sound these replaced: a low sine plus a click of high-passed
/// noise. Kept verbatim so the comparison is fair.
fn legacy_thud() -> AudioData {
    let rate = f64::from(RATE);
    let length = (0.09 * rate) as usize;
    let mut samples = vec![0.0f32; length];
    let mut noise = 0x1234_5678_9ABC_DEF0u64;
    let mut last = 0.0f32;
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / rate;
        let body = (2.0 * core::f64::consts::PI * 95.0 * t).sin() as f32;
        noise ^= noise << 13;
        noise ^= noise >> 7;
        noise ^= noise << 17;
        let white = (noise >> 40) as f32 / 8_388_608.0 - 1.0;
        let hp = white - last;
        last = white;
        let envelope = (-t / 0.03).exp() as f32;
        *slot = (body * 0.5 + hp * 0.18) * envelope * 0.5;
    }
    AudioData::from_mono(samples, RATE)
}

/// Where a sound's energy actually sits: its strongest partial, and
/// the share of magnitude below one kilohertz.
///
/// An amplitude-weighted centroid was tried first and is useless here.
/// These sounds are short and noisy, so the broadband floor spread
/// across 1-22 kHz dominates the mean: it reported ~4.5 kHz for every
/// candidate, including one whose strongest components are all under
/// 260 Hz. Two honest numbers beat one misleading one.
fn profile(audio: &AudioData) -> (f64, f64) {
    let samples = audio.samples();
    let n = samples.len();
    let rate = f64::from(audio.sample_rate());
    let (mut best, mut best_hz) = (0.0, 0.0);
    let (mut low, mut total) = (0.0, 0.0);
    for k in 1..n / 2 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in samples.iter().enumerate() {
            let angle = -2.0 * core::f64::consts::PI * (k * i) as f64 / n as f64;
            re += f64::from(s) * angle.cos();
            im += f64::from(s) * angle.sin();
        }
        let magnitude = re.hypot(im);
        let hz = k as f64 * rate / n as f64;
        if magnitude > best {
            best = magnitude;
            best_hz = hz;
        }
        if hz < 1_000.0 {
            low += magnitude;
        }
        total += magnitude;
    }
    (best_hz, if total > 0.0 { low / total } else { 0.0 })
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/beatbyte-sfx".to_owned());
    std::fs::create_dir_all(&dir).expect("create output directory");

    let candidates: Vec<(&str, AudioData)> = vec![
        ("1-miss", MISS_VOICE.render(RATE)),
        ("2-overstrum", OVERSTRUM_VOICE.render(RATE)),
        ("3-mute-thunk", MUTE_THUNK.render(RATE)),
        ("4-fret-buzz", FRET_BUZZ.render(RATE)),
        ("5-drop-bend", DROP_BEND.render(RATE)),
        ("6-scrape", SCRAPE.render(RATE)),
        ("7-legacy-thud", legacy_thud()),
    ];

    println!(
        "{:<16} {:>5} {:>6} {:>12} {:>11}",
        "sound", "ms", "peak", "strongest", "below 1 kHz"
    );
    for (name, audio) in &candidates {
        let path = std::path::Path::new(&dir).join(format!("{name}.wav"));
        write_wav_mono16(&path, audio).expect("write wav");
        let peak = audio.samples().iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let (strongest, low_share) = profile(audio);
        println!(
            "{name:<16} {:>5.0} {peak:>6.2} {strongest:>9.0} Hz {:>10.0}%",
            audio.duration_s() * 1000.0,
            low_share * 100.0
        );
    }
    // One file that plays the whole set. A 70-millisecond sound is
    // impossible to judge on its own: what matters is how it sits
    // against the others, and whether it still reads when it fires
    // three times in a row - which, in a bad passage, it will.
    let mut tour = vec![0.0f32; 0];
    for (_, audio) in &candidates {
        for _ in 0..3 {
            tour.extend_from_slice(audio.samples());
            tour.extend(std::iter::repeat_n(0.0, (0.10 * f64::from(RATE)) as usize));
        }
        tour.extend(std::iter::repeat_n(0.0, (0.55 * f64::from(RATE)) as usize));
    }
    let tour_path = std::path::Path::new(&dir).join("0-audition.wav");
    write_wav_mono16(&tour_path, &AudioData::from_mono(tour, RATE)).expect("write audition");

    println!("\nwritten to {dir}");
    println!("play them all: afplay {}", tour_path.display());
}
