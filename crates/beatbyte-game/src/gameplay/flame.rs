//! The hit flame, round style: three layered bodies, a flicker, and
//! embers that rise.
//!
//! Three curved, tapered bodies share a fixed mesh. A short hot
//! core leaves the taller coloured mantle visible; independent lean
//! and height modulation move the tips while the feet stay planted.
//!
//! So, on the instrument neck, a hit lights **three nested bodies**
//! per fret — a narrow white-gold core, an orange-to-lane mantle, a
//! broad dark aura in the lane's colour — each with its own flicker
//! phase, so their additive overlap gives the vertical gradient and
//! the soft edge a single mesh cannot; **six embers** per fret from a
//! fixed pool, relaunched on a hit, rising with buoyancy and a sway
//! and cooling from yellow to red to nothing; and a small warm
//! **light** whose intensity follows the flame, so the board takes
//! its glow.
//!
//! A hit raises `life` to at most 1.15, followed by a linear decay
//! over about 380 ms. Height falls faster than girth, the core cools,
//! and the embers outlive the body. Sustains retain a low flame.
//!
//! Everything lives in pre-spawned entities and is driven by pure
//! functions of `life`, time and a seed: no allocation per frame,
//! no per-hit spawn. Rapid hits re-raise `life` rather than stack,
//! and relaunch the embers only when the last launch is old enough
//! to have cleared the fret. The whole thing follows the project's
//! motion settings: `reduced_flashing` removes flicker and embers,
//! `particles` off removes embers, `fx_intensity` scales height and
//! ember count.
//!
//! No two flames burn alike: every ignition draws a [`Character`] —
//! height, girth, flicker rate, a slow gutter, a resting lean, a hue
//! nuance, the core's whiteness, how fast it dies — from a stable
//! seed of (player, fret, strike count), so a fret's flame differs
//! from its neighbour's and from its own last burn, and holds that
//! shape for the burn's length instead of being re-rolled per frame.
//! The three bodies of one flame share the character; the neutral
//! character is exactly the flame before it existed.
//!
//! Every number is a constant here, so the effect can be tuned or
//! rolled back by value.

use beatbyte_core::Lane;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::stage3d::{FretHeat, GEM_RADIUS, STAGE_LAYER, Stage3d, lane_x, neck_spread};
use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession};
use crate::config::Settings;

// ── Tuning ────────────────────────────────────────────────────────

/// How far `life` overshoots on a strike: the ignition spike.
pub const IGNITE_OVERSHOOT: f32 = 1.15;
/// How fast `life` decays per second. 3.0 puts the body out in about
/// a third of a second — gone before the next note needs the room.
pub const DECAY_PER_S: f32 = 3.0;
/// The floor a held sustain keeps the flame at.
pub const SUSTAIN_FLOOR: f32 = 0.34;
/// Flicker: two incommensurable frequencies, so the motion never
/// visibly repeats. Fire flickers around 8–12 Hz.
const FLICKER_HZ: (f32, f32) = (9.3, 13.7);
/// Height modulation at full flicker, ± this fraction.
const FLICKER_HEIGHT: f32 = 0.18;
/// Lean modulation at full flicker, ± radians.
const FLICKER_LEAN: f32 = 0.14;
/// How far a flame's own height may stray from the nominal, ± this.
const VARY_HEIGHT: f32 = 0.15;
/// How far its girth may stray, ± this.
const VARY_GIRTH: f32 = 0.10;
/// Flicker rate multiplier range: `1 - this ..= 1 + this`. Fire on a
/// draught flickers faster; on still air slower.
const VARY_RATE: f32 = 0.18;
/// The slow gutter — a 2–3 Hz breathing under the flicker — at most
/// this fraction of height.
const GUTTER_MAX: f32 = 0.06;
/// The gutter's rate, Hz, before the character's own multiplier.
const GUTTER_HZ: f32 = 2.6;
/// A resting lean, ± radians: a flame that stands a little to one
/// side, like one in a draught.
const VARY_LEAN: f32 = 0.06;
/// How far the mantle's orange may run toward red or yellow.
const VARY_HUE: f32 = 1.0;
/// The core's whiteness floor: the whitest core mixes fully to white,
/// the dullest only this far.
const WHITE_FLOOR: f32 = 0.85;
/// Decay multiplier range: some flames snuff, some linger.
const VARY_DECAY: f32 = 0.10;
/// Below this `life` the mantle reddens toward ember red: a dying
/// flame goes dark red before it goes out.
const REDDEN_BELOW: f32 = 0.35;
/// Peak body height in world units at `life` 1 and intensity 1.
/// 1.55 was the first value; with the core at 0.42 girth that gave a
/// 1:8 spike at the tip — the laser the whole exercise set out to
/// remove, measured on the first capture. A flame's aspect is nearer
/// 1:3.
const HEIGHT: f32 = 1.3;
/// Embers per fret in the pool.
pub const EMBERS: usize = 6;
/// Minimum seconds between two ember launches on one fret: a burst
/// of hits re-raises the flame but does not fire a new shower before
/// the last has cleared the fret.
pub const EMBER_RELAUNCH_GAP: f32 = 0.08;
/// Ember lifetime range, seconds.
const EMBER_TTL: (f32, f32) = (0.35, 0.7);
/// Whether each fret's flame casts a light on the board. The one
/// knob with real render cost (5 lights solo, 20 at four players).
pub const CAST_LIGHT: bool = true;
/// The light's peak intensity.
const LIGHT_INTENSITY: f32 = 60_000.0;

// ── Components ────────────────────────────────────────────────────

/// Which body of the flame an entity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Narrow, white-gold, hottest: the inside of the flame.
    Core,
    /// The orange body that cools into the lane's colour.
    Mantle,
    /// Broad, dark, the lane's colour: the glow around the flame.
    Aura,
}

/// What sets one flame apart from the next.
///
/// Drawn ONCE per ignition from a stable seed and held for the burn;
/// never re-rolled per frame — that would twitch, not live. Every
/// field is a multiplier or offset around the neutral flame, bounded
/// by the `VARY_*` constants. Pure — tested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Character {
    /// Height multiplier.
    pub height: f32,
    /// Girth multiplier.
    pub girth: f32,
    /// Flicker (and gutter) rate multiplier.
    pub rate: f32,
    /// Slow-breathing amplitude, fraction of height.
    pub gutter: f32,
    /// Resting lean, radians.
    pub lean: f32,
    /// Mantle hue nuance: −1 red … 0 orange … +1 yellow.
    pub hue: f32,
    /// How white the core gets at the strike, `WHITE_FLOOR..=1`.
    pub white: f32,
    /// Decay multiplier.
    pub decay: f32,
}

impl Character {
    /// The flame as it was before characters: every multiplier one,
    /// every offset zero.
    pub const NEUTRAL: Character = Character {
        height: 1.0,
        girth: 1.0,
        rate: 1.0,
        gutter: 0.0,
        lean: 0.0,
        hue: 0.0,
        white: 1.0,
        decay: 1.0,
    };

    /// The character of a fret's `strike`-th burn. Deterministic:
    /// the same fret and count draws the same flame, so a run can be
    /// reconstructed; different frets and different strikes draw
    /// different ones. Pure — tested.
    #[must_use]
    pub fn draw(player: usize, lane: Lane, strike: u32) -> Character {
        let base = player * 104_729 + lane.index() * 7_919 + strike as usize * 131;
        let unit = |i: usize| super::fx::hash01(base + i * 1_000_003);
        let signed = |i: usize| unit(i) * 2.0 - 1.0;
        Character {
            height: 1.0 + VARY_HEIGHT * signed(1),
            girth: 1.0 + VARY_GIRTH * signed(2),
            rate: 1.0 + VARY_RATE * signed(3),
            gutter: GUTTER_MAX * unit(4),
            lean: VARY_LEAN * signed(5),
            hue: VARY_HUE * signed(6),
            white: WHITE_FLOOR + (1.0 - WHITE_FLOOR) * unit(7),
            decay: 1.0 + VARY_DECAY * signed(8),
        }
    }
}

/// One body of a fret's flame.
#[derive(Component)]
pub struct FlameBody {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
    /// Which body.
    pub layer: Layer,
    /// Flicker phase, so the three bodies do not move as one.
    pub phase: f32,
}

/// The fret's flame state, carried on its core body.
#[derive(Component)]
pub struct FlameState {
    /// 1 (or a little over) at the strike, decaying to 0.
    pub life: f32,
    /// The hit strength seen last frame, to detect a rise.
    pub last_hit: f32,
    /// When the embers were last launched, in stage seconds.
    pub last_launch: f32,
    /// How many strikes this fret has taken: the seed of its burns.
    pub strikes: u32,
    /// This burn's character.
    pub character: Character,
}

/// One ember of a fret's pool.
#[derive(Component)]
pub struct Ember {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
    /// Position in the pool: seeds the ember's own path.
    pub index: usize,
    /// Seconds since launch; past `ttl` the ember is hidden.
    pub age: f32,
    /// This flight's lifetime.
    pub ttl: f32,
    /// Whether it is in flight.
    pub live: bool,
}

/// The warm light a fret's flame casts.
#[derive(Component)]
pub struct FlameLight {
    /// Owning player.
    pub player: usize,
    /// Which fret.
    pub lane: Lane,
}

// ── Pure functions ────────────────────────────────────────────────

/// Advance a fret's `life` by one frame.
///
/// A strike — a hit strength rising above what was seen last frame
/// — re-ignites the flame to the strength with the ignition
/// overshoot, but never lowers it: rapid hits re-raise, they do not
/// stack and they do not cut each other off. A held sustain keeps a
/// low flame alive. Otherwise it decays. Pure — tested.
#[must_use]
pub fn advance_life(life: f32, last_hit: f32, hit: f32, held: bool, delta: f32) -> f32 {
    advance_life_with(life, last_hit, hit, held, delta, &Character::NEUTRAL)
}

/// Whether a hit strength rising above last frame's is a strike — the
/// one rule ignition and the character draw share. Pure — tested.
#[must_use]
pub fn is_strike(last_hit: f32, hit: f32) -> bool {
    hit > last_hit + 1e-4
}

/// [`advance_life`] for a flame with a character: its decay is its
/// own, some snuff and some linger.
#[must_use]
pub fn advance_life_with(
    life: f32,
    last_hit: f32,
    hit: f32,
    held: bool,
    delta: f32,
    character: &Character,
) -> f32 {
    let mut next = life;
    if is_strike(last_hit, hit) {
        next = next.max(hit * IGNITE_OVERSHOOT);
    }
    next = (next - DECAY_PER_S * character.decay * delta).max(0.0);
    // The floor is applied AFTER the decay: a held fret burns at
    // least this brightly every frame, whatever the frame length.
    // (The first version applied it before, and its own test — a
    // one-second frame — decayed the floor away in the same step.)
    if held {
        next = next.max(SUSTAIN_FLOOR);
    }
    next
}

/// The flicker at `seconds`: `(height factor, lean radians)`.
///
/// Two sines at incommensurable rates, so the tip never settles into
/// a visible loop. `strength` 0 is a still flame (reduced motion).
/// Pure — tested.
#[must_use]
pub fn flicker(seconds: f32, phase: f32, strength: f32) -> (f32, f32) {
    flicker_with(seconds, phase, strength, &Character::NEUTRAL)
}

/// [`flicker`] for a flame with a character: its own rate, a slow
/// gutter under the flicker, and a resting lean that stays even when
/// the motion is stilled — a posture, not a movement.
#[must_use]
pub fn flicker_with(seconds: f32, phase: f32, strength: f32, character: &Character) -> (f32, f32) {
    let rate = character.rate;
    let a = (seconds * FLICKER_HZ.0 * rate + phase).sin();
    let b = (seconds * FLICKER_HZ.1 * rate + phase * 1.7).sin();
    let gutter = (seconds * GUTTER_HZ * rate + phase * 0.4).sin();
    let height =
        1.0 + strength * (FLICKER_HEIGHT * (0.65 * a + 0.35 * b) + character.gutter * gutter);
    let lean = character.lean + strength * FLICKER_LEAN * (0.5 * a - 0.5 * b);
    (height, lean)
}

/// How much a body narrows as its height stretches: a flame that
/// licks up gets thinner, one that sags gets fatter — girth follows
/// height inversely, half-strength, so the volume reads as conserved.
/// Neutral at a height factor of one. Pure — tested.
#[must_use]
pub fn breathe(height_factor: f32) -> f32 {
    (1.0 / height_factor.max(0.5)).sqrt().clamp(0.85, 1.15)
}

/// The shape of one body at `life`: `(girth, height)` in world
/// units before flicker.
///
/// Height falls faster than girth as the flame dies (height ∝ life,
/// girth ∝ √life): a dying flame gets short before it gets thin.
/// The core is short and hot, the mantle carries the coloured tip,
/// and the aura is broad and low — the
/// nesting is what gives the gradient. Pure — tested.
#[must_use]
pub fn body_shape(layer: Layer, life: f32, intensity: f32) -> (f32, f32) {
    body_shape_with(layer, life, intensity, &Character::NEUTRAL)
}

/// [`body_shape`] for a flame with a character: taller or shorter,
/// fatter or thinner than the nominal, the same for its three bodies
/// so they keep nesting.
#[must_use]
pub fn body_shape_with(
    layer: Layer,
    life: f32,
    intensity: f32,
    character: &Character,
) -> (f32, f32) {
    let (girth, height) = body_shape_neutral(layer, life, intensity);
    (girth * character.girth, height * character.height)
}

fn body_shape_neutral(layer: Layer, life: f32, intensity: f32) -> (f32, f32) {
    let life = life.clamp(0.0, IGNITE_OVERSHOOT);
    let (girth_k, height_k) = match layer {
        Layer::Core => (0.55, 0.52),
        Layer::Mantle => (0.9, 1.0),
        // The aura at 1.3 pooled across two lanes at the base; a
        // flame's glow is wider than its body, not wider than its
        // neighbour.
        Layer::Aura => (1.05, 0.72),
    };
    let scale = 0.6 + 0.4 * intensity.clamp(0.0, 1.0);
    let height = HEIGHT * height_k * life * scale;
    let girth = girth_k * life.sqrt() * scale;
    (girth, height)
}

/// The colour of one body at `life`: `(base, emissive strength)`.
///
/// Physically the base is hottest and whitest, the tip coolest; here
/// the nesting does the vertical work and `life` does the temporal:
/// the core is near-white at the strike and cools toward gold, the
/// mantle runs orange into the lane's colour, the aura is the lane's
/// colour darkened. Pure — tested.
#[must_use]
pub fn body_color(layer: Layer, life: f32, lane: Color) -> (Color, f32) {
    body_color_with(layer, life, lane, &Character::NEUTRAL)
}

/// [`body_color`] for a flame with a character, plus two things every
/// flame does: the strike itself flashes the core whiter than a
/// steady burn (the overshoot above `life` 1 is the flash), and a
/// dying mantle reddens toward ember red before it goes dark — the
/// lane's colour is what the flame burns in, red is what it burns
/// down to. The aura is untouched: it is the lane's glow, not fire.
#[must_use]
pub fn body_color_with(
    layer: Layer,
    life: f32,
    lane: Color,
    character: &Character,
) -> (Color, f32) {
    let flash = ((life - 1.0) / (IGNITE_OVERSHOOT - 1.0)).clamp(0.0, 1.0);
    let life = life.clamp(0.0, 1.0);
    let heat = life * life;
    match layer {
        Layer::Core => {
            let gold = Color::srgb(1.0, 0.86, 0.55);
            let white = Color::srgb(1.0, 0.97, 0.9);
            let whiteness = ((0.4 + 0.6 * heat) * character.white + 0.3 * flash).min(1.0);
            (gold.mix(&white, whiteness), 3.0 + 3.0 * heat + 2.0 * flash)
        }
        Layer::Mantle => {
            // The nuance: this flame's orange leans red or yellow.
            let red_orange = Color::srgb(1.0, 0.42, 0.12);
            let orange = Color::srgb(1.0, 0.52, 0.16);
            let yellow_orange = Color::srgb(1.0, 0.64, 0.2);
            let tint = if character.hue < 0.0 {
                orange.mix(&red_orange, -character.hue)
            } else {
                orange.mix(&yellow_orange, character.hue)
            };
            let mut colour = lane.mix(&tint, 0.35 + 0.45 * heat);
            if life < REDDEN_BELOW {
                let ember = Color::srgb(0.75, 0.12, 0.04);
                colour = colour.mix(&ember, 0.5 * (REDDEN_BELOW - life) / REDDEN_BELOW);
            }
            (colour, 3.0 + 3.0 * heat)
        }
        Layer::Aura => (lane.mix(&Color::BLACK, 0.35), 1.2 + 1.4 * heat),
    }
}

/// Where an ember is at `age` into a flight of `ttl` seconds:
/// `(dx, dy, alpha)` in world units from its launch point.
///
/// Buoyancy: it accelerates upward as it rises, and sways sideways
/// on its own seeded rhythm. Alpha holds, then falls away in the
/// last third — an ember fades, it does not blink out. Pure —
/// tested.
#[must_use]
pub fn ember_flight(age: f32, ttl: f32, seed: f32) -> (f32, f32, f32) {
    if ttl <= 0.0 || age < 0.0 || age >= ttl {
        return (0.0, 0.0, 0.0);
    }
    let t = age / ttl;
    // Upward, accelerating: y = v0·t + ½a·t², with a little of each
    // ember's own spread.
    let rise = (0.9 + 0.5 * seed) * age + 1.6 * age * age;
    let sway = (age * (5.0 + 3.0 * seed) + seed * 6.3).sin() * (0.05 + 0.08 * seed) * t.sqrt();
    let drift = (seed - 0.5) * 0.5 * age;
    let alpha = if t < 0.66 {
        1.0
    } else {
        1.0 - (t - 0.66) / 0.34
    };
    (sway + drift, rise, alpha)
}

/// An ember's colour as it cools: yellow-white when fresh, orange,
/// red, then dark. Pure — tested.
#[must_use]
pub fn ember_color(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let yellow = Color::srgb(1.0, 0.9, 0.55);
    let orange = Color::srgb(1.0, 0.5, 0.12);
    let red = Color::srgb(0.75, 0.12, 0.04);
    if t < 0.35 {
        yellow.mix(&orange, t / 0.35)
    } else {
        orange.mix(&red, (t - 0.35) / 0.65)
    }
}

/// Whether a strike at `now` may launch a new ember shower, given
/// when the last one went. Pure — tested.
#[must_use]
pub fn relaunch_allowed(now: f32, last_launch: f32) -> bool {
    now - last_launch >= EMBER_RELAUNCH_GAP
}

/// How many embers a launch uses under the intensity slider.
#[must_use]
pub fn ember_count(intensity: f32) -> usize {
    ((EMBERS as f32) * intensity.clamp(0.0, 1.0)).round() as usize
}

/// Bend and round a unit-height cone. The
/// base remains fixed; the narrowing tip curls away from its axis.
fn flame_vertex([x, y, z]: [f32; 3]) -> [f32; 3] {
    let t = (y + 0.5).clamp(0.0, 1.0);
    let shoulder = 1.0 + 0.45 * (core::f32::consts::PI * t).sin();
    [x * shoulder + 0.09 * t * t, y, z * shoulder]
}

/// A shared, low-resolution curved flame (192 triangles). Built
/// once on entering gameplay; no vertex updates during animation.
fn flame_mesh(radius: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut positions = Vec::with_capacity(117);
    let mut indices = Vec::with_capacity(576);
    for ring in 0..=8 {
        let t = ring as f32 / 8.0;
        for side in 0..=12 {
            let angle = core::f32::consts::TAU * side as f32 / 12.0;
            let r = radius * (1.0 - t);
            let mut p = flame_vertex([r * angle.cos(), t - 0.5, r * angle.sin()]);
            // Keep the curl proportional to lane size in multiplayer.
            p[0] += 0.09 * t * t * (radius / 0.25 - 1.0);
            positions.push(p);
            if ring < 8 && side < 12 {
                let a = ring * 13 + side;
                indices.extend_from_slice(&[a, a + 13, a + 1, a + 1, a + 13, a + 14]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![[0.0, 1.0, 0.0]; positions.len()],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(indices))
}

/// Deterministic seed in 0..1 for an ember of a fret.
fn ember_seed(player: usize, lane: Lane, index: usize, launch: u32) -> f32 {
    super::fx::hash01(player * 977 + lane.index() * 131 + index * 17 + launch as usize * 7919)
}

// ── Systems ───────────────────────────────────────────────────────

/// Spawn the layered flame, the ember pool and the light for every
/// fret. Instrument neck only.
pub fn spawn_flames(
    mut commands: Commands,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !super::stage3d::active(&settings) {
        return;
    }
    let layer_mask = RenderLayers::layer(STAGE_LAYER);
    let radius = GEM_RADIUS * 1.05 * neck_spread(&layout);
    // One cone with a rounded foot per body: the sphere at the base is
    // what turns a spike into a teardrop.
    let cone = meshes.add(flame_mesh(radius));
    let foot = meshes.add(Sphere::new(radius).mesh().uv(12, 8));
    // Small and round: at 0.035 with six segments the embers were
    // orange hexagons the size of a gem's centre (seen on the user's
    // own capture); an ember is a point of light.
    let ember_mesh = meshes.add(Sphere::new(0.016 * neck_spread(&layout)).mesh().uv(10, 6));

    for index in &players {
        let player = index.0;
        for lane in Lane::ALL {
            let x = lane_x(&layout, player, lane);
            let colour = theme.0.lane_color(lane);
            for (order, layer) in [Layer::Aura, Layer::Mantle, Layer::Core]
                .into_iter()
                .enumerate()
            {
                let (base, glow) = body_color(layer, 0.0, colour);
                let material = materials.add(StandardMaterial {
                    base_color: base.with_alpha(0.0),
                    emissive: base.to_linear() * glow,
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });
                let phase = order as f32 * 2.1 + lane.index() as f32 * 0.7 + player as f32 * 1.3;
                let mut body = commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    FlameBody {
                        player,
                        lane,
                        layer,
                        phase,
                    },
                    super::stage3d::on_the_neck(),
                    Mesh3d(cone.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(x, 0.5, 0.0).with_scale(Vec3::splat(0.001)),
                    Visibility::default(),
                    layer_mask.clone(),
                ));
                if layer == Layer::Core {
                    body.insert(FlameState {
                        life: 0.0,
                        last_hit: 0.0,
                        last_launch: -1.0,
                        strikes: 0,
                        character: Character::draw(player, lane, 0),
                    });
                }
                // The foot, as a child: it scales with the body.
                body.with_child((
                    super::stage3d::on_the_neck(),
                    Mesh3d(foot.clone()),
                    MeshMaterial3d(material),
                    // In the body's local space the cone spans y −0.5…0.5
                    // before scale; the foot sits at the base.
                    Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(1.0, 0.55, 1.0)),
                    layer_mask.clone(),
                ));
            }
            for index in 0..EMBERS {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    Ember {
                        player,
                        lane,
                        index,
                        age: 0.0,
                        ttl: 0.5,
                        live: false,
                    },
                    super::stage3d::on_the_neck(),
                    Mesh3d(ember_mesh.clone()),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: ember_color(0.0).with_alpha(0.0),
                        emissive: ember_color(0.0).to_linear() * 6.0,
                        alpha_mode: AlphaMode::Add,
                        unlit: true,
                        ..default()
                    })),
                    Transform::from_xyz(x, 0.1, 0.0),
                    Visibility::Hidden,
                    layer_mask.clone(),
                ));
            }
            if CAST_LIGHT {
                commands.spawn((
                    GameplayScreen,
                    Stage3d,
                    FlameLight { player, lane },
                    PointLight {
                        color: Color::srgb(1.0, 0.72, 0.4),
                        intensity: 0.0,
                        range: 1.6,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(x, 0.35, 0.0),
                    layer_mask.clone(),
                ));
            }
        }
    }
}

/// Drive the bodies and the light from the fret heat.
#[allow(clippy::type_complexity, clippy::too_many_arguments)] // Bevy system: params are DI
pub fn drive_flames(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    theme: Res<crate::theme::ActiveTheme>,
    heat: Res<FretHeat>,
    mut states: Query<(&FlameBody, &mut FlameState)>,
    mut bodies: Query<
        (
            &FlameBody,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<FlameLight>,
    >,
    mut lights: Query<(&FlameLight, &mut PointLight)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let delta = time.delta_secs();
    let now = time.elapsed_secs();
    let still = settings.reduced_flashing;
    let intensity = settings.fx_intensity;

    // Advance every fret's life from this frame's heat.
    for (body, mut state) in &mut states {
        let (hit, held) = heat
            .0
            .iter()
            .find(|e| e.player == body.player && e.lane == body.lane)
            .map_or((0.0, false), |e| (e.hit, e.held));
        // A strike draws this burn's character BEFORE the life moves,
        // so the ignition frame already wears it.
        if is_strike(state.last_hit, hit) {
            state.strikes = state.strikes.wrapping_add(1);
            state.character = Character::draw(body.player, body.lane, state.strikes);
        }
        let character = state.character;
        state.life = advance_life_with(state.life, state.last_hit, hit, held, delta, &character);
        state.last_hit = hit;
    }

    for (body, mut transform, material) in &mut bodies {
        let Some((_, state)) = states
            .iter()
            .find(|(b, _)| b.player == body.player && b.lane == body.lane)
        else {
            continue;
        };
        let life = state.life;
        let character = state.character;
        let (girth, height) = body_shape_with(body.layer, life, intensity, &character);
        let (flick_h, lean) =
            flicker_with(now, body.phase, if still { 0.0 } else { 1.0 }, &character);
        let height = height * flick_h;
        let girth = girth * breathe(flick_h);
        transform.scale = Vec3::new(girth.max(0.001), height.max(0.001), girth.max(0.001));
        transform.rotation = Quat::from_rotation_z(lean * life);
        // Rotate around the receptor, not the cone's centre: the
        // foot stays planted while the tip sways.
        transform.translation = Vec3::new(lane_x(&layout, body.player, body.lane), 0.03, 0.0)
            + transform.rotation * Vec3::Y * (height * 0.5);
        if let Some(mut paint) = materials.get_mut(&material.0) {
            let (base, glow) =
                body_color_with(body.layer, life, theme.0.lane_color(body.lane), &character);
            let alpha = match body.layer {
                Layer::Core => 0.9,
                Layer::Mantle => 0.7,
                Layer::Aura => 0.45,
            } * life.min(1.0);
            paint.base_color = base.with_alpha(alpha);
            paint.emissive = base.to_linear() * (glow * life.min(1.0));
        }
    }

    for (light, mut point) in &mut lights {
        let (life, character, phase) = states
            .iter()
            .find(|(b, _)| b.player == light.player && b.lane == light.lane)
            .map_or((0.0, Character::NEUTRAL, 0.0), |(b, s)| {
                (s.life, s.character, b.phase)
            });
        // The light breathes with ITS flame, not with every fret's at
        // once: the same phase and rate as the body it lights.
        let (flick_h, _) = flicker_with(now, phase, if still { 0.0 } else { 0.6 }, &character);
        point.intensity = LIGHT_INTENSITY
            * life.min(1.0)
            * flick_h
            * intensity.clamp(0.0, 1.0)
            * if still { 0.35 } else { 1.0 };
    }
}

/// Launch and fly the embers.
pub fn drive_embers(
    time: Res<Time>,
    settings: Res<Settings>,
    layout: Res<HighwayLayout>,
    mut states: Query<(&FlameBody, &mut FlameState)>,
    mut embers: Query<(
        &mut Ember,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let delta = time.delta_secs();
    let now = time.elapsed_secs();
    let allowed = settings.particles && !settings.reduced_flashing;
    let count = ember_count(settings.fx_intensity);

    // Launch: a fret whose life just went up past the relaunch gap
    // fires its pool (as many as the intensity allows).
    for (body, mut state) in &mut states {
        let launch = allowed && state.life > 0.95 && relaunch_allowed(now, state.last_launch);
        if !launch {
            continue;
        }
        state.last_launch = now;
        let launch_id = (now * 1000.0) as u32;
        for (mut ember, mut transform, mut visibility, _) in &mut embers {
            if ember.player != body.player || ember.lane != body.lane {
                continue;
            }
            if ember.index >= count {
                continue;
            }
            let seed = ember_seed(ember.player, ember.lane, ember.index, launch_id);
            ember.age = -0.03 * ember.index as f32; // staggered by a frame or two
            ember.ttl = EMBER_TTL.0 + (EMBER_TTL.1 - EMBER_TTL.0) * seed;
            ember.live = true;
            transform.translation.x = lane_x(&layout, ember.player, ember.lane);
            *visibility = Visibility::Inherited;
        }
    }

    for (mut ember, mut transform, mut visibility, material) in &mut embers {
        if !allowed || ember.index >= count {
            ember.live = false;
            *visibility = Visibility::Hidden;
            continue;
        }
        if !ember.live {
            continue;
        }
        ember.age += delta;
        if ember.age < 0.0 {
            *visibility = Visibility::Hidden;
            continue;
        }
        if ember.age >= ember.ttl {
            ember.live = false;
            *visibility = Visibility::Hidden;
            continue;
        }
        let seed = ember_seed(ember.player, ember.lane, ember.index, 0);
        *visibility = Visibility::Inherited;
        let (dx, dy, alpha) = ember_flight(ember.age, ember.ttl, seed);
        let x = lane_x(&layout, ember.player, ember.lane);
        transform.translation = Vec3::new(x + dx, 0.55 + dy, 0.0);
        let t = (ember.age / ember.ttl).clamp(0.0, 1.0);
        let shrink = 1.0 - 0.6 * t;
        transform.scale = Vec3::splat(shrink);
        if let Some(mut paint) = materials.get_mut(&material.0) {
            let colour = ember_color(t);
            paint.base_color = colour.with_alpha(alpha);
            paint.emissive = colour.to_linear() * (6.0 * alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma(c: Color) -> f32 {
        let l = c.to_linear();
        0.2126 * l.red + 0.7152 * l.green + 0.0722 * l.blue
    }

    #[test]
    fn no_two_flames_burn_alike_and_the_same_flame_burns_the_same() {
        // Neighbouring frets, the same strike: different characters.
        let a = Character::draw(0, Lane::One, 1);
        let b = Character::draw(0, Lane::Two, 1);
        assert_ne!(a, b, "two frets drew the same flame");
        // The same fret, the next strike: a different flame again.
        let c = Character::draw(0, Lane::One, 2);
        assert_ne!(a, c, "a fret's second burn repeated its first");
        // Deterministic: a run can be reconstructed.
        assert_eq!(a, Character::draw(0, Lane::One, 1));
        // And the difference is not one field by a hair: across the
        // whole neck, every field spreads.
        let draws: Vec<Character> = (0..40)
            .flat_map(|strike| Lane::ALL.map(|lane| Character::draw(0, lane, strike)))
            .collect();
        let spread = |f: fn(&Character) -> f32| {
            let (lo, hi) = draws
                .iter()
                .map(f)
                .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            hi - lo
        };
        assert!(spread(|c| c.height) > VARY_HEIGHT, "heights barely differ");
        assert!(spread(|c| c.girth) > VARY_GIRTH, "girths barely differ");
        assert!(spread(|c| c.rate) > VARY_RATE, "rates barely differ");
        assert!(spread(|c| c.hue) > VARY_HUE, "hues barely differ");
        assert!(spread(|c| c.decay) > VARY_DECAY, "decays barely differ");
    }

    #[test]
    fn a_character_stays_inside_its_bounds() {
        for strike in 0..200 {
            for lane in Lane::ALL {
                let c = Character::draw(1, lane, strike);
                assert!((c.height - 1.0).abs() <= VARY_HEIGHT + 1e-6);
                assert!((c.girth - 1.0).abs() <= VARY_GIRTH + 1e-6);
                assert!((c.rate - 1.0).abs() <= VARY_RATE + 1e-6);
                assert!(c.gutter >= 0.0 && c.gutter <= GUTTER_MAX + 1e-6);
                assert!(c.lean.abs() <= VARY_LEAN + 1e-6);
                assert!(c.hue.abs() <= VARY_HUE + 1e-6);
                assert!(c.white >= WHITE_FLOOR - 1e-6 && c.white <= 1.0 + 1e-6);
                assert!((c.decay - 1.0).abs() <= VARY_DECAY + 1e-6);
                // And the flicker it drives stays where the old bounds were.
                for tick in 0..50 {
                    let (h, lean) = flicker_with(tick as f32 * 0.037, 0.3, 1.0, &c);
                    assert!(h > 0.7 && h < 1.3, "height factor out of range: {h}");
                    assert!(lean.abs() <= FLICKER_LEAN + VARY_LEAN + 1e-6);
                }
            }
        }
    }

    #[test]
    fn the_neutral_character_is_the_flame_that_was() {
        // Every old entry point is the new one with the neutral
        // character — the pins above this line still pin the flame.
        for life in [0.0, 0.3, 0.8, 1.0, IGNITE_OVERSHOOT] {
            for layer in [Layer::Core, Layer::Mantle, Layer::Aura] {
                assert_eq!(
                    body_shape(layer, life, 0.7),
                    body_shape_with(layer, life, 0.7, &Character::NEUTRAL)
                );
                let lane = Color::srgb(0.2, 0.5, 1.0);
                assert_eq!(
                    body_color(layer, life, lane),
                    body_color_with(layer, life, lane, &Character::NEUTRAL)
                );
            }
        }
        assert_eq!(
            flicker(0.37, 1.0, 1.0),
            flicker_with(0.37, 1.0, 1.0, &Character::NEUTRAL)
        );
        assert_eq!(
            advance_life(0.5, 0.0, 0.0, false, 0.1),
            advance_life_with(0.5, 0.0, 0.0, false, 0.1, &Character::NEUTRAL)
        );
        assert!(
            (breathe(1.0) - 1.0).abs() < 1e-6,
            "no stretch, no narrowing"
        );
    }

    #[test]
    fn a_stretched_flame_narrows_and_a_sagging_one_fattens() {
        assert!(breathe(1.18) < 1.0);
        assert!(breathe(0.82) > 1.0);
        // Half strength: a 18 % stretch narrows by well under 18 %.
        assert!(1.0 - breathe(1.18) < 0.18);
    }

    #[test]
    fn the_strike_flashes_the_core_whiter_and_a_dying_mantle_reddens() {
        let lane = Color::srgb(0.2, 0.5, 1.0);
        let (steady, steady_glow) = body_color(Layer::Core, 1.0, lane);
        let (flash, flash_glow) = body_color(Layer::Core, IGNITE_OVERSHOOT, lane);
        assert!(
            luma(flash) >= luma(steady),
            "the strike is at least as bright"
        );
        assert!(flash_glow > steady_glow, "and glows harder");
        // A mantle low on life leans to ember red: more red per green
        // than the same mantle at half life.
        let ratio = |c: Color| {
            let s = c.to_srgba();
            s.red / s.green.max(1e-3)
        };
        let mid = body_color(Layer::Mantle, 0.5, lane).0;
        let dying = body_color(Layer::Mantle, 0.12, lane).0;
        assert!(ratio(dying) > ratio(mid), "a dying mantle should redden");
        // A hue nuance moves the mantle but not the aura.
        let warm = Character {
            hue: 1.0,
            ..Character::NEUTRAL
        };
        let red = Character {
            hue: -1.0,
            ..Character::NEUTRAL
        };
        assert_ne!(
            body_color_with(Layer::Mantle, 0.8, lane, &warm).0,
            body_color_with(Layer::Mantle, 0.8, lane, &red).0
        );
        assert_eq!(
            body_color_with(Layer::Aura, 0.8, lane, &warm).0,
            body_color_with(Layer::Aura, 0.8, lane, &red).0,
            "the aura is the lane's glow, not fire"
        );
    }

    #[test]
    fn some_flames_snuff_and_some_linger_but_all_are_out_in_time() {
        let quick = Character {
            decay: 1.0 + VARY_DECAY,
            ..Character::NEUTRAL
        };
        let slow = Character {
            decay: 1.0 - VARY_DECAY,
            ..Character::NEUTRAL
        };
        let time_to_die = |c: &Character| {
            let mut life = IGNITE_OVERSHOOT;
            let mut t = 0.0;
            while life > 0.0 {
                life = advance_life_with(life, 0.0, 0.0, false, 1.0 / 120.0, c);
                t += 1.0 / 120.0;
            }
            t
        };
        assert!(time_to_die(&quick) < time_to_die(&slow));
        for c in [&quick, &slow] {
            let t = time_to_die(c);
            assert!(
                t > 0.25 && t < 0.45,
                "took {t} s — outside the flame's window"
            );
        }
        assert!(is_strike(0.2, 0.5) && !is_strike(0.5, 0.5) && !is_strike(0.5, 0.2));
    }

    #[test]
    fn a_strike_ignites_with_overshoot_and_a_repeat_only_re_raises() {
        // A hit rising from 0 to 1: life overshoots.
        let lit = advance_life(0.0, 0.0, 1.0, false, 0.0);
        assert!((lit - IGNITE_OVERSHOOT).abs() < 1e-6);
        // The same hit strength next frame is not a new strike: it
        // decays, it does not re-ignite.
        let later = advance_life(lit, 1.0, 1.0, false, 0.1);
        assert!(later < lit);
        // A second strike while still burning re-raises to the
        // overshoot — never higher: hits do not stack.
        let again = advance_life(0.6, 0.2, 1.0, false, 0.0);
        assert!((again - IGNITE_OVERSHOOT).abs() < 1e-6);
        // A weaker second strike never LOWERS a burning flame.
        let weaker = advance_life(1.0, 0.0, 0.62, false, 0.0);
        assert!(
            (weaker - 1.0).abs() < 1e-6,
            "a Good after a Perfect does not cut the flame"
        );
    }

    #[test]
    fn the_flame_is_out_in_well_under_half_a_second() {
        let mut life = IGNITE_OVERSHOOT;
        let mut t = 0.0;
        while life > 0.0 {
            life = advance_life(life, 1.0, 1.0, false, 1.0 / 120.0);
            t += 1.0 / 120.0;
        }
        assert!(t < 0.45, "took {t} s to die");
        assert!(t > 0.25, "died too fast to be seen: {t} s");
    }

    #[test]
    fn a_held_sustain_keeps_a_low_flame_and_nothing_else_does() {
        let held = advance_life(0.0, 0.0, 0.0, true, 1.0);
        assert!(held > 0.0 && held < 0.5);
        let free = advance_life(0.0, 0.0, 0.0, false, 1.0);
        assert!(free.abs() < 1e-6);
    }

    #[test]
    fn flicker_is_bounded_never_still_at_full_and_still_when_asked() {
        let mut min_h = f32::MAX;
        let mut max_h = f32::MIN;
        for step in 0..2000 {
            let (h, lean) = flicker(step as f32 * 0.001, 1.0, 1.0);
            min_h = min_h.min(h);
            max_h = max_h.max(h);
            assert!(h > 0.7 && h < 1.3, "height factor out of range: {h}");
            assert!(lean.abs() <= FLICKER_LEAN + 1e-6);
        }
        assert!(
            max_h - min_h > 0.15,
            "a flame that barely moves is not flickering"
        );
        // Reduced motion: dead still.
        assert_eq!(flicker(0.37, 1.0, 0.0), (1.0, 0.0));
    }

    #[test]
    fn the_bodies_nest_core_inside_mantle_inside_aura() {
        // The mesh deformation leaves the ignition point planted
        // and gives the tip a different axis from the foot.
        assert_eq!(flame_vertex([0.1, -0.5, 0.0]), [0.1, -0.5, 0.0]);
        assert!(flame_vertex([0.0, 0.5, 0.0])[0] > 0.05);
        let (core_g, core_h) = body_shape(Layer::Core, 1.0, 1.0);
        let (mantle_g, mantle_h) = body_shape(Layer::Mantle, 1.0, 1.0);
        let (aura_g, aura_h) = body_shape(Layer::Aura, 1.0, 1.0);
        assert!(
            core_g < mantle_g && mantle_g < aura_g,
            "girth grows outward"
        );
        assert!(
            core_h < aura_h && aura_h < mantle_h,
            "the coloured mantle extends beyond the white core"
        );
        // Dying: at half life the height has halved but the girth has
        // only fallen to ~71 % — short before thin.
        let (g, h) = body_shape(Layer::Core, 0.5, 1.0);
        assert!((h / core_h - 0.5).abs() < 1e-6);
        assert!((g / core_g - 0.5f32.sqrt()).abs() < 1e-6);
        // At no life there is no body.
        assert_eq!(body_shape(Layer::Core, 0.0, 1.0), (0.0, 0.0));
        // Intensity scales, never below 60 %.
        assert!((body_shape(Layer::Core, 1.0, 0.0).1 / core_h - 0.6).abs() < 1e-6);
    }

    #[test]
    fn the_core_is_whitest_at_the_strike_and_the_aura_is_the_lane() {
        let lane = Color::srgb(0.2, 0.8, 0.4);
        let (hot, hot_glow) = body_color(Layer::Core, 1.0, lane);
        let (cool, cool_glow) = body_color(Layer::Core, 0.2, lane);
        let l = |c: Color| c.luminance();
        assert!(l(hot) > l(cool), "the core cools as the flame dies");
        assert!(hot_glow > cool_glow);
        let (aura, _) = body_color(Layer::Aura, 1.0, lane);
        let a: bevy::color::Hsla = aura.into();
        let n: bevy::color::Hsla = lane.into();
        assert!((a.hue - n.hue).abs() < 1.0, "the aura keeps the lane's hue");
        assert!(l(aura) < l(lane), "and is darker than it");
    }

    #[test]
    fn an_ember_rises_accelerates_sways_and_fades_at_the_end() {
        let ttl = 0.5;
        let (_, y1, a1) = ember_flight(0.1, ttl, 0.5);
        let (_, y2, a2) = ember_flight(0.2, ttl, 0.5);
        let (_, y3, a3) = ember_flight(0.3, ttl, 0.5);
        assert!(y1 > 0.0 && y2 > y1 && y3 > y2, "it rises");
        assert!(y3 - y2 > y2 - y1, "and accelerates");
        assert!((a1 - 1.0).abs() < 1e-6 && (a2 - 1.0).abs() < 1e-6 && (a3 - 1.0).abs() < 1e-6);
        let (_, _, late) = ember_flight(0.45, ttl, 0.5);
        assert!(late > 0.0 && late < 1.0, "fading near the end, got {late}");
        assert_eq!(ember_flight(0.5, ttl, 0.5), (0.0, 0.0, 0.0), "gone at ttl");
        assert_eq!(
            ember_flight(-0.03, ttl, 0.5),
            (0.0, 0.0, 0.0),
            "not born yet"
        );
        // Two seeds, two paths.
        assert_ne!(ember_flight(0.2, ttl, 0.1).0, ember_flight(0.2, ttl, 0.9).0);
    }

    #[test]
    fn embers_cool_from_yellow_through_orange_to_red() {
        let fresh: bevy::color::Srgba = ember_color(0.0).into();
        let mid: bevy::color::Srgba = ember_color(0.5).into();
        let old: bevy::color::Srgba = ember_color(1.0).into();
        assert!(
            fresh.green > mid.green && mid.green > old.green,
            "green falls as it cools"
        );
        assert!(fresh.red >= mid.red && mid.red > old.red);
        assert!(old.red > old.green * 4.0, "dying: red");
    }

    #[test]
    fn rapid_hits_re_raise_the_flame_but_do_not_fire_a_new_shower_each_time() {
        // Two strikes 30 ms apart: the second may re-raise life (see
        // advance_life) but must not relaunch the embers.
        assert!(relaunch_allowed(1.0, 0.0));
        assert!(!relaunch_allowed(1.03, 1.0));
        assert!(relaunch_allowed(1.0 + EMBER_RELAUNCH_GAP, 1.0));
        // A first launch is always allowed.
        assert!(relaunch_allowed(0.0, -1.0));
    }

    #[test]
    fn the_intensity_slider_scales_the_pool_to_zero() {
        assert_eq!(ember_count(1.0), EMBERS);
        assert_eq!(ember_count(0.5), 3);
        assert_eq!(ember_count(0.0), 0);
    }
}
