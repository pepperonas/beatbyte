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
//! Honesty rule (commission §30): a line without word stamps fades
//! in, holds, and fades out. It never pretends to know word timing.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use beatbyte_chart::lyrics::{cue_at, word_progress};

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

/// One glyph's window within its word: the fill crosses the glyph
/// while `word_progress` runs from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphCue {
    /// The word this glyph belongs to.
    pub word: usize,
    /// Word progress at which this glyph starts filling.
    pub from: f32,
    /// Word progress at which it is fully lit.
    pub to: f32,
}

/// Map every character of a line to its word window. Characters
/// outside any stamped word (spaces, unstamped lead-ins) light up
/// when their preceding word completes. Pure — tested.
#[must_use]
pub fn glyph_cues(text: &str, words: &[beatbyte_chart::lyrics::LyricWord]) -> Vec<GlyphCue> {
    let chars: Vec<char> = text.chars().collect();
    let mut cues = vec![
        GlyphCue {
            word: 0,
            from: 0.0,
            to: 0.0,
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
        // for the previous word to finish. A lead-in BEFORE the
        // first word keeps its init value instead — lit from the
        // line's start, not after a word it precedes.
        if index > 0 {
            for cue in cues.iter_mut().take(found).skip(cursor) {
                *cue = GlyphCue {
                    word: index - 1,
                    from: 1.0,
                    to: 1.0,
                };
            }
        }
        let len = needle.len() as f32;
        for offset in 0..needle.len() {
            cues[found + offset] = GlyphCue {
                word: index,
                from: offset as f32 / len,
                to: (offset + 1) as f32 / len,
            };
        }
        cursor = found + needle.len();
    }
    // A tail after the last word follows that word's completion.
    for cue in cues.iter_mut().skip(cursor) {
        *cue = GlyphCue {
            word: words.len().saturating_sub(1),
            from: 1.0,
            to: 1.0,
        };
    }
    cues
}

/// First occurrence of `needle` in `haystack` at or after `start`.
fn find_chars(haystack: &[char], needle: &[char], start: usize) -> Option<usize> {
    (start..=haystack.len().saturating_sub(needle.len()))
        .find(|&at| haystack[at..at + needle.len()] == *needle)
}

/// How lit one glyph is, given its word's progress: 0 dark, 1 sung,
/// linear across the glyph's own window. A degenerate window (the
/// waiting spaces) snaps when crossed. Pure — tested.
#[must_use]
pub fn glyph_fill(cue: &GlyphCue, progress: f32) -> f32 {
    let span = cue.to - cue.from;
    if span <= f32::EPSILON {
        return if progress >= cue.to { 1.0 } else { 0.0 };
    }
    ((progress - cue.from) / span).clamp(0.0, 1.0)
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
    /// This glyph's word window (None = line-timed lyric).
    cue: Option<GlyphCue>,
    /// The glyph's resting position.
    home: Vec3,
}

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
}

/// The per-frame drive: cue from the shared clock, rebuild on line
/// changes, fill glyphs by word progress.
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
) {
    let lyrics = song.lyrics.as_ref().filter(|_| settings.lyrics);
    // A song swap (MC set) or a disabled setting clears the board.
    if song.is_changed() || lyrics.is_none() {
        if display.line.is_some() || lyrics.is_none() {
            clear_glyphs(&mut commands, &glyphs);
            display.line = None;
            hide_chrome(&mut preview, &mut scrim, &mut highlight);
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
    let position = now + f64::from(settings.lyrics_offset_ms) / 1000.0;
    let cue = cue_at(lyrics, position);

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

    // The band behind the passage being sung right now.
    if let Ok((mut sprite, mut transform, mut visibility)) = highlight.single_mut() {
        let advance = glyph_advance(size * em, active_chars);
        let box_size = highlight_box(active_chars, advance, size);
        if box_size == Vec2::ZERO || !settings.lyrics {
            *visibility = Visibility::Hidden;
        } else {
            sprite.custom_size = Some(box_size);
            transform.translation.y = LYRIC_Y + size * 0.55;
            *visibility = Visibility::Inherited;
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
    let fade = if line.words.is_empty() {
        line_alpha(line.start, line.end, position)
    } else {
        1.0
    };
    let unsung = palette::dimmed(palette::TEXT, 0.42);
    for (_, glyph, mut color, mut transform) in &mut glyphs {
        let fill = glyph.cue.as_ref().map_or(1.0, |cue| {
            let progress = line
                .words
                .get(cue.word)
                .map_or(0.0, |word| word_progress(word, position));
            glyph_fill(cue, progress)
        });
        let base = if glyph.cue.is_some() {
            unsung.mix(&palette::BRAND, fill)
        } else {
            palette::TEXT
        };
        color.0 = base.with_alpha(base.alpha() * fade * enter);
        // The entry ease: the line rises the last few pixels into
        // place. Motion-gated via `enter` staying 1.0.
        transform.translation.y = glyph.home.y - 8.0 * (1.0 - enter);
    }
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
) {
    clear_glyphs(&mut commands, &glyphs);
    display.line = None;
    hide_chrome(&mut preview, &mut scrim, &mut highlight);
}

fn clear_glyphs(commands: &mut Commands, glyphs: &GlyphQuery) {
    for (entity, _, _, _) in glyphs.iter() {
        commands.entity(entity).despawn();
    }
}

fn hide_chrome(preview: &mut PreviewQuery, scrim: &mut ScrimQuery, highlight: &mut HighlightQuery) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_chart::lyrics::LyricWord;

    fn word(text: &str, start: f64, end: f64) -> LyricWord {
        LyricWord {
            text: text.to_owned(),
            start,
            end,
        }
    }

    #[test]
    fn glyphs_map_to_their_words_and_spaces_wait_for_completion() {
        let words = [word("Hello", 1.0, 2.0), word("world", 2.0, 3.0)];
        let cues = glyph_cues("Hello world", &words);
        assert_eq!(cues.len(), 11);
        // 'H' is the first fifth of "Hello".
        assert_eq!(cues[0].word, 0);
        assert!((cues[0].from - 0.0).abs() < 1e-6 && (cues[0].to - 0.2).abs() < 1e-6);
        // 'o' is the last fifth.
        assert!((cues[4].from - 0.8).abs() < 1e-6);
        // The space lights when "Hello" completes - a degenerate
        // window at progress 1.
        assert_eq!(cues[5].word, 0);
        assert!((cues[5].from - 1.0).abs() < 1e-6);
        // 'w' opens "world".
        assert_eq!(cues[6].word, 1);
        assert!((cues[6].from - 0.0).abs() < 1e-6);
    }

    #[test]
    fn the_fill_crosses_a_glyph_linearly_and_snaps_degenerates() {
        let cue = GlyphCue {
            word: 0,
            from: 0.2,
            to: 0.4,
        };
        assert!((glyph_fill(&cue, 0.1) - 0.0).abs() < 1e-6);
        assert!((glyph_fill(&cue, 0.3) - 0.5).abs() < 1e-6);
        assert!((glyph_fill(&cue, 0.9) - 1.0).abs() < 1e-6);
        let space = GlyphCue {
            word: 0,
            from: 1.0,
            to: 1.0,
        };
        assert!((glyph_fill(&space, 0.99) - 0.0).abs() < 1e-6);
        assert!((glyph_fill(&space, 1.0) - 1.0).abs() < 1e-6);
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
        assert!((cues[3].from - 0.0).abs() < 1e-6, "'y' opens the word");
        // The lead-in glyphs light as the word completes... they sit
        // before word 0, whose index they carry with a degenerate
        // window at 0 - lit the moment the word starts.
        assert!((cues[0].to - 0.0).abs() < 1e-6);
    }
}
