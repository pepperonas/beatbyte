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

/// A streak bulb's socket ring — its rim lights with the fill.
#[derive(Component)]
pub struct BulbRim;

/// The frame around the multiplier, which lights while hype runs.
#[derive(Component)]
pub struct MultiplierBox;

/// The line under the meter: what the player can do with it now.
#[derive(Component)]
pub struct HypeReadyText;

/// The Hype gauge's needle (solo panel).
#[derive(Component)]
pub struct HypeNeedle;

/// The hub the gauge needle pivots on (solo panel).
#[derive(Component)]
pub struct HypeHub;

/// The multiplier's pop animation state.
#[derive(Component)]
pub struct MultiplierPop {
    /// The multiplier last shown.
    pub seen: u32,
    /// Seconds since it last changed.
    pub age: f32,
}

/// The streak counter's pop animation state.
#[derive(Component)]
pub struct StreakPop {
    /// The streak value last shown.
    pub seen: u32,
    /// Seconds since it last rose.
    pub age: f32,
}

/// The needle's rotation for a meter level, radians. The sweep runs
/// −80°..+80° (empty → full) rather than the full half circle: a
/// needle lying flat on the horizon reads as broken, not as empty.
#[must_use]
pub fn gauge_angle(meter: f32) -> f32 {
    let sweep = 80.0f32.to_radians();
    sweep - meter.clamp(0.0, 1.0) * 2.0 * sweep
}

/// The streak counter's scale over its pop: an instant swell to
/// ~1.4× that settles within a fifth of a second — visible at note
/// rate without turning the corner into a metronome.
#[must_use]
pub fn pop_scale(age: f32) -> f32 {
    let t = (age / 0.2).clamp(0.0, 1.0);
    1.0 + 0.4 * (1.0 - t) * (1.0 - t)
}

/// Digits the counter reserves. Six is beyond any real score; the
/// point is the fixed width — a number that shifts about as it grows
/// reads as a sentence rather than an instrument.
const SCORE_DIGITS: usize = 6;

/// Beads in the streak row — one multiplier level's worth.
const STREAK_BEADS: u32 = 10;

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

/// The ribbon's title text - marked so an MC-set song swap can
/// rewrite it (it used to be written once at spawn and never again).
#[derive(Component)]
pub struct SongTitleText;

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
    shapes: Res<crate::shapes::LaneShapes>,
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
        SongTitleText,
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
        spawn_solo_panels(&mut commands, &font, &shapes);
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
fn plate(
    commands: &mut Commands,
    shapes: &crate::shapes::LaneShapes,
    center: Vec2,
    size: Vec2,
    accent: Color,
) {
    commands.spawn((
        GameplayScreen,
        Sprite::from_color(palette::dimmed(accent, 0.55), size),
        Transform::from_xyz(center.x, center.y, 3.0),
    ));
    // Brushed dark metal with a top-edge light catch and corner
    // rivets, tinted toward the accent: stage hardware, not a
    // coloured rectangle.
    commands.spawn((
        GameplayScreen,
        Sprite {
            image: shapes.plate(),
            color: palette::BACKGROUND.mix(&accent, 0.35),
            custom_size: Some(size - Vec2::splat(PLATE_BORDER * 2.0)),
            ..default()
        },
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
fn spawn_solo_panels(commands: &mut Commands, font: &UiFont, shapes: &crate::shapes::LaneShapes) {
    let accent = player_color(0);
    let left = Vec2::new(
        -640.0 + PLATE_INSET + PLATE_W / 2.0,
        -360.0 + PLATE_INSET + PLATE_H / 2.0,
    );
    let right = Vec2::new(-left.x, left.y);
    let size = Vec2::new(PLATE_W, PLATE_H);

    // ── Left: the counter, the multiplier, the streak ───────────────
    plate(commands, shapes, left, size, accent);
    caption(
        commands,
        font,
        "SCORE",
        left + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    // A recessed well, so the digits read as sitting IN the plate.
    commands.spawn((
        GameplayScreen,
        Sprite {
            image: shapes.well(),
            custom_size: Some(Vec2::new(PLATE_W - 40.0, 34.0)),
            ..default()
        },
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
    let box_at = Vec2::new(left.x - PLATE_W / 2.0 + 50.0, left.y - PLATE_H / 2.0 + 42.0);
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
        MultiplierPop { seen: 1, age: 1.0 },
        Text2d::new("x1"),
        font.text(16.0),
        TextColor(palette::TEXT),
        Anchor::CENTER,
        Transform::from_xyz(box_at.x, box_at.y, 5.0),
    ));

    // Beads: GH2's own grammar — "10 little dots above the
    // multiplier, each dot one note of the combo" (WikiHero,
    // Multiplier) — drawn CRISP: a socket ring whose rim lights
    // with the fill and a solid core, no additive halo. The first
    // version stacked 26 px soft-dot halos on a 13 px pitch; they
    // overlapped into one smear (user report: "zu viel glow").
    let bead_gap = 13.0f32;
    let bead_x = box_at.x + box_size.x / 2.0 + 18.0;
    for step in 0..STREAK_BEADS {
        let x = bead_gap.mul_add(step as f32, bead_x);
        // The socket ring: its rim takes the lit color, so a filled
        // dot reads as a lamp ON, not as a blob over a lamp.
        commands.spawn((
            GameplayScreen,
            StreakBead(step),
            BulbRim,
            Sprite {
                image: shapes.round_ring(),
                color: palette::dimmed(palette::TEXT_DIM, 0.5),
                custom_size: Some(Vec2::splat(11.0)),
                ..default()
            },
            Transform::from_xyz(x, box_at.y, 3.8),
        ));
        // The core: solid when lit, near-dark when not.
        commands.spawn((
            GameplayScreen,
            StreakBead(step),
            Sprite {
                image: shapes.round_core(),
                color: palette::dimmed(palette::TEXT_DIM, 0.25),
                custom_size: Some(Vec2::splat(7.0)),
                ..default()
            },
            Transform::from_xyz(x, box_at.y, 4.0),
        ));
    }
    // The streak counter, GH2-Deluxe style: bright digits with their
    // own clear line (it used to sit centred under the plate where
    // the pop scaled it INTO the bead row — "nicht gut sichtbar").
    commands.spawn((
        GameplayScreen,
        ComboText(0),
        StreakPop { seen: 0, age: 1.0 },
        Text2d::new(""),
        font.text(12.0),
        TextColor(palette::BRAND),
        Anchor::TOP_LEFT,
        Transform::from_xyz(
            box_at.x - box_size.x / 2.0,
            left.y - PLATE_H / 2.0 + 18.0,
            5.0,
        ),
    ));

    // ── Right: the energy meter, in the quarters it fills in ────────
    plate(commands, shapes, right, size, palette::HYPE);
    caption(
        commands,
        font,
        "HYPE",
        right + Vec2::new(0.0, PLATE_H / 2.0 - 12.0),
    );
    // The meter is a GAUGE: a half-circle dial with a needle, the
    // way the genre's classic meters read — the halfway tick is the
    // activation threshold, so "can I fire it?" is one glance at
    // which side of straight-up the needle stands.
    let pivot = Vec2::new(right.x, right.y - PLATE_H / 2.0 + 34.0);
    let dial = Vec2::new(150.0, 75.0);
    commands.spawn((
        GameplayScreen,
        Sprite {
            image: shapes.gauge_arc(),
            color: palette::dimmed(palette::HYPE, 0.85),
            custom_size: Some(dial),
            ..default()
        },
        Anchor::BOTTOM_CENTER,
        Transform::from_xyz(pivot.x, pivot.y, 3.4),
    ));
    commands.spawn((
        GameplayScreen,
        HypeNeedle,
        Sprite {
            image: shapes.glow_strip(),
            color: palette::TEXT,
            custom_size: Some(Vec2::new(4.0, 74.0)),
            ..default()
        },
        // The pivot sits 18 % up the strip, so the short end past
        // the hub reads as the needle's counterweight — a real dial
        // needle, not a rotating line.
        Anchor(Vec2::new(0.0, -0.32)),
        Transform::from_xyz(pivot.x, pivot.y, 3.6)
            .with_rotation(Quat::from_rotation_z(gauge_angle(0.0))),
    ));
    // The hub the needle pivots on.
    commands.spawn((
        GameplayScreen,
        HypeHub,
        Sprite {
            image: shapes.round_core(),
            color: palette::dimmed(palette::HYPE, 0.9),
            custom_size: Some(Vec2::splat(14.0)),
            ..default()
        },
        Transform::from_xyz(pivot.x, pivot.y, 3.7),
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
    mut fills: Query<(&HypeFill, &mut Transform), Without<HypeNeedle>>,
    mut needles: Query<&mut Transform, (With<HypeNeedle>, Without<HypeFill>)>,
    mut beads: Query<(&StreakBead, Has<BulbRim>, &mut Sprite)>,
    mut boxes: Query<&mut Sprite, (With<MultiplierBox>, Without<StreakBead>)>,
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
                let combo = if perf.streak() >= 2 {
                    format!("{} COMBO", perf.streak())
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
        // The solo gauge's needle sweeps with the meter; while Hype
        // runs it blazes white on a lit hub.
        if index.0 == 0 {
            for mut transform in &mut needles {
                transform.rotation = Quat::from_rotation_z(gauge_angle(perf.hype_meter() as f32));
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
        let lit_color = if perf.hype_active() {
            palette::HYPE
        } else {
            palette::BRAND
        };
        for (bead, rim, mut sprite) in &mut beads {
            let lit = bead.0 < toward_next;
            let next = bead.0 == toward_next && toward_next < STREAK_BEADS;
            sprite.color = match (lit, rim) {
                // The ring rim takes the lit color; a filled dot in
                // a lit ring reads as a lamp, crisply, without any
                // additive halo (they smeared at this pitch).
                (true, true) => lit_color,
                (false, true) => palette::dimmed(palette::TEXT_DIM, 0.5),
                (true, false) => lit_color.mix(&Color::WHITE, 0.25),
                // The bulb about to fill carries a faint ember — the
                // row points at where the streak is going.
                (false, false) if next => palette::dimmed(lit_color, 0.35),
                (false, false) => palette::dimmed(palette::TEXT_DIM, 0.25),
            };
        }

        let meter = perf.hype_meter() as f32;
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

/// Make the streak counter POP as it counts up: every rise swells
/// the number and lets it settle — the count is felt, not read.
pub fn pop_streak(
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut counters: Query<(&ComboText, &mut StreakPop, &mut Transform)>,
) {
    for (marker, mut pop, mut transform) in &mut counters {
        let Some((_, player)) = players.iter().find(|(index, _)| index.0 == marker.0) else {
            continue;
        };
        let streak = player.session.performance().streak();
        if streak > pop.seen {
            pop.age = 0.0;
        }
        pop.seen = streak;
        pop.age += time.delta_secs();
        let scale = pop_scale(pop.age);
        transform.scale = Vec3::splat(scale);
    }
}

/// Pop the multiplier when it CHANGES — up a level or lost to a
/// miss, both are moments the eye should be handed.
pub fn pop_multiplier(
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut counters: Query<(&MultiplierText, &mut MultiplierPop, &mut Transform)>,
) {
    for (marker, mut pop, mut transform) in &mut counters {
        let Some((_, player)) = players.iter().find(|(index, _)| index.0 == marker.0) else {
            continue;
        };
        let multiplier = player.session.performance().multiplier();
        if multiplier != pop.seen {
            pop.age = 0.0;
        }
        pop.seen = multiplier;
        pop.age += time.delta_secs();
        transform.scale = Vec3::splat(pop_scale(pop.age));
    }
}

/// The glow blend for the gauge while it stands READY: a slow
/// breath between a third and two thirds of the hot tone, so the
/// dial visibly asks to be fired without strobing.
#[must_use]
pub fn ready_glow(t: f32) -> f32 {
    0.16f32.mul_add((t * 4.0).sin(), 0.42)
}

/// Breathe the gauge when it matters: hub and needle pulse toward
/// the Hype tone once the meter can fire, and blaze steady white-hot
/// while Hype runs. Transforms and sprite tints only — no material
/// or texture writes.
pub fn pulse_gauge(
    time: Res<Time>,
    players: Query<(&PlayerIndex, &PlayerSession)>,
    mut needles: Query<&mut Sprite, (With<HypeNeedle>, Without<HypeHub>)>,
    mut hubs: Query<&mut Sprite, (With<HypeHub>, Without<HypeNeedle>)>,
) {
    let Some((_, player)) = players.iter().find(|(index, _)| index.0 == 0) else {
        return;
    };
    let perf = player.session.performance();
    let (needle_color, hub_color) = if perf.hype_active() {
        (
            palette::HYPE.mix(&Color::WHITE, 0.7),
            palette::HYPE.mix(&Color::WHITE, 0.5),
        )
    } else if perf.hype_meter() >= 0.5 {
        let blend = ready_glow(time.elapsed_secs());
        (
            palette::TEXT.mix(&palette::HYPE, blend),
            palette::dimmed(palette::HYPE, 0.9).mix(&Color::WHITE, blend * 0.5),
        )
    } else {
        (palette::TEXT, palette::dimmed(palette::HYPE, 0.9))
    };
    for mut sprite in &mut needles {
        sprite.color = needle_color;
    }
    for mut sprite in &mut hubs {
        sprite.color = hub_color;
    }
}

/// Advance the song ribbon: the bar fills, the clock counts.
pub fn update_song_ribbon(
    song: Res<crate::boot::LoadedSong>,
    game_clock: Res<super::GameClock>,
    time: Res<Time>,
    font: Res<crate::ui::UiFont>,
    mut fill: Query<&mut Transform, With<SongProgressFill>>,
    mut label: Query<&mut Text2d, (With<SongTimeText>, Without<SongTitleText>)>,
    mut title: Query<&mut Text2d, (With<SongTitleText>, Without<SongTimeText>)>,
) {
    // An MC-set swap replaces the song resource mid-gameplay; the
    // ribbon's title must follow it.
    if song.is_changed()
        && let Ok(mut text) = title.single_mut()
    {
        let wanted = format!(
            "{} - {}",
            font.safe(&song.chart.song.title),
            font.safe(&song.chart.song.artist)
        );
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
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

#[cfg(test)]
mod gauge_tests {
    use super::{gauge_angle, pop_scale, ready_glow};

    #[test]
    fn the_ready_breath_stays_a_glow_and_never_a_strobe() {
        // The blend must MOVE (it is a breath), but stay inside a
        // band that never reaches full-off or full-hot — a dial that
        // blinks to black reads as a fault light.
        let samples: Vec<f32> = (0..100).map(|i| ready_glow(i as f32 * 0.05)).collect();
        let lo = samples.iter().copied().fold(f32::MAX, f32::min);
        let hi = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            lo > 0.2 && hi < 0.7,
            "band {lo}..{hi} escapes the glow range"
        );
        assert!(hi - lo > 0.25, "breath {lo}..{hi} too shallow to see");
    }

    #[test]
    fn the_needle_sweeps_left_to_right_and_stands_up_at_the_threshold() {
        // Empty leans left (+80°), full leans right (−80°) — and the
        // halfway ACTIVATION mark is the needle standing straight
        // up, so "can I fire it?" is which side of vertical.
        assert!((gauge_angle(0.0) - 80f32.to_radians()).abs() < 1e-6);
        assert!(gauge_angle(0.5).abs() < 1e-6);
        assert!((gauge_angle(1.0) + 80f32.to_radians()).abs() < 1e-6);
        // Out-of-range meters clamp instead of spinning the needle
        // off the dial.
        assert_eq!(gauge_angle(1.7), gauge_angle(1.0));
        assert_eq!(gauge_angle(-0.3), gauge_angle(0.0));
    }

    #[test]
    fn the_pop_swells_instantly_and_settles_fast() {
        assert!((pop_scale(0.0) - 1.4).abs() < 1e-6, "full swell at the hit");
        assert!(pop_scale(0.1) > 1.0 && pop_scale(0.1) < 1.4);
        assert!((pop_scale(0.2) - 1.0).abs() < 1e-6, "settled after 0.2s");
        assert!((pop_scale(9.0) - 1.0).abs() < 1e-6, "idle stays at rest");
    }
}
