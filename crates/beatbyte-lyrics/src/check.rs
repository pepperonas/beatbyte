//! The check track: a song with a click on every aligned word, so an
//! alignment can be judged **by ear** instead of by number.
//!
//! Ground truth for the own fixture set (`docs/lyrics/fixtures.md`)
//! has to be corrected by a person, and a table of times is not
//! something a person can check. A click that lands on the word is
//! right, a click that lands beside it is wrong, and anybody hears
//! the difference in one pass.
//!
//! The click is deliberately a short, bright tick — 1.6 kHz for 12 ms
//! with a fast decay — because a thud disappears into a bass drum and
//! a long beep smears past the word it is marking. The song is ducked
//! under it so the tick stays audible in a loud mix, and the mixing
//! is pure so it can be tested without any audio device.

use beatbyte_audio::decode::AudioData;

/// Click frequency, hertz.
pub const CLICK_HZ: f64 = 1600.0;
/// Click length, seconds.
pub const CLICK_S: f64 = 0.012;
/// How far the song is pulled down under a click, 0..1 (0.35 = a
/// third of its level).
pub const DUCK: f32 = 0.35;
/// The click's own level.
pub const CLICK_GAIN: f32 = 0.7;

/// Mix a click into `samples` (mono, `rate` Hz) at every time in
/// `onsets`, ducking the song under each one. Times outside the audio
/// are ignored rather than clamped — a click at the very end would
/// mark a word that is not there. Pure — tested.
#[must_use]
pub fn click_track(samples: &[f32], rate: u32, onsets: &[f64]) -> Vec<f32> {
    let mut out = samples.to_vec();
    if rate == 0 {
        return out;
    }
    let click_len = (CLICK_S * f64::from(rate)) as usize;
    for &onset in onsets {
        if !onset.is_finite() || onset < 0.0 {
            continue;
        }
        let start = (onset * f64::from(rate)) as usize;
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

/// A check track for an alignment: the audio with a click on every
/// word that carries real timing. Estimated words are marked too —
/// they are exactly the ones worth listening to.
#[must_use]
pub fn check_track(audio: &AudioData, alignment: &crate::words::Alignment) -> AudioData {
    let onsets: Vec<f64> = alignment.words().map(|w| w.start).collect();
    AudioData::from_mono(
        click_track(audio.samples(), audio.sample_rate(), &onsets),
        audio.sample_rate(),
    )
}

/// The word list a listener reads along with the check track: one
/// line per lyric line, each word with its onset, estimated words
/// marked. Pure — tested.
#[must_use]
pub fn word_sheet(alignment: &crate::words::Alignment) -> String {
    let mut out = String::new();
    for (index, line) in alignment.lines.iter().enumerate() {
        out.push_str(&format!("{:>3}. [{:>8.3}] ", index + 1, line.start));
        for word in &line.words {
            out.push_str(&format!(
                "{}{}({:.3})  ",
                word.text,
                if word.estimated { "*" } else { "" },
                word.start
            ));
        }
        out.push('\n');
    }
    out.push_str("\n* = estimated (no acoustic evidence of its own)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::words::{AlignedLine, AlignedWord, Alignment, SCHEMA, Source};

    #[test]
    fn a_click_lands_on_its_onset_and_ducks_the_song() {
        // A loud constant song, one click at 0.5 s in a 1 s buffer.
        let rate = 16_000;
        let song = vec![0.5f32; rate as usize];
        let out = click_track(&song, rate, &[0.5]);
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
    fn impossible_onsets_are_skipped_not_clamped() {
        let rate = 16_000;
        let song = vec![0.25f32; rate as usize];
        // Past the end, negative, NaN: none of them may mark a word.
        let out = click_track(&song, rate, &[5.0, -1.0, f64::NAN]);
        assert_eq!(out, song, "nothing was marked");
        assert_eq!(click_track(&song, 0, &[0.1]), song, "no rate, no clicks");
    }

    #[test]
    fn the_sheet_reads_like_something_a_person_can_follow() {
        let alignment = Alignment {
            schema: SCHEMA.to_owned(),
            audio_sha256: "00".repeat(32),
            pipeline_version: 1,
            language: "en".to_owned(),
            source: Source {
                text: "t".to_owned(),
                separator: "none".to_owned(),
                aligner: "a".to_owned(),
            },
            offset_ms: 0,
            gate: None,
            lines: vec![AlignedLine {
                start: 1.0,
                end: 2.0,
                text: "Hi there".to_owned(),
                words: vec![
                    AlignedWord {
                        text: "Hi".to_owned(),
                        start: 1.0,
                        end: 1.4,
                        conf: 0.5,
                        estimated: false,
                        chars: Vec::new(),
                    },
                    AlignedWord {
                        text: "there".to_owned(),
                        start: 1.5,
                        end: 2.0,
                        conf: 0.0,
                        estimated: true,
                        chars: Vec::new(),
                    },
                ],
            }],
        };
        let sheet = word_sheet(&alignment);
        assert!(sheet.contains("Hi(1.000)"), "{sheet}");
        assert!(
            sheet.contains("there*(1.500)"),
            "an estimated word is marked"
        );
        assert!(sheet.contains("  1. [   1.000]"));
    }
}
