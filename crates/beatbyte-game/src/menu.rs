//! The main menu: play, settings, calibration, quit.

use bevy::prelude::*;

use crate::menu_list::list::{self, ListInput, ListPaint};
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;
use crate::ui_kit;

/// The four menu actions, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// Open the song browser (solo).
    Play,
    /// Open the multiplayer join screen.
    Multiplayer,
    /// Open the roster.
    Players,
    /// Open one player's statistics.
    ///
    /// ⚠️ Eight views of a career were reachable only by going to
    /// PLAYERS, choosing a person and pressing `S` — an unlabelled
    /// key on a screen about names. The way through PLAYERS stays;
    /// this is a second door, not a diversion.
    Statistics,
    /// Open the achievements overview.
    Achievements,
    /// Open settings.
    Settings,
    /// Credits, license, links and the changelog.
    About,
    /// Quit the game.
    Quit,
}

impl MenuAction {
    /// The entry is what a player came to do: play, with whom, who
    /// they are, how they are getting on, what they have earned, how
    /// it is set up, what this is, and out.
    ///
    /// ⚠️ CALIBRATION and INPUT TEST used to sit here. Both are
    /// set-up tools you visit once, and having them in the entry put
    /// nine rows in front of a player who wanted one. They live
    /// under SETTINGS now.
    const ALL: [MenuAction; 8] = [
        MenuAction::Play,
        MenuAction::Multiplayer,
        MenuAction::Players,
        MenuAction::Statistics,
        MenuAction::Achievements,
        MenuAction::Settings,
        MenuAction::About,
        MenuAction::Quit,
    ];

    const fn label(self) -> &'static str {
        match self {
            MenuAction::Play => "PLAY",
            MenuAction::Multiplayer => "MULTIPLAYER",
            MenuAction::Players => "PLAYERS",
            MenuAction::Statistics => "STATISTICS",
            MenuAction::Achievements => "ACHIEVEMENTS",
            MenuAction::Settings => "SETTINGS",
            MenuAction::About => "ABOUT",
            MenuAction::Quit => "QUIT",
        }
    }
}

/// The currently highlighted menu row.
#[derive(Resource, Default)]
pub(crate) struct MenuCursor(usize);

/// Plugin for the main menu.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuCursor>()
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(
                Update,
                (menu_input, highlight_cursor, pulse_title).run_if(in_state(AppState::MainMenu)),
            );
    }
}

/// Marker for the pulsing title.
#[derive(Component)]
struct MenuTitle;

/// The marker of the main menu's rows (the shared list renderer).
pub(crate) struct MenuRows;

fn spawn_menu(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((DespawnOnExit(AppState::MainMenu), ui_kit::screen_root()))
        .with_children(|parent| {
            // The title keeps its outsized treatment — it is the
            // game's wordmark, not a screen heading.
            parent.spawn((
                MenuTitle,
                Text::new("BEATBYTE"),
                font.text(ui_kit::WORDMARK),
                TextColor(palette::BRAND),
            ));
            parent.spawn((
                Text::new("five lanes. your music."),
                font.text(ui_kit::SMALL),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
                Node {
                    margin: UiRect::top(px(10)).with_bottom(px(ui_kit::HEADER_GAP)),
                    ..default()
                },
            ));
            parent.spawn(ui_kit::panel()).with_children(|panel| {
                list::spawn_label_rows::<MenuRows>(
                    panel,
                    &font,
                    MenuAction::ALL.iter().map(|action| action.label()),
                );
            });
            crate::prompts::device_footer(
                parent,
                &font,
                "UP/DOWN choose  ENTER confirm  ESC quit  MOUSE works too",
                "D-PAD choose  SOUTH confirm",
            );
        });
}

#[allow(clippy::too_many_arguments)] // Bevy system: params are DI, not an API
pub(crate) fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut list: ListInput<MenuRows>,
    mut cursor: ResMut<MenuCursor>,
    mut roster: ResMut<crate::multiplayer::PlayerRoster>,
    players: Res<crate::players::Players>,
    mut chosen: ResMut<crate::stats_ui::StatsFor>,
    mut next_state: ResMut<NextState<AppState>>,
    mut quit: MessageWriter<crate::crt::QuitRequested>,
    mut sounds: MessageWriter<crate::sfx::UiSound>,
) {
    let events = list.read(&mut cursor.0, MenuAction::ALL.len());
    if let Some(sound) = list::sound_for(None, events.moved) {
        sounds.write(sound);
    }
    // Escape closes the game from here, since there is no screen
    // above this one to go back to.
    //
    // Deliberately the KEY and not `nav.back`: that also fires on the
    // pad's East button, which the default map gives to fret 1. With a
    // guitar plugged in, noodling on the red fret at the menu would
    // close the application. A test pins that pairing so this cannot
    // be "simplified" to `nav.back` later.
    if keys.just_pressed(KeyCode::Escape) {
        quit.write(crate::crt::QuitRequested);
        return;
    }
    if events.nav.confirm || events.clicked {
        sounds.write(crate::sfx::UiSound::Confirm);
        match MenuAction::ALL[cursor.0] {
            MenuAction::Play => {
                // Solo: one keyboard player.
                *roster = crate::multiplayer::PlayerRoster::default();
                next_state.set(AppState::SongSelect);
            }
            MenuAction::Multiplayer => next_state.set(AppState::MultiplayerSetup),
            MenuAction::Players => next_state.set(AppState::Players),
            // With a player chosen the statistics are about them;
            // without one there is nothing to show, so the roster is
            // the honest answer rather than an empty screen.
            MenuAction::Statistics => {
                if players.0.selected.is_some() {
                    chosen.0 = players.0.selected;
                    next_state.set(AppState::Stats);
                } else {
                    next_state.set(AppState::Players);
                }
            }
            MenuAction::Achievements => next_state.set(AppState::Achievements),
            MenuAction::Settings => next_state.set(AppState::Settings),
            MenuAction::About => next_state.set(AppState::About),
            MenuAction::Quit => {
                quit.write(crate::crt::QuitRequested);
            }
        }
    }
}

/// Paint the highlighted row: accent bar, fill and label together.
fn highlight_cursor(
    settings: Res<crate::config::Settings>,
    cursor: Res<MenuCursor>,
    mut paint: ListPaint<MenuRows>,
) {
    // Label-only rows: no value is asked for.
    paint.paint(cursor.0, settings.high_contrast, |_| String::new());
}

/// The title breathes gently — a static menu reads as a frozen app.
fn pulse_title(time: Res<Time>, mut title: Query<&mut TextColor, With<MenuTitle>>) {
    if let Ok(mut color) = title.single_mut() {
        let pulse = 0.88 + 0.12 * (time.elapsed_secs() * 2.1).sin();
        let base = palette::BRAND.to_linear();
        color.0 = Color::LinearRgba(bevy::color::LinearRgba {
            red: base.red * pulse,
            green: base.green * pulse,
            blue: base.blue * pulse,
            alpha: 1.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::controls::{Binding, GameAction, InputMap};
    use bevy::input::gamepad::GamepadButton;

    #[test]
    fn the_pads_back_button_is_a_fret_so_it_must_not_quit() {
        // `MenuNav::back` is Escape OR the pad's East button, and the
        // default map gives East to fret 1. Wiring the menu's quit to
        // `nav.back` would close the game when a guitarist rests a
        // finger on the red fret at the menu.
        //
        // If this ever stops being true - East freed from the frets -
        // this test should be deleted along with the workaround in
        // `menu_input`, not silenced.
        let map = InputMap::default();
        let fret_one = map
            .bindings
            .iter()
            .find(|(action, _)| *action == GameAction::Fret(1))
            .map(|(_, bindings)| bindings.clone())
            .expect("fret 1 is bound");
        assert!(
            fret_one.contains(&Binding::Pad(GamepadButton::East)),
            "fret 1 no longer uses East: revisit the menu quit key"
        );
    }
}
