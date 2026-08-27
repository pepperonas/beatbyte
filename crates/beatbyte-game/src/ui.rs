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

impl UiFont {
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
    use super::font_safe;

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
    fn plain_text_passes_through_unchanged() {
        assert_eq!(
            font_safe("Never Gonna Give You Up"),
            "Never Gonna Give You Up"
        );
        assert_eq!(font_safe(""), "");
    }
}
