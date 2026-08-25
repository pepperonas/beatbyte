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
}

impl LaneShapes {
    /// The 8-bit shape image for a lane.
    #[must_use]
    pub fn image(&self, lane: Lane) -> Handle<Image> {
        self.per_lane[lane as usize].clone()
    }

    /// The gem body for a lane in the chosen style.
    #[must_use]
    pub fn body(&self, lane: Lane, round: bool) -> Handle<Image> {
        if round {
            self.round_body.clone()
        } else {
            self.image(lane)
        }
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
    });
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
