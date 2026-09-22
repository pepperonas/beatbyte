//! The quiet pass that fills in what a song's document still lacks.
//!
//! Some of what a document holds is expensive: the file fingerprint
//! reads every byte — 2.4 GB on this library — and the feature
//! measurement decodes the song on top of that. Neither can happen
//! during a scan, and neither may happen at start-up: the player
//! opened the game to play.
//!
//! So it happens later, slowly, one song at a time, and never while
//! anything the player asked for is running. A song is visited once
//! per measurement version: the questions are "has this file been
//! fingerprinted" and "has the current measurement run", not "is
//! this document complete" — most of what a document lacks can never
//! be filled (the files in this library carry no tags at all), so a
//! worker chasing completeness would walk the library for ever.
//!
//! Nothing here is user-visible by design. It writes `song.json` and
//! stops; the next scan reads what it wrote.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};

/// How long after the game starts the first song may be visited.
///
/// Long enough that a start-up is never competing with it, short
/// enough that a session spent in the menus still gets work done.
pub const GRACE_S: f32 = 20.0;

/// How long to wait between songs.
///
/// The work is not urgent and the machine belongs to the player.
pub const GAP_S: f32 = 3.0;

/// The songs still to visit, and the one in flight.
#[derive(Resource, Default)]
pub struct Librarian {
    /// Folders left to visit.
    queue: Vec<PathBuf>,
    /// The song being visited.
    task: Option<Task<Option<String>>>,
    /// Seconds since the session began.
    age_s: f32,
    /// Seconds since the last song finished.
    idle_s: f32,
    /// Whether the queue has been filled this session.
    filled: bool,
    /// How many songs have been given a fingerprint this session.
    pub done: usize,
}

impl Librarian {
    /// How many songs are still to visit.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.queue.len() + usize::from(self.task.is_some())
    }
}

/// Whether the quiet pass may run right now.
///
/// Pure so the rule can be pinned rather than read out of a system.
/// Every condition is a reason to stay out of the way: the player is
/// playing, they asked for something else, or the session is too
/// young for housekeeping.
#[must_use]
pub fn may_run(age_s: f32, idle_s: f32, playing: bool, busy: bool) -> bool {
    !playing && !busy && age_s >= GRACE_S && idle_s >= GAP_S
}

/// Which folders still owe a fingerprint.
///
/// Pure. A folder with no document is skipped rather than given one:
/// writing a document is the import's job and the migration's, and a
/// background pass inventing one for a folder it has never seen
/// would be the kind of surprise a quiet worker must not spring.
#[must_use]
pub fn work_list(folders: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    folders
        .into_iter()
        .filter(|dir| {
            beatbyte_library::store::read(dir)
                .is_some_and(|doc| beatbyte_library::completeness::needs_work(&doc))
        })
        .collect()
}

/// Visit one song: fingerprint its file, re-read its tags and its
/// lyric counts, and write the document if any of it is news.
///
/// Returns the song's title when something was written.
fn visit(dir: &Path) -> Option<String> {
    let doc = beatbyte_library::store::read(dir)?;
    let audio = dir.join(&doc.file.filename);
    if !audio.exists() {
        return None;
    }
    let chart_name = doc.gameplay.chart_file.clone()?;
    let chart_path = dir.join(&chart_name);
    let text = std::fs::read_to_string(&chart_path).ok()?;
    let chart = beatbyte_chart::ChartFile::from_json(&text).ok()?;
    let facts = beatbyte_library::build::FolderFacts {
        chart: Some(&chart),
        chart_version: Some(beatbyte_library::fresh::generation(&chart_name)),
        chart_filename: Some(chart_name),
        audio_filename: doc.file.filename.clone(),
        extension: audio
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase()),
        // The document already knows when this song arrived, and
        // `imported_at` is written once — this can never move it.
        oldest_file_ms: doc.lifecycle.imported_at,
        loudness: beatbyte_library::folder::read_loudness_facts(&audio),
        lyrics: beatbyte_library::folder::lyric_facts(&audio, &chart_path),
        content_hash: beatbyte_library::folder::fingerprint(&audio).map(|print| print.tagged()),
        tags: Some(beatbyte_audio::read_tags(&audio)),
        features: beatbyte_library::folder::measure_features(&audio),
        source_kind: doc.source.kind,
    };
    let title = doc.identity.title.value.clone();
    let built = beatbyte_library::build::document_for(
        &facts,
        Some(doc),
        beatbyte_library::SongId::from_parts(0, 0),
        now_ms(),
    );
    match beatbyte_library::store::save_if_changed(dir, &built) {
        Ok(true) => Some(title),
        Ok(false) => None,
        Err(error) => {
            warn!("librarian: cannot write {}: {error}", dir.display());
            None
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Registers the quiet pass.
pub struct LibrarianPlugin;

impl Plugin for LibrarianPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Librarian>().add_systems(Update, tick);
    }
}

/// One step per frame: fill the queue once, then visit songs.
///
/// Registered globally rather than on a screen — the pass outlives
/// every screen, and a poll that stops when the player leaves the
/// browser strands a task in the pool.
fn tick(
    mut librarian: ResMut<Librarian>,
    library: Option<Res<crate::library::SongLibrary>>,
    chore: Option<Res<crate::chore::Chore>>,
    imports: Option<Res<crate::import::ImportQueue>>,
    state: Option<Res<State<crate::states::AppState>>>,
    time: Res<Time>,
) {
    let delta = time.delta_secs();
    librarian.age_s += delta;
    if let Some(task) = librarian.task.as_mut() {
        if let Some(written) = block_on(future::poll_once(task)) {
            librarian.task = None;
            librarian.idle_s = 0.0;
            if let Some(title) = written {
                librarian.done += 1;
                debug!("librarian: {title}");
            }
        }
        return;
    }
    librarian.idle_s += delta;

    let playing =
        state.is_some_and(|state| matches!(state.get(), crate::states::AppState::Gameplay));
    let busy =
        chore.is_some_and(|chore| chore.running()) || imports.is_some_and(|queue| queue.active());
    if !may_run(librarian.age_s, librarian.idle_s, playing, busy) {
        return;
    }

    if !librarian.filled {
        let Some(library) = library else { return };
        let folders: Vec<PathBuf> = library
            .entries
            .iter()
            .filter_map(|entry| match &entry.source {
                crate::library::SongSource::File { chart_path, .. } => {
                    chart_path.parent().map(Path::to_path_buf)
                }
                crate::library::SongSource::Builtin(_) => None,
            })
            .collect();
        librarian.queue = work_list(folders);
        librarian.filled = true;
        if !librarian.queue.is_empty() {
            info!(
                "librarian: {} song(s) to fingerprint",
                librarian.queue.len()
            );
        }
        return;
    }

    let Some(dir) = librarian.queue.pop() else {
        return;
    };
    librarian.task = Some(AsyncComputeTaskPool::get().spawn(async move { visit(&dir) }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn housekeeping_waits_for_the_player_to_be_doing_nothing_else() {
        assert!(may_run(GRACE_S, GAP_S, false, false));
        assert!(!may_run(GRACE_S, GAP_S, true, false), "they are playing");
        assert!(
            !may_run(GRACE_S, GAP_S, false, true),
            "they asked for something else"
        );
        assert!(
            !may_run(GRACE_S - 0.1, GAP_S, false, false),
            "the game has only just started"
        );
        assert!(
            !may_run(GRACE_S, GAP_S - 0.1, false, false),
            "the last song finished a moment ago"
        );
    }

    #[test]
    fn only_songs_that_still_owe_a_measurement_are_queued() {
        let dir = std::env::temp_dir().join(format!("bb-lbn-{}", std::process::id()));
        let (todo, done, bare) = (dir.join("todo"), dir.join("done"), dir.join("bare"));
        for folder in [&todo, &done, &bare] {
            std::fs::create_dir_all(folder).expect("folder");
        }
        let mut doc = beatbyte_library::doc::SongDoc::new(
            beatbyte_library::SongId::from_parts(1, 1),
            beatbyte_library::Sourced::stated(
                "Maria".to_owned(),
                beatbyte_library::MetaSource::Embedded,
            ),
            "maria.m4a".to_owned(),
            beatbyte_library::SourceKind::LocalFile,
            1_000,
        );
        beatbyte_library::store::save(&todo, &doc).expect("writes");
        // "Done" means BOTH expensive answers are in: the
        // fingerprint and a feature run at the current version. A
        // document with only the first still owes.
        doc.file.content_hash = Some("fnv1a64:0000000000000001:2".to_owned());
        doc.analysis.push(beatbyte_library::doc::AnalysisRun {
            stage: beatbyte_library::build::FEATURES_STAGE.to_owned(),
            analyzer: "beatbyte-features".to_owned(),
            version: beatbyte_audio::features::VERSION,
            analyzed_at: 2_000,
        });
        beatbyte_library::store::save(&done, &doc).expect("writes");

        let list = work_list(vec![todo.clone(), done, bare]);
        // A folder with no document at all is left alone: writing
        // one is the import's job, not a background pass's.
        assert_eq!(list, vec![todo]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
