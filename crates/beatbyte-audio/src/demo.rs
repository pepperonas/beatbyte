//! "Circuit Breaker" by The Null Pointers — BeatByte's original,
//! fully synthesized demo track.
//!
//! BeatByte ships no copyrighted audio. This module renders a
//! deterministic ~64-second chiptune-flavored rock track (kick, snare,
//! hats, bass, lead over an Am–F–C–G progression) so the game, the CLI
//! and new users always have something legal to play, analyze and
//! chart. Same code, same song, every time — no randomness.

use crate::decode::AudioData;

/// Tempo of the demo track.
pub const DEMO_BPM: f64 = 128.0;

/// Sample rate the demo renders at.
pub const DEMO_SAMPLE_RATE: u32 = 44_100;

/// Demo song title.
pub const DEMO_TITLE: &str = "Circuit Breaker";

/// Demo song artist.
pub const DEMO_ARTIST: &str = "The Null Pointers";

/// Bars in the demo (4/4).
const BARS: usize = 34;

/// Chord roots per bar as semitones relative to A2 (110 Hz):
/// Am, F, C, G — the loop everyone has heard and no one owns.
const PROGRESSION: [i32; 4] = [0, -4, 3, -2];

/// Lead pattern per bar: sixteen slots of semitone offsets into an
/// A-minor pentatonic phrase; `None` = rest.
const LEAD_PATTERN: [Option<i32>; 16] = [
    Some(12),
    None,
    Some(15),
    None,
    Some(17),
    None,
    Some(15),
    Some(12),
    None,
    Some(10),
    None,
    Some(12),
    Some(15),
    None,
    Some(19),
    None,
];

/// Render the demo track as mono audio.
#[must_use]
pub fn render_demo_song() -> AudioData {
    let rate = DEMO_SAMPLE_RATE;
    let beat_s = 60.0 / DEMO_BPM;
    let bar_s = beat_s * 4.0;
    let duration_s = BARS as f64 * bar_s + 2.0;
    let mut mix = vec![0.0f32; (duration_s * f64::from(rate)) as usize];

    for bar in 0..BARS {
        let bar_start = bar as f64 * bar_s;
        let root = PROGRESSION[bar % PROGRESSION.len()];
        // Sections: intro (drums build), body, breakdown, finale.
        let (drums, bass_on, lead_on) = match bar {
            0..=3 => (bar >= 2, true, false),
            16..=19 => (false, true, true), // breakdown: drums drop
            _ => (true, true, true),
        };

        for beat in 0..4 {
            let t = bar_start + beat as f64 * beat_s;
            if drums {
                kick(&mut mix, rate, t);
                if beat == 1 || beat == 3 {
                    snare(&mut mix, rate, t);
                }
                for eighth in 0..2 {
                    hat(&mut mix, rate, t + eighth as f64 * beat_s / 2.0);
                }
            }
            if bass_on {
                for eighth in 0..2 {
                    let note_t = t + eighth as f64 * beat_s / 2.0;
                    let semis = root + if eighth == 1 && beat == 3 { 7 } else { 0 };
                    bass_note(&mut mix, rate, note_t, semis, beat_s * 0.45);
                }
            }
        }

        if lead_on {
            for (slot, semis) in LEAD_PATTERN.iter().enumerate() {
                if let Some(semis) = semis {
                    let t = bar_start + slot as f64 * bar_s / 16.0;
                    lead_note(&mut mix, rate, t, root + semis, bar_s / 16.0 * 1.8);
                }
            }
        }
    }

    // Gentle master headroom + soft clip.
    for sample in &mut mix {
        *sample = (*sample * 0.7).tanh();
    }
    AudioData::from_mono(mix, rate)
}

/// Frequency of A2 shifted by `semis` semitones.
fn pitch(semis: i32) -> f64 {
    110.0 * 2.0f64.powf(f64::from(semis) / 12.0)
}

/// Kick drum: a sine with a fast downward pitch sweep.
fn kick(mix: &mut [f32], rate: u32, time_s: f64) {
    let start = (time_s * f64::from(rate)) as usize;
    let length = (0.12 * f64::from(rate)) as usize;
    let mut phase = 0.0f64;
    for i in 0..length {
        let Some(slot) = mix.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / f64::from(rate);
        let freq = 40.0 + 90.0 * (-t / 0.02).exp();
        phase += 2.0 * core::f64::consts::PI * freq / f64::from(rate);
        let envelope = (-t / 0.05).exp();
        *slot += (phase.sin() * envelope * 0.9) as f32;
    }
}

/// Snare: filtered deterministic noise burst plus a 180 Hz body.
fn snare(mix: &mut [f32], rate: u32, time_s: f64) {
    let start = (time_s * f64::from(rate)) as usize;
    let length = (0.10 * f64::from(rate)) as usize;
    let mut noise_state = 0x2545_F491_4F6C_DD1Du64 ^ start as u64;
    let mut last = 0.0f32;
    for i in 0..length {
        let Some(slot) = mix.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / f64::from(rate);
        // xorshift noise, high-passed by first difference.
        noise_state ^= noise_state << 13;
        noise_state ^= noise_state >> 7;
        noise_state ^= noise_state << 17;
        let white = (noise_state >> 40) as f32 / 8_388_608.0 - 1.0;
        let hp = white - last;
        last = white;
        let body = (2.0 * core::f64::consts::PI * 180.0 * t).sin() as f32;
        let envelope = (-t / 0.035).exp() as f32;
        *slot += (hp * 0.5 + body * 0.3) * envelope * 0.7;
    }
}

/// Closed hat: very short bright noise.
fn hat(mix: &mut [f32], rate: u32, time_s: f64) {
    let start = (time_s * f64::from(rate)) as usize;
    let length = (0.03 * f64::from(rate)) as usize;
    let mut noise_state = 0x9E37_79B9_7F4A_7C15u64 ^ (start as u64).rotate_left(17);
    let mut last = 0.0f32;
    for i in 0..length {
        let Some(slot) = mix.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / f64::from(rate);
        noise_state ^= noise_state << 13;
        noise_state ^= noise_state >> 7;
        noise_state ^= noise_state << 17;
        let white = (noise_state >> 40) as f32 / 8_388_608.0 - 1.0;
        let hp = white - last;
        last = white;
        *slot += hp * ((-t / 0.008).exp() as f32) * 0.25;
    }
}

/// Bass: a naive square wave an octave down, punchy envelope.
fn bass_note(mix: &mut [f32], rate: u32, time_s: f64, semis: i32, length_s: f64) {
    let start = (time_s * f64::from(rate)) as usize;
    let length = (length_s * f64::from(rate)) as usize;
    let freq = pitch(semis) / 2.0;
    for i in 0..length {
        let Some(slot) = mix.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / f64::from(rate);
        let phase = (freq * t).fract();
        let square = if phase < 0.5 { 1.0 } else { -1.0 };
        let envelope = ((-t / (length_s * 0.6)).exp() * (1.0 - (-t / 0.004).exp())) as f32;
        *slot += square as f32 * envelope * 0.28;
    }
}

/// Lead: 25% pulse wave with light vibrato, chip style.
fn lead_note(mix: &mut [f32], rate: u32, time_s: f64, semis: i32, length_s: f64) {
    let start = (time_s * f64::from(rate)) as usize;
    let length = (length_s * f64::from(rate)) as usize;
    let freq = pitch(semis);
    for i in 0..length {
        let Some(slot) = mix.get_mut(start + i) else {
            break;
        };
        let t = i as f64 / f64::from(rate);
        let vibrato = 1.0 + 0.004 * (2.0 * core::f64::consts::PI * 5.5 * t).sin();
        let phase = (freq * vibrato * t).fract();
        let pulse = if phase < 0.25 { 1.0 } else { -1.0 };
        let envelope = ((-t / (length_s * 0.7)).exp() * (1.0 - (-t / 0.002).exp())) as f32;
        *slot += pulse as f32 * envelope * 0.16;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::analysis::{Analyzer, SpectralAnalyzer};

    #[test]
    fn demo_song_renders_deterministically() {
        let a = render_demo_song();
        let b = render_demo_song();
        assert_eq!(a, b);
        assert!(a.duration_s() > 60.0);
        // Every sample must be finite and within range after soft clip.
        assert!(a.samples().iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn demo_song_analyzes_to_its_own_tempo() {
        let audio = render_demo_song();
        let analysis = SpectralAnalyzer::default().analyze(&audio);
        assert!(
            (analysis.bpm - DEMO_BPM).abs() < 3.0
                || analysis
                    .alt_bpm
                    .is_some_and(|alt| (alt - DEMO_BPM).abs() < 3.0),
            "demo should analyze near {DEMO_BPM} BPM, got {} (alt {:?})",
            analysis.bpm,
            analysis.alt_bpm
        );
        assert!(
            analysis.onsets.len() > 100,
            "a busy track should produce onsets, got {}",
            analysis.onsets.len()
        );
    }
}
