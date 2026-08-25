//! The settings screen: volumes, scroll speed, latency offset,
//! effect toggles, fullscreen. Changes apply immediately and persist
//! on leaving the screen.

use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::config::{Settings, save_settings};
use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// The adjustable rows, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    MusicVolume,
    SfxVolume,
    ScrollSpeed,
    LatencyOffset,
    Particles,
    ScreenShake,
    BeatPulse,
    BackdropMotion,
    TapMode,
    NoteStyle,
    Fullscreen,
    Theme,
    Controls,
}

impl Row {
    const ALL: [Row; 13] = [
        Row::MusicVolume,
        Row::SfxVolume,
        Row::ScrollSpeed,
        Row::LatencyOffset,
        Row::Particles,
        Row::ScreenShake,
        Row::BeatPulse,
        Row::BackdropMotion,
        Row::TapMode,
        Row::NoteStyle,
        Row::Fullscreen,
        Row::Theme,
        Row::Controls,
    ];

    const fn label(self) -> &'static str {
        match self {
            Row::MusicVolume => "MUSIC VOLUME",
            Row::SfxVolume => "SFX VOLUME",
            Row::ScrollSpeed => "SCROLL SPEED",
            Row::LatencyOffset => "LATENCY OFFSET",
            Row::Particles => "PARTICLES",
            Row::ScreenShake => "SCREEN SHAKE",
            Row::BeatPulse => "BEAT PULSE",
            Row::BackdropMotion => "STAGE MOTION",
            Row::TapMode => "TAP MODE (NO STRUM)",
            Row::NoteStyle => "NOTE STYLE",
            Row::Fullscreen => "FULLSCREEN",
            Row::Theme => "STAGE THEME",
            Row::Controls => "CONTROLS",
        }
    }

    fn value(self, settings: &Settings) -> String {
        match self {
            Row::MusicVolume => format!("{:.0}%", settings.music_volume * 100.0),
            Row::SfxVolume => format!("{:.0}%", settings.sfx_volume * 100.0),
            Row::ScrollSpeed => format!("{:.0} px/s", settings.scroll_speed),
            Row::LatencyOffset => format!("{:+.0} ms", settings.latency_offset_ms),
            Row::Particles => on_off(settings.particles),
            Row::ScreenShake => on_off(settings.screen_shake),
            Row::BeatPulse => on_off(settings.beat_pulse),
            Row::BackdropMotion => on_off(settings.backdrop_motion),
            Row::TapMode => on_off(settings.tap_mode),
            Row::NoteStyle => if settings.round_gems {
                "ROUND"
            } else {
                "8-BIT SHAPES"
            }
            .to_owned(),
            Row::Fullscreen => on_off(settings.fullscreen),
            Row::Theme => settings.theme.to_uppercase(),
            Row::Controls => "...".to_owned(),
        }
    }

    /// Adjust by one step (direction −1 or +1).
    fn adjust(self, settings: &mut Settings, direction: f32) {
        match self {
            Row::MusicVolume => {
                settings.music_volume = (settings.music_volume + 0.1 * direction).clamp(0.0, 1.0);
            }
            Row::SfxVolume => {
                settings.sfx_volume = (settings.sfx_volume + 0.1 * direction).clamp(0.0, 1.0);
            }
            Row::ScrollSpeed => {
                settings.scroll_speed =
                    (settings.scroll_speed + 30.0 * direction).clamp(240.0, 900.0);
            }
            Row::LatencyOffset => {
                settings.latency_offset_ms =
                    (settings.latency_offset_ms + 5.0 * direction).clamp(-250.0, 250.0);
            }
            Row::Particles => settings.particles = !settings.particles,
            Row::ScreenShake => settings.screen_shake = !settings.screen_shake,
            Row::BeatPulse => settings.beat_pulse = !settings.beat_pulse,
            Row::BackdropMotion => settings.backdrop_motion = !settings.backdrop_motion,
            Row::TapMode => settings.tap_mode = !settings.tap_mode,
            Row::NoteStyle => settings.round_gems = !settings.round_gems,
            Row::Fullscreen => settings.fullscreen = !settings.fullscreen,
            Row::Theme => {
                // Cycle auto → themes → auto.
                let mut ids = vec!["auto"];
                ids.extend(crate::theme::THEMES.iter().map(|theme| theme.id));
                let position = ids.iter().position(|id| *id == settings.theme).unwrap_or(0) as i32;
                let count = ids.len() as i32;
                let next = (position + direction as i32 + count) % count;
                settings.theme = ids[next as usize].to_owned();
            }
            Row::Controls => {}
        }
    }
}

fn on_off(value: bool) -> String {
    if value { "ON" } else { "OFF" }.to_owned()
}

#[derive(Resource, Default)]
struct SettingsCursor(usize);

/// Plugin for the settings screen.
pub struct SettingsUiPlugin;

impl Plugin for SettingsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsCursor>()
            .add_systems(OnEnter(AppState::Settings), spawn_settings)
            .add_systems(
                Update,
                (settings_input, refresh_settings).run_if(in_state(AppState::Settings)),
            )
            .add_systems(
                OnExit(AppState::Settings),
                (persist_settings, despawn_settings),
            );
    }
}

#[derive(Component)]
struct SettingsScreen;

/// One row's value text (index into [`Row::ALL`]).
#[derive(Component)]
struct RowText(usize);

fn spawn_settings(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            SettingsScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(14),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("SETTINGS"),
                font.text(26.0),
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(px(20)),
                    ..default()
                },
            ));
            for (index, _) in Row::ALL.iter().enumerate() {
                parent.spawn((
                    RowText(index),
                    Text::new(""),
                    font.text(13.0),
                    TextColor(palette::TEXT_DIM),
                ));
            }
            parent.spawn((
                Text::new("UP/DOWN choose   LEFT/RIGHT adjust   ESC back"),
                font.text(9.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::top(px(24)),
                    ..default()
                },
            ));
        });
}

fn settings_input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut cursor: ResMut<SettingsCursor>,
    mut settings: ResMut<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let nav = MenuNav::read(&keys, pads.iter());
    let count = Row::ALL.len();
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    let row = Row::ALL[cursor.0];
    if row == Row::Controls && (nav.confirm || nav.right) {
        next_state.set(AppState::Controls);
        return;
    }
    if nav.left {
        row.adjust(&mut settings, -1.0);
    }
    if nav.right || nav.confirm {
        row.adjust(&mut settings, 1.0);
    }
    if nav.back {
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_settings(
    settings: Res<Settings>,
    cursor: Res<SettingsCursor>,
    mut rows: Query<(&RowText, &mut Text, &mut TextColor)>,
) {
    for (row, mut text, mut color) in &mut rows {
        let definition = Row::ALL[row.0];
        let marker = if row.0 == cursor.0 { ">" } else { " " };
        let line = format!(
            "{marker} {:<16} {:>9}",
            definition.label(),
            definition.value(&settings)
        );
        if text.0 != line {
            text.0 = line;
        }
        color.0 = if row.0 == cursor.0 {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
    }
}

fn persist_settings(settings: Res<Settings>) {
    save_settings(&settings);
}

fn despawn_settings(mut commands: Commands, entities: Query<Entity, With<SettingsScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
