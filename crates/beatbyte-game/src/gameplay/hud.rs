//! Per-player HUD blocks in world space, anchored above each highway.
//!
//! World-space text follows the highway layout for any player count —
//! the same code serves solo and four-player splits.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, player_color};
use crate::palette;
use crate::ui::UiFont;

/// Score line marker.
#[derive(Component)]
pub struct ScoreText(pub usize);
/// Combo line marker.
#[derive(Component)]
pub struct ComboText(pub usize);
/// Multiplier marker.
#[derive(Component)]
pub struct MultiplierText(pub usize);
/// The Hype meter fill sprite.
#[derive(Component)]
pub struct HypeFill(pub usize);

/// Vertical anchor of the HUD block above each highway.
const HUD_TOP: f32 = 330.0;

/// Spawn one HUD block per player.
pub fn spawn_huds(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    font: Res<UiFont>,
) {
    let compact = layout.players() > 2;
    let score_size = if compact { 14.0 } else { 22.0 };
    let line_size = if compact { 9.0 } else { 12.0 };
    for index in players.iter() {
        let player = index.0;
        let origin = layout.origin(player);
        commands.spawn((
            GameplayScreen,
            ScoreText(player),
            Text2d::new("0"),
            font.text(score_size),
            TextColor(player_color(player)),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP, 5.0),
        ));
        commands.spawn((
            GameplayScreen,
            MultiplierText(player),
            Text2d::new("x1"),
            font.text(line_size),
            TextColor(palette::TEXT),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP - score_size * 1.6, 5.0),
        ));
        commands.spawn((
            GameplayScreen,
            ComboText(player),
            Text2d::new(""),
            font.text(line_size),
            TextColor(palette::TEXT_DIM),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP - score_size * 1.6 - line_size * 1.8, 5.0),
        ));
        // Hype meter: frame + left-anchored fill.
        let bar_width = layout.bed_width() * 0.6;
        let bar_y = HUD_TOP - score_size * 1.6 - line_size * 3.9;
        commands.spawn((
            GameplayScreen,
            Sprite::from_color(
                palette::dimmed(palette::HYPE, 0.25),
                Vec2::new(bar_width, 6.0),
            ),
            Transform::from_xyz(origin, bar_y, 4.0),
        ));
        commands.spawn((
            GameplayScreen,
            HypeFill(player),
            Sprite::from_color(palette::HYPE, Vec2::new(bar_width, 6.0)),
            Anchor::CENTER_LEFT,
            Transform::from_xyz(origin - bar_width / 2.0, bar_y, 5.0)
                .with_scale(Vec3::new(0.0, 1.0, 1.0)),
        ));
    }
}

/// Push session numbers into every player's HUD.
#[allow(clippy::type_complexity)]
pub fn update_huds(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut texts: ParamSet<(
        Query<(&ScoreText, &mut Text2d)>,
        Query<(&ComboText, &mut Text2d)>,
        Query<(&MultiplierText, &mut Text2d, &mut TextColor)>,
    )>,
    mut fills: Query<(&HypeFill, &mut Transform)>,
) {
    for (index, player) in &players {
        let perf = player.session.performance();

        for (marker, mut text) in &mut texts.p0() {
            if marker.0 == index.0 {
                let score = perf.score().to_string();
                if text.0 != score {
                    text.0 = score;
                }
            }
        }
        for (marker, mut text) in &mut texts.p1() {
            if marker.0 == index.0 {
                let combo = if perf.streak() >= 4 {
                    format!("{} combo", perf.streak())
                } else {
                    String::new()
                };
                if text.0 != combo {
                    text.0 = combo;
                }
            }
        }
        for (marker, mut text, mut color) in &mut texts.p2() {
            if marker.0 == index.0 {
                let hype = perf.hype_active();
                let line = format!("x{}{}", perf.multiplier(), if hype { " HYPE" } else { "" });
                if text.0 != line {
                    text.0 = line;
                }
                color.0 = if hype {
                    palette::HYPE
                } else if perf.multiplier() >= 4 {
                    palette::BRAND
                } else {
                    palette::TEXT
                };
            }
        }
        for (fill, mut transform) in &mut fills {
            if fill.0 == index.0 {
                transform.scale.x = perf.hype_meter() as f32;
            }
        }
    }
}
