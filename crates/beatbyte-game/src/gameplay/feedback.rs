//! Immediate hit/miss feedback: receptor flashes and per-player
//! judgment popups in world space.

use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::{GameplayScreen, HighwayLayout, RECEPTOR_Y, SessionFeedback};
use crate::palette;
use crate::ui::UiFont;

/// A player's floating judgment popup (one reused entity each).
#[derive(Component)]
pub struct JudgmentPopup {
    /// The player it belongs to.
    pub player: usize,
    /// Seconds left before it fades out.
    pub ttl: f32,
}

/// Turn this frame's session events into flashes and popups.
pub fn spawn_feedback(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    mut feedback: MessageReader<SessionFeedback>,
    mut popups: Query<(&mut JudgmentPopup, &mut Text2d, &mut TextColor)>,
    font: Res<UiFont>,
) {
    for message in feedback.read() {
        let player = message.player_index;
        match message.event {
            SessionEvent::NoteHit { judgment, .. } => {
                let (label, color) = judgment_style(judgment);
                show_popup(
                    &mut popups,
                    &mut commands,
                    &layout,
                    &font,
                    player,
                    label,
                    color,
                );
            }
            SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum => {
                show_popup(
                    &mut popups,
                    &mut commands,
                    &layout,
                    &font,
                    player,
                    "MISS",
                    palette::MISS,
                );
            }
            SessionEvent::HypeActivated => {
                show_popup(
                    &mut popups,
                    &mut commands,
                    &layout,
                    &font,
                    player,
                    "HYPE!",
                    palette::HYPE,
                );
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

/// Show (or refresh) a player's judgment popup.
#[allow(clippy::too_many_arguments)]
fn show_popup(
    popups: &mut Query<(&mut JudgmentPopup, &mut Text2d, &mut TextColor)>,
    commands: &mut Commands,
    layout: &HighwayLayout,
    font: &UiFont,
    player: usize,
    label: &str,
    color: Color,
) {
    for (mut popup, mut text, mut text_color) in popups.iter_mut() {
        if popup.player == player {
            text.0 = label.to_owned();
            text_color.0 = color;
            popup.ttl = 0.5;
            return;
        }
    }
    let size = if layout.players() > 2 { 12.0 } else { 18.0 };
    commands.spawn((
        GameplayScreen,
        JudgmentPopup { player, ttl: 0.5 },
        Text2d::new(label),
        font.text(size),
        TextColor(color),
        Anchor::CENTER,
        Transform::from_xyz(layout.origin(player), RECEPTOR_Y + 130.0, 6.0),
    ));
}

/// Fade the popups (particle lifetimes live in `fx`).
pub fn animate_feedback(time: Res<Time>, mut popups: Query<(&mut JudgmentPopup, &mut TextColor)>) {
    let dt = time.delta_secs();
    for (mut popup, mut color) in &mut popups {
        popup.ttl -= dt;
        let alpha = (popup.ttl / 0.2).clamp(0.0, 1.0);
        color.0 = color.0.with_alpha(alpha);
    }
}
