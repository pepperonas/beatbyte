//! The gameplay HUD: score, combo, multiplier, accuracy, Hype meter.

use bevy::prelude::*;

use super::{GameplayScreen, PlayerSession};
use crate::palette;
use crate::ui::UiFont;

/// Marker components for the HUD's dynamic text fields.
#[derive(Component)]
pub struct ScoreText;
/// Combo counter marker.
#[derive(Component)]
pub struct ComboText;
/// Multiplier marker.
#[derive(Component)]
pub struct MultiplierText;
/// Accuracy marker.
#[derive(Component)]
pub struct AccuracyText;
/// The Hype meter fill bar.
#[derive(Component)]
pub struct HypeBar;
/// Hype hint text under the bar.
#[derive(Component)]
pub struct HypeHint;

/// Spawn the HUD.
pub fn spawn_hud(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            GameplayScreen,
            Node {
                position_type: PositionType::Absolute,
                top: px(18),
                left: px(22),
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                ScoreText,
                Text::new("0"),
                font.text(26.0),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                AccuracyText,
                Text::new("100.0%"),
                font.text(12.0),
                TextColor(palette::TEXT_DIM),
            ));
        });

    commands
        .spawn((
            GameplayScreen,
            Node {
                position_type: PositionType::Absolute,
                top: px(18),
                right: px(22),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(2),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                MultiplierText,
                Text::new("x1"),
                font.text(22.0),
                TextColor(palette::TEXT),
            ));
            parent.spawn((
                ComboText,
                Text::new(""),
                font.text(12.0),
                TextColor(palette::TEXT_DIM),
            ));
            // Hype meter: outline + fill.
            parent
                .spawn((
                    Node {
                        width: px(180),
                        height: px(14),
                        margin: UiRect::top(px(8)),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BorderColor::all(palette::dimmed(palette::HYPE, 0.6)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        HypeBar,
                        Node {
                            width: percent(0),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(palette::HYPE),
                    ));
                });
            parent.spawn((
                HypeHint,
                Text::new(""),
                font.text(10.0),
                TextColor(palette::HYPE),
            ));
        });
}

/// Push session numbers into the HUD.
#[allow(clippy::type_complexity)]
pub fn update_hud(
    players: Query<&PlayerSession>,
    mut texts: ParamSet<(
        Query<&mut Text, With<ScoreText>>,
        Query<&mut Text, With<ComboText>>,
        Query<(&mut Text, &mut TextColor), With<MultiplierText>>,
        Query<&mut Text, With<AccuracyText>>,
        Query<&mut Text, With<HypeHint>>,
    )>,
    mut hype_bar: Query<&mut Node, With<HypeBar>>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let perf = player.session.performance();

    if let Ok(mut text) = texts.p0().single_mut() {
        let score = perf.score().to_string();
        if text.0 != score {
            text.0 = score;
        }
    }
    if let Ok(mut text) = texts.p1().single_mut() {
        let combo = if perf.streak() >= 4 {
            format!("{} combo", perf.streak())
        } else {
            String::new()
        };
        if text.0 != combo {
            text.0 = combo;
        }
    }
    if let Ok((mut text, mut color)) = texts.p2().single_mut() {
        let hype = perf.hype_active();
        let multiplier = format!("x{}{}", perf.multiplier(), if hype { " HYPE" } else { "" });
        if text.0 != multiplier {
            text.0 = multiplier;
        }
        color.0 = if hype {
            palette::HYPE
        } else if perf.multiplier() >= 4 {
            palette::BRAND
        } else {
            palette::TEXT
        };
    }
    if let Ok(mut text) = texts.p3().single_mut() {
        let accuracy = format!("{:.1}%", perf.accuracy() * 100.0);
        if text.0 != accuracy {
            text.0 = accuracy;
        }
    }
    if let Ok(mut text) = texts.p4().single_mut() {
        let hint = if perf.hype_active() {
            "HYPE!"
        } else if perf.hype_meter() >= perf.config().hype_activation_threshold {
            "SPACE!"
        } else {
            ""
        };
        if text.0 != hint {
            text.0 = hint.to_owned();
        }
    }
    if let Ok(mut node) = hype_bar.single_mut() {
        node.width = percent((perf.hype_meter() * 100.0) as f32);
    }
}
