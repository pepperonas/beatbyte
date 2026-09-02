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
    settings: Res<crate::config::Settings>,
    mut feedback: MessageReader<SessionFeedback>,
    mut popups: Query<(&mut JudgmentPopup, &mut Text2d, &mut TextColor)>,
    font: Res<UiFont>,
) {
    // The lower, smaller placement belongs to the instrument neck
    // (3D stage, round style). The 8-bit stage keeps its word where
    // it always was — that mode is untouched by the round-six work.
    let placement = label_placement(settings.stage_3d && settings.round_gems, layout.players());
    for message in feedback.read() {
        let player = message.player_index;
        if !shows_label(settings.hit_labels, &message.event) {
            continue;
        }
        match message.event {
            SessionEvent::NoteHit {
                judgment, offset_s, ..
            } => {
                let (label, color) = judgment_style(judgment);
                let tagged = match timing_tag(judgment, offset_s) {
                    Some(tag) => format!("{label} ({tag})"),
                    None => label.to_owned(),
                };
                show_popup(
                    &mut popups,
                    &mut commands,
                    &layout,
                    &font,
                    placement,
                    player,
                    &tagged,
                    color,
                );
            }
            SessionEvent::NoteMissed { .. } | SessionEvent::Overstrum => {
                show_popup(
                    &mut popups,
                    &mut commands,
                    &layout,
                    &font,
                    placement,
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
                    placement,
                    player,
                    "HYPE!",
                    palette::HYPE,
                );
            }
            _ => {}
        }
    }
}

/// Whether an event gets a word over the neck.
///
/// With hit labels off, per-note grades and misses are silent — the
/// flame and the button say it, as in the classic guitar games —
/// but HYPE! still announces itself: it is a state change, not a
/// grade, and the genre shouts those too. Pure — tested.
#[must_use]
pub fn shows_label(hit_labels: bool, event: &SessionEvent) -> bool {
    match event {
        SessionEvent::NoteHit { .. }
        | SessionEvent::NoteMissed { .. }
        | SessionEvent::Overstrum => hit_labels,
        _ => true,
    }
}

/// Where the word sits and how big it is: `(font size, height above
/// the receptors)`.
///
/// On the instrument neck the old spot — 130 px up at 18 px — landed
/// in the middle of the neck, where the notes are; it now sits lower
/// and smaller, beside the strike rather than over the approach. Four
/// necks are cramped in every view. Pure — tested.
#[must_use]
pub fn label_placement(instrument_neck: bool, players: usize) -> (f32, f32) {
    match (instrument_neck, players > 2) {
        (_, true) => (12.0, 130.0),
        (true, false) => (14.0, 92.0),
        (false, false) => (18.0, 130.0),
    }
}

/// Which side of the note a non-perfect hit landed on. A PERFECT is
/// inside the tight window and needs no lecture; a miss has no
/// meaningful side — the tag exists exactly where it is actionable
/// (optimization plan P2: the popup already knows the signed
/// offset, showing it is the most actionable feedback there is).
/// Negative offset = the hit came before the note = EARLY.
fn timing_tag(judgment: Judgment, offset_s: f64) -> Option<&'static str> {
    match judgment {
        Judgment::Great | Judgment::Good => Some(if offset_s < 0.0 { "EARLY" } else { "LATE" }),
        Judgment::Perfect | Judgment::Miss => None,
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
    placement: (f32, f32),
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
    let (size, lift) = placement;
    commands.spawn((
        GameplayScreen,
        JudgmentPopup { player, ttl: 0.5 },
        Text2d::new(label),
        font.text(size),
        TextColor(color),
        Anchor::CENTER,
        Transform::from_xyz(layout.origin(player), RECEPTOR_Y + lift, 6.0),
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

#[cfg(test)]
mod tests {
    #[test]
    fn hit_labels_off_silences_grades_but_not_hype() {
        use beatbyte_core::SessionEvent;
        let hit = SessionEvent::NoteHit {
            event_index: 0,
            judgment: beatbyte_core::Judgment::Perfect,
            offset_s: 0.0,
        };
        let miss = SessionEvent::NoteMissed { event_index: 0 };
        assert!(super::shows_label(true, &hit));
        assert!(super::shows_label(true, &miss));
        assert!(!super::shows_label(false, &hit), "a grade is silenced");
        assert!(!super::shows_label(false, &miss), "so is a miss");
        assert!(!super::shows_label(false, &SessionEvent::Overstrum));
        // A state change is not a grade: it still announces itself.
        assert!(super::shows_label(false, &SessionEvent::HypeActivated));
    }

    #[test]
    fn the_word_sits_lower_and_smaller_on_the_instrument_neck() {
        // On the instrument neck the old spot was the middle of the
        // neck — where the notes are. Four necks are cramped in any
        // view. The caller gates this on stage_3d AND round_gems, so
        // the 8-bit stage keeps the old spot.
        let (size_3d, lift_3d) = super::label_placement(true, 1);
        let (size_2d, lift_2d) = super::label_placement(false, 1);
        assert!(size_3d < size_2d);
        assert!(lift_3d < lift_2d);
        assert_eq!(
            super::label_placement(true, 4),
            super::label_placement(false, 4)
        );
        assert!(super::label_placement(true, 4).0 < size_3d);
    }

    use super::timing_tag;
    use beatbyte_core::Judgment;

    #[test]
    fn only_the_judgments_that_can_act_on_it_get_a_side() {
        // Negative offset = hit before the note = EARLY.
        assert_eq!(timing_tag(Judgment::Great, -0.04), Some("EARLY"));
        assert_eq!(timing_tag(Judgment::Good, 0.07), Some("LATE"));
        assert_eq!(timing_tag(Judgment::Perfect, -0.01), None);
        assert_eq!(timing_tag(Judgment::Miss, 0.2), None);
    }
}
