//! Game feel: particles, screen shake, beat pulse, Hype ambience.
//!
//! Priorities, in order: readability > timing clarity > feedback >
//! spectacle. Everything here is decoration around the judgment
//! stream — none of it touches gameplay state. All effects respect
//! [`EffectSettings`] so the accessibility milestone only has to flip
//! flags.

use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;

use super::{GameplayScreen, PlayerSession, RECEPTOR_Y, SessionFeedback, lane_x};
use crate::audio_sys::GameClock;
use crate::palette;
use crate::states::{AppState, GamePhase};

/// Effect toggles (surfaced in settings at the UI milestone).
#[derive(Resource, Debug, Clone, Copy)]
pub struct EffectSettings {
    /// Particle bursts on hits and sustains.
    pub particles: bool,
    /// Camera shake on misses and Hype activation.
    pub screen_shake: bool,
    /// Stage elements pulsing with the beat.
    pub beat_pulse: bool,
}

impl Default for EffectSettings {
    fn default() -> Self {
        EffectSettings {
            particles: true,
            screen_shake: true,
            beat_pulse: true,
        }
    }
}

/// Hard cap on live particles (spectacle never wins over frame time).
const MAX_PARTICLES: usize = 600;

/// A pixel-confetti particle.
#[derive(Component)]
struct Particle {
    velocity: Vec2,
    age: f32,
    ttl: f32,
    gravity: f32,
}

/// Trauma-based camera shake (decays; offset ∝ trauma²).
#[derive(Resource, Default)]
pub struct Shake {
    trauma: f32,
}

impl Shake {
    /// Add trauma (clamped to 1.0).
    pub fn add(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).min(1.0);
    }
}

/// Marker: the highway bed sprite (beat pulse target).
#[derive(Component)]
pub struct HighwayBed;

/// Marker: the translucent overlay that breathes while Hype runs.
#[derive(Component)]
struct HypeOverlay;

/// Marker: full-screen flash on combo break.
#[derive(Component)]
struct BreakFlash {
    age: f32,
}

/// The game-feel plugin (registered by the gameplay plugin).
pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectSettings>()
            .init_resource::<Shake>()
            .add_systems(OnEnter(AppState::Gameplay), spawn_fx_scenery)
            .add_systems(
                Update,
                (react_to_feedback, sustain_sparks, beat_pulse, hype_ambience)
                    .run_if(in_state(GamePhase::Playing)),
            )
            .add_systems(
                Update,
                (simulate_particles, animate_break_flash, apply_shake)
                    .run_if(in_state(AppState::Gameplay)),
            )
            .add_systems(OnExit(AppState::Gameplay), reset_camera);
    }
}

/// Fx-owned scenery: the Hype overlay above the highway bed.
fn spawn_fx_scenery(mut commands: Commands) {
    commands.spawn((
        GameplayScreen,
        HypeOverlay,
        Sprite::from_color(
            palette::HYPE.with_alpha(0.0),
            Vec2::new(super::LANE_STEP * 5.0 + 24.0, 900.0),
        ),
        Transform::from_xyz(0.0, 0.0, -8.0),
    ));
}

/// Turn judgment events into bursts, shake and flashes.
fn react_to_feedback(
    mut commands: Commands,
    mut feedback: MessageReader<SessionFeedback>,
    players: Query<&PlayerSession>,
    settings: Res<EffectSettings>,
    mut shake: ResMut<Shake>,
    particles: Query<(), With<Particle>>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let mut live_particles = particles.iter().count();

    for message in feedback.read() {
        match message.event {
            SessionEvent::NoteHit {
                event_index,
                judgment,
                ..
            } => {
                if !settings.particles {
                    continue;
                }
                let event = player.session.track().events()[event_index];
                let (count, speed, spice) = match judgment {
                    Judgment::Perfect => (18, 300.0, true),
                    Judgment::Great => (12, 240.0, false),
                    _ => (7, 180.0, false),
                };
                for lane in event.lanes.iter() {
                    let color = palette::lane_color(lane);
                    let x = lane_x(lane);
                    spawn_burst(
                        &mut commands,
                        &mut live_particles,
                        Vec2::new(x, RECEPTOR_Y),
                        color,
                        count,
                        speed,
                        event_index,
                    );
                    if spice {
                        // A few white sparks make Perfect feel electric.
                        spawn_burst(
                            &mut commands,
                            &mut live_particles,
                            Vec2::new(x, RECEPTOR_Y),
                            Color::WHITE,
                            5,
                            speed * 1.4,
                            event_index + 7,
                        );
                    }
                }
            }
            SessionEvent::NoteMissed { .. } => {
                if settings.screen_shake {
                    shake.add(0.35);
                }
                commands.spawn((
                    GameplayScreen,
                    BreakFlash { age: 0.0 },
                    Sprite::from_color(palette::MISS.with_alpha(0.10), Vec2::new(4000.0, 4000.0)),
                    Transform::from_xyz(0.0, 0.0, 20.0),
                ));
            }
            SessionEvent::Overstrum if settings.screen_shake => {
                shake.add(0.22);
            }
            SessionEvent::HypeActivated => {
                if settings.screen_shake {
                    shake.add(0.30);
                }
                // A celebratory ring of sparks across all lanes.
                if settings.particles {
                    for lane in beatbyte_core::Lane::ALL {
                        spawn_burst(
                            &mut commands,
                            &mut live_particles,
                            Vec2::new(lane_x(lane), RECEPTOR_Y),
                            palette::HYPE,
                            10,
                            340.0,
                            lane.index() * 13,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Deterministic-ish particle burst (seeded by note index — no RNG
/// dependency, no frame-order sensitivity).
fn spawn_burst(
    commands: &mut Commands,
    live: &mut usize,
    origin: Vec2,
    color: Color,
    count: usize,
    speed: f32,
    seed: usize,
) {
    for i in 0..count {
        if *live >= MAX_PARTICLES {
            return;
        }
        *live += 1;
        let h = hash01(seed * 31 + i);
        let h2 = hash01(seed * 57 + i * 3 + 1);
        // Upward-biased fan.
        let angle = core::f32::consts::PI * (0.15 + 0.7 * h);
        let magnitude = speed * (0.5 + 0.8 * h2);
        let size = 3.0 + 4.0 * hash01(seed + i * 11);
        commands.spawn((
            GameplayScreen,
            Particle {
                velocity: Vec2::new(angle.cos() * magnitude, angle.sin() * magnitude),
                age: 0.0,
                ttl: 0.35 + 0.3 * h,
                gravity: 700.0,
            },
            Sprite::from_color(color, Vec2::splat(size)),
            Transform::from_xyz(origin.x, origin.y, 6.0),
        ));
    }
}

/// Cheap deterministic hash → 0.0..1.0.
fn hash01(seed: usize) -> f32 {
    let mut x = seed as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x >> 40) as f32) / 16_777_216.0
}

/// Integrate particles: gravity, fade, shrink, despawn.
fn simulate_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, mut sprite) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.ttl {
            commands.entity(entity).despawn();
            continue;
        }
        let gravity = particle.gravity;
        particle.velocity.y -= gravity * dt;
        transform.translation.x += particle.velocity.x * dt;
        transform.translation.y += particle.velocity.y * dt;
        let life = 1.0 - particle.age / particle.ttl;
        sprite.color = sprite.color.with_alpha(life);
    }
}

/// While a sustain is held, the receptor sprays little sparks.
fn sustain_sparks(
    mut commands: Commands,
    players: Query<&PlayerSession>,
    settings: Res<EffectSettings>,
    time: Res<Time>,
    particles: Query<(), With<Particle>>,
    mut accumulator: Local<f32>,
) {
    if !settings.particles {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let Some(index) = player.session.active_sustain() else {
        *accumulator = 0.0;
        return;
    };
    // ~24 sparks per second while holding.
    *accumulator += time.delta_secs() * 24.0;
    if *accumulator < 1.0 {
        return;
    }
    let mut live = particles.iter().count();
    let event = player.session.track().events()[index];
    let ticks = *accumulator as usize;
    *accumulator -= ticks as f32;
    for (i, lane) in event.lanes.iter().enumerate() {
        spawn_burst(
            &mut commands,
            &mut live,
            Vec2::new(lane_x(lane), RECEPTOR_Y + 10.0),
            palette::lane_color(lane),
            ticks.min(2),
            130.0,
            index * 101 + i + (time.elapsed_secs() * 60.0) as usize,
        );
    }
}

/// The stage breathes with the music: highway bed and receptor ring
/// brightness follow the beat grid.
fn beat_pulse(
    players: Query<&PlayerSession>,
    settings: Res<EffectSettings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut bed: Query<&mut Sprite, With<HighwayBed>>,
) {
    if !settings.beat_pulse {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let beats = player.session.track().tempo.beats_at(now);
    if beats < 0.0 {
        return;
    }
    // Sharp attack on the beat, exponential decay across it.
    let phase = beats.fract() as f32;
    let pulse = (-phase * 6.0).exp();
    let boost = if player.session.performance().hype_active() {
        1.8
    } else {
        1.0
    };
    if let Ok(mut sprite) = bed.single_mut() {
        let base = palette::SURFACE.to_linear();
        let lift = 1.0 + 0.55 * pulse * boost;
        sprite.color = Color::LinearRgba(LinearRgba {
            red: base.red * lift,
            green: base.green * lift,
            blue: base.blue * lift * 1.05,
            alpha: 1.0,
        });
    }
}

/// The Hype overlay breathes while Hype is active and hints at a full
/// meter before activation.
fn hype_ambience(
    players: Query<&PlayerSession>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut overlay: Query<&mut Sprite, With<HypeOverlay>>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let Ok(mut sprite) = overlay.single_mut() else {
        return;
    };
    let perf = player.session.performance();
    let alpha = if perf.hype_active() {
        let beats = game_clock
            .song_time(&time)
            .map(|now| player.session.track().tempo.beats_at(now))
            .unwrap_or(0.0);
        0.10 + 0.05 * ((beats * core::f64::consts::PI).sin().abs() as f32)
    } else if perf.hype_meter() >= perf.config().hype_activation_threshold {
        0.045
    } else {
        0.0
    };
    sprite.color = palette::HYPE.with_alpha(alpha);
}

/// Fade the combo-break flash.
fn animate_break_flash(
    mut commands: Commands,
    time: Res<Time>,
    mut flashes: Query<(Entity, &mut BreakFlash, &mut Sprite)>,
) {
    for (entity, mut flash, mut sprite) in &mut flashes {
        flash.age += time.delta_secs();
        let life = 1.0 - flash.age / 0.25;
        if life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        sprite.color = sprite.color.with_alpha(0.10 * life);
    }
}

/// Apply decaying trauma shake to the camera.
fn apply_shake(
    mut shake: ResMut<Shake>,
    settings: Res<EffectSettings>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
) {
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    if !settings.screen_shake || shake.trauma <= 0.0 {
        transform.translation.x = 0.0;
        transform.translation.y = 0.0;
        return;
    }
    shake.trauma = (shake.trauma - time.delta_secs() * 1.6).max(0.0);
    let strength = shake.trauma * shake.trauma * 7.0;
    let t = time.elapsed_secs() * 45.0;
    // Two incommensurate sines ≈ cheap smooth noise.
    transform.translation.x = strength * ((t * 1.3).sin() + (t * 2.17).sin()) * 0.5;
    transform.translation.y = strength * ((t * 1.7).cos() + (t * 2.71).sin()) * 0.5;
}

/// Leave the camera exactly where menus expect it.
fn reset_camera(mut camera: Query<&mut Transform, With<Camera2d>>) {
    if let Ok(mut transform) = camera.single_mut() {
        transform.translation.x = 0.0;
        transform.translation.y = 0.0;
    }
}
