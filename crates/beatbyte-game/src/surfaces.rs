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
    /// Stage-deck colour: planks, seams, scuffs.
    pub deck_color: Handle<Image>,
    /// Stage-deck normal map: seam grooves and plank warp.
    pub deck_normal: Handle<Image>,
    /// Stage-deck roughness / metallic tile.
    pub deck_rough: Handle<Image>,
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
            deck_color: images.add(with_mips(color_tile(TILE, deck_shade), false)),
            deck_normal: images.add(with_mips(
                normal_tile(TILE, deck_height, DECK_STRENGTH),
                true,
            )),
            deck_rough: images.add(with_mips(
                roughness_image(TILE, deck_rough, DECK_METALLIC),
                false,
            )),
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

/// How strongly the deck's height field tilts its normals.
pub const DECK_STRENGTH: f32 = 0.3;
/// The deck's metallic term: a sealed floor has a faint sheen.
pub const DECK_METALLIC: f32 = 0.05;
/// Planks across one tile.
pub const DECK_PLANKS: f32 = 6.0;
/// Half the gap between two boards, in texels.
const SEAM_HALF_TEXELS: f32 = 1.0;
/// The chamfer beside the gap, in texels: a board edge is eased, not
/// sawn square, and that eased edge is the line a stage light draws
/// along a floor. The first cut had none — a hard two-texel slot —
/// and the seams read as black chasms between slats.
///
/// Widened from 3.5 after looking: a narrow chamfer is a steep one,
/// every board answered a spot with the same hard rail of light, and
/// the deck read as corrugated plastic. A broad ease spreads that
/// answer instead of concentrating it.
const CHAMFER_TEXELS: f32 = 7.0;
/// Platform joints per tile: a stage deck is built from modules, so
/// the boards end. One joint per tile, and the tile is 3.3 m along
/// the deck (see the riser's `uv_transform`), which puts a joint at
/// about the length of a real staging platform.
const DECK_JOINTS: f32 = 1.0;
/// Half the width of a platform joint, in texels: wider than a board
/// seam, because two framed platforms meet there.
const JOINT_HALF_TEXELS: f32 = 1.6;
/// The chamfer beside a platform joint.
const JOINT_CHAMFER_TEXELS: f32 = 2.5;
/// How deeply a board is dished across its width. Boards cup as they
/// age, and a cupped board is a cylindrical mirror: it is what pulls
/// a light's reflection into a long streak ALONG the plank on a real
/// floor. Small — a hair of the chamfer's depth.
const DECK_CUP: f32 = 0.05;
/// The shallowest a board may be dished, as a share of [`DECK_CUP`].
///
/// Boards do not age alike, and identical ones are what turn a floor
/// into a comb: with every board answering a light the same way, the
/// deck read as ribbed. Some are nearly flat, some fully dished.
const DECK_CUP_FLOOR: f32 = 0.25;

fn plank_index(u: f32) -> usize {
    (u.rem_euclid(1.0) * DECK_PLANKS) as usize
}

/// Texels per plank across the tile.
fn plank_texels() -> f32 {
    TILE as f32 / DECK_PLANKS
}

/// How far `u` sits from the nearest board seam, in texels. Seams
/// run at the plank boundaries, so the measure is symmetric: a board
/// has one on each side. Pure — tested.
#[must_use]
pub fn seam_distance(u: f32) -> f32 {
    let across = (u.rem_euclid(1.0) * DECK_PLANKS).fract() * plank_texels();
    across.min(plank_texels() - across)
}

/// How far `v` sits from the nearest platform joint, in texels.
/// Pure — tested.
#[must_use]
pub fn joint_distance(v: f32) -> f32 {
    let per = TILE as f32 / DECK_JOINTS;
    let along = (v.rem_euclid(1.0) * DECK_JOINTS).fract() * per;
    along.min(per - along)
}

/// The surface across a groove: 0 in the gap (within `half` texels
/// of its centre), 1 on the flat beyond the chamfer, eased between.
/// Pure — tested.
#[must_use]
pub fn groove(distance_texels: f32, half: f32, chamfer: f32) -> f32 {
    smooth(((distance_texels - half) / chamfer).clamp(0.0, 1.0))
}

/// The board's surface across the tile: 0 in a seam, 1 on the flat.
/// Pure — tested.
#[must_use]
pub fn deck_profile(u: f32) -> f32 {
    groove(seam_distance(u), SEAM_HALF_TEXELS, CHAMFER_TEXELS)
}

/// The platform's surface along the tile: 0 in a joint, 1 elsewhere.
/// Pure — tested.
#[must_use]
pub fn deck_joint_profile(v: f32) -> f32 {
    groove(joint_distance(v), JOINT_HALF_TEXELS, JOINT_CHAMFER_TEXELS)
}

/// A hump that peaks on the chamfer itself and is zero both in the
/// gap and on the flat — the worn, paler edge of a board. Pure —
/// tested.
#[must_use]
pub fn deck_chamfer(u: f32) -> f32 {
    let p = deck_profile(u);
    4.0 * p * (1.0 - p)
}

/// How the board is dished across its width: 0 at the edges, 1 in
/// the middle of the board — scaled by how much THIS board has
/// cupped, which is not how much its neighbour has. Pure — tested.
#[must_use]
pub fn deck_cup(u: f32) -> f32 {
    let across = (u.rem_euclid(1.0) * DECK_PLANKS).fract();
    let centred = 2.0f32.mul_add(across, -1.0);
    let aged = (1.0 - DECK_CUP_FLOOR).mul_add(hash01(91 + plank_index(u)), DECK_CUP_FLOOR);
    (1.0 - centred * centred) * aged
}

/// Scuff streaks: three per tile, running along the planks, slowly
/// wandering. 0..1. Pure.
#[must_use]
pub fn deck_scuff(u: f32, v: f32) -> f32 {
    let mut mask: f32 = 0.0;
    for k in 0..3usize {
        let centre = hash01(31 + k * 17);
        let wander = 0.02 * (v * TAU + 3.0 * hash01(41 + k)).sin();
        let dx = ((u - centre - wander).rem_euclid(1.0) + 0.5).rem_euclid(1.0) - 0.5;
        let along = 0.5 + 0.5 * (v * TAU * 2.0 + 5.0 * hash01(51 + k)).sin();
        mask = mask.max((-(dx / 0.012).powi(2)).exp() * along);
    }
    mask.clamp(0.0, 1.0)
}

/// Deck brightness: planks with their own tone, grain along the
/// plank, pale worn edges, a dark gap between boards, a darker
/// platform joint across them, pale scuffs. Pure — tested.
#[must_use]
pub fn deck_shade(u: f32, v: f32) -> f32 {
    // Half again as much board-to-board variation as the first cut:
    // at rest the eye should be able to count the boards without a
    // light on them.
    let plank_tone = 0.09 * (hash01(61 + plank_index(u)) - 0.5);
    let grain = 0.78 + 0.14 * value_noise(u, v, 40, 6, 71) + plank_tone;
    let scuffed = lerp(grain, 0.92, 0.6 * deck_scuff(u, v));
    // The eased edge has lost its finish, so it gives back a little
    // of what the groove's shadow takes: the chamfer is still darker
    // than the flat, just not as dark as the gap it runs into. The
    // bright line an edge shows under a light comes from the normal
    // map, not from here.
    let edged = lerp(scuffed, (scuffed + 0.10).min(1.0), 0.7 * deck_chamfer(u));
    // Dark in the gap — but a gap between two boards has a board
    // edge at the bottom of it, not a hole through the stage.
    let gapped = lerp(0.34, edged, deck_profile(u));
    lerp(0.30, gapped, deck_joint_profile(v))
}

/// Deck relief: the gap between boards with its chamfer, the
/// platform joint across them, each board dished across its width,
/// and a long, gentle warp.
///
/// The warp used to be `value_noise(6, 6)` at 0.15 on a tile that
/// repeats thirty-six times over the deck — every light pool broke
/// into puddles. One cell per board, two along, a third of the
/// amplitude: boards sit a little proud of each other, and a pool
/// stays a pool.
#[must_use]
pub fn deck_height(u: f32, v: f32) -> f32 {
    let warp = 0.05 * value_noise(u, v, 6, 2, 81);
    let board = deck_profile(u).min(deck_joint_profile(v));
    let cup = DECK_CUP * deck_cup(u) * deck_profile(u);
    board - 0.5 + warp - cup
}

/// Deck roughness: glossy between scuffs, dull on them, and duller
/// still in the gaps and joints, where a floor is never polished.
#[must_use]
pub fn deck_rough(u: f32, v: f32) -> f32 {
    let open = (1.0 - deck_profile(u)).max(1.0 - deck_joint_profile(v));
    0.30 + 0.16 * deck_scuff(u, v) + 0.34 * open
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

    #[test]
    fn a_board_edge_is_eased_and_the_gap_is_not_a_hole() {
        // The first cut cut a hard two-texel slot: the seams read as
        // black chasms between slats rather than as boards laid
        // beside each other.
        let texel = |t: f32| t / TILE as f32;
        assert_eq!(deck_profile(texel(0.0)), 0.0, "the gap itself");
        // Positions come from the constants, not from the tuning of
        // the day: the first cut of this test wrote the texels down
        // and went red the moment the chamfer was widened.
        let across = SEAM_HALF_TEXELS + CHAMFER_TEXELS;
        let ramp: Vec<f32> = (1..5)
            .map(|k| deck_profile(texel(SEAM_HALF_TEXELS + across * k as f32 / 5.0)))
            .collect();
        for pair in ramp.windows(2) {
            assert!(pair[1] > pair[0], "the chamfer rises: {ramp:?}");
        }
        assert!(
            deck_profile(texel(across + 0.5)) > 0.999,
            "and reaches the flat of the board"
        );
        // Dark, but a gap between two boards has a board edge at the
        // bottom of it.
        let gap = deck_shade(texel(0.0), 0.5);
        assert!(
            gap > 0.25 && gap < deck_shade(texel(20.0), 0.5) - 0.2,
            "the gap is dark and not black: {gap}"
        );
        // ... and it is the darkest place across a board.
        for t in 1..40 {
            assert!(
                deck_shade(texel(t as f32), 0.5) >= gap,
                "nothing across the board is darker than its gap (texel {t})"
            );
        }
    }

    #[test]
    fn the_deck_is_built_from_platforms() {
        // A stage deck is modules; before this the boards ran the
        // whole thirty metres without a joint anywhere.
        let joint = deck_joint_profile(0.0);
        assert_eq!(joint, 0.0, "the joint itself");
        assert!(
            deck_joint_profile(0.5) > 0.999,
            "and the platform between joints is flat"
        );
        // It runs ACROSS the planks: on the flat of every board, not
        // only where a seam already darkens (each of these sits at
        // the middle of a board — `fract(u * DECK_PLANKS)` near 0.5).
        for u in [0.083, 0.25, 0.4167, 0.5833, 0.75] {
            assert!(
                deck_shade(u, 0.0) < deck_shade(u, 0.5) - 0.15,
                "the joint is dark at u {u}"
            );
            assert!(
                deck_height(u, 0.0) < deck_height(u, 0.5) - 0.5,
                "and grooved at u {u}"
            );
        }
        // And where a seam crosses it, the joint never brightens.
        assert!(deck_shade(0.0, 0.0) <= deck_shade(0.0, 0.5));
    }

    #[test]
    fn a_board_is_dished_across_its_width_but_only_just() {
        // A cupped board is a cylindrical mirror — it is what pulls a
        // stage light into a streak along the plank. It must stay far
        // shallower than the gap beside it, or the boards read as
        // gutters.
        let per = TILE as f32 / DECK_PLANKS;
        let flat = SEAM_HALF_TEXELS + CHAMFER_TEXELS + 1.0;
        // Every board, because they no longer cup alike: none may be
        // domed, and the deepest of them must actually dish.
        let mut deepest = 0.0f32;
        for board in 0..DECK_PLANKS as usize {
            let at = |t: f32| (board as f32 + t / per) / DECK_PLANKS;
            let middle = deck_height(at(per * 0.5), 0.5);
            let near_edge = deck_height(at(flat), 0.5);
            assert!(
                middle <= near_edge + 1e-4,
                "board {board} is domed: {middle} against {near_edge}"
            );
            deepest = deepest.max(near_edge - middle);
        }
        assert!(deepest > 0.01, "no board dishes at all: {deepest}");
        let gap = deck_height(flat / per / DECK_PLANKS, 0.5) - deck_height(0.0, 0.5);
        assert!(
            deepest < gap * 0.25,
            "the dish is a hair of the gap's depth: dish {deepest}, gap {gap}"
        );
    }

    #[test]
    fn the_flat_of_a_board_no_longer_puddles() {
        // The warp was value_noise(6, 6) at 0.15 on a tile that
        // repeats thirty-six times across the deck, and every light
        // pool broke into puddles. Two measures, both taken against
        // the old field before the thresholds were chosen: how often
        // the relief turns along one board (was 3 per tile, is 1),
        // and how far it wanders over a patch of the flat (was 0.140,
        // is 0.031 — against a gap almost a whole unit deep).
        let per = TILE as f32 / DECK_PLANKS;
        let u = 20.0 / per / DECK_PLANKS;
        let heights: Vec<f32> = (0..400)
            .map(|i| deck_height(u, 0.12 + i as f32 * 0.0019))
            .collect();
        let turns = heights
            .windows(3)
            .filter(|w| (w[1] - w[0]) * (w[2] - w[1]) < 0.0)
            .count();
        assert!(
            turns <= 2,
            "the relief along one board turns {turns} times in a tile"
        );
        let patch: Vec<f32> = (0..40)
            .flat_map(|i| {
                (0..40).map(move |j| {
                    deck_height(
                        (10.0 + i as f32 * 0.55) / per / DECK_PLANKS,
                        0.15 + j as f32 * 0.0175,
                    )
                })
            })
            .collect();
        let span = patch.iter().copied().fold(f32::MIN, f32::max)
            - patch.iter().copied().fold(f32::MAX, f32::min);
        assert!(span < 0.06, "the flat wanders {span} across a patch");
    }

    #[test]
    fn the_open_places_of_the_deck_are_never_polished() {
        // A floor is waxed where it is walked on, not down in the
        // gaps and joints where the mop never reaches.
        let flat = deck_rough(20.0 / (TILE as f32 / DECK_PLANKS) / DECK_PLANKS, 0.5);
        assert!(
            deck_rough(0.0, 0.5) > flat + 0.2,
            "the gap is duller than the board"
        );
        assert!(
            deck_rough(0.15, 0.0) > flat + 0.2,
            "and so is the platform joint"
        );
        for (u, v) in [(0.0f32, 0.0f32), (0.15, 0.5), (0.42, 0.27)] {
            let r = deck_rough(u, v);
            assert!((0.0..=1.0).contains(&r), "roughness {r} at {u},{v}");
        }
    }

    #[test]
    fn the_deck_seams_are_darker_than_the_plank() {
        let seam_u = 1.0 / DECK_PLANKS + 0.5 / TILE as f32;
        let plank_u = 1.0 / DECK_PLANKS + 0.5 / DECK_PLANKS;
        for v in [0.1, 0.5, 0.9] {
            assert!(
                deck_shade(seam_u, v) < deck_shade(plank_u, v) - 0.1,
                "seam {} vs plank {} at v {v}",
                deck_shade(seam_u, v),
                deck_shade(plank_u, v)
            );
        }
        assert!(deck_height(seam_u, 0.5) < deck_height(plank_u, 0.5) - 0.5);
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
