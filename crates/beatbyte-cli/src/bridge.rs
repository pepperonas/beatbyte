//! `bridge`: convert charts the Bridge downloader fetched into BG
//! twins of the library's songs (ADR-0022).
//!
//! Without a path it reads Bridge's own download folder from Bridge's
//! settings and converts everything in it; a download already in the
//! library is recognised by its fingerprint and skipped, so running it
//! again after every download session is the intended use.
//!
//! ⚠️ Nothing is written while the game runs — a folder appearing
//! under a live browser is harmless, but a half-written one is not
//! worth the risk; quit the game, or use its own Bridge import.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use beatbyte_library::bridge::{self, Outcome};

/// Convert `source` (one download, or a folder of them) into the
/// library at `library`. Each defaults to where it lives on this
/// machine.
pub fn run(source: Option<PathBuf>, library: Option<PathBuf>) -> ExitCode {
    if game_is_running() {
        eprintln!("BeatByte is running — quit it first, or use BRIDGE in the song browser.");
        return ExitCode::from(1);
    }
    let Some(source) = source.or_else(default_source) else {
        eprintln!("no Bridge download folder: pass one, or set it in Bridge's settings");
        return ExitCode::from(2);
    };
    let Some(library) = library.or_else(default_library) else {
        eprintln!("no library directory on this platform: pass --library");
        return ExitCode::from(2);
    };
    let Some(ffmpeg) = bridge::find_ffmpeg() else {
        eprintln!("ffmpeg is needed to convert the audio (brew install ffmpeg)");
        return ExitCode::from(2);
    };
    let downloads = bridge::downloads_under(&source);
    if downloads.is_empty() {
        eprintln!("no Bridge download under {}", source.display());
        return ExitCode::from(1);
    }
    if let Err(error) = std::fs::create_dir_all(&library) {
        eprintln!("cannot create {}: {error}", library.display());
        return ExitCode::from(1);
    }
    let transcode = bridge::ffmpeg_transcoder(ffmpeg);
    let (mut written, mut known, mut failed) = (0, 0, 0);
    for download in &downloads {
        let name = download.file_name().map_or_else(
            || download.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match bridge::import(download, &library, &transcode, now_ms()) {
            Ok(Outcome::Imported {
                folder,
                number,
                original,
                report,
            }) => {
                written += 1;
                println!(
                    "{}",
                    describe(&name, number, original.as_deref(), &folder, &report)
                );
            }
            Ok(Outcome::AlreadyThere { number, .. }) => {
                known += 1;
                println!("{name}: already BG-{number:02}");
            }
            Err(error) => {
                failed += 1;
                eprintln!("{name}: {error}");
            }
        }
    }
    println!("{written} converted, {known} already there, {failed} failed");
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// One line per converted download. Pure — tested.
fn describe(
    name: &str,
    number: u8,
    original: Option<&str>,
    folder: &Path,
    report: &beatbyte_chart::bridge::Report,
) -> String {
    let under = original.map_or_else(|| "a song of its own".to_owned(), |o| format!("under {o}"));
    let notes: Vec<String> = report
        .notes
        .iter()
        .map(|(difficulty, count)| format!("{} {count}", difficulty.id()))
        .collect();
    let mut line = format!(
        "{name}: BG-{number:02} {under} → {} ({})",
        folder.display(),
        notes.join(", ")
    );
    if report.open_notes_dropped > 0 {
        line.push_str(&format!(
            "; {} open notes left out",
            report.open_notes_dropped
        ));
    }
    if !report.derived.is_empty() {
        let levels: Vec<&str> = report.derived.iter().map(|d| d.id()).collect();
        line.push_str(&format!(
            "; {} derived from the level above",
            levels.join(", ")
        ));
    }
    if report.taps_as_hopo > 0 {
        line.push_str(&format!("; {} taps as HOPOs", report.taps_as_hopo));
    }
    if report.early_notes_dropped > 0 {
        line.push_str(&format!(
            "; {} notes before the start left out",
            report.early_notes_dropped
        ));
    }
    line
}

fn default_source() -> Option<PathBuf> {
    let settings = bridge::bridge_settings_path(&dirs::config_dir()?);
    bridge::library_path_from_settings(&std::fs::read_to_string(settings).ok()?)
}

fn default_library() -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("beatbyte")
            .join("songs")
            .join("imported"),
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn game_is_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "beatbyte"])
        .output()
        .is_ok_and(|out| !out.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beatbyte_core::Difficulty;

    #[test]
    fn a_conversion_says_where_it_went_and_what_it_left_out() {
        let report = beatbyte_chart::bridge::Report {
            notes: vec![(Difficulty::Hard, 900), (Difficulty::Expert, 1200)],
            open_notes_dropped: 3,
            early_notes_dropped: 0,
            taps_as_hopo: 0,
            derived: vec![Difficulty::Medium, Difficulty::Easy],
        };
        let line = describe(
            "TTFAF (Stargazer)",
            2,
            Some("dragonforce"),
            Path::new("/l/x"),
            &report,
        );
        assert_eq!(
            line,
            "TTFAF (Stargazer): BG-02 under dragonforce → /l/x (hard 900, expert 1200); \
             3 open notes left out; medium, easy derived from the level above"
        );
        assert!(describe("S", 1, None, Path::new("/l/y"), &report).contains("a song of its own"));
    }
}
