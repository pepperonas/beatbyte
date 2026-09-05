//! Live karaoke lyrics: the song's words, sung along the same clock
//! the notes fall on.
//!
//! One timebase (commission §21): the display reads
//! [`GameClock::visual_time`] — song position plus the video offset,
//! exactly what note drawing uses — plus the player's own lyrics
//! offset. Judgment reads none of it.
//!
//! One layout trick makes the karaoke fill cheap: Press Start 2P
//! advances exactly 1 em per glyph, so a line is laid out as one
//! `Text2d` PER CHARACTER at computed offsets, and the fill is
//! nothing but per-glyph `TextColor` writes. No strings are built
//! per frame, no text re-measures, and entities churn only when the
//! line changes — a handful of times per song.
//!
//! The fill is driven from the finest timing the song has (plan L4):
//! per-character spans from an alignment, else the word's span split
//! evenly across its letters, else — a line without word stamps —
//! the line fades in, holds, and fades out. It never pretends to know
//! word timing it does not have (commission §30).
//!
//! A line has a real END: once its last word is sung it dims instead
//! of staying "in progress" until the next line. The next line
//! appears a *lead-in* before its first word (its band grows across
//! the lead-in, so the eye knows what is coming), and a long
//! instrumental gap ends in a countdown of four pulses on the beat —
//! all read off the same clock the notes fall on.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use beatbyte_chart::lyrics::cue_at;

use crate::audio_sys::GameClock;
use crate::boot::LoadedSong;
use crate::config::Settings;
use crate::palette;
use crate::ui::UiFont;

/// Vertical center of the active line, in HUD world units: above the
/// highway's vanishing point, below the song ribbon — the one band
/// that covers neither notes nor HUD.
const LYRIC_Y: f32 = 262.0;
/// The widest a line may render, in world units.
const MAX_LINE_W: f32 = 1120.0;
/// Font size per size step (small / medium / large).
const SIZES: [f32; 3] = [16.0, 21.0, 26.0];
/// The preview line renders at this fraction of the active size.
const PREVIEW_SCALE: f32 = 0.62;
/// Seconds a line-timed lyric takes to fade in or out.
const LINE_FADE_S: f64 = 0.3;
/// Seconds the line-entry ease takes (motion-gated).
const ENTER_S: f32 = 0.18;

/// The lyric size for a step, clamped.
fn size_for(step: u8) -> f32 {
    SIZES[usize::from(step.min(2))]
}

/// One glyph's window on the song clock: the fill crosses the glyph
/// from `start` to `end` (song seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphCue {
    /// The word this glyph belongs to.
    pub word: usize,
    /// When the glyph starts filling.
    pub start: f64,
    /// When it is fully lit.
    pub end: f64,
}

/// Map every character of a line to its window. A word with aligned
/// character spans hands each glyph its own; a word without splits
/// its span evenly across its letters. Characters outside any
/// stamped word (spaces, unstamped lead-ins) light when their
/// preceding word completes; a lead-in before the first word lights
/// the moment that word starts. Pure — tested.
#[must_use]
pub fn glyph_cues(text: &str, words: &[beatbyte_chart::lyrics::LyricWord]) -> Vec<GlyphCue> {
    let chars: Vec<char> = text.chars().collect();
    let first_start = words.first().map_or(0.0, |word| word.start);
    let mut cues = vec![
        GlyphCue {
            word: 0,
            start: first_start,
            end: first_start,
        };
        chars.len()
    ];
    let mut cursor = 0usize;
    for (index, word) in words.iter().enumerate() {
        let needle: Vec<char> = word.text.chars().collect();
        if needle.is_empty() {
            continue;
        }
        let Some(found) = find_chars(&chars, &needle, cursor) else {
            continue;
        };
        // Everything between the previous word and this one waits
        // for the previous word to finish.
        if index > 0 {
            let previous_end = words[index - 1].end;
            for cue in cues.iter_mut().take(found).skip(cursor) {
                *cue = GlyphCue {
                    word: index - 1,
                    start: previous_end,
                    end: previous_end,
                };
            }
        }
        let aligned = word.chars.len() == needle.len();
        let len = needle.len() as f64;
        let span = word.end - word.start;
        for offset in 0..needle.len() {
            let (start, end) = if aligned {
                (word.chars[offset][0], word.chars[offset][1])
            } else {
                (
                    word.start + span * offset as f64 / len,
                    word.start + span * (offset + 1) as f64 / len,
                )
            };
            cues[found + offset] = GlyphCue {
                word: index,
                start,
                end,
            };
        }
        cursor = found + needle.len();
    }
    // A tail after the last word follows that word's completion.
    if let Some(last) = words.last() {
        for cue in cues.iter_mut().skip(cursor) {
            *cue = GlyphCue {
                word: words.len() - 1,
                start: last.end,
                end: last.end,
            };
        }
    }
    cues
}

/// First occurrence of `needle` in `haystack` at or after `start`.
fn find_chars(haystack: &[char], needle: &[char], start: usize) -> Option<usize> {
    (start..=haystack.len().saturating_sub(needle.len()))
        .find(|&at| haystack[at..at + needle.len()] == *needle)
}

/// How lit one glyph is at `position`: 0 dark, 1 sung, linear across
/// the glyph's own window — a window is a letter's worth of time, so
/// the edge reads as hard. A degenerate window (the waiting spaces)
/// snaps when crossed. Pure — tested.
#[must_use]
pub fn glyph_fill(cue: &GlyphCue, position: f64) -> f32 {
    let span = cue.end - cue.start;
    if span <= f64::EPSILON {
        return if position >= cue.end { 1.0 } else { 0.0 };
    }
    (((position - cue.start) / span) as f32).clamp(0.0, 1.0)
}

/// What the display shows at a song position — [`cue_at`] with the
/// lead-in and the real line end folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayCue {
    /// The line on the active row, if any.
    pub active: Option<usize>,
    /// What the active line is doing.
    pub phase: Phase,
    /// The line for the preview row, if any.
    pub upcoming: Option<usize>,
}

/// The active line's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing on the active row.
    Idle,
    /// On screen ahead of its first word.
    LeadIn,
    /// Being sung.
    Singing,
    /// Every word sung; the line dims until the next takes over.
    Sung,
}

/// Where the singing of a line ends: its last word's end, or — a
/// line without word timing — its display end.
fn sung_end(line: &beatbyte_chart::lyrics::LyricLine) -> f64 {
    line.words.last().map_or(line.end, |word| word.end)
}

/// The display cue at `position` with a `lead_in` (seconds). The
/// next line takes the active row `lead_in` before its first word —
/// but only once the current line is SUNG (or there is none): a
/// line still being sung is never cut for a preview. Pure — tested.
#[must_use]
pub fn display_cue(
    lyrics: &beatbyte_chart::lyrics::Lyrics,
    position: f64,
    lead_in: f64,
) -> DisplayCue {
    let base = cue_at(lyrics, position);
    let current = base.active.map(|index| {
        let line = &lyrics.lines[index];
        let phase = if !line.words.is_empty() && position >= sung_end(line) {
            Phase::Sung
        } else {
            Phase::Singing
        };
        (index, phase)
    });
    if let Some(next) = base.upcoming
        && lyrics.lines[next].start - position <= lead_in
        && current.is_none_or(|(_, phase)| phase == Phase::Sung)
    {
        return DisplayCue {
            active: Some(next),
            phase: Phase::LeadIn,
            upcoming: (next + 1 < lyrics.lines.len()).then_some(next + 1),
        };
    }
    match current {
        Some((index, phase)) => DisplayCue {
            active: Some(index),
            phase,
            upcoming: base.upcoming,
        },
        None => DisplayCue {
            active: None,
            phase: Phase::Idle,
            upcoming: base.upcoming,
        },
    }
}

/// Gaps longer than this end in a countdown.
pub const COUNTDOWN_GAP_S: f64 = 4.0;
/// Pulses in the countdown.
pub const COUNTDOWN_PULSES: usize = 4;

/// The countdown before a line: how many of the four pulses are lit
/// at `position`, when a countdown is due at all — the line starts
/// more than [`COUNTDOWN_GAP_S`] after the previous singing ended
/// (`gap_from`), and the position is within four beats of it. `None`
/// = no countdown on show. Pure — tested.
#[must_use]
pub fn countdown_lit(position: f64, line_start: f64, gap_from: f64, beat_s: f64) -> Option<usize> {
    if beat_s <= 0.0 || !beat_s.is_finite() || line_start - gap_from <= COUNTDOWN_GAP_S {
        return None;
    }
    let remaining = line_start - position;
    if remaining < 0.0 || remaining > beat_s * COUNTDOWN_PULSES as f64 {
        return None;
    }
    // Pulse k lights when fewer than (PULSES - k) beats remain.
    let lit = (COUNTDOWN_PULSES as f64 - remaining / beat_s).floor();
    Some((lit.max(0.0) as usize).min(COUNTDOWN_PULSES))
}

/// The fade of a line-timed lyric at `position`: rises over
/// `LINE_FADE_S` from the start, falls over the same span into the
/// end, 1 in between. Pure — tested.
#[must_use]
pub fn line_alpha(start: f64, end: f64, position: f64) -> f32 {
    if position < start || position > end {
        return 0.0;
    }
    let rise = ((position - start) / LINE_FADE_S).clamp(0.0, 1.0);
    let fall = ((end - position) / LINE_FADE_S).clamp(0.0, 1.0);
    (rise.min(fall)) as f32
}

/// What the display currently has built, so rebuilds happen only on
/// real changes (line, size, toggle, song).
#[derive(Resource, Default)]
pub struct LyricDisplay {
    /// The line index the glyph row was built for.
    line: Option<usize>,
    /// The size step it was built at.
    size_step: u8,
    /// When the current line's glyphs appeared (for the entry ease).
    entered_at: f32,
}

/// Marker for everything the lyric display owns.
#[derive(Component)]
pub struct LyricPart;

/// One glyph of the active line.
#[derive(Component)]
pub struct LyricGlyph {
    /// This glyph's window (None = line-timed lyric).
    cue: Option<GlyphCue>,
    /// The glyph's resting position.
    home: Vec3,
}

/// One pulse of the gap countdown.
#[derive(Component)]
pub struct LyricPulse(usize);

/// A sung line dims to this alpha until the next takes over.
const SUNG_ALPHA: f32 = 0.55;
/// Countdown pulse size and spacing, in world units.
const PULSE_SIZE: f32 = 10.0;
const PULSE_GAP: f32 = 22.0;

/// The dimmed preview of the next line.
#[derive(Component)]
pub struct LyricPreview;

/// The soft backing that keeps the text readable over the LED wall.
#[derive(Component)]
pub struct LyricScrim;

/// The band behind the line being sung RIGHT NOW.
///
/// The scrim keeps everything legible; this marks *which* passage is
/// current, in the background, while the neighbouring lines keep
/// their ordinary look. It is a flat brand-tinted bar rather than a
/// pulse, so it costs nothing under `reduced motion` and cannot
/// compete with the note highway for attention.
#[derive(Component)]
pub struct LyricHighlight;

/// How opaque the current passage's band is.
///
/// ⚠️ The band DARKENS. The first version tinted with plain
/// [`palette::BRAND`] at low alpha, which is a mid-luminance yellow:
/// measured on a real frame it left the white glyphs sitting on it
/// at **3.83:1**, under the 4.5:1 a line of lyrics needs. A
/// highlight that carries light text has to go the other way — deep
/// amber, so the marking reads against the LED wall *and* the words
/// gain contrast instead of losing it.
const HIGHLIGHT_ALPHA: f32 = 0.92;
/// How far the band's tint is pulled toward black. See
/// [`HIGHLIGHT_ALPHA`].
const HIGHLIGHT_SHADE: f32 = 0.85;

/// The band's colour: the brand hue, deepened. Pure — tested for
/// contrast against the text that sits on it.
#[must_use]
pub fn highlight_color() -> Color {
    palette::BRAND
        .mix(&Color::BLACK, HIGHLIGHT_SHADE)
        .with_alpha(HIGHLIGHT_ALPHA)
}

/// The glyph row's query, aliased for the lint's sake.
type GlyphQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LyricGlyph,
        &'static mut TextColor,
        &'static mut Transform,
    ),
    Without<LyricPreview>,
>;
/// The preview line's query.
type PreviewQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text2d,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (With<LyricPreview>, Without<LyricGlyph>, Without<LyricScrim>),
>;
/// The scrim's query.
type ScrimQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Sprite, &'static mut Visibility),
    (With<LyricScrim>, Without<LyricPreview>, Without<LyricGlyph>),
>;
/// The current passage's band.
type HighlightQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Sprite,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        With<LyricHighlight>,
        Without<LyricScrim>,
        Without<LyricPreview>,
        Without<LyricGlyph>,
    ),
>;

/// Spawn the persistent parts (preview + scrim); glyphs come and go
/// with the lines.
pub fn spawn_lyric_display(
    mut commands: Commands,
    font: Res<UiFont>,
    mut display: ResMut<LyricDisplay>,
) {
    *display = LyricDisplay::default();
    commands.spawn((
        super::GameplayScreen,
        LyricPart,
        LyricScrim,
        Sprite::from_color(Color::srgba(0.0, 0.0, 0.0, 0.5), Vec2::new(0.0, 0.0)),
        Visibility::Hidden,
        Transform::from_xyz(0.0, LYRIC_Y - 12.0, 3.8),
    ));
    commands.spawn((
        super::GameplayScreen,
        LyricPart,
        LyricHighlight,
        Sprite::from_color(highlight_color(), Vec2::new(0.0, 0.0)),
        Visibility::Hidden,
        Transform::from_xyz(0.0, LYRIC_Y, 3.9),
    ));
    commands.spawn((
        super::GameplayScreen,
        LyricPart,
        LyricPreview,
        Text2d::new(""),
        font.mono_text(SIZES[1] * PREVIEW_SCALE),
        TextColor(palette::dimmed(palette::TEXT_DIM, 0.55)),
        Anchor::CENTER,
        Transform::from_xyz(0.0, LYRIC_Y, 4.0),
    ));
    // The countdown: four squares above the row, lit one per beat.
    let span = PULSE_GAP * (COUNTDOWN_PULSES as f32 - 1.0);
    for pulse in 0..COUNTDOWN_PULSES {
        commands.spawn((
            super::GameplayScreen,
            LyricPart,
            LyricPulse(pulse),
            Sprite::from_color(palette::dimmed(palette::TEXT, 0.3), Vec2::splat(PULSE_SIZE)),
            Visibility::Hidden,
            Transform::from_xyz(-span / 2.0 + pulse as f32 * PULSE_GAP, LYRIC_Y, 4.1),
        ));
    }
}

/// The countdown pulses' query.
type PulseQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static LyricPulse,
        &'static mut Sprite,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (
        With<LyricPulse>,
        Without<LyricHighlight>,
        Without<LyricScrim>,
        Without<LyricPreview>,
        Without<LyricGlyph>,
    ),
>;

/// The per-frame drive: cue from the shared clock, rebuild on line
/// changes, fill glyphs by their windows.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub fn update_lyrics(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    song: Res<LoadedSong>,
    game_clock: Res<GameClock>,
    font: Res<UiFont>,
    mut display: ResMut<LyricDisplay>,
    mut glyphs: GlyphQuery,
    mut preview: PreviewQuery,
    mut scrim: ScrimQuery,
    mut highlight: HighlightQuery,
    mut pulses: PulseQuery,
) {
    let lyrics = song.lyrics.as_ref().filter(|_| settings.lyrics);
    // A song swap (MC set) or a disabled setting clears the board.
    if song.is_changed() || lyrics.is_none() {
        if display.line.is_some() || lyrics.is_none() {
            clear_glyphs(&mut commands, &glyphs);
            display.line = None;
            hide_chrome(&mut preview, &mut scrim, &mut highlight, &mut pulses);
        }
        if lyrics.is_none() {
            return;
        }
    }
    let Some(lyrics) = lyrics else {
        return;
    };
    let Some(now) = game_clock.visual_time(&time, &settings) else {
        return;
    };
    // The global offset AND the song's own: sources vary per song.
    let position = now
        + f64::from(settings.lyrics_offset_ms) / 1000.0
        + f64::from(song.lyric_offset_ms) / 1000.0;
    let lead_in = f64::from(settings.lyrics_lead_in_ms) / 1000.0;
    let cue = display_cue(lyrics, position, lead_in);

    // Rebuild the glyph row when the active line or the size changed.
    if display.line != cue.active || display.size_step != settings.lyrics_size {
        clear_glyphs(&mut commands, &glyphs);
        display.line = cue.active;
        display.size_step = settings.lyrics_size;
        display.entered_at = time.elapsed_secs();
        if let Some(line) = cue.active.and_then(|index| lyrics.lines.get(index)) {
            spawn_line_glyphs(&mut commands, &font, &settings, line);
        }
    }

    // The preview: the next line, dimmed, below the active one.
    let wanted_preview = cue
        .upcoming
        .and_then(|index| lyrics.lines.get(index))
        // Only tease a line that is actually near (a preview half a
        // song early reads as a stuck display).
        .filter(|line| line.start - position < 14.0)
        .map(|line| font.mono_safe(&line.text))
        .unwrap_or_default();
    let size = size_for(settings.lyrics_size);
    let preview_y = LYRIC_Y - size * 1.15 - 12.0;
    if let Ok((mut text, mut transform, mut visibility)) = preview.single_mut() {
        if text.0 != wanted_preview {
            text.0.clone_from(&wanted_preview);
        }
        transform.translation.y = preview_y;
        *visibility = if wanted_preview.is_empty() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }

    // The scrim sizes to whatever is on show.
    let active_line = cue.active.and_then(|index| lyrics.lines.get(index));
    let active_chars = active_line.map_or(0, |line| font.mono_safe(&line.text).chars().count());
    let preview_chars = wanted_preview.chars().count();
    let em = font.glyph_em();
    let content_w = (active_chars as f32 * glyph_advance(size * em, active_chars))
        .max(preview_chars as f32 * size * PREVIEW_SCALE * em);
    if let Ok((mut sprite, mut visibility)) = scrim.single_mut() {
        if content_w > 0.0 {
            let height = if preview_chars > 0 {
                size * 2.6
            } else {
                size * 1.7
            };
            sprite.custom_size = Some(Vec2::new(content_w + 48.0, height));
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    // The band behind the passage being sung right now. In the
    // lead-in it grows from nothing to the line's width, so the eye
    // knows the line is coming; once the line is sung it goes.
    if let Ok((mut sprite, mut transform, mut visibility)) = highlight.single_mut() {
        let advance = glyph_advance(size * em, active_chars);
        let box_size = highlight_box(active_chars, advance, size);
        let grow = match (cue.phase, active_line) {
            (Phase::LeadIn, Some(line)) => {
                lead_in_progress(line.start, lead_in, position).max(0.05)
            }
            (Phase::Singing, _) => 1.0,
            _ => 0.0,
        };
        if box_size == Vec2::ZERO || !settings.lyrics || grow <= 0.0 {
            *visibility = Visibility::Hidden;
        } else {
            sprite.custom_size = Some(Vec2::new(box_size.x * grow, box_size.y));
            transform.translation.y = LYRIC_Y + size * 0.55;
            *visibility = Visibility::Inherited;
        }
    }

    // The gap countdown: on the beat grid, off the same clock.
    let beat_s = 60.0 / song.chart.song.bpm.max(1.0);
    let countdown = match (cue.phase, cue.active, cue.upcoming) {
        // Idle: the countdown belongs to the next line.
        (Phase::Idle, None, Some(next)) => Some(next),
        // Lead-in: the countdown (if any) keeps running into the
        // line's own lead-in — the beats do not stop for it.
        (Phase::LeadIn, Some(next), _) => Some(next),
        _ => None,
    }
    .and_then(|next| {
        let line = &lyrics.lines[next];
        let gap_from = next
            .checked_sub(1)
            .map_or(0.0, |previous| sung_end(&lyrics.lines[previous]));
        countdown_lit(position, line.start, gap_from, beat_s)
    });
    for (pulse, mut sprite, mut transform, mut visibility) in &mut pulses {
        match countdown {
            None => *visibility = Visibility::Hidden,
            Some(lit) => {
                sprite.color = if pulse.0 < lit {
                    palette::BRAND
                } else {
                    palette::dimmed(palette::TEXT, 0.3)
                };
                // Above the row, clear of the band, whatever the size.
                transform.translation.y = pulse_y(size);
                *visibility = Visibility::Inherited;
            }
        }
    }

    // Fill the glyphs.
    let Some(line) = active_line else {
        return;
    };
    let enter = if settings.backdrop_motion {
        ((time.elapsed_secs() - display.entered_at) / ENTER_S).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let enter = enter * enter * (3.0 - 2.0 * enter); // smoothstep
    let fade = line_fade(cue.phase, line, position);
    let unsung = palette::dimmed(palette::TEXT, 0.42);
    for (_, glyph, mut color, mut transform) in &mut glyphs {
        let base = match glyph.cue.as_ref() {
            Some(cue) => {
                let fill = glyph_fill(cue, position);
                let singing = line
                    .words
                    .get(cue.word)
                    .is_some_and(|word| position < word.end);
                unsung.mix(&glyph_tone(singing), fill)
            }
            None => line_timed_tone(cue.phase, position < line.start),
        };
        color.0 = base.with_alpha(base.alpha() * fade * enter);
        // The entry ease: the line rises the last few pixels into
        // place. Motion-gated via `enter` staying 1.0.
        transform.translation.y = glyph.home.y - 8.0 * (1.0 - enter);
    }
}

/// Where the countdown pulses sit for a lyric size: above the band
/// behind the row (which reaches `LYRIC_Y + 1.3 × size`), never on
/// the words. Pure — tested.
#[must_use]
pub fn pulse_y(size: f32) -> f32 {
    LYRIC_Y + size * 1.3 + PULSE_SIZE
}

/// The whole line's alpha for its phase. In the lead-in the line
/// stands unlit at full alpha, whatever timing it has: a line-timed
/// line fades in at its start, but it is ON SCREEN before that (the
/// first version faded it to nothing and grew the band around an
/// empty row). A sung line dims. Pure — tested.
#[must_use]
pub fn line_fade(phase: Phase, line: &beatbyte_chart::lyrics::LyricLine, position: f64) -> f32 {
    match phase {
        Phase::LeadIn => 1.0,
        _ if line.words.is_empty() => line_alpha(line.start, line.end, position),
        Phase::Sung => SUNG_ALPHA,
        _ => 1.0,
    }
}

/// A line-timed glyph's colour: unlit through the lead-in (and any
/// moment before the line's start), the text colour once the line
/// has begun. Pure — tested.
#[must_use]
pub fn line_timed_tone(phase: Phase, before_start: bool) -> Color {
    if phase == Phase::LeadIn || before_start {
        palette::dimmed(palette::TEXT, 0.42)
    } else {
        palette::TEXT
    }
}

/// The lit colour of a glyph: the word being sung RIGHT NOW fills to
/// white, a word already sung settles to the brand amber — a colour
/// step on the current word, so it reads at speed. Pure — tested for
/// contrast against the band.
#[must_use]
pub fn glyph_tone(singing: bool) -> Color {
    if singing {
        palette::TEXT
    } else {
        palette::BRAND
    }
}

/// How far into a lead-in the position is: 0 as the line appears,
/// 1 at its first word. Pure — tested.
#[must_use]
pub fn lead_in_progress(line_start: f64, lead_in: f64, position: f64) -> f32 {
    if lead_in <= 0.0 {
        return 1.0;
    }
    (((position - (line_start - lead_in)) / lead_in) as f32).clamp(0.0, 1.0)
}

/// The band behind the current passage: its width and its centre
/// height, from the line's own length. Pure — tested.
///
/// It hugs the words rather than filling the panel: a full-width bar
/// over a five-lane highway would read as a second HUD element, and
/// the point is to mark THIS passage, not to draw a box.
#[must_use]
pub fn highlight_box(chars: usize, advance: f32, size: f32) -> Vec2 {
    if chars == 0 {
        return Vec2::ZERO;
    }
    Vec2::new(chars as f32 * advance + size * 0.9, size * 1.5)
}

/// The per-glyph advance for a line — the face's natural step,
/// shrunk when a long line would overflow the screen. Pure — tested.
#[must_use]
pub fn glyph_advance(step: f32, chars: usize) -> f32 {
    if chars == 0 {
        return step;
    }
    step.min(MAX_LINE_W / chars as f32)
}

fn spawn_line_glyphs(
    commands: &mut Commands,
    font: &UiFont,
    settings: &Settings,
    line: &beatbyte_chart::lyrics::LyricLine,
) {
    let text = font.mono_safe(&line.text);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return;
    }
    let cues = (!line.words.is_empty()).then(|| glyph_cues(&text, &line.words));
    let size = size_for(settings.lyrics_size);
    // The step between glyph centers follows the FACE's own advance
    // (1 em pixel font, 0.6 em smooth font) so the row reads as
    // ordinary text, not letter-spaced type.
    let em = font.glyph_em();
    let advance = glyph_advance(size * em, chars.len());
    let width = advance * chars.len() as f32;
    for (index, glyph) in chars.iter().enumerate() {
        if *glyph == ' ' {
            continue; // nothing to draw, the advance is the space
        }
        let home = Vec3::new(
            -width / 2.0 + (index as f32 + 0.5) * advance,
            LYRIC_Y + size * 0.55,
            4.0,
        );
        commands.spawn((
            super::GameplayScreen,
            LyricPart,
            LyricGlyph {
                cue: cues.as_ref().map(|cues| cues[index]),
                home,
            },
            Text2d::new(glyph.to_string()),
            font.mono_text(advance / em),
            TextColor(palette::dimmed(palette::TEXT, 0.42)),
            Anchor::CENTER,
            Transform::from_translation(home),
        ));
    }
}

/// The outro owns the stage: whatever line was mid-fill leaves with
/// the gameplay, instead of hanging frozen behind "YOU ROCK!!!".
pub fn clear_for_outro(
    mut commands: Commands,
    mut display: ResMut<LyricDisplay>,
    glyphs: GlyphQuery,
    mut preview: PreviewQuery,
    mut scrim: ScrimQuery,
    mut highlight: HighlightQuery,
    mut pulses: PulseQuery,
) {
    clear_glyphs(&mut commands, &glyphs);
    display.line = None;
    hide_chrome(&mut preview, &mut scrim, &mut highlight, &mut pulses);
}

fn clear_glyphs(commands: &mut Commands, glyphs: &GlyphQuery) {
    for (entity, _, _, _) in glyphs.iter() {
        commands.entity(entity).despawn();
    }
}

fn hide_chrome(
    preview: &mut PreviewQuery,
    scrim: &mut ScrimQuery,
    highlight: &mut HighlightQuery,
    pulses: &mut PulseQuery,
) {
    if let Ok((mut text, _, mut visibility)) = preview.single_mut() {
        text.0.clear();
        *visibility = Visibility::Hidden;
    }
    if let Ok((_, mut visibility)) = scrim.single_mut() {
        *visibility = Visibility::Hidden;
    }
    if let Ok((_, _, mut visibility)) = highlight.single_mut() {
        *visibility = Visibility::Hidden;
    }
    for (_, _, _, mut visibility) in pulses.iter_mut() {
        *visibility = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::lyrics::LyricWord;

    fn word(text: &str, start: f64, end: f64) -> LyricWord {
        LyricWord::new(text, start, end)
    }

    #[test]
    fn glyphs_map_to_their_words_and_spaces_wait_for_completion() {
        let words = [word("Hello", 1.0, 2.0), word("world", 2.0, 3.0)];
        let cues = glyph_cues("Hello world", &words);
        assert_eq!(cues.len(), 11);
        // 'H' is the first fifth of "Hello": 1.0..1.2.
        assert_eq!(cues[0].word, 0);
        assert!((cues[0].start - 1.0).abs() < 1e-9 && (cues[0].end - 1.2).abs() < 1e-9);
        // 'o' is the last fifth.
        assert!((cues[4].start - 1.8).abs() < 1e-9);
        // The space lights when "Hello" completes - a degenerate
        // window at the word's end.
        assert_eq!(cues[5].word, 0);
        assert!((cues[5].start - 2.0).abs() < 1e-9 && (cues[5].end - 2.0).abs() < 1e-9);
        // 'w' opens "world".
        assert_eq!(cues[6].word, 1);
        assert!((cues[6].start - 2.0).abs() < 1e-9);
    }

    #[test]
    fn aligned_character_spans_drive_the_glyphs_directly() {
        // "Hi" sung slowly on the H and fast on the i: the glyph
        // windows are the alignment's, not an even split.
        let mut hi = word("Hi", 1.0, 2.0);
        hi.chars = vec![[1.0, 1.8], [1.8, 2.0]];
        let cues = glyph_cues("Hi there", &[hi, word("there", 2.5, 3.0)]);
        assert!((cues[0].start - 1.0).abs() < 1e-9 && (cues[0].end - 1.8).abs() < 1e-9);
        assert!((cues[1].start - 1.8).abs() < 1e-9 && (cues[1].end - 2.0).abs() < 1e-9);
        // A count mismatch (a word with spans for another spelling)
        // falls back to the even split rather than misplacing spans.
        let mut odd = word("Hi", 1.0, 2.0);
        odd.chars = vec![[1.0, 2.0]];
        let cues = glyph_cues("Hi", &[odd]);
        assert!((cues[0].end - 1.5).abs() < 1e-9);
    }

    #[test]
    fn the_fill_crosses_a_glyph_linearly_and_snaps_degenerates() {
        let cue = GlyphCue {
            word: 0,
            start: 1.2,
            end: 1.4,
        };
        assert!((glyph_fill(&cue, 1.1) - 0.0).abs() < 1e-6);
        assert!((glyph_fill(&cue, 1.3) - 0.5).abs() < 1e-6);
        assert!((glyph_fill(&cue, 1.9) - 1.0).abs() < 1e-6);
        let space = GlyphCue {
            word: 0,
            start: 2.0,
            end: 2.0,
        };
        assert!((glyph_fill(&space, 1.99) - 0.0).abs() < 1e-6);
        assert!((glyph_fill(&space, 2.0) - 1.0).abs() < 1e-6);
    }

    fn two_lines() -> beatbyte_chart::lyrics::Lyrics {
        use beatbyte_chart::lyrics::{LyricLine, Lyrics};
        Lyrics {
            lines: vec![
                LyricLine {
                    start: 10.0,
                    end: 20.0,
                    text: "one two".to_owned(),
                    words: vec![word("one", 10.0, 10.5), word("two", 10.6, 11.0)],
                },
                LyricLine {
                    start: 20.0,
                    end: 24.0,
                    text: "three".to_owned(),
                    words: vec![word("three", 20.0, 21.0)],
                },
            ],
        }
    }

    #[test]
    fn a_line_has_a_real_end_and_the_next_leads_in_only_after_it() {
        let lyrics = two_lines();
        // Being sung.
        let cue = display_cue(&lyrics, 10.3, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::Singing));
        assert_eq!(cue.upcoming, Some(1));
        // Every word sung: the line is SUNG, not still in progress,
        // even though its display end (20.0) is far away.
        let cue = display_cue(&lyrics, 15.0, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::Sung));
        // 1.5 s before the next line it takes the row: lead-in.
        let cue = display_cue(&lyrics, 18.6, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(1), Phase::LeadIn));
        assert_eq!(cue.upcoming, None, "nothing after the last line");
        // Between: the sung line holds until the lead-in begins.
        let cue = display_cue(&lyrics, 18.4, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::Sung));
        // Before anything: idle, the first line upcoming.
        let cue = display_cue(&lyrics, 2.0, 1.5);
        assert_eq!((cue.active, cue.phase), (None, Phase::Idle));
        assert_eq!(cue.upcoming, Some(0));
        // ...until its lead-in.
        let cue = display_cue(&lyrics, 8.6, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::LeadIn));
    }

    #[test]
    fn a_line_still_being_sung_is_never_cut_for_the_next_ones_lead_in() {
        // Line-timed lyrics (no words) are "singing" until their
        // display end; a lead-in of 15 s would otherwise steal the
        // row from a line in progress.
        use beatbyte_chart::lyrics::{LyricLine, Lyrics};
        let lyrics = Lyrics {
            lines: vec![
                LyricLine {
                    start: 10.0,
                    end: 20.0,
                    text: "a".to_owned(),
                    words: Vec::new(),
                },
                LyricLine {
                    start: 20.0,
                    end: 24.0,
                    text: "b".to_owned(),
                    words: Vec::new(),
                },
            ],
        };
        let cue = display_cue(&lyrics, 19.0, 15.0);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::Singing));
        // With no line in progress the lead-in applies.
        let cue = display_cue(&lyrics, 9.0, 1.5);
        assert_eq!((cue.active, cue.phase), (Some(0), Phase::LeadIn));
    }

    #[test]
    fn the_countdown_runs_four_beats_into_a_long_gap_only() {
        let beat = 0.5;
        // A 10 s gap before a line at 30: four beats = 2 s.
        assert_eq!(countdown_lit(27.0, 30.0, 20.0, beat), None, "too early");
        assert_eq!(
            countdown_lit(28.0, 30.0, 20.0, beat),
            Some(0),
            "first beat pending"
        );
        assert_eq!(countdown_lit(28.6, 30.0, 20.0, beat), Some(1));
        assert_eq!(countdown_lit(29.1, 30.0, 20.0, beat), Some(2));
        assert_eq!(countdown_lit(29.6, 30.0, 20.0, beat), Some(3));
        assert_eq!(
            countdown_lit(30.0, 30.0, 20.0, beat),
            Some(4),
            "all lit at the line"
        );
        assert_eq!(countdown_lit(30.1, 30.0, 20.0, beat), None, "over");
        // A short gap gets no countdown: lines flow into each other.
        assert_eq!(countdown_lit(29.0, 30.0, 27.0, beat), None);
        // A degenerate beat never divides by zero.
        assert_eq!(countdown_lit(29.0, 30.0, 20.0, 0.0), None);
    }

    #[test]
    fn the_countdown_sits_above_the_band_at_every_size() {
        for step in 0..3u8 {
            let size = size_for(step);
            let band_top = LYRIC_Y + size * 0.55 + highlight_box(20, size, size).y / 2.0;
            assert!(
                pulse_y(size) - PULSE_SIZE / 2.0 >= band_top,
                "size {size}: pulses at {} overlap the band top {band_top}",
                pulse_y(size)
            );
        }
    }

    #[test]
    fn a_line_timed_line_stands_unlit_through_its_lead_in() {
        // Seen live: the band grew around an EMPTY row because the
        // line-timed fade was 0 before the line's start.
        let timed = beatbyte_chart::lyrics::LyricLine {
            start: 10.0,
            end: 14.0,
            text: "a".to_owned(),
            words: Vec::new(),
        };
        assert!(
            (line_fade(Phase::LeadIn, &timed, 9.0) - 1.0).abs() < 1e-6,
            "on screen"
        );
        assert!((line_fade(Phase::Singing, &timed, 12.0) - 1.0).abs() < 1e-6);
        assert!(
            line_fade(Phase::Singing, &timed, 13.9) < 1.0,
            "fades out at its end"
        );
        let worded = two_lines().lines.remove(0);
        assert!(
            (line_fade(Phase::Sung, &worded, 15.0) - SUNG_ALPHA).abs() < 1e-6,
            "dims"
        );
        assert!((line_fade(Phase::Singing, &worded, 10.3) - 1.0).abs() < 1e-6);
        assert_eq!(
            line_timed_tone(Phase::LeadIn, true),
            palette::dimmed(palette::TEXT, 0.42)
        );
        assert_eq!(line_timed_tone(Phase::Singing, false), palette::TEXT);
        assert_ne!(
            line_timed_tone(Phase::LeadIn, true),
            line_timed_tone(Phase::Singing, false)
        );
    }

    #[test]
    fn the_lead_in_band_grows_from_nothing_to_the_first_word() {
        assert!((lead_in_progress(10.0, 1.5, 8.5) - 0.0).abs() < 1e-6);
        assert!((lead_in_progress(10.0, 1.5, 9.25) - 0.5).abs() < 1e-6);
        assert!((lead_in_progress(10.0, 1.5, 10.0) - 1.0).abs() < 1e-6);
        assert!(
            (lead_in_progress(10.0, 0.0, 9.0) - 1.0).abs() < 1e-6,
            "no lead-in = full"
        );
    }

    #[test]
    fn a_line_without_words_fades_honestly() {
        // In, hold, out - and zero outside its own span. No fake
        // word sync (commission rule).
        assert!((line_alpha(10.0, 14.0, 9.9) - 0.0).abs() < 1e-6);
        assert!((line_alpha(10.0, 14.0, 10.15) - 0.5).abs() < 1e-6);
        assert!((line_alpha(10.0, 14.0, 12.0) - 1.0).abs() < 1e-6);
        assert!((line_alpha(10.0, 14.0, 13.85) - 0.5).abs() < 1e-6);
        assert!((line_alpha(10.0, 14.0, 14.1) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn long_lines_shrink_instead_of_overflowing() {
        // A 40-char line at LARGE keeps its size; a 200-char line
        // must fit the screen instead of running off it.
        assert!((glyph_advance(26.0, 40) - 26.0).abs() < 1e-6);
        let shrunk = glyph_advance(26.0, 200);
        assert!(shrunk < 26.0);
        assert!(shrunk * 200.0 <= MAX_LINE_W + 1e-3);
        assert!((glyph_advance(26.0, 0) - 26.0).abs() < 1e-6);
    }

    #[test]
    fn the_passage_band_hugs_its_words_and_vanishes_with_them() {
        // It marks THIS passage; a band that kept a size with no
        // line to mark would sit on the stage as an empty bar.
        assert_eq!(highlight_box(0, 20.0, 26.0), Vec2::ZERO);
        let band = highlight_box(20, 20.0, 26.0);
        assert!(band.x > 20.0 * 20.0, "the band clears the last glyph");
        assert!(
            band.x < 20.0 * 20.0 + 26.0 * 2.0,
            "but does not fill the stage"
        );
        assert!(band.y > 26.0, "a line of text fits inside it");
        // Longer line, wider band - it follows the words.
        assert!(highlight_box(40, 20.0, 26.0).x > band.x);
    }

    /// WCAG relative luminance of an opaque colour.
    fn luminance(color: Color) -> f32 {
        let c = color.to_linear();
        0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
    }

    /// WCAG contrast ratio between two opaque colours.
    fn contrast(a: Color, b: Color) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// One alpha composite, in the LINEAR space Bevy blends in.
    fn over(top: Color, bottom: Color) -> Color {
        let (t, b) = (top.to_linear(), bottom.to_linear());
        let a = t.alpha;
        Color::linear_rgb(
            t.red.mul_add(a, b.red * (1.0 - a)),
            t.green.mul_add(a, b.green * (1.0 - a)),
            t.blue.mul_add(a, b.blue * (1.0 - a)),
        )
    }

    /// The real stack the glyphs sit on: the scrim lies over the
    /// stage (z 3.8), the band over the scrim (z 3.9), the words on
    /// top (z 4.0).
    fn band_over(backdrop: Color) -> Color {
        let scrim = over(Color::srgba(0.0, 0.0, 0.0, 0.5), backdrop);
        over(highlight_color(), scrim)
    }

    #[test]
    fn the_words_stay_readable_on_their_own_highlight() {
        // Measured, not guessed: the first band was plain BRAND at
        // low alpha - a mid-luminance yellow - and a real frame put
        // the white glyphs on it at 3.83:1, under the 4.5:1 a line
        // of lyrics needs. The band composites over whatever the
        // stage shows, so the check runs against the BRIGHTEST
        // plausible backdrop (a lit LED wall) as well as the dark
        // one, because that is the worst case for a dark band.
        for backdrop in [
            palette::BACKGROUND,
            Color::srgb(0.55, 0.15, 0.3),
            Color::WHITE,
        ] {
            let band = band_over(backdrop);
            let ratio = contrast(band, palette::TEXT);
            assert!(
                ratio >= 4.5,
                "sung words sit at {ratio:.2}:1 on the band over {backdrop:?}"
            );
        }
    }

    #[test]
    fn the_band_is_visible_as_a_marking() {
        // A highlight nobody can see is not a highlight: it has to
        // read apart from the scrim it lies on.
        let scrim = over(Color::srgba(0.0, 0.0, 0.0, 0.5), palette::BACKGROUND);
        let ratio = contrast(band_over(palette::BACKGROUND), scrim);
        assert!(
            ratio > 1.15,
            "the band vanishes into the scrim ({ratio:.2}:1)"
        );
    }

    #[test]
    fn the_size_steps_are_clamped_and_distinct() {
        assert!(size_for(0) < size_for(1) && size_for(1) < size_for(2));
        assert!((size_for(9) - size_for(2)).abs() < 1e-6, "clamped");
    }

    #[test]
    fn unstamped_lead_ins_attach_to_the_first_word() {
        // "Oh " has no stamp; it must not break the mapping of the
        // stamped words after it.
        let words = [word("yeah", 5.0, 6.0)];
        let cues = glyph_cues("Oh yeah", &words);
        assert_eq!(cues[3].word, 0);
        assert!((cues[3].start - 5.0).abs() < 1e-9, "'y' opens the word");
        // The lead-in glyphs sit before word 0 with a degenerate
        // window at its start - lit the moment the word starts.
        assert!((cues[0].start - 5.0).abs() < 1e-9 && (cues[0].end - 5.0).abs() < 1e-9);
    }

    #[test]
    fn both_lit_tones_stay_readable_on_the_band() {
        // The current word fills to white, a sung word settles to
        // amber: both sit on the band, both need 4.5:1.
        for backdrop in [palette::BACKGROUND, Color::WHITE] {
            let band = band_over(backdrop);
            for singing in [true, false] {
                let ratio = contrast(band, glyph_tone(singing));
                assert!(
                    ratio >= 4.5,
                    "tone (singing={singing}) at {ratio:.2}:1 on the band"
                );
            }
        }
        assert_ne!(glyph_tone(true), glyph_tone(false), "the step is a step");
    }
}
