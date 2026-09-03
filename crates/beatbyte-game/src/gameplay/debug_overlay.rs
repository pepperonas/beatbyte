//! A debug overlay: live facts about the running game, on the key
//! left of `1` (or `F3`).
//!
//! Off by default, toggled at any moment of a song without a restart,
//! and **read-only** — it borrows every resource and component
//! immutably except its own text and its own frame-rate average. It
//! sits under the top-left mode badge, on a translucent plate, in
//! the monospace face: data text, laid out in columns that hold
//! still while the numbers move.
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
/// else in the game.
///
/// The key left of `1` arrives as `Backquote` on an ANSI (US) board
/// and as `IntlBackslash` on an ISO (German) one — Bevy's codes are
/// physical, and the two layouts put a different physical key
/// there — so both count. `F3` too, for the console habit, with the
/// caveat that macOS eats it for Mission Control unless the
/// standard-function-keys setting is on.
pub const TOGGLE: [KeyCode; 3] = [KeyCode::Backquote, KeyCode::IntlBackslash, KeyCode::F3];

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

/// The text entity.
#[derive(Component)]
pub struct DebugText;

/// The plate behind it.
#[derive(Component)]
pub struct DebugPlate;

/// Where the block sits: under the mode badge, top-left.
const AT: Vec2 = Vec2::new(-624.0, 328.0);
/// Text size, in the design's `SMALL` register.
const SIZE: f32 = 9.0;
/// Line height for the plate's height estimate.
const LINE_H: f32 = 12.0;
/// The plate's width.
const PLATE_W: f32 = 372.0;

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
            Vec2::new(PLATE_W, LINE_H * 8.0),
        ),
        Anchor::TOP_LEFT,
        Transform::from_xyz(AT.x - 6.0, AT.y + 4.0, 5.9),
        visibility,
    ));
    commands.spawn((
        GameplayScreen,
        DebugText,
        Text2d::new(""),
        font.mono_text(SIZE),
        TextColor(palette::TEXT),
        Anchor::TOP_LEFT,
        Transform::from_xyz(AT.x, AT.y, 6.0),
        visibility,
    ));
}

/// A toggle key flips the overlay, during a song only.
#[allow(clippy::type_complexity)] // Bevy query filter
pub fn toggle_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut overlay: ResMut<DebugOverlay>,
    mut parts: Query<&mut Visibility, Or<(With<DebugText>, With<DebugPlate>)>>,
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

/// Refresh the text every frame while shown. Reads only.
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
    mut text: Query<&mut Text2d, With<DebugText>>,
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

    let mut lines: Vec<String> = Vec::with_capacity(12);
    let mono = time.elapsed_secs_f64();
    let song = game_clock.clock.song_time(mono);
    let device = music.0.position_s();
    match song {
        Some(now) => lines.push(format!(
            "song {now:8.3}s  vis {:8.3}s  dev {device:8.3}s  drift {:+5.0}ms  rate {:.2}{}",
            now + settings.video_offset_s(),
            (now - device) * 1000.0,
            game_clock.clock.rate(),
            if game_clock.clock.is_playing() {
                ""
            } else {
                "  PAUSED"
            }
        )),
        None => lines.push(format!("song   --.---s  dev {device:8.3}s  clock stopped")),
    }
    let frame_ms = overlay.frame_s * 1000.0;
    lines.push(format!(
        "frame {frame_ms:5.2}ms  {:4.0} fps   entities {:5}   autopilot {}",
        if overlay.frame_s > 0.0 {
            1.0 / overlay.frame_s
        } else {
            0.0
        },
        entities.iter().count(),
        if autopilot.is_some_and(|a| a.enabled) {
            "ON"
        } else {
            "off"
        }
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
            lines.push(format!(
                "tempo {:6.2} bpm  beat {:7.2}   events {events:4}  judged {:4}  left {:4}",
                track.tempo.bpm_at(now),
                track.tempo.beats_at(now),
                counts.total(),
                events.saturating_sub(counts.total() as usize)
            ));
        }
        lines.push(format!(
            "P{} score {:7}  streak {:4}  x{}  acc {:5.1}%  P{:<3} G{:<3} Gd{:<3} M{:<3} over {}",
            index.0 + 1,
            perf.score(),
            perf.streak(),
            perf.multiplier(),
            perf.accuracy() * 100.0,
            counts.perfect,
            counts.great,
            counts.good,
            counts.miss,
            perf.overstrums()
        ));
        lines.push(format!(
            "   hype {:.2} {:3}  meter {:.2} {:6}  offset {:+6.1}ms  held {}  sustain {:>4}  spawn {}",
            perf.hype_meter(),
            if perf.hype_active() { "ON" } else { "off" },
            perf.meter(),
            if perf.failed() { "FAILED" } else { "ok" },
            perf.mean_offset_ms().unwrap_or(0.0),
            held_strip(session.held()),
            session
                .active_sustain()
                .map_or_else(|| "-".to_owned(), |i| format!("#{i}")),
            player.spawn_cursor
        ));
        if let Some(heat) = heat.as_ref() {
            let strip: String = Lane::ALL
                .iter()
                .map(|lane| {
                    heat.0
                        .iter()
                        .find(|e| e.player == index.0 && e.lane == *lane)
                        .map_or_else(|| " -.--".to_owned(), |e| format!(" {:.2}", e.hit))
                })
                .collect();
            lines.push(format!("   heat (hit per fret){strip}"));
        }
    }
    lines.push(format!(
        "set latency {:+.0}ms  video {:+.0}ms  scroll {:.0}  tap {}  nofail {}  style {}  3d {}",
        settings.latency_offset_ms,
        settings.video_offset_ms,
        settings.scroll_speed,
        if settings.tap_mode { "on" } else { "off" },
        if settings.no_fail { "on" } else { "off" },
        if settings.round_gems { "round" } else { "8bit" },
        if settings.stage_3d { "on" } else { "off" }
    ));
    lines.push("` / F3 hides this".to_owned());

    let count = lines.len();
    let joined = lines.join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
    if let Ok(mut sprite) = plate.single_mut() {
        sprite.custom_size = Some(Vec2::new(PLATE_W, LINE_H * count as f32 + 8.0));
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
    fn the_overlay_is_off_by_default_and_its_key_is_unbound_elsewhere() {
        assert!(!DebugOverlay::default().on);
        // The toggle must not collide with a bound key: the source
        // of every binding is the controls map and the mute badge,
        // and neither names an F-key. Checked textually here so a
        // future binding of F3 fails this test rather than the user.
        let controls = include_str!("../controls.rs");
        let mute = include_str!("../mute.rs");
        for src in [controls, mute] {
            for key in [
                "KeyCode::F3",
                "KeyCode::Backquote",
                "KeyCode::IntlBackslash",
            ] {
                assert!(!src.contains(key), "{key} is the debug overlay's key");
            }
        }
    }
}
