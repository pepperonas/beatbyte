//! Latency calibration: tap along with a click track, measure the
//! median offset, store it in settings.
//!
//! The offset model (ADR-0004): a positive offset means the player's
//! taps arrive *late* relative to the song timeline (typical
//! audio/display latency). Gameplay subtracts the offset from input
//! timestamps, so a correctly calibrated player judges as on-time.

use beatbyte_audio::decode::AudioData;
use bevy::prelude::*;

use crate::audio_sys::{GameClock, Music};
use crate::config::{Settings, save_settings};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// Calibration click tempo.
const CLICK_BPM: f64 = 120.0;

/// Seconds between clicks.
const CLICK_PERIOD_S: f64 = 60.0 / CLICK_BPM;

/// Length of the generated click track.
const TRACK_S: f64 = 120.0;

/// Taps needed before the result is trustworthy.
const MIN_TAPS: usize = 8;

/// Collected taps and the running result.
#[derive(Resource, Default)]
struct Calibration {
    /// Signed offsets (tap − nearest click) in seconds.
    offsets: Vec<f64>,
}

impl Calibration {
    /// Median offset in milliseconds, once enough taps exist.
    fn median_ms(&self) -> Option<f64> {
        if self.offsets.len() < MIN_TAPS {
            return None;
        }
        let mut sorted = self.offsets.clone();
        sorted.sort_by(f64::total_cmp);
        Some(sorted[sorted.len() / 2] * 1000.0)
    }
}

/// Plugin for the calibration screen.
pub struct CalibrationPlugin;

impl Plugin for CalibrationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Calibration>()
            .add_systems(OnEnter(AppState::Calibration), start_calibration)
            .add_systems(
                Update,
                (calibration_input, refresh_readout, pulse_beat_dot)
                    .run_if(in_state(AppState::Calibration)),
            )
            .add_systems(OnExit(AppState::Calibration), stop_calibration);
    }
}

#[derive(Component)]
struct CalibrationScreen;

/// The live readout line.
#[derive(Component)]
struct Readout;

/// The dot that flashes on every click.
#[derive(Component)]
struct BeatDot;

/// Build the click track: a dry tick every beat at 120 BPM.
fn click_track() -> AudioData {
    let rate = 44_100u32;
    let mut samples = vec![0.0f32; (TRACK_S * f64::from(rate)) as usize];
    let mut t = 0.0;
    while t < TRACK_S {
        beatbyte_audio::synth::add_burst(&mut samples, rate, t, 1_500.0, 0.02, 0.8);
        t += CLICK_PERIOD_S;
    }
    AudioData::from_mono(samples, rate)
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn start_calibration(
    mut commands: Commands,
    font: Res<UiFont>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
    mut calibration: ResMut<Calibration>,
    time: Res<Time>,
    settings: Res<Settings>,
    muted: Res<crate::mute::Muted>,
) {
    calibration.offsets.clear();
    music.0.play_buffer(click_track());
    music.0.set_volume(settings.music_volume * muted.factor());
    game_clock.clock.start(time.elapsed_secs_f64(), 0.0);

    commands
        .spawn((CalibrationScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "CALIBRATION", "tap SPACE on every click");
            parent
                .spawn(ui_kit::panel_centered())
                .with_children(|panel| {
                    panel.spawn((
                        BeatDot,
                        Node {
                            width: px(26),
                            height: px(26),
                            ..default()
                        },
                        BackgroundColor(palette::dimmed(palette::BRAND, 0.3)),
                    ));
                    panel.spawn((
                        Readout,
                        Text::new(""),
                        font.text(ui_kit::ROW),
                        TextColor(palette::TEXT),
                    ));
                });
            ui_kit::footer(parent, &font, "ENTER save  ESC cancel");
        });
}

fn calibration_input(
    keys: Res<ButtonInput<KeyCode>>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut calibration: ResMut<Calibration>,
    mut settings: ResMut<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Space)
        && let Some(now) = game_clock.song_time(&time)
    {
        // Signed distance to the nearest click.
        let position = now / CLICK_PERIOD_S;
        let offset = (position - position.round()) * CLICK_PERIOD_S;
        calibration.offsets.push(offset);
    }
    if keys.just_pressed(KeyCode::Enter)
        && let Some(median) = calibration.median_ms()
    {
        settings.latency_offset_ms = (median as f32).clamp(-250.0, 250.0);
        save_settings(&settings);
        info!("calibration saved: {:+.0} ms", settings.latency_offset_ms);
        next_state.set(AppState::MainMenu);
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_readout(
    calibration: Res<Calibration>,
    settings: Res<Settings>,
    mut readout: Query<&mut Text, With<Readout>>,
) {
    let Ok(mut text) = readout.single_mut() else {
        return;
    };
    let line = match calibration.median_ms() {
        Some(median) => format!(
            "taps {:>2}   offset {median:+.0} ms   (ENTER saves)",
            calibration.offsets.len()
        ),
        None => format!(
            "taps {:>2}   need {MIN_TAPS} for a reading   current {:+.0} ms",
            calibration.offsets.len(),
            settings.latency_offset_ms
        ),
    };
    if text.0 != line {
        text.0 = line;
    }
}

/// The dot flashes exactly on the click grid — a visual anchor.
fn pulse_beat_dot(
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut dots: Query<&mut BackgroundColor, With<BeatDot>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let phase = ((now / CLICK_PERIOD_S).fract()) as f32;
    let pulse = (-phase * 8.0).exp();
    for mut color in &mut dots {
        let base = palette::BRAND.to_linear();
        let level = 0.25 + 0.75 * pulse;
        color.0 = Color::LinearRgba(bevy::color::LinearRgba {
            red: base.red * level,
            green: base.green * level,
            blue: base.blue * level,
            alpha: 1.0,
        });
    }
}

fn stop_calibration(
    mut commands: Commands,
    entities: Query<Entity, With<CalibrationScreen>>,
    music: Res<Music>,
    mut game_clock: ResMut<GameClock>,
) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    music.0.stop();
    game_clock.clock.stop();
}

#[cfg(test)]
mod tests {
    use super::{Calibration, MIN_TAPS};

    fn with(offsets: &[f64]) -> Calibration {
        Calibration {
            offsets: offsets.to_vec(),
        }
    }

    #[test]
    fn too_few_taps_produce_no_verdict() {
        // Latency measured from two taps is noise wearing a number,
        // and the number would then be written into settings and
        // silently shift every judgment window afterwards.
        for count in 0..MIN_TAPS {
            let offsets = vec![0.020; count];
            assert!(
                with(&offsets).median_ms().is_none(),
                "{count} taps should not yield an offset"
            );
        }
        assert!(with(&[0.020; MIN_TAPS]).median_ms().is_some());
    }

    #[test]
    fn the_offset_is_reported_in_milliseconds() {
        // Taps are recorded in seconds and the setting is in
        // milliseconds; a factor of a thousand in the wrong place
        // would be invisible in the code and ruinous in play.
        let offsets = vec![0.020; MIN_TAPS];
        let median = with(&offsets).median_ms().expect("enough taps");
        assert!((median - 20.0).abs() < 1e-9, "expected 20 ms, got {median}");
    }

    #[test]
    fn one_wild_tap_does_not_move_the_result() {
        // The median is chosen over the mean precisely for this: a
        // player sneezes, one tap lands half a second out, and the
        // measurement must survive it.
        let mut offsets = vec![0.030; MIN_TAPS];
        let clean = with(&offsets).median_ms().expect("enough taps");
        offsets.push(0.5);
        let with_outlier = with(&offsets).median_ms().expect("enough taps");
        assert!(
            (clean - with_outlier).abs() < 5.0,
            "one outlier moved the median from {clean} to {with_outlier}"
        );
    }

    #[test]
    fn early_and_late_taps_keep_their_sign() {
        // The sign is the whole meaning: it says which way the player
        // is off, and therefore which way to correct.
        let early = with(&[-0.035; MIN_TAPS]).median_ms().expect("taps");
        let late = with(&[0.035; MIN_TAPS]).median_ms().expect("taps");
        assert!(early < 0.0, "an early tap should stay negative: {early}");
        assert!(late > 0.0, "a late tap should stay positive: {late}");
    }
}
