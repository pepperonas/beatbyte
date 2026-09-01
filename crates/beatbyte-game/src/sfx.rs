//! Procedurally generated sound effects.
//!
//! BeatByte ships no audio binaries; every SFX is synthesized at
//! startup with `beatbyte-audio`'s synthesis tools, WAV-encoded in
//! memory and registered as an engine audio asset. Menu sounds and
//! judgment feedback go through `bevy_audio` (fire-and-forget); the
//! *music* keeps its own thread and clock (ADR-0005).

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::synth::{MISS_VOICE, OVERSTRUM_VOICE};
use beatbyte_audio::wav_bytes_mono16;
use beatbyte_core::SessionEvent;
use bevy::audio::Volume;
use bevy::prelude::*;

use crate::config::Settings;
use crate::gameplay::SessionFeedback;
use crate::states::AppState;

/// One UI feedback sound, as a MESSAGE: screens say what happened
/// (the cursor moved, a toggle flipped, a capture conflicted) and one
/// player system turns that into audio. No widget owns an asset, and
/// gamepad or mouse interaction sounds exactly like the keyboard —
/// the message does not know the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Message)]
pub enum UiSound {
    /// The cursor moved.
    Navigate,
    /// A row was activated / a screen entered.
    Confirm,
    /// Backed out of a screen or canceled a capture.
    Back,
    /// Something refused: a conflict, an impossible action.
    Error,
    /// A boolean setting flipped.
    Toggle,
    /// A stepped value nudged.
    Slider,
}

/// Handles to the synthesized effects.
#[derive(Resource)]
pub struct SfxLib {
    /// Menu cursor movement.
    pub ui_move: Handle<AudioSource>,
    /// Menu confirm / start.
    pub ui_confirm: Handle<AudioSource>,
    /// Backing out: the confirm pair, falling instead of rising.
    pub ui_back: Handle<AudioSource>,
    /// Refusal: a short dull buzz, low and unmusical on purpose.
    pub ui_error: Handle<AudioSource>,
    /// A toggle's click: one very short high tick.
    pub ui_toggle: Handle<AudioSource>,
    /// A slider step: softer and lower than the toggle, because it
    /// fires in runs.
    pub ui_slider: Handle<AudioSource>,
    /// A missed note: dark, sagging, the note that never sounded.
    pub miss: Handle<AudioSource>,
    /// A stray strum: brighter and dissonant, a noise that should not
    /// exist. Deliberately a sibling of [`SfxLib::miss`] rather than an
    /// unrelated sound — the two are the same instrument making the
    /// same kind of unwanted noise, and telling them apart should be
    /// possible without being distracting.
    pub overstrum: Handle<AudioSource>,
    /// Hype activation: a rising sweep.
    pub hype: Handle<AudioSource>,
    /// Metronome tick (editor audition overlay).
    pub click: Handle<AudioSource>,
}

/// The sound-effects plugin.
pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UiSound>()
            .add_systems(Startup, build_sfx)
            .add_systems(
                Update,
                (
                    play_ui_sounds,
                    gameplay_sounds.run_if(in_state(AppState::Gameplay)),
                ),
            );
    }
}

/// Synthesize every effect once at startup.
fn build_sfx(mut commands: Commands, mut assets: ResMut<Assets<AudioSource>>) {
    let mut register = |audio: AudioData| -> Handle<AudioSource> {
        assets.add(AudioSource {
            bytes: wav_bytes_mono16(&audio).into(),
        })
    };
    commands.insert_resource(SfxLib {
        ui_move: register(blip(880.0, 0.045, 0.5)),
        ui_confirm: register(confirm()),
        ui_back: register(back()),
        ui_error: register(error()),
        ui_toggle: register(blip(1320.0, 0.03, 0.5)),
        ui_slider: register(blip(660.0, 0.025, 0.35)),
        miss: register(MISS_VOICE.render(44_100)),
        overstrum: register(OVERSTRUM_VOICE.render(44_100)),
        hype: register(riser()),
        click: register(blip(1760.0, 0.03, 0.6)),
    });
}

/// One short square-wave blip.
fn blip(freq: f64, length_s: f64, gain: f32) -> AudioData {
    let rate = 44_100u32;
    let mut samples = vec![0.0f32; (length_s * f64::from(rate)) as usize];
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / f64::from(rate);
        let phase = (freq * t).fract();
        let square = if phase < 0.5 { 1.0f32 } else { -1.0 };
        let envelope = ((-t / (length_s * 0.4)).exp() * (1.0 - (-t / 0.002).exp())) as f32;
        *slot = square * envelope * gain * 0.4;
    }
    AudioData::from_mono(samples, rate)
}

/// Two rising blips: the "go" sound.
fn confirm() -> AudioData {
    let rate = 44_100u32;
    let mut samples = vec![0.0f32; (0.16 * f64::from(rate)) as usize];
    mix(&mut samples, &blip(660.0, 0.07, 0.5), 0);
    mix(
        &mut samples,
        &blip(990.0, 0.09, 0.5),
        (0.06 * f64::from(rate)) as usize,
    );
    AudioData::from_mono(samples, rate)
}

/// Rising pulse sweep for Hype activation.
fn riser() -> AudioData {
    let rate = 44_100u32;
    let length_s = 0.28;
    let mut samples = vec![0.0f32; (length_s * f64::from(rate)) as usize];
    let mut phase = 0.0f64;
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / f64::from(rate);
        let progress = t / length_s;
        let freq = 220.0 * 4.0f64.powf(progress); // 220 → 880 Hz
        phase += freq / f64::from(rate);
        let pulse = if phase.fract() < 0.3 { 1.0f32 } else { -1.0 };
        let envelope = ((progress * core::f64::consts::PI).sin()) as f32;
        *slot = pulse * envelope * 0.22;
    }
    AudioData::from_mono(samples, rate)
}

/// Add `source` into `target` starting at `offset` samples.
fn mix(target: &mut [f32], source: &AudioData, offset: usize) {
    for (i, &sample) in source.samples().iter().enumerate() {
        if let Some(slot) = target.get_mut(offset + i) {
            *slot += sample;
        }
    }
}

/// Whether this event is a stray strum rather than a missed note.
///
/// The two get different sounds, so which one plays comes down to this
/// single question — worth naming, and worth pinning, because getting
/// it backwards would be inaudible in review and obvious in play.
fn is_stray_strum(event: &SessionEvent) -> bool {
    matches!(event, SessionEvent::Overstrum)
}

/// Play a one-shot effect at the configured volume.
pub(crate) fn play(commands: &mut Commands, handle: &Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}

/// The confirm pair, falling: leaving is the inverse of arriving.
fn back() -> AudioData {
    let rate = 44_100u32;
    let mut samples = vec![0.0f32; (0.16 * f64::from(rate)) as usize];
    mix(&mut samples, &blip(990.0, 0.07, 0.5), 0);
    mix(
        &mut samples,
        &blip(660.0, 0.09, 0.5),
        (0.06 * f64::from(rate)) as usize,
    );
    AudioData::from_mono(samples, rate)
}

/// A refusal: low, brief, deliberately unmusical next to the blips.
fn error() -> AudioData {
    let rate = 44_100u32;
    let length_s = 0.11;
    let mut samples = vec![0.0f32; (length_s * f64::from(rate)) as usize];
    for (i, slot) in samples.iter_mut().enumerate() {
        let t = i as f64 / f64::from(rate);
        let phase = (110.0 * t).fract();
        let square = if phase < 0.5 { 1.0f32 } else { -1.0 };
        let envelope = ((-t / (length_s * 0.5)).exp() * (1.0 - (-t / 0.004).exp())) as f32;
        *slot = square * envelope * 0.28;
    }
    AudioData::from_mono(samples, rate)
}

/// Turn [`UiSound`] messages into audio — the one place UI feedback
/// makes noise, on every screen and for every input device.
fn play_ui_sounds(
    mut commands: Commands,
    mut sounds: MessageReader<UiSound>,
    sfx: Res<SfxLib>,
    settings: Res<Settings>,
) {
    for sound in sounds.read() {
        let handle = match sound {
            UiSound::Navigate => &sfx.ui_move,
            UiSound::Confirm => &sfx.ui_confirm,
            UiSound::Back => &sfx.ui_back,
            UiSound::Error => &sfx.ui_error,
            UiSound::Toggle => &sfx.ui_toggle,
            UiSound::Slider => &sfx.ui_slider,
        };
        play(&mut commands, handle, settings.sfx_volume);
    }
}

/// Gameplay: misses thud, Hype roars. Hits stay silent — the music
/// *is* the hit sound; a click on every note would fight it.
fn gameplay_sounds(
    mut commands: Commands,
    mut feedback: MessageReader<SessionFeedback>,
    sfx: Res<SfxLib>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut last_miss: Local<f32>,
) {
    for message in feedback.read() {
        match message.event {
            SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum => {
                // Rate-limit: a chain of mistakes is one bad moment,
                // not a drum roll. The limiter is shared across both
                // kinds on purpose - a fumble usually produces a miss
                // and a stray strum together, and hearing both would
                // double the punishment for one mistake.
                let now = time.elapsed_secs();
                if now - *last_miss > 0.12 {
                    *last_miss = now;
                    let sound = if is_stray_strum(&message.event) {
                        &sfx.overstrum
                    } else {
                        &sfx.miss
                    };
                    play(&mut commands, sound, settings.sfx_volume);
                }
            }
            SessionEvent::HypeActivated => {
                play(&mut commands, &sfx.hype, settings.sfx_volume);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{back, blip, confirm, error, is_stray_strum};
    use beatbyte_core::SessionEvent;

    #[test]
    fn a_stray_strum_is_told_from_a_missed_note() {
        assert!(is_stray_strum(&SessionEvent::Overstrum));
        assert!(!is_stray_strum(&SessionEvent::NoteMissed {
            event_index: 0
        }));
    }

    /// Roughly how high a stretch of samples sounds: sign changes
    /// per sample.
    fn zero_crossing_rate(samples: &[f32]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
            .count();
        crossings as f32 / samples.len().max(1) as f32
    }

    #[test]
    fn the_back_voice_falls_where_confirm_rises() {
        // Leaving is the inverse of arriving: confirm's second blip
        // sits HIGHER than its first, back's second sits LOWER. If
        // someone swaps the pair, backing out starts sounding like
        // starting - inaudible in review, wrong in the hand.
        // Windows centred on each blip (they start at 0 and 0.06 s),
        // not naive halves: the halves overlap both blips and the
        // margins drowned in the mixture.
        let pitch_shape = |audio: super::AudioData| {
            let samples = audio.samples();
            let at = |from_s: f64, len_s: f64| {
                let a = (from_s * 44_100.0) as usize;
                let b = ((from_s + len_s) * 44_100.0) as usize;
                zero_crossing_rate(&samples[a..b.min(samples.len())])
            };
            (at(0.0, 0.05), at(0.07, 0.05))
        };
        let (confirm_a, confirm_b) = pitch_shape(confirm());
        let (back_a, back_b) = pitch_shape(back());
        // With margins: trailing silence alone shaves a few crossings
        // off any second half, so a plain `<` stayed green when both
        // blips were flattened to one pitch (found by mutation).
        assert!(confirm_b > confirm_a * 1.2, "confirm must clearly rise");
        assert!(back_b < back_a * 0.8, "back must clearly fall");
    }

    #[test]
    fn the_error_voice_sits_below_the_blips() {
        // A refusal must not sound like one more cheerful tick.
        let err = error();
        let move_blip = blip(880.0, 0.045, 0.5);
        assert!(
            zero_crossing_rate(err.samples()) < zero_crossing_rate(move_blip.samples()) * 0.5,
            "the error voice is not clearly lower than the move blip"
        );
    }
}
