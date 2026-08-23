//! The song browser: pick a song and a difficulty, see your best.

use beatbyte_core::Difficulty;
use bevy::prelude::*;

use crate::boot::{DemoSong, LoadedSong, SongAudio};
use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::palette;
use crate::scores::ScoreBoard;
use crate::states::AppState;
use crate::ui::UiFont;

/// The difficulty the player will play.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedDifficulty(pub Difficulty);

impl Default for SelectedDifficulty {
    fn default() -> Self {
        SelectedDifficulty(Difficulty::Medium)
    }
}

/// The highlighted song row.
#[derive(Resource, Default)]
struct BrowserCursor(usize);

/// Plugin for the song browser.
pub struct SongSelectPlugin;

impl Plugin for SongSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedDifficulty>()
            .init_resource::<BrowserCursor>()
            .add_systems(OnEnter(AppState::SongSelect), spawn_browser)
            .add_systems(
                Update,
                (browser_input, refresh_browser).run_if(in_state(AppState::SongSelect)),
            )
            .add_systems(OnExit(AppState::SongSelect), despawn_browser);
    }
}

#[derive(Component)]
struct BrowserScreen;

/// A song row (index into the library).
#[derive(Component)]
struct SongRow(usize);

/// The details block under the list.
#[derive(Component)]
struct DetailText;

fn spawn_browser(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
) {
    if cursor.0 >= library.entries.len() {
        cursor.0 = 0;
    }
    commands
        .spawn((
            BrowserScreen,
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(14),
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("SONG SELECT"),
                font.text(26.0),
                TextColor(palette::BRAND),
                Node {
                    margin: UiRect::bottom(px(18)),
                    ..default()
                },
            ));
            for (index, entry) in library.entries.iter().enumerate() {
                parent.spawn((
                    SongRow(index),
                    Text::new(format!("{} - {}", entry.title, entry.artist)),
                    font.text(14.0),
                    TextColor(palette::TEXT_DIM),
                ));
            }
            if library.entries.len() == 1 {
                parent.spawn((
                    Text::new("drop charts into songs/ to add your music"),
                    font.text(9.0),
                    TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                ));
            }
            parent.spawn((
                DetailText,
                Text::new(""),
                font.text(11.0),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(22)),
                    ..default()
                },
            ));
            parent.spawn((
                Text::new("UP/DOWN song   LEFT/RIGHT difficulty   ENTER rock   ESC back"),
                font.text(9.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.7)),
                Node {
                    margin: UiRect::top(px(20)),
                    ..default()
                },
            ));
        });
}

fn browser_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut selected: ResMut<SelectedDifficulty>,
    demo: Res<DemoSong>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let count = library.entries.len();
    if count == 0 {
        if keys.just_pressed(KeyCode::Escape) {
            next_state.set(AppState::MainMenu);
        }
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        cursor.0 = (cursor.0 + 1) % count;
    }
    let entry = &library.entries[cursor.0];

    // Difficulty stepping is constrained to what the chart offers.
    let offered = &entry.difficulties;
    if !offered.contains(&selected.0)
        && let Some(&first) = offered.first()
    {
        selected.0 = first;
    }
    let position = offered.iter().position(|d| *d == selected.0).unwrap_or(0);
    if keys.just_pressed(KeyCode::ArrowLeft) && position > 0 {
        selected.0 = offered[position - 1];
    }
    if keys.just_pressed(KeyCode::ArrowRight) && position + 1 < offered.len() {
        selected.0 = offered[position + 1];
    }

    if keys.just_pressed(KeyCode::Enter) {
        match prepare_song(entry, &demo) {
            Ok(song) => {
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("cannot load \"{}\": {reason}", entry.title),
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_state.set(AppState::MainMenu);
    }
}

/// Build the [`LoadedSong`] for an entry. Demo comes from cache; file
/// songs re-read their chart (it may have changed on disk) and stream
/// audio from the resolved path.
pub fn prepare_song(entry: &SongEntry, demo: &DemoSong) -> Result<LoadedSong, String> {
    match &entry.source {
        SongSource::BuiltinDemo => Ok(demo.0.clone()),
        SongSource::File {
            chart_path,
            audio_path,
        } => {
            let chart = beatbyte_chart::load_chart_file(chart_path).map_err(|e| e.to_string())?;
            let issues = chart.validate();
            if let Some(worst) = issues
                .iter()
                .find(|i| i.severity == beatbyte_chart::Severity::Error)
            {
                return Err(format!("chart became invalid: {worst}"));
            }
            Ok(LoadedSong {
                chart,
                audio: SongAudio::File(audio_path.clone()),
            })
        }
    }
}

/// Keep rows and the detail block in sync with the cursor.
fn refresh_browser(
    library: Res<SongLibrary>,
    cursor: Res<BrowserCursor>,
    selected: Res<SelectedDifficulty>,
    scores: Res<ScoreBoard>,
    mut rows: Query<(&SongRow, &mut TextColor)>,
    mut detail: Query<&mut Text, With<DetailText>>,
) {
    for (row, mut color) in &mut rows {
        color.0 = if row.0 == cursor.0 {
            palette::BRAND
        } else {
            palette::TEXT_DIM
        };
    }
    let Some(entry) = library.entries.get(cursor.0) else {
        return;
    };
    if let Ok(mut text) = detail.single_mut() {
        let duration = entry.duration_s.map_or_else(String::new, |d| {
            format!("  {}:{:02}", d as u32 / 60, d as u32 % 60)
        });
        let best = scores
            .best(&entry.title, &entry.artist, selected.0)
            .map_or_else(
                || "no record yet".to_owned(),
                |b| format!("best {}  ({:.1}%)", b.score, b.accuracy * 100.0),
            );
        let line = format!(
            "{:.0} BPM{duration}   <{}>   {best}",
            entry.bpm,
            selected.0.display_name().to_uppercase()
        );
        if text.0 != line {
            text.0 = line;
        }
    }
}

fn despawn_browser(mut commands: Commands, entities: Query<Entity, With<BrowserScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}
