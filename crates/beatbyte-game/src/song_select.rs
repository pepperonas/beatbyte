//! The song browser: pick a song and a difficulty, see your best.

use beatbyte_core::Difficulty;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;

use crate::controls::MenuNav;

use crate::boot::{BuiltinSongs, LoadedSong, SongAudio};
use crate::library::{SongEntry, SongLibrary, SongSource};
use crate::palette;
use crate::scores::ScoreBoard;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

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
pub struct BrowserCursor(pub usize);

/// Plugin for the song browser.
pub struct SongSelectPlugin;

impl Plugin for SongSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedDifficulty>()
            .init_resource::<BrowserCursor>()
            .add_systems(OnEnter(AppState::SongSelect), spawn_browser)
            .add_systems(
                Update,
                (
                    browser_input,
                    refresh_browser,
                    rebuild_after_import,
                    follow_selection,
                )
                    .run_if(in_state(AppState::SongSelect)),
            )
            .add_systems(OnExit(AppState::SongSelect), despawn_browser);
    }
}

#[derive(Component)]
struct BrowserScreen;

/// A song row (index into the library). Carries `Button`.
#[derive(Component)]
struct SongRow(usize);

/// A row's title text.
#[derive(Component)]
struct SongTitle(usize);

/// A row's artist text, in the right-hand column.
#[derive(Component)]
struct SongArtist(usize);

/// The details block under the list.
#[derive(Component)]
struct DetailText;

/// The scrolling viewport the song rows live in.
#[derive(Component)]
struct SongList;

fn spawn_browser(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
) {
    spawn_browser_impl(&mut commands, &font, &library, &mut cursor);
}

fn spawn_browser_impl(
    commands: &mut Commands,
    font: &UiFont,
    library: &SongLibrary,
    cursor: &mut BrowserCursor,
) {
    if cursor.0 >= library.entries.len() {
        cursor.0 = 0;
    }
    commands
        .spawn((BrowserScreen, ui_kit::screen_root()))
        .with_children(|parent| {
            ui_kit::header(parent, font, "SONG SELECT", "pick a track and a difficulty");
            parent
                .spawn((SongList, ui_kit::scroll_panel()))
                .with_children(|panel| {
                    for (index, entry) in library.entries.iter().enumerate() {
                        panel
                            .spawn((SongRow(index), Button, ui_kit::row()))
                            .with_children(|row| {
                                // Title and artist were one string joined
                                // by " - "; as two columns the list scans
                                // by title, which is how anyone looks for
                                // a song.
                                row.spawn((
                                    SongTitle(index),
                                    Text::new(font.safe(&entry.title)),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::label_node(),
                                ));
                                row.spawn((
                                    SongArtist(index),
                                    Text::new(font.safe(&entry.artist)),
                                    font.text(ui_kit::ROW),
                                    TextColor(palette::TEXT_DIM),
                                    ui_kit::value_node(),
                                ));
                            });
                    }
                });
            parent.spawn((
                DetailText,
                Text::new(""),
                font.text(ui_kit::ROW),
                TextColor(palette::TEXT),
                Node {
                    margin: UiRect::top(px(ui_kit::FOOTER_GAP)),
                    ..default()
                },
            ));
            parent.spawn((
                ImportNote,
                Text::new("drag an audio file onto the window to import it"),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.75)),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
            ui_kit::footer(
                parent,
                font,
                "UP/DOWN song  LEFT/RIGHT difficulty  ENTER rock  E edit  DEL delete  ESC back",
            );
        });
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn browser_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut library: ResMut<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    mut selected: ResMut<SelectedDifficulty>,
    builtins: Res<BuiltinSongs>,
    mut next_state: ResMut<NextState<AppState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    rows: Query<(&SongRow, &Interaction), Changed<Interaction>>,
    time: Res<Time>,
    mut status: ResMut<crate::import::ImportStatus>,
    mut delete_armed: Local<(Option<usize>, f32)>,
) {
    let nav = MenuNav::read(&keys, pads.iter());
    let back = nav.back || mouse.just_pressed(MouseButton::Right);
    let count = library.entries.len();
    if count == 0 {
        if back {
            next_state.set(AppState::MainMenu);
        }
        return;
    }
    if nav.up {
        cursor.0 = (cursor.0 + count - 1) % count;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % count;
    }
    // Mouse: wheel scrolls the list; clicking a row selects it, and
    // clicking the already-selected row starts it.
    for event in wheel.read() {
        if event.y > 0.0 {
            cursor.0 = (cursor.0 + count - 1) % count;
        } else if event.y < 0.0 {
            cursor.0 = (cursor.0 + 1) % count;
        }
    }
    // Hover selects, click starts — the same rule as every other
    // menu. This list used to need two clicks (one to select, one to
    // start) and ignored hover entirely.
    let pointer = ui_kit::read_rows(rows.iter().map(|(row, i)| (row.0, i)));
    if let Some(index) = pointer.hovered {
        cursor.0 = index;
    }
    let clicked_selected = pointer.clicked;
    let entry = &library.entries[cursor.0];

    // Difficulty stepping is constrained to what the chart offers.
    let offered = &entry.difficulties;
    if !offered.contains(&selected.0)
        && let Some(&first) = offered.first()
    {
        selected.0 = first;
    }
    let position = offered.iter().position(|d| *d == selected.0).unwrap_or(0);
    if nav.left && position > 0 {
        selected.0 = offered[position - 1];
    }
    if nav.right && position + 1 < offered.len() {
        selected.0 = offered[position + 1];
    }

    if nav.confirm || clicked_selected {
        match prepare_song(entry, &builtins) {
            Ok(song) => {
                commands.insert_resource(song);
                next_state.set(AppState::Gameplay);
            }
            Err(reason) => error!("cannot load \"{}\": {reason}", entry.title),
        }
    }
    // E opens the chart editor (file-based songs only — the demo is
    // generated, editing it would be lost on the next boot).
    if keys.just_pressed(KeyCode::KeyE)
        && let crate::library::SongSource::File {
            chart_path,
            audio_path,
        } = &entry.source
    {
        match crate::editor_ui::open_editor(&mut commands, chart_path, audio_path, selected.0) {
            Ok(()) => next_state.set(AppState::Editor),
            Err(reason) => error!("cannot edit \"{}\": {reason}", entry.title),
        }
    }
    // BACKSPACE/DEL removes the highlighted song from disk — twice,
    // because it deletes files. Built-ins cannot be removed.
    delete_armed.1 = (delete_armed.1 - time.delta_secs()).max(0.0);
    if delete_armed.1 <= 0.0 {
        delete_armed.0 = None;
    }
    if keys.just_pressed(KeyCode::Backspace) || keys.just_pressed(KeyCode::Delete) {
        match &entry.source {
            crate::library::SongSource::Builtin(_) => {
                status.0 = "built-in songs cannot be deleted".to_owned();
            }
            crate::library::SongSource::File { chart_path, .. } => {
                if delete_armed.0 == Some(cursor.0) {
                    let title = entry.title.clone();
                    match crate::library::remove_song_files(chart_path) {
                        Ok(()) => {
                            status.0 = format!("\"{title}\" deleted");
                            let charts: Vec<_> =
                                builtins.0.iter().map(|song| song.chart.clone()).collect();
                            *library = crate::library::scan_library(&charts);
                        }
                        Err(reason) => status.0 = format!("cannot delete: {reason}"),
                    }
                    *delete_armed = (None, 0.0);
                } else {
                    *delete_armed = (Some(cursor.0), 3.0);
                    status.0 = format!(
                        "delete \"{}\" and its files? press again to confirm",
                        entry.title
                    );
                }
            }
        }
    }
    if back {
        next_state.set(AppState::MainMenu);
    }
}

/// Build the [`LoadedSong`] for an entry. Built-ins come from cache;
/// file songs re-read their chart (it may have changed on disk) and
/// stream audio from the resolved path.
pub fn prepare_song(entry: &SongEntry, builtins: &BuiltinSongs) -> Result<LoadedSong, String> {
    match &entry.source {
        SongSource::Builtin(index) => builtins
            .0
            .get(*index)
            .cloned()
            .ok_or_else(|| format!("built-in song index {index} not loaded")),
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
#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
fn refresh_browser(
    library: Res<SongLibrary>,
    cursor: Res<BrowserCursor>,
    selected: Res<SelectedDifficulty>,
    scores: Res<ScoreBoard>,
    mut rows: Query<(&SongRow, &mut BackgroundColor, &mut BorderColor)>,
    mut titles: Query<(&SongTitle, &mut TextColor), Without<SongArtist>>,
    mut artists: Query<(&SongArtist, &mut TextColor), Without<SongTitle>>,
    mut detail: Query<&mut Text, With<DetailText>>,
) {
    for (row, mut background, mut border) in &mut rows {
        let style = ui_kit::row_style(ui_kit::state_for(row.0 == cursor.0, false));
        background.0 = style.background;
        *border = BorderColor::all(style.accent);
    }
    for (title, mut color) in &mut titles {
        color.0 = ui_kit::row_style(ui_kit::state_for(title.0 == cursor.0, false)).label;
    }
    for (artist, mut color) in &mut artists {
        color.0 = ui_kit::row_style(ui_kit::state_for(artist.0 == cursor.0, false)).value;
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
        // Where you are in the list, and how long it is. With the
        // rows now clipped to a window, nothing else says whether
        // three songs follow or thirty.
        let line = format!(
            "{}/{}   {:.0} BPM{duration}   <{}>   {best}",
            cursor.0 + 1,
            library.entries.len(),
            entry.bpm,
            selected.0.display_name().to_uppercase()
        );
        if text.0 != line {
            text.0 = line;
        }
    }
}

/// The import hint / status line.
#[derive(Component)]
struct ImportNote;

/// A finished import replaces the [`SongLibrary`] resource — rebuild
/// the list so the new song is visible, and keep the note line
/// showing the import's progress.
fn rebuild_after_import(
    mut commands: Commands,
    font: Res<UiFont>,
    library: Res<SongLibrary>,
    mut cursor: ResMut<BrowserCursor>,
    status: Res<crate::import::ImportStatus>,
    screens: Query<Entity, With<BrowserScreen>>,
    mut notes: Query<&mut Text, With<ImportNote>>,
) {
    if library.is_changed() && !library.is_added() {
        for entity in &screens {
            commands.entity(entity).despawn();
        }
        spawn_browser_impl(&mut commands, &font, &library, &mut cursor);
        return;
    }
    if status.is_changed()
        && !status.0.is_empty()
        && let Ok(mut text) = notes.single_mut()
    {
        text.0.clone_from(&status.0);
    }
}

fn despawn_browser(mut commands: Commands, entities: Query<Entity, With<BrowserScreen>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
}

/// Keep the selected row inside the viewport.
///
/// The row height is MEASURED rather than assumed: it comes from the
/// font size and the row padding, and hard-coding it here would put
/// this system and `ui_kit` quietly out of step the first time either
/// changed.
fn follow_selection(
    cursor: Res<BrowserCursor>,
    library: Res<SongLibrary>,
    rows: Query<(&SongRow, &ComputedNode)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition, &mut Node), With<SongList>>,
) {
    let Ok((list, mut scroll, mut node)) = lists.single_mut() else {
        return;
    };
    // Every row is the same height, so any of them answers the
    // question - but a row may not have been laid out yet on the
    // first frame, and a height of zero would send the offset to
    // infinity.
    let Some(row_h) = rows
        .iter()
        .map(|(_, node)| node.size().y)
        .find(|height| *height > 0.0)
    else {
        return;
    };
    let pitch = row_h + ui_kit::ROW_GAP;
    // Snap the window to whole rows, so the bottom one is not sliced
    // through the middle of its letters.
    if let Some(height) = ui_kit::whole_rows_height(
        row_h,
        ui_kit::ROW_GAP,
        library.entries.len(),
        ui_kit::PANEL_MAX_H,
    ) {
        let wanted = bevy::ui::px(height);
        if node.max_height != wanted {
            node.max_height = wanted;
        }
    }
    let count = library.entries.len() as f32;
    // The gaps sit BETWEEN rows, so there is one fewer of them.
    let content_h = count.mul_add(row_h, (count - 1.0).max(0.0) * ui_kit::ROW_GAP);
    let viewport_h = list.size().y - 2.0 * ui_kit::PANEL_PAD;
    let row_top = cursor.0 as f32 * pitch;
    let wanted = ui_kit::scroll_to_show(row_top, row_h, viewport_h, content_h, scroll.0.y);
    if (wanted - scroll.0.y).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}
