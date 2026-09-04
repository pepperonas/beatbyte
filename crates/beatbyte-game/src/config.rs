//! Persistent player settings.
//!
//! Stored as JSON in the platform config directory
//! (`~/Library/Application Support`, `%APPDATA%`, `~/.config`, …).
//! Corrupt or missing files fall back to defaults with a warning —
//! a broken settings file must never brick the game.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::controls::InputMap;
use crate::gameplay::fx::EffectSettings;

/// All persisted settings. Every field has a sane default so partial
/// files from older versions keep working.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Music volume, 0.0–1.0.
    pub music_volume: f32,
    /// Sound-effect volume, 0.0–1.0.
    pub sfx_volume: f32,
    /// Latency calibration offset in milliseconds. Positive = the
    /// player's inputs arrive late (typical audio latency); the offset
    /// is subtracted from input timestamps.
    pub latency_offset_ms: f32,
    /// Video offset in milliseconds: shifts where notes are DRAWN,
    /// never when they judge. Positive draws notes later (for a
    /// display that lags the audio). Purely presentational — the
    /// judgment clock never reads it.
    #[serde(default)]
    pub video_offset_ms: f32,
    /// Note scroll speed in pixels per second.
    pub scroll_speed: f32,
    /// Particle effects.
    pub particles: bool,
    /// Screen shake.
    pub screen_shake: bool,
    /// Stage beat pulse.
    pub beat_pulse: bool,
    /// Backdrop animation (turn off for a still stage —
    /// reduced-motion accessibility).
    pub backdrop_motion: bool,
    /// The PERFECT / GREAT / GOOD / MISS word over the neck. On by
    /// default; off gives the genre's flame-only hit feedback (the
    /// classic guitar games grade nothing per note on screen).
    pub hit_labels: bool,
    /// No Fail: the rock meter moves and shows, but an empty meter
    /// never ends the song. On by default (optimization plan P3: the
    /// goal is tension, not punishment). Off arms failing in solo.
    pub no_fail: bool,
    /// Reduced flashing: suppress full-screen flashes (accessibility).
    #[serde(default)]
    pub reduced_flashing: bool,
    /// Visual effect intensity, 0.0–1.0: scales particle counts,
    /// shake strength and flash opacity together.
    #[serde(default = "default_fx_intensity")]
    pub fx_intensity: f32,
    /// User UI scale multiplier on top of the window-height sync.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    /// High contrast: brighter idle text and stronger selection
    /// fills across every menu.
    #[serde(default)]
    pub high_contrast: bool,
    /// Live karaoke lyrics during gameplay, for songs that have them.
    #[serde(default = "default_lyrics")]
    pub lyrics: bool,
    /// Lyric text size step: 0 small, 1 medium, 2 large.
    #[serde(default = "default_lyrics_size")]
    pub lyrics_size: u8,
    /// Lyrics display offset in milliseconds: positive shows lyrics
    /// later. Purely presentational - judgment never reads it.
    #[serde(default)]
    pub lyrics_offset_ms: f32,
    /// Room Stage: post the game's own events to light services on
    /// the local network (roadmap G38). OFF by default, and the one
    /// place besides the lyrics lookup where anything leaves the
    /// machine — outbound, to an address you write here, or nowhere.
    pub room_lights: bool,
    /// Where those posts go: the base address of a light service.
    pub room_stage_url: String,
    /// Song previews in the browser: rest the cursor on a song and
    /// its hook plays. On by default — a seventy-song library is a
    /// list of names without it (optimization plan P4).
    pub song_preview: bool,
    /// Tap mode: notes hit on fret press alone, no strum required.
    /// ON by default — the first real playtest showed keyboard
    /// players press frets and nothing happens (receptors light up,
    /// notes die); strumming is the opt-in purist mode.
    pub tap_mode: bool,
    /// Depth view: the 2D highway runs toward a vanishing point.
    ///
    /// Always true now — the flat presentation it used to switch off
    /// was removed. The field stays so old settings files keep
    /// loading, and `sanitize` forces it back on.
    pub perspective: bool,
    /// The solid 3D stage: a lit highway with real geometry instead
    /// of the 2D projection. Presentation only — judgment is
    /// input-stamp driven and identical across all three views.
    pub stage_3d: bool,
    /// Round gems instead of the 8-bit per-lane shapes. Off by
    /// default: the shapes are the colorblind-safe look — turning
    /// them off makes color the only lane signal.
    pub round_gems: bool,
    /// Fullscreen window mode.
    pub fullscreen: bool,
    /// A folder watched for new audio tracks (set by dropping a
    /// FOLDER onto the window; `None` = no watching). The path is
    /// deliberately not validated in `sanitize`: an unmounted drive
    /// is a dormant setting, not a broken one.
    #[serde(default)]
    pub watch_folder: Option<std::path::PathBuf>,
    /// The bindings table (see [`crate::controls`]).
    pub input_map: InputMap,
    /// The stage theme id, or "auto" to rotate per song.
    pub theme: String,
    /// The song browser's sort column (a [`crate::song_select::SortMode`]
    /// label, lowercase). The filter is deliberately NOT persisted: an
    /// invisible stale filter across sessions is a trap.
    #[serde(default = "default_browser_sort")]
    pub browser_sort: String,
    /// Whether that sort runs reversed.
    #[serde(default)]
    pub browser_sort_reversed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            music_volume: 0.8,
            sfx_volume: 0.45,
            latency_offset_ms: 0.0,
            video_offset_ms: 0.0,
            scroll_speed: 420.0,
            particles: true,
            screen_shake: true,
            beat_pulse: true,
            backdrop_motion: true,
            hit_labels: true,
            no_fail: true,
            reduced_flashing: false,
            fx_intensity: 1.0,
            ui_scale: 1.0,
            high_contrast: false,
            lyrics: true,
            lyrics_size: 1,
            lyrics_offset_ms: 0.0,
            room_lights: false,
            room_stage_url: "http://127.0.0.1:5006".to_owned(),
            song_preview: true,
            tap_mode: true,
            perspective: true,
            stage_3d: true,
            round_gems: false,
            fullscreen: false,
            watch_folder: None,
            input_map: InputMap::default(),
            theme: "auto".to_owned(),
            browser_sort: default_browser_sort(),
            browser_sort_reversed: false,
        }
    }
}

impl Settings {
    /// The latency offset in seconds.
    #[must_use]
    pub fn latency_offset_s(&self) -> f64 {
        f64::from(self.latency_offset_ms) / 1000.0
    }

    /// The video offset in seconds (drawing only, never judgment).
    #[must_use]
    pub fn video_offset_s(&self) -> f64 {
        f64::from(self.video_offset_ms) / 1000.0
    }

    /// Clamp all values into their valid ranges (files are input too).
    pub fn sanitize(&mut self) {
        // The flat view was removed; a settings file that still
        // selects it would otherwise leave the highway with no depth
        // and no way to get it back from the VIEW row.
        self.perspective = true;
        // The 2D "depth" view is removed (user call, 2026-08-31,
        // with screenshots of both note styles): the 3D stage is the
        // game's one view. A stale settings file must not strand a
        // player in a view that no longer exists.
        self.stage_3d = true;
        self.music_volume = clean(self.music_volume, 0.0, 1.0, 0.8);
        self.sfx_volume = clean(self.sfx_volume, 0.0, 1.0, 0.45);
        self.latency_offset_ms = clean(self.latency_offset_ms, -250.0, 250.0, 0.0);
        self.video_offset_ms = clean(self.video_offset_ms, -100.0, 100.0, 0.0);
        self.scroll_speed = clean(self.scroll_speed, 240.0, 900.0, 420.0);
        self.fx_intensity = clean(self.fx_intensity, 0.0, 1.0, 1.0);
        self.ui_scale = clean(self.ui_scale, 0.75, 1.5, 1.0);
        self.lyrics_size = self.lyrics_size.min(2);
        self.lyrics_offset_ms = clean(self.lyrics_offset_ms, -500.0, 500.0, 0.0);
        self.input_map.sanitize();
        if self.theme != "auto" && crate::theme::Theme::by_id(&self.theme).is_none() {
            self.theme = "auto".to_owned();
        }
        if crate::song_select::SortMode::from_label(&self.browser_sort).is_none() {
            self.browser_sort = default_browser_sort();
        }
    }
}

/// The browser's default sort label.
fn default_browser_sort() -> String {
    "standard".to_owned()
}

/// Full visual effects, the default.
fn default_lyrics() -> bool {
    true
}

fn default_lyrics_size() -> u8 {
    1
}

fn default_fx_intensity() -> f32 {
    1.0
}

/// No extra UI scaling, the default.
fn default_ui_scale() -> f32 {
    1.0
}

fn clean(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// Where the settings file lives.
#[must_use]
pub fn settings_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join("beatbyte").join("settings.json"))
}

/// Load settings, falling back to defaults on any problem.
#[must_use]
pub fn load_settings() -> Settings {
    let Some(path) = settings_path() else {
        warn!("no config directory on this platform; settings won't persist");
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(mut settings) => {
                settings.sanitize();
                settings
            }
            Err(error) => {
                warn!(
                    "settings file {} is invalid ({error}); using defaults",
                    path.display()
                );
                Settings::default()
            }
        },
        // Missing file is the normal first run.
        Err(_) => Settings::default(),
    }
}

/// Persist settings (best effort, logged on failure).
pub fn save_settings(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(settings).unwrap_or_default();
        std::fs::write(&path, json)
    };
    if let Err(error) = write() {
        warn!("cannot save settings to {}: {error}", path.display());
    }
}

/// Plugin: loads settings at startup and mirrors them into the
/// resources other systems consume.
pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let settings = load_settings();
        info!(
            "settings: music {:.0}%, sfx {:.0}%, offset {} ms, scroll {}",
            settings.music_volume * 100.0,
            settings.sfx_volume * 100.0,
            settings.latency_offset_ms,
            settings.scroll_speed
        );
        // The live bindings table mirrors the persisted one; the
        // controls screen edits the resource and writes it back.
        app.insert_resource(settings.input_map.clone())
            .insert_resource(settings)
            .add_systems(Update, apply_settings);
    }
}

/// Push settings changes into their consumers whenever they change.
fn apply_settings(
    settings: Res<Settings>,
    music: Res<crate::audio_sys::Music>,
    muted: Res<crate::mute::Muted>,
    mut effects: ResMut<EffectSettings>,
    mut windows: Query<&mut Window>,
) {
    if !settings.is_changed() {
        return;
    }
    music.0.set_volume(settings.music_volume * muted.factor());
    effects.particles = settings.particles;
    effects.screen_shake = settings.screen_shake;
    effects.beat_pulse = settings.beat_pulse;
    effects.backdrop_motion = settings.backdrop_motion;
    effects.round_particles = settings.round_gems;
    effects.reduced_flashing = settings.reduced_flashing;
    effects.intensity = settings.fx_intensity;
    if let Ok(mut window) = windows.single_mut() {
        let wanted = if settings.fullscreen {
            bevy::window::WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            bevy::window::WindowMode::Windowed
        };
        if window.mode != wanted {
            window.mode = wanted;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn a_mangled_browser_sort_falls_back_and_a_real_one_survives() {
        let mut s = super::Settings {
            browser_sort: "Artist".to_owned(), // case from an older write
            browser_sort_reversed: true,
            ..Default::default()
        };
        s.sanitize();
        assert_eq!(s.browser_sort, "Artist", "a parseable label is kept");
        assert!(s.browser_sort_reversed, "the direction is a free bool");

        s.browser_sort = "bogus".to_owned();
        s.sanitize();
        assert_eq!(s.browser_sort, "standard", "unknown labels fall back");
    }

    use super::Settings;

    /// Forward compatibility: a settings file written by a NEWER
    /// BeatByte (extra fields) or an OLDER one (missing fields) must
    /// load — extra ignored, missing defaulted. Losing someone's
    /// calibration on upgrade is not acceptable.
    #[test]
    fn settings_round_trip_preserves_the_look_and_mode() {
        let mut settings = Settings::default();
        settings.tap_mode = !settings.tap_mode;
        settings.round_gems = !settings.round_gems;
        settings.perspective = !settings.perspective;
        settings.latency_offset_ms = 23.5;
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tap_mode, settings.tap_mode);
        assert_eq!(back.round_gems, settings.round_gems);
        assert_eq!(back.perspective, settings.perspective);
        assert!((back.latency_offset_ms - 23.5).abs() < f32::EPSILON);
    }

    #[test]
    fn the_video_offset_converts_and_clamps() {
        let mut s = Settings {
            video_offset_ms: 40.0,
            ..Default::default()
        };
        assert!((s.video_offset_s() - 0.040).abs() < 1e-9);
        s.video_offset_ms = 4000.0;
        s.sanitize();
        assert!((s.video_offset_ms - 100.0).abs() < f32::EPSILON, "clamped");
        s.video_offset_ms = f32::NAN;
        s.sanitize();
        assert!(
            (s.video_offset_ms - 0.0).abs() < f32::EPSILON,
            "NaN falls back"
        );
    }

    #[test]
    fn settings_tolerate_unknown_and_missing_fields() {
        let json = r#"{
            "music_volume": 0.25,
            "latency_offset_ms": 42.0,
            "some_future_option": {"nested": [1, 2, 3]}
        }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!((settings.music_volume - 0.25).abs() < 1e-6);
        assert!((settings.latency_offset_ms - 42.0).abs() < 1e-6);
        // Missing fields fall back to defaults.
        assert_eq!(settings.theme, Settings::default().theme);
        assert_eq!(settings.fullscreen, Settings::default().fullscreen);
    }

    /// Truly malformed JSON must error (the caller falls back to
    /// defaults with a warning) — never panic.
    #[test]
    fn malformed_settings_error_cleanly() {
        assert!(serde_json::from_str::<Settings>("{not json").is_err());
    }
}
