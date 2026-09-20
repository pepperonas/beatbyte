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

/// The telemetry writer's row, or nothing when it is not recording.
///
/// A store that is silently not writing looks exactly like a store
/// that is, and the first thing anybody asks of one is "did that run
/// go in?". So it says: what it is recording, how much has gone down,
/// how much is waiting, how long since the last commit, how long that
/// commit took, and — the number that must never be hidden — how many
/// events were dropped. Pure, so it can be pinned without a database.
#[must_use]
pub fn telemetry_rows(stats: Option<&beatbyte_telemetry::WriterStats>, level: &str) -> Vec<String> {
    let Some(stats) = stats else {
        return vec![row("TELEM", &[("state", "off".to_owned())])];
    };
    let mut lines = vec![row(
        "TELEM",
        &[
            ("level", level.to_owned()),
            ("wrote", stats.written.to_string()),
            ("queue", stats.queued.to_string()),
            (
                "dropped",
                if stats.dropped == 0 {
                    "0".to_owned()
                } else {
                    // Loud on purpose: a dropped event is a hole in
                    // the evidence, and a hole nobody notices is the
                    // failure this whole design exists to avoid.
                    format!("!! {}", stats.dropped)
                },
            ),
        ],
    )];
    lines.push(row(
        "STORE",
        &[
            ("flush", format!("{}ms", stats.since_flush_ms)),
            ("write", format!("{}us", stats.last_write_us)),
            ("events", stats.events.to_string()),
            ("size", format!("{:.1}MB", stats.bytes as f64 / 1_048_576.0)),
        ],
    ));
    if stats.errors > 0 {
        lines.push(row(
            "STORE",
            &[
                ("errors", stats.errors.to_string()),
                (
                    "last",
                    stats.last_error.clone().unwrap_or_else(|| "?".to_owned()),
                ),
            ],
        ));
    }
    lines
}

/// The microphone's rows, or nothing when nobody is singing.
///
/// Everything a vocal complaint needs answering with: is the device
/// open, is it hearing anything, what pitch does it think that is,
/// what was being asked for, how far apart are those two, and how
/// many frames the game has dropped on the floor. A report of "the
/// vocals feel off" is unanswerable without this and obvious with it.
///
/// ⚠️ Read from ONE borrow of the run. Sampling the same values in
/// several places across a frame would show a state that never
/// existed — the worker writes between reads. Pure — tested.
#[must_use]
pub fn vocal_rows(
    run: Option<&super::vocal::VocalRun>,
    ears: Option<&super::monitors::Ears>,
    settings: &Settings,
) -> Vec<String> {
    let Some(run) = run else {
        return Vec::new();
    };
    let tap = ears.and_then(|ears| ears.0.as_ref()).map(|l| l.vocals());
    let mut lines = Vec::with_capacity(3);
    lines.push(row(
        "MIC",
        &[
            (
                "state",
                match tap {
                    None => "no device".to_owned(),
                    Some(tap) if !tap.enabled() => "idle".to_owned(),
                    Some(_) => "live".to_owned(),
                },
            ),
            (
                "rate",
                tap.map_or_else(|| "-".to_owned(), |t| format!("{}", t.rate())),
            ),
            (
                "lag",
                tap.map_or_else(
                    || "-".to_owned(),
                    |t| format!("{:.0}ms", t.known_latency_s() * 1000.0),
                ),
            ),
            ("offset", format!("{:+.0}ms", settings.mic_offset_ms)),
            (
                "dropped",
                tap.map_or_else(|| "-".to_owned(), |t| t.dropped().to_string()),
            ),
        ],
    ));
    let heard = run.last;
    lines.push(row(
        "",
        &[
            (
                "level",
                heard.map_or_else(|| "-".to_owned(), |f| format!("{:.0}dB", f.rms_dbfs)),
            ),
            (
                "clarity",
                heard.map_or_else(|| "-".to_owned(), |f| format!("{:.2}", f.confidence)),
            ),
            (
                "midi",
                heard
                    .and_then(|f| f.midi)
                    .map_or_else(|| "-".to_owned(), |m| format!("{m:.2}")),
            ),
            (
                "hz",
                heard.and_then(|f| f.midi).map_or_else(
                    || "-".to_owned(),
                    |m| format!("{:.0}", beatbyte_core::vocal::midi_to_hz(m)),
                ),
            ),
            (
                "clip",
                if heard.is_some_and(|f| f.clipped) {
                    "on"
                } else {
                    "off"
                }
                .to_owned(),
            ),
        ],
    ));
    let perf = run.session.performance();
    lines.push(row(
        "",
        &[
            (
                "target",
                run.session
                    .active_note()
                    .and_then(|n| n.target_midi)
                    .map_or_else(|| "-".to_owned(), |m| format!("{m:.1}")),
            ),
            (
                "off",
                perf.mean_abs_cents()
                    .map_or_else(|| "-".to_owned(), |c| format!("{c:.0}c")),
            ),
            ("notes", format!("{}/{}", perf.notes_hit, perf.notes)),
            ("streak", perf.streak.to_string()),
            ("score", perf.score.to_string()),
            ("trace", run.trace.len().to_string()),
        ],
    ));
    lines
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
    vocal: Option<Res<super::vocal::VocalRun>>,
    ears: Option<Res<super::monitors::Ears>>,
    telemetry: Option<Res<crate::telemetry::TelemetryStore>>,
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
    for line in telemetry_rows(
        telemetry
            .as_deref()
            .and_then(crate::telemetry::TelemetryStore::writer)
            .map(beatbyte_telemetry::Telemetry::stats)
            .as_ref(),
        settings.telemetry.label(),
    ) {
        lines.push(line);
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
        // The difficulty gets the label row to itself. A fourth cell
        // on the score row would overrun the plate (75 characters =
        // 405 px against a 360 px budget, measured), and it is the
        // line a player looks for once rather than watches.
        //
        // From the TRACK, not from the browser's selection: an MC set
        // can hand a player a different difficulty mid-set, and this
        // says what is under their hands right now.
        lines.push(row(&label, &[("diff", track.difficulty.to_string())]));
        lines.push(row(
            "",
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
    for line in vocal_rows(vocal.as_deref(), ears.as_deref(), &settings) {
        lines.push(line);
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

    #[test]
    fn the_microphone_rows_appear_only_when_somebody_is_singing() {
        use crate::config::Settings;
        let settings = Settings::default();
        assert!(
            vocal_rows(None, None, &settings).is_empty(),
            "the overlay grew microphone rows with no vocal run"
        );
    }

    #[test]
    fn the_microphone_rows_answer_what_a_vocal_complaint_asks() {
        use crate::config::Settings;
        use beatbyte_core::vocal::{VocalKind, VocalNote, VocalPart, VocalPhrase, VocalRole};
        use beatbyte_core::vocal_session::{VocalInputFrame, VocalScoreConfig};

        let part = VocalPart {
            id: "lead".to_owned(),
            role: VocalRole::Lead,
            name: None,
            phrases: vec![VocalPhrase {
                start_s: 0.0,
                end_s: 2.0,
                confidence: 1.0,
                tokens: Vec::new(),
                notes: vec![VocalNote {
                    start_s: 0.0,
                    end_s: 2.0,
                    kind: VocalKind::Pitched,
                    target_midi: Some(64.0),
                    contour: Vec::new(),
                    confidence: 1.0,
                    token_range: None,
                }],
            }],
        };
        let mut run = super::super::vocal::VocalRun::new(part, VocalScoreConfig::default());
        run.last = Some(VocalInputFrame {
            song_time_s: 1.0,
            midi: Some(63.5),
            confidence: 0.82,
            rms_dbfs: -21.0,
            voiced: true,
            clipped: true,
        });
        let settings = Settings {
            mic_offset_ms: 40.0,
            ..Settings::default()
        };
        let lines = vocal_rows(Some(&run), None, &settings);
        let all = lines.join("\n");
        assert_eq!(lines.len(), 3, "{all}");
        // Without ears there is no device to report, and the overlay
        // must say so rather than printing a plausible zero.
        assert!(all.contains("no device"), "{all}");
        assert!(all.contains("+40ms"), "the offset is missing: {all}");
        assert!(all.contains("-21dB"), "the level is missing: {all}");
        assert!(all.contains("0.82"), "the clarity is missing: {all}");
        assert!(all.contains("63.50"), "the heard pitch is missing: {all}");
        assert!(all.contains("64.0"), "the target is missing: {all}");
        assert!(
            all.contains("clip") && all.contains("on"),
            "clipping: {all}"
        );
    }
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
    fn the_difficulty_row_fits_the_plate_and_a_fourth_cell_would_not() {
        // Why it sits on its own line: every difficulty name is short
        // enough for the label row, and the score row is already full.
        for difficulty in beatbyte_core::Difficulty::ALL {
            let line = row("P1", &[("diff", difficulty.to_string())]);
            assert!(
                line.len() as f32 * SIZE * 0.6 < PLATE_W - 12.0,
                "{difficulty} overflows the plate"
            );
        }
        let crowded = row(
            "P1",
            &[
                ("diff", "Expert".to_owned()),
                ("score", "1234567".to_owned()),
                ("streak", "999".to_owned()),
                ("mult", "x4".to_owned()),
            ],
        );
        assert!(
            crowded.len() as f32 * SIZE * 0.6 > PLATE_W - 12.0,
            "if four cells ever fit, put the difficulty back on the score row"
        );
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

#[cfg(test)]
mod telemetry_tests {
    use super::*;
    use beatbyte_telemetry::WriterStats;

    #[test]
    fn a_store_that_is_not_recording_says_so() {
        let lines = telemetry_rows(None, "OFF");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("off"), "{}", lines[0]);
    }

    #[test]
    fn a_dropped_event_is_shouted_and_a_clean_run_is_not() {
        let clean = WriterStats {
            queued: 3,
            written: 1200,
            dropped: 0,
            since_flush_ms: 412,
            last_write_us: 900,
            sessions: 475,
            events: 146_290,
            bytes: 7_340_032,
            errors: 0,
            last_error: None,
        };
        let lines = telemetry_rows(Some(&clean), "ACTIONS");
        assert_eq!(lines.len(), 2, "no error row when nothing failed");
        assert!(lines[0].contains("ACTIONS"));
        assert!(lines[0].contains("1200"));
        assert!(
            !lines[0].contains("!!"),
            "a clean run must not cry wolf: {}",
            lines[0]
        );
        assert!(lines[1].contains("7.0MB"), "{}", lines[1]);

        let holed = WriterStats {
            dropped: 4,
            errors: 1,
            last_error: Some("disk full".to_owned()),
            ..clean
        };
        let lines = telemetry_rows(Some(&holed), "ACTIONS");
        assert!(
            lines[0].contains("!! 4"),
            "a hole in the evidence is the one number that may not be \
             quiet: {}",
            lines[0]
        );
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("disk full"));
    }
}

/// The overlay's own systems in a real (headless) app.
///
/// The rows above are pure and pinned; this is the call site, which a
/// pure test cannot reach. It matters here more than usual: the
/// screen was locked for this whole session, so nobody has SEEN the
/// overlay — reading the text back out of the entity is the strongest
/// evidence available, and it is a different thing from having
/// looked.
#[cfg(test)]
mod wired_tests {
    use super::*;
    use crate::states::AppState;

    #[test]
    fn the_overlay_draws_the_telemetry_rows_it_was_given() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_resource::<Time>()
            .init_resource::<Settings>()
            .init_resource::<GameClock>()
            .init_resource::<DebugOverlay>()
            .init_state::<AppState>()
            .insert_resource(Music(beatbyte_audio::playback::spawn_music_thread()))
            .add_systems(Update, update_debug_overlay);
        app.world_mut().resource_mut::<DebugOverlay>().on = true;
        let text = app.world_mut().spawn((DebugText, Text2d::new(""))).id();
        app.update();

        let drawn = app
            .world()
            .entity(text)
            .get::<Text2d>()
            .map(|text| text.0.clone())
            .unwrap_or_default();
        assert!(
            drawn.contains("TELEM"),
            "the overlay has no telemetry row at all:\n{drawn}"
        );
        assert!(
            drawn.contains("off"),
            "with no store open it must say so rather than showing \
             zeroes that look like a healthy recording:\n{drawn}"
        );
        // And the row it replaced is still there, so this did not
        // push anything off the plate.
        assert!(drawn.contains("FRAME"), "{drawn}");
    }
}
