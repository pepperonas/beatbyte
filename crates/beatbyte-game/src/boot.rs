//! The boot screen: shows the title while the song library is
//! scanned in the background, then moves to the main menu.
//!
//! **BeatByte ships no songs.** It used to boot with two synthesized
//! chiptune instrumentals ("Circuit Breaker", "Solder Groove") that
//! demonstrated the pipeline without any copyrighted audio — and one
//! of them carried hand-written karaoke lyrics, so a track with no
//! voice on it sang along. The user's verdict was short: that cannot
//! be. They are gone from the game. The synthesis itself
//! ([`beatbyte_audio::demo`]) stays where it always belonged: as the
//! deterministic fixture the analysis and charting regression tests
//! are built on, and behind `beatbyte-cli demo` for anyone who wants
//! them on disk.
//!
//! The built-in MECHANISM stays (`SongSource::Builtin`,
//! [`BuiltinSongs`] — inserted empty): a bundled song remains a
//! supported shape, there simply is not one.

use beatbyte_audio::decode::AudioData;
use beatbyte_chart::ChartFile;
use bevy::prelude::*;
use bevy::tasks::futures_lite::future;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};

use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// A song ready to play.
#[derive(Resource, Clone)]
pub struct LoadedSong {
    /// The chart file (all difficulties).
    pub chart: ChartFile,
    /// Where the audio comes from.
    pub audio: SongAudio,
    /// Karaoke lyrics, when the song has them.
    pub lyrics: Option<beatbyte_chart::lyrics::Lyrics>,
    /// The song's own lyric offset in milliseconds (positive = lyrics
    /// later), read from beside the audio and adjustable from the
    /// pause menu. Sources vary per song; this is where that lives.
    pub lyric_offset_ms: i32,
}

/// The audio side of a loaded song.
#[derive(Clone)]
pub enum SongAudio {
    /// Samples already in memory. Nothing constructs this while no
    /// song is bundled; the path stays because a built-in song is
    /// still a supported shape (and the browser preview and
    /// `prepare_song` both handle it).
    Memory(AudioData),
    /// A file on disk, streamed by the music thread.
    File(std::path::PathBuf),
}

/// Scan the library and flag which built-ins carry lyrics.
///
/// The flag cannot come from the scan itself: a built-in's lyrics
/// are compiled in, not a file on disk. The pairing relies on the
/// documented order — `scan_library` puts the built-ins first, in
/// the order it was handed them — which a test pins.
#[must_use]
pub fn scan_with_builtins(songs: &[LoadedSong]) -> crate::library::SongLibrary {
    let charts: Vec<_> = songs.iter().map(|song| song.chart.clone()).collect();
    let mut library = crate::library::scan_library(&charts);
    for (entry, song) in library.entries.iter_mut().zip(songs) {
        entry.has_lyrics = song.lyrics.is_some();
    }
    library
}

/// The cached built-in songs, in library order (survive across
/// screens; selecting one in the browser clones from here instead of
/// re-rendering).
#[derive(Resource, Clone)]
pub struct BuiltinSongs(pub Vec<LoadedSong>);

/// The in-flight background scan of the song library.
#[derive(Resource)]
struct LibraryScanTask(Task<crate::library::SongLibrary>);

/// Plugin for the boot screen shown in [`AppState::Boot`].
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Boot),
            (spawn_boot_screen, start_library_scan),
        )
        .add_systems(Update, poll_library_scan.run_if(in_state(AppState::Boot)))
        .add_systems(OnExit(AppState::Boot), despawn_boot_screen);
    }
}

/// Marker for entities belonging to the boot screen.
#[derive(Component)]
struct BootScreen;

fn spawn_boot_screen(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            BootScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("BEATBYTE"),
                font.text(52.0),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new(format!("v{}", crate::VERSION)),
                font.text(12.0),
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                Text::new("tuning the amps..."),
                font.text(14.0),
                TextColor(palette::TEXT_DIM),
            ));
        });
}

/// Scan the song library off-thread: reading and parsing a chart per
/// song is file I/O, and a full library of them would stall the first
/// frames on the main thread. (It used to hide the demo synthesis,
/// which was the expensive part.)
fn start_library_scan(mut commands: Commands) {
    let task = AsyncComputeTaskPool::get().spawn(async move { crate::library::scan_library(&[]) });
    commands.insert_resource(LibraryScanTask(task));
}

/// When the scan is done: publish the library and enter the menu.
fn poll_library_scan(
    mut commands: Commands,
    mut task: ResMut<LibraryScanTask>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Some(library) = block_on(future::poll_once(&mut task.0)) {
        info!("song library: {} entr(ies)", library.entries.len());
        if library.entries.is_empty() {
            info!("no songs yet — drag an audio file onto the window to import one");
        }
        commands.insert_resource(library);
        // Empty, but present: systems take `Res<BuiltinSongs>`, and a
        // missing resource makes a Bevy system vanish silently.
        commands.insert_resource(BuiltinSongs(Vec::new()));
        commands.remove_resource::<LibraryScanTask>();
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_boot_screen(mut commands: Commands, entities: Query<Entity, With<BootScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
