//! Marking a recording with clicks, so a timing can be judged **by
//! ear** instead of by number.
//!
//! A table of times is not something a person can check; a click that
//! lands on the event is right, a click beside it is wrong, and
//! anybody hears the difference in one pass. Two producers of times
//! ask the same question of a recording — the lyrics aligner about
//! word onsets, `beatbyte-cli chart-check` about a chart's notes — so
//! the marking lives here rather than in either of them.
//!
//! The click is deliberately a short, bright tick — 1.6 kHz for 12 ms
//! with a fast decay — because a thud disappears into a bass drum and
//! a long beep smears past the event it is marking. The song is
//! ducked under it so the tick stays audible in a loud mix, and the
//! mixing is pure, so it is tested without any audio device.
//!
//! ⚠️ Not [`crate::synth::click_track`], which SYNTHESIZES a click
//! track out of nothing (demo material). This one mixes clicks OVER a
//! recording that already exists.

use crate::decode::AudioData;

/// Click frequency, hertz.
pub const CLICK_HZ: f64 = 1600.0;
/// Click length, seconds.
pub const CLICK_S: f64 = 0.012;
/// How far the recording is pulled down under a click, 0..1
/// (0.35 = a third of its level).
pub const DUCK: f32 = 0.35;
/// The click's own level.
pub const CLICK_GAIN: f32 = 0.7;

/// Mix a click into `samples` (mono, `rate` Hz) at every time in
/// `times`, ducking the recording under each one.
///
/// Times outside the audio are ignored rather than clamped — a click
/// at the very end would mark an event that is not there. Pure.
#[must_use]
pub fn clicks_over(samples: &[f32], rate: u32, times: &[f64]) -> Vec<f32> {
    let mut out = samples.to_vec();
    if rate == 0 {
        return out;
    }
    let click_len = (CLICK_S * f64::from(rate)) as usize;
    for &time in times {
        if !time.is_finite() || time < 0.0 {
            continue;
        }
        let start = (time * f64::from(rate)) as usize;
        if start >= out.len() {
            continue;
        }
        for i in 0..click_len.min(out.len() - start) {
            let t = i as f64 / f64::from(rate);
            // A tick, not a beep: full at the attack, gone in 12 ms.
            let envelope = (1.0 - i as f64 / click_len as f64).powi(2);
            let tick = (2.0 * core::f64::consts::PI * CLICK_HZ * t).sin() * envelope;
            let sample = &mut out[start + i];
            *sample = *sample * DUCK + (tick as f32) * CLICK_GAIN;
        }
    }
    out
}

/// The recording with a click on every time in `times`.
#[must_use]
pub fn marked(audio: &AudioData, times: &[f64]) -> AudioData {
    AudioData::from_mono(
        clicks_over(audio.samples(), audio.sample_rate(), times),
        audio.sample_rate(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_lands_on_its_onset_and_ducks_the_song() {
        // A loud constant song, one click at 0.5 s in a 1 s buffer.
        let rate = 16_000;
        let song = vec![0.5f32; rate as usize];
        let out = clicks_over(&song, rate, &[0.5]);
        assert_eq!(out.len(), song.len());
        let at = (0.5 * f64::from(rate)) as usize;
        // Before the click the song is untouched...
        assert!((out[at - 1] - 0.5).abs() < 1e-6);
        // ...at its very first sample the tick is still zero, so what
        // is left there is the SONG, ducked - that one sample proves
        // the duck on its own (a louder click could not fake it).
        assert!(
            (out[at] - 0.5 * DUCK).abs() < 1e-6,
            "the song is ducked under the click: {} vs {}",
            out[at],
            0.5 * DUCK
        );
        let click_len = (CLICK_S * f64::from(rate)) as usize;
        let click: Vec<f32> = out[at..at + click_len].to_vec();
        let peak = click.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.5, "the click must stand out of the song: {peak}");
        // A tick, not a beep: it is loudest at the attack and nearly
        // gone by its end.
        let third = click_len / 3;
        let attack = click[..third]
            .iter()
            .fold(0.0f32, |m, v| m.max((v - 0.5 * DUCK).abs()));
        let tail = click[click_len - third..]
            .iter()
            .fold(0.0f32, |m, v| m.max((v - 0.5 * DUCK).abs()));
        assert!(
            tail < attack * 0.25,
            "the click must decay: attack {attack}, tail {tail}"
        );
        // ...and 12 ms later the song is back.
        let after = at + (CLICK_S * f64::from(rate)) as usize + 1;
        assert!((out[after] - 0.5).abs() < 1e-6, "{}", out[after]);
    }

    #[test]
    fn impossible_times_are_skipped_not_clamped() {
        let rate = 16_000;
        let song = vec![0.25f32; rate as usize];
        // Past the end, negative, NaN: none of them may mark anything.
        let out = clicks_over(&song, rate, &[5.0, -1.0, f64::NAN]);
        assert_eq!(out, song, "nothing was marked");
        assert_eq!(clicks_over(&song, 0, &[0.1]), song, "no rate, no clicks");
    }

    #[test]
    fn marking_keeps_the_recording_s_length_and_rate() {
        let rate = 8_000;
        let audio = AudioData::from_mono(vec![0.2f32; rate as usize], rate);
        let out = marked(&audio, &[0.1, 0.9]);
        assert_eq!(out.sample_rate(), rate);
        assert_eq!(out.samples().len(), audio.samples().len());
    }
}
