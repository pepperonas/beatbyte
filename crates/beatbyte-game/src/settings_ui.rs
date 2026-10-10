//! The settings screen: volumes, scroll speed, latency offset,
//! effect toggles, fullscreen. Changes apply immediately and persist
//! on leaving the screen.

use bevy::prelude::*;

use crate::config::{
    FlashSync, MissColor, MissEffect, MissSound, Settings, TelemetryLevel, save_settings,
};
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
    /// Replay the configured miss sound.
    PreviewMiss,
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
    pub(crate) const MISS_COLOR: Row = row!(
        "MISS COLOR",
        Kind::Choice {
            labels: || [
                "STANDARD", "RED", "WHITE", "ORANGE", "YELLOW", "GREEN", "CYAN", "BLUE", "VIOLET"
            ]
            .iter()
            .map(|label| (*label).to_owned())
            .collect(),
            get: |s: &Settings| MissColor::ALL
                .iter()
                .position(|color| *color == s.miss_color)
                .unwrap_or(0),
            set: |s: &mut Settings, i| s.miss_color = MissColor::ALL[i],
            ends: Ends::Wrap,
        },
        Subtitle::Text("colour of the selected visual miss effect")
    );
    pub(crate) const MISS_PREVIEW: Row = row!(
        "MISS SOUND PREVIEW",
        Kind::Door(Act::PreviewMiss),
        Subtitle::Text("replay selected miss sound at its current volume; 0% is silent")
    );
    pub(crate) const MISS_EFFECT: Row = row!(
        "MISS EFFECT",
        Kind::Choice {
            labels: || vec![
                "SCREEN OVERLAY".to_owned(),
                "SCREEN FLASH".to_owned(),
                "BORDER FLASH".to_owned(),
                "HIGHWAY FLASH".to_owned(),
                "CEILING PULSE".to_owned(),
            ],
            get: |s: &Settings| match s.miss_effect {
                MissEffect::RedOverlay => 0,
                MissEffect::WhiteFlash => 1,
                MissEffect::BorderFlash => 2,
                MissEffect::HighwayFlash => 3,
                MissEffect::CeilingStrobe => 4,
            },
            set: |s: &mut Settings, i| {
                s.miss_effect = match i {
                    1 => MissEffect::WhiteFlash,
                    2 => MissEffect::BorderFlash,
                    3 => MissEffect::HighwayFlash,
                    4 => MissEffect::CeilingStrobe,
                    _ => MissEffect::RedOverlay,
                };
            },
            ends: Ends::Wrap,
        },
        Subtitle::Text("visual miss target; ceiling needs 3D, otherwise uses the screen border")
    );
    pub(crate) const MISS_INTENSITY: Row = row!(
        "MISS INTENSITY",
        slider!(miss_intensity, 0.0, 1.0, 0.1, Percent),
        Subtitle::Text("visual miss effect strength; 0% disables the effect and miss shake")
    );
    pub(crate) const MISS_SOUND: Row = row!(
        "MISS SOUND",
        Kind::Choice {
            labels: || vec![
                "SYNTH DULL".to_owned(),
                "SYNTH BUZZ".to_owned(),
                "ERROR BUZZ".to_owned(),
                "DESCEND".to_owned(),
                "DENIED".to_owned(),
                "KEY ERROR".to_owned(),
                "VINYL CLICK".to_owned(),
                "SCRATCH CHOP".to_owned(),
                "BASS MUTE".to_owned(),
            ],
            get: |s: &Settings| match s.miss_sound {
                MissSound::SynthDull => 0,
                MissSound::SynthBuzz => 1,
                MissSound::ErrorBuzz => 2,
                MissSound::Descend => 3,
                MissSound::Denied => 4,
                MissSound::KeyError => 5,
                MissSound::VinylClick => 6,
                MissSound::ScratchChop => 7,
                MissSound::BassMute => 8,
            },
            set: |s: &mut Settings, i| {
                s.miss_sound = match i {
                    1 => MissSound::SynthBuzz,
                    2 => MissSound::ErrorBuzz,
                    3 => MissSound::Descend,
                    4 => MissSound::Denied,
                    5 => MissSound::KeyError,
                    6 => MissSound::VinylClick,
                    7 => MissSound::ScratchChop,
                    8 => MissSound::BassMute,
                    _ => MissSound::SynthDull,
                };
            },
            ends: Ends::Wrap,
        },
        Subtitle::Text("LEFT/RIGHT select; ENTER or controller confirm replays the selected tone")
    );
    pub(crate) const MISS_VOLUME: Row = row!(
        "MISS VOLUME",
        slider!(miss_volume, 0.0, 1.0, 0.1, Percent),
        Subtitle::Text("miss sound volume, also scaled by SFX VOLUME; 0% disables; ENTER previews")
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
    pub(crate) const PERFORMANCE_MODE: Row = row!(
        "PERFORMANCE MODE",
        toggle!(performance_mode),
        Subtitle::Text("simpler lighting and effects for smoother play")
    );
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
    pub(crate) const ALL: [Row; 47] = [
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
        Row::MISS_COLOR,
        Row::MISS_EFFECT,
        Row::MISS_INTENSITY,
        Row::MISS_SOUND,
        Row::MISS_PREVIEW,
        Row::MISS_VOLUME,
        Row::MUSIC_VOLUME,
        Row::NO_FAIL,
        Row::ORIGINAL_VOCALS,
        Row::PARTICLES,
        Row::PERFORMANCE_MODE,
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

    /// These adjustments should audition the selected miss tone.
    pub(crate) fn previews_miss(self) -> bool {
        self == Self::MISS_SOUND || self == Self::MISS_VOLUME || self == Self::SFX_VOLUME
    }

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
        if self == Self::MISS_PREVIEW {
            return "PLAY".to_owned();
        }
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

/// The tabs are tasks; ALL retains the stable alphabetical row identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Play,
    Audio,
    Feedback,
    Stage,
    Vocals,
    Library,
    System,
    All,
    Changed,
}

impl Category {
    const ALL: [Self; 9] = [
        Self::Play,
        Self::Audio,
        Self::Feedback,
        Self::Stage,
        Self::Vocals,
        Self::Library,
        Self::System,
        Self::All,
        Self::Changed,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Play => "PLAY & INPUT",
            Self::Audio => "AUDIO",
            Self::Feedback => "MISS FEEDBACK",
            Self::Stage => "STAGE",
            Self::Vocals => "LYRICS & VOCALS",
            Self::Library => "LIBRARY",
            Self::System => "SYSTEM & DATA",
            Self::All => "ALL",
            Self::Changed => "CHANGED",
        }
    }
    fn rows(self) -> &'static [Row] {
        match self {
            Self::Play => &[
                Row::SCROLL_SPEED,
                Row::TAP_MODE,
                Row::NO_FAIL,
                Row::HIT_LABELS,
                Row::CONTROLS,
                Row::INPUT_TEST,
                Row::CALIBRATION,
                Row::LATENCY_OFFSET,
                Row::VIDEO_OFFSET,
            ],
            Self::Audio => &[
                Row::MUSIC_VOLUME,
                Row::SFX_VOLUME,
                Row::LOUDNESS_MATCH,
                Row::SONG_PREVIEW,
            ],
            Self::Feedback => &[
                Row::MISS_SOUND,
                Row::MISS_VOLUME,
                Row::MISS_PREVIEW,
                Row::MISS_EFFECT,
                Row::MISS_COLOR,
                Row::MISS_INTENSITY,
            ],
            Self::Stage => &[
                Row::THEME,
                Row::BACKDROP_MOTION,
                Row::BEAT_PULSE,
                Row::FX_INTENSITY,
                Row::PARTICLES,
                Row::SCREEN_SHAKE,
                Row::ROOM_LIGHTS,
                Row::FLASH_SYNC,
            ],
            Self::Vocals => &[
                Row::LYRICS,
                Row::LYRICS_SIZE,
                Row::LYRICS_LEAD_IN,
                Row::LYRICS_OFFSET,
                Row::LYRICS_MODEL,
                Row::VOCAL_CHARTS,
                Row::VOCAL_PITCH,
                Row::ORIGINAL_VOCALS,
                Row::MIC_OFFSET,
            ],
            Self::Library => &[
                Row::LIBRARY,
                Row::WATCH_FOLDER,
                Row::AI_SEARCH,
                Row::GUITAR_STUDY_TWINS,
            ],
            Self::System => &[
                Row::FULLSCREEN,
                Row::TEXT_SCALE,
                Row::HIGH_CONTRAST,
                Row::REDUCED_FLASHING,
                Row::PERFORMANCE_MODE,
                Row::TELEMETRY,
                Row::EXPORT_HISTORY,
            ],
            Self::All | Self::Changed => &Row::ALL,
        }
    }
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|category| *category == self)
            .unwrap_or(0)
    }
    fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| {
            category.label().eq_ignore_ascii_case(label)
                || format!("{category:?}").eq_ignore_ascii_case(label)
        })
    }
}

/// Search focus is exposed to the global mute shortcut.
#[derive(Resource)]
pub(crate) struct SettingsView {
    category: Category,
    query: String,
    pub(crate) editing: bool,
    order: Vec<usize>,
    remembered: [usize; 9],
}

impl Default for SettingsView {
    fn default() -> Self {
        let remembered = Category::ALL.map(|category| category.rows()[0].index());
        Self {
            category: Category::Play,
            query: String::new(),
            editing: false,
            order: Vec::new(),
            remembered,
        }
    }
}

impl SettingsView {
    fn position(&self, cursor: usize) -> usize {
        self.order
            .iter()
            .position(|index| *index == cursor)
            .unwrap_or(0)
    }
    fn rebuild(&mut self, settings: &Settings, cursor: &mut usize) {
        let defaults = Settings::default();
        let tokens: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let source = if tokens.is_empty() {
            self.category.rows()
        } else {
            &Row::ALL
        };
        let wanted: Vec<usize> = source
            .iter()
            .filter(|row| {
                if self.category == Category::Changed && !row.spec().differs(settings, &defaults) {
                    return false;
                }
                if tokens.is_empty() {
                    return true;
                }
                let haystack = format!("{} {}", row.label(), row.subtitle()).to_lowercase();
                tokens.iter().all(|token| haystack.contains(token))
            })
            .map(|row| row.index())
            .collect();
        if wanted != self.order {
            self.order = wanted;
        }
        if !self.order.contains(cursor)
            && let Some(first) = self.order.first()
        {
            *cursor = *first;
        }
    }
    fn switch(&mut self, category: Category, settings: &Settings, cursor: &mut usize) {
        self.remembered[self.category.index()] = *cursor;
        self.category = category;
        self.query.clear();
        self.editing = false;
        *cursor = self.remembered[category.index()];
        self.rebuild(settings, cursor);
    }
}

#[derive(Component)]
struct CategoryChip(Category);
#[derive(Component)]
struct SettingsSearch;
#[derive(Component)]
enum PreviewButton {
    Sound,
    Visual,
}

#[derive(Resource, Default)]
struct VisualAudition {
    age: f32,
}

#[derive(Component)]
struct SampleFlash;

#[derive(Component)]
struct SampleFrame;

/// Where the last in-app export landed (or why it did not). Shown
/// on the export row itself, so the answer sits where the question
/// was asked.
#[derive(Resource, Default)]
pub struct ExportNote(pub String);

/// Plugin for the settings screen.
pub struct SettingsUiPlugin;

impl Plugin for SettingsUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<crate::sfx::PreviewMiss>()
            .init_resource::<SettingsCursor>()
            .init_resource::<SettingsView>()
            .init_resource::<VisualAudition>()
            .init_resource::<SettingsWindow>()
            .init_resource::<ExportNote>()
            .add_systems(OnEnter(AppState::Settings), spawn_settings)
            .add_systems(
                Update,
                (
                    settings_input.in_set(crate::sfx::MissPreviewInput),
                    follow_settings_cursor,
                    refresh_settings,
                    paint_settings_chrome,
                    animate_visual_audition,
                )
                    .chain()
                    .run_if(in_state(AppState::Settings)),
            )
            // The screen's entities go with the state (`DespawnOnExit`);
            // only the save is left to do on the way out.
            .add_systems(OnExit(AppState::Settings), persist_settings);
    }
}

/// The marker of the settings list's rows.
pub(crate) struct SettingsRows;

/// A fixed whole-row window, independent of the number of settings.
#[derive(Resource, Clone, Copy, PartialEq)]
struct SettingsWindow {
    top: usize,
    visible: usize,
    height: f32,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            top: 0,
            visible: 12,
            height: crate::song_browser_view::BODY_H,
        }
    }
}

impl SettingsWindow {
    fn follow(&mut self, cursor: usize, row_h: f32, inverse_scale: f32) {
        let row_h = row_h * inverse_scale;
        if row_h > 0.0 {
            self.height = ui_kit::whole_rows_height(
                row_h,
                ui_kit::ROW_GAP,
                usize::MAX,
                crate::song_browser_view::BODY_H,
            )
            .unwrap_or(crate::song_browser_view::BODY_H);
            let inner = self.height - 2.0 * (ui_kit::PANEL_PAD + ui_kit::PANEL_BORDER);
            self.visible = ((inner + ui_kit::ROW_GAP) / (row_h + ui_kit::ROW_GAP))
                .floor()
                .max(1.0) as usize;
        }
        self.top =
            crate::song_browser_view::window_top(cursor, self.top, self.visible, Row::ALL.len());
    }

    #[cfg(test)]
    fn range(self) -> std::ops::Range<usize> {
        self.top..(self.top + self.visible).min(Row::ALL.len())
    }
}

#[derive(Component)]
struct SettingsBodyPanel;

fn follow_settings_cursor(
    cursor: Res<SettingsCursor>,
    mut window: ResMut<SettingsWindow>,
    view: Res<SettingsView>,
    rows: Query<&ComputedNode, With<ListRow<SettingsRows>>>,
    mut panels: Query<&mut Node, With<SettingsBodyPanel>>,
) {
    let mut wanted = *window;
    let measured = rows.iter().find(|node| node.size().y > 0.0);
    wanted.follow(
        view.position(cursor.0),
        measured.map_or(0.0, |row| row.size().y),
        measured.map_or(1.0, ComputedNode::inverse_scale_factor),
    );
    wanted.top = crate::song_browser_view::window_top(
        view.position(cursor.0),
        window.top,
        wanted.visible,
        view.order.len(),
    );
    if wanted != *window {
        *window = wanted;
    }
    for mut panel in &mut panels {
        if panel.height != px(wanted.height) {
            panel.height = px(wanted.height);
            panel.min_height = px(wanted.height);
            panel.max_height = px(wanted.height);
        }
    }
}

#[derive(Component)]
enum SettingDetail {
    Label,
    Value,
    Explanation,
    Position,
    Dependencies,
}

type DetailText<'w, 's> = Query<
    'w,
    's,
    (&'static SettingDetail, &'static mut Text),
    (
        Without<ListValue<SettingsRows>>,
        Without<ListLabel<SettingsRows>>,
    ),
>;

fn settings_body_node(width: f32) -> Node {
    let mut node = ui_kit::scroll_node(width);
    node.height = px(crate::song_browser_view::BODY_H);
    node.min_height = node.height;
    node.max_height = node.height;
    // The row parent may narrow both columns; their fixed min/max height stays intact.
    node.flex_shrink = 1.0;
    node.overflow = Overflow::clip();
    node
}

fn spawn_settings(
    mut commands: Commands,
    font: Res<UiFont>,
    mut view: ResMut<SettingsView>,
    mut cursor: ResMut<SettingsCursor>,
    settings: Res<Settings>,
) {
    if std::env::var_os("BEATBYTE_AUTOPILOT_MODEL").is_some()
        || std::env::var_os("BEATBYTE_SHOT_ROW").is_some()
    {
        view.category = Category::All;
    }
    if let Ok(label) = std::env::var("BEATBYTE_SHOT_VIEW")
        && let Some(category) = Category::from_label(&label)
    {
        view.category = category;
    }
    if let Ok(query) = std::env::var("BEATBYTE_SHOT_SEARCH") {
        view.query = query;
    }
    view.rebuild(&settings, &mut cursor.0);
    commands.insert_resource(SettingsWindow::default());
    commands
        .spawn((DespawnOnExit(AppState::Settings), ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "SETTINGS", "sound, feel and looks");
            parent
                .spawn(Node {
                    width: percent(95),
                    max_width: px(ui_kit::PANEL_WIDE),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(4),
                    margin: UiRect::bottom(px(8)),
                    ..default()
                })
                .with_children(|tabs| {
                    for category in Category::ALL {
                        let (fg, border, fill) =
                            ui_kit::selection_chip_colours(category == view.category, false);
                        tabs.spawn((
                            CategoryChip(category),
                            Button,
                            Text::new(category.label()),
                            font.text(ui_kit::SMALL),
                            TextColor(fg),
                            ui_kit::selection_chip_node(),
                            BorderColor::all(border),
                            BackgroundColor(fill),
                        ));
                    }
                });
            parent.spawn((
                SettingsSearch,
                Button,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(crate::palette::TEXT_DIM),
                Node {
                    width: percent(95),
                    max_width: px(ui_kit::PANEL_WIDE),
                    margin: UiRect::bottom(px(10)),
                    ..default()
                },
            ));
            parent
                .spawn(Node {
                    width: percent(95),
                    max_width: px(ui_kit::PANEL_WIDE),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(20),
                    ..default()
                })
                .with_children(|body| {
                    let frame = || {
                        (
                            BackgroundColor(crate::palette::SURFACE.with_alpha(0.55)),
                            BorderColor::all(crate::palette::dimmed(crate::palette::TEXT_DIM, 0.3)),
                        )
                    };
                    body.spawn((
                        SettingsBodyPanel,
                        ListPanel::<SettingsRows>::new(),
                        settings_body_node(crate::song_browser_view::LIST_W),
                        frame(),
                    ))
                    .with_children(|panel| {
                        list::spawn_fixed_rows::<SettingsRows>(
                            panel,
                            &font,
                            Row::ALL.iter().map(|row| row.label()),
                        );
                    });
                    body.spawn((
                        SettingsBodyPanel,
                        settings_body_node(crate::song_browser_view::PANEL_W),
                        frame(),
                    ))
                    .with_children(|panel| {
                        panel.spawn((
                            Text::new("SELECTED SETTING"),
                            font.text(ui_kit::SMALL),
                            TextColor(crate::palette::TEXT_DIM),
                        ));
                        panel.spawn((
                            SettingDetail::Label,
                            Text::new(""),
                            font.text(ui_kit::ROW),
                            TextColor(crate::palette::BRAND),
                            Node {
                                margin: UiRect::top(px(12)),
                                ..default()
                            },
                        ));
                        panel.spawn((
                            SettingDetail::Value,
                            Text::new(""),
                            font.text(ui_kit::ROW),
                            TextColor(crate::palette::TEXT),
                            Node {
                                margin: UiRect::top(px(8)),
                                ..default()
                            },
                        ));
                        panel.spawn((
                            SettingDetail::Explanation,
                            Text::new(""),
                            font.mono_text(ui_kit::SMALL),
                            TextColor(ui_kit::dimmed_subtitle()),
                            Node {
                                margin: UiRect::top(px(20)),
                                ..default()
                            },
                        ));
                        let (fg, border, fill) = ui_kit::selection_chip_colours(false, false);
                        panel.spawn((
                            PreviewButton::Sound,
                            Button,
                            Text::new("PLAY SOUND  ENTER"),
                            font.text(ui_kit::SMALL),
                            TextColor(fg),
                            ui_kit::selection_chip_node(),
                            BorderColor::all(border),
                            BackgroundColor(fill),
                        ));
                        panel.spawn((
                            SettingDetail::Dependencies,
                            Text::new(""),
                            font.mono_text(ui_kit::SMALL),
                            TextColor(ui_kit::dimmed_subtitle()),
                        ));
                        spawn_visual_sample(panel, &font);
                    });
                });
            parent.spawn((
                SettingDetail::Position,
                Text::new(""),
                font.text(ui_kit::SMALL),
                TextColor(crate::palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  LEFT/RIGHT adjust  BACKSPACE default  ESC back",
                "D-PAD choose and adjust  EAST back",
            );
            ui_kit::back_button(parent, &font, "MAIN MENU");
        });
}

#[derive(bevy::ecs::system::SystemParam)]
struct SettingsBrowserInput<'w, 's> {
    view: ResMut<'w, SettingsView>,
    typed: MessageReader<'w, 's, bevy::input::keyboard::KeyboardInput>,
    tabs: Query<'w, 's, (&'static CategoryChip, &'static Interaction), Changed<Interaction>>,
    search: Query<'w, 's, &'static Interaction, (With<SettingsSearch>, Changed<Interaction>)>,
    preview: Query<'w, 's, (&'static PreviewButton, &'static Interaction), Changed<Interaction>>,
    audition: ResMut<'w, VisualAudition>,
    held: Option<Res<'w, list::HeldRow>>,
    entered: Query<'w, 's, (), Added<ListRow<SettingsRows>>>,
    pads: Query<'w, 's, &'static bevy::input::gamepad::Gamepad>,
}

/// What a key or click does on the selected row. The list renderer
/// reads the devices and moves the cursor; this decides what the row
/// does: a custom row's own Enter, a door, or a step of its value.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI
fn settings_input(
    mut list: ListInput<SettingsRows>,
    browser: SettingsBrowserInput,
    mut cursor: ResMut<SettingsCursor>,
    mut settings: ResMut<Settings>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
    mut previews: MessageWriter<crate::sfx::PreviewMiss>,
    mut export_note: ResMut<ExportNote>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut smart: ResMut<crate::smart_lyrics::SmartLyrics>,
    mut library_move: ResMut<crate::library_move::LibraryMove>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let SettingsBrowserInput {
        mut view,
        mut typed,
        tabs,
        search,
        preview,
        mut audition,
        held,
        entered,
        pads,
    } = browser;

    if !entered.is_empty() {
        typed.clear();
    }
    if let Some((chip, _)) = tabs
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
    {
        view.switch(chip.0, &settings, &mut cursor.0);
        typed.clear();
    }
    let command = crate::editor_ui::command_held(&keys);
    let open_search = (command && keys.just_pressed(KeyCode::KeyF))
        || keys.just_pressed(KeyCode::Slash)
        || search
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed);
    if open_search {
        view.editing = true;
        typed.clear();
    }
    if view.editing {
        if keys.just_pressed(KeyCode::Escape) {
            view.query.clear();
            view.editing = false;
        } else if keys.just_pressed(KeyCode::Enter) {
            view.editing = false;
        } else if !command {
            for event in typed.read().filter(|event| event.state.is_pressed()) {
                match &event.logical_key {
                    bevy::input::keyboard::Key::Character(text) => {
                        view.query.extend(text.chars().filter(|c| !c.is_control()))
                    }
                    bevy::input::keyboard::Key::Space => view.query.push(' '),
                    bevy::input::keyboard::Key::Backspace => {
                        view.query.pop();
                    }
                    _ => {}
                }
            }
        } else {
            typed.clear();
        }
        view.rebuild(&settings, &mut cursor.0);
        let mut position = view.position(cursor.0);
        let _ = list.read_unheld(&mut position, view.order.len());
        return;
    }
    typed.clear();
    let previous = pads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::LeftTrigger));
    let next = pads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::RightTrigger));
    if keys.just_pressed(KeyCode::Tab) || previous || next {
        let direction =
            if previous || keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                -1
            } else {
                1
            };
        let index = (view.category.index() as i32 + direction)
            .rem_euclid(Category::ALL.len() as i32) as usize;
        view.switch(Category::ALL[index], &settings, &mut cursor.0);
        let mut position = view.position(cursor.0);
        let _ = list.read_unheld(&mut position, view.order.len());
        return;
    }
    view.rebuild(&settings, &mut cursor.0);
    let mut position = view.position(cursor.0);
    let events = list.read_unheld(&mut position, view.order.len());
    if let Some(held) = held
        && let Some(index) = view.order.iter().position(|index| *index == held.0)
    {
        position = index;
    }
    if ui_kit::wants_leave(
        events.nav.back,
        ui_kit::back_pressed(&mut back),
        events.right_click,
    ) {
        if !view.query.is_empty() {
            view.query.clear();
            view.rebuild(&settings, &mut cursor.0);
        } else {
            sounds.write(crate::sfx::UiSound::Back);
            next_state.set(AppState::MainMenu);
        }
        return;
    }
    let Some(global) = view.order.get(position).copied() else {
        return;
    };
    cursor.0 = global;
    let category_index = view.category.index();
    view.remembered[category_index] = global;
    let row = Row::ALL[global];
    if let Some((button, _)) = preview
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
    {
        match button {
            PreviewButton::Sound => {
                previews.write(crate::sfx::PreviewMiss);
            }
            PreviewButton::Visual => {
                audition.age = 0.0;
            }
        }
        return;
    }
    let activated = events.nav.confirm || events.clicked;
    let mut say = |sound| {
        sounds.write(sound);
    };
    if events.nav.confirm && row.previews_miss() {
        previews.write(crate::sfx::PreviewMiss);
        return;
    }
    match row.spec().action() {
        Some(Act::PreviewMiss) if activated => {
            previews.write(crate::sfx::PreviewMiss);
            return;
        }
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
    // Backspace puts the row back to the shipped default — the value a
    // player would otherwise have to remember. A row without a default
    // (a door, a custom row) ignores it.
    if keys.just_pressed(KeyCode::Backspace) {
        if row.spec().reset(&mut settings, &Settings::default()) {
            if row.previews_miss() {
                previews.write(crate::sfx::PreviewMiss);
            } else {
                say(crate::sfx::UiSound::Toggle);
            }
        }
        return;
    }
    let stepped = events
        .step
        .filter(|direction| row.spec().step(&mut settings, *direction))
        .map(|_| row.spec().feel());
    if stepped.is_some() && row.previews_miss() {
        previews.write(crate::sfx::PreviewMiss);
    } else if let Some(sound) = list::sound_for(stepped, events.moved) {
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

#[allow(clippy::too_many_arguments)] // Bevy system: parameters are world handles
fn refresh_settings(
    settings: Res<Settings>,
    cursor: Res<SettingsCursor>,
    mut paint: ListPaint<SettingsRows>,
    export_note: Res<ExportNote>,
    mut details: DetailText,
    window: Res<SettingsWindow>,
    view: Res<SettingsView>,
    mut smart: ResMut<crate::smart_lyrics::SmartLyrics>,
    library_move: Res<crate::library_move::LibraryMove>,
    fresh: Query<(), Added<ListRow<SettingsRows>>>,
) {
    // The model's standing is looked up the first time the screen
    // asks (a hash of the file, off the main thread); idempotent.
    smart.probe_model();
    if fresh.is_empty()
        && !settings.is_changed()
        && !cursor.is_changed()
        && !window.is_changed()
        && !view.is_changed()
        && !export_note.is_changed()
        && !smart.is_changed()
        && !library_move.is_changed()
    {
        return;
    }
    let defaults = Settings::default();
    let value = |index: usize| {
        let row = Row::ALL[index];
        match row.spec().action() {
            // The export row reports where the file went, once it has
            // written one - the answer belongs where the question was
            // asked, not in a log nobody reads.
            Some(Act::PreviewMiss) => "PLAY".to_owned(),
            Some(Act::ExportHistory) if !export_note.0.is_empty() => export_note.0.clone(),
            Some(Act::LyricsModel) => smart.model_text(),
            Some(Act::Library) => library_move.value(),
            _ => changed_mark(
                row.value(&settings),
                row.spec().differs(&settings, &defaults),
            ),
        }
    };
    let selected = view.position(cursor.0);
    let range = window.top..(window.top + window.visible).min(view.order.len());
    paint.paint_window_full(selected, settings.high_contrast, range, |slot| {
        let global = view.order[slot];
        (Some(Row::ALL[global].label().to_owned()), value(global))
    });
    let row = Row::ALL[cursor.0.min(Row::ALL.len() - 1)];
    let explanation = if row == Row::LYRICS_MODEL {
        smart.model_subtitle()
    } else {
        row.subtitle()
    };
    for (detail, mut text) in &mut details {
        let wanted = match detail {
            SettingDetail::Label if view.order.is_empty() => "NO MATCHING SETTINGS".to_owned(),
            SettingDetail::Value if view.order.is_empty() => String::new(),
            SettingDetail::Explanation if view.order.is_empty() => {
                if view.category == Category::Changed && view.query.is_empty() {
                    "All settings are at their defaults.".to_owned()
                } else {
                    "Try another search, or press ESC to clear it.".to_owned()
                }
            }
            SettingDetail::Dependencies if view.order.is_empty() => String::new(),
            SettingDetail::Dependencies if row.previews_miss() || row == Row::MISS_PREVIEW => {
                format!(
                    "SFX {}% x MISS {}% = {}%
0% on either volume is silent.",
                    (settings.sfx_volume * 100.0).round(),
                    (settings.miss_volume * 100.0).round(),
                    (settings.sfx_volume * settings.miss_volume * 100.0).round()
                )
            }
            SettingDetail::Dependencies if visual_row(row) => format!(
                "FX {}% x MISS {}% = {}%
0% disables visual feedback.",
                (settings.fx_intensity * 100.0).round(),
                (settings.miss_intensity * 100.0).round(),
                (settings.fx_intensity * settings.miss_intensity * 100.0).round()
            ),
            SettingDetail::Dependencies => String::new(),
            SettingDetail::Label => row.label().to_owned(),
            SettingDetail::Value => value(cursor.0),
            SettingDetail::Explanation if !explanation.is_empty() => explanation.clone(),
            SettingDetail::Explanation if row.spec().is_door() => {
                "ENTER opens this screen.".to_owned()
            }
            SettingDetail::Explanation => "LEFT/RIGHT adjusts the value.
BACKSPACE restores the default.
Changes apply immediately."
                .to_owned(),
            SettingDetail::Position => format!(
                "{} / {}  ·  TAB categories  ·  CTRL/CMD+F search  ·  HOME/END",
                if view.order.is_empty() {
                    0
                } else {
                    selected + 1
                },
                view.order.len()
            ),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

fn visual_row(row: Row) -> bool {
    row == Row::MISS_EFFECT || row == Row::MISS_COLOR || row == Row::MISS_INTENSITY
}

fn spawn_visual_sample(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    let (fg, border, fill) = ui_kit::selection_chip_colours(false, false);
    panel.spawn((
        PreviewButton::Visual,
        Button,
        Text::new("FLASH PREVIEW"),
        font.text(ui_kit::SMALL),
        TextColor(fg),
        ui_kit::selection_chip_node(),
        BorderColor::all(border),
        BackgroundColor(fill),
    ));
    panel
        .spawn((
            SampleFrame,
            Node {
                width: percent(100),
                height: px(90),
                flex_shrink: 0.0,
                margin: UiRect::top(px(8)),
                border: UiRect::all(px(1)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(crate::palette::BACKGROUND),
            BorderColor::all(crate::palette::TEXT_DIM),
        ))
        .with_children(|sample| {
            sample
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    left: percent(20),
                    width: percent(60),
                    height: percent(100),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|highway| {
                    for _ in 0..5 {
                        highway.spawn((
                            Node {
                                flex_grow: 1.0,
                                height: percent(100),
                                border: UiRect::right(px(1)),
                                ..default()
                            },
                            BackgroundColor(crate::palette::SURFACE),
                            BorderColor::all(crate::palette::TEXT_DIM),
                        ));
                    }
                });
            sample.spawn((
                SampleFlash,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)] // Disjoint Bevy component queries
fn paint_settings_chrome(
    view: Res<SettingsView>,
    cursor: Res<SettingsCursor>,
    mut tabs: Query<
        (
            &CategoryChip,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut TextColor,
        ),
        Without<PreviewButton>,
    >,
    mut search: Query<&mut Text, With<SettingsSearch>>,
    mut buttons: Query<
        (
            &PreviewButton,
            &Interaction,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut TextColor,
        ),
        Without<CategoryChip>,
    >,
    mut samples: Query<&mut Node, (With<SampleFrame>, Without<PreviewButton>)>,
) {
    for (chip, interaction, mut background, mut border, mut color) in &mut tabs {
        ui_kit::paint_selection_chip(
            *interaction,
            chip.0 == view.category && view.query.is_empty(),
            &mut background,
            &mut border,
            Some(&mut color),
        );
    }
    if let Ok(mut text) = search.single_mut() {
        let wanted = if view.query.is_empty() {
            "SEARCH ALL SETTINGS  ·  CTRL/CMD+F or /".to_owned()
        } else {
            format!(
                "SEARCH: {}{}  ·  ENTER selects  ·  ESC clears",
                view.query,
                if view.editing { "_" } else { "" }
            )
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let row = Row::ALL[cursor.0.min(Row::ALL.len() - 1)];
    let visual = !view.order.is_empty() && visual_row(row);
    for (button, interaction, mut node, mut background, mut border, mut color) in &mut buttons {
        let show = !view.order.is_empty()
            && match button {
                PreviewButton::Sound => row.previews_miss() || row == Row::MISS_PREVIEW,
                PreviewButton::Visual => visual,
            };
        node.display = if show { Display::Flex } else { Display::None };
        ui_kit::paint_selection_chip(
            *interaction,
            false,
            &mut background,
            &mut border,
            Some(&mut color),
        );
    }
    for mut node in &mut samples {
        node.display = if visual { Display::Flex } else { Display::None };
    }
}

#[derive(Clone, Copy, PartialEq)]
struct VisualSampleSettings {
    row: usize,
    effect: MissEffect,
    color: MissColor,
    intensity: f32,
    fx: f32,
    calm: bool,
}

fn animate_visual_audition(
    time: Res<Time>,
    settings: Res<Settings>,
    cursor: Res<SettingsCursor>,
    mut audition: ResMut<VisualAudition>,
    mut previous: Local<Option<VisualSampleSettings>>,
    mut flash: Query<(&mut Node, &mut BackgroundColor, &mut BorderColor), With<SampleFlash>>,
) {
    let snapshot = VisualSampleSettings {
        row: cursor.0,
        effect: settings.miss_effect,
        color: settings.miss_color,
        intensity: settings.miss_intensity,
        fx: settings.fx_intensity,
        calm: settings.reduced_flashing,
    };
    if *previous != Some(snapshot) && visual_row(Row::ALL[cursor.0.min(Row::ALL.len() - 1)]) {
        audition.age = 0.0;
    }
    *previous = Some(snapshot);
    audition.age += time.delta_secs();
    let duration = if settings.reduced_flashing { 0.9 } else { 0.45 };
    let fade = (1.0 - audition.age / duration).clamp(0.0, 1.0).powi(2);
    let strength = settings.fx_intensity
        * settings.miss_intensity
        * if settings.reduced_flashing { 0.35 } else { 1.0 };
    let color = settings
        .miss_color
        .color(settings.miss_effect)
        .with_alpha(fade * strength * 0.8);
    for (mut node, mut background, mut border) in &mut flash {
        node.left = px(0);
        node.top = px(0);
        node.width = percent(100);
        node.height = percent(100);
        node.border = UiRect::all(px(0));
        background.0 = color;
        *border = BorderColor::all(Color::NONE);
        match settings.miss_effect {
            MissEffect::BorderFlash => {
                node.border = UiRect::all(px(4));
                background.0 = Color::NONE;
                *border = BorderColor::all(color);
            }
            MissEffect::HighwayFlash => {
                node.left = percent(20);
                node.width = percent(60);
            }
            MissEffect::CeilingStrobe => {
                node.height = px(12);
            }
            _ => {}
        }
    }
}

/// A value that differs from the shipped default carries a dot, so a
/// player sees at a glance what they changed — and BACKSPACE takes it
/// back. Pure — tested.
fn changed_mark(value: String, differs: bool) -> String {
    if differs {
        format!("• {value}")
    } else {
        value
    }
}

fn persist_settings(settings: Res<Settings>) {
    save_settings(&settings);
}

#[cfg(test)]
mod tests {
    use super::{SUBTITLE_CHARS, changed_mark, short_path};

    #[test]
    fn seven_categories_cover_every_setting_once_and_keep_feedback_together() {
        use super::{Category, Row};
        let indices: Vec<_> = Category::ALL[..7]
            .iter()
            .flat_map(|category| category.rows())
            .map(|row| row.index())
            .collect();
        let unique: std::collections::HashSet<_> = indices.iter().copied().collect();
        assert_eq!(indices.len(), Row::ALL.len());
        assert_eq!(unique.len(), Row::ALL.len());
        assert_eq!(
            Category::Feedback.rows(),
            &[
                Row::MISS_SOUND,
                Row::MISS_VOLUME,
                Row::MISS_PREVIEW,
                Row::MISS_EFFECT,
                Row::MISS_COLOR,
                Row::MISS_INTENSITY
            ]
        );
    }

    #[test]
    fn search_crosses_categories_and_changed_filter_reacts_to_a_reset() {
        use super::{Category, Row, Settings, SettingsView};
        let mut settings = Settings::default();
        let mut view = SettingsView::default();
        let mut cursor = 0;
        view.switch(Category::Audio, &settings, &mut cursor);
        view.query = "HOW late microphone".to_owned();
        view.rebuild(&settings, &mut cursor);
        assert_eq!(view.order, vec![Row::MIC_OFFSET.index()]);
        assert_eq!(cursor, Row::MIC_OFFSET.index());
        view.switch(Category::Changed, &settings, &mut cursor);
        assert!(view.order.is_empty());
        settings.mic_offset_ms = 20.0;
        view.rebuild(&settings, &mut cursor);
        assert_eq!(view.order, vec![Row::MIC_OFFSET.index()]);
        assert!(
            Row::MIC_OFFSET
                .spec()
                .reset(&mut settings, &Settings::default())
        );
        view.rebuild(&settings, &mut cursor);
        assert!(view.order.is_empty());
    }

    fn category_input_app() -> bevy::prelude::App {
        use super::*;
        use bevy::input::{keyboard::KeyboardInput, mouse::MouseWheel};
        use bevy::window::CursorMoved;
        let mut app = App::new();
        let settings = Settings::default();
        let mut view = SettingsView::default();
        let mut cursor = 0;
        view.switch(Category::Feedback, &settings, &mut cursor);
        app.insert_resource(settings)
            .insert_resource(view)
            .insert_resource(SettingsCursor(cursor))
            .init_resource::<crate::controls::InputMap>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<VisualAudition>()
            .init_resource::<NextState<AppState>>()
            .init_resource::<ExportNote>()
            .init_resource::<crate::smart_lyrics::SmartLyrics>()
            .init_resource::<crate::library_move::LibraryMove>()
            .add_message::<KeyboardInput>()
            .add_message::<MouseWheel>()
            .add_message::<CursorMoved>()
            .add_message::<crate::sfx::UiSound>()
            .add_message::<crate::sfx::PreviewMiss>()
            .add_systems(Update, settings_input);
        app
    }

    #[test]
    fn category_navigation_edits_the_mapped_setting_and_remembers_selection() {
        use super::*;
        let mut app = category_input_app();
        let music = app.world().resource::<Settings>().music_volume;
        let miss = app.world().resource::<Settings>().miss_volume;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(
            app.world().resource::<SettingsCursor>().0,
            Row::MISS_VOLUME.index()
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ArrowLeft);
        }
        app.update();
        assert!(app.world().resource::<Settings>().miss_volume < miss);
        assert_eq!(app.world().resource::<Settings>().music_volume, music);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Tab);
        }
        app.update();
        assert_eq!(
            app.world().resource::<SettingsView>().category,
            Category::Stage
        );
        assert_eq!(
            app.world().resource::<SettingsCursor>().0,
            Row::THEME.index(),
            "Tab must not also move down a row"
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::Tab);
        }
        app.update();
        assert_eq!(
            app.world().resource::<SettingsView>().category,
            Category::Feedback
        );
        assert_eq!(
            app.world().resource::<SettingsCursor>().0,
            Row::MISS_VOLUME.index()
        );
    }

    #[test]
    fn focused_search_cannot_reset_a_value_and_empty_results_cannot_adjust_one() {
        use super::*;
        use bevy::input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        };
        let mut app = category_input_app();
        app.world_mut().resource_mut::<Settings>().mic_offset_ms = 20.0;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyF);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyM,
            logical_key: Key::Character("how late microphone".into()),
            state: ButtonState::Pressed,
            text: Some("how late microphone".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(
            app.world().resource::<SettingsCursor>().0,
            Row::MIC_OFFSET.index()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Backspace);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Backspace,
            logical_key: Key::Backspace,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(app.world().resource::<Settings>().mic_offset_ms, 20.0);
        assert_eq!(
            app.world().resource::<SettingsView>().query,
            "how late microphon"
        );
        app.world_mut().resource_mut::<SettingsView>().query = "no-such-setting".to_owned();
        app.world_mut().resource_mut::<SettingsView>().editing = false;
        let before = app.world().resource::<Settings>().clone();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ArrowRight);
        }
        app.update();
        assert!(app.world().resource::<SettingsView>().order.is_empty());
        assert_eq!(
            app.world().resource::<Settings>().mic_offset_ms,
            before.mic_offset_ms
        );
        assert_eq!(
            app.world().resource::<Settings>().miss_volume,
            before.miss_volume
        );
    }

    #[test]
    fn visual_audition_fades_even_when_cursor_is_marked_changed_and_zero_is_off() {
        use super::*;
        let mut app = App::new();
        app.insert_resource(Settings {
            miss_intensity: 1.0,
            fx_intensity: 1.0,
            ..default()
        })
        .insert_resource(SettingsCursor(Row::MISS_INTENSITY.index()))
        .init_resource::<Time>()
        .init_resource::<VisualAudition>()
        .add_systems(Update, animate_visual_audition);
        let sample = app
            .world_mut()
            .spawn((
                SampleFlash,
                Node::default(),
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.016));
        app.update();
        assert!(
            app.world()
                .get::<BackgroundColor>(sample)
                .expect("sample")
                .0
                .to_srgba()
                .alpha
                > 0.0
        );
        let _ = app.world_mut().resource_mut::<SettingsCursor>();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.6));
        app.update();
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(sample)
                .expect("sample")
                .0
                .to_srgba()
                .alpha,
            0.0
        );
        app.world_mut().resource_mut::<Settings>().miss_intensity = 0.0;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.016));
        app.update();
        assert_eq!(
            app.world()
                .get::<BackgroundColor>(sample)
                .expect("sample")
                .0
                .to_srgba()
                .alpha,
            0.0
        );
    }

    #[test]
    fn settings_window_keeps_every_selection_visible_at_both_display_scales() {
        use super::{Row, SettingsWindow};
        for row_h in [27.0, 34.0, 41.0] {
            let mut one = SettingsWindow::default();
            let mut retina = SettingsWindow::default();
            let cursors = (0..Row::ALL.len()).chain((0..Row::ALL.len()).rev()).chain([
                0,
                Row::ALL.len() - 1,
                8,
                30,
                0,
            ]);
            let mut fixed_height = None;
            for cursor in cursors {
                one.follow(cursor, row_h, 1.0);
                retina.follow(cursor, row_h * 2.0, 0.5);
                assert!(
                    one.range().contains(&cursor),
                    "selection {cursor} outside the visible list"
                );
                assert_eq!(one.top, retina.top);
                assert_eq!(one.visible, retina.visible);
                assert_eq!(one.height, retina.height);
                assert_eq!(*fixed_height.get_or_insert(one.height), one.height);
                let inner =
                    one.height - 2.0 * (crate::ui_kit::PANEL_PAD + crate::ui_kit::PANEL_BORDER);
                let content =
                    one.visible as f32 * (row_h + crate::ui_kit::ROW_GAP) - crate::ui_kit::ROW_GAP;
                assert!(
                    (inner - content).abs() < 0.001,
                    "no partial row at the frame edge"
                );
                assert!(one.range().end <= Row::ALL.len());
            }
        }
    }

    #[test]
    fn settings_panels_have_a_fixed_clipped_height_before_layout() {
        let node = super::settings_body_node(crate::song_browser_view::LIST_W);
        assert_eq!(
            node.height,
            bevy::prelude::px(crate::song_browser_view::BODY_H)
        );
        assert_eq!(node.height, node.min_height);
        assert_eq!(node.height, node.max_height);
        assert_eq!(node.flex_shrink, 1.0);
        assert_eq!(node.overflow, bevy::prelude::Overflow::clip());
        let mut window = super::SettingsWindow::default();
        window.follow(super::Row::ALL.len() - 1, 0.0, 1.0);
        assert!(
            window.range().contains(&(super::Row::ALL.len() - 1)),
            "reopening at the end works before the first measurement"
        );
    }

    /// The shipped settings show no dot at all; a changed value shows
    /// one, and resetting the row takes it away again.
    #[test]
    fn a_changed_setting_carries_a_dot_until_it_is_reset() {
        use super::{Row, Settings};
        let defaults = Settings::default();
        let mut settings = Settings::default();
        for row in Row::ALL {
            assert!(
                !row.spec().differs(&settings, &defaults),
                "{row:?} differs from itself"
            );
        }
        assert!(Row::HIT_LABELS.spec().step(&mut settings, 1));
        assert!(Row::HIT_LABELS.spec().differs(&settings, &defaults));
        assert_eq!(changed_mark("OFF".to_owned(), true), "• OFF");
        assert_eq!(changed_mark("ON".to_owned(), false), "ON");
        assert!(Row::HIT_LABELS.spec().reset(&mut settings, &defaults));
        assert!(!Row::HIT_LABELS.spec().differs(&settings, &defaults));
    }

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
            Row::PERFORMANCE_MODE,
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
