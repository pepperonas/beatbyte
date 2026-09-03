//! A debug overlay: live facts about the running game, on `L` (or
//! the key left of `1`).
//!
//! Off by default, toggled at any moment of a song without a restart,
//! and **read-only** — it borrows every resource and component
//! immutably except its own text and its own frame-rate average. It
//! sits under the top-left mode badge, on a translucent plate: the
//! frame rate large in the display face, and under it a TABLE in the
//! monospace face — a section label, then cells of a key and a
//! right-aligned value, in columns that hold still while the numbers
//! move.
//!
//! What it shows is what the game already knows. Nothing here is
//! computed from scratch except the frame-rate average and the
//! clock drift (song clock minus what the audio device reports —
//! the number the reconciler works with every frame).

use beatbyte_core::Lane;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::stage3d::FretHeat;
use super::{GameplayScreen, PlayerIndex, PlayerSession};
use crate::audio_sys::{GameClock, Music};
use crate::config::Settings;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// The keys that flip the overlay, any of them. Unbound anywhere
/// else during a song (the browser's `L` looks lyrics up, and the
/// browser is another state).
///
/// `L` is the key. The key left of `1` still counts: it arrives as
/// `Backquote` on an ANSI (US) board and as `IntlBackslash` on an
/// ISO (German) one — Bevy's codes are physical, and the two layouts
/// put a different physical key there. `F3` was dropped: macOS eats
/// it for Mission Control unless the standard-function-keys setting
/// is on, which made it a key that worked on some machines.
pub const TOGGLE: [KeyCode; 3] = [KeyCode::KeyL, KeyCode::Backquote, KeyCode::IntlBackslash];

/// Whether the overlay is on, plus the one piece of state it owns: a
/// smoothed frame time, so the number is readable rather than
/// flickering with every frame.
#[derive(Resource, Default)]
pub struct DebugOverlay {
    /// Shown?
    pub on: bool,
    /// Exponentially smoothed frame time in seconds (0 = no sample).
    pub frame_s: f32,
}

/// The table's text entity.
#[derive(Component)]
pub struct DebugText;

/// The large frame-rate figure above the table.
#[derive(Component)]
pub struct DebugFps;

/// The plate behind it.
#[derive(Component)]
pub struct DebugPlate;

/// Where the block sits: under the mode badge, top-left.
const AT: Vec2 = Vec2::new(-624.0, 328.0);
/// Table text size, in the design's `SMALL` register.
const SIZE: f32 = 9.0;
/// Line height for the plate's height estimate.
const LINE_H: f32 = 12.0;
/// The height the frame-rate figure takes above the table.
const FPS_H: f32 = 36.0;
/// The plate's width.
const PLATE_W: f32 = 372.0;

/// The table's columns, in characters of the monospace face: the
/// section label, then per cell a key and a right-aligned value.
/// Three cells make 58 characters — 313 px at 9 px, inside the plate.
const LABEL_W: usize = 7;
const KEY_W: usize = 7;
const VAL_W: usize = 8;

/// One table row: the label in its column, then each cell as key
/// and right-aligned value. A value longer than its column runs on
/// rather than being cut (the last cell of a row may be long).
/// Pure — tested.
#[must_use]
pub fn row(label: &str, cells: &[(&str, String)]) -> String {
    let mut out = format!("{label:<LABEL_W$}");
    for (key, value) in cells {
        out.push_str(&format!("{key:<KEY_W$}{value:>VAL_W$}  "));
    }
    out.trim_end().to_owned()
}

/// The frame rate's colour: the game targets the display's rate, and
/// a figure below 55 is a stutter worth seeing at a glance.
#[must_use]
pub fn fps_color(fps: f32) -> Color {
    if fps >= 55.0 {
        palette::PERFECT
    } else if fps >= 30.0 {
        palette::GOOD
    } else {
        palette::MISS
    }
}

/// Smooth a frame time toward the newest sample. Pure — tested.
#[must_use]
pub fn smooth_frame(previous: f32, delta: f32) -> f32 {
    if previous <= 0.0 {
        return delta;
    }
    previous + (delta - previous) * 0.1
}

/// The held frets as a five-character strip in lane order, `-` for
/// an open fret. Pure — tested.
#[must_use]
pub fn held_strip(held: beatbyte_core::LaneSet) -> String {
    const GLYPHS: [char; 5] = ['G', 'R', 'Y', 'B', 'O'];
    Lane::ALL
        .iter()
        .zip(GLYPHS)
        .map(|(lane, glyph)| if held.contains(*lane) { glyph } else { '-' })
        .collect()
}

/// Spawn the (hidden) overlay with the rest of the gameplay screen.
pub fn spawn_debug_overlay(mut commands: Commands, font: Res<UiFont>, overlay: Res<DebugOverlay>) {
    let visibility = if overlay.on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    commands.spawn((
        GameplayScreen,
        DebugPlate,
        Sprite::from_color(
            palette::BACKGROUND.with_alpha(0.72),
            Vec2::new(PLATE_W, FPS_H + LINE_H * 8.0),
        ),
        Anchor::TOP_LEFT,
        Transform::from_xyz(AT.x - 6.0, AT.y + 4.0, 5.9),
        visibility,
    ));
    commands.spawn((
        GameplayScreen,
        DebugFps,
        Text2d::new(""),
        font.text(crate::ui_kit::TITLE),
        TextColor(palette::PERFECT),
        Anchor::TOP_LEFT,
        Transform::from_xyz(AT.x, AT.y, 6.0),
        visibility,
    ));
    commands.spawn((
        GameplayScreen,
        DebugText,
        Text2d::new(""),
        font.mono_text(SIZE),
        TextColor(palette::TEXT),
        Anchor::TOP_LEFT,
        Transform::from_xyz(AT.x, AT.y - FPS_H, 6.0),
        visibility,
    ));
}

/// A toggle key flips the overlay, during a song only.
#[allow(clippy::type_complexity)] // Bevy query filter
pub fn toggle_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut overlay: ResMut<DebugOverlay>,
    mut parts: Query<&mut Visibility, Or<(With<DebugText>, With<DebugFps>, With<DebugPlate>)>>,
) {
    if *state.get() != AppState::Gameplay || !keys.any_just_pressed(TOGGLE) {
        return;
    }
    overlay.on = !overlay.on;
    // A line in the log, like every other state change in the game:
    // "did my key arrive?" is answerable without a screenshot.
    info!("debug overlay: {}", if overlay.on { "on" } else { "off" });
    let wanted = if overlay.on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut parts {
        *visibility = wanted;
    }
}

/// Refresh the figure and the table every frame while shown. Reads
/// only.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system: params are DI
pub fn update_debug_overlay(
    time: Res<Time>,
    mut overlay: ResMut<DebugOverlay>,
    settings: Res<Settings>,
    game_clock: Res<GameClock>,
    music: Res<Music>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    heat: Option<Res<FretHeat>>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    entities: Query<Entity>,
    mut text: Query<&mut Text2d, (With<DebugText>, Without<DebugFps>)>,
    mut fps_text: Query<(&mut Text2d, &mut TextColor), With<DebugFps>>,
    mut plate: Query<&mut Sprite, With<DebugPlate>>,
) {
    // The average keeps running while hidden, so it is honest the
    // moment the overlay comes on.
    overlay.frame_s = smooth_frame(overlay.frame_s, time.delta_secs());
    if !overlay.on {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };

    // The frame rate, large: the one figure a debug overlay is
    // opened for most often.
    let fps = if overlay.frame_s > 0.0 {
        1.0 / overlay.frame_s
    } else {
        0.0
    };
    if let Ok((mut figure, mut color)) = fps_text.single_mut() {
        let wanted = format!("{fps:.0} FPS");
        if figure.0 != wanted {
            figure.0 = wanted;
        }
        color.0 = fps_color(fps);
    }

    let on_off = |flag: bool| if flag { "on" } else { "off" }.to_owned();
    let mut lines: Vec<String> = Vec::with_capacity(20);
    let mono = time.elapsed_secs_f64();
    let song = game_clock.clock.song_time(mono);
    let device = music.0.position_s();
    match song {
        Some(now) => {
            lines.push(row(
                "CLOCK",
                &[
                    ("song", format!("{now:.3}s")),
                    ("vis", format!("{:.3}s", now + settings.video_offset_s())),
                    ("dev", format!("{device:.3}s")),
                ],
            ));
            lines.push(row(
                "",
                &[
                    ("drift", format!("{:+.0}ms", (now - device) * 1000.0)),
                    ("rate", format!("{:.2}", game_clock.clock.rate())),
                    (
                        "state",
                        if game_clock.clock.is_playing() {
                            "playing".to_owned()
                        } else {
                            "PAUSED".to_owned()
                        },
                    ),
                ],
            ));
        }
        None => lines.push(row(
            "CLOCK",
            &[
                ("song", "--".to_owned()),
                ("dev", format!("{device:.3}s")),
                ("state", "stopped".to_owned()),
            ],
        )),
    }
    lines.push(row(
        "FRAME",
        &[
            ("ms", format!("{:.2}", overlay.frame_s * 1000.0)),
            ("ents", entities.iter().count().to_string()),
            (
                "auto",
                if autopilot.is_some_and(|a| a.enabled) {
                    "ON".to_owned()
                } else {
                    "off".to_owned()
                },
            ),
        ],
    ));

    for (index, player) in &players {
        let session = &player.session;
        let perf = session.performance();
        let counts = perf.counts();
        let track = session.track();
        let events = track.events().len();
        if index.0 == 0
            && let Some(now) = song
        {
            lines.push(row(
                "TEMPO",
                &[
                    ("bpm", format!("{:.2}", track.tempo.bpm_at(now))),
                    ("beat", format!("{:.2}", track.tempo.beats_at(now))),
                ],
            ));
            lines.push(row(
                "NOTES",
                &[
                    ("events", events.to_string()),
                    ("judged", counts.total().to_string()),
                    (
                        "left",
                        events.saturating_sub(counts.total() as usize).to_string(),
                    ),
                ],
            ));
        }
        let label = format!("P{}", index.0 + 1);
        lines.push(row(
            &label,
            &[
                ("score", perf.score().to_string()),
                ("streak", perf.streak().to_string()),
                ("mult", format!("x{}", perf.multiplier())),
            ],
        ));
        lines.push(row(
            "",
            &[
                ("acc", format!("{:.1}%", perf.accuracy() * 100.0)),
                ("over", perf.overstrums().to_string()),
                (
                    "offset",
                    format!("{:+.1}ms", perf.mean_offset_ms().unwrap_or(0.0)),
                ),
            ],
        ));
        lines.push(row(
            "",
            &[
                ("perfect", counts.perfect.to_string()),
                ("great", counts.great.to_string()),
                ("good", counts.good.to_string()),
            ],
        ));
        lines.push(row("", &[("miss", counts.miss.to_string())]));
        lines.push(row(
            "",
            &[
                ("hype", format!("{:.2}", perf.hype_meter())),
                ("hype-on", on_off(perf.hype_active())),
                ("meter", format!("{:.2}", perf.meter())),
            ],
        ));
        lines.push(row(
            "",
            &[
                (
                    "crowd",
                    if perf.failed() { "FAILED" } else { "ok" }.to_owned(),
                ),
                ("held", held_strip(session.held())),
                (
                    "sustain",
                    session
                        .active_sustain()
                        .map_or_else(|| "-".to_owned(), |i| format!("#{i}")),
                ),
            ],
        ));
        let mut tail = vec![("spawn", player.spawn_cursor.to_string())];
        if let Some(heat) = heat.as_ref() {
            let strip: Vec<String> = Lane::ALL
                .iter()
                .zip(['G', 'R', 'Y', 'B', 'O'])
                .map(|(lane, glyph)| {
                    heat.0
                        .iter()
                        .find(|e| e.player == index.0 && e.lane == *lane)
                        .map_or_else(
                            || format!("{glyph} -.--"),
                            |e| format!("{glyph} {:.2}", e.hit),
                        )
                })
                .collect();
            tail.push(("heat", strip.join(" ")));
        }
        lines.push(row("", &tail));
    }
    lines.push(row(
        "SET",
        &[
            ("latency", format!("{:+.0}ms", settings.latency_offset_ms)),
            ("video", format!("{:+.0}ms", settings.video_offset_ms)),
            ("scroll", format!("{:.0}", settings.scroll_speed)),
        ],
    ));
    lines.push(row(
        "",
        &[
            ("tap", on_off(settings.tap_mode)),
            ("nofail", on_off(settings.no_fail)),
            (
                "style",
                if settings.round_gems { "round" } else { "8bit" }.to_owned(),
            ),
        ],
    ));
    lines.push(row("", &[("3d", on_off(settings.stage_3d))]));
    lines.push("L / ` hides this".to_owned());

    let count = lines.len();
    let joined = lines.join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
    if let Ok(mut sprite) = plate.single_mut() {
        sprite.custom_size = Some(Vec2::new(PLATE_W, FPS_H + LINE_H * count as f32 + 8.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::LaneSet;

    #[test]
    fn the_frame_average_starts_at_the_first_sample_and_then_smooths() {
        assert!((smooth_frame(0.0, 0.016) - 0.016).abs() < 1e-9);
        let next = smooth_frame(0.016, 0.032);
        assert!(
            next > 0.016 && next < 0.032,
            "moves toward the sample without jumping"
        );
    }

    #[test]
    fn the_held_strip_reads_in_lane_order() {
        assert_eq!(held_strip(LaneSet::EMPTY), "-----");
        assert_eq!(held_strip(LaneSet::single(Lane::One)), "G----");
        assert_eq!(held_strip(LaneSet::single(Lane::Five)), "----O");
        let two = LaneSet::from_lanes([Lane::Two, Lane::Four]);
        assert_eq!(held_strip(two), "-R-B-");
    }

    #[test]
    fn the_overlay_is_off_by_default_and_its_keys_are_unbound_during_a_song() {
        use crate::controls::{Binding, GameAction, InputMap, UiAction};
        assert!(!DebugOverlay::default().on);
        assert!(TOGGLE.contains(&KeyCode::KeyL), "L is the key");
        // The toggle must not collide with a bound key. The bindings
        // are a MAP, so the map is asked — not the source text.
        let map = InputMap::default();
        let bound = |key: KeyCode| {
            GameAction::ALL
                .iter()
                .flat_map(|a| map.of(*a).iter())
                .chain(UiAction::ALL.iter().flat_map(|a| map.ui_of(*a).iter()))
                .any(|b| *b == Binding::Key(key))
        };
        for key in TOGGLE {
            assert!(!bound(key), "{key:?} is the debug overlay's key");
        }
        // The two hard-wired song keys live outside the map: the
        // mute badge and the pause screen's quit. Checked textually.
        for src in [include_str!("../mute.rs"), include_str!("mod.rs")] {
            for key in [
                "KeyCode::KeyL",
                "KeyCode::Backquote",
                "KeyCode::IntlBackslash",
            ] {
                assert!(!src.contains(key), "{key} is the debug overlay's key");
            }
        }
    }

    #[test]
    fn a_row_lays_its_cells_out_in_fixed_columns() {
        let line = row(
            "P1",
            &[("score", "12345".to_owned()), ("streak", "12".to_owned())],
        );
        assert_eq!(line, "P1     score     12345  streak       12");
        // The value column is right-aligned, so "12" and "12345"
        // end on the same character wherever they sit.
        let a = row("", &[("x", "12".to_owned())]);
        let b = row("", &[("x", "12345".to_owned())]);
        assert_eq!(a.len(), b.len());
        // A blank label still occupies its column: continuation rows
        // line up under their section.
        assert!(row("", &[("k", "v".to_owned())]).starts_with("       k"));
        // Three cells fit the plate.
        let widest = row(
            "CLOCK",
            &[
                ("song", "1234.567s".to_owned()),
                ("vis", "1234.567s".to_owned()),
                ("dev", "1234.567s".to_owned()),
            ],
        );
        assert!(
            widest.len() as f32 * SIZE * 0.6 < PLATE_W - 12.0,
            "{} chars overflow the plate",
            widest.len()
        );
    }

    #[test]
    fn the_frame_rate_colour_flags_a_stutter() {
        assert_eq!(fps_color(120.0), palette::PERFECT);
        assert_eq!(fps_color(60.0), palette::PERFECT);
        assert_eq!(fps_color(45.0), palette::GOOD);
        assert_eq!(fps_color(20.0), palette::MISS);
    }
}
