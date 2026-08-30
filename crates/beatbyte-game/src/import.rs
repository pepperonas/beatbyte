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

/// The batch queue: every dropped file lands here; one import task
/// runs at a time. The first version imported ONE file per gesture
/// and silently dropped the rest — "it looked like songs were lost."
#[derive(Resource, Default)]
pub struct ImportQueue {
    pending: std::collections::VecDeque<PathBuf>,
    /// Files in the current batch (incl. skipped/failed).
    pub total: usize,
    /// Finished files (ok + failed + skipped).
    pub done: usize,
    /// Successfully imported.
    pub ok: usize,
    /// Failed to import.
    pub failed: usize,
    /// Skipped (unsupported extension, duplicate).
    pub skipped: usize,
    /// Title currently being imported.
    pub current: Option<String>,
    /// Seconds since the batch finished (drives the summary fade).
    pub since_finished: f32,
}

impl ImportQueue {
    /// Whether a batch is running.
    #[must_use]
    pub fn active(&self) -> bool {
        self.done < self.total
    }
}

/// One line summing up a finished batch. Pure — tested.
#[must_use]
pub fn summary_line(ok: usize, failed: usize, skipped: usize) -> String {
    let mut parts = vec![format!("{ok} imported")];
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    parts.join(" - ")
}

/// The in-flight import task.
#[derive(Resource)]
struct ImportTask(Task<Result<String, String>>);

/// The import plugin.
pub struct ImportPlugin;

impl Plugin for ImportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ImportStatus>()
            .init_resource::<ImportQueue>()
            .add_systems(Startup, spawn_import_panel)
            .add_systems(
                Update,
                (
                    handle_drops.run_if(
                        in_state(AppState::SongSelect).or_else(in_state(AppState::MainMenu)),
                    ),
                    start_next_import,
                    poll_import,
                    update_import_panel,
                )
                    .chain(),
            );
    }
}

/// Enqueue every dropped file. Unsupported extensions and duplicates
/// are counted as skipped RIGHT HERE so the batch summary is honest.
fn handle_drops(
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
    mut status: ResMut<ImportStatus>,
    mut queue: ResMut<ImportQueue>,
) {
    for drop in drops.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        // A fresh gesture after a finished batch starts new counters.
        if !queue.active() {
            *queue = ImportQueue::default();
        }
        queue.total += 1;
        let extension = path_buf
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            queue.skipped += 1;
            queue.done += 1;
            status.0 = format!("skipped `.{extension}` (not audio)");
            continue;
        }
        let duplicate = path_buf
            .file_name()
            .map(|name| sanitize_folder_name(&name.to_string_lossy()))
            .and_then(|folder| import_dir().ok().map(|dir| dir.join(folder)))
            .is_some_and(|dir| dir.exists());
        if duplicate {
            queue.skipped += 1;
            queue.done += 1;
            status.0 = "already imported - skipped".to_owned();
            continue;
        }
        queue.pending.push_back(path_buf.clone());
    }
}

/// Start the next queued import when nothing is running.
fn start_next_import(
    mut commands: Commands,
    mut queue: ResMut<ImportQueue>,
    mut status: ResMut<ImportStatus>,
    running: Option<Res<ImportTask>>,
) {
    if running.is_some() {
        return;
    }
    let Some(source) = queue.pending.pop_front() else {
        return;
    };
    let (title, artist) = song_name_from_stem(
        &source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "imported song".to_owned()),
    );
    status.0 = format!("importing \"{}\"...", crate::ui::font_safe(&title));
    info!("import: \"{title}\" by {artist} from {}", source.display());
    queue.current = Some(title.clone());
    let task = AsyncComputeTaskPool::get()
        .spawn(async move { import_song(&source, &title, &artist).map(|()| title) });
    commands.insert_resource(ImportTask(task));
}

/// When the import finishes: rescan the library so the browser
/// rebuilds (it watches the resource for changes).
fn poll_import(
    mut commands: Commands,
    task: Option<ResMut<ImportTask>>,
    mut status: ResMut<ImportStatus>,
    mut queue: ResMut<ImportQueue>,
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
    queue.done += 1;
    queue.current = None;
    match result {
        Ok(title) => {
            queue.ok += 1;
            // Log the finish, not just the start. Until this line
            // existed, a successful import wrote nothing at all, so a
            // log could not tell an import that worked from one that
            // silently did nothing - which is exactly the ambiguity
            // that made a real report ("import stopped working") take
            // an hour to answer.
            info!("import: \"{title}\" done");
            status.0 = format!("\"{}\" imported", crate::ui::font_safe(&title));
            if let (Some(builtins), Some(mut library)) = (builtins, library) {
                let charts: Vec<_> = builtins.0.iter().map(|song| song.chart.clone()).collect();
                *library = scan_library(&charts);
            }
        }
        Err(reason) => {
            queue.failed += 1;
            warn!("import failed: {reason}");
            status.0 = format!("import failed: {reason}");
        }
    }
    if !queue.active() {
        queue.since_finished = 0.0;
        status.0 = summary_line(queue.ok, queue.failed, queue.skipped);
    }
}

/// Root of the import overlay (visible while a batch runs).
#[derive(Component)]
struct ImportPanelRoot;

/// The panel box (its border pulses).
#[derive(Component)]
struct ImportPanelBox;

/// The panel's text line.
#[derive(Component)]
struct ImportPanelText;

/// The progress-bar fill.
#[derive(Component)]
struct ImportBarFill;

/// Build the (hidden) import overlay once. It lives outside any
/// screen so a drop is NEVER invisible, whichever state is active.
fn spawn_import_panel(mut commands: Commands, font: Res<crate::ui::UiFont>) {
    commands
        .spawn((
            ImportPanelRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::FlexEnd,
                justify_content: JustifyContent::Center,
                padding: UiRect::bottom(px(30)),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(50),
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    ImportPanelBox,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        padding: UiRect::all(px(14)),
                        width: px(440),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(10)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.05, 0.11, 0.94)),
                    BorderColor::all(crate::palette::BRAND.with_alpha(0.6)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        ImportPanelText,
                        Text::new(""),
                        font.text(11.0),
                        TextColor(crate::palette::TEXT),
                    ));
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(10),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                        ))
                        .with_children(|bar| {
                            bar.spawn((
                                ImportBarFill,
                                Node {
                                    width: percent(0),
                                    height: percent(100),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(crate::palette::BRAND),
                            ));
                        });
                });
        });
}

/// Drive the overlay: show while a batch runs (plus a 4-second
/// summary), pulse the border, ease the bar toward the batch
/// progress, and flash the fill whenever a file finishes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)] // Bevy system: params are DI
fn update_import_panel(
    time: Res<Time>,
    mut queue: ResMut<ImportQueue>,
    mut root: Query<&mut Visibility, With<ImportPanelRoot>>,
    mut boxes: Query<&mut BorderColor, With<ImportPanelBox>>,
    mut texts: Query<&mut Text, With<ImportPanelText>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<ImportBarFill>>,
    mut last_done: Local<usize>,
    mut flash: Local<f32>,
) {
    let Ok(mut visibility) = root.single_mut() else {
        return;
    };
    if queue.total == 0 {
        *visibility = Visibility::Hidden;
        return;
    }
    if !queue.active() {
        queue.since_finished += time.delta_secs();
    }
    let show = queue.active() || queue.since_finished < 4.0;
    *visibility = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !show {
        return;
    }

    if queue.done != *last_done {
        *last_done = queue.done;
        *flash = 1.0;
    }
    *flash = (*flash - time.delta_secs() * 2.5).max(0.0);

    if let Ok(mut text) = texts.single_mut() {
        let line = if queue.active() {
            let name = queue.current.as_deref().unwrap_or("...");
            let name = crate::ui::font_safe(name);
            format!(
                "importing \"{name}\"  ({}/{})",
                (queue.done + 1).min(queue.total),
                queue.total
            )
        } else {
            summary_line(queue.ok, queue.failed, queue.skipped)
        };
        if text.0 != line {
            text.0 = line;
        }
    }

    // Border pulse while working; steady when done.
    if let Ok(mut border) = boxes.single_mut() {
        let alpha = if queue.active() {
            0.45 + 0.35 * (time.elapsed_secs() * 6.0).sin()
        } else {
            0.8
        };
        *border = BorderColor::all(crate::palette::BRAND.with_alpha(alpha));
    }

    if let Ok((mut node, mut color)) = fills.single_mut() {
        let target = queue.done as f32 / queue.total.max(1) as f32 * 100.0;
        let current = match node.width {
            Val::Percent(value) => value,
            _ => 0.0,
        };
        // Ease toward the target; snap when close.
        let eased = current + (target - current) * (time.delta_secs() * 8.0).min(1.0);
        node.width = percent(if (eased - target).abs() < 0.5 {
            target
        } else {
            eased
        });
        color.0 = crate::palette::BRAND.mix(&Color::WHITE, *flash * 0.7);
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
    let (chart_path, pointer) = plan_chart_write(&folder)?;
    beatbyte_chart::save_chart_file(&chart_path, &chart).map_err(|e| e.to_string())?;
    if let Some((pointer_path, text)) = pointer {
        std::fs::write(pointer_path, text).map_err(|e| format!("cannot write pointer: {e}"))?;
    }
    Ok(())
}

/// Where a freshly generated chart may be written (ADR-0011).
///
/// NEVER over an existing one: the chart on disk may carry recorded
/// sessions, hand edits, or be a designed version — none of which a
/// re-analysis may destroy. A fresh folder gets `chart.json`; a
/// folder that already has one gets the next version file plus a
/// pointer naming it, so the re-import is still what plays while
/// everything it would have clobbered stays on disk.
fn plan_chart_write(
    folder: &Path,
) -> Result<(std::path::PathBuf, Option<(std::path::PathBuf, String)>), String> {
    use beatbyte_chart::versions;
    let base = folder.join(versions::BASE_CHART);
    if !base.exists() {
        return Ok((base, None));
    }
    let existing: Vec<String> = std::fs::read_dir(folder)
        .map_err(|e| format!("cannot list folder: {e}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let version_name = versions::next_version_name(&existing);
    let pointer = versions::ActivePointer {
        active: version_name.clone(),
    };
    let text = serde_json::to_string_pretty(&pointer)
        .map_err(|e| format!("cannot serialize pointer: {e}"))?;
    Ok((
        folder.join(version_name),
        Some((folder.join(versions::POINTER_FILE), text)),
    ))
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

#[cfg(test)]
mod summary_tests {
    use super::summary_line;

    #[test]
    fn summary_mentions_only_nonzero_buckets() {
        assert_eq!(summary_line(3, 0, 0), "3 imported");
        assert_eq!(summary_line(2, 0, 1), "2 imported - 1 skipped");
        assert_eq!(summary_line(0, 2, 1), "0 imported - 1 skipped - 2 failed");
    }
}

#[cfg(test)]
mod write_plan_tests {
    use super::plan_chart_write;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("beatbyte-wp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_fresh_folder_gets_the_base_chart_and_no_pointer() {
        let dir = scratch("fresh");
        let (path, pointer) = plan_chart_write(&dir).expect("plans");
        assert_eq!(path, dir.join("chart.json"));
        assert!(pointer.is_none());
    }

    #[test]
    fn an_existing_chart_is_never_the_write_target() {
        // The chart on disk may carry recorded sessions or hand
        // edits; a re-analysis must not destroy either. The fresh
        // chart becomes v2 and the pointer makes it the one that
        // plays.
        let dir = scratch("occupied");
        std::fs::write(dir.join("chart.json"), "{}").expect("existing chart");
        let (path, pointer) = plan_chart_write(&dir).expect("plans");
        assert_eq!(path, dir.join("chart.v2.json"));
        let (pointer_path, text) = pointer.expect("a pointer moves play to the new version");
        assert_eq!(pointer_path, dir.join("chart-active.json"));
        assert!(text.contains("chart.v2.json"));
    }

    #[test]
    fn versions_keep_counting_up_never_reusing_a_name() {
        // Reusing a version name would overwrite the very thing the
        // scheme exists to protect.
        let dir = scratch("counting");
        std::fs::write(dir.join("chart.json"), "{}").expect("base");
        std::fs::write(dir.join("chart.v2.json"), "{}").expect("v2");
        std::fs::write(dir.join("chart.v3.json"), "{}").expect("v3");
        let (path, _) = plan_chart_write(&dir).expect("plans");
        assert_eq!(path, dir.join("chart.v4.json"));
    }
}
