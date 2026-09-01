//! The BeatByte color language.
//!
//! One place for every color the game uses — themes (Milestone 10)
//! will replace this with data-driven palettes, so gameplay code only
//! ever asks *semantic* questions ("the color of lane 3", "the miss
//! color") and never invents hex values.

use beatbyte_core::Lane;
use bevy::prelude::*;

/// Deep-space navy: the base clear color.
pub const BACKGROUND: Color = Color::srgb(0.043, 0.043, 0.086);

/// Slightly lifted panel tone (highway bed, HUD panels).
pub const SURFACE: Color = Color::srgb(0.075, 0.078, 0.13);

/// Brand yellow (title, score).
pub const BRAND: Color = Color::srgb(1.0, 0.85, 0.25);

/// Muted UI text.
pub const TEXT_DIM: Color = Color::srgb(0.55, 0.6, 0.75);

/// Bright UI text.
pub const TEXT: Color = Color::srgb(0.92, 0.93, 0.97);

/// Judgment colors.
pub const PERFECT: Color = Color::srgb(0.45, 1.0, 0.85);
/// Great judgment color.
pub const GREAT: Color = Color::srgb(0.55, 0.85, 1.0);
/// Good judgment color.
pub const GOOD: Color = Color::srgb(0.95, 0.85, 0.45);
/// Miss/danger color.
pub const MISS: Color = Color::srgb(1.0, 0.35, 0.4);

/// Hype meter / active hype tint.
pub const HYPE: Color = Color::srgb(0.75, 0.5, 1.0);

/// The five lane colors, left to right (classic mapping, original hues).
pub const LANES: [Color; 5] = [
    Color::srgb(0.24, 0.86, 0.52), // green
    Color::srgb(1.0, 0.32, 0.32),  // red
    Color::srgb(1.0, 0.84, 0.25),  // yellow
    Color::srgb(0.25, 0.77, 1.0),  // blue
    Color::srgb(1.0, 0.67, 0.25),  // orange
];

/// The color of a lane.
#[must_use]
pub fn lane_color(lane: Lane) -> Color {
    LANES[lane.index()]
}

/// A dimmed variant of a color (for outlines, idle receptors).
#[must_use]
pub fn dimmed(color: Color, factor: f32) -> Color {
    let l = color.to_linear();
    Color::LinearRgba(LinearRgba {
        red: l.red * factor,
        green: l.green * factor,
        blue: l.blue * factor,
        alpha: l.alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear-space brightness of a color's brightest channel.
    fn brightness(color: Color) -> f32 {
        let l = color.to_linear();
        l.red.max(l.green).max(l.blue)
    }

    #[test]
    fn every_lane_has_its_own_color() {
        // The lane color is a gameplay signal; two lanes sharing one
        // would make shape the ONLY difference between them.
        for (i, a) in LANES.iter().enumerate() {
            for b in LANES.iter().skip(i + 1) {
                assert_ne!(a, b, "two lanes share a color");
            }
        }
        for lane in Lane::ALL {
            assert_eq!(lane_color(lane), LANES[lane.index()]);
        }
    }

    #[test]
    fn dimming_darkens_and_keeps_the_alpha() {
        let base = LANES[0].with_alpha(0.7);
        let dim = dimmed(base, 0.5);
        assert!(brightness(dim) < brightness(base), "dimming must darken");
        assert!(
            (dim.alpha() - 0.7).abs() < 1e-6,
            "dimming a color must not fade it - outlines rely on that"
        );
        // Factor 1.0 is the identity, so 'dimmed' can be used
        // unconditionally in color expressions.
        let same = dimmed(base, 1.0);
        assert!((brightness(same) - brightness(base)).abs() < 1e-6);
    }

    #[test]
    fn the_judgment_colors_are_pairwise_distinct() {
        // PERFECT/GREAT/GOOD/MISS are read at a glance mid-song; any
        // two collapsing into one silently weakens the feedback.
        let set = [PERFECT, GREAT, GOOD, MISS];
        for (i, a) in set.iter().enumerate() {
            for b in set.iter().skip(i + 1) {
                assert_ne!(a, b, "two judgments share a color");
            }
        }
    }

    #[test]
    fn text_reads_on_the_background() {
        // The UI's base contrast: bright text on the navy ground.
        assert!(brightness(TEXT) > brightness(BACKGROUND) * 5.0);
        assert!(brightness(TEXT_DIM) > brightness(BACKGROUND) * 2.0);
    }
}
