//! Immediate hit/miss feedback: receptor flashes and judgment popups.
//!
//! Milestone 5 keeps this minimal-but-alive; the full game-feel pass
//! (particles, shake, lighting) is Milestone 6.

use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;

use super::{GameplayScreen, PlayerSession, RECEPTOR_Y, SessionFeedback, lane_x};
use crate::palette;

/// A short-lived visual effect.
#[derive(Component)]
pub struct Effect {
    /// Seconds lived so far.
    pub age: f32,
    /// Total lifetime in seconds.
    pub lifetime: f32,
    /// Scale growth per second.
    pub growth: f32,
}

/// The floating judgment popup (a single reused entity).
#[derive(Component)]
pub struct JudgmentPopup {
    /// Seconds left before it fades out.
    pub ttl: f32,
}

/// Turn this frame's session events into visual effects.
pub fn spawn_feedback(
    mut commands: Commands,
    players: Query<&PlayerSession>,
    mut feedback: MessageReader<SessionFeedback>,
    mut popup: Query<(&mut Text, &mut TextColor, &mut JudgmentPopup)>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    for message in feedback.read() {
        match message.event {
            SessionEvent::NoteHit {
                event_index,
                judgment,
                ..
            } => {
                let lanes = player.session.track().events()[event_index].lanes;
                let (label, color) = judgment_style(judgment);
                for lane in lanes.iter() {
                    commands.spawn((
                        GameplayScreen,
                        Effect {
                            age: 0.0,
                            lifetime: 0.18,
                            growth: 4.0,
                        },
                        Sprite::from_color(color, Vec2::new(40.0, 40.0)),
                        Transform::from_xyz(lane_x(lane), RECEPTOR_Y, 5.0),
                    ));
                }
                show_popup(&mut popup, &mut commands, label, color);
            }
            SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum => {
                show_popup(&mut popup, &mut commands, "MISS", palette::MISS);
            }
            SessionEvent::HypeActivated => {
                show_popup(&mut popup, &mut commands, "HYPE!", palette::HYPE);
            }
            _ => {}
        }
    }
}

fn judgment_style(judgment: Judgment) -> (&'static str, Color) {
    match judgment {
        Judgment::Perfect => ("PERFECT", palette::PERFECT),
        Judgment::Great => ("GREAT", palette::GREAT),
        Judgment::Good => ("GOOD", palette::GOOD),
        Judgment::Miss => ("MISS", palette::MISS),
    }
}

/// Show (or refresh) the single judgment popup.
fn show_popup(
    popup: &mut Query<(&mut Text, &mut TextColor, &mut JudgmentPopup)>,
    commands: &mut Commands,
    label: &str,
    color: Color,
) {
    if let Ok((mut text, mut text_color, mut state)) = popup.single_mut() {
        text.0 = label.to_owned();
        text_color.0 = color;
        state.ttl = 0.5;
    } else {
        commands.spawn((
            GameplayScreen,
            JudgmentPopup { ttl: 0.5 },
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(34.0),
                ..default()
            },
            TextColor(color),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(210),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            TextLayout::justify(Justify::Center),
        ));
    }
}

/// Age and despawn effects; fade the popup.
pub fn animate_feedback(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &mut Effect, &mut Transform, &mut Sprite)>,
    mut popup: Query<(&mut JudgmentPopup, &mut TextColor)>,
) {
    let dt = time.delta_secs();
    for (entity, mut effect, mut transform, mut sprite) in &mut effects {
        effect.age += dt;
        if effect.age >= effect.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = effect.age / effect.lifetime;
        let scale = 1.0 + effect.growth * progress;
        transform.scale = Vec3::splat(scale);
        sprite.color = sprite.color.with_alpha(1.0 - progress);
    }
    if let Ok((mut state, mut color)) = popup.single_mut() {
        state.ttl -= dt;
        let alpha = (state.ttl / 0.2).clamp(0.0, 1.0);
        color.0 = color.0.with_alpha(alpha);
    }
}
