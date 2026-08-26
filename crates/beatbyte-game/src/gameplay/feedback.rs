//! Immediate hit/miss feedback: receptor flashes and per-player
//! judgment popups in world space.

use beatbyte_core::{Judgment, SessionEvent};
use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::{GameplayScreen, HighwayLayout, RECEPTOR_Y, SessionFeedback};
use crate::palette;
use crate::ui::UiFont;

/// The "you must strum!" coach line: appears when tap mode is OFF
/// and a note dies while its fret is correctly HELD — the exact
/// moment a player who does not know about strumming sits confused
/// (field find: "Töne werden nicht erkannt"). Fades after a moment;
/// rate-limited so it teaches instead of nagging.
#[derive(Component)]
pub struct StrumCoach {
    /// Seconds left visible.
    pub ttl: f32,
}

/// Show the coach when a held-fret note is missed without tap mode.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn coach_strum(
    mut commands: Commands,
    settings: Res<crate::config::Settings>,
    mut feedback: MessageReader<SessionFeedback>,
    players: Query<(&super::PlayerIndex, &super::PlayerSession)>,
    mut coaches: Query<(&mut StrumCoach, &mut TextColor)>,
    layout: Res<HighwayLayout>,
    font: Res<UiFont>,
    time: Res<Time>,
    mut cooldown: Local<f32>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    // Fade running coaches.
    for (mut coach, mut color) in &mut coaches {
        coach.ttl -= time.delta_secs();
        color.0 = color.0.with_alpha(coach.ttl.clamp(0.0, 1.0));
    }
    if settings.tap_mode {
        return;
    }
    for message in feedback.read() {
        let SessionEvent::NoteMissed { event_index } = message.event else {
            continue;
        };
        if *cooldown > 0.0 {
            continue;
        }
        let Some((index, player)) = players
            .iter()
            .find(|(index, _)| index.0 == message.player_index)
        else {
            continue;
        };
        let Some(event) = player.session.track().events().get(event_index).copied() else {
            continue;
        };
        // Only coach when the fret WAS held — that is the "why did
        // it not count" moment; a plain miss needs no lecture.
        let held = player.session.held();
        if !event.lanes.iter().any(|lane| held.contains(lane)) {
            continue;
        }
        *cooldown = 6.0;
        if let Some((mut coach, mut color)) = coaches.iter_mut().next() {
            coach.ttl = 2.5;
            color.0 = color.0.with_alpha(1.0);
        } else {
            commands.spawn((
                GameplayScreen,
                StrumCoach { ttl: 2.5 },
                Text2d::new("STRUM! (SPACE or strum bar)"),
                font.text(15.0),
                TextColor(palette::MISS),
                bevy::sprite::Anchor::CENTER,
                Transform::from_xyz(layout.origin(index.0), RECEPTOR_Y + 150.0, 8.0),
            ));
        }
    }
}

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
