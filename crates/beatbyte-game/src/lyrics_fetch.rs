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
// The length rule and the query cleaning moved to the chart crate
// when a third caller appeared (the metadata catalogue) and the CLI
// could not reach them here. Re-exported so this module still reads
// as the place they are used.
pub use beatbyte_chart::catalogue::{
    DURATION_TOLERANCE_S, DURATION_TOLERANCE_SHARE, clean_query, duration_fits,
};

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

/// How long the catalogue thinks this song is.
///
/// The one fact the search needs BEFORE it fetches anything: with it,
/// a recording of the wrong length is rejected on its metadata and
/// never downloaded. `None` when the catalogue has no entry, which
/// simply means length cannot decide.
#[must_use]
pub fn catalogue_duration(artist: &str, title: &str) -> Option<f64> {
    if artist.trim().is_empty() || title.trim().is_empty() {
        return None;
    }
    let response = ureq::get("https://lrclib.net/api/get")
        .query("artist_name", artist.trim())
        .query("track_name", title.trim())
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .call()
        .ok()?;
    let body = response.into_string().ok()?;
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()?
        .get("duration")
        .and_then(serde_json::Value::as_f64)
}

/// Ask lrclib for a track's lyrics.
///
/// `duration_s` is the song's own length. It is sent along, so the
/// catalogue answers about THIS recording rather than about whatever
/// else carries the title.
///
/// Blocking — call it off the frame thread (the browser runs it on
/// the async compute pool, like an import).
#[must_use]
pub fn fetch(artist: &str, title: &str, duration_s: Option<f64>) -> Outcome {
    let first = get(artist, title, duration_s);
    if matches!(first, Outcome::Synced(_) | Outcome::Failed(_)) {
        return first;
    }
    // The names a download left behind: ask again without the
    // uploader's furniture, but only if cleaning changed anything.
    let (clean_artist, clean_title) = clean_query(artist, title);
    if (clean_artist.as_str(), clean_title.as_str()) != (artist.trim(), title.trim())
        && !clean_artist.is_empty()
        && !clean_title.is_empty()
    {
        let second = get(&clean_artist, &clean_title, duration_s);
        if matches!(second, Outcome::Synced(_)) {
            return second;
        }
    }
    // Still nothing timed. `get` matches a length within two seconds
    // of its own, which turns away a rip of the same recording; the
    // search returns every entry and lets us judge the length by our
    // own rule.
    match search(&clean_artist, &clean_title, duration_s) {
        Outcome::NotFound => first,
        other => other,
    }
}

/// One `get` call. The 404 reading is the source app's, kept: an
/// empty catalogue entry is not a broken lookup.
fn get(artist: &str, title: &str, duration_s: Option<f64>) -> Outcome {
    let mut request = ureq::get("https://lrclib.net/api/get")
        .query("artist_name", artist.trim())
        .query("track_name", title.trim());
    if let Some(seconds) = duration_s.filter(|s| s.is_finite() && *s > 0.0) {
        request = request.query("duration", &format!("{seconds:.0}"));
    }
    match request
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .call()
    {
        Ok(raw) => match raw.into_string() {
            Ok(body) => classify(&body),
            Err(error) => Outcome::Failed(format!("cannot read the reply: {error}")),
        },
        Err(ureq::Error::Status(404, _)) => Outcome::NotFound,
        Err(error) => Outcome::Failed(format!("{error}")),
    }
}

/// The search fallback: every entry under these names, judged by our
/// own length rule rather than the catalogue's two seconds.
fn search(artist: &str, title: &str, duration_s: Option<f64>) -> Outcome {
    if artist.trim().is_empty() || title.trim().is_empty() {
        return Outcome::NotFound;
    }
    let response = ureq::get("https://lrclib.net/api/search")
        .query("artist_name", artist.trim())
        .query("track_name", title.trim())
        .timeout(std::time::Duration::from_secs(TIMEOUT_S))
        .call();
    match response {
        Ok(raw) => match raw.into_string() {
            Ok(body) => pick_from_search(&body, duration_s),
            Err(error) => Outcome::Failed(format!("cannot read the reply: {error}")),
        },
        Err(ureq::Error::Status(404, _)) => Outcome::NotFound,
        Err(error) => Outcome::Failed(format!("{error}")),
    }
}

/// Choose from a search result: the timed entry whose length is
/// closest to ours, among those our own rule accepts.
///
/// Pure — tested against synthetic bodies. Real lyrics are
/// copyrighted and never enter this repository, so the fixtures
/// carry the shape and not the words.
#[must_use]
pub fn pick_from_search(body: &str, duration_s: Option<f64>) -> Outcome {
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(body) else {
        return Outcome::NotFound;
    };
    let mut best: Option<(f64, &serde_json::Value)> = None;
    let mut saw_words = false;
    for entry in &entries {
        let timed = entry
            .get("syncedLyrics")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        if entry
            .get("plainLyrics")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
        {
            saw_words = true;
        }
        if !timed {
            continue;
        }
        let theirs = entry.get("duration").and_then(serde_json::Value::as_f64);
        // Without a length of our own there is nothing to compare, so
        // the first timed entry wins; with one, only entries our rule
        // accepts are candidates and the closest of them is chosen.
        let distance = match (duration_s, theirs) {
            (Some(ours), Some(theirs)) => {
                if !duration_fits(ours, theirs) {
                    continue;
                }
                (ours - theirs).abs()
            }
            (Some(_), None) => continue,
            (None, _) => 0.0,
        };
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, entry));
        }
        if duration_s.is_none() {
            break;
        }
    }
    match best {
        Some((_, entry)) => {
            let text = entry
                .get("syncedLyrics")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let lyrics = beatbyte_chart::lyrics::parse_lrc(text);
            if lyrics.lines.is_empty() {
                Outcome::NotFound
            } else {
                Outcome::Synced(lyrics)
            }
        }
        None if saw_words => Outcome::PlainOnly,
        None => Outcome::NotFound,
    }
}

/// Turn a `get` response body into an outcome. Pure — tested against
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
pub fn fetch_and_cache(
    artist: &str,
    title: &str,
    duration_s: Option<f64>,
    audio_path: &Path,
) -> Outcome {
    let outcome = fetch(artist, title, duration_s);
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
    fn the_search_picks_the_closest_entry_our_rule_accepts() {
        // Synthetic bodies: the shape of a search result, never real
        // lyrics.
        let body = r#"[
          {"duration": 500.0, "syncedLyrics": "[00:01.00]far", "plainLyrics": "far"},
          {"duration": 262.0, "syncedLyrics": "[00:02.00]near", "plainLyrics": "near"},
          {"duration": 258.0, "syncedLyrics": "[00:03.00]nearest", "plainLyrics": "nearest"},
          {"duration": 259.0, "syncedLyrics": "", "plainLyrics": "untimed"}
        ]"#;
        let Outcome::Synced(picked) = pick_from_search(body, Some(255.0)) else {
            panic!("a fitting timed entry exists");
        };
        assert_eq!(picked.lines[0].text, "nearest", "the closest length wins");

        // Every entry too far away: the words exist, the timing for
        // THIS recording does not.
        assert_eq!(
            pick_from_search(body, Some(120.0)),
            Outcome::PlainOnly,
            "an entry we may not use is not 'nothing found'"
        );
        // Nothing at all.
        assert_eq!(pick_from_search("[]", Some(255.0)), Outcome::NotFound);
        assert_eq!(pick_from_search("not json", Some(255.0)), Outcome::NotFound);
        // Without a length of our own the first timed entry is taken:
        // there is nothing to judge lengths against.
        let Outcome::Synced(any) = pick_from_search(body, None) else {
            panic!("a timed entry exists");
        };
        assert_eq!(any.lines[0].text, "far");
        // An entry without a length cannot be judged, so it is not a
        // candidate when we do have one.
        let no_len = r#"[{"syncedLyrics": "[00:01.00]x", "plainLyrics": "x"}]"#;
        assert_eq!(pick_from_search(no_len, Some(200.0)), Outcome::PlainOnly);
    }

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
