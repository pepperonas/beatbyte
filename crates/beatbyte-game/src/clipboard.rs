//! The system clipboard, for the one field that needs it.
//!
//! Bevy has no clipboard, and the ADD A SONG field takes a YouTube
//! link — eleven characters of case-sensitive nonsense inside a URL
//! nobody should be asked to retype. This is the whole of the
//! dependency's use.
//!
//! ⚠️ Reading it can fail on every platform (no clipboard server, a
//! Wayland session without one, an image on the board rather than
//! text), and none of that is worth an error dialog: the caller says
//! "nothing to paste" and the player types instead.

/// What the clipboard holds, cleaned the way typed text is.
///
/// Control characters are stripped — a copied line carries a newline
/// and a one-line field must not grow one — and the result is
/// trimmed and capped, because a clipboard can hold a novel.
#[must_use]
pub fn read() -> Option<String> {
    let raw = arboard::Clipboard::new().ok()?.get_text().ok()?;
    Some(clean(&raw))
}

/// The longest paste a one-line field accepts.
///
/// A URL with a playlist and a timestamp on it runs past 100; a
/// clipboard holding a document must not become a "song name".
const MAX: usize = 300;

/// Strip, trim and cap a pasted string. Pure — tested.
#[must_use]
pub fn clean(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_arrives_as_one_line() {
        assert_eq!(
            clean("  https://youtu.be/dQw4w9WgXcQ\n"),
            "https://youtu.be/dQw4w9WgXcQ"
        );
        assert_eq!(
            clean("two\nlines"),
            "twolines",
            "a newline would split the field"
        );
        assert_eq!(clean("\t\u{7f}x"), "x");
        assert_eq!(clean(""), "");
        assert_eq!(clean("   "), "");
    }

    #[test]
    fn a_clipboard_holding_a_novel_does_not_become_a_song_name() {
        let long = "a".repeat(10_000);
        assert_eq!(clean(&long).len(), MAX);
    }
}
