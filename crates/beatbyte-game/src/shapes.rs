//! Per-lane note shapes — color is never the only channel.
//!
//! Every lane has a distinct 16×16 pixel-art shape (square, circle,
//! diamond, triangle, cross), used for note gems and receptors alike.
//! This is always on: a colorblind player must not have to find an
//! option before the game is playable, and distinct gems suit the
//! 8-bit identity anyway. The masks are generated, not drawn — no
//! assets, and the geometry is unit-tested.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use beatbyte_core::Lane;

/// Shape mask side length in pixels.
pub const SHAPE_SIZE: usize = 16;

/// The generated shape images, indexed by lane.
#[derive(Resource)]
pub struct LaneShapes([Handle<Image>; 5]);

impl LaneShapes {
    /// The shape image for a lane.
    #[must_use]
    pub fn image(&self, lane: Lane) -> Handle<Image> {
        self.0[lane as usize].clone()
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
    let array: [Handle<Image>; 5] = match handles.try_into() {
        Ok(array) => array,
        Err(_) => unreachable!("exactly five shapes are generated"),
    };
    commands.insert_resource(LaneShapes(array));
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
