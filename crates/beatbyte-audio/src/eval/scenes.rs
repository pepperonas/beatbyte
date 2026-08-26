//! The evaluation scenes: synthetic songs whose ground truth is known
//! *by construction* — every note in a scene was placed by the scene
//! itself, so precision, recall, pitch and sustain error are exact
//! measurements rather than opinions.
//!
//! The scenes deliberately cover the failure modes a guitar charter
//! actually has to survive, not just the happy path: drums that are
//! louder than the guitar, a voice sitting on top of the riff,
//! syncopation that must not be quantized away, and a groove whose
//! tempo is genuinely ambiguous.

use super::instruments::{
    Noise, add_hat, add_kick, add_pluck, add_snare, add_sustained, add_voice, midi_hz,
};
use crate::decode::AudioData;

/// Analysis-rate scenes; the pipeline keeps ≤32 kHz audio as-is, so
/// this is exactly the rate the analyzer will see.
pub const RATE: u32 = 22_050;

/// What part of the arrangement an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The line a guitar chart should follow.
    Lead,
    /// Bass guitar: pitched, but not the chart.
    Bass,
    /// Drums: no stable pitch, not the chart.
    Percussion,
    /// A voice competing with the lead — must not hijack the chart.
    Vocal,
}

/// One ground-truth event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TruthNote {
    /// When the event starts, seconds.
    pub time_s: f64,
    /// When it stops sounding, seconds.
    pub end_s: f64,
    /// Pitch as a MIDI number; `None` for unpitched percussion.
    pub midi: Option<f32>,
    /// Which part of the arrangement this is.
    pub role: Role,
}

impl TruthNote {
    /// How long the event is held.
    #[must_use]
    pub fn len_s(&self) -> f64 {
        (self.end_s - self.time_s).max(0.0)
    }
}

/// A synthetic song plus its ground truth.
#[derive(Debug, Clone)]
pub struct Scene {
    /// Stable identifier used in reports and test names.
    pub name: &'static str,
    /// One-line description of what the scene proves.
    pub about: &'static str,
    /// The rendered audio.
    pub audio: AudioData,
    /// The true tempo the analyzer should find.
    pub bpm: f64,
    /// Every event that was placed, in every role.
    pub truth: Vec<TruthNote>,
}

impl Scene {
    /// The events a guitar chart is supposed to contain.
    #[must_use]
    pub fn chartable(&self) -> Vec<TruthNote> {
        self.truth
            .iter()
            .copied()
            .filter(|note| note.role == Role::Lead)
            .collect()
    }

    /// Events that must NOT drive the chart (drums, bass, vocals).
    #[must_use]
    pub fn distractors(&self) -> Vec<TruthNote> {
        self.truth
            .iter()
            .copied()
            .filter(|note| note.role != Role::Lead)
            .collect()
    }
}

/// Builder that renders audio and records ground truth in one step,
/// so the two can never drift apart.
struct Stage {
    samples: Vec<f32>,
    truth: Vec<TruthNote>,
    noise: Noise,
}

impl Stage {
    fn new(duration_s: f64, seed: u64) -> Stage {
        Stage {
            samples: vec![0.0; (duration_s * f64::from(RATE)) as usize],
            truth: Vec::new(),
            noise: Noise::new(seed),
        }
    }

    /// A plucked lead note (the guitar).
    fn lead(&mut self, time_s: f64, midi: f32, hold_s: f64, gain: f32) -> &mut Stage {
        add_pluck(&mut self.samples, RATE, time_s, midi_hz(midi), hold_s, gain);
        self.truth.push(TruthNote {
            time_s,
            end_s: time_s + hold_s,
            midi: Some(midi),
            role: Role::Lead,
        });
        self
    }

    /// A genuinely held lead tone (no pluck decay).
    fn lead_held(&mut self, time_s: f64, midi: f32, hold_s: f64, gain: f32) -> &mut Stage {
        add_sustained(&mut self.samples, RATE, time_s, midi_hz(midi), hold_s, gain);
        self.truth.push(TruthNote {
            time_s,
            end_s: time_s + hold_s,
            midi: Some(midi),
            role: Role::Lead,
        });
        self
    }

    fn bass(&mut self, time_s: f64, midi: f32, hold_s: f64, gain: f32) -> &mut Stage {
        add_pluck(&mut self.samples, RATE, time_s, midi_hz(midi), hold_s, gain);
        self.truth.push(TruthNote {
            time_s,
            end_s: time_s + hold_s,
            midi: Some(midi),
            role: Role::Bass,
        });
        self
    }

    fn vocal(&mut self, time_s: f64, midi: f32, hold_s: f64, gain: f32) -> &mut Stage {
        add_voice(&mut self.samples, RATE, time_s, midi_hz(midi), hold_s, gain);
        self.truth.push(TruthNote {
            time_s,
            end_s: time_s + hold_s,
            midi: Some(midi),
            role: Role::Vocal,
        });
        self
    }

    fn kick(&mut self, time_s: f64, gain: f32) -> &mut Stage {
        add_kick(&mut self.samples, RATE, time_s, gain);
        self.percussion(time_s, 0.16);
        self
    }

    fn snare(&mut self, time_s: f64, gain: f32) -> &mut Stage {
        add_snare(&mut self.samples, RATE, time_s, gain, &mut self.noise);
        self.percussion(time_s, 0.13);
        self
    }

    fn hat(&mut self, time_s: f64, gain: f32) -> &mut Stage {
        add_hat(&mut self.samples, RATE, time_s, gain, &mut self.noise);
        self.percussion(time_s, 0.045);
        self
    }

    fn percussion(&mut self, time_s: f64, len_s: f64) {
        self.truth.push(TruthNote {
            time_s,
            end_s: time_s + len_s,
            midi: None,
            role: Role::Percussion,
        });
    }

    fn finish(mut self, name: &'static str, about: &'static str, bpm: f64) -> Scene {
        self.truth.sort_by(|a, b| {
            a.time_s
                .partial_cmp(&b.time_s)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        Scene {
            name,
            about,
            audio: AudioData::from_mono(self.samples, RATE),
            bpm,
            truth: self.truth,
        }
    }
}

/// **A — Simple melody.** One clean plucked line, nothing else. If a
/// pipeline cannot get this exactly right, nothing downstream matters.
#[must_use]
pub fn simple_melody() -> Scene {
    let bpm = 100.0;
    let beat = 60.0 / bpm;
    let mut stage = Stage::new(14.0, 0x5EED_0001);
    // An up-and-down scale in quarter notes: unambiguous contour.
    let line = [60.0, 62.0, 64.0, 65.0, 67.0, 65.0, 64.0, 62.0];
    for (bar, _) in (0..2).enumerate() {
        for (i, midi) in line.iter().enumerate() {
            let t = 1.0 + (bar * line.len() + i) as f64 * beat;
            stage.lead(t, *midi, beat * 0.85, 0.7);
        }
    }
    stage.finish(
        "a_simple_melody",
        "one clean plucked line, no accompaniment",
        bpm,
    )
}

/// **B — Guitar riff.** Fast sixteenth-note figure: tests onset
/// resolution and whether the pitch contour survives density.
#[must_use]
pub fn guitar_riff() -> Scene {
    let bpm = 140.0;
    let beat = 60.0 / bpm;
    let step = beat / 4.0;
    let mut stage = Stage::new(13.0, 0x5EED_0002);
    // A pentatonic figure, repeated — the shape must stay recognizable.
    let figure = [40.0, 43.0, 45.0, 43.0, 40.0, 45.0, 47.0, 45.0];
    for repeat in 0..12 {
        for (i, midi) in figure.iter().enumerate() {
            let t = 1.0 + (repeat * figure.len() + i) as f64 * step;
            stage.lead(t, *midi, step * 0.9, 0.7);
        }
    }
    stage.finish(
        "b_guitar_riff",
        "fast sixteenth-note riff with a repeating pitch figure",
        bpm,
    )
}

/// **C — Chords.** Real simultaneous harmony: three pitches sharing
/// one attack. A chart may widen these; it must not invent chords
/// where only a drum and a melody note coincide (see scene D).
#[must_use]
pub fn chords() -> Scene {
    let bpm = 90.0;
    let beat = 60.0 / bpm;
    let mut stage = Stage::new(15.0, 0x5EED_0003);
    // I–V–vi–IV triads, one per bar, ringing most of the bar.
    let triads = [
        [52.0, 56.0, 59.0],
        [59.0, 63.0, 66.0],
        [57.0, 60.0, 64.0],
        [53.0, 57.0, 60.0],
    ];
    // Two passes of the progression: 16 beats at 90 BPM ≈ 10.7 s,
    // which fits the 15 s stage with the final chord's ring.
    for bar in 0..2 {
        for (i, triad) in triads.iter().enumerate() {
            let t = 1.0 + (bar * triads.len() + i) as f64 * beat * 2.0;
            for midi in triad {
                stage.lead(t, *midi, beat * 1.8, 0.5);
            }
        }
    }
    stage.finish(
        "c_chords",
        "true triads: three pitches sharing one attack",
        bpm,
    )
}

/// **D — Drums plus guitar.** The drums are *louder* than the guitar,
/// exactly as in a real mix. The chart must still follow the guitar.
#[must_use]
pub fn drums_and_guitar() -> Scene {
    let bpm = 120.0;
    let beat = 60.0 / bpm;
    let mut stage = Stage::new(14.0, 0x5EED_0004);
    let bars = 6;
    for bar in 0..bars {
        let bar_start = 1.0 + bar as f64 * beat * 4.0;
        // Kick on 1 and 3, snare on 2 and 4, hats on every eighth.
        stage.kick(bar_start, 0.95);
        stage.kick(bar_start + beat * 2.0, 0.95);
        stage.snare(bar_start + beat, 0.8);
        stage.snare(bar_start + beat * 3.0, 0.8);
        for eighth in 0..8 {
            stage.hat(bar_start + f64::from(eighth) * beat * 0.5, 0.35);
        }
        // The guitar line: quarters, quieter than the drums.
        let line = [52.0, 55.0, 59.0, 55.0];
        for (i, midi) in line.iter().enumerate() {
            stage.lead(bar_start + i as f64 * beat, *midi, beat * 0.8, 0.55);
        }
        // A bass under it all: pitched, low, and loud — the other
        // thing that steals a pitch tracker away from the guitar.
        stage.bass(bar_start, 33.0, beat * 1.9, 0.8);
        stage.bass(bar_start + beat * 2.0, 40.0, beat * 1.9, 0.8);
    }
    stage.finish(
        "d_drums_and_guitar",
        "loud drums and a loud bass over a quieter guitar line",
        bpm,
    )
}

/// **E — Vocals plus guitar.** A voice with vibrato sits above the
/// riff. The guitar is what a charter charts; the voice is the trap.
#[must_use]
pub fn vocals_and_guitar() -> Scene {
    let bpm = 110.0;
    let beat = 60.0 / bpm;
    let mut stage = Stage::new(15.0, 0x5EED_0005);
    let bars = 6;
    for bar in 0..bars {
        let bar_start = 1.0 + bar as f64 * beat * 4.0;
        // Guitar: steady eighths, plucked.
        let figure = [45.0, 45.0, 48.0, 50.0, 52.0, 50.0, 48.0, 45.0];
        for (i, midi) in figure.iter().enumerate() {
            stage.lead(bar_start + i as f64 * beat * 0.5, *midi, beat * 0.45, 0.6);
        }
        // Voice: two long notes per bar, LOUDER, higher, with vibrato.
        stage.vocal(bar_start, 69.0, beat * 1.8, 0.85);
        stage.vocal(bar_start + beat * 2.0, 72.0, beat * 1.8, 0.85);
    }
    stage.finish(
        "e_vocals_and_guitar",
        "a loud vibrato voice above the guitar figure",
        bpm,
    )
}

/// **F — Sustains.** Long, genuinely held tones separated by rests.
/// Each must become ONE event of its real length, not a note pile.
#[must_use]
pub fn sustains() -> Scene {
    let bpm = 80.0;
    let mut stage = Stage::new(18.0, 0x5EED_0006);
    // Beat-aligned on purpose: a scene may only claim a tempo its
    // audio actually implies. The first version placed the holds at
    // arbitrary times and then asserted 80 BPM — the analyzer was
    // marked wrong for disagreeing with a number nothing supported.
    let beat = 60.0 / bpm; // 0.75 s
    let holds = [
        (2.0, 55.0, 2.0),
        (6.0, 60.0, 3.0),
        (10.0, 57.0, 1.5),
        (13.0, 62.0, 4.0),
        (18.0, 59.0, 2.0),
    ];
    let holds: Vec<(f64, f32, f64)> = holds
        .iter()
        .map(|&(beats, midi, hold_beats)| (1.0 + beats * beat, midi, hold_beats * beat))
        .collect();
    for (time, midi, hold) in holds {
        stage.lead_held(time, midi, hold, 0.7);
    }
    stage.finish(
        "f_sustains",
        "long held tones that must stay single events",
        bpm,
    )
}

/// **G — Syncopation.** Everything lands on the off-beat. A chart that
/// quantizes these onto the beat has destroyed the song's character.
#[must_use]
pub fn syncopation() -> Scene {
    let bpm = 96.0;
    let beat = 60.0 / bpm;
    // 6 bars at 96 BPM = 15 s of music after the 1 s lead-in.
    let mut stage = Stage::new(17.0, 0x5EED_0007);
    for bar in 0..6 {
        let bar_start = 1.0 + bar as f64 * beat * 4.0;
        // Downbeat, then the "and" of 2, the "and" of 3, the "and" of 4.
        stage.lead(bar_start, 50.0, beat * 0.4, 0.7);
        for offbeat in [1.5, 2.5, 3.5] {
            stage.lead(bar_start + offbeat * beat, 55.0, beat * 0.4, 0.7);
        }
        stage.kick(bar_start, 0.7);
        stage.snare(bar_start + beat * 2.0, 0.6);
    }
    stage.finish(
        "g_syncopation",
        "off-beat figure that must survive quantization",
        bpm,
    )
}

/// **H — Tempo ambiguity.** A half-time feel: the backbeat suggests
/// 75 BPM, the eighth-note guitar suggests 150. The musical answer is
/// 150 — the note grid the player actually plays.
#[must_use]
pub fn tempo_ambiguity() -> Scene {
    let bpm = 150.0;
    let beat = 60.0 / bpm;
    let mut stage = Stage::new(15.0, 0x5EED_0008);
    for bar in 0..7 {
        let bar_start = 1.0 + bar as f64 * beat * 4.0;
        // Half-time drums: kick on 1, snare on 3 only.
        stage.kick(bar_start, 0.9);
        stage.snare(bar_start + beat * 2.0, 0.85);
        // Guitar drives the real pulse: every beat.
        for quarter in 0..4 {
            let midi = if quarter % 2 == 0 { 47.0 } else { 52.0 };
            stage.lead(bar_start + f64::from(quarter) * beat, midi, beat * 0.8, 0.6);
        }
    }
    stage.finish(
        "h_tempo_ambiguity",
        "half-time drums against an on-beat guitar pulse",
        bpm,
    )
}

/// Every scene, in report order.
#[must_use]
pub fn all() -> Vec<Scene> {
    vec![
        simple_melody(),
        guitar_riff(),
        chords(),
        drums_and_guitar(),
        vocals_and_guitar(),
        sustains(),
        syncopation(),
        tempo_ambiguity(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_renders_audible_audio_with_truth() {
        for scene in all() {
            let peak = scene
                .audio
                .samples()
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.05, "{}: silent audio (peak {peak})", scene.name);
            assert!(
                peak < 8.0,
                "{}: absurd level {peak} — layers are summing out of control",
                scene.name
            );
            assert!(!scene.truth.is_empty(), "{}: no ground truth", scene.name);
            assert!(
                !scene.chartable().is_empty(),
                "{}: nothing to chart",
                scene.name
            );
        }
    }

    #[test]
    fn ground_truth_is_ordered_and_sane() {
        for scene in all() {
            let mut previous = f64::NEG_INFINITY;
            for note in &scene.truth {
                assert!(note.time_s >= previous, "{}: truth unsorted", scene.name);
                previous = note.time_s;
                assert!(note.len_s() > 0.0, "{}: zero-length event", scene.name);
                assert!(
                    note.end_s <= scene.audio.duration_s() + 0.5,
                    "{}: event runs past the audio",
                    scene.name
                );
                match note.role {
                    Role::Percussion => assert!(note.midi.is_none()),
                    _ => assert!(
                        note.midi.is_some(),
                        "{}: pitched role without a pitch",
                        scene.name
                    ),
                }
            }
        }
    }

    #[test]
    fn scenes_are_byte_identical_across_runs() {
        for (first, second) in all().into_iter().zip(all()) {
            assert_eq!(
                first.audio.samples(),
                second.audio.samples(),
                "{}: scene synthesis is not deterministic",
                first.name
            );
            assert_eq!(first.truth, second.truth);
        }
    }

    #[test]
    fn the_distractor_scenes_really_are_hostile() {
        // D must have drums LOUDER than the guitar, and E a voice
        // louder than the riff — otherwise those tests prove nothing.
        let drums = drums_and_guitar();
        assert!(
            drums.distractors().len() > drums.chartable().len(),
            "scene D needs more percussion than guitar events"
        );
        let vocals = vocals_and_guitar();
        assert!(
            vocals
                .distractors()
                .iter()
                .all(|note| note.role == Role::Vocal),
            "scene E's only distractor should be the voice"
        );
    }
}
