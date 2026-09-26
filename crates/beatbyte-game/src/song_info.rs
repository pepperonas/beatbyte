//! Everything a song's document says, on one screen.
//!
//! Why it is Deep House, when it arrived, which analyser said what,
//! and what is simply not known. The substance is
//! [`beatbyte_library::report`] — a pure function over the document,
//! tested without a window. This file turns its sections into nodes
//! and scrolls them, and does nothing else: a screen that decided
//! what to show would be a second place for the rules to live.

use bevy::input::gamepad::Gamepad;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::controls::MenuNav;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// How far one press of a direction scrolls, in pixels.
const SCROLL_STEP: f32 = 48.0;

/// Width of the provenance column, pixels. Wide enough for "from a
/// catalogue · 0.92", which is the longest note this can produce.
const NOTE_COLUMN: f32 = 230.0;

/// The document this screen is showing, and where it came from.
#[derive(Resource)]
pub struct Showing {
    /// The song's title, for the header.
    pub title: String,
    /// The sections to draw.
    pub sections: Vec<beatbyte_library::report::Section>,
    /// The folder it was read from — to read it again when the
    /// librarian or a chore rewrites the document while it is shown.
    pub folder: std::path::PathBuf,
    /// The document's modification time when it was read.
    pub modified: Option<std::time::SystemTime>,
}

/// When a document was last written, if it can be told.
fn modified(folder: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(beatbyte_library::store::path(folder))
        .and_then(|meta| meta.modified())
        .ok()
}

impl Showing {
    /// Read a folder's document and prepare it for the screen.
    ///
    /// `None` when the folder has no document yet — a song imported
    /// by an older build, or one whose folder was made by hand.
    #[must_use]
    pub fn read(folder: &std::path::Path) -> Option<Showing> {
        let doc = beatbyte_library::store::read(folder)?;
        Some(Showing {
            title: beatbyte_chart::twin::display_title(&doc.identity.title.value),
            sections: beatbyte_library::report::describe(&doc),
            folder: folder.to_path_buf(),
            modified: modified(folder),
        })
    }
}

/// Marks this screen's root, so leaving it takes everything with it.
#[derive(Component)]
struct SongInfoScreen;

/// Marks the scrolling body.
#[derive(Component)]
struct Body;

/// Registers the screen.
pub struct SongInfoPlugin;

impl Plugin for SongInfoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::SongInfo), spawn)
            .add_systems(
                Update,
                (restore_scroll, reread, scroll, leave)
                    .chain()
                    .run_if(in_state(AppState::SongInfo)),
            )
            .add_systems(OnExit(AppState::SongInfo), despawn);
    }
}

fn spawn(mut commands: Commands, font: Res<UiFont>, showing: Option<Res<Showing>>) {
    let Some(showing) = showing else {
        return;
    };
    commands
        .spawn((ui_kit::screen_root(), SongInfoScreen))
        .with_children(|parent| {
            ui_kit::header(parent, &font, "SONG", &showing.title);
            parent
                .spawn((ui_kit::scroll_panel(ui_kit::PANEL_WIDE), Body))
                .with_children(|panel| {
                    for section in &showing.sections {
                        panel.spawn((
                            Text::new(section.title.to_owned()),
                            font.text(ui_kit::ROW),
                            TextColor(palette::BRAND),
                            Node {
                                margin: UiRect::top(px(10)).with_bottom(px(2)),
                                ..default()
                            },
                        ));
                        for row in &section.rows {
                            panel.spawn(ui_kit::row()).with_children(|line| {
                                line.spawn((
                                    Text::new(row.label.clone()),
                                    font.text(ui_kit::SMALL),
                                    TextColor(palette::dimmed(palette::TEXT_DIM, 0.85)),
                                    ui_kit::label_node(),
                                ));
                                // ⚠️ Three columns, always three. The
                                // kit's row spreads its children apart,
                                // so a row WITH a provenance note put
                                // its value in the middle while one
                                // without put it at the right edge —
                                // the same column reading as two
                                // different columns down the page. The
                                // value takes the slack, and the note's
                                // column is there even when the note is
                                // not.
                                line.spawn((
                                    Text::new(row.value.clone()),
                                    font.text(ui_kit::SMALL),
                                    TextColor(palette::TEXT),
                                    Node {
                                        flex_grow: 1.0,
                                        flex_basis: px(0),
                                        ..default()
                                    },
                                    TextLayout::justify(Justify::Right),
                                ));
                                line.spawn((
                                    Text::new(row.note.clone().unwrap_or_default()),
                                    font.text(ui_kit::SMALL),
                                    TextColor(ui_kit::dimmed_subtitle()),
                                    Node {
                                        width: px(NOTE_COLUMN),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    TextLayout::justify(Justify::Right),
                                ));
                            });
                        }
                    }
                });
            ui_kit::back_button(parent, &font, "SONG SELECT");
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN scroll  ESC back",
                "D-PAD scroll  EAST back",
            );
        });
}

/// How often the shown document is checked for a newer write.
const REREAD_EVERY_S: f32 = 1.0;

/// Read the document again when it was rewritten while shown — the
/// librarian fills in a fingerprint or features, a redesign chore
/// finishes — and redraw, keeping the scroll position. A check is one
/// `stat` a second; nothing is parsed unless the file changed.
fn reread(
    mut commands: Commands,
    time: Res<Time>,
    mut since: Local<f32>,
    showing: Option<ResMut<Showing>>,
    screens: Query<Entity, With<SongInfoScreen>>,
    bodies: Query<&ScrollPosition, With<Body>>,
) {
    *since += time.delta_secs();
    if *since < REREAD_EVERY_S {
        return;
    }
    *since = 0.0;
    let Some(mut showing) = showing else {
        return;
    };
    let now = modified(&showing.folder);
    if now.is_none() || now == showing.modified {
        return;
    }
    let Some(fresh) = Showing::read(&showing.folder) else {
        return;
    };
    let scrolled = bodies.iter().next().map_or(0.0, |position| position.y);
    *showing = fresh;
    for screen in &screens {
        commands.entity(screen).despawn();
    }
    commands.run_system_cached(spawn);
    commands.insert_resource(RestoreScroll(scrolled));
}

/// A scroll position to put back once the redrawn body exists.
#[derive(Resource)]
struct RestoreScroll(f32);

fn restore_scroll(
    mut commands: Commands,
    restore: Option<Res<RestoreScroll>>,
    mut bodies: Query<&mut ScrollPosition, With<Body>>,
) {
    let Some(restore) = restore else {
        return;
    };
    if let Some(mut position) = bodies.iter_mut().next() {
        position.y = restore.0;
        commands.remove_resource::<RestoreScroll>();
    }
}

fn despawn(mut commands: Commands, screens: Query<Entity, With<SongInfoScreen>>) {
    for screen in screens.iter() {
        commands.entity(screen).despawn();
    }
}

fn scroll(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    map: Res<crate::controls::InputMap>,
    mut wheel: MessageReader<MouseWheel>,
    mut bodies: Query<&mut ScrollPosition, With<Body>>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    let mut delta = 0.0;
    if nav.down {
        delta += SCROLL_STEP;
    }
    if nav.up {
        delta -= SCROLL_STEP;
    }
    for event in wheel.read() {
        delta -= event.y * SCROLL_STEP * 0.5;
    }
    if delta == 0.0 {
        return;
    }
    for mut position in bodies.iter_mut() {
        position.y = (position.y + delta).max(0.0);
    }
}

fn leave(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    map: Res<crate::controls::InputMap>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut back: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ui_kit::BackButton>,
    >,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let nav = MenuNav::read(&map, &keys, pads.iter());
    if ui_kit::wants_leave(
        nav.back,
        ui_kit::back_pressed(&mut back),
        mouse.just_pressed(MouseButton::Right),
    ) {
        sounds.write(crate::sfx::UiSound::Back);
        next_state.set(AppState::SongSelect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_without_a_document_shows_nothing_rather_than_an_empty_screen() {
        // A song imported by an older build, or a folder made by
        // hand. The browser must not open a blank panel for it.
        let dir = std::env::temp_dir().join(format!("bb-info-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        assert!(Showing::read(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_study_twins_header_reads_as_the_song_it_practises() {
        let dir = std::env::temp_dir().join(format!("bb-info-gs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        let doc = beatbyte_library::doc::SongDoc::new(
            beatbyte_library::SongId::from_parts(1, 1),
            beatbyte_library::Sourced::stated(
                beatbyte_chart::study::twin_title("Maria"),
                beatbyte_library::MetaSource::Inferred,
            ),
            "maria.m4a".to_owned(),
            beatbyte_library::SourceKind::LocalFile,
            1_000,
        );
        beatbyte_library::store::save(&dir, &doc).expect("writes");
        let showing = Showing::read(&dir).expect("a document");
        assert_eq!(showing.title, "[GS] Maria");
        assert!(
            showing
                .sections
                .iter()
                .any(|section| section.title == "Song"),
            "the report reached the screen"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn doc(title: &str) -> beatbyte_library::doc::SongDoc {
        beatbyte_library::doc::SongDoc::new(
            beatbyte_library::SongId::from_parts(1, 1),
            beatbyte_library::Sourced::stated(
                title.to_owned(),
                beatbyte_library::MetaSource::Inferred,
            ),
            "song.m4a".to_owned(),
            beatbyte_library::SourceKind::LocalFile,
            1_000,
        )
    }

    /// The document rewritten while the screen shows it — the
    /// librarian, a finished chore — is read again and redrawn; an
    /// unchanged one is left alone.
    #[test]
    fn a_document_rewritten_while_shown_is_shown_anew() {
        let dir = std::env::temp_dir().join(format!("bb-info-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        beatbyte_library::store::save(&dir, &doc("Maria")).expect("writes");
        let mut app = App::new();
        app.add_plugins((bevy::MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Font>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_millis(600),
            ))
            .insert_resource(Showing::read(&dir).expect("a document"))
            .add_systems(Startup, spawn)
            .add_systems(Update, (restore_scroll, reread).chain());
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .reserve_handle();
        app.insert_resource(UiFont::from_handle(font));
        let headers = |app: &mut App| -> Vec<String> {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .map(|text| text.0.clone())
                .filter(|text| text.contains("Maria") || text.contains("Heroes"))
                .collect()
        };
        for _ in 0..4 {
            app.update();
        }
        // The title stands twice: the header and the document's own row.
        let before = headers(&mut app);
        assert!(
            !before.is_empty() && before.iter().all(|t| t == "Maria"),
            "{before:?}"
        );
        // Rewritten, and dated a minute later so the stamp surely moves.
        beatbyte_library::store::save(&dir, &doc("Heroes")).expect("writes");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(beatbyte_library::store::path(&dir))
            .and_then(|file| file.set_modified(later))
            .expect("re-dated");
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(app.world().resource::<Showing>().title, "Heroes");
        let after = headers(&mut app);
        assert_eq!(
            after.len(),
            before.len(),
            "one screen, redrawn, not two: {after:?}"
        );
        assert!(
            after.iter().all(|t| t == "Heroes"),
            "the old screen is gone: {after:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
