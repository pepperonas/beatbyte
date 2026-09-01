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

/// Whether a path carries one of the supported audio extensions.
fn is_audio(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

// ── Content fingerprints ────────────────────────────────────────────
//
// A duplicate is the same BYTES, not the same file name: the old rule
// (skip when the sanitized folder name exists) re-imported a renamed
// copy and wrongly skipped a different song that happened to share a
// file name. The fingerprint is a 64-bit FNV-1a over the file's
// content plus its size — std-only, deterministic, and plenty for
// telling one's own music library apart (this is de-duplication, not
// cryptography).

/// One file's identity for de-duplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    /// FNV-1a 64 of the file's bytes.
    pub hash: u64,
    /// The file's size — a nearly free second factor.
    pub size: u64,
}

/// FNV-1a 64, one chunk at a time. Pure — tested against the
/// published test vectors.
#[must_use]
pub fn fnv1a_update(mut hash: u64, chunk: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    for byte in chunk {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// The FNV-1a 64 offset basis (the empty-input hash).
pub const FNV_BASIS: u64 = 0xCBF2_9CE4_8422_2325;

/// Fingerprint a file by streaming its bytes. `None` when the file
/// cannot be read (vanished mid-scan, permissions).
#[must_use]
pub fn audio_fingerprint(path: &Path) -> Option<Fingerprint> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = FNV_BASIS;
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hash = fnv1a_update(hash, &buffer[..read]);
    }
    Some(Fingerprint { hash, size })
}

/// Every fingerprint ever imported, persisted so an in-game delete
/// stays deleted: the watcher must not resurrect a song just because
/// its file still sits in the watched folder (user decision,
/// 2026-09-01).
#[derive(Resource, Default)]
pub struct ImportedIndex {
    entries: std::collections::HashSet<Fingerprint>,
}

impl ImportedIndex {
    /// Whether this fingerprint was imported before.
    #[must_use]
    pub fn contains(&self, fingerprint: Fingerprint) -> bool {
        self.entries.contains(&fingerprint)
    }

    /// Record a fingerprint (call on SUCCESSFUL import only — a
    /// failed one stays retryable) and persist best-effort.
    pub fn record(&mut self, fingerprint: Fingerprint) {
        if self.entries.insert(fingerprint) {
            self.save();
        }
    }

    /// The index file: `imported-hashes.json` beside the imports.
    fn path() -> Option<PathBuf> {
        import_dir()
            .ok()
            .map(|dir| dir.join("imported-hashes.json"))
    }

    /// Load the persisted index (missing file = empty, first run).
    #[must_use]
    pub fn load() -> ImportedIndex {
        let Some(path) = ImportedIndex::path() else {
            return ImportedIndex::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return ImportedIndex::default();
        };
        ImportedIndex {
            entries: parse_index(&text),
        }
    }

    fn save(&self) {
        let Some(path) = ImportedIndex::path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, render_index(&self.entries)) {
            warn!("cannot save the import index: {error}");
        }
    }
}

/// The index file format: one `hash-size` pair per line inside a
/// JSON string array — trivially forward-compatible. Pure — tested.
#[must_use]
pub fn render_index(entries: &std::collections::HashSet<Fingerprint>) -> String {
    let mut lines: Vec<String> = entries
        .iter()
        .map(|f| format!("\"{:016x}-{}\"", f.hash, f.size))
        .collect();
    lines.sort();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

/// Parse the index file; unreadable entries are dropped (the file is
/// input too). Pure — tested.
#[must_use]
pub fn parse_index(text: &str) -> std::collections::HashSet<Fingerprint> {
    text.split('"')
        .filter_map(|token| {
            let (hash, size) = token.split_once('-')?;
            Some(Fingerprint {
                hash: u64::from_str_radix(hash, 16).ok()?,
                size: size.parse().ok()?,
            })
        })
        .collect()
}

/// Status of the current import, shown in the browser.
#[derive(Resource, Default)]
pub struct ImportStatus(pub String);

/// The batch queue: every dropped file lands here; one import task
/// runs at a time. The first version imported ONE file per gesture
/// and silently dropped the rest — "it looked like songs were lost."
#[derive(Resource, Default)]
pub struct ImportQueue {
    pending: std::collections::VecDeque<(PathBuf, Option<Fingerprint>)>,
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
    /// Fingerprint of the file currently being imported (recorded in
    /// the index on success).
    current_fingerprint: Option<Fingerprint>,
    /// Source path of the running import (a failure is remembered so
    /// the watcher does not retry it every five seconds).
    current_source: Option<PathBuf>,
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
        app.insert_resource(ImportedIndex::load())
            .init_resource::<ImportStatus>()
            .init_resource::<ImportQueue>()
            .init_resource::<WatchState>()
            .add_systems(Startup, spawn_import_panel)
            .add_systems(
                Update,
                (
                    handle_drops.run_if(
                        in_state(AppState::SongSelect).or_else(in_state(AppState::MainMenu)),
                    ),
                    watch_song_folder.run_if(
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
    index: Res<ImportedIndex>,
    mut settings: ResMut<crate::config::Settings>,
) {
    for drop in drops.read() {
        let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        // A dropped FOLDER is not an import - it is the answer to
        // "which folder should BeatByte watch?" (there is no native
        // folder picker in a keyboard/gamepad UI, but dropping is
        // already the import gesture). Files keep importing directly.
        if path_buf.is_dir() {
            info!("watch folder set: {}", path_buf.display());
            status.0 = format!(
                "watching {} for new songs",
                path_buf.file_name().map_or_else(
                    || path_buf.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                )
            );
            settings.watch_folder = Some(path_buf.clone());
            crate::config::save_settings(&settings);
            continue;
        }
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
        // Duplicates by CONTENT, not by name: the fingerprint knows a
        // renamed copy and clears a different song that shares a file
        // name. The old folder-name check stays as a fast second net
        // (same name AND not in the index = an import that predates
        // the index).
        let fingerprint = audio_fingerprint(path_buf);
        if fingerprint.is_some_and(|f| index.contains(f)) {
            queue.skipped += 1;
            queue.done += 1;
            status.0 = "already imported (same content) - skipped".to_owned();
            continue;
        }
        let name_taken = path_buf
            .file_name()
            .map(|name| sanitize_folder_name(&name.to_string_lossy()))
            .and_then(|folder| import_dir().ok().map(|dir| dir.join(folder)))
            .is_some_and(|dir| dir.exists());
        if name_taken {
            queue.skipped += 1;
            queue.done += 1;
            status.0 = "already imported - skipped".to_owned();
            continue;
        }
        queue.pending.push_back((path_buf.clone(), fingerprint));
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
    let Some((source, fingerprint)) = queue.pending.pop_front() else {
        return;
    };
    queue.current_fingerprint = fingerprint;
    queue.current_source = Some(source.clone());
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
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn poll_import(
    mut commands: Commands,
    task: Option<ResMut<ImportTask>>,
    mut status: ResMut<ImportStatus>,
    mut queue: ResMut<ImportQueue>,
    mut index: ResMut<ImportedIndex>,
    mut watch: ResMut<WatchState>,
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
            // Only a SUCCESSFUL import burns the fingerprint - a
            // failed one stays retryable.
            if let Some(fingerprint) = queue.current_fingerprint.take() {
                index.record(fingerprint);
            }
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
            // The watcher must not retry a broken file every poll
            // for the rest of the session.
            if let Some(source) = queue.current_source.take() {
                watch.failed.insert(source);
            }
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
    // A .lrc beside the source travels with the song, so imported
    // tracks keep their karaoke lyrics. Best-effort: a failed copy
    // must not fail the import.
    let lyric_source = source.with_extension("lrc");
    if lyric_source.is_file() {
        let _ = std::fs::copy(&lyric_source, audio_dest.with_extension("lrc"));
    }

    let audio = beatbyte_audio::decode_file(&audio_dest).map_err(|e| e.to_string())?;
    let analysis = SpectralAnalyzer::default().analyze(&audio);
    let mut chart = generate_chart(
        &analysis,
        &GenerateMeta {
            title: title.to_owned(),
            artist: artist.to_owned(),
            audio: file_name.to_string_lossy().into_owned(),
        },
    );
    // The file's own genre tag, when it carries one - a metadata
    // probe, no second decode.
    chart.song.genre = beatbyte_audio::read_genre(&audio_dest);
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

// ── The watched song folder ─────────────────────────────────────────
//
// Polling, not filesystem events (user decision, 2026-09-01): every
// few seconds a cheap directory walk lists the audio files and
// compares size+mtime against what was seen last time. Only NEW or
// CHANGED files get hashed - a settled library costs a walk and no
// I/O beyond directory metadata. No new dependency.

/// Seconds between scans of the watched folder.
const WATCH_PERIOD_S: f32 = 5.0;
/// How many new candidates are hashed per tick - spreads the initial
/// scan of a large folder over several ticks instead of hitching the
/// menu once.
const WATCH_HASH_BUDGET: usize = 2;
/// How deep the recursive walk goes (artist/album nesting, not `/`).
const WATCH_MAX_DEPTH: usize = 5;

/// What one poll remembered about one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSighting {
    /// File size in bytes.
    pub size: u64,
    /// Modification time, as seconds (resolution is irrelevant —
    /// only equality between two sightings matters).
    pub mtime: u64,
}

/// A file is imported only once two consecutive sightings agree:
/// a file still being copied into the folder grows between polls,
/// and importing half a song would chart half a song. Pure — tested.
#[must_use]
pub fn settled(previous: Option<FileSighting>, current: FileSighting) -> bool {
    previous == Some(current)
}

/// The watcher's memory between polls.
#[derive(Resource, Default)]
pub struct WatchState {
    timer: f32,
    /// Last sighting per path; a path present here with an equal
    /// sighting is either settled-and-handled or unchanged.
    seen: std::collections::HashMap<PathBuf, FileSighting>,
    /// Paths already handled (enqueued or skipped) — never touched
    /// again unless the file CHANGES.
    handled: std::collections::HashSet<PathBuf>,
    /// Imports that failed this session — not retried every poll.
    failed: std::collections::HashSet<PathBuf>,
}

/// Collect the audio files under `root`, bounded depth, no symlink
/// following.
fn walk_audio(root: &Path, depth: usize, into: &mut Vec<PathBuf>) {
    if depth > WATCH_MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk_audio(&path, depth + 1, into);
        } else if is_audio(&path) {
            into.push(path);
        }
    }
}

/// One sighting of a file, from metadata only (no content I/O).
fn sight(path: &Path) -> Option<FileSighting> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(FileSighting {
        size: meta.len(),
        mtime,
    })
}

/// Scan the watched folder and enqueue what is new.
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn watch_song_folder(
    time: Res<Time>,
    settings: Res<crate::config::Settings>,
    index: Res<ImportedIndex>,
    mut watch: ResMut<WatchState>,
    mut queue: ResMut<ImportQueue>,
    mut status: ResMut<ImportStatus>,
) {
    let Some(root) = settings.watch_folder.clone() else {
        return;
    };
    watch.timer += time.delta_secs();
    if watch.timer < WATCH_PERIOD_S {
        return;
    }
    watch.timer = 0.0;
    // A running batch owns the pipeline and the status line.
    if queue.active() {
        return;
    }
    if !root.is_dir() {
        // Unmounted drive, renamed folder: dormant, not an error.
        return;
    }
    let mut files = Vec::new();
    walk_audio(&root, 0, &mut files);
    let mut budget = WATCH_HASH_BUDGET;
    for path in files {
        if budget == 0 {
            break;
        }
        let Some(current) = sight(&path) else {
            continue;
        };
        let previous = watch.seen.get(&path).copied();
        if previous != Some(current) {
            // New or still changing: remember the sighting; a file
            // that changed is eligible again next poll.
            watch.seen.insert(path.clone(), current);
            watch.handled.remove(&path);
            continue;
        }
        if watch.handled.contains(&path) || watch.failed.contains(&path) {
            continue;
        }
        if !settled(previous, current) {
            continue;
        }
        // Settled and unhandled: fingerprint it (the budgeted, only
        // expensive step) and decide.
        budget -= 1;
        watch.handled.insert(path.clone());
        let Some(fingerprint) = audio_fingerprint(&path) else {
            continue;
        };
        if index.contains(fingerprint) {
            // Silently: the watcher re-seeing the library every boot
            // is normal life, not a batch worth a summary line.
            continue;
        }
        if !queue.active() {
            *queue = ImportQueue::default();
        }
        queue.total += 1;
        queue.pending.push_back((path.clone(), Some(fingerprint)));
        status.0 = format!(
            "found new song in the watched folder: {}",
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned()
            )
        );
        info!("watch: enqueued {}", path.display());
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn fnv1a_matches_the_published_vectors() {
        // The classic FNV-1a 64 test vectors; a silent tweak to the
        // prime or basis would change every fingerprint and make the
        // whole index disagree with itself after an update.
        assert_eq!(fnv1a_update(FNV_BASIS, b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a_update(FNV_BASIS, b"a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(fnv1a_update(FNV_BASIS, b"foobar"), 0x85944171F73967E8);
        // Chunked = whole: the streaming update must not depend on
        // buffer boundaries.
        let whole = fnv1a_update(FNV_BASIS, b"foobar");
        let chunked = fnv1a_update(fnv1a_update(FNV_BASIS, b"foo"), b"bar");
        assert_eq!(whole, chunked);
    }

    #[test]
    fn the_index_round_trips_and_shrugs_off_garbage() {
        let mut entries = std::collections::HashSet::new();
        entries.insert(Fingerprint {
            hash: 0xDEAD_BEEF_0000_0001,
            size: 4_567_890,
        });
        entries.insert(Fingerprint { hash: 7, size: 0 });
        let text = render_index(&entries);
        assert_eq!(parse_index(&text), entries, "round trip");
        // The file is input too: junk entries drop, good ones stay.
        let dirty = text.replace('[', "[\n\"not-a-hash\",");
        assert_eq!(parse_index(&dirty), entries);
        assert!(parse_index("").is_empty());
        assert!(parse_index("{ wrong: true }").is_empty());
    }

    #[test]
    fn a_file_is_settled_only_when_two_sightings_agree() {
        // A file still being copied grows between polls - importing
        // it would chart half a song.
        let first = FileSighting {
            size: 100,
            mtime: 10,
        };
        let grown = FileSighting {
            size: 200,
            mtime: 11,
        };
        assert!(!settled(None, first), "never on first sight");
        assert!(!settled(Some(first), grown), "not while it grows");
        assert!(settled(Some(grown), grown), "two equal sightings");
    }

    #[test]
    fn fingerprints_tell_files_apart_by_content_not_name() {
        // Two files, same name in different dirs, different bytes -
        // and a renamed copy with identical bytes.
        let dir = std::env::temp_dir().join(format!("bb-fp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).expect("mkdir");
        std::fs::create_dir_all(dir.join("b")).expect("mkdir");
        std::fs::write(dir.join("a/song.wav"), b"AAAA").expect("write");
        std::fs::write(dir.join("b/song.wav"), b"BBBBBB").expect("write");
        std::fs::write(dir.join("b/renamed.wav"), b"AAAA").expect("write");
        let a = audio_fingerprint(&dir.join("a/song.wav")).expect("fp a");
        let b = audio_fingerprint(&dir.join("b/song.wav")).expect("fp b");
        let renamed = audio_fingerprint(&dir.join("b/renamed.wav")).expect("fp r");
        assert_ne!(a, b, "same name, different content");
        assert_eq!(a, renamed, "different name, same content");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_walk_finds_nested_audio_and_ignores_the_rest() {
        let dir = std::env::temp_dir().join(format!("bb-walk-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("artist/album")).expect("mkdir");
        std::fs::write(dir.join("top.mp3"), b"x").expect("write");
        std::fs::write(dir.join("artist/album/deep.ogg"), b"x").expect("write");
        std::fs::write(dir.join("artist/cover.jpg"), b"x").expect("write");
        let mut found = Vec::new();
        walk_audio(&dir, 0, &mut found);
        let names: Vec<String> = found
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(found.len(), 2, "exactly the two audio files: {names:?}");
        assert!(names.contains(&"top.mp3".to_owned()));
        assert!(names.contains(&"deep.ogg".to_owned()));
        std::fs::remove_dir_all(&dir).ok();
    }
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
