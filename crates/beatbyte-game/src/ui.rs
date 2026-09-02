//! Shared UI building blocks: the pixel font and common text styling.
//!
//! Press Start 2P is the game's voice — chunky, unmistakably 8-bit,
//! properly licensed (OFL, bundled next to the font). It runs wide, so
//! sizes here are roughly half of what a normal font would use.

use bevy::prelude::*;

/// How much larger the display face is set than the pixel face for
/// the same nominal size — see [`UiFont::text`].
pub const DISPLAY_SCALE: f32 = 1.3;

/// The UI font, loaded at startup. In the round (non-8-bit) note
/// style the whole game drops the pixel font for a **display face**
/// of its own — "not 8-bit" has to include the type, and until
/// v0.13.30 the round style had no voice of its own: it borrowed the
/// engine's monospace fallback.
///
/// The display face is Bebas Neue (OFL, bundled): bold, condensed,
/// all-caps — the register a stage HUD speaks in — and chosen for a
/// measured property, **tabular digits**: all ten at the same
/// advance, so the score counter never jitters. Oswald (ten digit
/// widths) and Anton (a narrow 1) failed that test.
///
/// The engine's monospace face stays for two jobs where a fixed
/// advance is the point: the karaoke line, laid out glyph by glyph,
/// and data text — a folder path, a typed search — where all-caps
/// would misrepresent what is there.
#[derive(Resource)]
pub struct UiFont {
    pixel: Handle<Font>,
    display: Handle<Font>,
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
    /// Text ready for the display face of this style.
    ///
    /// Maps the look-alikes neither face carries. Both display faces
    /// draw the Latin repertoire (Press Start 2P: 656 glyphs; Bebas
    /// Neue: probed for every letter the old fold handled), so
    /// nothing is folded here any more — "Motörhead" stays
    /// "Motörhead". The fold lives on in [`Self::mono_safe`], for the
    /// one face that still needs it.
    #[must_use]
    pub fn safe(&self, text: &str) -> String {
        font_safe(text)
    }

    /// Text ready for the engine's monospace face, which carries 95
    /// glyphs — plain ASCII — so Latin letters with diacritics are
    /// folded onto what it can draw: a plain letter beats a box.
    #[must_use]
    pub fn mono_safe(&self, text: &str) -> String {
        font_safe(text)
            .chars()
            .map(|c| fold_latin(c).map_or_else(|| c.to_string(), ToOwned::to_owned))
            .collect()
    }

    /// A [`TextFont`] in the engine's monospace face, at the given
    /// size — for the karaoke line and for data text. In the pixel
    /// style this is the pixel face: that style is monospace already
    /// and has only one voice.
    #[must_use]
    pub fn mono_text(&self, size: f32) -> TextFont {
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

    /// Horizontal advance of one glyph of the MONOSPACE face
    /// ([`Self::mono_text`]), as a fraction of the font size. Press
    /// Start 2P moves a full em (measured from the bundled TTF), the
    /// engine's face 0.6 em — verified against a live frame, after
    /// correcting for the UI-scale zoom that first made the
    /// measurement read 0.7. The display face is proportional and
    /// has no single advance; nothing lays it out glyph by glyph.
    #[must_use]
    pub fn glyph_em(&self) -> f32 {
        if self.smooth { 0.6 } else { 1.0 }
    }

    /// A [`TextFont`] in the active style's display face at the given
    /// size.
    ///
    /// The type scale in `ui_kit` was drawn for Press Start 2P, whose
    /// capitals fill the whole em. Bebas Neue's reach 70 % of it and
    /// the face is condensed besides, so at the same pixel size every
    /// screen read small (seen on the first capture: row labels the
    /// height of their own margins). The display face is set at
    /// [`DISPLAY_SCALE`] times the requested size, which puts its
    /// capitals at ~91 % of the em — the same visual weight the scale
    /// was designed around.
    #[must_use]
    pub fn text(&self, size: f32) -> TextFont {
        TextFont {
            font: if self.smooth {
                self.display.clone().into()
            } else {
                self.pixel.clone().into()
            },
            font_size: FontSize::Px(if self.smooth {
                size * DISPLAY_SCALE
            } else {
                size
            }),
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
        let server = app.world().resource::<AssetServer>();
        let pixel = server.load("fonts/PressStart2P-Regular.ttf");
        let display = server.load("fonts/BebasNeue-Regular.ttf");
        app.insert_resource(UiFont {
            pixel,
            display,
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
    fn the_mono_face_folds_what_it_cannot_draw() {
        // Bevy's built-in face carries 95 glyphs — measured — so on
        // the karaoke line a diacritic is a box, and a box is worse
        // than a plain letter.
        let smooth = UiFont {
            pixel: Handle::default(),
            display: Handle::default(),
            smooth: true,
        };
        assert_eq!(smooth.mono_safe("Skatebård"), "Skatebard");
        assert_eq!(smooth.mono_safe("Straße"), "Strasse");
        assert_eq!(smooth.mono_safe("Beyoncé"), "Beyonce");
        assert_eq!(smooth.mono_safe("Motörhead"), "Motorhead");
    }

    #[test]
    fn both_display_faces_keep_letters_they_can_draw() {
        // Press Start 2P has 656 glyphs; Bebas Neue was probed for
        // every letter the old fold handled and draws them all.
        // Folding here would be damage, not safety — and until
        // v0.13.30 the round style DID fold, because it borrowed the
        // engine's 95-glyph face for everything.
        for smooth in [false, true] {
            let font = UiFont {
                pixel: Handle::default(),
                display: Handle::default(),
                smooth,
            };
            assert_eq!(font.safe("Skatebård"), "Skatebård", "smooth={smooth}");
            assert_eq!(font.safe("Straße"), "Straße", "smooth={smooth}");
            assert_eq!(font.safe("Motörhead"), "Motörhead", "smooth={smooth}");
        }
    }

    #[test]
    fn both_styles_map_the_look_alikes() {
        // Neither face has the fullwidth block.
        for smooth in [true, false] {
            let font = UiFont {
                pixel: Handle::default(),
                display: Handle::default(),
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
