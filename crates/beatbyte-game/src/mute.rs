//! Sound off — one key, one badge, and one answer per situation.
//!
//! Born in the test sessions: harness runs could only be silenced by
//! an env var set BEFORE launch (`BEATBYTE_AUTOPILOT_MUTE`), so
//! whoever watched a run had no way to (un)silence it live. `M` — or
//! clicking the corner badge — toggles all audio at any moment; in
//! the editor `M` belongs to the metronome, so only the badge
//! toggles there.
//!
//! ⚠️ The state is **per situation**, remembered in the settings.
//! Browsing plays a preview of whatever the cursor rests on, a run
//! plays the song you chose, and an autopilot run plays a song
//! nobody is listening to on purpose — one answer for all three was
//! wrong for at least one of them every time somebody sat down.
//! `M` toggles the situation you are in, and the badge says which
//! one it just changed.

use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;

use crate::config::Settings;
use crate::palette;
use crate::states::AppState;
use crate::ui::UiFont;

/// Which of the three situations the game is making sound in.
///
/// A partition: every screen falls in exactly one, so `M` always has
/// somewhere to put its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub enum Situation {
    /// The menus and the song browser — where the preview plays.
    Browsing,
    /// A song, played by a person.
    Playing,
    /// A song, played by the autopilot: a test run.
    Autopilot,
}

impl Situation {
    /// The word the badge shows after a toggle, so the player knows
    /// what they just changed.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Situation::Browsing => "BROWSING",
            Situation::Playing => "PLAYING",
            Situation::Autopilot => "TEST RUN",
        }
    }
}

/// Which situation a state and an autopilot flag mean. Pure — tested.
///
/// ⚠️ The autopilot wins wherever it is: it drives the menus on its
/// way into a song, and a run that goes quiet only once the song
/// starts is a run whose first fifteen seconds still woke the house.
#[must_use]
pub const fn situation(state: AppState, autopilot: bool) -> Situation {
    if autopilot {
        return Situation::Autopilot;
    }
    match state {
        // Results still plays the song's tail and the fanfare, so it
        // belongs with the run it reports on rather than with the
        // menus the player has not reached yet.
        AppState::Gameplay | AppState::Results => Situation::Playing,
        _ => Situation::Browsing,
    }
}

/// Read one situation's answer out of the settings. Pure — tested.
#[must_use]
pub const fn muted_in(settings: &Settings, situation: Situation) -> bool {
    match situation {
        Situation::Browsing => settings.mute_browsing,
        Situation::Playing => settings.mute_playing,
        Situation::Autopilot => settings.mute_autopilot,
    }
}

/// `BEATBYTE_AUTOPILOT_MUTE`: this process's test run is silent.
///
/// ⚠️ A resource of its own, NOT a value poked into the settings.
/// That was the first draft, and it was a promise the code could not
/// keep: leaving the pause menu writes the settings to disk
/// (`persist_pause_settings`), so one harness run with the variable
/// set would have left mute in the player's file for ever. An env
/// var silences one run; it is not a preference.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct EnvMute(pub bool);

/// The answer in force for a situation: the settings', unless the
/// env var is overriding the test run for this process. Pure —
/// tested.
#[must_use]
pub const fn effective(settings: &Settings, situation: Situation, env: EnvMute) -> bool {
    if env.0 && matches!(situation, Situation::Autopilot) {
        return true;
    }
    muted_in(settings, situation)
}

/// Write one situation's answer back. Pure — tested.
pub const fn set_muted_in(settings: &mut Settings, situation: Situation, muted: bool) {
    match situation {
        Situation::Browsing => settings.mute_browsing = muted,
        Situation::Playing => settings.mute_playing = muted,
        Situation::Autopilot => settings.mute_autopilot = muted,
    }
}

/// The always-present corner badge showing (and toggling) the state.
#[derive(Component)]
struct MuteBadge;

/// One rectangle of the badge's speaker, in grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Left edge, in cells from the icon's left.
    pub x: u32,
    /// Top edge, in cells from the icon's top.
    pub y: u32,
    /// Width in cells.
    pub w: u32,
    /// Height in cells.
    pub h: u32,
}

/// One cell, written short.
///
/// ⚠️ A constructor rather than a struct literal because the tables
/// below are a DRAWING: `cargo fmt` expands `Cell { x: 0, … }` to
/// four lines apiece, and forty lines of field names is not a shape
/// anybody can read or edit.
#[must_use]
const fn cell(x: u32, y: u32, w: u32, h: u32) -> Cell {
    Cell { x, y, w, h }
}

/// The icon's grid: wide enough for the cone and its waves.
pub const GRID_W: u32 = 11;
/// The icon's grid height.
pub const GRID_H: u32 = 10;
/// How many pixels one cell is drawn at.
///
/// ⚠️ A WHOLE number, and that is the point. The first draft used
/// 1.5, which at a window where the UI scale is 1 puts every cell
/// boundary in the middle of a pixel: the cone's one-cell steps
/// antialias into each other and the speaker reads as three bars
/// again. It only looked right because the screenshots happened to
/// come back at twice that scale. A pixel glyph has to land ON
/// pixels at the SMALLEST scale it is drawn at.
const CELL_PX: f32 = 2.0;

/// The speaker itself: a body and a cone that steps outward.
///
/// Drawn from rectangles rather than a glyph because the two bundled
/// fonts are a pixel face and a condensed display face, and neither
/// carries a speaker — `font.safe` would have replaced it with a
/// box. Steps rather than a smooth triangle is the point: it is the
/// same pixel voice as everything else on screen.
/// ⚠️ The cone is THREE steps of one column each. The first draft
/// used two, the second of them two columns wide — and at sixteen
/// pixels that is not a cone, it is a bar: the badge read as an
/// equaliser, which is what the screenshot showed before anything
/// else was measured.
const SPEAKER: [Cell; 4] = [
    // The body: wider than it is tall, against the left edge.
    cell(0, 3, 3, 4),
    // The cone, opening to the right one column at a time.
    cell(3, 2, 1, 6),
    cell(4, 1, 1, 8),
    cell(5, 0, 1, 10),
];

/// Two arcs: a pixel face draws one as a bar whose ends curl back
/// toward the cone, which is what tells it from a plain stroke.
const WAVES: [Cell; 6] = [
    // The near arc.
    cell(7, 4, 1, 2),
    cell(6, 3, 1, 1),
    cell(6, 6, 1, 1),
    // The far one, taller and further out.
    cell(10, 3, 1, 4),
    cell(9, 1, 1, 2),
    cell(9, 7, 1, 2),
];

/// Every cell of the badge's icon for a state. Pure — tested.
///
/// The speaker is the same in both, so the eye reads the CHANGE
/// rather than re-reading the whole symbol: the arcs are there, or
/// they are not.
///
/// ⚠️ Muted draws the speaker ALONE — no bar through it, no cross
/// beside it. Both were built and photographed first, and both
/// failed for the same reason: eleven cells is not room for two
/// marks. A bar through the body MERGES with it in one colour and
/// the icon becomes a blob (print keeps them apart with a knockout,
/// which needs the colour behind the badge — whatever screen it is
/// floating over). A cross where the arcs were has five columns to
/// live in, and strokes thin enough to leave gaps are a pixel and a
/// half wide: a smudge. What is left is better than either, and it
/// is the older convention anyway — the silence is drawn by drawing
/// nothing. Two channels still change at once, which is the rule
/// `ui_kit` states for every other selected thing: the arcs go AND
/// the whole badge turns to the warm accent.
#[must_use]
pub fn glyph(muted: bool) -> Vec<Cell> {
    let mut cells = SPEAKER.to_vec();
    if !muted {
        cells.extend_from_slice(&WAVES);
    }
    cells
}

/// How long the badge names the situation after a toggle.
///
/// Long enough to read, short enough that the corner goes back to
/// being a corner — the word is an answer to "what did I just
/// change?", not a label the screen has to carry for ever.
const SAY_SITUATION_S: f32 = 2.0;

/// What the badge is currently saying, if anything.
#[derive(Resource, Default)]
struct Saying(f32);

/// The word under the badge: the situation for a moment after a
/// toggle, nothing the rest of the time. Pure — tested.
///
/// ⚠️ It exists because the feature creates a surprise: press `M` in
/// the browser, walk into a song, and the sound is back. Without a
/// word the screen looks like it forgot. With one, it answers the
/// only question a player has — what did that just change?
#[must_use]
pub fn word_for(left_s: f32, here: Situation) -> &'static str {
    if left_s > 0.0 { here.label() } else { "" }
}

/// The mute plugin: badge + key + volume application.
pub struct MutePlugin;

impl Plugin for MutePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Saying>()
            .insert_resource(EnvMute(
                std::env::var_os("BEATBYTE_AUTOPILOT_MUTE").is_some(),
            ))
            .add_systems(Startup, spawn_badge)
            .add_systems(Update, (toggle_mute, apply_mute).chain());
    }
}

fn spawn_badge(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            MuteBadge,
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: px(10),
                bottom: px(8),
                align_items: AlignItems::Center,
                column_gap: px(5),
                ..default()
            },
            GlobalZIndex(50),
        ))
        .with_children(|badge| {
            // The icon: one node per cell, laid out absolutely
            // inside a fixed box. Spawned for BOTH states and shown
            // by visibility, so a toggle never respawns anything.
            badge
                .spawn(Node {
                    width: px(f32::from(u16::try_from(GRID_W).unwrap_or(0)) * CELL_PX),
                    height: px(f32::from(u16::try_from(GRID_H).unwrap_or(0)) * CELL_PX),
                    ..default()
                })
                .with_children(|icon| {
                    for muted in [false, true] {
                        for cell in glyph(muted) {
                            icon.spawn((
                                IconCell(muted),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(cell.x as f32 * CELL_PX),
                                    top: px(cell.y as f32 * CELL_PX),
                                    width: px(cell.w as f32 * CELL_PX),
                                    height: px(cell.h as f32 * CELL_PX),
                                    ..default()
                                },
                                BackgroundColor(palette::TEXT_DIM),
                            ));
                        }
                    }
                });
            badge.spawn((
                KeyHint,
                Text::new("[M]".to_owned()),
                font.text(8.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.6)),
            ));
            badge.spawn((
                SituationWord,
                Text::new(String::new()),
                font.text(8.0),
                TextColor(palette::dimmed(palette::TEXT_DIM, 0.45)),
            ));
        });
}

/// The `[M]` beside the icon: the only place the shortcut is named.
#[derive(Component)]
struct KeyHint;

/// One drawn cell, tagged with the state it belongs to.
#[derive(Component)]
struct IconCell(bool);

/// The word that names the situation after a toggle.
#[derive(Component)]
struct SituationWord;

/// `M` (outside the editor — its metronome owns the key) or a badge
/// click flips the state OF THE SITUATION THE GAME IS IN.
#[allow(clippy::needless_pass_by_value)] // Bevy system params
fn toggle_mute(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    badges: Query<&Interaction, (With<MuteBadge>, Changed<Interaction>)>,
    mut settings: ResMut<Settings>,
    mut env: ResMut<EnvMute>,
    mut saying: ResMut<Saying>,
) {
    let key = keys.just_pressed(KeyCode::KeyM) && *state.get() != AppState::Editor;
    let clicked = badges.iter().any(|i| *i == Interaction::Pressed);
    if !key && !clicked {
        return;
    }
    let here = situation(
        *state.get(),
        autopilot.is_some_and(|autopilot| autopilot.enabled),
    );
    // Flip what is actually in force, which may be the env var's
    // answer rather than the file's — otherwise `M` during a
    // silenced harness run would write "muted" and change nothing.
    let now = !effective(&settings, here, *env);
    set_muted_in(&mut settings, here, now);
    // The player has spoken; the variable steps aside.
    env.0 = false;
    // Saved here, not on exit: this screen has no "apply" and the
    // player expects the answer to stick the way calibration's does.
    crate::config::save_settings(&settings);
    saying.0 = SAY_SITUATION_S;
}

/// Whether the state still has to be pushed to the audio side.
/// `None` = nothing pushed yet. Pure — tested.
///
/// Deliberately NOT `Res::is_changed`: that fires once, on the frame
/// the state flips, and anything that misses that frame (a resource
/// inserted later, a system that did not run yet) would leave the
/// audio side out of step until the next keypress.
///
/// ⚠️ The SITUATION is part of what was pushed. Walking from the
/// browser into a song can change the answer without anyone touching
/// a key, and a check that watched only the flag would carry the
/// browser's silence into the run.
#[must_use]
pub fn needs_push(pushed: Option<(Situation, bool)>, now: (Situation, bool)) -> bool {
    pushed != Some(now)
}

/// Push the state to both audio paths and the badge, once per change.
///
/// The music side is a GATE in the player (`MusicHandle::set_muted`),
/// not a volume — the volume belongs to the settings and to whoever
/// starts a song, and folding mute into it made every new call site a
/// chance to lose the silence.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system
fn apply_mute(
    time: Res<Time>,
    settings: Res<Settings>,
    state: Res<State<AppState>>,
    autopilot: Option<Res<crate::autopilot::Autopilot>>,
    env: Res<EnvMute>,
    music: Res<crate::audio_sys::Music>,
    mut saying: ResMut<Saying>,
    mut pushed: Local<Option<(Situation, bool)>>,
    mut global: ResMut<GlobalVolume>,
    mut cells: Query<(&IconCell, &mut Node, &mut BackgroundColor)>,
    mut word: Query<(&mut Text, &mut TextColor), With<SituationWord>>,
    mut hint: Query<&mut TextColor, (With<KeyHint>, Without<SituationWord>)>,
) {
    let here = situation(
        *state.get(),
        autopilot.is_some_and(|autopilot| autopilot.enabled),
    );
    let muted = effective(&settings, here, *env);

    // The word fades on its own clock, whatever the audio is doing.
    if saying.0 > 0.0 {
        saying.0 = (saying.0 - time.delta_secs()).max(0.0);
        if let Ok((mut text, _)) = word.single_mut() {
            let wanted = word_for(saying.0, here);
            if text.0 != wanted {
                text.0 = wanted.to_owned();
            }
        }
    }

    if !needs_push(*pushed, (here, muted)) {
        return;
    }
    // Said out loud, because mute is the one setting whose effect
    // cannot be seen — a harness run has no other way to show that
    // the silence landed where it was meant to.
    info!(
        "sound: {} in {}",
        if muted { "off" } else { "on" },
        here.label()
    );
    *pushed = Some((here, muted));
    music.0.set_muted(muted);
    global.volume = Volume::Linear(if muted { 0.0 } else { 1.0 });

    let lit = if muted {
        palette::HYPE
    } else {
        palette::dimmed(palette::TEXT_DIM, 0.75)
    };
    for (cell, mut node, mut colour) in &mut cells {
        // Both glyphs are spawned; only one is shown. Display rather
        // than `Visibility`, so the hidden one takes no layout —
        // they sit in the same absolutely-positioned box.
        node.display = if cell.0 == muted {
            Display::Flex
        } else {
            Display::None
        };
        colour.0 = lit;
    }
    if let Ok((_, mut colour)) = word.single_mut() {
        colour.0 = palette::dimmed(lit, 0.7);
    }
    // ⚠️ The key hint wears the icon's colour. Left grey while the
    // speaker went amber, the badge read as two unrelated things
    // sitting next to one another.
    if let Ok(mut colour) = hint.single_mut() {
        colour.0 = palette::dimmed(lit, 0.8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_is_pushed_once_per_change_and_always_at_least_once() {
        let browsing = Situation::Browsing;
        // Nothing pushed yet: push, whatever the state — the env var
        // can start a run muted, and that must reach the audio side
        // without anyone touching the key.
        assert!(needs_push(None, (browsing, false)));
        assert!(needs_push(None, (browsing, true)));
        // Steady state is quiet.
        assert!(!needs_push(Some((browsing, true)), (browsing, true)));
        assert!(!needs_push(Some((browsing, false)), (browsing, false)));
        // A flip in either direction is pushed.
        assert!(needs_push(Some((browsing, false)), (browsing, true)));
        assert!(needs_push(Some((browsing, true)), (browsing, false)));
        // ⚠️ And so is a change of SITUATION at the same flag: the
        // browser's silence must not walk into the song.
        assert!(needs_push(
            Some((browsing, true)),
            (Situation::Playing, true)
        ));
    }

    /// Every screen falls in exactly one situation, and the
    /// autopilot wins wherever it is.
    #[test]
    fn every_screen_belongs_to_exactly_one_situation() {
        assert_eq!(situation(AppState::SongSelect, false), Situation::Browsing);
        assert_eq!(situation(AppState::MainMenu, false), Situation::Browsing);
        assert_eq!(situation(AppState::Settings, false), Situation::Browsing);
        assert_eq!(situation(AppState::Gameplay, false), Situation::Playing);
        // The results screen still plays the song's tail.
        assert_eq!(situation(AppState::Results, false), Situation::Playing);
        // ⚠️ The autopilot drives the menus on its way into a song;
        // going quiet only at the song would still have woken the
        // house for the first fifteen seconds.
        for state in [
            AppState::MainMenu,
            AppState::SongSelect,
            AppState::Gameplay,
            AppState::Results,
        ] {
            assert_eq!(situation(state, true), Situation::Autopilot, "{state:?}");
        }
    }

    /// ⚠️ The whole point: one answer per situation, and a toggle
    /// touches only the one the game is in.
    #[test]
    fn a_toggle_changes_one_situation_and_leaves_the_others_alone() {
        let mut settings = Settings::default();
        assert!(!muted_in(&settings, Situation::Browsing));
        assert!(!muted_in(&settings, Situation::Playing));
        assert!(!muted_in(&settings, Situation::Autopilot));

        set_muted_in(&mut settings, Situation::Browsing, true);
        assert!(muted_in(&settings, Situation::Browsing));
        assert!(
            !muted_in(&settings, Situation::Playing),
            "muting the browser silenced the song"
        );
        assert!(
            !muted_in(&settings, Situation::Autopilot),
            "muting the browser silenced the test run"
        );

        set_muted_in(&mut settings, Situation::Autopilot, true);
        set_muted_in(&mut settings, Situation::Browsing, false);
        assert!(!muted_in(&settings, Situation::Browsing));
        assert!(muted_in(&settings, Situation::Autopilot));
    }

    /// ⚠️ An env var silences ONE harness run and must never reach
    /// the file: leaving the pause menu saves the settings, so a
    /// value poked in there would outlive the run that asked for it.
    #[test]
    fn the_env_var_silences_the_test_run_without_touching_the_file() {
        let settings = Settings::default();
        let env = EnvMute(true);
        assert!(effective(&settings, Situation::Autopilot, env));
        // And only the test run: the other two are the file's.
        assert!(!effective(&settings, Situation::Browsing, env));
        assert!(!effective(&settings, Situation::Playing, env));
        // The file itself is untouched, which is the whole point.
        assert!(!settings.mute_autopilot);
        // Without it, the file decides.
        let settings = Settings {
            mute_autopilot: true,
            ..Settings::default()
        };
        assert!(effective(&settings, Situation::Autopilot, EnvMute(false)));
    }

    /// A file written before this existed keeps the sound it had.
    #[test]
    fn a_settings_file_from_before_this_keeps_its_sound() {
        let old = r#"{"music_volume":0.8,"sfx_volume":0.3}"#;
        let settings: Settings = serde_json::from_str(old).expect("an older file loads");
        assert!(!settings.mute_browsing);
        assert!(!settings.mute_playing);
        assert!(!settings.mute_autopilot);
    }

    /// The icon reads as a speaker whose OUTPUT changed, not as two
    /// unrelated symbols.
    #[test]
    fn both_glyphs_share_the_speaker_and_differ_only_after_it() {
        let loud = glyph(false);
        let quiet = glyph(true);
        assert_ne!(loud, quiet, "the two states draw the same thing");
        for cell in SPEAKER {
            assert!(loud.contains(&cell), "the speaker is missing when loud");
            assert!(quiet.contains(&cell), "the speaker is missing when muted");
        }
        // What comes out of the cone is what differs.
        for cell in WAVES {
            assert!(loud.contains(&cell));
            assert!(!quiet.contains(&cell), "a wave survived the mute");
        }
        // ⚠️ Muted is the speaker and NOTHING else. A second mark —
        // a bar through it, a cross beside it — was tried twice and
        // photographed twice: at this size, in one colour, both came
        // out as a blob.
        assert_eq!(
            quiet,
            SPEAKER.to_vec(),
            "something was drawn beside the muted speaker"
        );
    }

    /// ⚠️ A cell must cover whole pixels at the smallest scale the
    /// UI is drawn at, or the one-cell steps of the cone blur into
    /// one another and the speaker becomes a row of bars. Found by
    /// photographing the badge at a window where the UI scale is 1;
    /// every earlier shot happened to be taken at 2.
    #[test]
    fn a_cell_covers_whole_pixels() {
        assert!(
            (CELL_PX - CELL_PX.round()).abs() < f32::EPSILON,
            "a cell of {CELL_PX} px straddles pixel boundaries"
        );
        const { assert!(CELL_PX >= 1.0, "a cell smaller than a pixel draws nothing") };
    }

    /// Nothing may be drawn outside the box the icon reserves, or it
    /// would sit on the text beside it.
    #[test]
    fn every_cell_stays_inside_the_icon() {
        for muted in [false, true] {
            for cell in glyph(muted) {
                assert!(cell.w > 0 && cell.h > 0, "an empty cell draws nothing");
                assert!(
                    cell.x + cell.w <= GRID_W,
                    "{cell:?} runs past the icon's right edge"
                );
                assert!(
                    cell.y + cell.h <= GRID_H,
                    "{cell:?} runs past the icon's bottom"
                );
            }
        }
    }

    /// The word shows for a moment and then gets out of the way.
    #[test]
    fn the_badge_names_the_situation_only_just_after_a_toggle() {
        assert_eq!(word_for(SAY_SITUATION_S, Situation::Browsing), "BROWSING");
        assert_eq!(word_for(0.01, Situation::Autopilot), "TEST RUN");
        assert_eq!(word_for(0.0, Situation::Playing), "", "the word overstayed");
        assert_eq!(word_for(-1.0, Situation::Playing), "");
        const {
            assert!(
                SAY_SITUATION_S > 0.5,
                "a word nobody can read is not an answer"
            )
        };
    }

    /// The badge is the only discoverable path to the shortcut, and
    /// each situation says its own name after a toggle.
    #[test]
    fn each_situation_names_itself() {
        let mut seen = std::collections::HashSet::new();
        for s in [
            Situation::Browsing,
            Situation::Playing,
            Situation::Autopilot,
        ] {
            assert!(!s.label().is_empty());
            assert!(seen.insert(s.label()), "two situations share a name");
        }
    }
}
