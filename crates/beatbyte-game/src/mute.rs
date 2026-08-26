//! Global sound mute — one key, one badge, everywhere.
//!
//! Born in the test sessions: harness runs could only be silenced by
//! an env var set BEFORE launch (`BEATBYTE_AUTOPILOT_MUTE`), so
//! whoever watched a run had no way to (un)silence it live. `M` — or
//! clicking the corner badge — now toggles ALL audio at any moment;
//! the env var only sets the starting state. In the editor `M`
//! belongs to the metronome, so only the badge toggles there.

use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;

use crate::config::Settings;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// Whether all sound is currently muted (music + SFX).
#[derive(Resource)]
pub struct Muted(pub bool);

impl Muted {
    /// Volume multiplier for the current state.
    #[must_use]
    pub fn factor(&self) -> f32 {
        if self.0 { 0.0 } else { 1.0 }
    }
}

/// The always-present corner badge showing (and toggling) the state.
#[derive(Component)]
struct MuteBadge;

/// Badge label for a mute state. Pure — tested.
#[must_use]
pub fn mute_label(muted: bool) -> &'static str {
    if muted { "[M] MUTED" } else { "[M] SOUND" }
}

/// The mute plugin: badge + key + volume application.
pub struct MutePlugin;

impl Plugin for MutePlugin {
    fn build(&self, app: &mut App) {
        let start_muted = std::env::var_os("BEATBYTE_AUTOPILOT_MUTE").is_some();
        app.insert_resource(Muted(start_muted))
            .add_systems(Startup, spawn_badge)
            .add_systems(Update, (toggle_mute, apply_mute).chain());
    }
}

fn spawn_badge(mut commands: Commands, font: Res<UiFont>, muted: Res<Muted>) {
    commands.spawn((
        MuteBadge,
        Button,
        Text::new(mute_label(muted.0)),
        font.text(8.0),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            right: px(10),
            bottom: px(8),
            ..default()
        },
        GlobalZIndex(50),
    ));
}

/// `M` (outside the editor — its metronome owns the key) or a badge
/// click flips the state.
fn toggle_mute(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    badges: Query<&Interaction, (With<MuteBadge>, Changed<Interaction>)>,
    mut muted: ResMut<Muted>,
) {
    let key = keys.just_pressed(KeyCode::KeyM) && *state.get() != AppState::Editor;
    let clicked = badges.iter().any(|i| *i == Interaction::Pressed);
    if key || clicked {
        muted.0 = !muted.0;
    }
}

/// Apply on every state change (including the insert frame, which
/// carries the env-var starting state): the music thread gets its
/// scaled volume, Bevy's [`GlobalVolume`] silences every SFX sink,
/// and the badge re-labels.
fn apply_mute(
    muted: Res<Muted>,
    settings: Res<Settings>,
    music: Res<crate::audio_sys::Music>,
    mut global: ResMut<GlobalVolume>,
    mut badges: Query<(&mut Text, &mut TextColor), With<MuteBadge>>,
) {
    if !muted.is_changed() {
        return;
    }
    music.0.set_volume(settings.music_volume * muted.factor());
    global.volume = Volume::Linear(muted.factor());
    for (mut text, mut color) in &mut badges {
        text.0 = mute_label(muted.0).to_owned();
        color.0 = if muted.0 {
            palette::HYPE
        } else {
            palette::dimmed(palette::TEXT_DIM, 0.6)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{Muted, mute_label};

    #[test]
    fn the_factor_silences_or_passes() {
        assert!((Muted(true).factor()).abs() < f32::EPSILON);
        assert!((Muted(false).factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_badge_always_names_its_key() {
        // Both labels must carry the [M] hint — the badge is the only
        // discoverable path to the shortcut.
        assert!(mute_label(true).contains("[M]"));
        assert!(mute_label(false).contains("[M]"));
        assert_ne!(mute_label(true), mute_label(false));
    }
}
