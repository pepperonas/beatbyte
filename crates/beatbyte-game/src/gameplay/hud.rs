//! Per-player HUD blocks in world space, anchored above each highway.
//!
//! World-space text follows the highway layout for any player count —
//! the same code serves solo and four-player splits.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use super::{GameplayScreen, HighwayLayout, PlayerIndex, PlayerSession, player_color};
use crate::palette;
use crate::ui::UiFont;

/// Score line marker.
#[derive(Component)]
pub struct ScoreText(pub usize);
/// Combo line marker.
#[derive(Component)]
pub struct ComboText(pub usize);
/// Multiplier marker.
#[derive(Component)]
pub struct MultiplierText(pub usize);
/// The Hype meter fill sprite.
#[derive(Component)]
pub struct HypeFill(pub usize);

/// The dim leading zeros behind the solo score counter.
#[derive(Component)]
pub struct ScorePad;

/// One bead of the streak row, lit as the next multiplier approaches.
#[derive(Component)]
pub struct StreakBead(pub u32);

/// One quarter of the energy meter.
#[derive(Component)]
pub struct HypeSegment(pub u32);

/// The frame around the multiplier, which lights while hype runs.
#[derive(Component)]
pub struct MultiplierBox;

/// The line under the meter: what the player can do with it now.
#[derive(Component)]
pub struct HypeReadyText;

/// Digits the counter reserves. Six is beyond any real score; the
/// point is the fixed width — a number that shifts about as it grows
/// reads as a sentence rather than an instrument.
const SCORE_DIGITS: usize = 6;

/// Beads in the streak row — one multiplier level's worth.
const STREAK_BEADS: u32 = 10;

/// Quarters, because a completed phrase awards exactly a quarter.
const HYPE_SEGMENTS: u32 = 4;

/// Vertical anchor of the HUD block above each highway.
const HUD_TOP: f32 = 330.0;

// ── Solo corner panels ──────────────────────────────────────────────
//
// Solo play puts the readouts in framed plates in the bottom corners,
// the way the arcade-era games did: score and multiplier bottom-left,
// the meter bottom-right, the highway left alone in between.
//
// The old layout stacked everything above the highway, which the depth
// view could carry but the 3D stage could not: there the neck runs to
// a vanishing point, so "above the highway" is the middle of the
// screen, and the numbers floated in empty space over the horizon.
//
// The ortho projection is `AutoMin{1280, 720}`, so world coordinates
// within ±640 × ±360 are on screen at every window size — the corners
// are reachable in world space and need no screen-space layer.
//
// Multiplayer keeps the per-highway blocks: with two to four necks
// side by side there are no free corners, and a score has to sit above
// the highway it belongs to.

/// Half-width of a corner plate.
const PLATE_W: f32 = 268.0;
/// Half-height of a corner plate.
const PLATE_H: f32 = 122.0;
/// Distance from the viewport edge to a plate.
const PLATE_INSET: f32 = 18.0;
/// Thickness of a plate's border.
const PLATE_BORDER: f32 = 2.0;

/// The filling part of the song-progress bar.
#[derive(Component)]
pub struct SongProgressFill;

/// The elapsed / total readout beside it.
#[derive(Component)]
pub struct SongTimeText;

/// Width of the song ribbon, in world units.
const RIBBON_W: f32 = 620.0;

/// How far through the song `now` is, in 0..1.
///
/// Clamped at both ends because the clock starts NEGATIVE - there is a
/// pre-roll before the first beat - and runs a little past the last
/// note. A bar that filled backwards during the count-in, or overflowed
/// at the end, would be worse than no bar.
#[must_use]
pub fn song_progress(now: f64, duration_s: f64) -> f32 {
    if duration_s <= 0.0 {
        return 0.0;
    }
    (now / duration_s).clamp(0.0, 1.0) as f32
}

/// A song position as `m:ss`, for the readout.
///
/// Negative times (the pre-roll) show as `0:00` rather than as a
/// minus: the song has not started, and "-0:02" reads as a fault.
#[must_use]
pub fn clock_text(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Spawn one HUD block per player.
pub fn spawn_huds(
    mut commands: Commands,
    layout: Res<HighwayLayout>,
    song: Res<crate::boot::LoadedSong>,
    players: Query<&PlayerIndex, With<PlayerSession>>,
    font: Res<UiFont>,
    settings: Res<crate::config::Settings>,
) {
    // Quiet corner badge: which input mode this song runs in — one
    // glance answers "why did/didn't that hit?" while testing tap
    // vs. strum on keyboard or guitar (user request).
    let (badge, color) = if settings.tap_mode {
        ("< TAP >", palette::dimmed(palette::TEXT_DIM, 0.8))
    } else {
        ("< STRUM >", palette::dimmed(palette::HYPE, 0.8))
    };
    commands.spawn((
        GameplayScreen,
        Text2d::new(badge),
        font.text(9.0),
        TextColor(color),
        Anchor::TOP_LEFT,
        // Top-left: the bottom-left corner now belongs to the score
        // plate, and two readouts in one corner read as one crowded
        // block rather than two facts.
        Transform::from_xyz(-624.0, 348.0, 5.0),
    ));

    // Where you are in the song. Not decoration: hype is spent, and
    // spending it depends on knowing whether the song has thirty
    // seconds left or three minutes. Nothing on screen said so.
    //
    // The top strip is the one part of the frame the neck never
    // reaches - it runs to a vanishing point in the middle - so the
    // ribbon costs no playfield and covers nothing.
    let title = format!(
        "{} - {}",
        font.safe(&song.chart.song.title),
        font.safe(&song.chart.song.artist)
    );
    commands.spawn((
        GameplayScreen,
        Text2d::new(title),
        font.text(9.0),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.9)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(-RIBBON_W / 2.0, 350.0, 5.0),
    ));
    commands.spawn((
        GameplayScreen,
        SongTimeText,
        Text2d::new("0:00"),
        font.text(9.0),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.9)),
        Anchor::TOP_RIGHT,
        Transform::from_xyz(RIBBON_W / 2.0, 350.0, 5.0),
    ));
    // Track, then fill. The track has to be visible on its own or an
    // empty bar looks like a missing one.
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(
            palette::dimmed(palette::TEXT_DIM, 0.22),
            Vec2::new(RIBBON_W, 3.0),
        ),
        Transform::from_xyz(0.0, 336.0, 4.0),
    ));
    commands.spawn((
        GameplayScreen,
        SongProgressFill,
        Sprite::from_color(
            palette::dimmed(palette::HYPE, 0.85),
            Vec2::new(RIBBON_W, 3.0),
        ),
        Anchor::CENTER_LEFT,
        Transform::from_xyz(-RIBBON_W / 2.0, 336.0, 5.0).with_scale(Vec3::new(0.0, 1.0, 1.0)),
    ));

    if layout.players() == 1 {
        spawn_solo_panels(&mut commands, &font);
        return;
    }
    let compact = layout.players() > 2;
    let score_size = if compact { 14.0 } else { 22.0 };
    let line_size = if compact { 9.0 } else { 12.0 };
    for index in players.iter() {
        let player = index.0;
        let origin = layout.origin(player);
        commands.spawn((
            GameplayScreen,
            ScoreText(player),
            Text2d::new("0"),
            font.text(score_size),
            TextColor(player_color(player)),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP, 5.0),
        ));
        commands.spawn((
            GameplayScreen,
            MultiplierText(player),
            Text2d::new("x1"),
            font.text(line_size),
            TextColor(palette::TEXT),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP - score_size * 1.6, 5.0),
        ));
        commands.spawn((
            GameplayScreen,
            ComboText(player),
            Text2d::new(""),
            font.text(line_size),
            TextColor(palette::TEXT_DIM),
            Anchor::TOP_CENTER,
            Transform::from_xyz(origin, HUD_TOP - score_size * 1.6 - line_size * 1.8, 5.0),
        ));
        // Hype meter: frame + left-anchored fill.
        let bar_width = layout.bed_width() * 0.6;
        let bar_y = HUD_TOP - score_size * 1.6 - line_size * 3.9;
        commands.spawn((
            GameplayScreen,
            Sprite::from_color(
                palette::dimmed(palette::HYPE, 0.25),
                Vec2::new(bar_width, 6.0),
            ),
            Transform::from_xyz(origin, bar_y, 4.0),
        ));
        commands.spawn((
            GameplayScreen,
            HypeFill(player),
            Sprite::from_color(palette::HYPE, Vec2::new(bar_width, 6.0)),
            Anchor::CENTER_LEFT,
            Transform::from_xyz(origin - bar_width / 2.0, bar_y, 5.0)
                .with_scale(Vec3::new(0.0, 1.0, 1.0)),
        ));
    }
}

/// A framed plate: a border rectangle with a darker fill on top.
///
/// Bevy sprites have no border, so a plate is two rectangles. The fill
/// is drawn slightly in front of the border so the border shows as a
/// hairline edge rather than being covered.
fn plate(commands: &mut Commands, center: Vec2, size: Vec2, accent: Color) {
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(palette::dimmed(accent, 0.55), size),
        Transform::from_xyz(center.x, center.y, 3.0),
    ));
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(
            palette::BACKGROUND.with_alpha(0.88),
            size - Vec2::splat(PLATE_BORDER * 2.0),
        ),
        Transform::from_xyz(center.x, center.y, 3.1),
    ));
}

/// A small caption above a readout.
fn caption(commands: &mut Commands, font: &UiFont, text: &str, at: Vec2) {
    commands.spawn((
        GameplayScreen,
        Text2d::new(text.to_owned()),
        font.text(8.0),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
        Anchor::TOP_CENTER,
        Transform::from_xyz(at.x, at.y, 5.0),
    ));
}

/// The solo layout: score and multiplier bottom-left, meter
/// bottom-right, nothing over the highway.
fn spawn_solo_panels(commands: &mut Commands, font: &UiFont) {
    let accent = player_color(0);
    let left = Vec2::new(
        -640.0 + PLATE_INSET + PLATE_W / 2.0,
        -360.0 + PLATE_INSET + PLATE_H / 2.0,
    );
    let right = Vec2::new(-left.x, left.y);
    let size = Vec2::new(PLATE_W, PLATE_H);

    // ── Left: the counter, the multiplier, the streak ───────────────
    plate(commands, left, size, accent);
    caption(
        commands,
        font,
        "SCORE",
        left + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    // A recessed well, so the digits read as sitting IN the plate.
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(
            palette::BACKGROUND.mix(&Color::BLACK, 0.5),
            Vec2::new(PLATE_W - 40.0, 34.0),
        ),
        Transform::from_xyz(left.x, left.y + PLATE_H / 2.0 - 44.0, 3.2),
    ));
    // Leading zeros dim, significant digits bright, as ONE line: the
    // number keeps a fixed width without the padding shouting as
    // loudly as the score. A counter whose digits shift about as it
    // grows is a sentence, not an instrument.
    commands
        .spawn((
            GameplayScreen,
            ScorePad,
            Text2d::new("00000"),
            font.text(24.0),
            TextColor(palette::dimmed(accent, 0.22)),
            Anchor::CENTER,
            Transform::from_xyz(left.x, left.y + PLATE_H / 2.0 - 44.0, 5.0),
        ))
        .with_child((
            ScoreText(0),
            TextSpan::new("0"),
            font.text(24.0),
            TextColor(accent),
        ));

    // The multiplier gets its own box: here it is a state, not a
    // statistic.
    let box_size = Vec2::new(72.0, 30.0);
    let box_at = Vec2::new(left.x - PLATE_W / 2.0 + 50.0, left.y - PLATE_H / 2.0 + 32.0);
    commands.spawn((
        GameplayScreen,
        MultiplierBox,
        Sprite::from_color(palette::dimmed(palette::TEXT_DIM, 0.5), box_size),
        Transform::from_xyz(box_at.x, box_at.y, 3.2),
    ));
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(
            palette::BACKGROUND.with_alpha(0.92),
            box_size - Vec2::splat(3.0),
        ),
        Transform::from_xyz(box_at.x, box_at.y, 3.3),
    ));
    commands.spawn((
        GameplayScreen,
        MultiplierText(0),
        Text2d::new("x1"),
        font.text(16.0),
        TextColor(palette::TEXT),
        Anchor::CENTER,
        Transform::from_xyz(box_at.x, box_at.y, 5.0),
    ));

    // Beads: how far the streak has come toward the next multiplier,
    // and — the part that matters — what a miss just cost.
    let bead_gap = 12.0f32;
    let bead_x = box_at.x + box_size.x / 2.0 + 20.0;
    for step in 0..STREAK_BEADS {
        commands.spawn((
            GameplayScreen,
            StreakBead(step),
            Sprite::from_color(palette::dimmed(palette::TEXT_DIM, 0.3), Vec2::splat(7.0)),
            Transform::from_xyz(bead_gap.mul_add(step as f32, bead_x), box_at.y, 4.0),
        ));
    }
    commands.spawn((
        GameplayScreen,
        ComboText(0),
        Text2d::new(""),
        font.text(9.0),
        TextColor(palette::TEXT_DIM),
        Anchor::TOP_CENTER,
        Transform::from_xyz(left.x, left.y - PLATE_H / 2.0 + 14.0, 5.0),
    ));

    // ── Right: the energy meter, in the quarters it fills in ────────
    plate(commands, right, size, palette::HYPE);
    caption(
        commands,
        font,
        "HYPE",
        right + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    let bar = Vec2::new(PLATE_W - 56.0, 24.0);
    let bar_y = right.y + 6.0;
    // Four wells. A completed phrase awards exactly a quarter, so a
    // continuous bar could never show what is being collected.
    let seg_w = 6.0f32.mul_add(-((HYPE_SEGMENTS - 1) as f32), bar.x) / HYPE_SEGMENTS as f32;
    for step in 0..HYPE_SEGMENTS {
        let x = (seg_w + 6.0).mul_add(step as f32, right.x - bar.x / 2.0 + seg_w / 2.0);
        commands.spawn((
            GameplayScreen,
            Sprite::from_color(
                palette::dimmed(palette::HYPE, 0.18),
                Vec2::new(seg_w, bar.y),
            ),
            Transform::from_xyz(x, bar_y, 3.4),
        ));
        commands.spawn((
            GameplayScreen,
            HypeSegment(step),
            Sprite::from_color(palette::HYPE, Vec2::new(seg_w, bar.y)),
            Transform::from_xyz(x, bar_y, 3.5),
        ));
    }
    // A hairline under the wells carrying the partial quarter, so the
    // meter still moves between whole segments.
    commands.spawn((
        GameplayScreen,
        HypeFill(0),
        Sprite::from_color(palette::HYPE.with_alpha(0.6), Vec2::new(bar.x, 3.0)),
        Anchor::CENTER_LEFT,
        Transform::from_xyz(right.x - bar.x / 2.0, bar_y - bar.y / 2.0 - 6.0, 5.0)
            .with_scale(Vec3::new(0.0, 1.0, 1.0)),
    ));
    commands.spawn((
        GameplayScreen,
        HypeReadyText,
        Text2d::new(""),
        font.text(9.0),
        TextColor(palette::HYPE),
        Anchor::TOP_CENTER,
        Transform::from_xyz(right.x, right.y - PLATE_H / 2.0 + 24.0, 5.0),
    ));
}

/// Push session numbers into every player's HUD.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_huds(
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut texts: ParamSet<(
        Query<(&ScoreText, &mut TextSpan)>,
        Query<(&ComboText, &mut Text2d)>,
        Query<(&MultiplierText, &mut Text2d, &mut TextColor)>,
        Query<&mut Text2d, With<ScorePad>>,
        Query<&mut Text2d, With<HypeReadyText>>,
    )>,
    mut fills: Query<(&HypeFill, &mut Transform)>,
    mut beads: Query<(&StreakBead, &mut Sprite), Without<HypeSegment>>,
    mut segments: Query<(&HypeSegment, &mut Sprite), Without<StreakBead>>,
    mut boxes: Query<
        &mut Sprite,
        (
            With<MultiplierBox>,
            Without<StreakBead>,
            Without<HypeSegment>,
        ),
    >,
) {
    for (index, player) in &players {
        let perf = player.session.performance();

        for (marker, mut span) in &mut texts.p0() {
            if marker.0 == index.0 {
                let score = perf.score().to_string();
                if span.0 != score {
                    span.0 = score;
                }
            }
        }
        for (marker, mut text) in &mut texts.p1() {
            if marker.0 == index.0 {
                let combo = if perf.streak() >= 4 {
                    format!("{} combo", perf.streak())
                } else {
                    String::new()
                };
                if text.0 != combo {
                    text.0 = combo;
                }
            }
        }
        for (marker, mut text, mut color) in &mut texts.p2() {
            if marker.0 == index.0 {
                let hype = perf.hype_active();
                let line = format!("x{}", perf.multiplier());
                if text.0 != line {
                    text.0 = line;
                }
                color.0 = if hype {
                    palette::HYPE
                } else if perf.multiplier() >= 4 {
                    palette::BRAND
                } else {
                    palette::TEXT
                };
            }
        }
        for (fill, mut transform) in &mut fills {
            if fill.0 == index.0 {
                transform.scale.x = perf.hype_meter() as f32;
            }
        }

        // Everything below is the solo plate, which only player one
        // owns; in multiplayer these queries simply find nothing.
        if index.0 != 0 {
            continue;
        }

        // The counter's padding shrinks as the score grows, so the
        // number stays the same width and only gains bright digits.
        if let Ok(mut pad) = texts.p3().single_mut() {
            let digits = perf.score().to_string().len();
            let wanted = "0".repeat(SCORE_DIGITS.saturating_sub(digits));
            if pad.0 != wanted {
                pad.0 = wanted;
            }
        }

        // Beads fill toward the next multiplier and empty on a miss,
        // which is the part a bare "x3" never showed.
        let per_level = player
            .session
            .performance()
            .config()
            .streak_per_level
            .max(1);
        let toward_next =
            if perf.multiplier() >= player.session.performance().config().max_multiplier {
                STREAK_BEADS
            } else {
                perf.streak() % per_level
            };
        for (bead, mut sprite) in &mut beads {
            sprite.color = if bead.0 < toward_next {
                palette::BRAND
            } else {
                palette::dimmed(palette::TEXT_DIM, 0.3)
            };
        }

        // Whole quarters light; the hairline underneath carries the
        // part-quarter in progress.
        let meter = perf.hype_meter() as f32;
        let filled = (meter * HYPE_SEGMENTS as f32).floor() as u32;
        for (segment, mut sprite) in &mut segments {
            let lit = segment.0 < filled;
            sprite.color = if !lit {
                Color::NONE
            } else if perf.hype_active() {
                Color::WHITE
            } else {
                palette::HYPE
            };
        }

        if let Ok(mut sprite) = boxes.single_mut() {
            sprite.color = if perf.hype_active() {
                palette::HYPE
            } else if perf.multiplier() >= 4 {
                palette::BRAND
            } else {
                palette::dimmed(palette::TEXT_DIM, 0.5)
            };
        }

        if let Ok(mut text) = texts.p4().single_mut() {
            let line = if perf.hype_active() {
                "HYPE RUNNING - DOUBLE POINTS"
            } else if meter >= 0.5 {
                "READY - PRESS HYPE"
            } else {
                "HIT MARKED NOTES TO FILL"
            };
            if text.0 != line {
                text.0 = line.to_owned();
            }
        }
    }
}

/// Advance the song ribbon: the bar fills, the clock counts.
pub fn update_song_ribbon(
    song: Res<crate::boot::LoadedSong>,
    game_clock: Res<super::GameClock>,
    time: Res<Time>,
    mut fill: Query<&mut Transform, With<SongProgressFill>>,
    mut label: Query<&mut Text2d, With<SongTimeText>>,
) {
    let Some(now) = game_clock.song_time(&time) else {
        return;
    };
    // A chart need not declare a duration; the last note is then the
    // best available end, and it is the end that matters to a player.
    let duration = song.chart.song.duration_s.unwrap_or_else(|| {
        song.chart
            .charts
            .iter()
            .flat_map(|chart| chart.notes.iter())
            .map(|note| note.time + note.len)
            .fold(0.0, f64::max)
    });
    let progress = song_progress(now, duration);
    for mut transform in &mut fill {
        transform.scale.x = progress;
    }
    for mut text in &mut label {
        text.0 = format!("{} / {}", clock_text(now), clock_text(duration));
    }
}

#[cfg(test)]
mod ribbon_tests {
    use super::{clock_text, song_progress};

    #[test]
    fn the_bar_does_not_fill_backwards_during_the_count_in() {
        // The song clock starts NEGATIVE: there is a pre-roll before
        // the first beat. Unclamped, the bar would start somewhere in
        // the middle and run backwards to zero.
        assert!((song_progress(-2.0, 200.0) - 0.0).abs() < f32::EPSILON);
        assert!((song_progress(-0.01, 200.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_bar_does_not_overflow_past_the_end() {
        // Play runs a little past the last note before the results
        // screen takes over.
        assert!((song_progress(260.0, 200.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_bar_tracks_the_song_in_between() {
        assert!((song_progress(50.0, 200.0) - 0.25).abs() < 1e-6);
        assert!((song_progress(100.0, 200.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_song_of_no_length_leaves_the_bar_empty() {
        // Rather than dividing by zero and filling with NaN, which
        // scales a sprite to nothing visible and is unbounded.
        assert!((song_progress(10.0, 0.0) - 0.0).abs() < f32::EPSILON);
        assert!(song_progress(10.0, 0.0).is_finite());
    }

    #[test]
    fn the_clock_never_shows_a_negative_time() {
        // "-0:02" during the count-in reads as a fault, not as a wait.
        assert_eq!(clock_text(-2.0), "0:00");
        assert_eq!(clock_text(0.0), "0:00");
    }

    #[test]
    fn the_clock_pads_seconds_and_rolls_minutes() {
        assert_eq!(clock_text(9.0), "0:09");
        assert_eq!(clock_text(59.9), "0:59");
        assert_eq!(clock_text(60.0), "1:00");
        assert_eq!(clock_text(125.0), "2:05");
        assert_eq!(clock_text(3661.0), "61:01");
    }
}
