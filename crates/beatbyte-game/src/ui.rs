//! Shared UI building blocks: the pixel font and common text styling.
//!
//! Press Start 2P is the game's voice — chunky, unmistakably 8-bit,
//! properly licensed (OFL, bundled next to the font). It runs wide, so
//! sizes here are roughly half of what a normal font would use.

use bevy::prelude::*;

/// The UI font, loaded at startup. In the round (non-8-bit) note
/// style the whole game drops the pixel font for the engine's smooth
/// built-in face — "not 8-bit" has to include the type.
#[derive(Resource)]
pub struct UiFont {
    pixel: Handle<Font>,
    /// Mirrors `Settings::round_gems`; synced every frame.
    pub smooth: bool,
}

/// Fold a Latin letter onto the ASCII the built-in font can draw.
///
/// Only used in the smooth style. Measured: Bevy's bundled face has
/// **95 glyphs** — plain ASCII, no `å`, `ä`, `ö`, `ü`, `é`, `ß`, no
/// en-dash and no curly quotes. Press Start 2P has 656 and draws all
/// of them, which is why this is style-dependent rather than a blanket
/// rule: folding "Björk" to "Bjork" when the font can render it would
/// be damage, and leaving it when the font cannot is a box.
pub(crate) fn fold_latin(c: char) -> Option<&'static str> {
    Some(match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => "a",
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => "A",
        'é' | 'è' | 'ê' | 'ë' => "e",
        'É' | 'È' | 'Ê' | 'Ë' => "E",
        'í' | 'ì' | 'î' | 'ï' => "i",
        'Í' | 'Ì' | 'Î' | 'Ï' => "I",
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => "o",
        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ø' => "O",
        'ú' | 'ù' | 'û' | 'ü' => "u",
        'Ú' | 'Ù' | 'Û' | 'Ü' => "U",
        'ñ' => "n",
        'Ñ' => "N",
        'ç' => "c",
        'Ç' => "C",
        'ý' | 'ÿ' => "y",
        'ß' => "ss",
        'æ' => "ae",
        'Æ' => "AE",
        'œ' => "oe",
        'Œ' => "OE",
        // Typographic punctuation, which the built-in face also lacks.
        '–' | '—' | '‐' | '‑' => "-",
        '’' | '‘' | '‚' => "'",
        '“' | '”' | '„' => "\"",
        '…' => "...",
        '·' | '•' => "-",
        '×' => "x",
        _ => return None,
    })
}

impl UiFont {
    /// Text ready for the face this style actually uses.
    ///
    /// Always maps the look-alikes neither face carries; in the smooth
    /// style it also folds Latin letters onto ASCII, because the
    /// built-in face has nothing else.
    #[must_use]
    pub fn safe(&self, text: &str) -> String {
        let mapped = font_safe(text);
        if !self.smooth {
            return mapped;
        }
        mapped
            .chars()
            .map(|c| fold_latin(c).map_or_else(|| c.to_string(), ToOwned::to_owned))
            .collect()
    }

    /// A [`TextFont`] in the active style at the given size.
    #[must_use]
    pub fn text(&self, size: f32) -> TextFont {
        TextFont {
            font: if self.smooth {
                Handle::default().into()
            } else {
                self.pixel.clone().into()
            },
            font_size: FontSize::Px(size),
            ..default()
        }
    }
}

/// Rewrite the characters the pixel font cannot draw.
///
/// Press Start 2P covers 656 glyphs — plenty of Latin, including
/// `å`, `ü` and `ß` — but nothing from the fullwidth or mathematical
/// blocks. Titles taken from downloaded file names are full of them,
/// because those forms are what a downloader substitutes for
/// characters a file system forbids: `Delilah ⧸ Billie ｜ Glastonbury`
/// rendered as two empty boxes on the song list.
///
/// Only the look-alikes are mapped, and only to the character they
/// were standing in for. Nothing else is touched: a title in a script
/// the font cannot draw is not improved by turning it into question
/// marks, and the chart keeps its true title either way — this is a
/// display concern, not a data one.
#[must_use]
pub fn font_safe(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // Fullwidth ASCII maps onto ASCII by a fixed offset. This
            // is the block downloaders use for : | ? * and friends.
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            // The slashes a file name cannot contain, substituted.
            '\u{29f8}' | '\u{2044}' | '\u{2215}' => '/',
            '\u{ff5f}' => '(',
            '\u{ff60}' => ')',
            other => other,
        })
        .collect()
}

/// Loads the font before any screen spawns text.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // Insert at build time, not from a startup system: the initial
        // state's OnEnter may run before startup-command flushes, and
        // every screen's spawn system reads this resource.
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load("fonts/PressStart2P-Regular.ttf");
        app.insert_resource(UiFont {
            pixel: handle,
            smooth: false,
        })
        .add_systems(Update, sync_font_style);
    }
}

/// Keep the font choice in step with the note-style setting. Screens
/// rebuild on state changes, so newly spawned text picks it up; text
/// already on screen keeps its face until its screen rebuilds.
fn sync_font_style(settings: Res<crate::config::Settings>, mut font: ResMut<UiFont>) {
    if font.smooth != settings.round_gems {
        font.smooth = settings.round_gems;
    }
}

#[cfg(test)]
mod tests {
    use super::{UiFont, font_safe};
    use bevy::prelude::*;

    #[test]
    fn fullwidth_forms_become_the_characters_they_stand_for() {
        // What a downloader writes when a file name may not contain
        // `|` or `:` — and what the pixel font draws as an empty box.
        assert_eq!(font_safe("Billie ｜ Glastonbury"), "Billie | Glastonbury");
        assert_eq!(font_safe("Ｒｏｃｋ"), "Rock");
        assert_eq!(font_safe("What？"), "What?");
    }

    #[test]
    fn substituted_slashes_become_slashes() {
        assert_eq!(font_safe("Delilah ⧸ Billie"), "Delilah / Billie");
        assert_eq!(font_safe("a ∕ b"), "a / b");
    }

    #[test]
    fn letters_the_font_can_draw_are_left_alone() {
        // Press Start 2P has these; transliterating them would make
        // the titles WORSE, not safer.
        for text in ["Skatebård", "Björk", "Motörhead", "Beyoncé", "Straße"] {
            assert_eq!(font_safe(text), text, "{text} should be untouched");
        }
    }

    #[test]
    fn scripts_the_font_cannot_draw_are_still_left_alone() {
        // A box is bad; a row of question marks is no better, and it
        // destroys information. Only look-alikes get mapped.
        assert_eq!(font_safe("初音ミク"), "初音ミク");
    }

    #[test]
    fn the_smooth_style_folds_what_its_face_cannot_draw() {
        // Bevy's built-in face carries 95 glyphs — measured — so in
        // the smooth style a diacritic is a box, and a box is worse
        // than a plain letter.
        let smooth = UiFont {
            pixel: Handle::default(),
            smooth: true,
        };
        assert_eq!(smooth.safe("Skatebård"), "Skatebard");
        assert_eq!(smooth.safe("Straße"), "Strasse");
        assert_eq!(smooth.safe("Beyoncé"), "Beyonce");
        assert_eq!(smooth.safe("Motörhead"), "Motorhead");
    }

    #[test]
    fn the_pixel_style_keeps_letters_it_can_draw() {
        // Press Start 2P has 656 glyphs and renders all of these.
        // Folding them here would be damage, not safety.
        let pixel = UiFont {
            pixel: Handle::default(),
            smooth: false,
        };
        assert_eq!(pixel.safe("Skatebård"), "Skatebård");
        assert_eq!(pixel.safe("Straße"), "Straße");
    }

    #[test]
    fn both_styles_map_the_look_alikes() {
        // Neither face has the fullwidth block.
        for smooth in [true, false] {
            let font = UiFont {
                pixel: Handle::default(),
                smooth,
            };
            assert_eq!(font.safe("Billie ｜ Glastonbury"), "Billie | Glastonbury");
            assert_eq!(font.safe("Delilah ⧸ Billie"), "Delilah / Billie");
        }
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        assert_eq!(
            font_safe("Never Gonna Give You Up"),
            "Never Gonna Give You Up"
        );
        assert_eq!(font_safe(""), "");
    }
}
