//! Game feel: particles, screen shake, beat pulse, Hype ambience —
//! per-player where it matters.
//!
//! Priorities, in order: readability > timing clarity > feedback >
//! spectacle. Everything here is decoration around the judgment
//! stream — none of it touches gameplay state. All effects respect
//! [`EffectSettings`] so the accessibility settings only flip flags.

use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;

use super::{
    GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, RECEPTOR_Y, SessionFeedback,
};
use crate::audio_sys::GameClock;
use crate::palette;
use crate::states::{AppState, GamePhase};

/// Effect toggles (surfaced in the settings screen).
#[derive(Resource, Debug, Clone, Copy)]
pub struct EffectSettings {
    /// Particle bursts on hits and sustains.
    pub particles: bool,
    /// Camera shake on misses and Hype activation.
    pub screen_shake: bool,
    /// Stage elements pulsing with the beat.
    pub beat_pulse: bool,
    /// Backdrop animation (off = a still stage, reduced motion).
    pub backdrop_motion: bool,
    /// Round style: particles render as soft discs, not pixels.
    /// Whether the 3D stage is drawing the highway.
    ///
    /// These sprites are placed with the FLAT layout, and the 3D solo
    /// neck is drawn 1.45× wider than that layout implies — so on the
    /// stage the outer lanes' sparks land at 69 % of the way out,
    /// beside their receptor rather than on it. The stage throws its
    /// own, in world space (`spark3d`); this flag is what stops the
    /// two from doubling up. Same rule the flat notes and the flat
    /// scenery already follow.
    pub stage_3d: bool,
    /// Suppress full-screen flashes (accessibility).
    pub reduced_flashing: bool,
    /// Scales particle counts, shake and flash opacity, 0.0–1.0.
    pub intensity: f32,
}

/// A burst's particle count under the intensity slider: identity at
/// full, nothing at zero, rounded in between. Pure — tested.
#[must_use]
pub fn scaled_count(count: usize, intensity: f32) -> usize {
    (count as f32 * intensity.clamp(0.0, 1.0)).round() as usize
}

/// The full-screen combo-break flash's opacity: gone entirely under
/// reduced flashing, scaled by intensity otherwise. Pure — tested.
#[must_use]
pub fn flash_alpha(reduced_flashing: bool, intensity: f32) -> f32 {
    if reduced_flashing {
        0.0
    } else {
        0.10 * intensity.clamp(0.0, 1.0)
    }
}

impl Default for EffectSettings {
    fn default() -> Self {
        EffectSettings {
            particles: true,
            stage_3d: false,
            screen_shake: true,
            beat_pulse: true,
            backdrop_motion: true,
            reduced_flashing: false,
            intensity: 1.0,
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
    /// A spark off a flame: it cools as it flies (yellow → red) and
    /// rises instead of falling. Round style only — the 8-bit
    /// confetti keeps its colour and its fall.
    ember: bool,
}

/// The gravity a burst gets: confetti falls, embers rise.
///
/// Positive pulls down. An ember's negative value is buoyancy — the
/// upward acceleration of hot gas — which is what makes sparks from
/// a flame climb rather than rain. Pure — tested.
#[must_use]
pub fn burst_gravity(ember: bool) -> f32 {
    if ember { -380.0 } else { 700.0 }
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

/// Marker: a highway bed sprite (beat pulse target).
#[derive(Component)]
pub struct HighwayBed;

/// Marker: the translucent overlay that breathes while a player's
/// Hype runs.
#[derive(Component)]
struct HypeOverlay(usize);

/// Marker: the one full-screen flash quad, spawned once with the rest
/// of the scenery and never again.
///
/// One entity, not one per event. Two reasons, and the second is the
/// important one: a flash at the moment a star-power phrase lands is
/// exactly the moment a stutter would be felt, so nothing is created
/// at the trigger — and with a single quad the alpha CANNOT
/// accumulate, whatever lands on top of whatever.
#[derive(Component)]
struct ScreenFlashQuad;

/// What one kind of full-screen flash looks like.
///
/// The red combo-break flash and the white star-power flash are the
/// same effect with different numbers, so they are the same code with
/// different numbers. A third one is a constant, not a system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlashProfile {
    /// What colour the screen goes.
    pub color: Color,
    /// How opaque it gets at its brightest.
    pub peak: f32,
    /// How long the rise takes.
    pub attack: f32,
    /// How long the whole thing lasts.
    pub life: f32,
}

impl FlashProfile {
    /// A missed note: red, faint, and over in a quarter of a second.
    #[must_use]
    pub fn miss(reduced_flashing: bool, intensity: f32) -> FlashProfile {
        FlashProfile {
            color: palette::MISS,
            peak: flash_alpha(reduced_flashing, intensity),
            attack: 0.0,
            life: 0.25,
        }
    }

    /// A star-power phrase landing whole: white, brighter, and
    /// shorter — a camera flash rather than a wash.
    ///
    /// The peak is held well under half: this fires while notes are
    /// still coming down the neck, and the one thing the effect may
    /// not do is make the next pattern harder to read.
    #[must_use]
    pub fn star(reduced_flashing: bool, intensity: f32) -> FlashProfile {
        FlashProfile {
            color: Color::WHITE,
            // The same accessibility promise the combo-break flash
            // makes, and for the same reason: under REDUCED FLASHING
            // a full-screen flash is not dimmer, it is absent. The
            // neck's glow carries the moment instead.
            peak: if reduced_flashing {
                0.0
            } else {
                STAR_FLASH_PEAK * intensity.clamp(0.0, 1.0)
            },
            attack: 0.03,
            life: super::starpower::SCREEN_S,
        }
    }
}

/// How white the screen goes when a star-power phrase lands.
///
/// **Tuned by looking, after reasoning got it badly wrong.** The
/// first value was 0.28 — the middle of the commissioned range, and
/// defensible on every number available without a screen. On screen
/// it was a whiteout: mean frame luma 2.86× the baseline, the score
/// digits, the hit label, the crowd and the PA all behind a veil,
/// and the neck's own lift invisible underneath it — the opposite of
/// "the highway lights up". The frame that looked right was the one
/// 0.12 s into that flash, at 1.90× and an alpha of ~0.10, which is
/// the combo-break flash's long-settled value. So this sits just
/// above it: a bigger, positive moment in the same class of
/// brightness, not a different order of it.
pub const STAR_FLASH_PEAK: f32 = 0.12;

/// The flash's shape `age` seconds in, 0..1.
///
/// A rise and a square fall — sparks die, they do not switch, which
/// is this game's own recipe everywhere else. With a zero attack the
/// rise is instantaneous, which is what a combo break wants. Pure —
/// tested.
#[must_use]
pub fn flash_curve(age: f32, attack: f32, life: f32) -> f32 {
    if !(0.0..life).contains(&age) || life <= 0.0 {
        return 0.0;
    }
    if attack > 0.0 && age < attack {
        return age / attack;
    }
    let fall = (life - age) / (life - attack).max(f32::EPSILON);
    (fall * fall).clamp(0.0, 1.0)
}

/// The one full-screen flash, and what it is currently doing.
#[derive(Resource, Debug, Default)]
pub struct ScreenFlash {
    /// The profile that is running, if any.
    profile: Option<FlashProfile>,
    /// How far into it.
    age: f32,
}

impl ScreenFlash {
    /// How opaque the screen is right now.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.profile.map_or(0.0, |profile| {
            profile.peak * flash_curve(self.age, profile.attack, profile.life)
        })
    }

    /// Ask for a flash.
    ///
    /// **The brighter event wins**, and that is the whole priority
    /// rule: a request is taken when its peak is at least what the
    /// screen already shows. So a star-power phrase always interrupts
    /// a combo-break flash, a combo break never cuts a star flash
    /// short at its brightest — and the overlay can never jump DOWN
    /// when one replaces the other, which is the thing that would
    /// actually be seen. Pure — tested.
    pub fn request(&mut self, profile: FlashProfile) {
        if profile.peak <= 0.0 {
            return;
        }
        if profile.peak >= self.alpha() {
            self.profile = Some(profile);
            self.age = 0.0;
        }
    }

    /// Age it, and forget it once it is over.
    pub fn advance(&mut self, dt: f32) {
        let Some(profile) = self.profile else {
            return;
        };
        self.age += dt;
        if self.age >= profile.life {
            self.profile = None;
            self.age = 0.0;
        }
    }

    /// Back to nothing.
    pub fn clear(&mut self) {
        self.profile = None;
        self.age = 0.0;
    }

    /// What colour it is, for the one quad that draws it.
    #[must_use]
    pub fn color(&self) -> Color {
        self.profile.map_or(Color::WHITE, |profile| profile.color)
    }
}

/// The game-feel plugin (registered by the gameplay plugin).
pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EffectSettings>()
            .init_resource::<Shake>()
            .init_resource::<ScreenFlash>()
            .add_systems(
                Update,
                (react_to_feedback, sustain_sparks, beat_pulse, hype_ambience)
                    .run_if(in_state(GamePhase::Playing)),
            )
            .add_systems(
                Update,
                (
                    simulate_particles,
                    drive_screen_flash,
                    apply_shake,
                    celebrate_outro,
                )
                    .run_if(in_state(AppState::Gameplay)),
            )
            .add_systems(
                Update,
                flash_on_star_power
                    .after(super::drain_feedback)
                    .before(drive_screen_flash)
                    .run_if(in_state(AppState::Gameplay)),
            )
            .add_systems(
                OnExit(AppState::Gameplay),
                (reset_camera, clear_screen_flash),
            )
            .add_systems(OnEnter(AppState::Gameplay), clear_screen_flash);
    }
}

/// Whether the FLAT sprite bursts are this view's job.
///
/// They are placed with the flat layout, and the 3D solo neck is
/// drawn 1.45× wider than that layout implies — on the stage the
/// outer lanes' sparks land at 69 % of the way out, beside their
/// receptor rather than on it. Measured, then replaced: the stage
/// throws its own in world space (`spark3d`), a held sustain has the
/// receptor flame, and Hype lights the whole venue. The outro's
/// fireworks are the one exception and say so where they stand.
/// Pure — tested.
#[must_use]
pub fn throws_flat_sparks(settings: &EffectSettings) -> bool {
    settings.particles && !settings.stage_3d
}

/// Fx-owned scenery: one Hype overlay per player (spawned by the
/// highway builder's chain, reading the layout).
pub fn spawn_fx_scenery(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    settings: Res<crate::config::Settings>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
) {
    // The one full-screen flash quad, spawned here and reused for
    // every flash of the song — a combo break, a star-power phrase,
    // whatever comes next. It starts hidden and is only ever made
    // visible by `drive_screen_flash`, so nothing has to remember to
    // clean it up.
    commands.spawn((
        GameplayScreen,
        ScreenFlashQuad,
        Sprite::from_color(Color::WHITE.with_alpha(0.0), Vec2::new(4000.0, 4000.0)),
        Transform::from_xyz(0.0, 0.0, 20.0),
        Visibility::Hidden,
    ));
    // The overlay is a 900-pixel-tall vertical band the width of the
    // bed — which is exactly the shape of a highway in the flat and
    // depth views, and nothing like one in 3D. There the neck is a
    // receding plane, so the band misses it entirely and instead
    // washes the venue standing behind the vanishing point: measured,
    // the rear wall forty units back turned violet while the rails in
    // the foreground did not. The 3D stage tints its own surfaces.
    if super::stage3d::active(&settings) {
        return;
    }
    for index in players.iter() {
        commands.spawn((
            GameplayScreen,
            HypeOverlay(index.0),
            Sprite::from_color(
                palette::HYPE.with_alpha(0.0),
                Vec2::new(layout.bed_width(), 900.0),
            ),
            Transform::from_xyz(layout.origin(index.0), 0.0, -8.0),
        ));
    }
}

/// Turn judgment events into bursts, shake and flashes.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn react_to_feedback(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    mut feedback: MessageReader<SessionFeedback>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    settings: Res<EffectSettings>,
    shapes: Res<crate::shapes::LaneShapes>,
    mut shake: ResMut<Shake>,
    mut flash: ResMut<ScreenFlash>,
    particles: Query<(), With<Particle>>,
) {
    let mut live_particles = particles.iter().count();
    let soft = Some(shapes.soft_dot());

    for message in feedback.read() {
        let player = message.player_index;
        match message.event {
            SessionEvent::NoteHit {
                event_index,
                judgment,
                ..
            } => {
                if !throws_flat_sparks(&settings) {
                    continue;
                }
                let Some((_, session)) = players.iter().find(|(index, _)| index.0 == player) else {
                    continue;
                };
                let event = session.session.track().events()[event_index];
                let (count, speed, spice) = match judgment {
                    Judgment::Perfect => (18, 300.0, true),
                    Judgment::Great => (12, 240.0, false),
                    _ => (7, 180.0, false),
                };
                let count = scaled_count(count, settings.intensity);
                for lane in event.lanes.iter() {
                    let color = theme.0.lane_color(lane);
                    let x = layout.lane_x(player, lane);
                    spawn_burst(
                        &mut commands,
                        &mut live_particles,
                        soft.clone(),
                        Vec2::new(x, RECEPTOR_Y),
                        color,
                        count,
                        speed,
                        event_index,
                        true,
                    );
                    if spice {
                        // A few white sparks make Perfect feel electric.
                        spawn_burst(
                            &mut commands,
                            &mut live_particles,
                            soft.clone(),
                            Vec2::new(x, RECEPTOR_Y),
                            Color::WHITE,
                            scaled_count(5, settings.intensity),
                            speed * 1.4,
                            event_index + 7,
                            true,
                        );
                    }
                }
            }
            SessionEvent::NoteMissed { .. } => {
                if settings.screen_shake {
                    shake.add(0.30 * settings.intensity);
                }
                flash.request(FlashProfile::miss(
                    settings.reduced_flashing,
                    settings.intensity,
                ));
            }
            SessionEvent::Overstrum if settings.screen_shake => {
                shake.add(0.20 * settings.intensity);
            }
            SessionEvent::HypeActivated => {
                if settings.screen_shake {
                    shake.add(0.30 * settings.intensity);
                }
                // A celebratory salvo across the player's lanes.
                if throws_flat_sparks(&settings) {
                    for lane in beatbyte_core::Lane::ALL {
                        spawn_burst(
                            &mut commands,
                            &mut live_particles,
                            soft.clone(),
                            Vec2::new(layout.lane_x(player, lane), RECEPTOR_Y),
                            palette::HYPE,
                            scaled_count(10, settings.intensity),
                            340.0,
                            lane.index() * 13 + player * 101,
                            false,
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
#[allow(clippy::too_many_arguments)] // internal helper mirroring the systems' DI
fn spawn_burst(
    commands: &mut Commands,
    live: &mut usize,
    soft: Option<Handle<Image>>,
    origin: Vec2,
    color: Color,
    count: usize,
    speed: f32,
    seed: usize,
    ember: bool,
) {
    for i in 0..count {
        if *live >= MAX_PARTICLES {
            return;
        }
        *live += 1;
        let h = hash01(seed * 31 + i);
        let h2 = hash01(seed * 57 + i * 3 + 1);
        // Upward-biased fan; an ember's fan is narrower — sparks
        // leave a flame upward, not sideways.
        let spread = if ember { 0.45 } else { 0.7 };
        let angle = core::f32::consts::PI * (0.5 - spread / 2.0 + spread * h);
        let magnitude = speed * (0.5 + 0.8 * h2) * if ember { 0.55 } else { 1.0 };
        let size = 3.0 + 4.0 * hash01(seed + i * 11);
        commands.spawn((
            GameplayScreen,
            Particle {
                velocity: Vec2::new(angle.cos() * magnitude, angle.sin() * magnitude),
                age: 0.0,
                ttl: 0.35 + 0.3 * h,
                gravity: burst_gravity(ember),
                ember,
            },
            Sprite {
                image: soft.clone().unwrap_or_default(),
                color,
                custom_size: Some(Vec2::splat(size)),
                ..Default::default()
            },
            Transform::from_xyz(origin.x, origin.y, 6.0),
        ));
    }
}

/// Cheap deterministic hash → 0.0..1.0.
pub(crate) fn hash01(seed: usize) -> f32 {
    let mut x = seed as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x >> 40) as f32) / 16_777_216.0
}

/// Integrate particles: gravity, fade, despawn.
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
        if particle.ember {
            // Cooling: the same ramp the 3D embers use, so the two
            // layers agree on what a dying spark looks like.
            sprite.color = super::flame::ember_color(1.0 - life).with_alpha(life);
        } else {
            sprite.color = sprite.color.with_alpha(life);
        }
    }
}

/// While a sustain is held, its receptor sprays little sparks.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn sustain_sparks(
    mut commands: Commands,
    shapes: Res<crate::shapes::LaneShapes>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    settings: Res<EffectSettings>,
    time: Res<Time>,
    particles: Query<(), With<Particle>>,
    mut accumulator: Local<f32>,
) {
    if !throws_flat_sparks(&settings) {
        return;
    }
    let soft = Some(shapes.soft_dot());
    // Shared spark budget across players.
    *accumulator += time.delta_secs() * 24.0;
    if *accumulator < 1.0 {
        return;
    }
    let ticks = (*accumulator as usize).min(3);
    *accumulator -= ticks as f32;

    let mut live = particles.iter().count();
    for (index, player) in &players {
        let Some(sustain_index) = player.session.active_sustain() else {
            continue;
        };
        let event = player.session.track().events()[sustain_index];
        for (i, lane) in event.lanes.iter().enumerate() {
            spawn_burst(
                &mut commands,
                &mut live,
                soft.clone(),
                Vec2::new(layout.lane_x(index.0, lane), RECEPTOR_Y + 10.0),
                theme.0.lane_color(lane),
                ticks.min(2),
                130.0,
                sustain_index * 101 + i + (time.elapsed_secs() * 60.0) as usize,
                // Sustain sparks are embers too —
                // they leave a burning fret.
                true,
            );
        }
    }
}

/// The stage breathes with the music: every highway bed pulses on the
/// beat grid (harder when anyone is in Hype).
fn beat_pulse(
    players: Query<&PlayerSession>,
    settings: Res<EffectSettings>,
    theme: Res<crate::theme::ActiveTheme>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut beds: Query<&mut Sprite, With<HighwayBed>>,
) {
    if !settings.beat_pulse {
        return;
    }
    let Some(reference) = players.iter().next() else {
        return;
    };
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    let beats = reference.session.track().tempo.beats_at(now);
    if beats < 0.0 {
        return;
    }
    let phase = beats.fract() as f32;
    let pulse = (-phase * 6.0).exp();
    let boost = if players
        .iter()
        .any(|player| player.session.performance().hype_active())
    {
        1.8
    } else {
        1.0
    };
    for mut sprite in &mut beds {
        let base = theme.0.surface.to_linear();
        let lift = 1.0 + theme.0.pulse_strength * pulse * boost;
        sprite.color = Color::LinearRgba(LinearRgba {
            red: base.red * lift,
            green: base.green * lift,
            blue: base.blue * lift * 1.05,
            alpha: 1.0,
        });
    }
}

/// Each player's Hype overlay breathes with their own meter.
fn hype_ambience(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut overlays: Query<(&HypeOverlay, &mut Sprite)>,
) {
    for (index, player) in &players {
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
        for (overlay, mut sprite) in &mut overlays {
            if overlay.0 == index.0 {
                sprite.color = palette::HYPE.with_alpha(alpha);
            }
        }
    }
}

/// Fireworks through the outro: lane-colored bursts marching across
/// the highway every beat-ish while "YOU ROCK!!!" stands. Runs the
/// particle budget and the intensity slider like every other burst.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn celebrate_outro(
    mut commands: Commands,
    time: Res<Time>,
    clock: Option<Res<super::OutroClock>>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    settings: Res<EffectSettings>,
    shapes: Res<crate::shapes::LaneShapes>,
    particles: Query<(), With<Particle>>,
    mut fired: Local<u32>,
) {
    // Deliberately NOT gated on the 3D stage, unlike the hit, hype
    // and sustain sparks above: those have a replacement in world
    // space, and the outro's fireworks do not. Misplaced by the
    // neck's widening is still better than a bare celebration, and
    // the song is over — nothing is being read any more.
    if !settings.particles {
        return;
    }
    let Some(clock) = clock else {
        // A fresh outro: the resource appears with age zero, and the
        // salvo counter must start over with it.
        *fired = 0;
        return;
    };
    if clock.0 == 0.0 {
        *fired = 0;
    }
    let _ = time;
    // One salvo every 0.8 s, deterministic from the clock.
    let due = (clock.0 / 0.8) as u32 + 1;
    if *fired >= due {
        return;
    }
    *fired = due;
    let mut live = particles.iter().count();
    let soft = Some(shapes.soft_dot());
    // The salvo walks the lanes so the whole highway celebrates.
    let lane = (due as usize) % beatbyte_core::Lane::ALL.len();
    let color = theme.0.lane_color(beatbyte_core::Lane::ALL[lane]);
    spawn_burst(
        &mut commands,
        &mut live,
        soft,
        Vec2::new(layout.lane_x(0, beatbyte_core::Lane::ALL[lane]), RECEPTOR_Y),
        color,
        scaled_count(16, settings.intensity),
        360.0,
        due as usize * 31,
        // The outro salvo is confetti in every style.
        false,
    );
}

/// Ask for the white flash when a star-power phrase lands whole.
///
/// Its own system rather than another arm of `react_to_feedback`,
/// because the two read the bus for different reasons and this one
/// has to be ordered against the drain as well as against the driver.
fn flash_on_star_power(
    settings: Res<EffectSettings>,
    mut flash: ResMut<ScreenFlash>,
    mut feedback: MessageReader<SessionFeedback>,
) {
    let landed = feedback
        .read()
        .any(|message| matches!(message.event, SessionEvent::PhraseCompleted { .. }));
    if landed {
        flash.request(FlashProfile::star(
            settings.reduced_flashing,
            settings.intensity,
        ));
    }
}

/// Drive the one flash quad: age the effect and put its colour on the
/// sprite. Hidden the instant it is over, so nothing can be left
/// tinting the screen.
fn drive_screen_flash(
    time: Res<Time>,
    mut flash: ResMut<ScreenFlash>,
    mut quad: Query<(&mut Sprite, &mut Visibility), With<ScreenFlashQuad>>,
) {
    flash.advance(time.delta_secs());
    let alpha = flash.alpha();
    let colour = flash.color();
    for (mut sprite, mut visibility) in &mut quad {
        if alpha <= 0.0 {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        }
        *visibility = Visibility::Visible;
        sprite.color = colour.with_alpha(alpha);
    }
}

/// Leave no tint behind when gameplay is entered or left.
fn clear_screen_flash(
    mut flash: ResMut<ScreenFlash>,
    mut quad: Query<&mut Visibility, With<ScreenFlashQuad>>,
) {
    flash.clear();
    for mut visibility in &mut quad {
        *visibility = Visibility::Hidden;
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

#[cfg(test)]
mod tests {
    use super::{EffectSettings, throws_flat_sparks};

    #[test]
    fn the_flat_bursts_belong_to_the_flat_view() {
        let flat = EffectSettings {
            particles: true,
            stage_3d: false,
            ..EffectSettings::default()
        };
        assert!(throws_flat_sparks(&flat));
        assert!(
            !throws_flat_sparks(&EffectSettings {
                stage_3d: true,
                ..flat
            }),
            "the stage places them 31 % short on the outer lanes and \
             throws its own instead"
        );
        assert!(
            !throws_flat_sparks(&EffectSettings {
                particles: false,
                ..flat
            }),
            "particles off is off in either view"
        );
    }

    #[test]
    fn embers_rise_and_confetti_falls() {
        assert!(super::burst_gravity(true) < 0.0, "an ember is buoyant");
        assert!(super::burst_gravity(false) > 0.0, "confetti falls");
    }

    use super::{flash_alpha, scaled_count};

    #[test]
    fn intensity_scales_counts_from_identity_to_silence() {
        assert_eq!(scaled_count(18, 1.0), 18, "full intensity is identity");
        assert_eq!(scaled_count(18, 0.5), 9);
        assert_eq!(scaled_count(18, 0.0), 0, "zero intensity spawns nothing");
        // Out-of-range files clamp instead of multiplying upward.
        assert_eq!(scaled_count(18, 7.0), 18);
    }

    #[test]
    fn reduced_flashing_kills_the_full_screen_flash_entirely() {
        // The accessibility promise is NO flash, not a dimmer one.
        assert_eq!(flash_alpha(true, 1.0), 0.0);
        assert!(flash_alpha(false, 1.0) > 0.0);
        assert!(
            flash_alpha(false, 0.5) < flash_alpha(false, 1.0),
            "intensity dims the flash when it is allowed at all"
        );
    }

    use super::{FlashProfile, ScreenFlash, flash_curve};
    use bevy::prelude::Color;

    #[test]
    fn a_flash_rises_and_dies_and_is_gone() {
        // Zero attack: a combo break has no rise you could see.
        assert!(flash_curve(0.0, 0.0, 0.25) > 0.99);
        assert_eq!(flash_curve(0.25, 0.0, 0.25), 0.0);
        assert_eq!(flash_curve(-0.1, 0.0, 0.25), 0.0);
        assert_eq!(flash_curve(9.0, 0.0, 0.25), 0.0);
        // With an attack it rises through it and then falls.
        assert_eq!(flash_curve(0.0, 0.03, 0.26), 0.0);
        assert!(flash_curve(0.03, 0.03, 0.26) > 0.99);
        assert!(flash_curve(0.15, 0.03, 0.26) < 0.5);
        // A zero-length profile is a no-op, not a division.
        assert_eq!(flash_curve(0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn the_two_profiles_read_as_opposite_things() {
        let miss = FlashProfile::miss(false, 1.0);
        let star = FlashProfile::star(false, 1.0);
        assert_eq!(miss.color, crate::palette::MISS, "a miss is red");
        assert_eq!(star.color, Color::WHITE, "a landed phrase is white");
        assert!(
            star.peak > miss.peak,
            "the positive event is the bigger one — otherwise a combo \
             break would outshine an achievement"
        );
        assert!(star.life < miss.life * 1.2, "and it is not a longer wash");
        assert!(star.peak < 0.4, "but never enough to hide the notes");
    }

    #[test]
    fn reduced_flashing_removes_the_star_flash_too() {
        // Not dimmer. The promise this game already makes for full
        // screen flashes is none, and a new effect does not get to
        // soften an existing promise — the neck's glow carries the
        // moment in that mode instead.
        assert_eq!(FlashProfile::star(true, 1.0).peak, 0.0);
        assert_eq!(FlashProfile::miss(true, 1.0).peak, 0.0);
        assert!(FlashProfile::star(false, 0.5).peak < FlashProfile::star(false, 1.0).peak);
        assert_eq!(FlashProfile::star(false, 0.0).peak, 0.0);
    }

    #[test]
    fn the_brighter_event_wins_and_the_screen_never_jumps_down() {
        let mut flash = ScreenFlash::default();
        assert_eq!(flash.alpha(), 0.0);

        // A star flash at its brightest is not cut short by a miss.
        flash.request(FlashProfile::star(false, 1.0));
        flash.advance(0.03);
        let bright = flash.alpha();
        // Derived, not a literal: the peak is a tuned number, and a
        // test that pins it would go red every time somebody looks at
        // the effect. What has to hold is that the star flash really
        // IS the brighter of the two, or the rest of this proves
        // nothing.
        let miss_peak = FlashProfile::miss(false, 1.0).peak;
        assert!(bright > miss_peak, "star {bright} vs miss {miss_peak}");
        flash.request(FlashProfile::miss(false, 1.0));
        assert_eq!(
            flash.alpha(),
            bright,
            "a dimmer request may not replace a brighter running flash \
             — the overlay would visibly jump down"
        );
        assert_eq!(flash.color(), Color::WHITE);

        // …but once it has faded past the miss's own peak, it may.
        flash.advance(0.18);
        assert!(flash.alpha() < FlashProfile::miss(false, 1.0).peak);
        flash.request(FlashProfile::miss(false, 1.0));
        assert_eq!(flash.color(), crate::palette::MISS);
    }

    #[test]
    fn alpha_never_accumulates_however_much_lands_at_once() {
        let mut flash = ScreenFlash::default();
        let star = FlashProfile::star(false, 1.0);
        for _ in 0..50 {
            flash.request(star);
            flash.request(FlashProfile::miss(false, 1.0));
            flash.advance(0.001);
            assert!(
                flash.alpha() <= star.peak + 1e-6,
                "fifty requests may not add up to a white screen: {}",
                flash.alpha()
            );
        }
    }

    #[test]
    fn a_flash_forgets_itself_and_can_be_cleared() {
        let mut flash = ScreenFlash::default();
        flash.request(FlashProfile::star(false, 1.0));
        for _ in 0..40 {
            flash.advance(0.016);
        }
        assert_eq!(flash.alpha(), 0.0, "it ends on its own");
        flash.request(FlashProfile::star(false, 1.0));
        flash.advance(0.03);
        assert!(flash.alpha() > 0.0);
        flash.clear();
        assert_eq!(flash.alpha(), 0.0, "and a cleared screen is clear");
        // A profile that cannot be seen is never taken at all, so a
        // reduced-flashing request cannot park an invisible timer.
        flash.request(FlashProfile::star(true, 1.0));
        assert_eq!(flash.alpha(), 0.0);
    }
}
