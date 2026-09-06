//! `cargo run --release -p beatbyte-game --example fetch_missing_lyrics`
//!
//! Ask the catalogue once for every song in the library that has no
//! lyrics beside it, through the same three-step lookup the browser's
//! `L` key uses — exact names, then the names without a download's
//! furniture, then a search judged by our own length rule.
//!
//! It writes the same `.lrc` beside the audio that the game writes,
//! so a song it finds is indistinguishable from one the player looked
//! up by hand. Nothing else is touched: aligning the result is a
//! separate step (`beatbyte-cli align`).
//!
//! Pass `--dry-run` to see what it would ask and find without writing
//! anything.

use std::path::Path;

use beatbyte_game::library::{SongSource, scan_library};
use beatbyte_game::lyrics_fetch::{Outcome, cache_path, clean_query, fetch, fetch_and_cache};

fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");
    let library = scan_library(&[]);
    let mut asked = 0usize;
    let mut found = 0usize;
    let mut plain = 0usize;
    let mut missing = 0usize;
    let mut failed = 0usize;

    for entry in &library.entries {
        let SongSource::File { audio_path, .. } = &entry.source else {
            continue;
        };
        if beatbyte_chart::lyrics::words_path(audio_path).is_file()
            || cache_path(audio_path).is_file()
        {
            continue;
        }
        asked += 1;
        let (clean_artist, clean_title) = clean_query(&entry.artist, &entry.title);
        let cleaned = (clean_artist.as_str(), clean_title.as_str())
            != (entry.artist.trim(), entry.title.trim());
        let outcome = if dry_run {
            fetch(&entry.artist, &entry.title, entry.duration_s)
        } else {
            fetch_and_cache(
                &entry.artist,
                &entry.title,
                entry.duration_s,
                Path::new(audio_path),
            )
        };
        let note = match &outcome {
            Outcome::Synced(lyrics) => {
                found += 1;
                format!("{} lines", lyrics.lines.len())
            }
            Outcome::PlainOnly => {
                plain += 1;
                "words without timing".to_owned()
            }
            Outcome::NotFound => {
                missing += 1;
                "nothing in the catalogue".to_owned()
            }
            Outcome::Failed(reason) => {
                failed += 1;
                format!("lookup failed: {reason}")
            }
        };
        println!(
            "{:<28} {:<38} {:>6}s {}{}",
            truncate(&entry.artist, 28),
            truncate(&entry.title, 38),
            entry.duration_s.unwrap_or(0.0).round(),
            note,
            if cleaned { "  (names cleaned)" } else { "" }
        );
        // The catalogue is a free service run by volunteers; a batch
        // walks, it does not sprint.
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    println!(
        "\n{asked} asked: {found} timed, {plain} untimed, {missing} absent, {failed} failed{}",
        if dry_run {
            " (dry run, nothing written)"
        } else {
            ""
        }
    );
}

fn truncate(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_owned();
    }
    text.chars().take(at.saturating_sub(1)).collect::<String>() + "…"
}
