//! Drag-and-drop song import.
//!
//! Drop an audio file onto the window (menu or song browser): it is
//! copied into the imported-songs folder, analyzed and charted off
//! the main thread, and appears in the browser when done. The
//! terminal workflow (`beatbyte-cli generate`) keeps existing for
//! fine control — this is the zero-friction path.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::futures_lite::future;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};

use beatbyte_audio::{Analyzer, SpectralAnalyzer};
use beatbyte_chart::{GenerateMeta, generate_chart};

use crate::library::{SONGS_DIR, scan_library, user_songs_dir};
use crate::states::AppState;

/// Extensions the decoder is verified to read (see the decode tests).
const AUDIO_EXTENSIONS: [&str; 5] = ["wav", "ogg", "flac", "mp3", "m4a"];

/// Status of the current import, shown in the browser.
#[derive(Resource, Default)]
pub struct ImportStatus(pub String);

/// The in-flight import task.
#[derive(Resource)]
struct ImportTask(Task<Result<String, String>>);

/// The import plugin.
pub struct ImportPlugin;

impl Plugin for ImportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ImportStatus>().add_systems(
            Update,
            (
                handle_drops
                    .run_if(in_state(AppState::SongSelect).or_else(in_state(AppState::MainMenu))),
                poll_import,
            ),
        );
    }
}

/// React to files dropped onto the window.
fn handle_drops(
    mut commands: Commands,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
    mut status: ResMut<ImportStatus>,
    running: Option<Res<ImportTask>>,
) {
    for drop in drops.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        if running.is_some() {
            status.0 = "an import is already running".to_owned();
            continue;
        }
        let extension = path_buf
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            status.0 = format!(
                "cannot import `.{extension}` - supported: {}",
                AUDIO_EXTENSIONS.join("/")
            );
            continue;
        }
        let source = path_buf.clone();
        let (title, artist) = song_name_from_stem(
            &source
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "imported song".to_owned()),
        );
        status.0 = format!("importing \"{title}\"...");
        info!("import: \"{title}\" by {artist} from {}", source.display());
        let task = AsyncComputeTaskPool::get()
            .spawn(async move { import_song(&source, &title, &artist).map(|()| title) });
        commands.insert_resource(ImportTask(task));
        // One file per gesture; extra dropped files are ignored.
        break;
    }
}

/// When the import finishes: rescan the library so the browser
/// rebuilds (it watches the resource for changes).
fn poll_import(
    mut commands: Commands,
    task: Option<ResMut<ImportTask>>,
    mut status: ResMut<ImportStatus>,
    builtins: Option<Res<crate::boot::BuiltinSongs>>,
    library: Option<ResMut<crate::library::SongLibrary>>,
) {
    let Some(mut task) = task else {
        return;
    };
    let Some(result) = block_on(future::poll_once(&mut task.0)) else {
        return;
    };
    commands.remove_resource::<ImportTask>();
    match result {
        Ok(title) => {
            status.0 = format!("\"{title}\" imported - have fun!");
            if let (Some(builtins), Some(mut library)) = (builtins, library) {
                let charts: Vec<_> = builtins.0.iter().map(|song| song.chart.clone()).collect();
                *library = scan_library(&charts);
            }
        }
        Err(reason) => {
            warn!("import failed: {reason}");
            status.0 = format!("import failed: {reason}");
        }
    }
}

/// Copy, analyze, chart and save — the blocking part, off-thread.
fn import_song(source: &Path, title: &str, artist: &str) -> Result<(), String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| "file has no name".to_owned())?;
    let folder = import_dir()?.join(sanitize_folder_name(&file_name.to_string_lossy()));
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create folder: {e}"))?;
    let audio_dest = folder.join(file_name);
    std::fs::copy(source, &audio_dest).map_err(|e| format!("cannot copy audio: {e}"))?;

    let audio = beatbyte_audio::decode_file(&audio_dest).map_err(|e| e.to_string())?;
    let analysis = SpectralAnalyzer::default().analyze(&audio);
    let chart = generate_chart(
        &analysis,
        &GenerateMeta {
            title: title.to_owned(),
            artist: artist.to_owned(),
            audio: file_name.to_string_lossy().into_owned(),
        },
    );
    if chart
        .validate()
        .iter()
        .any(|i| i.severity == beatbyte_chart::Severity::Error)
    {
        return Err("generated chart failed validation".to_owned());
    }
    let chart_path = folder.join("chart.json");
    beatbyte_chart::save_chart_file(&chart_path, &chart).map_err(|e| e.to_string())?;
    Ok(())
}

/// Where imports land: `songs/imported/` next to a development /
/// portable install, the user songs dir otherwise.
fn import_dir() -> Result<PathBuf, String> {
    let local = PathBuf::from(SONGS_DIR);
    if local.is_dir() {
        return Ok(local.join("imported"));
    }
    user_songs_dir()
        .map(|dir| dir.join("imported"))
        .ok_or_else(|| "no songs directory on this platform".to_owned())
}

/// A filesystem-safe folder name from a file name.
fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "imported-song".to_owned()
    } else {
        trimmed.chars().take(60).collect()
    }
}

/// Title/artist from a file stem. Downloaded files tend to look like
/// `Artist - Title (Official Video) (4K Remaster) [id]`; the
/// bracketed noise goes, and an `Artist - Title` split is honored.
pub fn song_name_from_stem(stem: &str) -> (String, String) {
    let mut cleaned = String::with_capacity(stem.len());
    let mut depth = 0i32;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((artist, title)) = cleaned.split_once(" - ") {
        let artist = artist.trim();
        let title = title.trim();
        if !artist.is_empty() && !title.is_empty() {
            return (title.to_owned(), artist.to_owned());
        }
    }
    let title = if cleaned.is_empty() {
        stem.to_owned()
    } else {
        cleaned
    };
    (title, "Unknown".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{sanitize_folder_name, song_name_from_stem};

    #[test]
    fn downloaded_file_names_come_out_clean() {
        let (title, artist) = song_name_from_stem(
            "Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster) [dQw4w9WgXcQ]",
        );
        assert_eq!(title, "Never Gonna Give You Up");
        assert_eq!(artist, "Rick Astley");
    }

    #[test]
    fn plain_stems_become_the_title_with_unknown_artist() {
        let (title, artist) = song_name_from_stem("my-cool-track");
        assert_eq!(title, "my-cool-track");
        assert_eq!(artist, "Unknown");
    }

    #[test]
    fn all_bracket_noise_leaves_the_stem_itself() {
        let (title, artist) = song_name_from_stem("[123] (live)");
        assert_eq!(title, "[123] (live)");
        assert_eq!(artist, "Unknown");
    }

    #[test]
    fn folder_names_are_filesystem_safe() {
        assert_eq!(
            sanitize_folder_name("Never Gonna (4K) [id].m4a"),
            "never-gonna--4k---id--m4a"
        );
        assert_eq!(sanitize_folder_name("///"), "imported-song");
    }
}
