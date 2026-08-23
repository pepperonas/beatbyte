//! BeatByte's original, fully synthesized built-in tracks.
//!
//! BeatByte ships no copyrighted audio. This module renders two
//! deterministic tracks by the fictional band The Null Pointers, so
//! the game, the CLI and new users always have something legal to
//! play, analyze and chart — same code, same songs, every time, no
//! randomness:
//!
//! - **"Circuit Breaker"** (~64 s, 128 BPM): chiptune-flavored rock —
//!   driving eighths, kick/snare/hats, bass, lead over Am–F–C–G.
//! - **"Solder Groove"** (~70 s, 92 BPM): a half-time groove over
//!   Dm–Bb–F–C — syncopated bass, sparse drums, long sustained lead
//!   notes and a drum-less bridge, so generated charts exercise
//!   sustains and slower reading instead of note streams.

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

/// Tempo of the groove track.
pub const GROOVE_BPM: f64 = 92.0;

/// Groove song title.
pub const GROOVE_TITLE: &str = "Solder Groove";

/// Groove song artist.
pub const GROOVE_ARTIST: &str = "The Null Pointers";

/// Bars in the groove track (4/4).
const GROOVE_BARS: usize = 26;

/// Groove chord roots per bar as semitones relative to A2:
/// Dm, Bb, F, C.
const GROOVE_PROGRESSION: [i32; 4] = [5, 1, -4, 3];

/// Syncopated bass slots per bar (sixteenths): (slot, semitone offset
/// from the root, length in sixteenths). The inter-onset pattern is
/// 3-1-4 / 3-1-4: the quarter-note lag stays dominant so the analyzer
/// hears 92 BPM — a straight 3-3-2 tresillo here made the *dotted
/// eighth* the strongest pulse and the whole track analyzed as
/// ~122 BPM (measured), which would put the count-in at war with the
/// music.
const GROOVE_BASS: [(usize, i32, f64); 6] = [
    (0, 0, 2.5),
    (3, 0, 0.8),
    (4, 7, 3.0),
    (8, 0, 2.5),
    (11, 12, 0.8),
    (12, 7, 3.0),
];

/// Lead phrase per bar (sixteenths): (slot, semitone offset from the
/// root into D-minor-pentatonic territory, length in sixteenths).
/// Long values are the point — they chart as sustains.
const GROOVE_LEAD: [(usize, i32, f64); 3] = [(0, 12, 6.0), (8, 15, 4.0), (13, 10, 3.0)];

/// Render the groove track as mono audio.
#[must_use]
pub fn render_groove_song() -> AudioData {
    let rate = DEMO_SAMPLE_RATE;
    let beat_s = 60.0 / GROOVE_BPM;
    let bar_s = beat_s * 4.0;
    let sixteenth_s = bar_s / 16.0;
    let duration_s = GROOVE_BARS as f64 * bar_s + 2.0;
    let mut mix = vec![0.0f32; (duration_s * f64::from(rate)) as usize];

    for bar in 0..GROOVE_BARS {
        let bar_start = bar as f64 * bar_s;
        let root = GROOVE_PROGRESSION[bar % GROOVE_PROGRESSION.len()];
        // Sections: bass-only intro, drum-less sustained bridge, groove.
        let (drums, lead_mode) = match bar {
            0..=1 => (false, LeadMode::Off),
            10..=13 => (false, LeadMode::Pad),
            22..=25 => (true, LeadMode::Pad),
            _ => (true, LeadMode::Phrase),
        };

        if drums {
            // Half-time: kick on 1 and the and-of-3, snare on 3 only.
            kick(&mut mix, rate, bar_start);
            kick(&mut mix, rate, bar_start + 2.5 * beat_s);
            snare(&mut mix, rate, bar_start + 2.0 * beat_s);
            for beat in 0..4 {
                hat(&mut mix, rate, bar_start + beat as f64 * beat_s);
            }
            hat(&mut mix, rate, bar_start + 3.75 * beat_s);
        }
        if matches!(lead_mode, LeadMode::Pad) {
            // Pad bars hold ONE long bass note: the resulting onset
            // silence (>2 s to the next note) is what the chart
            // generator turns into sustains — with the full pattern
            // running, no difficulty ever got a gap wide enough.
            bass_note(&mut mix, rate, bar_start, root, bar_s * 0.9);
        } else {
            for (slot, offset, len) in GROOVE_BASS {
                let t = bar_start + slot as f64 * sixteenth_s;
                bass_note(&mut mix, rate, t, root + offset, len * sixteenth_s * 0.9);
            }
        }
        match lead_mode {
            LeadMode::Off => {}
            LeadMode::Phrase => {
                for (slot, offset, len) in GROOVE_LEAD {
                    let t = bar_start + slot as f64 * sixteenth_s;
                    lead_note(&mut mix, rate, t, root + offset, len * sixteenth_s);
                }
            }
            LeadMode::Pad => {
                // A held two-note chord for the whole bar.
                lead_note(&mut mix, rate, bar_start, root + 12, bar_s * 0.95);
                lead_note(&mut mix, rate, bar_start, root + 19, bar_s * 0.95);
            }
        }
    }

    for sample in &mut mix {
        *sample = (*sample * 0.7).tanh();
    }
    AudioData::from_mono(mix, rate)
}

/// What the groove lead plays in a bar.
enum LeadMode {
    /// Silence.
    Off,
    /// The syncopated phrase with long notes.
    Phrase,
    /// A bar-long sustained chord.
    Pad,
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
    fn groove_song_renders_deterministically() {
        let a = render_groove_song();
        let b = render_groove_song();
        assert_eq!(a, b);
        assert!(a.duration_s() > 60.0);
        assert!(a.samples().iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn groove_song_analyzes_to_its_own_tempo() {
        let audio = render_groove_song();
        let analysis = SpectralAnalyzer::default().analyze(&audio);
        // Half-time feel makes the octave (184) a legitimate reading;
        // either locks the grid to the same onsets.
        let near =
            |bpm: f64| (bpm - GROOVE_BPM).abs() < 3.0 || (bpm - GROOVE_BPM * 2.0).abs() < 6.0;
        assert!(
            near(analysis.bpm) || analysis.alt_bpm.is_some_and(near),
            "groove should analyze near {GROOVE_BPM} (or its octave), got {} (alt {:?})",
            analysis.bpm,
            analysis.alt_bpm
        );
        assert!(
            analysis.onsets.len() > 60,
            "the groove should produce onsets, got {}",
            analysis.onsets.len()
        );
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
