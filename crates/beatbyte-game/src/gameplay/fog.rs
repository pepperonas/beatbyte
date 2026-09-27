//! The fog machines: two nozzles that let go a puff now and then.
//!
//! Asked for as "von Zeit zu Zeit soll auch Nebelmaschine etwas Nebel
//! sprühen". A hazer runs all night and gives the beams a body — that
//! is the static haze in [`super::stage3d`] and it is already there.
//! This is the other thing: a machine that fires, throws a slug of fog
//! out over the boards, and goes quiet again.
//!
//! # What it may not do
//!
//! **Never cover the neck.** The highway is a reading surface (see
//! [`super::stage3d::on_the_neck`]) and a cloud drifting over it would
//! be exactly the kind of decoration that costs a player notes. The
//! nozzles sit outboard of the deck's edge, upstage, and the puffs
//! drift further OUT — [`NOZZLE_X`] is checked against the neck's own
//! corridor by a test.
//!
//! # How it is built
//!
//! A fixed pool of billboard quads, additive and unlit, parked
//! invisible. A firing wakes a slice of them; each one rises, spreads,
//! drifts and fades on its own curve. No allocation per frame, no
//! material written per frame, and nothing at all when STAGE MOTION is
//! off — the same contract the crowd and the light show keep.
//!
//! A puff is a **noise cloud** ([`crate::surfaces::fog_sprite`], three
//! variants, each puff turned about its own axis), not a round dot: a
//! soft disc stays a ball however faint it is drawn. It fades by
//! swapping between a **ladder** of shared materials — one per side,
//! variant and alpha step — rather than by writing its own material
//! every frame; puffs on one step batch. And it wears the **wash** of
//! its side of the room ([`stage3d::wash_colour`]), mixed toward white:
//! stage fog is white until the light hits it.
//!
//! Under all of it, a **low fog** lies on the floor outboard of the PA:
//! a few large flat clouds that drift slowly, always there while the
//! stage moves, and gone when STAGE MOTION is off.
//!
//! The schedule is a **hash of the firing count**, like the strips':
//! deterministic, so two runs of the same song fog alike, and
//! irregular, so it never reads as a metronome.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::fx::hash01;
use super::stage3d::{self, STAGE_LAYER, Stage3d};
use super::{GameplayScreen, pa};
use crate::audio_sys::GameClock;
use crate::config::Settings;

/// How many puffs one machine can have in the air at once.
pub const PUFFS_PER_NOZZLE: usize = 26;

/// The two nozzles.
pub const NOZZLES: usize = 2;

/// Every quad the pool holds.
pub const POOL: usize = NOZZLES * PUFFS_PER_NOZZLE;

/// Where a nozzle stands, at the deck's outer edge.
///
/// Outboard on purpose: fog that starts over the boards ends up over
/// the neck, and the neck is a reading surface.
///
/// Measured from the DECK rather than from the PA. It hung off
/// `pa::STACK_X` at first, and when the stacks were asked to move in
/// the nozzles came with them — far enough that the const assert
/// below stopped holding, at compile time, which is exactly where a
/// rule like this should bite. Where the machines stand is a fact
/// about the stage, not about the speakers.
pub const NOZZLE_X: f32 = stage3d::DECK_WIDTH / 2.0 - 0.6;

/// How far upstage they sit — behind the PA, in front of the crowd.
pub const NOZZLE_Z: f32 = -12.0;

/// The nozzle's mouth, above the boards.
pub const NOZZLE_Y: f32 = pa::DECK_TOP + 0.35;

/// The shortest wait between two firings of one machine, seconds.
pub const QUIET_MIN_S: f32 = 26.0;

/// The spread on top of that.
///
/// With the minimum, a machine fires every 26–52 s and the two are
/// out of step, so the room gets fog roughly every quarter minute
/// without a rhythm anybody can count.
pub const QUIET_SPREAD_S: f32 = 26.0;

/// How long one puff lives.
///
/// Long: real fog does not end, it dissolves. At 7 s a puff was still
/// visibly a thing that appeared and went; over eleven it drifts far
/// enough to stop reading as an object at all.
pub const PUFF_LIFE_S: f32 = 11.0;

/// How long a firing keeps releasing, seconds.
///
/// A machine does not cough: it runs for a moment, and the fog that
/// left first is already spreading while the last of it is still
/// coming out. That overlap is most of what makes a bank read as one
/// body rather than as a row of puffs.
pub const BURST_S: f32 = 4.0;

/// How fast a puff rises.
pub const RISE_PER_S: f32 = 0.26;

/// How fast it drifts outward, away from the neck.
pub const DRIFT_PER_S: f32 = 0.62;

/// A puff's size when it leaves the nozzle.
pub const BORN_SIZE: f32 = 2.1;

/// How much it swells over its life.
pub const SWELL: f32 = 7.0;

/// The thickest a puff ever draws.
///
/// Thin, and then thinner: 0.16 read as a ball of smoke and 0.10 as a
/// cloud with an outline. A few dense round sprites are balls however
/// soft their edges; fog is many faint overlapping ones, and the
/// thinner each is the more the bank is made of their sum rather than
/// of any one of them.
pub const PEAK_ALPHA: f32 = 0.045;

/// The slowest and fastest a puff travels, as a share of the drift.
///
/// Real fog stretches as it goes, because no two parts of it move at
/// the same speed. Released at one speed the bank stays the lump it
/// left as; spread over this range it draws itself out into a bank.
pub const PACE: (f32, f32) = (0.55, 1.5);

/// How far a puff wanders across its own path, in world units.
///
/// Fog does not travel in a straight line. Each puff swings slowly
/// about the drift it is on, on its own period and phase, so the bank
/// curls instead of sliding. Small enough that it never overcomes the
/// outward drift — a puff that wandered back over the neck would be a
/// bug, and there is a test for it.
pub const WANDER: f32 = 0.55;

/// The slowest and fastest a puff wanders, in cycles per second.
pub const WANDER_HZ: (f32, f32) = (0.05, 0.13);

/// How much wider than tall a puff may be drawn.
///
/// Written when the sprite was a round disc, which spinning left
/// unchanged; the noise clouds that replaced it also turn (see
/// [`roll_of`]), and the stretch still lays each one down on its own
/// terms, so the bank does not read as a row of like shapes.
pub const STRETCH: f32 = 1.7;

/// How many sprite variants the puffs share.
pub const VARIANTS: usize = 3;

/// How many alpha steps the material ladder has, from nothing to
/// [`PEAK_ALPHA`]. At twenty a step is under 0.003 of additive light —
/// below what a fade can be seen to step through (sixteen was exactly
/// 0.003, and the test drew the line there).
pub const LEVELS: usize = 20;

/// The sprite's size in texels.
const SPRITE_SIZE: usize = 256;

/// How far the fog is tinted toward its side's wash: stage fog is
/// white, and the light gives it the colour.
pub const WASH_SHARE: f32 = 0.45;

/// How many flat clouds lie on the floor on each side.
pub const LOW_PER_SIDE: usize = 5;

/// How thick the low fog draws. Constant: it does not come and go,
/// it lies there and drifts.
pub const LOW_ALPHA: f32 = 0.05;

/// The band the low fog lies in, as distance from the centre line:
/// outboard of the PA stacks, and far outboard of the barriers.
pub const LOW_X: (f32, f32) = (8.5, 11.5);

/// How far upstage and downstage it reaches.
pub const LOW_Z: (f32, f32) = (-22.0, -3.0);

/// Its height above the boards.
pub const LOW_Y: f32 = pa::DECK_TOP + 0.18;

/// The largest a low cloud is drawn (width across, depth along).
pub const LOW_SIZE: Vec2 = Vec2::new(5.0, 6.5);

/// One quad in the pool.
#[derive(Component, Debug, Clone, Copy)]
pub struct Puff {
    /// Which machine owns it.
    pub nozzle: usize,
    /// Its place in that machine's slice, which sets its delay,
    /// its size and where it drifts.
    pub slot: usize,
    /// When it was released; `None` while it is parked.
    pub born_s: Option<f64>,
}

/// The machines' shared clock.
#[derive(Resource, Debug, Clone, Copy)]
pub struct FogSchedule {
    /// When each machine fires next, in song seconds.
    pub next_at: [f64; NOZZLES],
    /// How many times each has fired, which seeds the next wait.
    pub fired: [u32; NOZZLES],
}

impl Default for FogSchedule {
    fn default() -> Self {
        FogSchedule {
            // Not at zero: the first bars of a song belong to the
            // song, and a machine that fires on the count-in reads as
            // a fault rather than as a cue.
            next_at: [12.0, 30.0],
            fired: [0; NOZZLES],
        }
    }
}

/// How long a machine waits after its `count`-th firing. Pure —
/// tested.
#[must_use]
pub fn quiet_after(nozzle: usize, count: u32) -> f32 {
    QUIET_SPREAD_S.mul_add(
        hash01(nozzle * 7919 + count as usize * 4243 + 17),
        QUIET_MIN_S,
    )
}

/// A puff's age in seconds, or `None` if it has not been released or
/// has already died. Pure — tested.
#[must_use]
pub fn age_of(puff: &Puff, now: f64) -> Option<f32> {
    let born = puff.born_s?;
    let age = (now - born) as f32;
    (0.0..PUFF_LIFE_S).contains(&age).then_some(age)
}

/// How thick a puff draws at `age`.
///
/// It blooms fast and thins slowly — a slug of fog leaves the machine
/// dense and dissolves. Never a hard edge at either end: it fades in
/// over its first tenth and is already at zero when it dies, so a puff
/// is never seen to appear or to blink out. Pure — tested.
#[must_use]
pub fn thickness(age: f32) -> f32 {
    let t = (age / PUFF_LIFE_S).clamp(0.0, 1.0);
    // Slower in, slower out than the first cut: fog that arrives in a
    // tenth of its life pops, and fog that leaves on a straight line
    // switches off. Both ends are curves now.
    let bloom = (t / 0.22).clamp(0.0, 1.0);
    let fade = (1.0 - t).powf(2.4);
    PEAK_ALPHA * bloom * fade
}

/// Where a puff sits, relative to its nozzle, at `age`.
///
/// Out and up: the machines stand outboard of the deck and throw
/// their fog further out still, so nothing drifts over the neck.
/// Pure — tested.
#[must_use]
pub fn drift(side: f32, slot: usize, age: f32) -> Vec3 {
    let spread = hash01(slot * 6151 + 5) - 0.5;
    let lift = hash01(slot * 3163 + 11).mul_add(0.5, 0.75);
    // The wander: a slow swing about the path, its own rate and phase
    // per puff, so no two curl together. It grows with age — fog
    // leaves the nozzle in a jet and only loses its way once it has
    // spread — and it is bounded well under the outward drift.
    let pace = (PACE.1 - PACE.0).mul_add(hash01(slot * 7333 + 19), PACE.0);
    let rate = (WANDER_HZ.1 - WANDER_HZ.0).mul_add(hash01(slot * 5309 + 7), WANDER_HZ.0);
    let phase = hash01(slot * 9377 + 3) * core::f32::consts::TAU;
    let swing = (age / PUFF_LIFE_S).min(1.0);
    let wander =
        |turn: f32| WANDER * swing * (age * rate * core::f32::consts::TAU + phase + turn).sin();
    Vec3::new(
        side.signum() * (DRIFT_PER_S * pace * age + spread * 0.9) + wander(0.0),
        RISE_PER_S * lift * age + wander(2.1) * 0.35,
        (hash01(slot * 2777 + 23) - 0.5).mul_add(2.4, -0.35 * age)
            + wander(core::f32::consts::FRAC_PI_2),
    )
}

/// How big a puff draws at `age`.
#[must_use]
pub fn size_at(slot: usize, age: f32) -> f32 {
    let t = (age / PUFF_LIFE_S).clamp(0.0, 1.0);
    let own = hash01(slot * 8161 + 31).mul_add(0.6, 0.7);
    own * SWELL.mul_add(t, BORN_SIZE)
}

/// The scale a puff draws at: wider than tall, by its own amount.
/// Pure — tested.
#[must_use]
pub fn scale_at(slot: usize, age: f32) -> Vec3 {
    let size = size_at(slot, age);
    let wide = hash01(slot * 4877 + 41).mul_add(STRETCH - 0.9, 0.9);
    Vec3::new(size * wide, size, 1.0)
}

/// Which sprite variant a puff draws with.
#[must_use]
pub fn variant_of(slot: usize) -> usize {
    slot % VARIANTS
}

/// How far a puff is turned about its own axis, radians. A cloud has
/// no up, so a turn is a new cloud for free. Pure.
#[must_use]
pub fn roll_of(slot: usize) -> f32 {
    hash01(slot * 1307 + 53) * core::f32::consts::TAU
}

/// The ladder step nearest `alpha`: 0 is nothing, `LEVELS - 1` the
/// peak. Pure — tested.
#[must_use]
pub fn level_of(alpha: f32) -> usize {
    let share = (alpha / PEAK_ALPHA).clamp(0.0, 1.0);
    (share * (LEVELS - 1) as f32).round() as usize
}

/// The alpha a ladder step draws at.
#[must_use]
pub fn level_alpha(level: usize) -> f32 {
    PEAK_ALPHA * level.min(LEVELS - 1) as f32 / (LEVELS - 1) as f32
}

/// Where a ladder material sits in [`FogLook::ladder`].
#[must_use]
pub fn ladder_index(nozzle: usize, variant: usize, level: usize) -> usize {
    (nozzle * VARIANTS + variant) * LEVELS + level
}

/// The fog's colour on a side: white, tinted toward the wash that
/// lights that side. Pure — tested.
#[must_use]
pub fn fog_tint(wash: Color) -> Color {
    Color::WHITE.mix(&wash, WASH_SHARE)
}

/// Where low cloud `index` on `side` lies at song time `t`, its size
/// and its turn: a slow drift in both directions on its own period,
/// inside [`LOW_X`] × [`LOW_Z`]. Deterministic. Pure — tested.
#[must_use]
pub fn low_fog_at(side: f32, index: usize, t: f32) -> (Vec3, Vec2, f32) {
    let seed = index * 911 + usize::from(side > 0.0) * 37;
    let own = |k: usize| hash01(seed * 13 + k);
    let size = LOW_SIZE * own(1).mul_add(0.35, 0.65);
    // Evenly down the length, each offset a little.
    let slot = (index as f32 + own(2).mul_add(0.6, 0.2)) / LOW_PER_SIDE as f32;
    let z0 = (LOW_Z.1 - LOW_Z.0).mul_add(slot, LOW_Z.0);
    let rate = own(3).mul_add(0.03, 0.02) * core::f32::consts::TAU;
    let phase = own(4) * core::f32::consts::TAU;
    // The centre stays far enough out that the cloud's inner edge never
    // reaches past LOW_X.0.
    let reach_x = (LOW_X.1 - LOW_X.0 - size.x).max(0.0) * 0.5;
    let mid_x = LOW_X.0 + size.x * 0.5 + reach_x;
    let x = side.signum() * reach_x.mul_add((t * rate + phase).sin(), mid_x);
    let z = 1.2f32.mul_add((t * rate * 0.7 + phase * 1.3).cos(), z0);
    let turn = own(5).mul_add(0.8, -0.4) + 0.15 * (t * rate * 0.5).sin();
    (Vec3::new(x, LOW_Y, z), size, turn)
}

/// A low cloud.
#[derive(Component, Debug, Clone, Copy)]
pub struct LowFog {
    /// −1 left, +1 right.
    pub side: f32,
    /// Its place down the side.
    pub index: usize,
}

/// The fog's three sprites, made once for the whole run.
#[derive(Resource, Debug, Clone)]
pub struct FogSprites(pub Vec<Handle<Image>>);

/// The fog's materials for this song: the puff ladder and the low fog.
#[derive(Resource, Debug, Clone)]
pub struct FogLook {
    /// Indexed by [`ladder_index`].
    pub ladder: Vec<Handle<StandardMaterial>>,
}

/// When a puff in `slot` is released after its machine fires.
#[must_use]
pub fn release_delay(slot: usize) -> f32 {
    BURST_S * (slot as f32 / PUFFS_PER_NOZZLE as f32)
}

/// Everything the fog machines need.
pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogSchedule>()
            .add_systems(OnEnter(crate::AppState::Gameplay), spawn_fog)
            .add_systems(
                Update,
                (drive_fog, drive_low_fog).run_if(in_state(crate::AppState::Gameplay)),
            );
    }
}

/// Park the pool: every quad exists from the start, invisible. Build
/// the ladder once per song (its colours follow the theme's wash).
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn spawn_fog(
    mut commands: Commands,
    settings: Res<Settings>,
    mut schedule: ResMut<FogSchedule>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    sprites: Option<Res<FogSprites>>,
    theme: Res<crate::theme::ActiveTheme>,
) {
    if !stage3d::active(&settings) {
        return;
    }
    *schedule = FogSchedule::default();
    // The sprites cost a moment to draw; they are made once per run.
    let sprites = if let Some(sprites) = sprites {
        sprites.0.clone()
    } else {
        let made: Vec<Handle<Image>> = (0..VARIANTS)
            .map(|v| images.add(crate::surfaces::fog_sprite(SPRITE_SIZE, 3 + v * 8)))
            .collect();
        commands.insert_resource(FogSprites(made.clone()));
        made
    };
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let paint = |materials: &mut Assets<StandardMaterial>,
                 tint: Color,
                 alpha: f32,
                 sprite: &Handle<Image>| {
        materials.add(StandardMaterial {
            base_color: tint.with_alpha(alpha),
            base_color_texture: Some(sprite.clone()),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    let mut ladder = Vec::with_capacity(NOZZLES * VARIANTS * LEVELS);
    for nozzle in 0..NOZZLES {
        let side = if nozzle == 0 { -1.0 } else { 1.0 };
        let tint = fog_tint(stage3d::wash_colour(theme.0, side));
        for sprite in &sprites {
            for level in 0..LEVELS {
                ladder.push(paint(&mut materials, tint, level_alpha(level), sprite));
            }
        }
    }
    for nozzle in 0..NOZZLES {
        for slot in 0..PUFFS_PER_NOZZLE {
            commands.spawn((
                GameplayScreen,
                Stage3d,
                NotShadowCaster,
                Puff {
                    nozzle,
                    slot,
                    born_s: None,
                },
                Mesh3d(quad.clone()),
                MeshMaterial3d(ladder[ladder_index(nozzle, variant_of(slot), 0)].clone()),
                Transform::from_xyz(0.0, -50.0, 0.0),
                Visibility::Hidden,
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }
    // The low fog: one material per side, constant — it moves, it
    // does not fade.
    for side in [-1.0f32, 1.0] {
        let tint = fog_tint(stage3d::wash_colour(theme.0, side));
        for index in 0..LOW_PER_SIDE {
            let sprite = &sprites[index % VARIANTS];
            commands.spawn((
                GameplayScreen,
                Stage3d,
                NotShadowCaster,
                LowFog { side, index },
                Mesh3d(quad.clone()),
                MeshMaterial3d(paint(&mut materials, tint, LOW_ALPHA, sprite)),
                Transform::from_xyz(0.0, -50.0, 0.0),
                Visibility::Hidden,
                RenderLayers::layer(STAGE_LAYER),
            ));
        }
    }
    commands.insert_resource(FogLook { ladder });
}

/// Lay the low fog down and drift it; hide it when STAGE MOTION is
/// off. Transforms and visibility only.
pub fn drive_low_fog(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut clouds: Query<(&LowFog, &mut Transform, &mut Visibility)>,
) {
    let on = stage3d::active(&settings) && settings.backdrop_motion;
    let t = game_clock.song_time(&time).unwrap_or(0.0) as f32;
    for (cloud, mut transform, mut visibility) in &mut clouds {
        let wanted = if on {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !on {
            continue;
        }
        let (at, size, turn) = low_fog_at(cloud.side, cloud.index, t);
        // Flat on the floor: the quad's face turned up, then about Y.
        *transform = Transform::from_translation(at)
            .with_rotation(
                Quat::from_rotation_y(turn) * Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2),
            )
            .with_scale(size.extend(1.0));
    }
}

/// Fire the machines and carry every live puff.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
pub fn drive_fog(
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    time: Res<Time>,
    mut schedule: ResMut<FogSchedule>,
    mut puffs: Query<(
        &mut Puff,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    look: Option<Res<FogLook>>,
) {
    if !stage3d::active(&settings) || !settings.backdrop_motion {
        return;
    }
    let (Some(now), Some(look)) = (game_clock.song_time(&time), look) else {
        return;
    };
    // The song clock steps BACK at the count-in handover and again,
    // by a fraction, whenever it corrects itself against the device.
    // A schedule that treated either as a new song would fire on
    // every correction; one that treated a new song as a correction
    // would never fire again. The gap between firings is the measure.
    for nozzle in 0..NOZZLES {
        if now + f64::from(QUIET_MIN_S) < schedule.next_at[nozzle] {
            schedule.next_at[nozzle] = now + 4.0;
        }
        if now >= schedule.next_at[nozzle] {
            let count = schedule.fired[nozzle];
            schedule.fired[nozzle] = count.wrapping_add(1);
            schedule.next_at[nozzle] = now + f64::from(quiet_after(nozzle, count));
            for (mut puff, _, _, _) in &mut puffs {
                if puff.nozzle == nozzle {
                    puff.born_s = Some(now + f64::from(release_delay(puff.slot)));
                }
            }
        }
    }

    // The stage camera never moves, so the billboard turns toward a
    // constant rather than toward a queried transform — one fewer
    // query, and no chance of facing the menu camera by mistake.
    let eye = stage3d::CAMERA_POS;
    for (mut puff, mut transform, mut visibility, mut material) in &mut puffs {
        let age = age_of(&puff, now);
        // A puff on the lowest step draws nothing: hidden, not drawn
        // at zero.
        let level = age.map_or(0, |age| level_of(thickness(age)));
        let Some(age) = age.filter(|_| level > 0) else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            // A puff whose life ran out is parked, so the next firing
            // finds it free rather than skipping it.
            if puff
                .born_s
                .is_some_and(|born| (now - born) as f32 >= PUFF_LIFE_S)
            {
                puff.born_s = None;
            }
            continue;
        };
        let side = if puff.nozzle == 0 { -1.0 } else { 1.0 };
        let home = Vec3::new(side * NOZZLE_X, NOZZLE_Y, NOZZLE_Z);
        let at = home + drift(side, puff.slot, age);
        transform.translation = at;
        transform.scale = scale_at(puff.slot, age);
        // Billboarded, because a flat quad seen edge-on is a line.
        let away = (at - eye).with_y(0.0);
        if away.length_squared() > 1e-4 {
            transform.rotation = Quat::from_rotation_y(away.x.atan2(away.z))
                * Quat::from_rotation_z(roll_of(puff.slot));
        }
        if *visibility != Visibility::Visible {
            *visibility = Visibility::Visible;
        }
        // Fade by stepping down the ladder: a handle swap, never a
        // material write.
        let wanted = &look.ladder[ladder_index(puff.nozzle, variant_of(puff.slot), level)];
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nozzles_stand_clear_of_the_neck_and_blow_outward() {
        // The rule the whole module is built around: fog may never
        // drift over the highway.
        const { assert!(NOZZLE_X > stage3d::DECK_WIDTH / 2.0 - 1.0) } // clear of the boards
        for side in [-1.0f32, 1.0] {
            let early = drift(side, 3, 0.5);
            let late = drift(side, 3, PUFF_LIFE_S - 0.1);
            // Signed, in the nozzle's OWN direction. Comparing
            // magnitudes was not enough: a probe that sent the fog
            // inward still passed it, because a puff moving toward
            // the neck also grows |x| once it is past the nozzle.
            assert!(
                late.x * side > early.x * side,
                "the puff must travel outward, away from the neck: {} then {}",
                early.x,
                late.x
            );
            let home = side * NOZZLE_X;
            assert!(
                (home + late.x) * side > home * side,
                "a puff ended inboard of its own machine"
            );
            assert!(
                (home + late.x).abs() > stage3d::DECK_WIDTH / 2.0 - 1.0,
                "a puff drifted in over the boards"
            );
            assert!(late.y > early.y, "and rise");
        }
        // It stretches as it travels: no two puffs at the same speed,
        // or the bank stays the lump it left the nozzle as.
        let far: Vec<f32> = (0..PUFFS_PER_NOZZLE)
            .map(|slot| drift(1.0, slot, PUFF_LIFE_S - 0.1).x)
            .collect();
        let nearest = far.iter().copied().fold(f32::MAX, f32::min);
        let furthest = far.iter().copied().fold(0.0f32, f32::max);
        assert!(
            furthest - nearest > DRIFT_PER_S * PUFF_LIFE_S * 0.4,
            "the bank barely drew itself out: {nearest} to {furthest}"
        );
    }

    #[test]
    fn a_puff_wanders_instead_of_travelling_in_a_line() {
        // The "fließender" half of the ask, and the half no other
        // test covered: a probe that switched the wander off left
        // every one of them green.
        //
        // A straight path would have the puff's cross-track offset
        // grow monotonically. This one swings: sampled across its
        // life, the offset from the straight line reverses.
        let straight = |age: f32| DRIFT_PER_S * age;
        let offsets: Vec<f32> = (0..40)
            .map(|i| {
                let age = i as f32 * PUFF_LIFE_S / 40.0;
                drift(1.0, 5, age).x - straight(age)
            })
            .collect();
        let reversals = offsets
            .windows(3)
            .filter(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum())
            .count();
        assert!(
            reversals >= 1,
            "the path never turns: {reversals} reversals in {} samples",
            offsets.len()
        );
        // And no two puffs curl together, or the bank moves as a slab.
        let at = |slot: usize| drift(1.0, slot, PUFF_LIFE_S * 0.6);
        let a = at(2);
        let b = at(3);
        assert!(
            (a.x - b.x).abs() + (a.z - b.z).abs() > 0.2,
            "two puffs on the same path: {a:?} and {b:?}"
        );
    }

    #[test]
    fn a_puff_is_never_seen_to_appear_or_to_blink_out() {
        assert!(thickness(0.0).abs() < 1e-6, "it fades in");
        assert!(thickness(PUFF_LIFE_S).abs() < 1e-6, "and out");
        assert!(
            thickness(PUFF_LIFE_S - 0.01) < 0.01,
            "with nothing left at the end"
        );
        // Dense early, thinning late: a slug of fog, not a cloud that
        // grows.
        let peak = (0..70)
            .map(|i| thickness(i as f32 * 0.1))
            .fold(0.0f32, f32::max);
        assert!(peak <= PEAK_ALPHA + 1e-6, "never thicker than its cap");
        assert!(thickness(1.2) > thickness(5.0), "it thins as it ages");
        // Thin enough to be atmosphere: a puff that hides the stage
        // behind it is not fog.
        const { assert!(PEAK_ALPHA < 0.25) } // a veil, not a curtain
    }

    #[test]
    fn a_puff_grows_and_then_is_parked() {
        assert!(size_at(2, 0.0) < size_at(2, PUFF_LIFE_S), "fog spreads");
        // Wider than tall, and not all alike: round sprites of one
        // size read as balls however soft their edges.
        let shapes: Vec<Vec3> = (0..PUFFS_PER_NOZZLE).map(|s| scale_at(s, 2.0)).collect();
        for shape in &shapes {
            assert!(shape.x > shape.y * 0.85, "a puff lies down: {shape:?}");
        }
        let widest = shapes.iter().fold(0.0f32, |a, s| a.max(s.x / s.y));
        let narrowest = shapes.iter().fold(f32::MAX, |a, s| a.min(s.x / s.y));
        assert!(
            widest - narrowest > 0.3,
            "every puff the same shape: {narrowest}..{widest}"
        );
        let mut puff = Puff {
            nozzle: 0,
            slot: 0,
            born_s: Some(100.0),
        };
        assert!(age_of(&puff, 100.5).is_some(), "alive just after release");
        assert!(
            age_of(&puff, 99.5).is_none(),
            "a puff released in the future is not on stage yet"
        );
        assert!(
            age_of(&puff, 100.0 + f64::from(PUFF_LIFE_S) + 0.1).is_none(),
            "and it dies"
        );
        puff.born_s = None;
        assert!(
            age_of(&puff, 100.0).is_none(),
            "a parked puff draws nothing"
        );
    }

    #[test]
    fn the_machines_fire_now_and_then_rather_than_on_a_count() {
        // The ask was "von Zeit zu Zeit", which is neither a metronome
        // nor a one-off.
        let waits: Vec<f32> = (0..24).map(|c| quiet_after(0, c)).collect();
        for wait in &waits {
            assert!(
                (QUIET_MIN_S..=QUIET_MIN_S + QUIET_SPREAD_S).contains(wait),
                "a wait of {wait} s is outside the range the room was tuned for"
            );
        }
        let shortest = waits.iter().copied().fold(f32::MAX, f32::min);
        let longest = waits.iter().copied().fold(0.0f32, f32::max);
        assert!(
            longest - shortest > QUIET_SPREAD_S * 0.4,
            "the waits barely differ: {shortest} to {longest} reads as a count"
        );
        // The two machines are out of step, or the room gets one big
        // cloud instead of fog drifting from both sides.
        let together = (0..12).filter(|c| (quiet_after(0, *c) - quiet_after(1, *c)).abs() < 0.5);
        assert!(together.count() < 4, "the nozzles fire in lockstep");
        // Deterministic: the same song fogs the same way twice.
        assert!((quiet_after(1, 7) - quiet_after(1, 7)).abs() < f32::EPSILON);
    }

    #[test]
    fn a_puff_fades_down_a_ladder_without_a_visible_step() {
        assert_eq!(level_of(0.0), 0, "nothing is the bottom step");
        assert_eq!(level_of(PEAK_ALPHA), LEVELS - 1, "the peak is the top");
        assert_eq!(
            level_of(PEAK_ALPHA * 3.0),
            LEVELS - 1,
            "and nothing above it"
        );
        let mut last = 0;
        for i in 0..=400 {
            let alpha = PEAK_ALPHA * i as f32 / 400.0;
            let level = level_of(alpha);
            assert!(level >= last, "the ladder only climbs");
            last = level;
            // The step drawn is within half a step of the curve.
            let half = PEAK_ALPHA / (LEVELS - 1) as f32 * 0.5;
            assert!((level_alpha(level) - alpha).abs() <= half + 1e-6);
        }
        // A step is too small to be seen as one: under 0.003 of
        // additive light.
        assert!(level_alpha(1) < 0.003, "a step of {}", level_alpha(1));
        // Every side, variant and step has its own place.
        let mut seen = std::collections::HashSet::new();
        for nozzle in 0..NOZZLES {
            for variant in 0..VARIANTS {
                for level in 0..LEVELS {
                    let i = ladder_index(nozzle, variant, level);
                    assert!(i < NOZZLES * VARIANTS * LEVELS);
                    assert!(seen.insert(i), "two rungs on one handle");
                }
            }
        }
        // Puffs use every variant and are not all turned alike.
        let variants: std::collections::HashSet<usize> =
            (0..PUFFS_PER_NOZZLE).map(variant_of).collect();
        assert_eq!(variants.len(), VARIANTS);
        let rolls: Vec<f32> = (0..PUFFS_PER_NOZZLE).map(roll_of).collect();
        let spread = rolls.iter().copied().fold(f32::MIN, f32::max)
            - rolls.iter().copied().fold(f32::MAX, f32::min);
        assert!(spread > 3.0, "the puffs are all turned alike: {spread}");
    }

    #[test]
    fn the_fog_is_white_until_the_wash_colours_it() {
        let magenta = Color::srgb(1.0, 0.0, 0.8);
        let tint = fog_tint(magenta).to_srgba();
        // Lighter than the wash: fog is white stuff in coloured light.
        assert!(tint.green > 0.4, "the fog is a colour, not fog: {tint:?}");
        // And it carries the wash's hue.
        assert!(tint.red > tint.green && tint.blue > tint.green, "{tint:?}");
        let white = fog_tint(Color::WHITE).to_srgba();
        assert!((white.red - 1.0).abs() < 1e-4 && (white.blue - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_low_fog_lies_on_the_floor_outboard_and_drifts() {
        use crate::gameplay::crowd::BARRIER_X;
        for side in [-1.0f32, 1.0] {
            for index in 0..LOW_PER_SIDE {
                let mut xs = Vec::new();
                for step in 0..240 {
                    let t = step as f32 * 0.5;
                    let (at, size, turn) = low_fog_at(side, index, t);
                    // Its inner edge (turned at most ~0.55 rad, so
                    // the half-diagonal bounds it) never reaches past
                    // the PA, let alone the barriers or the neck.
                    let reach = size.length() * 0.5;
                    assert!(at.x * side > 0.0, "on its own side");
                    assert!(
                        at.x.abs() - reach > BARRIER_X + 1.0,
                        "side {side} cloud {index} at {t}: inner edge {}",
                        at.x.abs() - reach
                    );
                    assert!(turn.abs() < 0.6, "it lies along the stage, {turn}");
                    assert!((at.y - LOW_Y).abs() < 1e-6, "on the floor");
                    assert!(
                        (LOW_Z.0 - 2.0..LOW_Z.1 + 2.0).contains(&at.z),
                        "inside the length it was given: {}",
                        at.z
                    );
                    xs.push(at);
                }
                let moved = xs.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>();
                assert!(moved > 1.0, "cloud {index} lies still: {moved}");
                // The same song, the same fog.
                assert_eq!(low_fog_at(side, index, 7.3), low_fog_at(side, index, 7.3));
            }
        }
    }

    #[test]
    fn the_fog_never_writes_a_material_per_frame() {
        // Fading is a handle swap. Pinned on the source, comments
        // stripped: the systems that run every frame must not hold
        // the material store at all.
        let code: String = include_str!("fog.rs")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for system in ["pub fn drive_fog(", "pub fn drive_low_fog("] {
            let start = code.find(system).expect(system);
            let body = &code[start..];
            let signature = &body[..body.find(") {").expect("a signature")];
            assert!(
                !signature.contains("Assets<StandardMaterial>"),
                "{system} can write materials"
            );
        }
    }

    #[test]
    fn a_firing_lets_go_over_time_rather_than_all_at_once() {
        let mut delays: Vec<f32> = (0..PUFFS_PER_NOZZLE).map(release_delay).collect();
        assert!((delays[0]).abs() < 1e-6, "the first puff is immediate");
        let last = delays.pop().expect("a pool holds puffs");
        assert!(last > 0.5, "the burst has a length: {last}");
        assert!(last < BURST_S + 1e-6, "and ends when the burst does");
    }
}
