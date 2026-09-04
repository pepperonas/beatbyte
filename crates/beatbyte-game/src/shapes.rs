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

use beatbyte_core::Lane;

/// Shape mask side length in pixels.
pub const SHAPE_SIZE: usize = 16;

/// The generated shape images, indexed by lane, plus the round-gem
/// set used when the player turns the 8-bit shapes off.
#[derive(Resource)]
pub struct LaneShapes {
    per_lane: [Handle<Image>; 5],
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
    beam_gradient: Handle<Image>,
    speaker_sub: Handle<Image>,
    speaker_top: Handle<Image>,
    led_module: Handle<Image>,
    plate: Handle<Image>,
    well: Handle<Image>,
}

impl LaneShapes {
    /// The 8-bit shape image for a lane.
    #[must_use]
    pub fn image(&self, lane: Lane) -> Handle<Image> {
        self.per_lane[lane as usize].clone()
    }

    /// The gem body for a lane in the chosen style. Round bodies are
    /// LIT spheres (grayscale shading × the sprite tint).
    #[must_use]
    pub fn body(&self, lane: Lane, round: bool) -> Handle<Image> {
        if round {
            self.sphere_body.clone()
        } else {
            self.image(lane)
        }
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

    /// A subwoofer cabinet front: one big driver.
    #[must_use]
    pub fn speaker_sub(&self) -> Handle<Image> {
        self.speaker_sub.clone()
    }

    /// A top-cabinet front: woofer below, tweeter above.
    #[must_use]
    pub fn speaker_top(&self) -> Handle<Image> {
        self.speaker_top.clone()
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

fn build_shapes(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let handles: Vec<Handle<Image>> = (0..5)
        .map(|lane| images.add(mask_to_image(&shape_mask(lane))))
        .collect();
    let per_lane: [Handle<Image>; 5] = match handles.try_into() {
        Ok(array) => array,
        Err(_) => unreachable!("exactly five shapes are generated"),
    };
    commands.insert_resource(LaneShapes {
        per_lane,
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
        hype_glass: images.add(shaded_image_wh(64, 296, hype_glass_shading)),
        hype_fill: images.add(shaded_image_wh(64, 64, hype_fill_shading)),
        beam_gradient: images.add(shaded_image(beam_shading)),
        speaker_sub: images.add(shaded_image(|u, v| speaker_shading(u, v, true))),
        speaker_top: images.add(shaded_image(|u, v| speaker_shading(u, v, false))),
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

/// One loudspeaker driver at `(cx, cy)` with radius `r`: surround
/// ring, dark cone falling toward the centre, a small glinting dust
/// cap. Returns the brightness to paint at `(u, v)`, or None when
/// the point is outside the driver.
fn driver_at(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> Option<f32> {
    let dx = u - cx;
    let dy = v - cy;
    let distance = (dx * dx + dy * dy).sqrt() / r;
    if distance > 1.0 {
        return None;
    }
    Some(if distance > 0.82 {
        0.30 // the rubber surround catches a little light
    } else if distance < 0.16 {
        0.42 // dust cap glint
    } else {
        // The cone: darker toward the throat, as a real cone shades.
        0.16 - 0.10 * (1.0 - distance)
    })
}

/// A speaker cabinet front (`sub` = one big driver; otherwise a
/// woofer below and a tweeter above), on a dark grille with a faint
/// weave. Pure — tested.
#[must_use]
pub fn speaker_shading(u: f32, v: f32, sub: bool) -> Shade {
    // The grille weave: a faint regular dot lattice.
    let weave = 0.015
        * ((u * core::f32::consts::TAU * 24.0).sin() * (v * core::f32::consts::TAU * 24.0).sin())
            .abs();
    let mut value = 0.10 + weave;
    if sub {
        if let Some(driver) = driver_at(u, v, 0.5, 0.5, 0.40) {
            value = driver;
        }
    } else {
        if let Some(driver) = driver_at(u, v, 0.5, 0.66, 0.30) {
            value = driver;
        }
        if let Some(driver) = driver_at(u, v, 0.5, 0.22, 0.13) {
            value = driver;
        }
    }
    (value, 1.0)
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

/// How many times taller than wide the Hype tube is drawn.
pub const TUBE_ASPECT: f32 = 74.0 / 16.0;

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
    // The app-wide default is nearest (pixel art); these two lines are
    // exactly why the round style does not look 8-bit.
    image.sampler = bevy::image::ImageSampler::linear();
    image
}

/// The pixel mask for a lane's shape. Pure — the geometry tests live
/// on this.
#[must_use]
pub fn shape_mask(lane: usize) -> [[bool; SHAPE_SIZE]; SHAPE_SIZE] {
    let mut mask = [[false; SHAPE_SIZE]; SHAPE_SIZE];
    let center = (SHAPE_SIZE as f32 - 1.0) / 2.0; // 7.5
    for (y, row) in mask.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            *cell = match lane {
                // Lane 0: square (1 px margin).
                0 => dx.abs() <= 6.5 && dy.abs() <= 6.5,
                // Lane 1: circle.
                1 => dx * dx + dy * dy <= 7.3 * 7.3,
                // Lane 2: diamond.
                2 => dx.abs() + dy.abs() <= 7.5,
                // Lane 3: triangle, point up, base at the bottom.
                3 => {
                    let progress = y as f32 / (SHAPE_SIZE as f32 - 1.0);
                    dy >= -7.0 && dx.abs() <= progress * 7.4
                }
                // Lane 4: X cross (thick diagonals).
                _ => {
                    let on_diag = (dx - dy).abs() <= 2.2 || (dx + dy).abs() <= 2.2;
                    on_diag && dx.abs() <= 7.0 && dy.abs() <= 7.0
                }
            };
        }
    }
    mask
}

/// White-on-transparent RGBA image from a mask (tinted by sprite
/// color at draw time).
fn mask_to_image(mask: &[[bool; SHAPE_SIZE]; SHAPE_SIZE]) -> Image {
    let mut data = Vec::with_capacity(SHAPE_SIZE * SHAPE_SIZE * 4);
    for row in mask {
        for &on in row {
            data.extend_from_slice(if on { &[255u8; 4] } else { &[0u8; 4] });
        }
    }
    Image::new(
        Extent3d {
            width: SHAPE_SIZE as u32,
            height: SHAPE_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {

    #[test]
    fn the_plate_reads_as_hardware_not_a_rectangle() {
        use super::plate_shading;
        // The top edge catches light, the corners carry rivets that
        // outshine the metal beside them, and the field vignettes
        // darker than the centre.
        let (edge, _) = plate_shading(0.5, 0.01);
        let (centre, _) = plate_shading(0.5, 0.5);
        let (corner_field, _) = plate_shading(0.18, 0.09);
        let (rivet, _) = plate_shading(0.045, 0.09);
        assert!(
            edge > centre,
            "top catch {edge} must outshine the field {centre}"
        );
        assert!(
            rivet > corner_field,
            "rivet {rivet} vs plain corner {corner_field}"
        );
    }

    #[test]
    fn the_well_shades_under_its_lip() {
        use super::well_shading;
        // A recess is darkest right under the lip - that shadow is
        // what makes it read as IN the plate instead of ON it.
        let (under_lip, _) = well_shading(0.5, 0.02);
        let (floor, _) = well_shading(0.5, 0.7);
        assert!(under_lip < floor, "lip shadow {under_lip} vs floor {floor}");
    }

    #[test]
    fn the_speaker_front_reads_as_drivers_on_a_dark_grille() {
        use super::speaker_shading;
        // The sub's big cone is darker than the grille around it,
        // its surround brighter than the cone - that contrast IS
        // the driver.
        let (grille, _) = speaker_shading(0.06, 0.06, true);
        let (cone, _) = speaker_shading(0.5, 0.62, true);
        let (surround, _) = speaker_shading(0.5, 0.5 + 0.40 * 0.9, true);
        assert!(
            cone < grille,
            "cone {cone} must sit darker than grille {grille}"
        );
        assert!(
            surround > cone,
            "surround {surround} must catch light over cone {cone}"
        );
        // The top cab really has TWO drivers - sampled mid-cone,
        // not dead centre (the centre is the glinting dust cap).
        assert!(super::speaker_shading(0.5, 0.66 + 0.30 * 0.5, false).0 < 0.2);
        assert!(super::speaker_shading(0.5, 0.22 + 0.13 * 0.5, false).0 < 0.2);
    }

    #[test]
    fn the_led_module_is_dots_on_a_dark_carrier() {
        use super::led_module_shading;
        // Dead centre of an emitter: bright. Between four emitters:
        // the dark carrier. Without that gap a module is a lamp.
        let (dot, _) = led_module_shading(0.5 / 12.0, 0.5 / 12.0);
        let (carrier, _) = led_module_shading(1.0 / 12.0, 1.0 / 12.0);
        assert!(dot > 0.8, "emitter centre must glow ({dot})");
        assert!(carrier < 0.15, "carrier must stay dark ({carrier})");
    }

    #[test]
    fn the_beam_dissolves_from_lamp_to_foot_and_tiles_seamlessly() {
        use super::beam_shading;
        // Densest just below the lens, thinner halfway, gone at the
        // foot — that falloff IS the volumetric read.
        let near = beam_shading(0.25, 0.1).1;
        let mid = beam_shading(0.25, 0.5).1;
        let foot = beam_shading(0.25, 0.999).1;
        assert!(near > mid && mid > foot, "{near} > {mid} > {foot}");
        assert!(foot < 0.01, "the foot must dissolve, got {foot}");
        // The tip fades in instead of starting as a hard edge.
        assert!(beam_shading(0.25, 0.0).1 < 0.01);
        // The striations close around the shaft: u=0 and u=1 are the
        // same line, or the cone shows a seam.
        let (a, _) = beam_shading(0.0, 0.4);
        let (b, _) = beam_shading(1.0, 0.4);
        assert!((a - b).abs() < 1e-4, "seam: {a} vs {b}");
    }

    #[test]
    fn the_hype_glass_is_a_frame_and_not_a_lid() {
        use super::hype_glass_shading;
        // THE defect this pins, found by looking at it: the first
        // version was a filled capsule, and it painted over the very
        // charge it was supposed to contain.
        let (_, middle) = hype_glass_shading(0.5, 0.5);
        assert!(middle < 0.2, "the middle must be see-through, not {middle}");
        // The rim carries the shape. It is a THIN band — about a
        // pixel and a half at the size this is drawn — so the sample
        // has to sit inside it rather than in the soft outer edge.
        for u in [0.035, 0.965] {
            let (value, alpha) = hype_glass_shading(u, 0.5);
            assert!(alpha > 0.7, "the rim is solid at {u}, not {alpha}");
            assert!(value > 0.6, "and bright at {u}, not {value}");
        }
        // Outside the capsule there is nothing at all: the caps are
        // round, so a corner of the tile is empty.
        assert_eq!(hype_glass_shading(0.02, 0.001).1, 0.0, "outside the cap");
        // The specular sits on the left shoulder, and is brighter up
        // top than down at the bottom.
        let (top, _) = hype_glass_shading(0.275, 0.2);
        let (bottom, _) = hype_glass_shading(0.275, 0.85);
        assert!(top > bottom, "the light comes from above");
    }

    #[test]
    fn the_hype_fill_is_a_lit_column() {
        use super::hype_fill_shading;
        let (core, core_a) = hype_fill_shading(0.5, 0.5);
        let (flank, _) = hype_fill_shading(0.14, 0.5);
        assert!(core > flank, "a core brighter than its flanks");
        assert!(core > 0.9 && flank < 0.75, "and enough of a range to read");
        assert!(core_a > 0.9, "solid down the middle");
        // Shaped across the width only: scaling it vertically must
        // not change what it looks like.
        assert_eq!(hype_fill_shading(0.5, 0.05), hype_fill_shading(0.5, 0.95));
    }

    #[test]
    fn the_gauge_arc_is_a_band_with_its_strongest_tick_at_the_top() {
        use super::gauge_arc_shading;
        // On the band at the top (u=0.5 near v small): visible.
        let (top_value, top_alpha) = gauge_arc_shading(0.5, 1.0 - 0.79);
        assert!(top_alpha > 0.5, "the band must be visible ({top_alpha})");
        // The activation tick at the top is the strongest mark.
        let (side_value, _) = gauge_arc_shading(0.5 + 0.35, 1.0 - 0.63);
        assert!(
            top_value > side_value,
            "the halfway tick ({top_value}) must outshine the plain band ({side_value})"
        );
        assert!(
            top_value >= 0.99,
            "the activation tick is the dial's BRIGHTEST mark ({top_value})"
        );
        // Outside the ring: nothing.
        assert_eq!(gauge_arc_shading(0.5, 0.9).1, 0.0, "inside the hub");
        assert_eq!(gauge_arc_shading(0.5, 0.005).1, 0.0, "beyond the rim");
        // The horizon endpoints carry the 0%/100% ticks — brighter
        // than the plain band, like the activation mark.
        let (end_value, end_alpha) = gauge_arc_shading(0.1, 1.0);
        assert!(end_alpha > 0.5 && end_value > side_value);
    }
    use super::{SHAPE_SIZE, shape_mask};

    fn count(mask: &[[bool; SHAPE_SIZE]; SHAPE_SIZE]) -> usize {
        mask.iter().flatten().filter(|c| **c).count()
    }

    #[test]
    fn every_lane_shape_is_substantial() {
        for lane in 0..5 {
            let filled = count(&shape_mask(lane));
            assert!(
                (40..=220).contains(&filled),
                "lane {lane}: {filled} pixels — too sparse or a filled block"
            );
        }
    }

    /// The whole point: shapes must be tellable apart WITHOUT color.
    /// Any two lanes must differ in a meaningful fraction of pixels.
    #[test]
    fn shapes_are_pairwise_distinct() {
        for a in 0..5 {
            for b in (a + 1)..5 {
                let (ma, mb) = (shape_mask(a), shape_mask(b));
                let differing = ma
                    .iter()
                    .flatten()
                    .zip(mb.iter().flatten())
                    .filter(|(x, y)| x != y)
                    .count();
                assert!(
                    differing >= 30,
                    "lanes {a} and {b} differ in only {differing} pixels"
                );
            }
        }
    }

    /// Left-right symmetry keeps every shape readable in any lane
    /// position (nothing points sideways).
    #[test]
    fn shapes_are_horizontally_symmetric() {
        for lane in 0..5 {
            let mask = shape_mask(lane);
            for row in &mask {
                for x in 0..SHAPE_SIZE / 2 {
                    assert_eq!(
                        row[x],
                        row[SHAPE_SIZE - 1 - x],
                        "lane {lane} is not mirror-symmetric"
                    );
                }
            }
        }
    }
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
