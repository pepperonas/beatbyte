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

// ── Solo corner panels ──────────────────────────────────────────────
//
// Solo play puts the readouts in framed plates in the bottom corners,
// the way the arcade-era games did: score and multiplier bottom-left,
// the meter bottom-right, the highway left alone in between.
//
// The old layout stacked everything above the highway, which the depth
// view could carry but the 3D stage could not: there the neck runs to
// a vanishing point, so "above the highway" is the middle of the
// screen, and the numbers floated in empty space over the horizon.
//
// The ortho projection is `AutoMin{1280, 720}`, so world coordinates
// within ±640 × ±360 are on screen at every window size — the corners
// are reachable in world space and need no screen-space layer.
//
// Multiplayer keeps the per-highway blocks: with two to four necks
// side by side there are no free corners, and a score has to sit above
// the highway it belongs to.

/// Half-width of a corner plate.
const PLATE_W: f32 = 268.0;
/// Half-height of a corner plate.
const PLATE_H: f32 = 122.0;
/// Distance from the viewport edge to a plate.
const PLATE_INSET: f32 = 18.0;
/// Thickness of a plate's border.
const PLATE_BORDER: f32 = 2.0;

/// Spawn one HUD block per player.
pub fn spawn_huds(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    font: Res<UiFont>,
    settings: Res<crate::config::Settings>,
) {
    // Quiet corner badge: which input mode this song runs in — one
    // glance answers "why did/didn't that hit?" while testing tap
    // vs. strum on keyboard or guitar (user request).
    let (badge, color) = if settings.tap_mode {
        ("< TAP >", palette::dimmed(palette::TEXT_DIM, 0.8))
    } else {
        ("< STRUM >", palette::dimmed(palette::HYPE, 0.8))
    };
    commands.spawn((
        GameplayScreen,
        Text2d::new(badge),
        font.text(9.0),
        TextColor(color),
        Anchor::TOP_LEFT,
        // Top-left: the bottom-left corner now belongs to the score
        // plate, and two readouts in one corner read as one crowded
        // block rather than two facts.
        Transform::from_xyz(-624.0, 348.0, 5.0),
    ));
    if layout.players() == 1 {
        spawn_solo_panels(&mut commands, &font);
        return;
    }
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

/// A framed plate: a border rectangle with a darker fill on top.
///
/// Bevy sprites have no border, so a plate is two rectangles. The fill
/// is drawn slightly in front of the border so the border shows as a
/// hairline edge rather than being covered.
fn plate(commands: &mut Commands, center: Vec2, size: Vec2, accent: Color) {
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(palette::dimmed(accent, 0.55), size),
        Transform::from_xyz(center.x, center.y, 3.0),
    ));
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(
            palette::BACKGROUND.with_alpha(0.88),
            size - Vec2::splat(PLATE_BORDER * 2.0),
        ),
        Transform::from_xyz(center.x, center.y, 3.1),
    ));
}

/// A small caption above a readout.
fn caption(commands: &mut Commands, font: &UiFont, text: &str, at: Vec2) {
    commands.spawn((
        GameplayScreen,
        Text2d::new(text.to_owned()),
        font.text(8.0),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
        Anchor::TOP_CENTER,
        Transform::from_xyz(at.x, at.y, 5.0),
    ));
}

/// The solo layout: score and multiplier bottom-left, meter
/// bottom-right, nothing over the highway.
fn spawn_solo_panels(commands: &mut Commands, font: &UiFont) {
    let accent = player_color(0);
    let left = Vec2::new(
        -640.0 + PLATE_INSET + PLATE_W / 2.0,
        -360.0 + PLATE_INSET + PLATE_H / 2.0,
    );
    let right = Vec2::new(-left.x, left.y);
    let size = Vec2::new(PLATE_W, PLATE_H);

    // ── Left: score, multiplier, combo ──────────────────────────────
    plate(commands, left, size, accent);
    caption(
        commands,
        font,
        "SCORE",
        left + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    commands.spawn((
        GameplayScreen,
        ScoreText(0),
        Text2d::new("0"),
        font.text(26.0),
        TextColor(accent),
        Anchor::TOP_CENTER,
        Transform::from_xyz(left.x, left.y + PLATE_H / 2.0 - 30.0, 5.0),
    ));
    commands.spawn((
        GameplayScreen,
        MultiplierText(0),
        Text2d::new("x1"),
        font.text(16.0),
        TextColor(palette::TEXT),
        Anchor::TOP_CENTER,
        Transform::from_xyz(left.x, left.y - PLATE_H / 2.0 + 46.0, 5.0),
    ));
    commands.spawn((
        GameplayScreen,
        ComboText(0),
        Text2d::new(""),
        font.text(9.0),
        TextColor(palette::TEXT_DIM),
        Anchor::TOP_CENTER,
        Transform::from_xyz(left.x, left.y - PLATE_H / 2.0 + 22.0, 5.0),
    ));

    // ── Right: the hype meter ───────────────────────────────────────
    plate(commands, right, size, palette::HYPE);
    caption(
        commands,
        font,
        "HYPE",
        right + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    let bar = Vec2::new(PLATE_W - 56.0, 22.0);
    let bar_y = right.y + 4.0;
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(palette::dimmed(palette::HYPE, 0.22), bar),
        Transform::from_xyz(right.x, bar_y, 4.0),
    ));
    commands.spawn((
        GameplayScreen,
        HypeFill(0),
        Sprite::from_color(palette::HYPE, bar),
        Anchor::CENTER_LEFT,
        Transform::from_xyz(right.x - bar.x / 2.0, bar_y, 5.0).with_scale(Vec3::new(0.0, 1.0, 1.0)),
    ));
    caption(
        commands,
        font,
        "FILL IT, THEN HIT HYPE",
        right + Vec2::new(0.0, -PLATE_H / 2.0 + 28.0),
    );
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
