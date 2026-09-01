//! Fetching a song's karaoke lyrics from lrclib.net.
//!
//! Ported from `inspector-rust`, where this exact call has been
//! finding lyrics reliably in its Shazam mode
//! (`core/rust-lib/src/shazam.rs`, `fetch_lyrics`): the same
//! endpoint, the same two query parameters, the same 10-second
//! timeout, and the same reading of a 404 as "the catalogue simply
//! has no entry" rather than a failure.
//!
//! **No account, no key, no configuration.** lrclib is anonymous;
//! only the artist and the title leave the machine, and nothing is
//! stored anywhere but beside the song the player asked about.
//!
//! One thing is deliberately NOT ported: the source app prefers the
//! response's `plainLyrics` and strips the timestamps out of
//! `syncedLyrics`, because it only displays the words. BeatByte
//! sings along a clock, so it keeps the timing —
//! [`beatbyte_chart::lyrics::parse_lrclib_response`] carries that
//! inversion and its reasoning.

use std::path::{Path, PathBuf};

use beatbyte_chart::lyrics::{Lyrics, has_plain_only, parse_lrclib_response};

/// How long a request may take before it is given up on. The value
/// the source app has been running with.
const TIMEOUT_S: u64 = 10;

/// What a lookup produced. Every outcome is a state the player can
/// see — a lookup never ends in a blank screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Timed lyrics, ready to sing along with.
    Synced(Lyrics),
    /// The catalogue has the words but no timing for this track.
    /// A state of its own: "we found nothing" would be a lie.
    PlainOnly,
    /// The catalogue has no entry for this artist/title.
    NotFound,
    /// The lookup itself failed (offline, timeout, unreadable body).
    Failed(String),
}

impl Outcome {
    /// A line for the browser's status row.
    #[must_use]
    pub fn message(&self, title: &str) -> String {
        match self {
            Outcome::Synced(lyrics) => {
                let words = if lyrics.has_word_timing() {
                    " (word-timed)"
                } else {
                    ""
                };
                format!(
                    "lyrics for \"{title}\": {} lines{words}",
                    lyrics.lines.len()
                )
            }
            Outcome::PlainOnly => {
                format!("\"{title}\": lyrics exist but carry no timing - nothing to sing along")
            }
            Outcome::NotFound => format!("no lyrics in the catalogue for \"{title}\""),
            Outcome::Failed(reason) => format!("lyrics lookup failed: {reason}"),
        }
    }
}

/// The `.lrc` a fetched result is cached in: beside the audio, which
/// is exactly where [`beatbyte_chart::lyrics::lyrics_beside`] looks
/// on the next start. User content — the imported songs folder is
/// gitignored, and the file is a plain text document the player can
/// edit or delete.
#[must_use]
pub fn cache_path(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("lrc")
}

/// Ask lrclib for a track's lyrics.
///
/// Blocking — call it off the frame thread (the browser runs it on
/// the async compute pool, like an import).
#[must_use]
pub fn fetch(artist: &str, title: &str) -> Outcome {
    let response = ureq::get("https://lrclib.net/api/get")
        .query("artist_name", artist.trim())
        .query("track_name", title.trim())
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .call();
    match response {
        Ok(raw) => match raw.into_string() {
            Ok(body) => classify(&body),
            Err(error) => Outcome::Failed(format!("cannot read the reply: {error}")),
        },
        // The source app's reading, kept: a 404 is an empty
        // catalogue entry, not a broken lookup.
        Err(ureq::Error::Status(404, _)) => Outcome::NotFound,
        Err(error) => Outcome::Failed(format!("{error}")),
    }
}

/// Turn a response body into an outcome. Pure — tested against
/// synthetic bodies (real lyrics are copyrighted and never enter
/// this repository).
#[must_use]
pub fn classify(body: &str) -> Outcome {
    parse_lrclib_response(body).map_or_else(
        || {
            if has_plain_only(body) {
                Outcome::PlainOnly
            } else {
                Outcome::NotFound
            }
        },
        Outcome::Synced,
    )
}

/// Fetch and, on success, cache the raw `.lrc` beside the audio so
/// the next start picks it up through the ordinary file path.
#[must_use]
pub fn fetch_and_cache(artist: &str, title: &str, audio_path: &Path) -> Outcome {
    let outcome = fetch(artist, title);
    if let Outcome::Synced(lyrics) = &outcome {
        // Written from the parsed model rather than the raw body:
        // it is the same content, minus whatever the response
        // wrapped it in, and it round-trips through the parser the
        // game already uses.
        if let Err(error) = std::fs::write(cache_path(audio_path), render_lrc(lyrics)) {
            warn(&format!("cannot cache lyrics: {error}"));
        }
    }
    outcome
}

/// Render lyrics back to enhanced LRC. Pure — tested by round-trip.
#[must_use]
pub fn render_lrc(lyrics: &Lyrics) -> String {
    let stamp = |seconds: f64| {
        let seconds = seconds.max(0.0);
        let minutes = (seconds / 60.0) as u64;
        format!("{minutes:02}:{:05.2}", seconds - minutes as f64 * 60.0)
    };
    let mut out = String::new();
    for line in &lyrics.lines {
        out.push_str(&format!("[{}]", stamp(line.start)));
        if line.words.is_empty() {
            out.push_str(&line.text);
        } else {
            for word in &line.words {
                out.push_str(&format!("<{}>{} ", stamp(word.start), word.text));
            }
        }
        out.push('\n');
    }
    out
}

/// Logging without pulling Bevy into a module that is otherwise
/// engine-free.
fn warn(message: &str) {
    bevy::log::warn!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_response_shape_becomes_a_visible_state() {
        // Synthetic bodies only.
        let synced = r#"{"plainLyrics":"x","syncedLyrics":"[00:01.00]one\n[00:02.00]two"}"#;
        assert!(matches!(classify(synced), Outcome::Synced(_)));
        assert_eq!(
            classify(r#"{"plainLyrics":"words","syncedLyrics":""}"#),
            Outcome::PlainOnly,
            "words without timing must not read as 'no lyrics'"
        );
        assert_eq!(
            classify(r#"{"plainLyrics":"","syncedLyrics":""}"#),
            Outcome::NotFound
        );
        assert_eq!(classify("garbage"), Outcome::NotFound);
    }

    #[test]
    fn every_outcome_says_something_the_player_can_read() {
        // The honesty rule: no outcome may render as an empty
        // screen, and each must be distinguishable from the others.
        let lyrics = beatbyte_chart::lyrics::parse_lrc("[00:01.00]<00:01.00>hey\n");
        let messages = [
            Outcome::Synced(lyrics).message("Song"),
            Outcome::PlainOnly.message("Song"),
            Outcome::NotFound.message("Song"),
            Outcome::Failed("offline".to_owned()).message("Song"),
        ];
        for message in &messages {
            assert!(!message.trim().is_empty());
            assert!(message.contains("Song") || message.contains("failed"));
        }
        let unique: std::collections::HashSet<&String> = messages.iter().collect();
        assert_eq!(unique.len(), messages.len(), "states must read apart");
    }

    #[test]
    fn a_fetched_file_round_trips_through_the_games_own_parser() {
        // The cache is written for the ordinary `.lrc` path to read
        // back, so what is written has to survive that parser -
        // including the word stamps, which are the whole point.
        let source = "[00:12.30]<00:12.30>Hello <00:12.75>synthetic <00:13.60>world\n[00:15.00]second line\n";
        let original = beatbyte_chart::lyrics::parse_lrc(source);
        let round_tripped = beatbyte_chart::lyrics::parse_lrc(&render_lrc(&original));
        assert_eq!(round_tripped.lines.len(), original.lines.len());
        assert_eq!(round_tripped.lines[0].text, original.lines[0].text);
        assert_eq!(
            round_tripped.lines[0].words.len(),
            original.lines[0].words.len(),
            "word timing must survive the cache"
        );
        for (before, after) in original.lines[0]
            .words
            .iter()
            .zip(&round_tripped.lines[0].words)
        {
            assert!((before.start - after.start).abs() < 0.02, "stamps drift");
        }
    }

    #[test]
    fn the_cache_lands_where_the_loader_looks() {
        // `lyrics_beside` checks `<audio>.lrc` first; writing
        // anywhere else would make the fetch invisible next start.
        let audio = std::path::Path::new("/songs/imported/track/song.mp3");
        assert_eq!(
            cache_path(audio),
            std::path::Path::new("/songs/imported/track/song.lrc")
        );
    }
}
