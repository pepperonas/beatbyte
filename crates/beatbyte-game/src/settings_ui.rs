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
use crate::ui_kit;

/// The adjustable rows, in display order. `pub(crate)` because the
/// pause menu reuses a safe subset — one definition of every step
/// size and clamp, two places that draw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    MusicVolume,
    SfxVolume,
    ScrollSpeed,
    LatencyOffset,
    VideoOffset,
    Particles,
    ScreenShake,
    BeatPulse,
    BackdropMotion,
    TapMode,
    NoteStyle,
    Fullscreen,
    Theme,
    WatchFolder,
    ReducedFlashing,
    FxIntensity,
    TextScale,
    HighContrast,
    Controls,
}

impl Row {
    const ALL: [Row; 19] = [
        Row::MusicVolume,
        Row::SfxVolume,
        Row::ScrollSpeed,
        Row::LatencyOffset,
        Row::VideoOffset,
        Row::Particles,
        Row::ScreenShake,
        Row::BeatPulse,
        Row::BackdropMotion,
        Row::ReducedFlashing,
        Row::FxIntensity,
        Row::TextScale,
        Row::HighContrast,
        Row::TapMode,
        Row::NoteStyle,
        Row::Fullscreen,
        Row::Theme,
        Row::WatchFolder,
        Row::Controls,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Row::MusicVolume => "MUSIC VOLUME",
            Row::SfxVolume => "SFX VOLUME",
            Row::ScrollSpeed => "SCROLL SPEED",
            Row::LatencyOffset => "LATENCY OFFSET",
            Row::VideoOffset => "VIDEO OFFSET",
            Row::Particles => "PARTICLES",
            Row::ScreenShake => "SCREEN SHAKE",
            Row::BeatPulse => "BEAT PULSE",
            Row::BackdropMotion => "STAGE MOTION",
            Row::ReducedFlashing => "REDUCED FLASHING",
            Row::FxIntensity => "EFFECT INTENSITY",
            Row::TextScale => "UI SCALE",
            Row::HighContrast => "HIGH CONTRAST",
            Row::TapMode => "TAP MODE (NO STRUM)",
            Row::NoteStyle => "NOTE STYLE",
            Row::Fullscreen => "FULLSCREEN",
            Row::Theme => "STAGE THEME",
            Row::WatchFolder => "SONG FOLDER",
            Row::Controls => "CONTROLS",
        }
    }

    pub(crate) fn value(self, settings: &Settings) -> String {
        match self {
            Row::MusicVolume => format!("{:.0}%", settings.music_volume * 100.0),
            Row::SfxVolume => format!("{:.0}%", settings.sfx_volume * 100.0),
            Row::ScrollSpeed => format!("{:.0} px/s", settings.scroll_speed),
            Row::LatencyOffset => format!("{:+.0} ms", settings.latency_offset_ms),
            Row::VideoOffset => format!("{:+.0} ms", settings.video_offset_ms),
            Row::Particles => on_off(settings.particles),
            Row::ScreenShake => on_off(settings.screen_shake),
            Row::BeatPulse => on_off(settings.beat_pulse),
            Row::BackdropMotion => on_off(settings.backdrop_motion),
            Row::WatchFolder => settings.watch_folder.as_ref().map_or_else(
                || "drop a folder onto the window".to_owned(),
                |path| {
                    path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| format!("watching: {}", name.to_string_lossy()),
                    )
                },
            ),
            Row::ReducedFlashing => on_off(settings.reduced_flashing),
            Row::FxIntensity => format!("{:.0}%", settings.fx_intensity * 100.0),
            Row::TextScale => format!("{:.0}%", settings.ui_scale * 100.0),
            Row::HighContrast => on_off(settings.high_contrast),
            Row::TapMode => on_off(settings.tap_mode),
            Row::NoteStyle => if settings.round_gems {
                "ROUND"
            } else {
                "8-BIT SHAPES"
            }
            .to_owned(),
            Row::Fullscreen => on_off(settings.fullscreen),
            Row::Theme => settings.theme.to_uppercase(),
            Row::Controls => "OPEN >".to_owned(),
        }
    }

    /// Adjust by one step (direction −1 or +1).
    pub(crate) fn adjust(self, settings: &mut Settings, direction: f32) {
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
            Row::VideoOffset => {
                settings.video_offset_ms =
                    (settings.video_offset_ms + 5.0 * direction).clamp(-100.0, 100.0);
            }
            Row::Particles => settings.particles = !settings.particles,
            Row::ScreenShake => settings.screen_shake = !settings.screen_shake,
            Row::BeatPulse => settings.beat_pulse = !settings.beat_pulse,
            Row::BackdropMotion => settings.backdrop_motion = !settings.backdrop_motion,
            Row::WatchFolder => settings.watch_folder = None,
            Row::ReducedFlashing => settings.reduced_flashing = !settings.reduced_flashing,
            Row::FxIntensity => {
                settings.fx_intensity = (settings.fx_intensity + 0.1 * direction).clamp(0.0, 1.0);
            }
            Row::TextScale => {
                settings.ui_scale = (settings.ui_scale + 0.05 * direction).clamp(0.75, 1.5);
            }
            Row::HighContrast => settings.high_contrast = !settings.high_contrast,
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

    /// The feedback voice a row speaks with when it adjusts: toggles
    /// click, stepped values tick.
    const fn sound(self) -> crate::sfx::UiSound {
        match self {
            Row::Particles
            | Row::ScreenShake
            | Row::BeatPulse
            | Row::BackdropMotion
            | Row::WatchFolder
            | Row::ReducedFlashing
            | Row::HighContrast
            | Row::TapMode
            | Row::NoteStyle
            | Row::Fullscreen => crate::sfx::UiSound::Toggle,
            _ => crate::sfx::UiSound::Slider,
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
                (settings_input, refresh_settings, follow_settings_cursor)
                    .run_if(in_state(AppState::Settings)),
            )
            .add_systems(
                OnExit(AppState::Settings),
                (persist_settings, despawn_settings),
            );
    }
}

#[derive(Component)]
struct SettingsScreen;

/// The scrolling list of settings rows.
#[derive(Component)]
struct SettingsList;

/// Keep the cursor row in view — the same measured whole-row window
/// the browser and the controls screen use. Seventeen rows outgrew
/// the safe area exactly the way fifteen did on the controls screen.
fn follow_settings_cursor(
    cursor: Res<SettingsCursor>,
    rows: Query<(&RowText, &ComputedNode)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition, &mut Node), With<SettingsList>>,
) {
    let Ok((list, mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    let Some(row_h) = rows
        .iter()
        .map(|(_, node)| node.size().y)
        .find(|height| *height > 0.0)
    else {
        return;
    };
    let count = Row::ALL.len();
    let pitch = row_h + ui_kit::ROW_GAP;
    if let Some(height) =
        ui_kit::whole_rows_height(row_h, ui_kit::ROW_GAP, count, ui_kit::PANEL_MAX_H)
    {
        let wanted = px(height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
    }
    let total = count as f32;
    let content_h = total.mul_add(row_h, (total - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = list.size().y - 2.0 * ui_kit::PANEL_PAD;
    let row_top = cursor.0 as f32 * pitch;
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

/// A settings row (index into [`Row::ALL`]). Stays on the entity that
/// carries `Button`, so the existing input handler is untouched.
#[derive(Component)]
struct RowText(usize);

/// A row's label text — static, written once at spawn.
#[derive(Component)]
struct SettingLabel(usize);

/// A row's value text — the only part that changes at runtime.
#[derive(Component)]
struct SettingValue(usize);

fn spawn_settings(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((SettingsScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "SETTINGS", "sound, feel and looks");
            parent
                .spawn((SettingsList, ui_kit::scroll_panel(ui_kit::PANEL_WIDTH)))
                .with_children(|panel| {
                    for (index, definition) in Row::ALL.iter().enumerate() {
                        panel
                            .spawn((RowText(index), Button, ui_kit::row()))
                            .with_children(|row| {
                                // Label and value are separate texts in a
                                // space-between row. The old single-string
                                // layout padded the label to 16 characters,
                                // which "TAP MODE (NO STRUM)" overflows by
                                // three — that one row's value hung out of
                                // the column.
                                row.spawn((
                                    SettingLabel(index),
                                    Text::new(definition.label()),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::label_node(),
                                ));
                                row.spawn((
                                    SettingValue(index),
                                    Text::new(""),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::value_node(),
                                ));
                            });
                    }
                });
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  LEFT/RIGHT adjust  ESC back",
                "D-PAD choose and adjust  EAST back",
            );
        });
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn settings_input(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::controls::InputMap>,
    pads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    rows: Query<(&RowText, &Interaction), Changed<Interaction>>,
    mut cursor: ResMut<SettingsCursor>,
    mut settings: ResMut<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let count = Row::ALL.len();
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    if nav.up || nav.down {
        sounds.write(crate::sfx::UiSound::Navigate);
    }
    // Mouse: hover selects; click on the selected row steps it (like
    // RIGHT); the wheel steps the hovered value either way.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        cursor.0 = index;
    }
    let clicked = pointer.clicked;
    // The wheel SCROLLS the rows, exactly like the song list - it
    // used to step the hovered value, which changed settings by
    // accident while browsing them (user report, 2026-09-01).
    for event in wheel.read() {
        if event.y > 0.0 {
            cursor.0 = (cursor.0 + count - 1) % count;
        } else if event.y < 0.0 {
            cursor.0 = (cursor.0 + 1) % count;
        }
        if event.y != 0.0 {
            sounds.write(crate::sfx::UiSound::Navigate);
        }
    }
    let row = Row::ALL[cursor.0];
    if row == Row::Controls && (nav.confirm || nav.right || clicked) {
        sounds.write(crate::sfx::UiSound::Confirm);
        next_state.set(AppState::Controls);
        return;
    }
    let mut adjusted = false;
    if nav.left {
        row.adjust(&mut settings, -1.0);
        adjusted = true;
    }
    if nav.right || nav.confirm || clicked {
        row.adjust(&mut settings, 1.0);
        adjusted = true;
    }
    if adjusted {
        sounds.write(row.sound());
    }
    if nav.back || mouse.just_pressed(MouseButton::Right) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_settings(
    settings: Res<Settings>,
    cursor: Res<SettingsCursor>,
    mut rows: Query<(&RowText, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&SettingLabel, &mut TextColor), Without<SettingValue>>,
    mut values: Query<(&SettingValue, &mut Text, &mut TextColor), Without<SettingLabel>>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = ui_kit::styled_row(
            ui_kit::state_for(row.0 == cursor.0, false),
            settings.high_contrast,
        );
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (label, mut color) in &mut labels {
        color.0 = ui_kit::styled_row(
            ui_kit::state_for(label.0 == cursor.0, false),
            settings.high_contrast,
        )
        .label;
    }
    for (value, mut text, mut color) in &mut values {
        let wanted = Row::ALL[value.0].value(&settings);
        if text.0 != wanted {
            text.0 = wanted;
        }
        color.0 = ui_kit::styled_row(
            ui_kit::state_for(value.0 == cursor.0, false),
            settings.high_contrast,
        )
        .value;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Step a row `count` times in one direction.
    fn step(row: Row, settings: &mut Settings, direction: f32, count: usize) {
        for _ in 0..count {
            row.adjust(settings, direction);
        }
    }

    #[test]
    fn volumes_stay_inside_their_range() {
        // Held LEFT must not drive the volume negative, which would
        // silence the game with no way back through the same key.
        let mut settings = Settings::default();
        step(Row::MusicVolume, &mut settings, -1.0, 50);
        assert!((0.0..=1.0).contains(&settings.music_volume));
        step(Row::MusicVolume, &mut settings, 1.0, 50);
        assert!((0.0..=1.0).contains(&settings.music_volume));
        step(Row::SfxVolume, &mut settings, -1.0, 50);
        assert!((0.0..=1.0).contains(&settings.sfx_volume));
    }

    #[test]
    fn scroll_speed_and_latency_stay_playable() {
        let mut settings = Settings::default();
        step(Row::ScrollSpeed, &mut settings, -1.0, 100);
        assert!((240.0..=900.0).contains(&settings.scroll_speed));
        step(Row::ScrollSpeed, &mut settings, 1.0, 100);
        assert!((240.0..=900.0).contains(&settings.scroll_speed));
        step(Row::LatencyOffset, &mut settings, 1.0, 200);
        assert!((-250.0..=250.0).contains(&settings.latency_offset_ms));
        step(Row::LatencyOffset, &mut settings, -1.0, 200);
        assert!((-250.0..=250.0).contains(&settings.latency_offset_ms));
    }

    #[test]
    fn the_theme_cycle_only_ever_produces_a_real_setting() {
        // Cycling past either end must wrap onto a known id. An
        // unknown id would silently fall back to auto forever.
        let known: Vec<String> = std::iter::once("auto".to_owned())
            .chain(crate::theme::THEMES.iter().map(|t| t.id.to_owned()))
            .collect();
        let mut settings = Settings::default();
        for direction in [1.0, -1.0] {
            for _ in 0..(known.len() * 2 + 1) {
                Row::Theme.adjust(&mut settings, direction);
                assert!(
                    known.contains(&settings.theme),
                    "cycled onto unknown theme `{}`",
                    settings.theme
                );
            }
        }
    }

    #[test]
    fn the_theme_cycle_visits_every_stage_and_returns() {
        let known_count = crate::theme::THEMES.len() + 1; // + "auto"
        let mut settings = Settings::default();
        let start = settings.theme.clone();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..known_count {
            seen.insert(settings.theme.clone());
            Row::Theme.adjust(&mut settings, 1.0);
        }
        assert_eq!(seen.len(), known_count, "cycle skipped a stage");
        assert_eq!(settings.theme, start, "cycle did not come back around");
    }

    #[test]
    fn no_setting_can_reach_a_removed_view() {
        // Two views are gone by now — the flat highway and the 2D
        // depth view. A stale settings file re-opening either would
        // strand the player in a presentation that no longer exists;
        // sanitize() forces both flags back.
        let mut settings = Settings {
            perspective: false,
            stage_3d: false,
            ..Settings::default()
        };
        settings.sanitize();
        assert!(settings.perspective, "flat highway reachable again");
        assert!(
            settings.stage_3d,
            "the removed 2D depth view reachable again"
        );
    }

    #[test]
    fn toggles_flip_and_report_themselves() {
        let mut settings = Settings::default();
        for row in [
            Row::Particles,
            Row::ScreenShake,
            Row::BeatPulse,
            Row::BackdropMotion,
            Row::TapMode,
            Row::Fullscreen,
        ] {
            let before = row.value(&settings);
            row.adjust(&mut settings, 1.0);
            let after = row.value(&settings);
            assert_ne!(before, after, "{} did not change", row.label());
            assert!(matches!(after.as_str(), "ON" | "OFF"));
        }
    }

    #[test]
    fn the_controls_row_is_a_door_and_holds_no_value() {
        // It navigates; stepping it must not mutate anything.
        let mut settings = Settings::default();
        let before = settings.clone();
        Row::Controls.adjust(&mut settings, 1.0);
        Row::Controls.adjust(&mut settings, -1.0);
        assert_eq!(
            format!("{before:?}"),
            format!("{settings:?}"),
            "the CONTROLS row changed a setting"
        );
    }

    #[test]
    fn every_row_renders_a_value() {
        // A blank right-hand column reads as a broken row.
        let settings = Settings::default();
        for row in Row::ALL {
            assert!(
                !row.value(&settings).is_empty(),
                "{} has no value",
                row.label()
            );
            assert!(!row.label().is_empty());
        }
    }
}
