//! The boot screen: shows the title while the built-in songs render,
//! analyze and chart themselves in the background, then scans the
//! song library and moves to the main menu.

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::{Analyzer, SpectralAnalyzer};
use beatbyte_chart::{ChartFile, GenerateMeta, generate_chart};
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
}

/// The audio side of a loaded song.
#[derive(Clone)]
pub enum SongAudio {
    /// Fully synthesized/decoded samples (the demo).
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

/// The in-flight background build of the built-in songs.
#[derive(Resource)]
struct DemoLoadTask(Task<Vec<LoadedSong>>);

/// Plugin for the boot screen shown in [`AppState::Boot`].
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Boot),
            (spawn_boot_screen, start_demo_load),
        )
        .add_systems(Update, poll_demo_load.run_if(in_state(AppState::Boot)))
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

/// Kick off the built-in song builds: render → analyze → chart each,
/// off-thread.
fn start_demo_load(mut commands: Commands) {
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let build = |audio: beatbyte_audio::decode::AudioData, title: &str, artist: &str| {
            let analysis = SpectralAnalyzer::default().analyze(&audio);
            let mut chart = generate_chart(
                &analysis,
                &GenerateMeta {
                    title: title.to_owned(),
                    artist: artist.to_owned(),
                    audio: String::new(), // played from memory, not a file
                },
            );
            // Honest metadata: the demo songs ARE chiptune.
            chart.song.genre = Some("Chiptune".to_owned());
            LoadedSong {
                chart,
                audio: SongAudio::Memory(audio),
                lyrics: None,
            }
        };
        use beatbyte_audio::demo;
        let mut songs = vec![
            build(
                demo::render_demo_song(),
                demo::DEMO_TITLE,
                demo::DEMO_ARTIST,
            ),
            build(
                demo::render_groove_song(),
                demo::GROOVE_TITLE,
                demo::GROOVE_ARTIST,
            ),
        ];
        // The demo song ships original karaoke lyrics (enhanced LRC,
        // hand-timed to its 128 BPM grid), so a fresh clone
        // demonstrates the feature without any user content.
        let demo_lyrics = beatbyte_chart::lyrics::parse_lrc(include_str!(
            "../../../assets/lyrics/circuit-breaker.lrc"
        ));
        if let Some(song) = songs.first_mut() {
            song.lyrics = Some(demo_lyrics);
        }
        songs
    });
    commands.insert_resource(DemoLoadTask(task));
}

/// When the built-ins are ready: publish them, scan the library,
/// enter the menu.
fn poll_demo_load(
    mut commands: Commands,
    mut task: ResMut<DemoLoadTask>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Some(songs) = block_on(future::poll_once(&mut task.0)) {
        for song in &songs {
            info!(
                "built-in song ready: \"{}\" — {:.1} BPM, {} difficulties",
                song.chart.song.title,
                song.chart.song.bpm,
                song.chart.charts.len()
            );
        }
        let library = scan_with_builtins(&songs);
        info!("song library: {} entr(ies)", library.entries.len());
        commands.insert_resource(library);
        commands.insert_resource(BuiltinSongs(songs));
        commands.remove_resource::<DemoLoadTask>();
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_boot_screen(mut commands: Commands, entities: Query<Entity, With<BootScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
