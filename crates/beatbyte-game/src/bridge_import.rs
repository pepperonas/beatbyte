//! **B** in the song browser: convert what the Bridge downloader
//! fetched into BG versions of the library's songs (ADR-0022).
//!
//! The work is `beatbyte_library::bridge`, the same as
//! `beatbyte-cli bridge`; this module only finds the two folders on
//! this machine — Bridge's download folder from Bridge's own settings,
//! the library from where imports land — and words the outcome for the
//! status line. It runs as a [`crate::chore`]: off the main thread,
//! reported on the overlay, and the library is rescanned when it
//! finishes, wherever the player has gone by then.

use std::path::{Path, PathBuf};

use beatbyte_library::bridge::{self, Outcome};

/// Convert every download in Bridge's folder that is not in the
/// library yet.
///
/// # Errors
/// When Bridge's folder, the library or ffmpeg cannot be found, there
/// is nothing to convert, or every conversion failed.
pub fn import_all() -> Result<String, String> {
    let source = bridge_folder()
        .ok_or("no Bridge download folder - set one in Bridge's settings and download a chart")?;
    let library = crate::import::import_dir()?;
    let ffmpeg = bridge::find_ffmpeg()
        .ok_or("the Bridge import needs ffmpeg for the audio (brew install ffmpeg)")?;
    let downloads = bridge::downloads_under(&source);
    if downloads.is_empty() {
        return Err(format!("no Bridge download in {}", source.display()));
    }
    std::fs::create_dir_all(&library)
        .map_err(|e| format!("cannot create {}: {e}", library.display()))?;
    let transcode = bridge::ffmpeg_transcoder(ffmpeg);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let mut tally = Tally::default();
    for download in &downloads {
        match bridge::import(download, &library, &transcode, now_ms) {
            Ok(Outcome::Imported {
                folder,
                kind,
                number,
                ..
            }) => {
                bevy::log::info!(
                    "bridge: {}-{number:02} → {}",
                    beatbyte_chart::twin::numbered_tag(kind),
                    folder.display()
                );
                tally.written.push(number);
            }
            Ok(Outcome::AlreadyThere { .. }) => tally.known += 1,
            Err(error) => {
                bevy::log::warn!("bridge: {}: {error}", name_of(download));
                tally.failed.push(name_of(download));
            }
        }
    }
    let line = tally.line();
    if tally.written.is_empty() && tally.known == 0 {
        Err(line)
    } else {
        Ok(line)
    }
}

/// The download folder Bridge's settings name.
fn bridge_folder() -> Option<PathBuf> {
    let settings = bridge::bridge_settings_path(&dirs::config_dir()?);
    bridge::library_path_from_settings(&std::fs::read_to_string(settings).ok()?)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// What a run did.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    /// The BG numbers written.
    written: Vec<u8>,
    /// Downloads already in the library.
    known: usize,
    /// Downloads that failed, by folder name.
    failed: Vec<String>,
}

impl Tally {
    /// The status line. Pure — tested.
    fn line(&self) -> String {
        let mut parts = Vec::new();
        match self.written.len() {
            0 => parts.push("Bridge: nothing new".to_owned()),
            1 => parts.push(format!(
                "Bridge: 1 chart converted (BG-{:02})",
                self.written[0]
            )),
            n => parts.push(format!("Bridge: {n} charts converted")),
        }
        if self.known > 0 {
            parts.push(format!("{} already in the library", self.known));
        }
        if !self.failed.is_empty() {
            parts.push(format!(
                "{} failed ({})",
                self.failed.len(),
                crate::ui::font_safe(&self.failed.join(", "))
            ));
        }
        parts.join(" - ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_says_what_happened() {
        let one = Tally {
            written: vec![2],
            known: 3,
            failed: vec![],
        };
        assert_eq!(
            one.line(),
            "Bridge: 1 chart converted (BG-02) - 3 already in the library"
        );
        let none = Tally {
            written: vec![],
            known: 0,
            failed: vec!["Band - Song".into()],
        };
        assert_eq!(none.line(), "Bridge: nothing new - 1 failed (Band - Song)");
        let many = Tally {
            written: vec![1, 2],
            ..Tally::default()
        };
        assert_eq!(many.line(), "Bridge: 2 charts converted");
    }
}
