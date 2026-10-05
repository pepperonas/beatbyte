//! The settings screen: volumes, scroll speed, latency offset,
//! effect toggles, fullscreen. Changes apply immediately and persist
//! on leaving the screen.

use bevy::prelude::*;

use crate::config::{FlashSync, Settings, TelemetryLevel, save_settings};
use crate::menu_list::list::{
    self, ListInput, ListLabel, ListPaint, ListPanel, ListRow, ListValue,
};
use crate::menu_list::spec::{self, Ends, Feel, Kind, RowSpec, Subtitle, Unit};
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;
use beatbyte_core::vocal::PitchMode;

/// What a settings row does besides editing a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Opens another screen.
    Open(AppState),
    /// The song folder that is watched for new tracks (a step clears it).
    WatchFolder,
    /// Where the library lives, and moving it there.
    Library,
    /// Writes the play history to Downloads.
    ExportHistory,
    /// The lyrics model: its standing, and the one download.
    LyricsModel,
}

/// The spec type of a settings row.
pub(crate) type Spec = RowSpec<Settings, Act>;

/// One settings row: a handle on its spec. Two rows are the same row
/// when their labels are — labels are unique, a test says so.
///
/// `pub(crate)` because the pause menu reuses a safe subset — one
/// definition of every step size and clamp, two places that draw it.
#[derive(Clone, Copy)]
pub(crate) struct Row(&'static Spec);

impl PartialEq for Row {
    fn eq(&self, other: &Row) -> bool {
        self.0.label == other.0.label
    }
}

impl Eq for Row {}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label)
    }
}

/// `ON` / `OFF` for nearly every toggle.
fn on_off(settings: &Settings, value: bool) -> String {
    spec::on_off(settings, value)
}

/// A toggle on a `bool` field of `Settings`.
macro_rules! toggle {
    ($field:ident) => {
        Kind::Toggle {
            get: |s: &Settings| s.$field,
            set: |s: &mut Settings, v| s.$field = v,
            words: on_off,
        }
    };
}

/// A slider on an `f32` field of `Settings`.
macro_rules! slider {
    ($field:ident, $min:expr, $max:expr, $step:expr, $unit:ident) => {
        Kind::Slider {
            get: |s: &Settings| s.$field,
            set: |s: &mut Settings, v| s.$field = v,
            min: $min,
            max: $max,
            step: $step,
            unit: Unit::$unit,
        }
    };
}

/// A row, written as one entry.
macro_rules! row {
    ($label:literal, $kind:expr) => {
        Row(&RowSpec {
            label: $label,
            kind: $kind,
            subtitle: Subtitle::None,
        })
    };
    ($label:literal, $kind:expr, $subtitle:expr) => {
        Row(&RowSpec {
            label: $label,
            kind: $kind,
            subtitle: $subtitle,
        })
    };
}

impl Row {
    // ---- The rows. One entry each; then list it in `ALL` where its
    // ---- label falls. A row defined and not listed fails the build
    // ---- (dead code under `-D warnings`).
    pub(crate) const AI_SEARCH: Row = row!(
        "AI SEARCH",
        Kind::Toggle {
            get: |s: &Settings| s.ai_search,
            set: |s: &mut Settings, v| s.ai_search = v,
            words: ai_search_words,
        },
        Subtitle::Text("picks which recording a song search fetches")
    );
    pub(crate) const BEAT_PULSE: Row = row!("BEAT PULSE", toggle!(beat_pulse));
    pub(crate) const CALIBRATION: Row =
        row!("CALIBRATION", Kind::Door(Act::Open(AppState::Calibration)));
    pub(crate) const CONTROLS: Row = row!("CONTROLS", Kind::Door(Act::Open(AppState::Controls)));
    pub(crate) const FX_INTENSITY: Row = row!(
        "EFFECT INTENSITY",
        slider!(fx_intensity, 0.0, 1.0, 0.1, Percent)
    );
    pub(crate) const EXPORT_HISTORY: Row = row!(
        "EXPORT PLAY HISTORY",
        Kind::Custom {
            id: Act::ExportHistory,
            value: |_: &Settings| "ENTER > DOWNLOADS".to_owned(),
            adjust: None,
            feel: Feel::Tick,
        }
    );
    pub(crate) const FLASH_SYNC: Row = row!(
        "FLASH SYNC",
        Kind::Choice {
            labels: || vec!["ROOM LEVEL".to_owned(), "SONG BEAT".to_owned()],
            get: |s: &Settings| usize::from(s.flash_sync == FlashSync::Beat),
            set: |s: &mut Settings, i| {
                s.flash_sync = if i == 1 {
                    FlashSync::Beat
                } else {
                    FlashSync::Level
                };
            },
            ends: Ends::Wrap,
        },
        // Which of the two clocks this is, and what it costs: one
        // needs a microphone, the other needs nothing.
        Subtitle::Text("ROOM LEVEL hears the room; SONG BEAT needs no mic")
    );
    pub(crate) const FULLSCREEN: Row = row!("FULLSCREEN", toggle!(fullscreen));
    pub(crate) const GUITAR_STUDY_TWINS: Row = row!(
        "GUITAR STUDY TWINS",
        toggle!(guitar_study_twins),
        Subtitle::Live(crate::study_twin::row_subtitle)
    );
    pub(crate) const HIGH_CONTRAST: Row = row!("HIGH CONTRAST", toggle!(high_contrast));
    pub(crate) const HIT_LABELS: Row = row!("HIT LABELS", toggle!(hit_labels));
    pub(crate) const INPUT_TEST: Row =
        row!("INPUT TEST", Kind::Door(Act::Open(AppState::InputTest)));
    pub(crate) const LATENCY_OFFSET: Row = row!(
        "LATENCY OFFSET",
        slider!(latency_offset_ms, -250.0, 250.0, 5.0, SignedMs)
    );
    pub(crate) const LIBRARY: Row = row!(
        "LIBRARY",
        Kind::Custom {
            id: Act::Library,
            // The live value comes from `LibraryMove` in the refresh.
            value: |_: &Settings| "-".to_owned(),
            adjust: None,
            feel: Feel::Tick,
        },
        Subtitle::Text("ENTER picks a new place - or drop a folder here")
    );
    pub(crate) const LOUDNESS_MATCH: Row = row!("LOUDNESS MATCH", toggle!(normalize_loudness));
    pub(crate) const LYRICS: Row = row!("LYRICS", toggle!(lyrics));
    pub(crate) const LYRICS_LEAD_IN: Row = row!(
        "LYRICS LEAD-IN",
        slider!(lyrics_lead_in_ms, 0.0, 4000.0, 250.0, SecondsOfMs)
    );
    pub(crate) const LYRICS_MODEL: Row = row!(
        "LYRICS MODEL",
        Kind::Custom {
            id: Act::LyricsModel,
            // The live value comes from `SmartLyrics` in the refresh;
            // this is what a build without the resource would show.
            value: |_: &Settings| "CHECKING...".to_owned(),
            adjust: None,
            feel: Feel::Tick,
        }
    );
    pub(crate) const LYRICS_OFFSET: Row = row!(
        "LYRICS OFFSET",
        slider!(lyrics_offset_ms, -500.0, 500.0, 10.0, SignedMs)
    );
    pub(crate) const LYRICS_SIZE: Row = row!(
        "LYRICS SIZE",
        Kind::Choice {
            labels: || ["SMALL", "MEDIUM", "LARGE"].map(str::to_owned).to_vec(),
            get: |s: &Settings| match s.lyrics_size {
                0 => 0,
                2 => 2,
                _ => 1,
            },
            set: |s: &mut Settings, i| s.lyrics_size = i as u8,
            ends: Ends::Stop,
        }
    );
    pub(crate) const MIC_OFFSET: Row = row!(
        "MIC OFFSET",
        slider!(mic_offset_ms, -250.0, 500.0, 5.0, SignedMs),
        Subtitle::Text("how late the microphone hears the song")
    );
    pub(crate) const MUSIC_VOLUME: Row = row!(
        "MUSIC VOLUME",
        slider!(music_volume, 0.0, 1.0, 0.1, Percent)
    );
    pub(crate) const NO_FAIL: Row = row!("NO FAIL", toggle!(no_fail));
    pub(crate) const ORIGINAL_VOCALS: Row = row!(
        "ORIGINAL VOCALS",
        slider!(original_vocals, 0.0, 1.0, 0.05, Percent),
        Subtitle::Text("above zero marks a vocal run assisted")
    );
    pub(crate) const PARTICLES: Row = row!("PARTICLES", toggle!(particles));
    pub(crate) const REDUCED_FLASHING: Row = row!("REDUCED FLASHING", toggle!(reduced_flashing));
    pub(crate) const ROOM_LIGHTS: Row = row!("ROOM LIGHTS", toggle!(room_lights));
    pub(crate) const SCREEN_SHAKE: Row = row!("SCREEN SHAKE", toggle!(screen_shake));
    pub(crate) const SCROLL_SPEED: Row = row!(
        "SCROLL SPEED",
        slider!(scroll_speed, 240.0, 900.0, 30.0, PxPerS)
    );
    pub(crate) const SFX_VOLUME: Row =
        row!("SFX VOLUME", slider!(sfx_volume, 0.0, 1.0, 0.1, Percent));
    pub(crate) const WATCH_FOLDER: Row = row!(
        "SONG FOLDER",
        Kind::Custom {
            id: Act::WatchFolder,
            value: watch_folder_value,
            adjust: Some(|s: &mut Settings| s.watch_folder = None),
            feel: Feel::Click,
        },
        Subtitle::Live(watch_folder_subtitle)
    );
    pub(crate) const SONG_PREVIEW: Row = row!("SONG PREVIEW", toggle!(song_preview));
    pub(crate) const BACKDROP_MOTION: Row = row!("STAGE MOTION", toggle!(backdrop_motion));
    pub(crate) const THEME: Row = row!(
        "STAGE THEME",
        Kind::Choice {
            labels: || theme_ids().iter().map(|id| id.to_uppercase()).collect(),
            get: |s: &Settings| theme_ids()
                .iter()
                .position(|id| *id == s.theme)
                .unwrap_or(0),
            set: |s: &mut Settings, i| {
                s.theme = theme_ids().get(i).copied().unwrap_or("auto").to_owned();
            },
            ends: Ends::Wrap,
        }
    );
    pub(crate) const TAP_MODE: Row = row!("TAP MODE (NO STRUM)", toggle!(tap_mode));
    pub(crate) const TELEMETRY: Row = row!(
        "TELEMETRY",
        Kind::Choice {
            labels: || TelemetryLevel::ALL
                .iter()
                .map(|l| l.label().to_owned())
                .collect(),
            get: |s: &Settings| {
                TelemetryLevel::ALL
                    .iter()
                    .position(|l| *l == s.telemetry)
                    .unwrap_or(2)
            },
            set: |s: &mut Settings, i| s.telemetry = TelemetryLevel::ALL[i.min(3)],
            ends: Ends::Wrap,
        },
        Subtitle::Text("what a run records, on this machine only")
    );
    pub(crate) const TEXT_SCALE: Row =
        row!("UI SCALE", slider!(ui_scale, 0.75, 1.5, 0.05, Percent));
    pub(crate) const VIDEO_OFFSET: Row = row!(
        "VIDEO OFFSET",
        slider!(video_offset_ms, -100.0, 100.0, 5.0, SignedMs)
    );
    pub(crate) const VOCAL_CHARTS: Row = row!(
        "VOCAL CHARTS",
        toggle!(vocal_charts),
        Subtitle::Live(crate::study_twin::vocal_row_subtitle)
    );
    pub(crate) const VOCAL_PITCH: Row = row!(
        "VOCAL PITCH",
        Kind::Choice {
            labels: || vec!["ANY OCTAVE".to_owned(), "AS WRITTEN".to_owned()],
            get: |s: &Settings| usize::from(s.vocal_pitch_mode == PitchMode::Strict),
            set: |s: &mut Settings, i| {
                s.vocal_pitch_mode = if i == 1 {
                    PitchMode::Strict
                } else {
                    PitchMode::OctaveIndependent
                };
            },
            ends: Ends::Wrap,
        },
        Subtitle::Text("an octave out: forgiven, or counted")
    );

    /// Every row, in the order the screen shows them: **alphabetical
    /// by label**, and kept that way by a test — a new row goes where
    /// its name falls, not at the end of the list.
    pub(crate) const ALL: [Row; 40] = [
        Row::AI_SEARCH,
        Row::BEAT_PULSE,
        Row::CALIBRATION,
        Row::CONTROLS,
        Row::FX_INTENSITY,
        Row::EXPORT_HISTORY,
        Row::FLASH_SYNC,
        Row::FULLSCREEN,
        Row::GUITAR_STUDY_TWINS,
        Row::HIGH_CONTRAST,
        Row::HIT_LABELS,
        Row::INPUT_TEST,
        Row::LATENCY_OFFSET,
        Row::LIBRARY,
        Row::LOUDNESS_MATCH,
        Row::LYRICS,
        Row::LYRICS_LEAD_IN,
        Row::LYRICS_MODEL,
        Row::LYRICS_OFFSET,
        Row::LYRICS_SIZE,
        Row::MIC_OFFSET,
        Row::MUSIC_VOLUME,
        Row::NO_FAIL,
        Row::ORIGINAL_VOCALS,
        Row::PARTICLES,
        Row::REDUCED_FLASHING,
        Row::ROOM_LIGHTS,
        Row::SCREEN_SHAKE,
        Row::SCROLL_SPEED,
        Row::SFX_VOLUME,
        Row::WATCH_FOLDER,
        Row::SONG_PREVIEW,
        Row::BACKDROP_MOTION,
        Row::THEME,
        Row::TAP_MODE,
        Row::TELEMETRY,
        Row::TEXT_SCALE,
        Row::VIDEO_OFFSET,
        Row::VOCAL_CHARTS,
        Row::VOCAL_PITCH,
    ];

    /// The row's spec.
    #[must_use]
    pub(crate) const fn spec(self) -> &'static Spec {
        self.0
    }

    /// Where a row sits in the list (for a drill that has to arrow
    /// down to it with real keys).
    #[must_use]
    pub(crate) fn index(self) -> usize {
        Row::ALL.iter().position(|row| *row == self).unwrap_or(0)
    }

    /// The row's name.
    pub(crate) const fn label(self) -> &'static str {
        self.0.label
    }

    /// The value as the row shows it.
    pub(crate) fn value(self, settings: &Settings) -> String {
        self.0.value(settings)
    }

    /// The line under a row: the fact behind the setting, where
    /// there is one. Empty for every row that needs no explaining.
    pub(crate) fn subtitle(self) -> String {
        self.0.subtitle()
    }

    /// Adjust by one step (direction −1 or +1).
    pub(crate) fn adjust(self, settings: &mut Settings, direction: f32) {
        self.0.step(settings, if direction < 0.0 { -1 } else { 1 });
    }
}

/// `auto` and every stage theme, in the order the row cycles.
fn theme_ids() -> Vec<&'static str> {
    std::iter::once("auto")
        .chain(crate::theme::THEMES.iter().map(|theme| theme.id))
        .collect()
}

/// AI SEARCH's value names WHERE the model runs, or says that nothing
/// can — switched on with nothing to run it, the row says so rather
/// than promising a step that will not happen.
fn ai_search_words(settings: &Settings, on: bool) -> String {
    if !on {
        return "OFF".to_owned();
    }
    match crate::discover::backend_for(
        true,
        crate::discover::cli_available(),
        crate::discover::api_key(&settings.anthropic_api_key).as_deref(),
    ) {
        crate::discover::Backend::Cli => "ON (CLAUDE CLI)".to_owned(),
        crate::discover::Backend::Api(_) => "ON (API KEY)".to_owned(),
        crate::discover::Backend::Off => "ON (NOTHING TO RUN IT)".to_owned(),
    }
}

/// SONG FOLDER's value: the watched folder's name, or how to set one.
fn watch_folder_value(settings: &Settings) -> String {
    settings.watch_folder.as_ref().map_or_else(
        || "drop a folder onto the window".to_owned(),
        |path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| format!("watching: {}", name.to_string_lossy()),
            )
        },
    )
}

/// Where the tracks actually come from. The value line says whether a
/// folder is WATCHED for new ones; this says which directories the
/// library is read from, which is a different question and the one a
/// player asks when a song is missing.
fn watch_folder_subtitle() -> String {
    let roots = crate::library::live_scan_roots();
    if roots.is_empty() {
        "no song folder on disk yet".to_owned()
    } else {
        roots
            .iter()
            .map(|root| short_path(&root.display().to_string(), SUBTITLE_CHARS))
            .collect::<Vec<_>>()
            .join("   ")
    }
}
/// How many glyphs a subtitle may use before it is shortened.
/// Press Start 2P advances a full em, and the panel is
/// [`ui_kit::PANEL_WIDTH`] wide minus its padding — a test pins
/// that this fits.
pub(crate) const SUBTITLE_CHARS: usize = 56;

/// Shorten a path to fit, keeping the END.
///
/// The tail is the informative part — `.../beat-byte/songs` says
/// what the head never does — so an over-long path loses its front
/// to an ellipsis, and the home directory collapses to `~` first.
/// Pure — tested.
#[must_use]
pub fn short_path(path: &str, limit: usize) -> String {
    let home = dirs::home_dir().map(|home| home.display().to_string());
    let shortened = match home {
        Some(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_owned(),
    };
    let count = shortened.chars().count();
    if count <= limit {
        return shortened;
    }
    let tail: String = shortened
        .chars()
        .skip(count - limit.saturating_sub(3))
        .collect();
    format!("...{tail}")
}

#[derive(Resource, Default)]
/// The highlighted settings row. Public so the screenshot harness
/// can select a row below the fold.
pub struct SettingsCursor(pub usize);

/// Whether the cursor sits on the LIBRARY row (a dropped folder is
/// then the answer to "where?").
#[must_use]
pub(crate) fn library_row_selected(cursor: &SettingsCursor) -> bool {
    Row::ALL.get(cursor.0) == Some(&Row::LIBRARY)
}

/// Where the last in-app export landed (or why it did not). Shown
/// on the export row itself, so the answer sits where the question
/// was asked.
#[derive(Resource, Default)]
pub struct ExportNote(pub String);

/// Plugin for the settings screen.
pub struct SettingsUiPlugin;

impl Plugin for SettingsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsCursor>()
            .init_resource::<ExportNote>()
            .add_systems(OnEnter(AppState::Settings), spawn_settings)
            .add_systems(
                Update,
                (settings_input, refresh_settings, follow_settings_cursor)
                    .run_if(in_state(AppState::Settings)),
            )
            // The screen's entities go with the state (`DespawnOnExit`);
            // only the save is left to do on the way out.
            .add_systems(OnExit(AppState::Settings), persist_settings);
    }
}

/// The marker of the settings list's rows.
pub(crate) struct SettingsRows;

/// Keep the cursor row in view — the same measured whole-row window
/// the browser and the controls screen use. Seventeen rows outgrew
/// the safe area exactly the way fifteen did on the controls screen.
fn follow_settings_cursor(
    cursor: Res<SettingsCursor>,
    rows: Query<(&ListRow<SettingsRows>, &ComputedNode)>,
    mut lists: Query<(&mut ScrollPosition, &mut Node), With<ListPanel<SettingsRows>>>,
) {
    list::follow_cursor(cursor.0, Row::ALL.len(), &rows, &mut lists);
}

/// The line under the panel: the selected row's explanation.
#[derive(Component)]
struct SettingSubtitle;

/// The subtitle's query: it must exclude the value texts to satisfy
/// Bevy's aliasing rules.
type SubtitleText<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<SettingSubtitle>,
        Without<ListValue<SettingsRows>>,
        Without<ListLabel<SettingsRows>>,
    ),
>;

fn spawn_settings(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((DespawnOnExit(AppState::Settings), ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "SETTINGS", "sound, feel and looks");
            parent
                .spawn((
                    ListPanel::<SettingsRows>::new(),
                    ui_kit::scroll_panel(ui_kit::PANEL_WIDTH),
                ))
                .with_children(|panel| {
                    list::spawn_rows::<SettingsRows>(
                        panel,
                        &font,
                        Row::ALL.iter().map(|row| row.label()),
                    );
                });
            // The selected row's explanation, under the panel — the
            // pattern the song browser already uses for the same
            // job. ⚠️ NOT a second line inside the row: the panel's
            // scroll window is built from ONE measured row height
            // (`ui_kit::follow_list`), so a taller row is clipped by
            // the frame. The first attempt did exactly that and the
            // path came out cut in half.
            parent.spawn((
                SettingSubtitle,
                Text::new(""),
                // Data text: the watch folder is a path, and a path
                // in the display face's all-caps would be a lie.
                ui_kit::data_subtitle_text(&font),
                Node {
                    max_width: px(ui_kit::PANEL_WIDTH),
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  LEFT/RIGHT adjust  click left/right half  ESC back",
                "D-PAD choose and adjust  EAST back",
            );
            ui_kit::back_button(parent, &font, "MAIN MENU");
        });
}

/// What a key or click does on the selected row. The list renderer
/// reads the devices and moves the cursor; this decides what the row
/// does: a custom row's own Enter, a door, or a step of its value.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn settings_input(
    mut list: ListInput<SettingsRows>,
    mut cursor: ResMut<SettingsCursor>,
    mut settings: ResMut<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut export_note: ResMut<ExportNote>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut smart: ResMut<crate::smart_lyrics::SmartLyrics>,
    mut library_move: ResMut<crate::library_move::LibraryMove>,
) {
    let events = list.read(&mut cursor.0, Row::ALL.len());
    let row = Row::ALL[cursor.0];
    let activated = events.nav.confirm || events.clicked;
    let mut say = |sound| {
        sounds.write(sound);
    };
    match row.spec().action() {
        Some(Act::ExportHistory) if activated => {
            match crate::history::export_csv() {
                Ok(path) => {
                    say(crate::sfx::UiSound::Confirm);
                    // The path IS the feedback: an export that only
                    // says "done" leaves the player hunting for the file.
                    export_note.0 = path.display().to_string();
                }
                Err(reason) => {
                    say(crate::sfx::UiSound::Error);
                    export_note.0 = format!("export failed: {reason}");
                }
            }
            return;
        }
        Some(Act::Library) if activated => {
            library_move.confirm();
            say(crate::sfx::UiSound::Confirm);
            return;
        }
        Some(Act::Library) if events.nav.left => {
            library_move.decline();
            say(crate::sfx::UiSound::Back);
            return;
        }
        // The one explicit action that fetches anything (README):
        // nothing is downloaded until this row is confirmed.
        Some(Act::LyricsModel) if activated => {
            smart.confirm_model_row();
            say(crate::sfx::UiSound::Confirm);
            return;
        }
        Some(Act::Open(screen)) if activated || events.nav.right => {
            say(crate::sfx::UiSound::Confirm);
            next_state.set(screen);
            return;
        }
        _ => {}
    }
    let stepped = events
        .step
        .filter(|direction| row.spec().step(&mut settings, *direction))
        .map(|_| row.spec().feel());
    if let Some(sound) = list::sound_for(stepped, events.moved) {
        say(sound);
    }
    if ui_kit::wants_leave(
        events.nav.back,
        ui_kit::back_pressed(&mut back),
        events.right_click,
    ) {
        say(crate::sfx::UiSound::Back);
        next_state.set(AppState::MainMenu);
    }
}

fn refresh_settings(
    settings: Res<Settings>,
    cursor: Res<SettingsCursor>,
    mut paint: ListPaint<SettingsRows>,
    export_note: Res<ExportNote>,
    mut subtitles: SubtitleText,
    mut smart: ResMut<crate::smart_lyrics::SmartLyrics>,
    library_move: Res<crate::library_move::LibraryMove>,
) {
    // The model's standing is looked up the first time the screen
    // asks (a hash of the file, off the main thread); idempotent.
    smart.probe_model();
    if let Ok(mut text) = subtitles.single_mut() {
        let row = Row::ALL[cursor.0.min(Row::ALL.len() - 1)];
        let wanted = if row == Row::LYRICS_MODEL {
            smart.model_subtitle()
        } else {
            row.subtitle()
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    paint.paint(cursor.0, settings.high_contrast, |index| {
        let row = Row::ALL[index];
        match row.spec().action() {
            // The export row reports where the file went, once it has
            // written one - the answer belongs where the question was
            // asked, not in a log nobody reads.
            Some(Act::ExportHistory) if !export_note.0.is_empty() => export_note.0.clone(),
            Some(Act::LyricsModel) => smart.model_text(),
            Some(Act::Library) => library_move.value(),
            _ => row.value(&settings),
        }
    });
}

fn persist_settings(settings: Res<Settings>) {
    save_settings(&settings);
}

#[cfg(test)]
mod tests {
    use super::{SUBTITLE_CHARS, short_path};

    #[test]
    fn a_long_path_keeps_its_end() {
        // The tail says what the head never does: ".../beat-byte/
        // songs" answers "which folder", a truncated head does not.
        let long = "/Users/someone/very/deeply/nested/place/that/keeps/going/beat-byte/songs";
        let shown = short_path(long, 30);
        assert!(shown.starts_with("..."), "the FRONT is what goes: {shown}");
        assert!(shown.ends_with("beat-byte/songs"), "the tail survives");
        assert_eq!(shown.chars().count(), 30);
        // Short enough already: left exactly alone.
        assert_eq!(short_path("/songs", 30), "/songs");
    }

    #[test]
    fn the_home_directory_collapses_first() {
        // Shortening before abbreviating: "~" is both shorter AND
        // more readable than an ellipsis over the same characters.
        let Some(home) = dirs::home_dir() else {
            return; // no home on this machine (CI)
        };
        let path = home.join("Music").join("songs").display().to_string();
        assert!(short_path(&path, 60).starts_with("~/"), "home becomes ~");
    }

    #[test]
    fn the_flash_sync_row_names_both_clocks_and_flips_either_way() {
        use crate::config::FlashSync;
        let mut settings = Settings::default();
        assert_eq!(
            settings.flash_sync,
            FlashSync::Level,
            "the room's level is what the light show has always followed"
        );
        assert_eq!(Row::FLASH_SYNC.label(), "FLASH SYNC");
        assert_eq!(Row::FLASH_SYNC.value(&settings), "ROOM LEVEL");
        Row::FLASH_SYNC.adjust(&mut settings, 1.0);
        assert_eq!(settings.flash_sync, FlashSync::Beat);
        assert_eq!(Row::FLASH_SYNC.value(&settings), "SONG BEAT");
        // Two values: either direction is the other one.
        Row::FLASH_SYNC.adjust(&mut settings, -1.0);
        assert_eq!(settings.flash_sync, FlashSync::Level);
    }

    #[test]
    fn a_subtitle_fits_the_panel() {
        // Press Start 2P advances a full em, so the widest subtitle
        // is a plain multiplication - and it has to fit inside the
        // panel's padding, or the path runs under the frame.
        let widest = SUBTITLE_CHARS as f32 * crate::ui_kit::SMALL;
        let inner = crate::ui_kit::PANEL_WIDTH - 2.0 * crate::ui_kit::PANEL_PAD - 2.0 * 14.0;
        assert!(
            widest <= inner,
            "{widest} px of subtitle in {inner} px of panel"
        );
    }

    #[test]
    fn only_a_row_that_needs_explaining_carries_a_subtitle() {
        // A subtitle on every row would be noise. Two rows earn
        // one: "which folder delivers my tracks" is a question the
        // value line ("watching: …") does not answer, and neither
        // ROOM LEVEL nor SONG BEAT says on its own what it costs.
        use super::Row;
        assert!(!Row::WATCH_FOLDER.subtitle().is_empty());
        assert!(!Row::FLASH_SYNC.subtitle().is_empty());
        assert!(
            Row::FLASH_SYNC.subtitle().chars().count() <= SUBTITLE_CHARS,
            "the subtitle runs past the panel: {}",
            Row::FLASH_SYNC.subtitle()
        );
        for row in [Row::MUSIC_VOLUME, Row::LYRICS, Row::CONTROLS, Row::THEME] {
            assert!(row.subtitle().is_empty(), "{row:?} should stay quiet");
        }
    }

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
        step(Row::MUSIC_VOLUME, &mut settings, -1.0, 50);
        assert!((0.0..=1.0).contains(&settings.music_volume));
        step(Row::MUSIC_VOLUME, &mut settings, 1.0, 50);
        assert!((0.0..=1.0).contains(&settings.music_volume));
        step(Row::SFX_VOLUME, &mut settings, -1.0, 50);
        assert!((0.0..=1.0).contains(&settings.sfx_volume));
    }

    #[test]
    fn scroll_speed_and_latency_stay_playable() {
        let mut settings = Settings::default();
        step(Row::SCROLL_SPEED, &mut settings, -1.0, 100);
        assert!((240.0..=900.0).contains(&settings.scroll_speed));
        step(Row::SCROLL_SPEED, &mut settings, 1.0, 100);
        assert!((240.0..=900.0).contains(&settings.scroll_speed));
        step(Row::LATENCY_OFFSET, &mut settings, 1.0, 200);
        assert!((-250.0..=250.0).contains(&settings.latency_offset_ms));
        step(Row::LATENCY_OFFSET, &mut settings, -1.0, 200);
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
                Row::THEME.adjust(&mut settings, direction);
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
            Row::THEME.adjust(&mut settings, 1.0);
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
    fn the_rows_are_alphabetical_and_stay_that_way() {
        // User, 2026-09-03: "settings alphabetisch sortieren und
        // sortiert halten wenn neue einträge hinzukommen." The list
        // is the display order; this is what keeps it sorted when the
        // next row is added — it names the pair that is out of place.
        for pair in Row::ALL.windows(2) {
            assert!(
                pair[0].label() <= pair[1].label(),
                "settings rows out of order: {:?} is listed before {:?} but sorts after it — \
                 move it to where its name falls",
                pair[0].label(),
                pair[1].label()
            );
        }
        // And nothing is listed twice. The array's own type already
        // fixes its length, so a count here only ever needed editing
        // when a row was added — it never caught anything. Duplicate
        // labels it does catch, and a duplicate is what a
        // copy-pasted row actually looks like.
        let mut labels: Vec<&str> = Row::ALL.iter().map(|row| row.label()).collect();
        labels.sort_unstable();
        let unique = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), unique, "a settings row is listed twice");
    }

    #[test]
    fn toggles_flip_and_report_themselves() {
        let mut settings = Settings::default();
        for row in [
            Row::PARTICLES,
            Row::SCREEN_SHAKE,
            Row::BEAT_PULSE,
            Row::LOUDNESS_MATCH,
            Row::GUITAR_STUDY_TWINS,
            Row::BACKDROP_MOTION,
            Row::HIT_LABELS,
            Row::NO_FAIL,
            Row::TAP_MODE,
            Row::FULLSCREEN,
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
        Row::CONTROLS.adjust(&mut settings, 1.0);
        Row::CONTROLS.adjust(&mut settings, -1.0);
        assert_eq!(
            format!("{before:?}"),
            format!("{settings:?}"),
            "the CONTROLS row changed a setting"
        );
    }

    /// Which top-level fields of `Settings` differ between two values,
    /// read off their pretty `Debug` form (one `name: value` line per
    /// field at the first indentation).
    fn changed_fields(before: &Settings, after: &Settings) -> Vec<String> {
        let fields = |s: &Settings| -> Vec<(String, String)> {
            let text = format!("{s:#?}");
            let mut out: Vec<(String, String)> = Vec::new();
            for line in text.lines().skip(1) {
                if let Some(rest) = line.strip_prefix("    ")
                    && !rest.starts_with(' ')
                    && let Some((name, value)) = rest.split_once(':')
                {
                    out.push((name.to_owned(), value.to_owned()));
                } else if let Some(last) = out.last_mut() {
                    last.1.push_str(line);
                }
            }
            out
        };
        let (a, b) = (fields(before), fields(after));
        a.iter()
            .zip(&b)
            .filter(|(x, y)| x != y)
            .map(|(x, _)| x.0.clone())
            .collect()
    }

    /// The table binds every row to a field through a small closure,
    /// and a copy-pasted entry bound to its neighbour's field would
    /// still compile and still flip SOMETHING. So: every row that
    /// steps changes exactly one field, and no two rows the same one.
    #[test]
    fn every_row_edits_one_field_of_its_own() {
        let mut owner: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for row in Row::ALL {
            let base = Settings::default();
            let mut changed = Vec::new();
            for direction in [1.0, -1.0] {
                let mut edited = base.clone();
                row.adjust(&mut edited, direction);
                changed = changed_fields(&base, &edited);
                if !changed.is_empty() {
                    break;
                }
            }
            match row.spec().kind {
                Kind::Door(_) => assert!(
                    changed.is_empty(),
                    "{row:?} is a door and edited {changed:?}"
                ),
                Kind::Custom { adjust: None, .. } => {
                    assert!(changed.is_empty(), "{row:?} edited {changed:?}");
                }
                // Clearing the watched folder changes nothing when no
                // folder is set; set one and clear it.
                Kind::Custom {
                    adjust: Some(_), ..
                } => {
                    let mut set = Settings {
                        watch_folder: Some(std::path::PathBuf::from("/x")),
                        ..Settings::default()
                    };
                    let before = set.clone();
                    row.adjust(&mut set, 1.0);
                    let changed = changed_fields(&before, &set);
                    assert_eq!(changed, ["watch_folder"], "{row:?}");
                }
                _ => {
                    assert_eq!(changed.len(), 1, "{row:?} changed {changed:?}");
                    if let Some(other) = owner.insert(changed[0].clone(), row.label()) {
                        panic!("{} and {other} both edit `{}`", row.label(), changed[0]);
                    }
                }
            }
        }
        // The probe itself: it must see a change where there is one.
        let louder = Settings {
            music_volume: 0.1,
            ..Settings::default()
        };
        assert_eq!(
            changed_fields(&Settings::default(), &louder),
            ["music_volume"]
        );
    }

    /// A switch clicks and a dial ticks. Two rows changed voice when
    /// the table took over: VOCAL CHARTS was the one toggle missing
    /// from the old hand-kept list of clicking rows, and VOCAL PITCH
    /// is a two-way choice like FLASH SYNC, which already clicked.
    #[test]
    fn a_switch_clicks_and_a_dial_ticks() {
        use crate::sfx::UiSound;
        for row in [
            Row::VOCAL_CHARTS,
            Row::VOCAL_PITCH,
            Row::FLASH_SYNC,
            Row::HIT_LABELS,
            Row::AI_SEARCH,
        ] {
            assert_eq!(
                list::sound_for(Some(row.spec().feel()), false),
                Some(UiSound::Toggle),
                "{row:?}"
            );
        }
        for row in [
            Row::MUSIC_VOLUME,
            Row::THEME,
            Row::TELEMETRY,
            Row::LYRICS_SIZE,
        ] {
            assert_eq!(
                list::sound_for(Some(row.spec().feel()), false),
                Some(UiSound::Slider),
                "{row:?}"
            );
        }
    }

    /// TELEMETRY used to cycle forward on LEFT too; as a choice it
    /// steps back on LEFT like every other row.
    #[test]
    fn telemetry_steps_both_ways() {
        let mut settings = Settings::default();
        let start = settings.telemetry;
        Row::TELEMETRY.adjust(&mut settings, -1.0);
        assert_ne!(settings.telemetry, start);
        Row::TELEMETRY.adjust(&mut settings, 1.0);
        assert_eq!(settings.telemetry, start, "left then right comes back");
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
