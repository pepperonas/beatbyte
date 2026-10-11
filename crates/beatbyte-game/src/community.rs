//! The first step of adding a song by name (ADR-0025): a finished chart
//! from the community catalogue behind the Bridge app.
//!
//! A chart a person made — or the commercial game's own — plays better
//! than anything BeatByte can generate, so the search asks Encore before
//! it fetches a single recording. The catalogue's answer is chosen by
//! [`beatbyte_encore::choose`]; the chosen package is unpacked into a
//! scratch folder and converted by the same
//! [`beatbyte_library::bridge::import`] that **B** and `beatbyte-cli
//! bridge` use, so it lands as a COMMUNITY or RIP version of the song.
//!
//! Every failure here is a reason to fall back, never an error of the
//! search: no network, no ffmpeg, no fitting chart, a package that does
//! not convert — the caller says why and generates a chart instead.

use std::path::{Path, PathBuf};

use beatbyte_encore::{Client, Hit, Wanted, candidates, choose};
use beatbyte_library::bridge::{self, Outcome};

/// How many fitting charts are tried before falling back.
const MAX_TRIES: usize = 3;

/// A chart that came in: the status line and its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrived {
    /// What the status row says.
    pub line: String,
    /// The version's folder.
    pub folder: PathBuf,
}

/// Ask the catalogue for this song and bring the best chart in.
///
/// `progress` hears a line for every step. `Err` carries the reason the
/// caller falls back with.
///
/// # Errors
/// When nothing fitting could be found, fetched or converted.
pub fn fetch(
    artist: &str,
    title: &str,
    length_s: Option<f64>,
    progress: &dyn Fn(String),
) -> Result<Arrived, String> {
    if artist.trim().is_empty() {
        return Err("no artist to look a community chart up by".to_owned());
    }
    fetch_into(
        &crate::import::import_dir()?,
        artist,
        title,
        length_s,
        progress,
    )
}

/// [`fetch`] into a given library folder.
///
/// # Errors
/// As [`fetch`].
pub fn fetch_into(
    library: &Path,
    artist: &str,
    title: &str,
    length_s: Option<f64>,
    progress: &dyn Fn(String),
) -> Result<Arrived, String> {
    if artist.trim().is_empty() {
        return Err("no artist to look a community chart up by".to_owned());
    }
    let ffmpeg =
        bridge::find_ffmpeg().ok_or("community charts need ffmpeg (brew install ffmpeg)")?;
    let client = Client::new(env!("CARGO_PKG_VERSION"));
    let hits = client.search(artist, title)?;
    let wanted = Wanted {
        artist: artist.to_owned(),
        title: title.to_owned(),
        length_s,
    };
    let fitting = candidates(&hits, &wanted);
    if fitting.is_empty() {
        return Err(choose::refusal(&hits, &wanted));
    }
    std::fs::create_dir_all(library)
        .map_err(|e| format!("cannot create {}: {e}", library.display()))?;
    let transcode = bridge::ffmpeg_transcoder(ffmpeg);
    let mut last = String::from("no community chart converted");
    for hit in fitting.into_iter().take(MAX_TRIES) {
        progress(format!("fetching the chart by {}...", who(hit)));
        match bring_in(&client, hit, library, &transcode) {
            Ok(arrived) => return Ok(arrived),
            Err(reason) => {
                bevy::log::warn!("community: {} ({}): {reason}", hit.md5, who(hit));
                last = reason;
            }
        }
    }
    Err(last)
}

/// Who made a chart, for a line: the charter, or the game for a rip.
fn who(hit: &Hit) -> String {
    choose::game_of(hit).unwrap_or_else(|| {
        if hit.charter.trim().is_empty() {
            "the community".to_owned()
        } else {
            hit.charter.trim().to_owned()
        }
    })
}

/// Download, unpack, convert one chart.
fn bring_in(
    client: &Client,
    hit: &Hit,
    library: &Path,
    transcode: &bridge::Transcoder<'_>,
) -> Result<Arrived, String> {
    let bytes = client.download(hit)?;
    let package = beatbyte_encore::decode(&bytes)?;
    drop(bytes);
    let scratch = std::env::temp_dir().join("beatbyte-encore").join(&hit.md5);
    // A leftover from an interrupted run must not mix into this one.
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)
        .map_err(|e| format!("cannot create {}: {e}", scratch.display()))?;
    let game = choose::game_of(hit);
    let written = package.write_folder(&scratch, game.as_deref());
    let outcome = written.and_then(|()| bridge::import(&scratch, library, transcode, now_ms()));
    let _ = std::fs::remove_dir_all(&scratch);
    match outcome? {
        Outcome::Imported {
            folder,
            kind,
            number,
            report,
            ..
        } => {
            let expert = report
                .notes
                .iter()
                .find(|(difficulty, _)| *difficulty == beatbyte_core::Difficulty::Expert)
                .map_or(0, |(_, n)| *n);
            if choose::too_many_opens(report.open_notes_dropped, expert) {
                // Holes where the open notes were: not this chart.
                let _ = std::fs::remove_dir_all(&folder);
                return Err(format!(
                    "{} open notes would be missing",
                    report.open_notes_dropped
                ));
            }
            // A downloaded chart brings no karaoke: look the words up as
            // the recording path does, timed to this chart's length.
            #[allow(clippy::cast_precision_loss)] // a song's milliseconds
            let length = (hit.length_ms > 0).then(|| hit.length_ms as f64 / 1000.0);
            let words = crate::lyrics_fetch::fetch_and_cache(
                &hit.artist,
                &hit.name,
                length,
                &folder.join(bridge::AUDIO_FILE),
            );
            let lyrics = if matches!(words, crate::lyrics_fetch::Outcome::Synced(_)) {
                ", with lyrics"
            } else {
                ""
            };
            Ok(Arrived {
                line: format!(
                    "added the {} chart by {} ({}){lyrics}",
                    label(kind),
                    who(hit),
                    version_name(kind, number)
                ),
                folder,
            })
        }
        Outcome::AlreadyThere {
            folder,
            kind,
            number,
        } => Ok(Arrived {
            line: format!("already in your library as {}", version_name(kind, number)),
            folder,
        }),
    }
}

/// "community" or "game".
fn label(kind: beatbyte_chart::twin::Kind) -> &'static str {
    if kind == beatbyte_chart::twin::Kind::GameRip {
        "game"
    } else {
        "community"
    }
}

/// The version's name as the browser shows it. Pure — tested.
#[must_use]
pub fn version_name(kind: beatbyte_chart::twin::Kind, number: u8) -> String {
    if kind == beatbyte_chart::twin::Kind::GameRip {
        format!("RIP {number}")
    } else {
        format!("COMMUNITY {number}")
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The status line names versions the way the chips do.
    #[test]
    fn a_version_is_named_like_its_chip() {
        use beatbyte_chart::twin::Kind;
        assert_eq!(version_name(Kind::Bridge, 2), "COMMUNITY 2");
        assert_eq!(version_name(Kind::GameRip, 1), "RIP 1");
    }

    /// The whole way against the real catalogue, into a throwaway home
    /// (never the player's library): the song arrives as a version with
    /// a chart and audio. Ignored by default — network, ffmpeg, minutes;
    /// `cargo test -p beatbyte-game --lib community -- --ignored`.
    #[test]
    #[ignore = "network and ffmpeg"]
    fn a_community_chart_arrives_as_a_version() {
        let home = std::env::temp_dir().join(format!("bb-community-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("home");
        let heard = std::cell::RefCell::new(Vec::new());
        let arrived = fetch_into(&home, "3 Doors Down", "Kryptonite", None, &|line| {
            heard.borrow_mut().push(line);
        })
        .expect("arrives");
        assert!(
            arrived.folder.starts_with(&home),
            "{}",
            arrived.folder.display()
        );
        assert!(arrived.folder.join("chart.json").exists());
        assert!(arrived.folder.join(bridge::AUDIO_FILE).exists());
        assert!(arrived.line.contains("COMMUNITY 1"), "{}", arrived.line);
        assert!(!heard.borrow().is_empty());
        // Asked again, it is already there.
        let again = fetch_into(&home, "3 Doors Down", "Kryptonite", None, &|_| {}).expect("again");
        assert!(again.line.contains("already"), "{}", again.line);
        std::fs::remove_dir_all(&home).ok();
    }

    /// Without an artist nothing can be told apart: no request is made.
    #[test]
    fn no_artist_means_no_lookup() {
        let heard = std::cell::RefCell::new(Vec::new());
        let result = fetch("  ", "Kryptonite", None, &|line| {
            heard.borrow_mut().push(line)
        });
        assert!(result.is_err());
        assert!(heard.borrow().is_empty(), "nothing was attempted");
    }
}
