//! The boot screen: shows the title while the demo song renders,
//! analyzes and charts itself in the background, then moves to the
//! main menu.

use beatbyte_audio::decode::AudioData;
use beatbyte_audio::{Analyzer, SpectralAnalyzer};
use beatbyte_chart::{ChartFile, GenerateMeta, generate_chart};
use bevy::prelude::*;
use bevy::tasks::futures_lite::future;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};

use crate::palette;
use crate::states::AppState;

/// A song ready to play: audio plus its chart.
#[derive(Resource, Clone)]
pub struct LoadedSong {
    /// Decoded (or synthesized) audio.
    pub audio: AudioData,
    /// The chart file (all difficulties).
    pub chart: ChartFile,
}

/// The in-flight background load of the demo song.
#[derive(Resource)]
struct DemoLoadTask(Task<LoadedSong>);

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

fn spawn_boot_screen(mut commands: Commands) {
    commands
        .spawn((
            BootScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(12),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("BEATBYTE"),
                TextFont {
                    font_size: FontSize::Px(96.0),
                    ..default()
                },
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new(format!("v{}", crate::VERSION)),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
            parent.spawn((
                Text::new("tuning the amps..."),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(palette::TEXT_DIM),
            ));
        });
}

/// Kick off the demo song build: render → analyze → chart, off-thread.
fn start_demo_load(mut commands: Commands) {
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let audio = beatbyte_audio::demo::render_demo_song();
        let analysis = SpectralAnalyzer::default().analyze(&audio);
        let chart = generate_chart(
            &analysis,
            &GenerateMeta {
                title: beatbyte_audio::demo::DEMO_TITLE.to_owned(),
                artist: beatbyte_audio::demo::DEMO_ARTIST.to_owned(),
                audio: String::new(), // played from memory, not a file
            },
        );
        LoadedSong { audio, chart }
    });
    commands.insert_resource(DemoLoadTask(task));
}

/// When the demo is ready, publish it and enter the menu.
fn poll_demo_load(
    mut commands: Commands,
    mut task: ResMut<DemoLoadTask>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if let Some(song) = block_on(future::poll_once(&mut task.0)) {
        info!(
            "demo song ready: \"{}\" — {:.1} BPM, {} difficulties",
            song.chart.song.title,
            song.chart.song.bpm,
            song.chart.charts.len()
        );
        commands.insert_resource(song);
        commands.remove_resource::<DemoLoadTask>();
        next_state.set(AppState::MainMenu);
    }
}

fn despawn_boot_screen(mut commands: Commands, entities: Query<Entity, With<BootScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
