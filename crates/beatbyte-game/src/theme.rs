//! The theme system: six original stage identities, all data.
//!
//! A theme is a palette plus a procedural backdrop — no assets, no
//! per-theme code paths in gameplay. Themes broadly channel eras of
//! rock without copying anyone's stage: readability first, spectacle
//! second (the note lanes stay high-contrast in every theme).

use beatbyte_core::Lane;
use bevy::prelude::*;

use crate::gameplay::{GameplayScreen, HighwayLayout, PlayerSession};
use crate::states::{AppState, GamePhase};

/// A complete stage identity.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Stable id (settings value).
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Clear color behind everything.
    pub background: Color,
    /// Highway bed tone.
    pub surface: Color,
    /// Stage accent (backdrop tint).
    pub accent: Color,
    /// The five lane colors, left to right.
    pub lane_colors: [Color; 5],
    /// The procedural backdrop.
    pub backdrop: Backdrop,
    /// Beat-pulse brightness lift (default feel = 0.55).
    pub pulse_strength: f32,
}

/// Procedural backdrop styles (pixel sprites, engine-drawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// Drifting stars that twinkle on the beat.
    Starfield,
    /// A synthwave floor: horizontal lines rolling toward the player.
    Grid,
    /// Embers rising from below.
    Flames,
    /// Rows of crowd dots that bounce on the beat.
    Crowd,
    /// Slow pixel bubbles floating up, shifting hue.
    Bubbles,
    /// Sweeping spotlight beams.
    Spotlights,
}

/// The classic lane palette (shared by several themes, tinted by
/// others). Order: green red yellow blue orange.
const CLASSIC_LANES: [Color; 5] = [
    Color::srgb(0.24, 0.86, 0.52),
    Color::srgb(1.0, 0.32, 0.32),
    Color::srgb(1.0, 0.84, 0.25),
    Color::srgb(0.25, 0.77, 1.0),
    Color::srgb(1.0, 0.67, 0.25),
];

/// All built-in themes.
pub const THEMES: [Theme; 6] = [
    Theme {
        id: "garage",
        name: "GARAGE",
        background: Color::srgb(0.075, 0.06, 0.045),
        surface: Color::srgb(0.115, 0.095, 0.07),
        accent: Color::srgb(1.0, 0.72, 0.35),
        lane_colors: CLASSIC_LANES,
        backdrop: Backdrop::Starfield,
        pulse_strength: 0.45,
    },
    Theme {
        id: "punk",
        name: "PUNK",
        background: Color::srgb(0.07, 0.035, 0.06),
        surface: Color::srgb(0.12, 0.05, 0.1),
        accent: Color::srgb(1.0, 0.18, 0.53),
        lane_colors: [
            Color::srgb(0.35, 0.95, 0.55),
            Color::srgb(1.0, 0.25, 0.45),
            Color::srgb(1.0, 0.9, 0.3),
            Color::srgb(0.4, 0.7, 1.0),
            Color::srgb(1.0, 0.55, 0.2),
        ],
        backdrop: Backdrop::Crowd,
        pulse_strength: 0.8,
    },
    Theme {
        id: "metal",
        name: "METAL",
        background: Color::srgb(0.04, 0.045, 0.06),
        surface: Color::srgb(0.08, 0.09, 0.115),
        accent: Color::srgb(1.0, 0.3, 0.15),
        lane_colors: CLASSIC_LANES,
        backdrop: Backdrop::Flames,
        pulse_strength: 0.7,
    },
    Theme {
        id: "stadium",
        name: "STADIUM",
        background: Color::srgb(0.03, 0.045, 0.09),
        surface: Color::srgb(0.06, 0.085, 0.15),
        accent: Color::srgb(0.85, 0.92, 1.0),
        lane_colors: CLASSIC_LANES,
        backdrop: Backdrop::Spotlights,
        pulse_strength: 0.6,
    },
    Theme {
        id: "psychedelic",
        name: "PSYCHEDELIC",
        background: Color::srgb(0.06, 0.03, 0.09),
        surface: Color::srgb(0.1, 0.055, 0.15),
        accent: Color::srgb(0.65, 0.4, 1.0),
        lane_colors: [
            Color::srgb(0.45, 0.95, 0.7),
            Color::srgb(1.0, 0.4, 0.6),
            Color::srgb(1.0, 0.85, 0.4),
            Color::srgb(0.45, 0.65, 1.0),
            Color::srgb(0.95, 0.55, 1.0),
        ],
        backdrop: Backdrop::Bubbles,
        pulse_strength: 0.65,
    },
    Theme {
        id: "cyber",
        name: "CYBER",
        background: Color::srgb(0.015, 0.03, 0.045),
        surface: Color::srgb(0.03, 0.065, 0.09),
        accent: Color::srgb(0.2, 1.0, 0.9),
        lane_colors: [
            Color::srgb(0.2, 1.0, 0.6),
            Color::srgb(1.0, 0.25, 0.55),
            Color::srgb(1.0, 0.95, 0.35),
            Color::srgb(0.25, 0.85, 1.0),
            Color::srgb(1.0, 0.6, 0.2),
        ],
        backdrop: Backdrop::Grid,
        pulse_strength: 0.75,
    },
];

impl Theme {
    /// Look up a theme by id.
    #[must_use]
    pub fn by_id(id: &str) -> Option<&'static Theme> {
        THEMES.iter().find(|theme| theme.id == id)
    }

    /// The color of a lane in this theme.
    #[must_use]
    pub fn lane_color(&self, lane: Lane) -> Color {
        self.lane_colors[lane.index()]
    }
}

/// The theme in effect for the current song.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveTheme(pub Theme);

impl Default for ActiveTheme {
    fn default() -> Self {
        ActiveTheme(THEMES[0])
    }
}

/// Pick the theme for a song title from the settings ("auto" rotates
/// deterministically by title).
#[must_use]
pub fn choose_theme(setting: &str, song_title: &str) -> Theme {
    if let Some(theme) = Theme::by_id(setting) {
        return *theme;
    }
    // Auto: a stable hash of the title picks the stage.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in song_title.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    THEMES[(hash % THEMES.len() as u64) as usize]
}

/// One animated backdrop sprite.
#[derive(Component)]
struct BackdropBit {
    kind: Backdrop,
    seed: f32,
    base: Vec2,
}

/// The theme plugin: applies palettes and animates backdrops.
pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTheme>()
            .add_systems(OnExit(AppState::Gameplay), restore_clear_color)
            .add_systems(
                Update,
                animate_backdrop.run_if(in_state(GamePhase::Playing)),
            );
    }
}

/// Spawned from the gameplay setup chain (needs the layout + theme).
pub fn spawn_backdrop(
    mut commands: Commands,
    theme: Res<ActiveTheme>,
    layout: Res<HighwayLayout>,
    settings: Res<crate::config::Settings>,
    shapes: Res<crate::shapes::LaneShapes>,
) {
    // The 3D stage builds its own venue as real geometry behind the
    // neck. These sprites are drawn by the 2D camera, which sits IN
    // FRONT of the stage camera (order -1), so in 3D they are not a
    // backdrop at all — they are confetti over the fretboard.
    if crate::gameplay::stage3d::active(&settings) {
        return;
    }
    let theme = theme.0;
    let accent = theme.accent;
    // Bits spread across the whole screen behind the highways.
    let count = match theme.backdrop {
        Backdrop::Spotlights => 6,
        Backdrop::Grid => 24,
        _ => 60,
    };
    for i in 0..count {
        let seed = hash01(i * 37 + 5);
        let seed2 = hash01(i * 91 + 11);
        let base = match theme.backdrop {
            Backdrop::Grid => Vec2::new(0.0, -80.0 - 400.0 * seed),
            Backdrop::Spotlights => Vec2::new(-520.0 + 208.0 * i as f32, 380.0),
            _ => Vec2::new(-620.0 + 1240.0 * seed, -340.0 + 700.0 * seed2),
        };
        let (size, alpha) = match theme.backdrop {
            Backdrop::Starfield => (Vec2::splat(2.0 + 3.0 * seed2), 0.5),
            Backdrop::Grid => (Vec2::new(1400.0, 2.0), 0.25),
            Backdrop::Flames => (Vec2::splat(3.0 + 4.0 * seed2), 0.5),
            Backdrop::Crowd => (Vec2::new(8.0, 10.0), 0.35),
            Backdrop::Bubbles => (Vec2::splat(5.0 + 8.0 * seed2), 0.3),
            Backdrop::Spotlights => (Vec2::new(60.0, 1000.0), 0.06),
        };
        // Crowd sits in rows at the bottom of the stage.
        let base = if theme.backdrop == Backdrop::Crowd {
            Vec2::new(-600.0 + 1200.0 * seed, -400.0 + 26.0 * (i % 4) as f32)
        } else {
            base
        };
        // Round style: roughly square bits become soft discs — pixel
        // squares would betray "not 8-bit". Long strips (grid lines,
        // spotlight cones) keep their rectangle.
        let squarish = size.x / size.y < 2.0 && size.y / size.x < 2.0;
        let image = if settings.round_gems && squarish {
            shapes.soft_dot()
        } else {
            Handle::default()
        };
        commands.spawn((
            GameplayScreen,
            BackdropBit {
                kind: theme.backdrop,
                seed: seed * core::f32::consts::TAU,
                base,
            },
            Sprite {
                image,
                color: accent.with_alpha(alpha),
                custom_size: Some(size),
                ..Default::default()
            },
            Transform::from_xyz(base.x, base.y, -25.0),
        ));
    }
    let _ = layout; // reserved for layout-aware backdrops
}

/// Animate the backdrop per style — beat-aware where it reads well.
fn animate_backdrop(
    time: Res<Time>,
    effects: Res<crate::gameplay::fx::EffectSettings>,
    players: Query<&PlayerSession>,
    game_clock: Res<crate::audio_sys::GameClock>,
    mut bits: Query<(&BackdropBit, &mut Transform, &mut Sprite)>,
) {
    // Reduced motion: leave the backdrop as a still image.
    if !effects.backdrop_motion {
        return;
    }
    let t = time.elapsed_secs();
    // Beat phase drives crowd bounce / star twinkle.
    let beat_pulse = players
        .iter()
        .next()
        .and_then(|player| {
            game_clock
                .song_time(&time)
                .map(|now| player.session.track().tempo.beats_at(now))
        })
        .map(|beats| {
            if beats < 0.0 {
                0.0
            } else {
                (-(beats.fract() as f32) * 5.0).exp()
            }
        })
        .unwrap_or(0.0);

    for (bit, mut transform, mut sprite) in &mut bits {
        match bit.kind {
            Backdrop::Starfield => {
                let twinkle = 0.25
                    + 0.35 * ((t * 0.8 + bit.seed * 7.0).sin() * 0.5 + 0.5)
                    + 0.25 * beat_pulse;
                sprite.color = sprite.color.with_alpha(twinkle);
            }
            Backdrop::Grid => {
                // Lines roll downward and wrap — an endless floor.
                let speed = 60.0;
                let range = 420.0;
                let y = bit.base.y - (t * speed) % range;
                transform.translation.y = if y < -500.0 { y + range } else { y };
                sprite.color = sprite.color.with_alpha(0.10 + 0.25 * beat_pulse);
            }
            Backdrop::Flames => {
                let rise = 90.0 + 40.0 * (bit.seed % 1.0);
                let span = 760.0;
                let y = (bit.base.y + t * rise + bit.seed * 120.0) % span - 380.0;
                transform.translation.y = y;
                transform.translation.x = bit.base.x + (t * 1.3 + bit.seed * 9.0).sin() * 14.0;
                // Embers dim as they climb.
                let life = 1.0 - (y + 380.0) / span;
                sprite.color = sprite.color.with_alpha(0.15 + 0.45 * life);
            }
            Backdrop::Crowd => {
                let hop = beat_pulse * 10.0 * (0.6 + 0.4 * (bit.seed * 3.7).sin().abs());
                transform.translation.y = bit.base.y + hop;
            }
            Backdrop::Bubbles => {
                let rise = 18.0 + 14.0 * (bit.seed % 1.0);
                let span = 760.0;
                let y = (bit.base.y + t * rise + bit.seed * 80.0) % span - 380.0;
                transform.translation.y = y;
                transform.translation.x = bit.base.x + (t * 0.6 + bit.seed * 5.0).sin() * 30.0;
            }
            Backdrop::Spotlights => {
                let sway = (t * 0.5 + bit.seed).sin() * 0.45;
                transform.rotation = Quat::from_rotation_z(sway);
                sprite.color = sprite.color.with_alpha(0.045 + 0.05 * beat_pulse);
            }
        }
    }
}

fn restore_clear_color(mut clear: ResMut<ClearColor>) {
    clear.0 = crate::palette::BACKGROUND;
}

/// Cheap deterministic hash → 0.0..1.0.
fn hash01(seed: usize) -> f32 {
    let mut x = seed as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((x >> 40) as f32) / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_has_a_unique_id() {
        // `by_id` returns the FIRST match, so a duplicate id would
        // make one theme permanently unreachable from settings.
        let mut ids: Vec<&str> = THEMES.iter().map(|theme| theme.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate theme id");
    }

    #[test]
    fn a_named_theme_is_honoured_exactly() {
        for theme in THEMES {
            assert_eq!(
                choose_theme(theme.id, "any title").id,
                theme.id,
                "setting `{}` must select its own theme",
                theme.id
            );
        }
    }

    #[test]
    fn auto_is_stable_for_a_title() {
        // The stage must not change between two runs of the same
        // song, or a player's memory of "the red one" is worthless.
        let first = choose_theme("auto", "Circuit Breaker").id;
        let second = choose_theme("auto", "Circuit Breaker").id;
        assert_eq!(first, second);
    }

    #[test]
    fn auto_spreads_titles_across_stages() {
        // A hash that collapsed onto one stage would still be
        // "stable" and would look like the feature does nothing.
        let titles = [
            "Circuit Breaker",
            "Solder Groove",
            "Girls Just Want to Have Fun",
            "The Passenger",
            "Never Gonna Give You Up",
            "Two of Hearts",
            "Delilah",
            "Some Other Song",
        ];
        let mut seen: Vec<&str> = titles
            .iter()
            .map(|title| choose_theme("auto", title).id)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert!(
            seen.len() >= 3,
            "8 titles landed on only {} stage(s): {seen:?}",
            seen.len()
        );
    }

    #[test]
    fn an_unknown_setting_falls_back_to_auto_not_to_a_panic() {
        // Settings files are edited by hand and survive across
        // versions; a removed theme id must not take the game down.
        let theme = choose_theme("this-theme-does-not-exist", "Song");
        assert!(THEMES.iter().any(|known| known.id == theme.id));
    }

    #[test]
    fn every_stage_lights_all_five_lanes() {
        // A theme that left a lane fully black would make its notes
        // invisible on the highway.
        for theme in THEMES {
            for index in 0..5 {
                let lane = Lane::from_index(index).expect("five lanes");
                let color = theme.lane_color(lane).to_linear();
                let brightness = color.red.max(color.green).max(color.blue);
                assert!(
                    brightness > 0.15,
                    "theme `{}` lane {index} is nearly black ({brightness})",
                    theme.id
                );
            }
        }
    }
}
