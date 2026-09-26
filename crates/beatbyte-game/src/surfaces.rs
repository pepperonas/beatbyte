//! Procedural PBR surfaces for the 3D stage: the tolex of a PA
//! cabinet, grille cloth, brushed metal, a loudspeaker cone and the
//! stage deck — colour, normal and roughness tiles generated at
//! startup from a deterministic hash. No image files: every asset in
//! this repository is original or generated, and these are generated.
//!
//! Three rules the tiles live by, each of which failed silently when
//! it was first broken:
//!
//! - **Colour tiles are sRGB, data tiles are not.** A normal map or a
//!   roughness map encoded as `Rgba8UnormSrgb` is decoded through the
//!   sRGB curve by the sampler and every normal leans.
//! - **A normal map needs tangents.** A mesh without
//!   `ATTRIBUTE_TANGENT` renders its normal map as flat; see
//!   [`tangent_mesh`].
//! - **A repeated tile needs mips.** The deck is seen at a grazing
//!   angle for thirty units; without a mip chain the grain shimmers
//!   (MSAA is edge anti-aliasing, it does nothing for textures).
//!
//! Every generator here is pure and tested; the plugin only bakes
//! them into GPU images once, at `PreStartup`, like the note shapes.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use core::f32::consts::{PI, TAU};

use crate::gameplay::fx::hash01;

/// Side length of the surface tiles, in texels.
pub const TILE: usize = 256;

/// Side length of the brushed-metal roughness tile.
pub const METAL_TILE: usize = 128;

/// Bakes the surface tiles once, before anything spawns.
pub struct SurfacesPlugin;

impl Plugin for SurfacesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build_surfaces);
    }
}

/// The baked tiles, one handle each; materials share them.
#[derive(Resource, Clone)]
pub struct StageSurfaces {
    /// Tolex (pebble-grain vinyl) colour, greyscale — multiplied by
    /// the theme's cabinet colour.
    pub tolex_color: Handle<Image>,
    /// Tolex normal map.
    pub tolex_normal: Handle<Image>,
    /// Tolex roughness (G) / metallic (B) tile.
    pub tolex_rough: Handle<Image>,
    /// Grille-cloth weave normal map.
    pub cloth_normal: Handle<Image>,
    /// Brushed-metal roughness tile: striations along `u`.
    pub metal_rough: Handle<Image>,
    /// Loudspeaker driver colour (surround, cone, dust cap).
    pub driver_color: Handle<Image>,
    /// Loudspeaker driver normal map.
    pub driver_normal: Handle<Image>,
    /// Stage-deck colour: black panels, worn paint, tape, joints.
    pub deck_color: Handle<Image>,
    /// Stage-deck normal map: panel joints, tape, paint relief.
    pub deck_normal: Handle<Image>,
    /// Stage-deck roughness / metallic tile.
    pub deck_rough: Handle<Image>,
    /// Venue-floor colour: sealed concrete with aggregate and slab joints.
    pub concrete_color: Handle<Image>,
    /// Venue-floor normal map: fine aggregate and recessed slab joints.
    pub concrete_normal: Handle<Image>,
    /// Venue-floor roughness / metallic tile.
    pub concrete_rough: Handle<Image>,
}

pub(crate) fn build_surfaces(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(StageSurfaces::build(&mut images));
}

impl StageSurfaces {
    /// Bake every tile. A plain function so tests can build the set
    /// without an `App`.
    #[must_use]
    pub fn build(images: &mut Assets<Image>) -> StageSurfaces {
        StageSurfaces {
            tolex_color: images.add(with_mips(color_tile(TILE, tolex_shade), false)),
            tolex_normal: images.add(with_mips(
                normal_tile(TILE, tolex_height, TOLEX_STRENGTH),
                true,
            )),
            tolex_rough: images.add(with_mips(roughness_image(TILE, tolex_rough, 0.0), false)),
            cloth_normal: images.add(with_mips(
                normal_tile(TILE, weave_height, CLOTH_STRENGTH),
                true,
            )),
            metal_rough: images.add(with_mips(
                roughness_image(METAL_TILE, metal_rough, 1.0),
                false,
            )),
            driver_color: images.add(color_tile(TILE, driver_shade)),
            driver_normal: images.add(normal_tile(TILE, driver_height, DRIVER_STRENGTH)),
            deck_color: images.add(with_mips(color_tile(DECK_TILE, deck_shade), false)),
            deck_normal: images.add(with_mips(
                normal_tile(DECK_TILE, deck_height, DECK_STRENGTH),
                true,
            )),
            deck_rough: images.add(with_mips(
                roughness_image(DECK_TILE, deck_rough, DECK_METALLIC),
                false,
            )),
            concrete_color: images.add(with_mips(color_tile(TILE, concrete_shade), false)),
            concrete_normal: images.add(with_mips(
                normal_tile(TILE, concrete_height, CONCRETE_STRENGTH),
                true,
            )),
            concrete_rough: images
                .add(with_mips(roughness_image(TILE, concrete_rough, 0.0), false)),
        }
    }
}

// ---- noise ---------------------------------------------------------------

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Tileable value noise in 0..1: a `cells_u × cells_v` lattice of
/// hashed values, smoothly interpolated, wrapping at the tile edge so
/// a repeated tile shows no seam. Pure.
#[must_use]
pub fn value_noise(u: f32, v: f32, cells_u: u32, cells_v: u32, seed: usize) -> f32 {
    let cells_u = cells_u.max(1);
    let cells_v = cells_v.max(1);
    let fu = u.rem_euclid(1.0) * cells_u as f32;
    let fv = v.rem_euclid(1.0) * cells_v as f32;
    let iu = fu.floor();
    let iv = fv.floor();
    let tu = smooth(fu - iu);
    let tv = smooth(fv - iv);
    let lattice = |x: u32, y: u32| {
        let x = x % cells_u;
        let y = y % cells_v;
        hash01(
            seed.wrapping_mul(1_000_003)
                .wrapping_add(y as usize * cells_u as usize + x as usize),
        )
    };
    let x0 = iu as u32;
    let y0 = iv as u32;
    let top = lerp(lattice(x0, y0), lattice(x0 + 1, y0), tu);
    let bottom = lerp(lattice(x0, y0 + 1), lattice(x0 + 1, y0 + 1), tu);
    lerp(top, bottom, tv)
}

/// A per-texel speckle, constant inside one texel of a `TILE`-sized
/// tile so it survives the height→normal derivative as grain rather
/// than noise.
fn speckle(u: f32, v: f32, seed: usize) -> f32 {
    let x = (u.rem_euclid(1.0) * TILE as f32) as usize;
    let y = (v.rem_euclid(1.0) * TILE as f32) as usize;
    hash01(seed.wrapping_mul(7_919).wrapping_add(y * TILE + x))
}

// ---- tolex ---------------------------------------------------------------

/// How strongly the tolex height field tilts its normals.
pub const TOLEX_STRENGTH: f32 = 0.025;

/// Pebble-grain vinyl: two octaves of noise and a fine speckle.
#[must_use]
pub fn tolex_height(u: f32, v: f32) -> f32 {
    0.6 * value_noise(u, v, 24, 24, 11)
        + 0.3 * value_noise(u, v, 48, 48, 12)
        + 0.1 * speckle(u, v, 13)
}

/// Tolex brightness, 0.62..1.0 — multiplied by the cabinet colour.
#[must_use]
pub fn tolex_shade(u: f32, v: f32) -> f32 {
    0.62 + 0.30 * value_noise(u, v, 24, 24, 11) + 0.08 * speckle(u, v, 13)
}

/// Tolex roughness: matte, with a little variation between pebbles.
#[must_use]
pub fn tolex_rough(u: f32, v: f32) -> f32 {
    0.72 + 0.16 * value_noise(u, v, 48, 48, 12)
}

// ---- grille cloth --------------------------------------------------------

/// How strongly the weave tilts its normals.
pub const CLOTH_STRENGTH: f32 = 0.02;

/// Threads per tile edge of the grille cloth.
pub const WEAVE_THREADS: f32 = 16.0;

/// A plain weave: sixteen threads each way, over-under alternating,
/// each thread a half-sine bump across its width. The pattern repeats
/// every TWO threads (one over, one under), so the period is an
/// eighth of the tile, not a sixteenth. Pure — tested.
#[must_use]
pub fn weave_height(u: f32, v: f32) -> f32 {
    let su = u.rem_euclid(1.0) * WEAVE_THREADS;
    let sv = v.rem_euclid(1.0) * WEAVE_THREADS;
    let over = (su.floor() + sv.floor()) as i64 % 2 == 0;
    let across_u = (PI * (su - su.floor())).sin();
    let across_v = (PI * (sv - sv.floor())).sin();
    // The thread on top is the one running ALONG the other axis: an
    // "over" cell shows the thread that runs along u, whose profile
    // varies across v.
    if over {
        0.35 * across_u + 0.65 * across_v
    } else {
        0.65 * across_u + 0.35 * across_v
    }
}

// ---- brushed metal -------------------------------------------------------

/// Brushed metal: roughness striations along `u` (constant along the
/// brush direction, varying across it) plus a little noise.
#[must_use]
pub fn metal_rough(u: f32, v: f32) -> f32 {
    0.48 + 0.14 * (0.5 + 0.5 * (v * TAU * 48.0).sin()) + 0.04 * value_noise(u, v, 32, 32, 21)
}

// ---- loudspeaker driver --------------------------------------------------

/// How strongly the driver's height field tilts its normals.
pub const DRIVER_STRENGTH: f32 = 0.35;

/// Radius (of the unit disc) where the cone meets the surround.
const CONE_EDGE: f32 = 0.82;
/// Radius of the dust cap.
const DUST_CAP: f32 = 0.16;

fn driver_distance(u: f32, v: f32) -> f32 {
    let dx = (u - 0.5) * 2.0;
    let dy = (v - 0.5) * 2.0;
    (dx * dx + dy * dy).sqrt()
}

/// Driver brightness: a rubber surround catching a little light, a
/// cone darker toward the throat, a glinting dust cap; the baffle
/// beyond the rim is near-black. Pure — tested.
#[must_use]
pub fn driver_shade(u: f32, v: f32) -> f32 {
    let d = driver_distance(u, v);
    if d > 1.0 {
        0.06
    } else if d > CONE_EDGE {
        0.30
    } else if d < DUST_CAP {
        0.42
    } else {
        0.16 - 0.10 * (1.0 - d)
    }
}

/// Driver relief: the cone falls from its rim to the throat, the
/// surround is a half-round bump, the dust cap a dome. Pure.
#[must_use]
pub fn driver_height(u: f32, v: f32) -> f32 {
    let d = driver_distance(u, v);
    if d > 1.0 {
        0.6
    } else if d > CONE_EDGE {
        0.6 + 0.4 * (PI * (d - CONE_EDGE) / (1.0 - CONE_EDGE)).sin()
    } else if d < DUST_CAP {
        let x = d / DUST_CAP;
        0.25 * (1.0 - x * x).max(0.0).sqrt()
    } else {
        0.6 * d / CONE_EDGE
    }
}

// ---- stage deck ----------------------------------------------------------

// Black stage panels, since 2026-09-26. A concert deck is built from
// framed platforms (the common module is 2 x 1 m), painted matte black
// so the light, not the floor, carries the colour — the old deck was
// seventy-two narrow boards tinted from the theme, and under a
// coloured wash they read as purple stripes. The tile is 4 x 4 m: two
// panels across, four along, so eight panels with their own tone,
// scuffs and tape marks before the pattern repeats.

/// Texels per deck tile edge: 128 per metre on a 4 m tile.
pub const DECK_TILE: usize = 512;
/// How strongly the deck's height field tilts its normals.
pub const DECK_STRENGTH: f32 = 0.22;
/// The deck's metallic term: painted plywood has none.
pub const DECK_METALLIC: f32 = 0.0;
/// Panels across one tile (along `u`, 2 m each).
pub const DECK_COLUMNS: usize = 2;
/// Panels along one tile (along `v`, 1 m each).
pub const DECK_ROWS: usize = 4;
/// Half the gap between two panels, in texels (about 1 cm).
const JOINT_HALF_TEXELS: f32 = 1.2;
/// The eased edge of a panel, in texels: a framed platform has a
/// radiused rim, and that rim is what a stage light draws along.
const JOINT_CHAMFER_TEXELS: f32 = 3.5;
/// The paint's brightness: black, not a hole — lights must still read
/// on it.
const PAINT: f32 = 0.11;
/// The worn, lighter grey where feet and cases have scuffed through.
const WORN: f32 = 0.19;
/// Gaffer tape: a dusty grey mark, the brightest thing on the deck.
/// Not white: the first cut at 0.82 read as road markings, repeated
/// every tile like a car park.
const TAPE: f32 = 0.40;
/// The gap between panels.
const GAP: f32 = 0.05;

fn tile_texels() -> f32 {
    DECK_TILE as f32
}

/// Which of the tile's panels `(u, v)` lies on. Pure — tested.
#[must_use]
pub fn panel_index(u: f32, v: f32) -> usize {
    let column = ((u.rem_euclid(1.0) * DECK_COLUMNS as f32) as usize).min(DECK_COLUMNS - 1);
    let row = ((v.rem_euclid(1.0) * DECK_ROWS as f32) as usize).min(DECK_ROWS - 1);
    row * DECK_COLUMNS + column
}

/// How far `(u, v)` sits from the nearest panel joint, in texels.
/// Pure — tested.
#[must_use]
pub fn joint_distance(u: f32, v: f32) -> f32 {
    let along = |x: f32, count: usize| {
        let per = tile_texels() / count as f32;
        let at = (x.rem_euclid(1.0) * count as f32).fract() * per;
        at.min(per - at)
    };
    along(u, DECK_COLUMNS).min(along(v, DECK_ROWS))
}

/// The surface across a groove: 0 in the gap (within `half` texels
/// of its centre), 1 on the flat beyond the chamfer, eased between.
/// Pure — tested.
#[must_use]
pub fn groove(distance_texels: f32, half: f32, chamfer: f32) -> f32 {
    smooth(((distance_texels - half) / chamfer).clamp(0.0, 1.0))
}

/// The panel's surface: 0 in a joint, 1 on the flat. Pure — tested.
#[must_use]
pub fn deck_profile(u: f32, v: f32) -> f32 {
    groove(
        joint_distance(u, v),
        JOINT_HALF_TEXELS,
        JOINT_CHAMFER_TEXELS,
    )
}

/// Where the paint is worn through, 0..1: long, soft streaks running
/// along the deck (the way cases are pushed and people walk), broken
/// up by a finer noise so no streak is a clean stripe. Pure — tested.
#[must_use]
pub fn deck_wear(u: f32, v: f32) -> f32 {
    let streaks = value_noise(u, v, 22, 3, 71);
    let breakup = value_noise(u, v, 64, 48, 72);
    ((streaks - 0.55) * 3.0 * (0.4 + 0.8 * breakup)).clamp(0.0, 1.0)
}

/// Gaffer tape, 0..1: on some panels a spike mark (a short L at a
/// corner) or a strip across the panel — where somebody marked a
/// monitor or a mic stand. Sparse on purpose: a deck taped all over
/// reads as a pattern, not as use. Pure — tested.
#[must_use]
pub fn deck_tape(u: f32, v: f32) -> f32 {
    let panel = panel_index(u, v);
    let roll = hash01(301 + panel * 7);
    // One panel in four or so: a deck taped all over reads as a
    // pattern repeating with every tile, not as use.
    if roll > 0.3 {
        return 0.0;
    }
    // Position inside the panel, 0..1 each way.
    let pu = (u.rem_euclid(1.0) * DECK_COLUMNS as f32).fract();
    let pv = (v.rem_euclid(1.0) * DECK_ROWS as f32).fract();
    // Tape is ~5 cm wide: 5 cm of a 2 m panel across, of a 1 m panel along.
    let (half_u, half_v) = (0.0125, 0.025);
    let x0 = 0.2 + 0.6 * hash01(311 + panel);
    let y0 = 0.2 + 0.6 * hash01(321 + panel);
    let edge = |d: f32, half: f32| if d.abs() < half { 1.0 } else { 0.0 };
    if roll < 0.18 {
        // A spike mark: an L, arms 20 cm (0.1 of the panel across,
        // 0.2 along).
        let horizontal = edge(pv - y0, half_v) * f32::from(u8::from(pu >= x0 && pu <= x0 + 0.1));
        let vertical = edge(pu - x0, half_u) * f32::from(u8::from(pv >= y0 && pv <= y0 + 0.2));
        horizontal.max(vertical)
    } else {
        // A strip across part of the panel, along the deck.
        let length = 0.25 + 0.35 * hash01(331 + panel);
        edge(pu - x0, half_u)
            * f32::from(u8::from(pv >= y0 - length * 0.5 && pv <= y0 + length * 0.5))
    }
}

/// Deck brightness: matte black paint with a per-panel tone, worn
/// streaks, gaffer tape, a darker joint between panels. Pure —
/// tested.
#[must_use]
pub fn deck_shade(u: f32, v: f32) -> f32 {
    let tone = 0.03 * (hash01(61 + panel_index(u, v)) - 0.5);
    let paint = PAINT + tone + 0.04 * (value_noise(u, v, 96, 96, 73) - 0.5);
    let worn = lerp(paint, WORN, 0.8 * deck_wear(u, v));
    let taped = lerp(worn, TAPE, deck_tape(u, v));
    lerp(GAP, taped, deck_profile(u, v))
}

/// Deck relief: the joint between panels with its eased rim, the
/// tape standing a hair proud, and a fine orange-peel of the paint.
/// No warp: a framed platform is flat, and a pool of light on it stays
/// a pool.
#[must_use]
pub fn deck_height(u: f32, v: f32) -> f32 {
    let peel = 0.03 * value_noise(u, v, 128, 128, 74);
    deck_profile(u, v) - 0.5 + peel + 0.08 * deck_tape(u, v)
}

/// Deck roughness: matte paint, a little smoother where it is worn
/// (feet polish what they scuff), tape a touch shinier than paint,
/// and the joints dullest of all.
#[must_use]
pub fn deck_rough(u: f32, v: f32) -> f32 {
    let surface = 0.74 - 0.22 * deck_wear(u, v);
    let taped = lerp(surface, 0.58, deck_tape(u, v));
    lerp(0.95, taped, deck_profile(u, v))
}

// ---- venue concrete -----------------------------------------------------

/// How strongly the aggregate and slab joints tilt the concrete normals.
pub const CONCRETE_STRENGTH: f32 = 0.055;

fn concrete_joint(u: f32, v: f32) -> f32 {
    let edge = |x: f32| {
        let x = x.rem_euclid(1.0);
        x.min(1.0 - x)
    };
    let distance = edge(u).min(edge(v));
    smooth((distance / 0.025).clamp(0.0, 1.0))
}

/// Sealed venue concrete: broad mottling, fine aggregate and recessed
/// expansion joints. The range stays narrow so it receives coloured light
/// without becoming a second playfield.
#[must_use]
pub fn concrete_shade(u: f32, v: f32) -> f32 {
    let broad = value_noise(u, v, 5, 5, 101);
    let aggregate = value_noise(u, v, 28, 28, 102);
    let body = 0.62 + 0.10 * broad + 0.045 * (aggregate - 0.5);
    lerp(0.42, body, concrete_joint(u, v))
}

/// Fine concrete relief with a shallow groove at each slab boundary.
#[must_use]
pub fn concrete_height(u: f32, v: f32) -> f32 {
    let aggregate = 0.65 * value_noise(u, v, 24, 24, 103) + 0.35 * speckle(u, v, 104);
    aggregate - 0.75 * (1.0 - concrete_joint(u, v))
}

/// Mostly matte concrete; aggregate varies the finish and joints collect
/// dust, making them rougher than the slab faces.
#[must_use]
pub fn concrete_rough(u: f32, v: f32) -> f32 {
    let aggregate = value_noise(u, v, 18, 18, 105);
    (0.67 + 0.11 * aggregate + 0.16 * (1.0 - concrete_joint(u, v))).clamp(0.0, 1.0)
}

// ---- height → normal -----------------------------------------------------

/// Sample a height function at every texel centre of a `size²`
/// tile, row-major, row 0 at `v = 0`.
#[must_use]
pub fn height_field(size: usize, height: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let mut field = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            field.push(height(
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            ));
        }
    }
    field
}

/// Tangent-space normals from a height field, by central
/// differences that wrap at the tile edge (so the result tiles as
/// seamlessly as the field). `strength` scales the gradient measured
/// per unit of UV. The convention is the one Bevy's shader unpacks
/// (`flip_normal_map_y` stays false): x leans away from a hill to the
/// right, y leans toward the TOP of the image on a hill's upper side
/// (green points up the image, as glTF/OpenGL normal maps do).
/// Pure — tested.
#[must_use]
pub fn normals_from_height(field: &[f32], size: usize, strength: f32) -> Vec<[f32; 3]> {
    let at = |x: isize, y: isize| {
        let x = x.rem_euclid(size as isize) as usize;
        let y = y.rem_euclid(size as isize) as usize;
        field[y * size + x]
    };
    let per_uv = size as f32 * 0.5;
    let mut normals = Vec::with_capacity(size * size);
    for y in 0..size as isize {
        for x in 0..size as isize {
            let gx = (at(x + 1, y) - at(x - 1, y)) * per_uv;
            let gy = (at(x, y + 1) - at(x, y - 1)) * per_uv;
            let n = Vec3::new(-gx * strength, gy * strength, 1.0).normalize();
            normals.push([n.x, n.y, n.z]);
        }
    }
    normals
}

fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..ImageSamplerDescriptor::linear()
    })
}

fn rgba_image(size: usize, data: Vec<u8>, format: TextureFormat) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = repeat_sampler();
    image
}

/// Encode a normal into a texel: `(n · 0.5 + 0.5) · 255`.
#[must_use]
pub fn encode_normal(n: [f32; 3]) -> [u8; 4] {
    let byte = |c: f32| ((c * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(n[0]), byte(n[1]), byte(n[2]), 255]
}

/// Decode a texel back into a unit-length normal (tests, mips).
#[must_use]
pub fn decode_normal(texel: [u8; 4]) -> [f32; 3] {
    let c = |b: u8| f32::from(b) / 255.0 * 2.0 - 1.0;
    let n = Vec3::new(c(texel[0]), c(texel[1]), c(texel[2])).normalize_or_zero();
    [n.x, n.y, n.z]
}

/// A normal map as a linear (`Rgba8Unorm`, NOT sRGB) repeating tile.
#[must_use]
pub fn normal_image(normals: &[[f32; 3]], size: usize) -> Image {
    let mut data = Vec::with_capacity(size * size * 4);
    for n in normals {
        data.extend_from_slice(&encode_normal(*n));
    }
    rgba_image(size, data, TextureFormat::Rgba8Unorm)
}

/// Height function → normal map in one step.
#[must_use]
pub fn normal_tile(size: usize, height: impl Fn(f32, f32) -> f32, strength: f32) -> Image {
    let field = height_field(size, height);
    normal_image(&normals_from_height(&field, size, strength), size)
}

/// A roughness/metallic tile in the layout Bevy samples (G =
/// roughness, B = metallic), multiplied by the material's scalars.
/// Linear format, repeating.
#[must_use]
pub fn roughness_image(size: usize, rough: impl Fn(f32, f32) -> f32, metallic: f32) -> Image {
    let mut data = Vec::with_capacity(size * size * 4);
    let m = (metallic.clamp(0.0, 1.0) * 255.0).round() as u8;
    for y in 0..size {
        for x in 0..size {
            let r = rough(
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            data.extend_from_slice(&[255, (r.clamp(0.0, 1.0) * 255.0).round() as u8, m, 255]);
        }
    }
    rgba_image(size, data, TextureFormat::Rgba8Unorm)
}

/// A greyscale colour tile (sRGB, repeating); the material's base
/// colour supplies the hue.
#[must_use]
pub fn color_tile(size: usize, shade: impl Fn(f32, f32) -> f32) -> Image {
    let mut data = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let value = shade(
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let v = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    rgba_image(size, data, TextureFormat::Rgba8UnormSrgb)
}

// ---- mips ----------------------------------------------------------------

/// The full mip chain of a square RGBA8 image: level 0 as given,
/// every further level the 2×2 box average of the one before, down
/// to one texel. A normal map (`normal`) is averaged as vectors and
/// renormalised, so a distant tile still faces the way it should.
/// Pure — tested.
#[must_use]
pub fn mip_chain(level0: &[u8], size: usize, normal: bool) -> Vec<Vec<u8>> {
    let mut levels = vec![level0.to_vec()];
    let mut current = level0.to_vec();
    let mut edge = size;
    while edge > 1 {
        let next_edge = edge / 2;
        let mut next = Vec::with_capacity(next_edge * next_edge * 4);
        for y in 0..next_edge {
            for x in 0..next_edge {
                let texel = |dx: usize, dy: usize| {
                    let i = ((2 * y + dy) * edge + (2 * x + dx)) * 4;
                    [current[i], current[i + 1], current[i + 2], current[i + 3]]
                };
                let quad = [texel(0, 0), texel(1, 0), texel(0, 1), texel(1, 1)];
                if normal {
                    let mut sum = Vec3::ZERO;
                    for t in quad {
                        let n = decode_normal(t);
                        sum += Vec3::new(n[0], n[1], n[2]);
                    }
                    let n = sum.normalize_or(Vec3::Z);
                    next.extend_from_slice(&encode_normal([n.x, n.y, n.z]));
                } else {
                    for c in 0..4 {
                        let sum: u32 = quad.iter().map(|t| u32::from(t[c])).sum();
                        next.push(((sum + 2) / 4) as u8);
                    }
                }
            }
        }
        levels.push(next.clone());
        current = next;
        edge = next_edge;
    }
    levels
}

/// Give a square RGBA8 tile its full mip chain.
#[must_use]
pub fn with_mips(mut image: Image, normal: bool) -> Image {
    let size = image.texture_descriptor.size.width as usize;
    let Some(level0) = image.data.take() else {
        return image;
    };
    let levels = mip_chain(&level0, size, normal);
    image.texture_descriptor.mip_level_count = levels.len() as u32;
    image.data = Some(levels.concat());
    image
}

// ---- tangents ------------------------------------------------------------

/// Give a mesh the tangents its normal map needs. On failure the
/// mesh is returned untangented and a warning names the cause: the
/// map then renders flat, which is the honest fallback, not a crash.
#[must_use]
pub fn tangent_mesh(mut mesh: Mesh) -> Mesh {
    if let Err(error) = mesh.generate_tangents() {
        warn!("surfaces: no tangents ({error}); the normal map on this mesh stays flat");
    }
    mesh
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn decoded(image: &Image, size: usize, x: usize, y: usize) -> [f32; 3] {
        let data = image.data.as_ref().unwrap();
        let i = (y * size + x) * 4;
        decode_normal([data[i], data[i + 1], data[i + 2], data[i + 3]])
    }

    #[test]
    fn a_flat_field_faces_straight_up() {
        let size = 16;
        let field = vec![0.37; size * size];
        let image = normal_image(&normals_from_height(&field, size, 1.0), size);
        let data = image.data.as_ref().unwrap();
        for texel in data.chunks(4) {
            assert!((i32::from(texel[0]) - 128).abs() <= 1, "{texel:?}");
            assert!((i32::from(texel[1]) - 128).abs() <= 1, "{texel:?}");
            assert_eq!(texel[2], 255, "{texel:?}");
        }
    }

    #[test]
    fn every_normal_is_unit_length_after_encoding() {
        let size = 64;
        let image = normal_tile(size, tolex_height, TOLEX_STRENGTH);
        let data = image.data.as_ref().unwrap();
        assert_eq!(data.len(), size * size * 4);
        for texel in data.chunks(4) {
            let c = |b: u8| f32::from(b) / 255.0 * 2.0 - 1.0;
            let len = (c(texel[0]).powi(2) + c(texel[1]).powi(2) + c(texel[2]).powi(2)).sqrt();
            assert!((len - 1.0).abs() < 0.02, "|n| = {len} for {texel:?}");
        }
    }

    #[test]
    fn the_normal_leans_away_from_the_hill() {
        // One bump in the middle of the tile.
        let size = 32;
        let field = height_field(size, |u, v| {
            let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)) / 0.02;
            (-d).exp()
        });
        let image = normal_image(&normals_from_height(&field, size, 0.2), size);
        let peak = size / 2;
        let right = decoded(&image, size, peak + 3, peak);
        let left = decoded(&image, size, peak - 3, peak);
        let below = decoded(&image, size, peak, peak + 3);
        let above = decoded(&image, size, peak, peak - 3);
        assert!(
            right[0] > 0.05,
            "right of the hill leans right (+x): {right:?}"
        );
        assert!(
            left[0] < -0.05,
            "left of the hill leans left (-x): {left:?}"
        );
        // Green points UP the image: the lower side leans down.
        assert!(below[1] < -0.05, "below the hill leans down: {below:?}");
        assert!(above[1] > 0.05, "above the hill leans up: {above:?}");
    }

    #[test]
    fn the_tile_wraps_seamlessly() {
        let size = 32;
        let field = height_field(size, tolex_height);
        let shift = |f: &[f32]| -> Vec<f32> {
            let mut out = vec![0.0; size * size];
            for y in 0..size {
                for x in 0..size {
                    out[((y + size / 2) % size) * size + (x + size / 2) % size] = f[y * size + x];
                }
            }
            out
        };
        let shifted_then_normals = normals_from_height(&shift(&field), size, 0.1);
        let normals = normals_from_height(&field, size, 0.1);
        let mut normals_then_shifted = vec![[0.0; 3]; size * size];
        for y in 0..size {
            for x in 0..size {
                normals_then_shifted[((y + size / 2) % size) * size + (x + size / 2) % size] =
                    normals[y * size + x];
            }
        }
        for (a, b) in shifted_then_normals.iter().zip(&normals_then_shifted) {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 1e-5, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn the_weave_repeats_every_two_threads_and_not_sooner() {
        let mut differs_at_a_thread = false;
        for i in 0..40 {
            let u = 0.013 + i as f32 * 0.0237;
            let v = 0.021 + i as f32 * 0.0191;
            let period = 2.0 / WEAVE_THREADS;
            assert!(
                (weave_height(u + period, v) - weave_height(u, v)).abs() < 1e-4,
                "period along u at ({u}, {v})"
            );
            assert!(
                (weave_height(u, v + period) - weave_height(u, v)).abs() < 1e-4,
                "period along v at ({u}, {v})"
            );
            if (weave_height(u + period / 4.0, v) - weave_height(u, v)).abs() > 1e-3 {
                differs_at_a_thread = true;
            }
        }
        assert!(
            differs_at_a_thread,
            "a quarter period must change the weave"
        );
    }

    /// The middle of panel `(column, row)` of the tile.
    fn panel_middle(column: usize, row: usize) -> (f32, f32) {
        (
            (column as f32 + 0.5) / DECK_COLUMNS as f32,
            (row as f32 + 0.5) / DECK_ROWS as f32,
        )
    }

    #[test]
    fn the_deck_is_a_grid_of_two_by_one_panels() {
        // Two across, four along: 2 x 1 m on a 4 m tile, eight panels
        // with their own identity.
        let mut seen = std::collections::HashSet::new();
        for column in 0..DECK_COLUMNS {
            for row in 0..DECK_ROWS {
                let (u, v) = panel_middle(column, row);
                assert!(seen.insert(panel_index(u, v)));
                assert!(deck_profile(u, v) > 0.999, "the middle of a panel is flat");
            }
        }
        assert_eq!(seen.len(), 8);
        // Joints at the panel edges, both ways.
        let texel = 1.0 / DECK_TILE as f32;
        assert_eq!(deck_profile(0.5, 0.125), 0.0, "joint between the columns");
        assert_eq!(deck_profile(0.25, 0.25), 0.0, "joint between the rows");
        assert_eq!(deck_profile(0.0, 0.1), 0.0, "and at the tile edge");
        assert!(
            deck_profile(0.5 + 10.0 * texel, 0.125) > 0.999,
            "a joint is narrow"
        );
    }

    #[test]
    fn a_panel_edge_is_eased_and_the_joint_is_not_a_hole() {
        let texel = |t: f32| t / DECK_TILE as f32;
        let across = JOINT_HALF_TEXELS + JOINT_CHAMFER_TEXELS;
        let ramp: Vec<f32> = (1..5)
            .map(|k| {
                deck_profile(
                    0.5 + texel(JOINT_HALF_TEXELS + across * k as f32 / 5.0),
                    0.125,
                )
            })
            .collect();
        for pair in ramp.windows(2) {
            assert!(pair[1] > pair[0], "the rim rises: {ramp:?}");
        }
        // The joint is the darkest line, dark but not black.
        let joint = deck_shade(0.5, 0.1);
        assert!(joint > 0.0 && joint < PAINT - 0.05, "joint {joint}");
        assert!(
            deck_height(0.5, 0.1) < deck_height(0.3, 0.1) - 0.5,
            "and a groove"
        );
    }

    #[test]
    fn the_paint_is_black_and_the_colour_is_left_to_the_light() {
        // The old deck multiplied a theme-tinted base with bright
        // planks and read as purple stripes. Paint is dark and grey;
        // the only bright things are worn spots and tape.
        let mut sum = 0.0;
        let mut n = 0.0;
        for i in 0..64 {
            for j in 0..64 {
                let (u, v) = (i as f32 / 64.0 + 0.003, j as f32 / 64.0 + 0.003);
                let s = deck_shade(u, v);
                assert!((0.0..=1.0).contains(&s));
                if deck_tape(u, v) == 0.0 {
                    assert!(s <= WORN + 0.03, "paint stays dark: {s} at {u},{v}");
                }
                sum += s;
                n += 1.0;
            }
        }
        let mean = sum / n;
        assert!(mean > 0.06 && mean < 0.18, "mean brightness {mean}");
    }

    #[test]
    fn tape_is_sparse_bright_and_off_the_joints() {
        let mut taped = 0usize;
        let total = 256 * 256;
        for i in 0..256 {
            for j in 0..256 {
                let (u, v) = ((i as f32 + 0.5) / 256.0, (j as f32 + 0.5) / 256.0);
                if deck_tape(u, v) > 0.5 {
                    taped += 1;
                    assert!(deck_shade(u, v) > WORN || deck_profile(u, v) < 1.0);
                }
            }
        }
        let share = taped as f32 / total as f32;
        assert!(
            share > 0.001 && share < 0.03,
            "tape covers {share} of the deck"
        );
        // Some panels have none at all.
        let bare = (0..8)
            .filter(|&p| {
                let (u, v) = panel_middle(p % DECK_COLUMNS, p / DECK_COLUMNS);
                (0..40).all(|k| {
                    let du = (k as f32 / 40.0 - 0.5) / DECK_COLUMNS as f32;
                    (0..40).all(|m| {
                        deck_tape(u + du, v + (m as f32 / 40.0 - 0.5) / DECK_ROWS as f32) == 0.0
                    })
                })
            })
            .count();
        assert!(bare >= 3, "only {bare} panels are untaped");
    }

    #[test]
    fn the_joints_are_never_polished_and_wear_is_smoother() {
        let (u, v) = panel_middle(0, 1);
        assert!(
            deck_rough(0.5, v) > deck_rough(u, v) + 0.15,
            "joint duller than paint"
        );
        for (a, b) in [(0.0f32, 0.0f32), (0.3, 0.6), (0.77, 0.42)] {
            let r = deck_rough(a, b);
            assert!((0.0..=1.0).contains(&r));
        }
        // Wear polishes: where the wear is strongest, roughness is lowest.
        let mut worn = (0.0f32, 1.0f32);
        for i in 0..200 {
            let (a, b) = (0.13 + i as f32 * 0.0017, 0.61);
            if deck_tape(a, b) == 0.0 && deck_profile(a, b) > 0.999 && deck_wear(a, b) > worn.0 {
                worn = (deck_wear(a, b), deck_rough(a, b));
            }
        }
        assert!(worn.0 > 0.3, "some wear exists");
        assert!(
            worn.1 < 0.74 - 0.05,
            "and is smoother than fresh paint: {worn:?}"
        );
    }

    #[test]
    fn the_deck_tile_repeats_without_a_seam() {
        for k in 0..20 {
            let v = 0.05 + k as f32 * 0.045;
            assert!((deck_shade(0.0001, v) - deck_shade(0.9999, v)).abs() < 0.08);
            assert!((deck_height(0.3, 0.0001) - deck_height(0.3, 0.9999)).abs() < 0.08);
        }
    }

    #[test]
    fn concrete_has_matte_slabs_with_recessed_joints() {
        let face = (0.5, 0.5);
        let joint = (0.0, 0.5);
        assert!(concrete_shade(joint.0, joint.1) < concrete_shade(face.0, face.1) - 0.1);
        assert!(concrete_height(joint.0, joint.1) < concrete_height(face.0, face.1) - 0.2);
        assert!(concrete_rough(joint.0, joint.1) > concrete_rough(face.0, face.1));
        for (u, v) in [(0.0, 0.0), (0.17, 0.62), (0.5, 0.5)] {
            assert!((0.0..=1.0).contains(&concrete_shade(u, v)));
            assert!((0.0..=1.0).contains(&concrete_rough(u, v)));
        }
    }

    #[test]
    fn the_driver_reads_as_a_cone_in_a_surround() {
        let at = |d: f32| driver_shade(0.5 + d * 0.5, 0.5);
        assert!(
            at(0.9) > at(0.5),
            "the surround catches more light than the cone"
        );
        assert!(at(0.05) > at(0.5), "the dust cap glints");
        assert!(at(0.3) < at(0.7), "the cone darkens toward the throat");
        assert!(at(1.1) < at(0.9), "beyond the rim is the baffle");
        let h = |d: f32| driver_height(0.5 + d * 0.5, 0.5);
        assert!(h(0.75) > h(0.3), "the cone falls toward the throat");
        assert!(
            h(0.91) > h(0.82) && h(0.91) > h(0.99),
            "the surround is a bump"
        );
    }

    #[test]
    fn the_mip_chain_halves_to_one_texel() {
        // A 4×4 checker of 0 and 255.
        let size = 4;
        let mut level0 = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                level0.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let levels = mip_chain(&level0, size, false);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[1].len(), 2 * 2 * 4);
        assert_eq!(levels[2].len(), 4);
        for texel in levels[1].chunks(4) {
            assert!((i32::from(texel[0]) - 128).abs() <= 1, "{texel:?}");
        }
        let image = with_mips(color_tile(size, |_, _| 0.5), false);
        assert_eq!(image.texture_descriptor.mip_level_count, 3);
        assert_eq!(image.data.as_ref().unwrap().len(), (16 + 4 + 1) * 4);
        // A normal map's mips stay unit length.
        let flat = normal_image(&[[0.0, 0.0, 1.0]; 16], 4);
        let chain = mip_chain(flat.data.as_ref().unwrap(), 4, true);
        assert_eq!(chain[2], vec![128, 128, 255, 255]);
    }

    #[test]
    fn data_tiles_are_not_srgb() {
        let normal = normal_tile(8, weave_height, CLOTH_STRENGTH);
        assert_eq!(normal.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        let rough = roughness_image(8, metal_rough, 1.0);
        assert_eq!(rough.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        let texel = &rough.data.as_ref().unwrap()[0..4];
        assert_eq!(texel[2], 255, "metallic lives in the blue channel");
        let color = color_tile(8, deck_shade);
        assert_eq!(
            color.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
    }

    #[test]
    fn value_noise_tiles_and_stays_in_range() {
        for i in 0..50 {
            let u = i as f32 * 0.0731;
            let v = i as f32 * 0.0419;
            let n = value_noise(u, v, 12, 12, 5);
            assert!((0.0..=1.0).contains(&n));
            assert!((value_noise(u + 1.0, v, 12, 12, 5) - n).abs() < 1e-5);
            assert!((value_noise(u, v + 2.0, 12, 12, 5) - n).abs() < 1e-5);
        }
    }
}
