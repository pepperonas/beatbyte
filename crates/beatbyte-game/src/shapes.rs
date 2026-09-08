//! Per-lane note shapes — color is never the only channel.
//!
//! Every lane has a distinct 16×16 pixel-art shape (square, circle,
//! diamond, triangle, cross), used for note gems and receptors alike.
//! This is the DEFAULT: the colorblind-safe look ships on. The
//! "Note Style" setting can swap gems to plain round discs (classic
//! rhythm-game look) — an explicit player choice that makes color
//! the only lane signal. The masks are generated, not drawn — no
//! assets, and the geometry is unit-tested.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// The generated gem and surface images.
#[derive(Resource)]
pub struct LaneShapes {
    round_body: Handle<Image>,
    round_core: Handle<Image>,
    round_ring: Handle<Image>,
    sphere_body: Handle<Image>,
    sphere_gloss: Handle<Image>,
    soft_dot: Handle<Image>,
    tube: Handle<Image>,
    glow_strip: Handle<Image>,
    bed_gradient: Handle<Image>,
    vignette: Handle<Image>,
    gauge_arc: Handle<Image>,
    hype_glass: Handle<Image>,
    hype_fill: Handle<Image>,
    star: Handle<Image>,
    beam_gradient: Handle<Image>,
    led_module: Handle<Image>,
    plate: Handle<Image>,
    well: Handle<Image>,
}

impl LaneShapes {
    /// The gem body: a LIT sphere (grayscale shading × the sprite
    /// tint).
    #[must_use]
    pub fn body(&self) -> Handle<Image> {
        self.sphere_body.clone()
    }

    /// The plain disc texture (soft particles, backdrop dots).
    #[must_use]
    pub fn round_body(&self) -> Handle<Image> {
        self.round_body.clone()
    }

    /// The round gem's center dot.
    #[must_use]
    pub fn round_core(&self) -> Handle<Image> {
        self.round_core.clone()
    }

    /// The round gem's outer ring.
    #[must_use]
    pub fn round_ring(&self) -> Handle<Image> {
        self.round_ring.clone()
    }

    /// A lit sphere in grayscale (tinted by the lane color).
    #[must_use]
    pub fn sphere_body(&self) -> Handle<Image> {
        self.sphere_body.clone()
    }

    /// The sphere's untinted specular highlight overlay.
    #[must_use]
    pub fn sphere_gloss(&self) -> Handle<Image> {
        self.sphere_gloss.clone()
    }

    /// A gaussian soft dot (particles, backdrop glows).
    #[must_use]
    pub fn soft_dot(&self) -> Handle<Image> {
        self.soft_dot.clone()
    }

    /// The light-beam gradient: bright at the source, gone at the
    /// foot, with faint seamless striations around the shaft.
    #[must_use]
    pub fn beam_gradient(&self) -> Handle<Image> {
        self.beam_gradient.clone()
    }

    /// A HUD plate: brushed dark metal, vignetted, a light catch
    /// along the top edge, rivets in the corners.
    #[must_use]
    pub fn plate(&self) -> Handle<Image> {
        self.plate.clone()
    }

    /// A recessed readout well with faint scanlines.
    #[must_use]
    pub fn well(&self) -> Handle<Image> {
        self.well.clone()
    }

    /// An LED wall module: a fine dot matrix on a dark carrier.
    #[must_use]
    pub fn led_module(&self) -> Handle<Image> {
        self.led_module.clone()
    }

    /// The half-circle gauge track (the Hype gauge's dial): a ring
    /// over the upper half, with tick notches at every quarter and a
    /// stronger one at the halfway activation mark.
    #[must_use]
    pub fn gauge_arc(&self) -> Handle<Image> {
        self.gauge_arc.clone()
    }

    /// The Hype tube's glass housing.
    #[must_use]
    pub fn hype_glass(&self) -> Handle<Image> {
        self.hype_glass.clone()
    }

    /// The Hype tube's fill column.
    #[must_use]
    pub fn hype_fill(&self) -> Handle<Image> {
        self.hype_fill.clone()
    }

    /// A five-point star, tip up: the Hype tube's crown.
    #[must_use]
    pub fn star(&self) -> Handle<Image> {
        self.star.clone()
    }

    /// A soft-edged tube cross-section (sustain tails).
    #[must_use]
    pub fn tube(&self) -> Handle<Image> {
        self.tube.clone()
    }

    /// A thin soft glow strip (lane guides, fret lines).
    #[must_use]
    pub fn glow_strip(&self) -> Handle<Image> {
        self.glow_strip.clone()
    }

    /// A vertical depth gradient (highway bed).
    #[must_use]
    pub fn bed_gradient(&self) -> Handle<Image> {
        self.bed_gradient.clone()
    }

    /// The stage vignette overlay.
    #[must_use]
    pub fn vignette(&self) -> Handle<Image> {
        self.vignette.clone()
    }
}

/// Builds the shape images at startup.
pub struct ShapesPlugin;

impl Plugin for ShapesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build_shapes);
    }
}

pub(crate) fn build_shapes(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(LaneShapes {
        round_body: images.add(round_image(RoundPart::Body)),
        round_core: images.add(round_image(RoundPart::Core)),
        round_ring: images.add(round_image(RoundPart::Ring)),
        sphere_body: images.add(shaded_image(sphere_shading)),
        sphere_gloss: images.add(shaded_image(gloss_shading)),
        soft_dot: images.add(shaded_image(soft_dot_shading)),
        tube: images.add(shaded_image(tube_shading)),
        glow_strip: images.add(shaded_image(glow_strip_shading)),
        bed_gradient: images.add(shaded_image(bed_shading)),
        vignette: images.add(shaded_image(vignette_shading)),
        gauge_arc: images.add(shaded_image(gauge_arc_shading)),
        // Twice the pixels the tube occupies on a retina panel, and
        // in its own aspect so the caps stay round.
        hype_glass: images.add(shaded_image_wh(
            TUBE_TEXTURE_W,
            (TUBE_TEXTURE_W as f32 * TUBE_ASPECT).round() as usize,
            hype_glass_shading,
        )),
        hype_fill: images.add(shaded_image_wh(64, 64, hype_fill_shading)),
        star: images.add(shaded_image_wh(96, 96, star_shading)),
        beam_gradient: images.add(shaded_image(beam_shading)),
        led_module: images.add(shaded_image(led_module_shading)),
        plate: images.add(shaded_image(plate_shading)),
        well: images.add(shaded_image(well_shading)),
    });
}

/// A (value, alpha) shading sample; value is grayscale 0..1 so the
/// sprite tint supplies the hue.
pub type Shade = (f32, f32);

/// A light shaft's skin: `v` runs from the source (0) to the foot
/// (1). Real beams are dense at the lamp and dissolve into the air,
/// so alpha falls as a power curve; faint striations around the
/// shaft (`u`, seamless — whole sine periods) break the cone's
/// machined smoothness the way dust does. Pure — tested.
#[must_use]
pub fn beam_shading(u: f32, v: f32) -> Shade {
    // A short fade-in at the very tip, so the shaft does not start
    // with a hard bright edge at the lens.
    let head = (v / 0.06).clamp(0.0, 1.0);
    let body = (1.0 - v).clamp(0.0, 1.0).powf(1.7);
    let striae = 1.0 - 0.18 * (u * core::f32::consts::TAU * 5.0).sin().abs();
    (striae, head * body)
}

/// A HUD plate face: dark brushed metal (fine horizontal grain), a
/// vignette pulling the corners down, a light catch along the top
/// edge, and a rivet in each corner — the plates should read as
/// stage hardware, not as coloured rectangles. Pure — tested.
#[must_use]
pub fn plate_shading(u: f32, v: f32) -> Shade {
    let grain = 0.03 * (v * 640.0).sin();
    let vignette = 1.0 - 0.55 * (((u - 0.5).abs().powi(2) + (v - 0.5).abs().powi(2)) * 2.4);
    let mut value = (0.16 + grain) * vignette.max(0.25);
    // The top-edge light catch, tight and bright.
    if v < 0.05 {
        value = value.max(0.5 * (1.0 - v / 0.05));
    }
    // Corner rivets: four small bright domes.
    for (cx, cy) in [(0.045, 0.09), (0.955, 0.09), (0.045, 0.91), (0.955, 0.91)] {
        let dx = (u - cx) * 8.0;
        let dy = (v - cy) * 4.0;
        let dome = (1.0 - (dx * dx + dy * dy) * 14.0).clamp(0.0, 1.0);
        value = value.max(0.28f32.mul_add(dome, value));
    }
    (value.min(1.0), 1.0)
}

/// A recessed readout well: darker toward the top (light comes from
/// above, a recess shades under its lip) with faint scanlines.
/// Pure — tested.
#[must_use]
pub fn well_shading(u: f32, v: f32) -> Shade {
    let _ = u;
    let lip = 0.5 * (1.0 - (v / 0.16).min(1.0));
    let scan = 0.025 * ((v * 90.0).sin() * 0.5 + 0.5);
    ((0.06 + scan) * (1.0 - lip), 1.0)
}

/// An LED wall module: a matrix of small emitters on a dark carrier
/// — the pixel structure is what tells a screen from a lamp. Pure —
/// tested.
#[must_use]
pub fn led_module_shading(u: f32, v: f32) -> Shade {
    const GRID: f32 = 12.0;
    let fx = (u * GRID).fract() - 0.5;
    let fy = (v * GRID).fract() - 0.5;
    let distance = (fx * fx + fy * fy).sqrt();
    // Emitter dots glow; the carrier between them stays dark.
    let dot = (1.0 - (distance / 0.34).powi(2)).clamp(0.0, 1.0);
    (0.28f32.mul_add(0.25, 0.90 * dot).min(1.0), 1.0)
}

/// The gauge dial: a ring band across the UPPER half of the tile,
/// centred on the bottom-middle, so a needle pivoting there sweeps
/// it. Quarter ticks notch the band brighter; the halfway tick — the
/// activation threshold — is strongest. Pure — tested.
#[must_use]
pub fn gauge_arc_shading(u: f32, v: f32) -> Shade {
    // Pivot at (0.5, 1.0); the tile is meant to be drawn twice as
    // wide as tall, so u distances count double.
    let dx = (u - 0.5) * 2.0;
    let dy = 1.0 - v;
    let r = (dx * dx + dy * dy).sqrt();
    if !(0.62..=0.96).contains(&r) || dy < 0.0 {
        return (0.0, 0.0);
    }
    // Soft edges on both rims of the band.
    let edge = ((r - 0.62) / 0.03).min((0.96 - r) / 0.03).clamp(0.0, 1.0);
    // Angle across the sweep: 0 at the left horizon, 1 at the right.
    let sweep = 1.0 - (dy.atan2(-dx) / core::f32::consts::PI);
    // The band brightens along the sweep — the dial itself says
    // "more is that way" — and the READY half (past the activation
    // mark) sits a step brighter as a zone.
    let mut value = 0.30f32.mul_add(sweep, 0.20);
    if sweep >= 0.5 {
        value += 0.12;
    }
    for (tick, strength) in [(0.0, 0.9), (0.25, 0.7), (0.5, 1.0), (0.75, 0.7), (1.0, 0.9)] {
        let distance = (sweep - tick).abs();
        if distance < 0.012 {
            value = value.max(strength);
        }
    }
    (value, edge * 0.9)
}

/// The Hype tube's glass housing: a capsule with rounded caps, a
/// dark well inside, a bright rim, and a specular running down the
/// left shoulder.
///
/// The tile is meant to be drawn [`TUBE_ASPECT`] times taller than it
/// is wide, and the cap geometry is corrected for that — a capsule
/// drawn from a square tile would have oval ends. Pure — tested.
#[must_use]
pub fn hype_glass_shading(u: f32, v: f32) -> Shade {
    // Distance to the capsule's skeleton, in units of half-width.
    let dx = (u - 0.5) * 2.0;
    // The caps are half a width tall, which is 0.5/ASPECT in v.
    let cap = 0.5 / TUBE_ASPECT;
    let dy = if v < cap {
        (cap - v) * TUBE_ASPECT * 2.0
    } else if v > 1.0 - cap {
        (v - (1.0 - cap)) * TUBE_ASPECT * 2.0
    } else {
        0.0
    };
    let r = (dx * dx + dy * dy).sqrt();
    if r > 1.0 {
        return (0.0, 0.0);
    }
    // ⚠️ The glass is a FRAME, not a lid. Its alpha has to fall to
    // almost nothing across the middle, or it paints over the charge
    // it is supposed to contain — which is exactly what the first
    // version did: a handsome capsule with an unreadable meter
    // inside it.
    //
    // The rim: bright and opaque at the outside, gone by 78 % of the
    // way in.
    let rim = ((r - 0.78) / 0.20).clamp(0.0, 1.0);
    // A specular streak down the left shoulder, brightest up top.
    let streak = (1.0 - ((dx + 0.45).abs() / 0.22).min(1.0)).powi(2) * (1.0 - v * 0.55);
    let value = (0.34 + 0.66 * rim.max(streak * 0.9)).clamp(0.0, 1.0);
    // Alpha: the rim carries it, the streak adds a sheen, and the
    // middle keeps a whisper so the tube still reads as glazed.
    let alpha = (0.06 + 0.94 * rim.max(streak * 0.55)).min(1.0);
    // A soft outer edge so the capsule has no staircase.
    let edge = ((1.0 - r) / 0.06).clamp(0.0, 1.0);
    (value, alpha * edge)
}

/// The Hype tube's drawn width, in logical pixels.
pub const TUBE_W: f32 = 22.0;
/// The Hype tube's drawn height, in logical pixels.
pub const TUBE_H: f32 = 64.0;
/// How many times taller than wide the Hype tube is drawn. Derived,
/// never typed: the glass texture's cap geometry is corrected for
/// this ratio, and a ratio that drifted from the sprite's size would
/// stretch the caps into ovals again.
pub const TUBE_ASPECT: f32 = TUBE_H / TUBE_W;
// A tube, not a pill: the cap geometry assumes the straight middle
// is longer than the two caps together.
const _: () = assert!(TUBE_ASPECT > 2.0);
/// The glass texture's width in texels: about twice the tube's
/// retina footprint.
const TUBE_TEXTURE_W: usize = 96;

/// Signed distance from `(x, y)` (y up) to a five-point star with one
/// tip pointing up, `outer` the tip radius and `inner` the valley
/// radius. Negative inside. Pure — tested through [`star_shading`].
///
/// The star has five-fold rotational symmetry and a mirror through
/// every tip, so a point is folded into the wedge between a tip and
/// its neighbouring valley and measured against that ONE edge — as a
/// segment, not a line: measured against the line, points straight
/// beyond a tip sit close to both edge lines and would smear every
/// tip into a comet.
fn star_distance(x: f32, y: f32, outer: f32, inner: f32) -> f32 {
    use core::f32::consts::{FRAC_PI_2, TAU};
    let sector = TAU / 5.0;
    let radius = x.hypot(y);
    if radius < 1e-6 {
        return -inner;
    }
    let angle = (y.atan2(x) - FRAC_PI_2).rem_euclid(sector);
    // Angle from the nearest tip, 0 at the tip, sector/2 at the valley.
    let phi = if angle > sector / 2.0 {
        sector - angle
    } else {
        angle
    };
    let (px, py) = (radius * phi.cos(), radius * phi.sin());
    let (tx, ty) = (outer, 0.0);
    let (vx, vy) = (inner * (sector / 2.0).cos(), inner * (sector / 2.0).sin());
    let (ex, ey) = (vx - tx, vy - ty);
    let (wx, wy) = (px - tx, py - ty);
    let t = ((wx * ex + wy * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
    let (cx, cy) = (tx + t * ex, ty + t * ey);
    let distance = (px - cx).hypot(py - cy);
    // Same side of the edge as the origin = inside.
    let side = ex * wy - ey * wx;
    let origin_side = ex * (0.0 - ty) - ey * (0.0 - tx);
    if side * origin_side >= 0.0 {
        -distance
    } else {
        distance
    }
}

/// A five-point star, tip up, filling the tile: opaque with a soft
/// edge, lit from the centre out so the tint reads as a body rather
/// than a flat cut-out, with a faint facet on the left half of every
/// point. Pure — tested.
#[must_use]
pub fn star_shading(u: f32, v: f32) -> Shade {
    let (x, y) = ((u - 0.5) * 2.0, (0.5 - v) * 2.0);
    const OUTER: f32 = 0.92;
    const INNER: f32 = 0.42;
    let d = star_distance(x, y, OUTER, INNER);
    let alpha = ((-d) / 0.05 + 0.5).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return (0.0, 0.0);
    }
    let radius = (x.hypot(y) / OUTER).min(1.0);
    // Facets: the left flank of each point catches the light. From a
    // tip, angles grow counter-clockwise — leftward — so the first
    // half of the sector is that tip's left flank.
    use core::f32::consts::{FRAC_PI_2, TAU};
    let sector = TAU / 5.0;
    let angle = (y.atan2(x) - FRAC_PI_2).rem_euclid(sector);
    let facet = if angle <= sector / 2.0 { 0.08 } else { 0.0 };
    let value = (0.62 + 0.38 * (1.0 - radius).powf(0.8) + facet).clamp(0.0, 1.0);
    (value, alpha)
}

/// The Hype tube's fill: a column with a lit core and shaded flanks,
/// so the charge reads as a body of light rather than a flat bar.
///
/// Shaped across the WIDTH on purpose: the sprite is scaled
/// vertically as the meter fills, and anything that varied down the
/// tile would stretch with it. Pure — tested.
#[must_use]
pub fn hype_fill_shading(u: f32, _v: f32) -> Shade {
    let dx = (u - 0.5) * 2.0;
    // Lit core, falling off to the flanks; never fully dark, or the
    // column would look like two stripes.
    // A wide value range on purpose: multiplied by a pale violet, a
    // narrow one comes out as flat lavender. Dark flanks and a near
    // white core are what make it read as a lit column.
    let core = (1.0 - dx.abs()).powf(0.6);
    let value = 0.22 + 0.78 * core.powf(0.8);
    // The column is inset inside the glass, with a soft edge.
    let edge = ((1.0 - dx.abs()) / 0.18).clamp(0.0, 1.0);
    (value, edge)
}

/// Lit-sphere shading: Lambert diffuse from an upper-left light over
/// a hemisphere normal, ambient floor, darkened contact rim. Pure —
/// tested.
#[must_use]
pub fn sphere_shading(u: f32, v: f32) -> Shade {
    let (dx, dy) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    let r2 = dx * dx + dy * dy;
    let alpha = ((1.0 - r2.sqrt()) / 0.02 + 0.5).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return (0.0, 0.0);
    }
    let nz = (1.0 - r2).max(0.0).sqrt();
    // Light from upper-left, toward the viewer.
    let (lx, ly, lz) = (-0.42, -0.5, 0.76);
    let lambert = (dx * lx + dy * ly + nz * lz).max(0.0);
    let value = (0.32 + 0.68 * lambert) * (1.0 - 0.25 * r2 * r2);
    (value.clamp(0.0, 1.0), alpha)
}

/// The sphere's white gloss: a tight specular spot plus a soft upper
/// sheen — kept as a SEPARATE untinted layer, because a tinted white
/// highlight multiplies into the lane color and vanishes.
#[must_use]
pub fn gloss_shading(u: f32, v: f32) -> Shade {
    let (dx, dy) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    let r2 = dx * dx + dy * dy;
    if r2 > 1.0 {
        return (0.0, 0.0);
    }
    let sx = dx + 0.38;
    let sy = dy + 0.42;
    let spec = (1.0 - (sx * sx + sy * sy) * 6.0).max(0.0).powi(3);
    let sheen = ((-dy - 0.1).max(0.0) * 0.30) * (1.0 - r2);
    (
        (spec * 0.95 + sheen).min(1.0),
        (spec * 0.95 + sheen).min(1.0),
    )
}

/// Gaussian soft dot: bright core melting into nothing.
#[must_use]
pub fn soft_dot_shading(u: f32, v: f32) -> Shade {
    let (dx, dy) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    let a = (-(dx * dx + dy * dy) * 4.5).exp();
    (1.0, a)
}

/// Tube cross-section: solid glowing core, soft edges; vertically
/// uniform so the sprite can stretch to any sustain length.
#[must_use]
pub fn tube_shading(u: f32, _v: f32) -> Shade {
    let d = (u * 2.0 - 1.0).abs();
    let alpha = ((1.0 - d) / 0.35).clamp(0.0, 1.0);
    let core = ((0.45 - d) / 0.45).clamp(0.0, 1.0);
    ((0.7 + 0.3 * core).min(1.0), alpha * 0.9)
}

/// A narrow glow strip for guides and fret lines.
#[must_use]
pub fn glow_strip_shading(u: f32, _v: f32) -> Shade {
    let d = (u * 2.0 - 1.0).abs();
    (1.0, (-d * d * 5.0).exp() * 0.9)
}

/// Stage vignette: clear center, darkened corners — pulls the eye to
/// the highway like a lit stage in a dark venue.
#[must_use]
pub fn vignette_shading(u: f32, v: f32) -> Shade {
    let (dx, dy) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    let r = (dx * dx + dy * dy).sqrt();
    (0.0, ((r - 0.55) / 0.6).clamp(0.0, 1.0).powi(2) * 0.6)
}

/// Highway-bed depth gradient: darker far (top), lighter near.
#[must_use]
pub fn bed_shading(_u: f32, v: f32) -> Shade {
    (0.55 + 0.45 * v, 1.0)
}

/// Bake a shading function into a texture of the given size.
///
/// Non-square exists for the Hype tube: it is drawn four and a half
/// times taller than it is wide, and a square texture stretched to
/// that shape turns its round caps into ellipses and its specular
/// into a smear.
fn shaded_image_wh(width: usize, height: usize, shade: fn(f32, f32) -> Shade) -> Image {
    let mut data = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let (value, alpha) = shade(
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            );
            let v = (value.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[v, v, v, (alpha.clamp(0.0, 1.0) * 255.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// Bake a shading function into a 256-px linearly sampled texture.
fn shaded_image(shade: fn(f32, f32) -> Shade) -> Image {
    const SIZE: usize = 256;
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (value, alpha) = shade(
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            );
            let v = (value.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[v, v, v, (alpha.clamp(0.0, 1.0) * 255.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// The three layers of a round gem.
#[derive(Clone, Copy)]
pub enum RoundPart {
    /// The filled disc (tinted in the lane color).
    Body,
    /// The small center dot (white in play — the documented look:
    /// every gem carries a white center).
    Core,
    /// The outer ring (dark on strum notes; ABSENT on HOPOs, which
    /// is the documented strum/HOPO distinction).
    Ring,
}

/// Side length of the high-resolution round-gem textures. The 8-bit
/// shapes stay 16×16 nearest-sampled ON PURPOSE (that IS the look);
/// the round style is the opposite promise — smooth, so it renders
/// large with anti-aliased edges and linear sampling.
pub const ROUND_SIZE: usize = 128;

/// Anti-aliased coverage (0..1) of a round-gem layer at pixel
/// (x, y) of a `size`-pixel texture. Pure — the geometry tests and
/// the texture builder share it.
#[must_use]
pub fn round_coverage(part: RoundPart, x: f32, y: f32, size: f32) -> f32 {
    let half = size / 2.0;
    let dx = x + 0.5 - half;
    let dy = y + 0.5 - half;
    let r = (dx * dx + dy * dy).sqrt() / half; // 0 at center, 1 at edge
    // ~1.5 texture pixels of edge softness, in normalized units.
    let aa = 1.5 / half;
    let inside = |edge: f32| ((edge - r) / aa + 0.5).clamp(0.0, 1.0);
    match part {
        RoundPart::Body => inside(0.925),
        RoundPart::Core => inside(0.325),
        RoundPart::Ring => (inside(0.925) - inside(0.7375)).clamp(0.0, 1.0),
    }
}

/// Build one high-resolution, linearly sampled round-gem texture.
fn round_image(part: RoundPart) -> Image {
    let size = ROUND_SIZE;
    let mut data = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let alpha = round_coverage(part, x as f32, y as f32, size as f32);
            data.extend_from_slice(&[255, 255, 255, (alpha * 255.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // The app-wide default is nearest; these two lines are what
    // keeps a generated texture from looking like pixel art.
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod round_tests {
    use super::{ROUND_SIZE, RoundPart, round_coverage};

    fn sample(part: RoundPart, x: usize, y: usize) -> f32 {
        round_coverage(part, x as f32, y as f32, ROUND_SIZE as f32)
    }

    fn area(part: RoundPart) -> f32 {
        let mut sum = 0.0;
        for y in 0..ROUND_SIZE {
            for x in 0..ROUND_SIZE {
                sum += sample(part, x, y);
            }
        }
        sum / (ROUND_SIZE * ROUND_SIZE) as f32
    }

    #[test]
    fn body_is_a_substantial_disc_and_core_a_small_dot() {
        let body = area(RoundPart::Body);
        assert!((0.5..0.8).contains(&body), "body area off: {body}");
        let core = area(RoundPart::Core);
        assert!((0.02..0.15).contains(&core), "core area off: {core}");
    }

    #[test]
    fn ring_is_hollow() {
        let mid = ROUND_SIZE / 2;
        assert!(sample(RoundPart::Ring, mid, mid) < 0.01, "center not empty");
        assert!(
            sample(RoundPart::Ring, ROUND_SIZE - 10, mid) > 0.9,
            "ring band not filled"
        );
    }

    #[test]
    fn layers_nest_inside_the_body() {
        for part in [RoundPart::Core, RoundPart::Ring] {
            for y in 0..ROUND_SIZE {
                for x in 0..ROUND_SIZE {
                    if sample(part, x, y) > 0.5 {
                        assert!(
                            sample(RoundPart::Body, x, y) > 0.5,
                            "layer outside body at {x},{y}"
                        );
                    }
                }
            }
        }
    }

    /// The point of the high-resolution set: edges must be SOFT —
    /// there exist genuinely partial pixels (anti-aliasing), which
    /// the 16×16 boolean masks can never produce.
    #[test]
    fn edges_are_anti_aliased() {
        let mid = ROUND_SIZE / 2;
        let partial = (0..ROUND_SIZE)
            .filter(|&x| {
                let a = sample(RoundPart::Body, x, mid);
                a > 0.05 && a < 0.95
            })
            .count();
        assert!(partial >= 2, "no soft edge pixels found: {partial}");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod shading_tests {
    use super::*;

    #[test]
    fn sphere_is_lit_from_the_upper_left() {
        let (bright, _) = sphere_shading(0.32, 0.30);
        let (dark, _) = sphere_shading(0.75, 0.78);
        assert!(
            bright > dark + 0.2,
            "upper-left {bright} must clearly outshine lower-right {dark}"
        );
        assert!(sphere_shading(0.99, 0.99).1 < 0.05, "corner must be clear");
    }

    #[test]
    fn gloss_peaks_near_the_light_and_stays_inside() {
        let (peak, _) = gloss_shading(0.31, 0.29);
        assert!(peak > 0.5, "specular spot missing: {peak}");
        assert_eq!(gloss_shading(0.99, 0.99).1, 0.0, "gloss outside sphere");
    }

    #[test]
    fn soft_dot_and_strip_fade_to_nothing() {
        assert!(soft_dot_shading(0.5, 0.5).1 > 0.9);
        assert!(soft_dot_shading(0.02, 0.5).1 < 0.05);
        assert!(glow_strip_shading(0.5, 0.0).1 > 0.8);
        assert!(glow_strip_shading(0.02, 0.0).1 < 0.05);
    }

    #[test]
    fn tube_is_symmetric_with_a_bright_core() {
        let (core, core_a) = tube_shading(0.5, 0.1);
        let (edge, _) = tube_shading(0.85, 0.9);
        assert!(core > edge, "core {core} must outshine edge {edge}");
        assert!(core_a > 0.8);
        let left = tube_shading(0.3, 0.5);
        let right = tube_shading(0.7, 0.5);
        assert!((left.0 - right.0).abs() < 1e-6 && (left.1 - right.1).abs() < 1e-6);
    }

    #[test]
    fn bed_darkens_with_distance() {
        assert!(bed_shading(0.5, 0.05).0 < bed_shading(0.5, 0.95).0 - 0.3);
    }
}
