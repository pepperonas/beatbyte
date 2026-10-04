//! `song.ini`: the metadata a community chart folder carries.
//!
//! An INI file with one `[song]` section of `key = value` lines. Keys
//! are matched case-insensitively (the files in the wild spell them
//! every way), values are trimmed, and a value may carry rich-text
//! tags (`<color=#0000FF>MrO</color>…`) that a player's own screen
//! renders — here they are stripped, BeatByte draws its own text.
//!
//! Untrusted input: every number is parsed defensively and dropped
//! when it is not finite or not plausible; nothing here can fail.

/// What `song.ini` says, as far as a conversion needs it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SongIni {
    /// `name` — the title.
    pub name: Option<String>,
    /// `artist`.
    pub artist: Option<String>,
    /// `genre`.
    pub genre: Option<String>,
    /// `charter` (or `frets`, the older spelling), tags stripped.
    pub charter: Option<String>,
    /// `delay` in milliseconds: how much later than its ticks say
    /// the chart plays.
    pub delay_ms: f64,
    /// `preview_start_time` in milliseconds.
    pub preview_start_ms: Option<f64>,
    /// `song_length` in milliseconds.
    pub song_length_ms: Option<f64>,
    /// `hopo_frequency`: the natural-HOPO threshold in ticks.
    pub hopo_frequency: Option<u32>,
    /// `eighthnote_hopo`: an eighth note is a HOPO distance.
    pub eighthnote_hopo: bool,
    /// `sustain_cutoff_threshold`: sustains up to this many ticks
    /// are plain notes.
    pub sustain_cutoff: Option<u32>,
}

/// Parse a `song.ini`. Lines outside `[song]` are ignored; so is
/// anything malformed. Pure — tested.
#[must_use]
pub fn parse_ini(text: &str) -> SongIni {
    let mut ini = SongIni::default();
    let mut in_song = false;
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_song = line.eq_ignore_ascii_case("[song]");
            continue;
        }
        if !in_song || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = strip_tags(value.trim());
        let text = || (!value.is_empty()).then(|| value.clone());
        let number = || value.parse::<f64>().ok().filter(|v| v.is_finite());
        let ticks = || {
            number()
                .filter(|v| (0.0..=1_000_000.0).contains(v))
                .map(|v| v.round() as u32)
        };
        match key.as_str() {
            "name" => ini.name = text(),
            "artist" => ini.artist = text(),
            "genre" => ini.genre = text(),
            "charter" => ini.charter = text(),
            "frets" if ini.charter.is_none() => ini.charter = text(),
            "delay" => {
                ini.delay_ms = number().filter(|v| v.abs() <= 60_000.0).unwrap_or(0.0);
            }
            "preview_start_time" => {
                ini.preview_start_ms = number().filter(|v| *v >= 0.0);
            }
            "song_length" => ini.song_length_ms = number().filter(|v| *v > 0.0),
            "hopo_frequency" => ini.hopo_frequency = ticks().filter(|t| *t > 0),
            "eighthnote_hopo" => {
                ini.eighthnote_hopo = matches!(value.to_ascii_lowercase().as_str(), "1" | "true");
            }
            "sustain_cutoff_threshold" => ini.sustain_cutoff = ticks(),
            _ => {}
        }
    }
    ini
}

/// `text` without `<…>` tags. A `<` that never closes is kept as
/// text. Pure — tested.
#[must_use]
pub fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_song_section_is_read_case_insensitively() {
        let ini = parse_ini(
            "\u{feff}[Song]\nName = Through the Fire\nARTIST=DragonForce\n\
             delay = 120\nhopo_frequency = 170\neighthnote_hopo = True\n\
             preview_start_time = 97530\nsong_length = 441718\n\
             [other]\nname = not this\n",
        );
        assert_eq!(ini.name.as_deref(), Some("Through the Fire"));
        assert_eq!(ini.artist.as_deref(), Some("DragonForce"));
        assert_eq!(ini.delay_ms, 120.0);
        assert_eq!(ini.hopo_frequency, Some(170));
        assert!(ini.eighthnote_hopo);
        assert_eq!(ini.preview_start_ms, Some(97_530.0));
        assert_eq!(ini.song_length_ms, Some(441_718.0));
    }

    #[test]
    fn rich_text_tags_are_stripped_from_values() {
        let ini =
            parse_ini("[song]\ncharter = <color=#0000FF>MrO</color><color=#00FFFF>rig</color>\n");
        assert_eq!(ini.charter.as_deref(), Some("MrOrig"));
        assert_eq!(strip_tags("a < b"), "a < b");
    }

    #[test]
    fn implausible_numbers_are_dropped_not_trusted() {
        let ini = parse_ini("[song]\ndelay = 99999999\nhopo_frequency = -5\nsong_length = nan\n");
        assert_eq!(ini.delay_ms, 0.0);
        assert_eq!(ini.hopo_frequency, None);
        assert_eq!(ini.song_length_ms, None);
    }

    #[test]
    fn frets_is_the_charter_only_when_charter_is_missing() {
        assert_eq!(
            parse_ini("[song]\nfrets = Old\n").charter.as_deref(),
            Some("Old")
        );
        assert_eq!(
            parse_ini("[song]\ncharter = New\nfrets = Old\n")
                .charter
                .as_deref(),
            Some("New")
        );
    }
}
