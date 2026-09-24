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
                (scroll, leave).chain().run_if(in_state(AppState::SongInfo)),
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
}
